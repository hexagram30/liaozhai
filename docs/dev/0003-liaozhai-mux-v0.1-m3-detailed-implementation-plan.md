# Liaozhai MUX v0.1 — M3 Detailed Implementation Plan

## Context

M2 is complete: the telnet codec strips IAC, splits lines, handles overflow, and drives an echo loop with session terminators. M3 replaces the echo loop with a connection state machine: banner → username → password → world list → world selection → goodbye. Auth is stubbed (any non-empty input succeeds); worlds are hardcoded placeholders.

**Source documents:**
- `workbench/m3-implementation-plan.md` (design decisions)
- `docs/design/05-active/0011-v0.1-implementation-plan.md` (M3 acceptance criteria, demo format)

---

## Implementation Order

1. New constants in `liaozhai-core/src/constants.rs`
2. `WorldRegistry::placeholder()` + `get()` in `liaozhai-worlds/src/registry.rs`
3. Add `liaozhai-auth` and `liaozhai-worlds` deps to `liaozhai-net/Cargo.toml`
4. New `liaozhai-net/src/session.rs`: SessionState, Transition, Session, input handlers, format helpers, unit tests
5. Rewrite `liaozhai-net/src/connection.rs`: state-machine-driven I/O loop, updated integration tests
6. Update `liaozhai-net/src/lib.rs`: add `pub(crate) mod session;`
7. Update `liaozhai-server/src/listener.rs`: construct `Arc<WorldRegistry>`, pass to handler

---

## Step 1: New Constants

### `crates/liaozhai-core/src/constants.rs` (MODIFY) — append:
```rust
pub const USERNAME_PROMPT: &str = "Username: ";
pub const PASSWORD_PROMPT: &str = "Password: ";
pub const WORLDS_HEADER: &str = "Available worlds:";
pub const EMPTY_USERNAME_MSG: &str = "Username cannot be empty.\r\n";
pub const EMPTY_PASSWORD_MSG: &str = "Password cannot be empty.\r\n";
pub const WORLD_SELECTION_NON_NUMERIC_MSG: &str = "Please enter a number.\r\n";
pub const WORLD_SELECTION_OUT_OF_RANGE_MSG: &str = "Please enter a number between 1 and {n}.\r\n";

// Templates with placeholders, applied via `replace` or `format!` in session.rs.
pub const WELCOME_TEMPLATE: &str = "Welcome, {username}.\r\n\r\n";
pub const WORLD_SELECTED_TEMPLATE: &str = "In v0.1, you would now be in {world}. Disconnecting.\r\n";
pub const WORLD_SELECT_PROMPT_TEMPLATE: &str = "Select a world (1-{n}, or 'quit'): ";
```

Two error messages on the world-selection state because the failure modes are distinct: non-numeric input and out-of-range numeric input deserve different help text. The out-of-range message uses a `{n}` placeholder substituted at format time so the same constant works for any registry size.

Templates are kept as constants (rather than inline `format!` literals in `session.rs`) so the wording is editable in one place — matters for future internationalization or tone tweaks.

Tests: `username_prompt_is_non_empty`, `password_prompt_is_non_empty`, `templates_contain_placeholders`.

---

## Step 2: WorldRegistry::placeholder() + get()

### `crates/liaozhai-worlds/src/registry.rs` (MODIFY)

Add:
```rust
pub fn placeholder() -> Self {
    Self { worlds: vec![
        WorldMetadata::new("studio-dusk", "The Studio at Dusk", "A small interior, warmly lit."),
        WorldMetadata::new("mountain-trail", "The Mountain Trail", "A path winding into mist."),
        WorldMetadata::new("library-echoes", "The Library of Echoes", "A reading room of recursive proportions."),
    ]}
}

pub fn get_by_position(&self, one_based_index: usize) -> Option<&WorldMetadata> {
    if one_based_index == 0 { return None; }
    self.worlds.get(one_based_index - 1)
}
```

The method name is `get_by_position` rather than `get` because M5 will add sibling lookup methods (`get_by_slug`, `get_by_id`) once worlds are TOML-loaded with stable identifiers, and a bare `get` would collide. `position` is the user-facing concept (the number they typed at the prompt); the implementation detail of "one-based index" stays on the parameter name.

Tests: `placeholder_has_three_worlds`, `placeholder_world_names`, `get_by_position_valid`, `get_by_position_zero_returns_none`, `get_by_position_out_of_range_returns_none`.

---

## Step 3: Add Dependencies to liaozhai-net

### `crates/liaozhai-net/Cargo.toml` (MODIFY) — add:
```toml
liaozhai-auth.workspace = true
liaozhai-worlds.workspace = true
```

Dependency graph stays acyclic: `core ← auth, worlds ← net ← server`.

---

## Step 4: New session.rs (CREATE)

### `crates/liaozhai-net/src/session.rs`

The state machine is pure logic — no I/O. The I/O loop in connection.rs calls `session.handle_input(line)` and writes the returned output.

**Types:**
```rust
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SessionState {
    Authenticating { username: Option<String> },
    WorldSelection { account: Account },
    Disconnected,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Transition {
    Stay { output: String },
    Advance { next: SessionState, output: String },
    Disconnect { goodbye: String },
}
```

**Session struct:**
```rust
pub(crate) struct Session {
    state: SessionState,
    registry: Arc<WorldRegistry>,
}
```

The registry is owned via `Arc` (not borrowed via `&'a WorldRegistry`) because (a) the listener already holds the registry as `Arc<WorldRegistry>` and clones one per connection, (b) the Arc-owning shape avoids a generic lifetime parameter without measurable cost (one atomic increment per `Session::new`), and (c) it matches the broader async-Rust idiom for shared read-only state in long-lived handlers (Tokio tutorial, Hyper, Axum's `State<T>` extractor, Tower services all converge on this pattern).

**Session methods:**
- `new(registry: Arc<WorldRegistry>) -> Self` — starts in `Authenticating { username: None }`
- `initial_prompt() -> &'static str` — returns `USERNAME_PROMPT`
- `handle_input(&self, input: &str) -> Transition` — dispatches to state-specific handler
- `apply(&mut self, next: SessionState)` — advances the state
- `state(&self) -> &SessionState` — for logging
- `is_password_input(&self) -> bool` — true when state is `Authenticating { username: Some(_) }`; used by the I/O loop's trace-log redaction

**Input handlers (private methods):**

`handle_username_input(input)`:
- `is_session_terminator` → Disconnect
- empty/whitespace → Stay (error + re-prompt USERNAME_PROMPT)
- valid → Advance to `Authenticating { username: Some(name) }`, output = PASSWORD_PROMPT

`handle_password_input(username, input)`:
- `is_session_terminator` → Disconnect
- empty/whitespace → Stay (EMPTY_PASSWORD_MSG + re-prompt PASSWORD_PROMPT)
- valid → Advance to `WorldSelection { account }`, output = welcome + blank line + WORLDS_HEADER + blank line + formatted world list + blank line + select prompt
  - // TODO(M4): replace with real SQLite + argon2 authentication

`handle_world_selection_input(account, input)`:
- `is_session_terminator` → Disconnect
- parse number → valid index (1..=n) → Disconnect with WORLD_SELECTED_TEMPLATE filled
- parse number → out of range → Stay (WORLD_SELECTION_OUT_OF_RANGE_MSG with `{n}` substituted + re-prompt)
- non-numeric / empty → Stay (WORLD_SELECTION_NON_NUMERIC_MSG + re-prompt)

**`is_session_terminator`** — moved from connection.rs (same logic, same tests).

**`format_world_list(worlds: &[WorldMetadata]) -> String`:**
```
Available worlds:
  1. The Studio at Dusk    A small interior, warmly lit.
  2. The Mountain Trail    A path winding into mist.
  3. The Library of Echoes A reading room of recursive proportions.
```
Names left-aligned to `longest_name + 1` for column alignment.

**`format_world_select_prompt(count: usize) -> String`:**
```
Select a world (1-3, or 'quit'):
```

**Unit tests (~25 tests):**
- is_session_terminator: 7 tests (moved from connection.rs)
- Username state: accepts non-empty, rejects empty, rejects whitespace, quit disconnects, exit disconnects
- Password state: accepts non-empty (produces welcome + world list), rejects empty, quit disconnects
- World selection: valid choice (1/2/3), zero rejected, out-of-range, non-numeric, empty, quit disconnects
- format_world_list: matches demo format, empty registry
- format_world_select_prompt: shows range

---

## Step 5: Rewrite connection.rs

### `crates/liaozhai-net/src/connection.rs` (MODIFY)

**Signature changes:**
```rust
pub async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    registry: Arc<WorldRegistry>,
) -> liaozhai_core::error::Result<()>

pub async fn handle_connection_with_codec(
    stream: TcpStream,
    peer: SocketAddr,
    codec: TelnetLineCodec,
    registry: Arc<WorldRegistry>,
) -> liaozhai_core::error::Result<()>
```

**I/O loop changes:**
- Remove echo logic, remove `is_session_terminator` (moved to session.rs)
- Create `Session::new(registry.clone())` (the connection handler receives `Arc<WorldRegistry>`; cloning the Arc into the Session is a single atomic increment)
- Write `BANNER + session.initial_prompt()` as a single combined initial output (atomic — one `write_all`, matching the M2 LineWriter discipline)
- On `CodecItem::Line(line)`: call `session.handle_input(&line)`, match on Transition:
  - `Stay { output }` → write_raw
  - `Advance { next, output }` → log transition at debug, write_raw, session.apply(next)
  - `Disconnect { goodbye }` → write_raw goodbye, break
- LineTooLong / BufferOverflow / Io / None handling unchanged from M2
- **Password redaction in trace logs** — before tracing the received line, check `session.state()`. When the state is `Authenticating { username: Some(_) }`, log `line = "<redacted>"` instead of the actual content. This is M3-mandatory rather than deferred to M4: even with stubbed auth, an operator running `RUST_LOG=trace` should not see passwords in their logs. Implementation suggestion:

  ```rust
  let logged_line = if session.is_password_input() {
      "<redacted>"
  } else {
      line.as_str()
  };
  trace!(%conn_id, %peer, line_count, line = %logged_line, "received line");
  ```

  The `is_password_input` helper on `Session` (listed in the methods above) reads more cleanly than an inline `matches!` on the state enum, and centralizes the "what counts as password input" decision so it stays correct as M4 adds password-related sub-states.

**Remove:** M2 session-terminator unit tests (moved to session.rs)
**Remove:** M2 echo-specific integration tests (banner_then_echo, quit_ends_session, exit_ends_session, multiple_lines_echoed)
**Keep:** client_disconnect_without_quit, iac_bytes_stripped, line_too_long, buffer_overflow (updated for new signature)

**Add integration tests:**
- `full_v01_demo` — banner → alice → secret → world 1 → goodbye
- `quit_at_username_state` → goodbye
- `quit_at_password_state` — alice → quit → goodbye
- `quit_at_world_selection_state` — alice → secret → quit → goodbye
- `invalid_username_re_prompts` — empty → error → alice → password prompt
- `invalid_world_selection_re_prompts` — 4 (range error) → abc (invalid) → 2 → goodbye
- `exit_alias_at_world_selection` — EXIT at world selection
- `iac_bytes_stripped_during_session` — IAC bytes with username
- `line_too_long_during_session` — overflow then continue normally
- `buffer_overflow_disconnects_client` — updated for new signature
- `client_disconnect_without_quit` — updated for new signature

**Test setup helper** updated to return `(TcpListener, SocketAddr, Arc<WorldRegistry>)`.

---

## Step 6: Update lib.rs

### `crates/liaozhai-net/src/lib.rs` (MODIFY) — add:
```rust
pub(crate) mod session;
```

---

## Step 7: Update listener.rs

### `crates/liaozhai-server/src/listener.rs` (MODIFY)

- Import `std::sync::Arc` and `liaozhai_worlds::registry::WorldRegistry`
- Before accept loop: `let registry = Arc::new(WorldRegistry::placeholder());`
- Log: `info!(world_count = registry.len(), "world registry loaded");`
- In accept loop: `let reg = registry.clone();` then pass to `handle_connection(stream, peer, reg)`

---

## File Inventory

### CREATE (1 file)
| File | Purpose |
|------|---------|
| `crates/liaozhai-net/src/session.rs` | SessionState, Transition, Session, input handlers, format helpers, ~25 unit tests |

### MODIFY (6 files)
| File | Change |
|------|--------|
| `crates/liaozhai-core/src/constants.rs` | Add 6 prompt/error constants |
| `crates/liaozhai-worlds/src/registry.rs` | Add `placeholder()`, `get()`, 5 tests |
| `crates/liaozhai-net/Cargo.toml` | Add liaozhai-auth, liaozhai-worlds deps |
| `crates/liaozhai-net/src/lib.rs` | Add `pub(crate) mod session;` |
| `crates/liaozhai-net/src/connection.rs` | Rewrite I/O loop for state machine; add Arc<WorldRegistry> param; ~11 integration tests |
| `crates/liaozhai-server/src/listener.rs` | Construct Arc<WorldRegistry>, pass to handler |

---

## Test Coverage Target

Per the M3 design plan:

- **`liaozhai-net::session`**: 80–90% coverage. Pure logic; the state machine is unit-testable through synthetic inputs without needing sockets.
- **`liaozhai-net::connection`**: 65–75%. I/O-bound; integration tests cover the main flows.
- **`liaozhai-worlds::registry`**: 80%+. The `placeholder` and `get` methods are simple, fully testable.
- **Workspace overall (M3 cumulative)**: 70–80%. ADR-0011's 80%-by-M6 target stays on track and is now in striking distance.

## Risks

**Risk: state machine extensibility.** The enum-with-data approach works for three states. M4 adds auth sub-states (account-not-found, retry-counter-exhausted, rate-limited) which will push variant count up. If the enum gets larger than ~6 variants with non-trivial data, consider migrating to a trait-object pattern. M3 doesn't need to anticipate this.

**Risk: integration-test suite size.** M3 adds ~11 new integration tests on top of M1's and M2's. Each spins up a server task and a TCP loopback. If the suite slows materially, parallelism via `tokio::test(flavor = "multi_thread")` is the lever. Don't pre-emptively optimize.

**Risk: clear-text passwords feel sloppy.** Even with stubbed auth and the trace-log redaction above, anyone testing M3 manually will see their typed password echoed by the codec (because telnet line mode echoes by default). Document this clearly. M4 fixes it permanently via IAC ECHO negotiation.

**Risk: test setup helper churn.** The setup helper now returns `(TcpListener, SocketAddr, Arc<WorldRegistry>)`. Every existing M2 integration test needs updating. Mitigation: do the helper-signature change in one focused commit before adding new M3 tests, so the diff stays readable.

## Verification

```bash
make check                          # build + clippy + fmt + test
make run                            # then telnet 127.0.0.1 4444
RUST_LOG=debug make run             # verify state-transition logs at debug
RUST_LOG=trace make run             # verify password is redacted at trace level

# Scripted demo:
{ sleep 1; printf 'alice\r\n'; sleep 0.5; printf 'secret\r\n'; sleep 0.5; printf '1\r\n'; } | nc 127.0.0.1 4444

# Quit at each state:
printf 'quit\r\n' | nc 127.0.0.1 4444
{ printf 'alice\r\n'; sleep 0.2; printf 'exit\r\n'; } | nc 127.0.0.1 4444
{ printf 'alice\r\n'; sleep 0.2; printf 'p\r\n'; sleep 0.2; printf 'bye\r\n'; } | nc 127.0.0.1 4444

# Invalid world selections:
{ printf 'alice\r\n'; sleep 0.2; printf 'p\r\n'; sleep 0.2; printf '4\r\n'; sleep 0.2; printf 'abc\r\n'; sleep 0.2; printf '2\r\n'; } | nc 127.0.0.1 4444
```

Confirm: banner + Username prompt on connect, world list column-aligned, welcome message has the expected blank-line spacing, state transitions appear in debug logs (`from = ... to = ...`), passwords appear as `<redacted>` in trace logs, IAC stripped at every state, quit/exit/bye/disconnect work at every state, client disconnect logged cleanly.

## Definition of done for M3

- All acceptance criteria pass.
- `make check` (build + clippy + fmt + test) is green.
- The full v0.1 acceptance demo from ADR-0011 runs end-to-end manually (with stubbed auth/worlds).
- Code review surfaces no must-fix items remaining.
- `RUST_LOG=trace` does not leak passwords.
- Manual exploratory testing across at least two telnet clients confirms no breakage.
