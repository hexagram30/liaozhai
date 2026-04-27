# Liaozhai MUX v0.1 — M4 Detailed Implementation Plan

## Context

M3 is complete: the connection state machine drives banner → username → password → world-list → world-selection → goodbye with stubbed auth. M4 replaces the stub with real SQLite-backed credentials, argon2id hashing, per-IP rate limiting, per-connection retry counter, IAC ECHO password masking, and working CLI account management.

**Source documents:**
- `workbench/m4-implementation-plan.md` (design decisions, rationale)
- `docs/design/05-active/0011-v0.1-implementation-plan.md` (M4 acceptance criteria)

---

## Implementation Order (13 steps)

```
1. New constants (liaozhai-core)
2. Account struct changes (liaozhai-auth)
3. Argon2Params struct (liaozhai-auth/params.rs) — CREATE
4. AccountStore (liaozhai-auth/store.rs) — CREATE, depends on 2,3
5. AuthRateLimiter (liaozhai-auth/rate_limiter.rs) — CREATE
6. liaozhai-auth Cargo.toml + lib.rs wiring
7. AuthConfig expansion (liaozhai-server/config.rs)
8. SessionContext + Transition::AuthPending (liaozhai-net)
9. Session changes (session.rs) — AuthPending, complete_auth
10. Connection handler rewrite (connection.rs) — retry, rate limit, IAC ECHO
11. Listener changes (listener.rs) — construct SessionContext
12. CLI account subcommands (main.rs) — real create/list
13. Example config + workspace deps update
```

---

## Step 1: New Constants

### `crates/liaozhai-core/src/constants.rs` (MODIFY) — append:
```rust
pub const AUTH_FAILED_MSG: &str = "Authentication failed.\r\n";
pub const AUTH_MAX_RETRIES_MSG: &str = "Too many failed attempts. Disconnecting.\r\n";
pub const AUTH_RATE_LIMITED_MSG: &str = "Too many failed attempts from your IP. Try again later.\r\n";
pub const AUTH_INTERNAL_ERROR_MSG: &str = "Authentication error. Please try again later.\r\n";
pub const IAC_WILL_ECHO: &[u8] = &[0xFF, 0xFB, 0x01];
pub const IAC_WONT_ECHO: &[u8] = &[0xFF, 0xFC, 0x01];
pub const DEFAULT_MAX_LOGIN_ATTEMPTS: u32 = 3;
pub const DEFAULT_ARGON2_MEMORY_COST: u32 = 19_456;
pub const DEFAULT_ARGON2_TIME_COST: u32 = 2;
pub const DEFAULT_ARGON2_PARALLELISM: u32 = 1;
pub const DEFAULT_RATE_LIMIT_WINDOW_SECS: u64 = 60;
pub const DEFAULT_RATE_LIMIT_MAX_FAILURES: usize = 10;
```

---

## Step 2: Account Struct Changes

### `crates/liaozhai-auth/src/account.rs` (MODIFY)

Add `created_at: i64` and `last_login_at: Option<i64>` fields with getters.

**Keep `Account::new(username)`** as the in-memory constructor for tests and any non-DB construction site — it now sets `created_at` to the current Unix epoch (via `SystemTime::UNIX_EPOCH.elapsed()`) and `last_login_at` to `None`. **Add `Account::from_row(id, username, created_at, last_login_at)`** for loading from SQLite.

Two constructors instead of replacing `new` — keeps M3 tests untouched (no cascading edits to `crates/liaozhai-net/src/session.rs` test fixtures), and `new()` remains the natural shape for "give me an in-memory Account I can use for testing." `from_row` is named for what it does (constructing from a database row).

---

## Step 3: Argon2Params Struct

### `crates/liaozhai-auth/src/params.rs` (CREATE)
```rust
pub struct Argon2Params {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}
```

**Constructors:**
- `pub fn new(m_cost: u32, t_cost: u32, p_cost: u32) -> Self` — explicit values
- `impl Default` — production values from `liaozhai_core::constants` (`DEFAULT_ARGON2_*`)
- `pub fn test_fast() -> Self` — small values for tests (`m_cost: 256, t_cost: 1, p_cost: 1`); produces ~1 ms hashes so test suites run quickly. Document in rustdoc that this is for tests only.

**Methods:**
- `pub fn to_argon2_params(&self) -> Result<argon2::Params>` — converts to the argon2 crate's Params type, returning an error if the values are out of range.

The named `test_fast` constructor is the central place for "small-but-realistic argon2 params" and is reused across `liaozhai-auth` and `liaozhai-net` test setup.

---

## Step 4: AccountStore

### `crates/liaozhai-auth/src/store.rs` (CREATE)

The biggest new file. `Arc<Mutex<rusqlite::Connection>>` + `spawn_blocking`.

**Schema:** `accounts(id TEXT PK, username TEXT UNIQUE COLLATE NOCASE, password_hash TEXT, created_at INTEGER, last_login_at INTEGER)`

**API:**
- `open(path, params) -> Result<Self>` — sync, creates file+table, generates dummy hash
- `create_account(username, password) -> Result<Account>` — async; uses the params stored at `open()`. Returns `Err(Error::Auth("account already exists"))` on UNIQUE constraint violation (detect via `rusqlite::Error::SqliteFailure` with `ErrorCode::ConstraintViolation`).
- `verify_credentials(username, password) -> Result<Option<Account>>` — async, timing-attack resistant (dummy hash for unknown users)
- `list_accounts() -> Result<Vec<Account>>` — async
- `record_login(account_id) -> Result<()>` — async

**No `username_exists` method.** The CLI's "is this username taken?" check happens atomically inside `create_account` via the UNIQUE constraint and the resulting error variant. A separate pre-check would introduce a TOCTOU race window between the check and the insert (someone else could create the username in between) and would also be a wasted query in the success path.

Dummy hash generated once at `open()` (using the configured argon2 params) for timing-attack resistance — verifying against the dummy on missing-user takes the same time as verifying against a real hash.

**Schema documentation.** Module-level rustdoc on `store.rs` documents the table schema, column meanings, and the timing-resistant verification approach. No separate `docs/dev/schema.md` file — keeping schema docs co-located with the code that owns them is more maintainable.

Tests: ~14 using `tempfile` for temp DB paths. Use `Argon2Params::test_fast()` (defined in step 3) so the test suite stays fast. Include explicit duplicate-username test that exercises the UNIQUE constraint error.

---

## Step 5: AuthRateLimiter

### `crates/liaozhai-auth/src/rate_limiter.rs` (CREATE)
```rust
pub struct AuthRateLimiter {
    window: Duration, max_failures: usize,
    failures: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}
```
- `new(window, max_failures)`, `is_throttled(ip)`, `record_failure(ip)`, `reset(ip)`
- Lazy pruning on every call. `// TODO(M6): cap HashMap with LRU eviction`

Tests: ~6 (not_throttled, throttled_after_max, below_max, reset, pruning, independent_ips).

---

## Step 6: liaozhai-auth Cargo.toml + lib.rs

### `crates/liaozhai-auth/Cargo.toml` (MODIFY) — add:
```toml
rusqlite.workspace = true
argon2.workspace = true
tokio.workspace = true
uuid.workspace = true

[dev-dependencies]
tempfile.workspace = true
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
```

### `crates/liaozhai-auth/src/lib.rs` (MODIFY):
```rust
pub mod account;
pub mod params;
pub mod rate_limiter;
pub mod store;
```

---

## Step 7: AuthConfig Expansion

### `crates/liaozhai-server/src/config.rs` (MODIFY)

Expand `AuthConfig` with: `max_login_attempts`, `argon2_memory_cost`, `argon2_time_cost`, `argon2_parallelism`, `rate_limit_window_secs`, `rate_limit_max_failures`. All with defaults from constants. Add `argon2_params() -> Argon2Params` and `rate_limit_window() -> Duration` helpers.

---

## Step 8: SessionContext + Transition::AuthPending

### `crates/liaozhai-net/src/context.rs` (CREATE)
```rust
pub struct SessionContext {
    pub account_store: Arc<AccountStore>,
    pub world_registry: Arc<WorldRegistry>,
    pub rate_limiter: Arc<AuthRateLimiter>,
    pub max_login_attempts: u32,
}
```

### `crates/liaozhai-net/src/session.rs` (MODIFY) — add Transition variant:
```rust
AuthPending { username: String, password: String },
```

The session is sync; the connection handler does the async `verify_credentials` call when it sees `AuthPending`, then calls `session.complete_auth(result)`.

Add `Session::complete_auth(&self, Option<Account>) -> Transition`:
- `Some(account)` → Advance to WorldSelection with welcome + world list
- `None` → Advance to Authenticating { username: None } with "Authentication failed." + username prompt

### `crates/liaozhai-net/src/lib.rs` (MODIFY) — add `pub mod context;`

---

## Step 9: Session Changes

Modify `handle_password_input` to return `AuthPending` instead of the stubbed success. Update all tests: `password_accepts_non_empty` becomes `password_returns_auth_pending`. Add `complete_auth_success` and `complete_auth_failure` tests. Update all `Account::new("alice")` calls to `Account::from_row(...)`.

---

## Step 10: Connection Handler Rewrite

### `crates/liaozhai-net/src/connection.rs` (MODIFY)

**Signature:** `handle_connection(stream, peer, ctx: Arc<SessionContext>)`

**New logic in I/O loop:**

1. **Rate limit check on connect** — `ctx.rate_limiter.is_throttled(peer.ip())` → send rate-limit message, disconnect immediately.

2. **AuthPending handling sequence (precise ordering matters):**
   1. Receive `Transition::AuthPending { username, password }` from `session.handle_input`.
   2. **Immediately write `IAC_WONT_ECHO`** to the client. The client should restore local echo as soon as we have the password — regardless of whether verification succeeds, the *next* prompt the user sees (welcome or auth-failed) is non-secret.
   3. Call `ctx.account_store.verify_credentials(&username, &password).await`.
   4. Drop the password (Rust scope drop is sufficient for v0.1; explicit zeroization is a v0.2+ hardening question).
   5. Call `session.complete_auth(result)` to get the next `Transition`.
   6. On success: `ctx.rate_limiter.reset(peer.ip())`, then `ctx.account_store.record_login(account.id()).await` (awaited but failure is logged-and-ignored — don't drop the user back to login because a timestamp update failed).
   7. On failure: `ctx.rate_limiter.record_failure(peer.ip())`, increment `auth_failures`. If `auth_failures >= ctx.max_login_attempts`, write `AUTH_MAX_RETRIES_MSG` and disconnect.

3. **Retry counter** — `auth_failures: u32`, disconnect after `ctx.max_login_attempts`. Counter resets implicitly on successful auth (the connection moves past the auth state).

4. **IAC ECHO** — three trigger points:
   - **Entering password state**: when `Transition::Advance.next` is `Authenticating { username: Some(_) }`, prepend `IAC_WILL_ECHO` to the output before writing.
   - **Receiving AuthPending**: write `IAC_WONT_ECHO` immediately (per the AuthPending sequence above), before invoking `verify_credentials`.
   - **Disconnect from password state**: if the user types a session terminator at the password prompt, the session returns `Transition::Disconnect`; the connection handler must check `session.is_password_input()` *before applying the disconnect* and write `IAC_WONT_ECHO` if true, then write the goodbye.

**Test changes:** All existing tests updated for `Arc<SessionContext>` signature. Setup helper creates temp DB with `Argon2Params::test_fast()` and a pre-created "alice" account; the test password is a module-level constant (e.g., `const TEST_PASSWORD: &str = "secret"`) so multiple tests can reference it without drift. Add ~8 new integration tests (failed login, retry limit, rate limiter, IAC WILL ECHO precedes password prompt, IAC WONT ECHO follows password consumption, IAC WONT ECHO on quit-at-password, record_login updates timestamp, unknown user).

Add `tempfile` to `liaozhai-net` dev-deps.

---

## Step 11: Listener Changes

### `crates/liaozhai-server/src/listener.rs` (MODIFY)

Construct `AccountStore::open`, `AuthRateLimiter::new`, `Arc<SessionContext>` before accept loop. Pass `ctx` to `handle_connection`.

---

## Step 12: CLI Account Subcommands

### `crates/liaozhai-server/src/main.rs` (MODIFY)

**Move `--config` to global Cli arg** (not per-subcommand). Both `Run` and `Account` branches use it. This is a CLI-shape change from M3 — `liaozhai-server run --config foo.toml` becomes `liaozhai-server --config foo.toml run`. Pre-1.0 we can do this without ceremony.

**`account create <username>`:**
1. Validate the username is non-empty after trim. (No whitespace / special-char restrictions for v0.1; SQLite's UNIQUE NOT NULL handles the rest.)
2. Open the store with the configured argon2 params.
3. Prompt for password twice via `rpassword::prompt_password("Password: ")` and `rpassword::prompt_password("Confirm:  ")`. `rpassword` reads from the controlling TTY when available, and falls back to stdin in non-TTY contexts (which is what `assert_cmd` tests rely on — pipe `secret\nsecret\n` to stdin).
4. If the two passwords don't match, print "Passwords do not match." to stderr and exit code 1.
5. Call `account_store.create_account(username, password).await`. The UNIQUE constraint check is atomic — a duplicate-username error returns `Err(Error::Auth("account already exists"))` from the store (per step 4 above). The CLI translates this into "Account '{username}' already exists." to stderr and exit code 1.
6. On success: "Account '{username}' created." to stdout, exit code 0.

**`account list`:**
1. Open the store.
2. Call `account_store.list_accounts().await`.
3. Print a column-aligned table to stdout with columns: ID, USERNAME, CREATED, LAST LOGIN. Use the `time` crate to format Unix epoch as ISO-ish local time. NULL `last_login_at` displays as `(never)`.

**CLI test framework.** Use `assert_cmd` (already a workspace dev-dep from M2). Tests pipe stdin for password input — no `--password` or `--stdin` flag needed because `rpassword` handles non-TTY stdin natively.

### `crates/liaozhai-server/Cargo.toml` (MODIFY) — add `rpassword`, `time` (formatting only — no parsing needed since we only display timestamps)

---

## Step 13: Workspace + Example Config

### `Cargo.toml` (workspace) — add:
```toml
tempfile = "3"
time = { version = "0.3", features = ["formatting"] }
```

(`time`'s `parsing` feature is dropped — we only format timestamps for display, never parse user-typed time strings.)

### `liaozhai.example.toml` (MODIFY) — add auth section fields

---

## File Inventory

### CREATE (4 files)
| File | Purpose |
|------|---------|
| `crates/liaozhai-auth/src/params.rs` | Argon2Params struct |
| `crates/liaozhai-auth/src/store.rs` | AccountStore (SQLite + argon2) |
| `crates/liaozhai-auth/src/rate_limiter.rs` | AuthRateLimiter |
| `crates/liaozhai-net/src/context.rs` | SessionContext |

### MODIFY (14 files)
| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Add tempfile, time |
| `crates/liaozhai-core/src/constants.rs` | Auth/IAC/rate-limit constants |
| `crates/liaozhai-auth/Cargo.toml` | Add rusqlite, argon2, tokio, uuid, tempfile |
| `crates/liaozhai-auth/src/lib.rs` | Add params, store, rate_limiter modules |
| `crates/liaozhai-auth/src/account.rs` | New fields, from_row constructor |
| `crates/liaozhai-net/Cargo.toml` | Add tempfile dev-dep |
| `crates/liaozhai-net/src/lib.rs` | Add context module |
| `crates/liaozhai-net/src/session.rs` | AuthPending variant, complete_auth, test updates |
| `crates/liaozhai-net/src/connection.rs` | Full rewrite with retry/rate-limit/IAC ECHO |
| `crates/liaozhai-server/Cargo.toml` | Add rpassword, time |
| `crates/liaozhai-server/src/config.rs` | Expanded AuthConfig |
| `crates/liaozhai-server/src/main.rs` | Real CLI subcommands, global --config |
| `crates/liaozhai-server/src/listener.rs` | Construct SessionContext |
| `liaozhai.example.toml` | New auth config fields |

---

## Verification

```bash
make check                          # build + clippy + fmt + test

# Create account and run server:
cargo run --bin liaozhai-server -- account create alice
cargo run --bin liaozhai-server -- account list
cargo run --bin liaozhai-server -- run --port 4444

# Test auth flow:
telnet 127.0.0.1 4444               # alice + correct password → world list
                                     # alice + wrong password → "Authentication failed."
                                     # 3 wrong passwords → disconnect
                                     # PuTTY: verify password is masked

# Rate limiting:
# Fail 10+ times from same IP → next connection rejected immediately

RUST_LOG=debug cargo run --bin liaozhai-server -- run
# Verify state-transition logs, auth success/failure logs
RUST_LOG=trace cargo run --bin liaozhai-server -- run
# Verify password is "<redacted>" in trace

# Manual timing-attack measurement (loose):
time printf 'doesnotexist\r\nwhatever\r\n' | nc 127.0.0.1 4444
time printf 'alice\r\nwrongpass\r\n' | nc 127.0.0.1 4444
# These should be within ~10ms of each other.
```

## Test Coverage Target

- **`liaozhai-auth::store`**: 80–90%. The store has dense logic (hashing, timing-resistant verification, schema management) all of which is testable through the public API.
- **`liaozhai-auth::rate_limiter`**: 80%+. Pure logic — manipulate `Instant`-based time via test helpers if needed.
- **`liaozhai-auth::params`**: 90%+. Trivial.
- **`liaozhai-net::connection`**: 65–75%. I/O-bound; integration tests cover the main paths including the new auth flow.
- **`liaozhai-net::session`**: maintained from M3 (80–90%). M4 adds the `AuthPending` variant and `complete_auth` method but the rest of the machine is unchanged.
- **CLI subcommand handlers**: 70%+ via `assert_cmd`.
- **Workspace overall (M4 cumulative)**: 75–80%. ADR-0011's 80%-by-M6 target is on track.

## Definition of done for M4

- All acceptance criteria pass.
- `make check` (build + clippy + fmt + test) is green.
- The full v0.1 acceptance demo from ADR-0011 runs end-to-end with real auth.
- Code review surfaces no must-fix items remaining.
- Schema documented in module-level rustdoc on `liaozhai-auth/src/store.rs`.
- Manual exploratory testing across at least two telnet clients confirms password masking works (PuTTY + BSD telnet at minimum).
- Manual timing measurement (per the verification block) confirms unknown-user vs. wrong-password latencies are within ~10 ms.
- `RUST_LOG=trace` does not leak passwords.
