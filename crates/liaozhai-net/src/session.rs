//! Connection session state machine.
//!
//! Drives the connection through banner → username → password → world list →
//! world selection → goodbye. Authentication runs through the connection
//! handler (which awaits `AccountStore::verify_credentials`) via the
//! [`Transition::AuthPending`] variant; the session itself stays sync.
//! The world list is populated from a `WorldRegistry`.

use std::sync::Arc;

use liaozhai_auth::account::Account;
use liaozhai_core::constants;
use liaozhai_worlds::metadata::WorldMetadata;
use liaozhai_worlds::registry::WorldRegistry;

/// The current state of a client session.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SessionState {
    /// Collecting credentials.
    /// `username: None` = waiting for username input.
    /// `username: Some(name)` = waiting for password input.
    Authenticating { username: Option<String> },

    /// Authenticated; showing world list and awaiting selection.
    WorldSelection { account: Account },
}

/// The result of processing one line of client input.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Transition {
    /// Stay in the current state; write the output to the client.
    Stay { output: String },

    /// Advance to a new state; write the output (which includes the
    /// new state's entry prompt) to the client.
    Advance { next: SessionState, output: String },

    /// End the session; write the goodbye message and disconnect.
    Disconnect { goodbye: String },

    /// The session needs async credential verification.
    /// The connection handler performs the async call, then calls
    /// `session.complete_auth(result)` with the outcome.
    AuthPending { username: String, password: String },
}

/// A client session, managing state transitions through the connection lifecycle.
///
/// The session is pure logic — it never performs I/O. The connection handler
/// calls [`Session::handle_input`] with each line of client input and writes
/// the resulting output to the socket.
pub(crate) struct Session {
    state: SessionState,
    registry: Arc<WorldRegistry>,
}

impl Session {
    pub(crate) fn new(registry: Arc<WorldRegistry>) -> Self {
        Self {
            state: SessionState::Authenticating { username: None },
            registry,
        }
    }

    pub(crate) const fn initial_prompt() -> &'static str {
        constants::USERNAME_PROMPT
    }

    pub(crate) fn handle_input(&self, input: &str) -> Transition {
        match &self.state {
            SessionState::Authenticating { username } => match username {
                None => Self::handle_username_input(input),
                Some(name) => Self::handle_password_input(name, input),
            },
            SessionState::WorldSelection { account } => {
                self.handle_world_selection_input(account, input)
            }
        }
    }

    pub(crate) fn apply(&mut self, next: SessionState) {
        self.state = next;
    }

    pub(crate) fn state(&self) -> &SessionState {
        &self.state
    }

    pub(crate) fn is_password_input(&self) -> bool {
        matches!(
            self.state,
            SessionState::Authenticating { username: Some(_) }
        )
    }

    /// Complete an authentication attempt after the connection handler
    /// has resolved the async credential verification.
    pub(crate) fn complete_auth(&self, auth_result: Option<Account>) -> Transition {
        match auth_result {
            Some(account) => {
                let welcome = constants::WELCOME_TEMPLATE.replace("{username}", account.username());
                let world_list = format_world_list(self.registry.worlds());
                let select_prompt = format_world_select_prompt(self.registry.len());
                let output = format!("{welcome}{world_list}\r\n{select_prompt}");

                Transition::Advance {
                    next: SessionState::WorldSelection { account },
                    output,
                }
            }
            None => Transition::Advance {
                next: SessionState::Authenticating { username: None },
                output: format!(
                    "{}{}",
                    constants::AUTH_FAILED_MSG,
                    constants::USERNAME_PROMPT,
                ),
            },
        }
    }

    fn handle_username_input(input: &str) -> Transition {
        let trimmed = input.trim();

        if is_session_terminator(trimmed) {
            return Transition::Disconnect {
                goodbye: constants::GOODBYE_MSG.to_owned(),
            };
        }

        if trimmed.is_empty() {
            return Transition::Stay {
                output: format!(
                    "{}{}",
                    constants::EMPTY_USERNAME_MSG,
                    constants::USERNAME_PROMPT
                ),
            };
        }

        Transition::Advance {
            next: SessionState::Authenticating {
                username: Some(trimmed.to_owned()),
            },
            output: constants::PASSWORD_PROMPT.to_owned(),
        }
    }

    fn handle_password_input(username: &str, input: &str) -> Transition {
        let trimmed = input.trim();

        if is_session_terminator(trimmed) {
            return Transition::Disconnect {
                goodbye: constants::GOODBYE_MSG.to_owned(),
            };
        }

        if trimmed.is_empty() {
            return Transition::Stay {
                output: format!(
                    "{}{}",
                    constants::EMPTY_PASSWORD_MSG,
                    constants::PASSWORD_PROMPT
                ),
            };
        }

        Transition::AuthPending {
            username: username.to_owned(),
            password: trimmed.to_owned(),
        }
    }

    fn handle_world_selection_input(&self, _account: &Account, input: &str) -> Transition {
        let trimmed = input.trim();

        if is_session_terminator(trimmed) {
            return Transition::Disconnect {
                goodbye: constants::GOODBYE_MSG.to_owned(),
            };
        }

        if trimmed.is_empty() {
            return Transition::Stay {
                output: format!(
                    "{}{}",
                    constants::WORLD_SELECTION_NON_NUMERIC_MSG,
                    format_world_select_prompt(self.registry.len()),
                ),
            };
        }

        match trimmed.parse::<usize>() {
            Ok(n) => match self.registry.get_by_position(n) {
                Some(world) => {
                    let goodbye =
                        constants::WORLD_SELECTED_TEMPLATE.replace("{world}", world.name());
                    Transition::Disconnect {
                        goodbye: format!("\r\n{goodbye}"),
                    }
                }
                None => Transition::Stay {
                    output: format!(
                        "{}{}",
                        constants::WORLD_SELECTION_OUT_OF_RANGE_MSG
                            .replace("{n}", &self.registry.len().to_string()),
                        format_world_select_prompt(self.registry.len()),
                    ),
                },
            },
            Err(_) => Transition::Stay {
                output: format!(
                    "{}{}",
                    constants::WORLD_SELECTION_NON_NUMERIC_MSG,
                    format_world_select_prompt(self.registry.len()),
                ),
            },
        }
    }
}

pub(crate) fn is_session_terminator(line: &str) -> bool {
    matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "quit" | "exit" | "bye" | "disconnect"
    )
}

/// Render the world list as a multi-line string for display to the client.
///
/// Names are left-padded to the longest-name + 1 column so descriptions
/// line up. Output ends with `\r\n` after the last entry. An empty
/// registry produces just the header line.
pub(crate) fn format_world_list(worlds: &[WorldMetadata]) -> String {
    use std::fmt::Write;

    let name_width = worlds.iter().map(|w| w.name().len()).max().unwrap_or(0) + 1;

    let mut output = String::new();
    output.push_str(constants::WORLDS_HEADER);
    output.push_str("\r\n");

    for (i, world) in worlds.iter().enumerate() {
        let num = i + 1;
        let name = world.name();
        let desc = world.short_description();
        let _ = write!(output, "  {num}. {name:<name_width$}{desc}\r\n");
    }

    output
}

fn format_world_select_prompt(count: usize) -> String {
    constants::WORLD_SELECT_PROMPT_TEMPLATE.replace("{n}", &count.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_registry() -> Arc<WorldRegistry> {
        Arc::new(WorldRegistry::new(vec![
            WorldMetadata::new(
                "studio-dusk",
                "The Studio at Dusk",
                "A small interior, warmly lit.",
            ),
            WorldMetadata::new(
                "mountain-trail",
                "The Mountain Trail",
                "A path winding into mist.",
            ),
            WorldMetadata::new(
                "library-echoes",
                "The Library of Echoes",
                "A reading room of recursive proportions.",
            ),
        ]))
    }

    // --- is_session_terminator ---

    #[test]
    fn session_terminator_quit() {
        assert!(is_session_terminator("quit"));
    }

    #[test]
    fn session_terminator_exit() {
        assert!(is_session_terminator("exit"));
    }

    #[test]
    fn session_terminator_bye() {
        assert!(is_session_terminator("bye"));
    }

    #[test]
    fn session_terminator_disconnect() {
        assert!(is_session_terminator("disconnect"));
    }

    #[test]
    fn session_terminator_case_insensitive() {
        assert!(is_session_terminator("QUIT"));
        assert!(is_session_terminator("Exit"));
        assert!(is_session_terminator("BYE"));
    }

    #[test]
    fn session_terminator_whitespace() {
        assert!(is_session_terminator("  quit  "));
    }

    #[test]
    fn session_terminator_rejects_other() {
        assert!(!is_session_terminator("hello"));
        assert!(!is_session_terminator(""));
        assert!(!is_session_terminator("quitting"));
    }

    // --- Username state ---

    #[test]
    fn username_accepts_non_empty() {
        let session = Session::new(test_registry());
        let t = session.handle_input("alice");
        match t {
            Transition::Advance { next, output } => {
                assert_eq!(
                    next,
                    SessionState::Authenticating {
                        username: Some("alice".into())
                    }
                );
                assert!(output.contains("Password"));
            }
            other => panic!("expected Advance, got {other:?}"),
        }
    }

    #[test]
    fn username_rejects_empty() {
        let session = Session::new(test_registry());
        let t = session.handle_input("");
        match t {
            Transition::Stay { output } => {
                assert!(output.contains("cannot be empty"));
                assert!(output.contains("Username: "));
            }
            other => panic!("expected Stay, got {other:?}"),
        }
    }

    #[test]
    fn username_rejects_whitespace_only() {
        let session = Session::new(test_registry());
        let t = session.handle_input("   ");
        match t {
            Transition::Stay { output } => {
                assert!(output.contains("cannot be empty"));
            }
            other => panic!("expected Stay, got {other:?}"),
        }
    }

    #[test]
    fn username_quit_disconnects() {
        let session = Session::new(test_registry());
        let t = session.handle_input("quit");
        assert!(matches!(t, Transition::Disconnect { .. }));
    }

    #[test]
    fn username_exit_disconnects() {
        let session = Session::new(test_registry());
        let t = session.handle_input("exit");
        assert!(matches!(t, Transition::Disconnect { .. }));
    }

    // --- Password state ---

    #[test]
    fn password_returns_auth_pending() {
        let mut session = Session::new(test_registry());
        session.apply(SessionState::Authenticating {
            username: Some("alice".into()),
        });
        let t = session.handle_input("secret");
        match t {
            Transition::AuthPending { username, password } => {
                assert_eq!(username, "alice");
                assert_eq!(password, "secret");
            }
            other => panic!("expected AuthPending, got {other:?}"),
        }
    }

    #[test]
    fn complete_auth_success_advances_to_world_selection() {
        let session = Session::new(test_registry());
        let account = Account::new("alice");
        let t = session.complete_auth(Some(account));
        match t {
            Transition::Advance { next, output } => {
                assert!(matches!(next, SessionState::WorldSelection { .. }));
                assert!(output.contains("Welcome, alice"));
                assert!(output.contains("Available worlds:"));
            }
            other => panic!("expected Advance, got {other:?}"),
        }
    }

    #[test]
    fn complete_auth_failure_returns_to_username() {
        let session = Session::new(test_registry());
        let t = session.complete_auth(None);
        match t {
            Transition::Advance { next, output } => {
                assert_eq!(next, SessionState::Authenticating { username: None });
                assert!(output.contains("Authentication failed"));
                assert!(output.contains("Username: "));
            }
            other => panic!("expected Advance, got {other:?}"),
        }
    }

    #[test]
    fn password_rejects_empty() {
        let mut session = Session::new(test_registry());
        session.apply(SessionState::Authenticating {
            username: Some("alice".into()),
        });
        let t = session.handle_input("");
        match t {
            Transition::Stay { output } => {
                assert!(output.contains("cannot be empty"));
                assert!(output.contains("Password: "));
            }
            other => panic!("expected Stay, got {other:?}"),
        }
    }

    #[test]
    fn password_quit_disconnects() {
        let mut session = Session::new(test_registry());
        session.apply(SessionState::Authenticating {
            username: Some("alice".into()),
        });
        let t = session.handle_input("quit");
        assert!(matches!(t, Transition::Disconnect { .. }));
    }

    // --- World selection state ---

    #[test]
    fn world_selection_valid_choice() {
        let mut session = Session::new(test_registry());
        session.apply(SessionState::WorldSelection {
            account: Account::new("alice"),
        });
        let t = session.handle_input("1");
        match t {
            Transition::Disconnect { goodbye } => {
                assert!(goodbye.contains("The Studio at Dusk"));
                assert!(goodbye.contains("Disconnecting"));
            }
            other => panic!("expected Disconnect, got {other:?}"),
        }
    }

    #[test]
    fn world_selection_all_valid_choices() {
        let expected = [
            "The Studio at Dusk",
            "The Mountain Trail",
            "The Library of Echoes",
        ];
        for (i, name) in expected.iter().enumerate() {
            let mut session = Session::new(test_registry());
            session.apply(SessionState::WorldSelection {
                account: Account::new("alice"),
            });
            let t = session.handle_input(&(i + 1).to_string());
            match t {
                Transition::Disconnect { goodbye } => {
                    assert!(goodbye.contains(name));
                }
                other => panic!("expected Disconnect for world {}, got {other:?}", i + 1),
            }
        }
    }

    #[test]
    fn world_selection_zero_rejected() {
        let mut session = Session::new(test_registry());
        session.apply(SessionState::WorldSelection {
            account: Account::new("alice"),
        });
        let t = session.handle_input("0");
        match t {
            Transition::Stay { output } => {
                assert!(output.contains("between 1 and 3"));
            }
            other => panic!("expected Stay, got {other:?}"),
        }
    }

    #[test]
    fn world_selection_out_of_range_rejected() {
        let mut session = Session::new(test_registry());
        session.apply(SessionState::WorldSelection {
            account: Account::new("alice"),
        });
        let t = session.handle_input("4");
        match t {
            Transition::Stay { output } => {
                assert!(output.contains("between 1 and 3"));
            }
            other => panic!("expected Stay, got {other:?}"),
        }
    }

    #[test]
    fn world_selection_non_numeric_rejected() {
        let mut session = Session::new(test_registry());
        session.apply(SessionState::WorldSelection {
            account: Account::new("alice"),
        });
        let t = session.handle_input("abc");
        match t {
            Transition::Stay { output } => {
                assert!(output.contains("Please enter a number"));
            }
            other => panic!("expected Stay, got {other:?}"),
        }
    }

    #[test]
    fn world_selection_empty_rejected() {
        let mut session = Session::new(test_registry());
        session.apply(SessionState::WorldSelection {
            account: Account::new("alice"),
        });
        let t = session.handle_input("");
        match t {
            Transition::Stay { output } => {
                assert!(output.contains("Please enter a number"));
                assert!(output.contains("Select a world"));
            }
            other => panic!("expected Stay, got {other:?}"),
        }
    }

    #[test]
    fn world_selection_quit_disconnects() {
        let mut session = Session::new(test_registry());
        session.apply(SessionState::WorldSelection {
            account: Account::new("alice"),
        });
        let t = session.handle_input("quit");
        match t {
            Transition::Disconnect { goodbye } => {
                assert!(goodbye.contains("strange tale"));
            }
            other => panic!("expected Disconnect, got {other:?}"),
        }
    }

    // --- format helpers ---

    #[test]
    fn format_world_list_matches_demo() {
        let reg = test_registry();
        let output = format_world_list(reg.worlds());
        assert!(output.starts_with("Available worlds:\r\n"));
        assert!(output.contains("  1. The Studio at Dusk"));
        assert!(output.contains("  2. The Mountain Trail"));
        assert!(output.contains("  3. The Library of Echoes"));
    }

    #[test]
    fn format_world_list_empty_registry() {
        let output = format_world_list(&[]);
        assert_eq!(output, "Available worlds:\r\n");
    }

    #[test]
    fn world_select_prompt_shows_range() {
        let prompt = format_world_select_prompt(3);
        assert_eq!(prompt, "Select a world (1-3, or 'quit'): ");
    }

    #[test]
    fn is_password_input_when_waiting_for_password() {
        let mut session = Session::new(test_registry());
        assert!(!session.is_password_input());
        session.apply(SessionState::Authenticating {
            username: Some("alice".into()),
        });
        assert!(session.is_password_input());
    }
}
