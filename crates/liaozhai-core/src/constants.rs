//! Shared constants: version, banner, network defaults.

/// Project version, sourced from `Cargo.toml` at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Connection banner shown to clients on accept.
///
/// Uses `concat!` + `env!` so the version is embedded at compile time
/// with no runtime allocation.
pub const BANNER: &str = concat!(
    "\r\n  Liaozhai MUX \u{804a}\u{9f4b} \u{2014} v",
    env!("CARGO_PKG_VERSION"),
    "\r\n  Multi-User eXegesis\r\n\r\n",
);

/// Default TCP port.
pub const DEFAULT_PORT: u16 = 4444;

/// Default bind address.
pub const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1";

/// Default maximum concurrent connections.
pub const DEFAULT_MAX_CONNECTIONS: usize = 100;

/// Default logging filter.
pub const DEFAULT_LOG_FILTER: &str = "info";

/// Maximum length of a single input line, in bytes (post-IAC-stripping).
pub const MAX_LINE_LENGTH: usize = 4096;

/// Maximum per-connection buffer size before forced disconnect, in bytes.
pub const MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024; // 10 MB

/// Message sent to client when a line exceeds `MAX_LINE_LENGTH`.
pub const LINE_TOO_LONG_MSG: &str = "Line too long; ignored.\r\n";

/// Message sent to client when buffer exceeds `MAX_BUFFER_SIZE`.
pub const BUFFER_OVERFLOW_MSG: &str = "Buffer overflow; disconnecting.\r\n";

/// Goodbye message sent on session-terminating command.
pub const GOODBYE_MSG: &str = "Until the next strange tale.\r\n";

/// Prompt shown when requesting the client's username.
pub const USERNAME_PROMPT: &str = "Username: ";

/// Prompt shown when requesting the client's password.
pub const PASSWORD_PROMPT: &str = "Password: ";

/// Header line above the world list.
pub const WORLDS_HEADER: &str = "Available worlds:";

/// Error message for empty username input.
pub const EMPTY_USERNAME_MSG: &str = "Username cannot be empty.\r\n";

/// Error message for empty password input.
pub const EMPTY_PASSWORD_MSG: &str = "Password cannot be empty.\r\n";

/// Error message for non-numeric world selection input.
pub const WORLD_SELECTION_NON_NUMERIC_MSG: &str = "Please enter a number.\r\n";

/// Error message template for out-of-range world selection.
/// Replace `{n}` with the registry size at format time.
pub const WORLD_SELECTION_OUT_OF_RANGE_MSG: &str = "Please enter a number between 1 and {n}.\r\n";

/// Welcome message template. Replace `{username}` at format time.
pub const WELCOME_TEMPLATE: &str = "Welcome, {username}.\r\n\r\n";

/// Placeholder message template for world selection in v0.1.
/// Replace `{world}` with the world name at format time.
pub const WORLD_SELECTED_TEMPLATE: &str =
    "In v0.1, you would now be in {world}. Disconnecting.\r\n";

/// World selection prompt template. Replace `{n}` with the registry size.
pub const WORLD_SELECT_PROMPT_TEMPLATE: &str = "Select a world (1-{n}, or 'quit'): ";

// --- Authentication messages ---

/// Error message for failed authentication (generic; doesn't leak user-exists info).
pub const AUTH_FAILED_MSG: &str = "Authentication failed.\r\n";

/// Disconnect message after exceeding per-connection retry limit.
pub const AUTH_MAX_RETRIES_MSG: &str = "Too many failed attempts. Disconnecting.\r\n";

/// Disconnect message when per-IP rate limiter triggers.
pub const AUTH_RATE_LIMITED_MSG: &str =
    "Too many failed attempts from your IP. Try again later.\r\n";

/// Error message when `AccountStore` itself errors (rare DB failure).
pub const AUTH_INTERNAL_ERROR_MSG: &str = "Authentication error. Please try again later.\r\n";

// --- Telnet IAC ECHO negotiation ---

/// IAC WILL ECHO: server takes over echo (client should suppress local echo).
pub const IAC_WILL_ECHO: &[u8] = &[0xFF, 0xFB, 0x01];

/// IAC WONT ECHO: server stops handling echo (client should resume local echo).
pub const IAC_WONT_ECHO: &[u8] = &[0xFF, 0xFC, 0x01];

// --- Default auth parameters ---

/// Default maximum login attempts per connection.
pub const DEFAULT_MAX_LOGIN_ATTEMPTS: u32 = 3;

/// Default argon2 memory cost in KiB (19 MiB, per OWASP 2024).
pub const DEFAULT_ARGON2_MEMORY_COST: u32 = 19_456;

/// Default argon2 time cost (iterations).
pub const DEFAULT_ARGON2_TIME_COST: u32 = 2;

/// Default argon2 parallelism (lanes).
pub const DEFAULT_ARGON2_PARALLELISM: u32 = 1;

/// Default rate-limiter window duration in seconds.
pub const DEFAULT_RATE_LIMIT_WINDOW_SECS: u64 = 60;

/// Default maximum failures per IP within the rate-limit window.
pub const DEFAULT_RATE_LIMIT_MAX_FAILURES: usize = 10;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn banner_contains_version() {
        assert!(BANNER.contains(VERSION));
    }

    #[test]
    fn max_line_length_is_4096() {
        assert_eq!(MAX_LINE_LENGTH, 4096);
    }

    #[test]
    fn max_buffer_size_is_10mb() {
        assert_eq!(MAX_BUFFER_SIZE, 10 * 1024 * 1024);
    }

    #[test]
    fn username_prompt_is_non_empty() {
        assert!(!USERNAME_PROMPT.is_empty());
    }

    #[test]
    fn password_prompt_is_non_empty() {
        assert!(!PASSWORD_PROMPT.is_empty());
    }

    #[test]
    fn templates_contain_placeholders() {
        assert!(WELCOME_TEMPLATE.contains("{username}"));
        assert!(WORLD_SELECTED_TEMPLATE.contains("{world}"));
        assert!(WORLD_SELECT_PROMPT_TEMPLATE.contains("{n}"));
        assert!(WORLD_SELECTION_OUT_OF_RANGE_MSG.contains("{n}"));
    }

    #[test]
    fn iac_echo_sequences_are_3_bytes() {
        assert_eq!(IAC_WILL_ECHO.len(), 3);
        assert_eq!(IAC_WONT_ECHO.len(), 3);
    }

    #[test]
    fn auth_messages_non_empty() {
        assert!(!AUTH_FAILED_MSG.is_empty());
        assert!(!AUTH_MAX_RETRIES_MSG.is_empty());
        assert!(!AUTH_RATE_LIMITED_MSG.is_empty());
    }

    #[test]
    fn banner_contains_project_name() {
        assert!(BANNER.contains("Liaozhai MUX"));
        assert!(BANNER.contains("Multi-User eXegesis"));
    }
}
