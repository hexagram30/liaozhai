# Liaozhai MUX v0.1 — M6 Detailed Implementation Plan

## Context

M1-M5 are complete: the server accepts telnet connections, authenticates against SQLite+argon2, loads worlds from TOML, and walks clients through the full demo flow. M6 adds graceful shutdown (SIGINT/SIGTERM), `max_connections` enforcement via semaphore, rate limiter HashMap bounding, documentation polish, CI, and tags v0.1.0.

**Source documents:**
- `workbench/m6-implementation-plan.md` (design decisions)
- `docs/design/05-active/0011-v0.1-implementation-plan.md` (M6 acceptance criteria)

---

## Implementation Order

1. New constants (SERVER_FULL_MSG, shutdown/rate-limiter defaults)
2. Rate limiter HashMap bounding (eviction when at cap)
3. AuthConfig + ServerConfig expansion (new fields)
4. SessionContext: add `shutdown: CancellationToken`
5. Connection handler: integrate shutdown cancellation via `tokio::select!`
6. Listener: semaphore for max_connections + shutdown signal + drain timeout
7. Example config update
8. CI workflow (.github/workflows/check.yml)
9. Documentation polish (README quick-start, per-crate lib.rs docs, cargo doc clean)
10. Coverage gap analysis + targeted tests
11. Final verification + tag

---

## Step 1: New Constants

### `crates/liaozhai-core/src/constants.rs` (MODIFY) — append:
```rust
pub const SERVER_FULL_MSG: &str = "Server is full. Try again later.\r\n";
pub const SHUTDOWN_MSG: &str = "The studio closes for now. Until the next strange tale.\r\n";
pub const DEFAULT_SHUTDOWN_DRAIN_SECS: u64 = 10;
pub const DEFAULT_RATE_LIMITER_MAX_ENTRIES: usize = 10_000;
```

**Note on `SHUTDOWN_MSG` wording:** the message is intentionally Liaozhai-voiced ("The studio closes for now") rather than a generic "Server shutting down." The project's literary register carries through to operational messages. The constant is distinct from `GOODBYE_MSG` (the `quit`-triggered farewell) so clients can distinguish "I quit" from "server going down" in their experience and so operators see different log lines for the two paths.

---

## Step 2: Rate Limiter HashMap Bounding

### `crates/liaozhai-auth/src/rate_limiter.rs` (MODIFY)

Add `max_entries: usize` field to `AuthRateLimiter`. Constructor becomes `new(window, max_failures, max_entries)`.

In `record_failure`: when map is at capacity and the IP isn't already tracked, evict the entry with the oldest most-recent failure before inserting.

```rust
fn find_oldest_entry(map: &HashMap<IpAddr, VecDeque<Instant>>) -> Option<IpAddr> {
    map.iter()
        .filter_map(|(ip, deque)| deque.back().map(|t| (*ip, *t)))
        .min_by_key(|(_, t)| *t)
        .map(|(ip, _)| ip)
}
```

Tests: `eviction_when_at_cap`, `no_eviction_below_cap`, `existing_ip_not_evicted`.

---

## Step 3: Config Expansion

### `crates/liaozhai-server/src/config.rs` (MODIFY)

**ServerConfig** — add `shutdown_drain_secs: u64` (default 10).
**AuthConfig** — add `rate_limiter_max_entries: usize` (default 10_000).

---

## Step 4: SessionContext + CancellationToken

### `crates/liaozhai-net/src/context.rs` (MODIFY)

Add `shutdown: CancellationToken` field. Import from `tokio_util::sync::CancellationToken`.

**`tokio-util` features:** the existing dep enables only `codec`. `CancellationToken` lives behind the `sync` feature. Update workspace `Cargo.toml`:

```toml
tokio-util = { version = "0.7", features = ["codec", "sync"] }
```

This is required, not optional — without the `sync` feature, `tokio_util::sync::CancellationToken` doesn't exist and the import fails.

---

## Step 5: Connection Handler Shutdown

### `crates/liaozhai-net/src/connection.rs` (MODIFY)

Wrap the main I/O loop's `lines.next().await` in `tokio::select!` with `ctx.shutdown.cancelled()`:

```rust
loop {
    tokio::select! {
        biased;
        _ = ctx.shutdown.cancelled() => {
            let _ = writer.write_raw(constants::SHUTDOWN_MSG.as_bytes()).await;
            break;
        }
        line_result = lines.next() => match line_result { ... }
    }
}
```

`biased;` ensures shutdown is checked first each iteration — without it, a steady stream of incoming bytes from the codec could starve the shutdown branch.

**Don't wrap the rate-limit early-return path.** That path runs before the main I/O loop is entered, sends one short message (the `SERVER_FULL_MSG` analog for rate-limiting), and closes. If shutdown fires during that fraction-of-a-second write, the connection closes anyway. Adding cancellation handling there is overengineering for v0.1; v0.2+ can revisit if it surfaces as a real concern.

Tests in `connection.rs`:
- `shutdown_cancels_active_session` — open connection past the banner, cancel token, verify client receives the shutdown message and the server task exits cleanly.
- `shutdown_during_auth` — cancel token while client is at password prompt; verify clean cancellation.
- `shutdown_during_world_selection` — cancel token while client is at world selection prompt; verify the same.
- `shutdown_at_idle_session` — open connection past auth, then cancel without further client input; verify shutdown message arrives.

---

## Step 6: Listener Rewrite

### `crates/liaozhai-server/src/listener.rs` (MODIFY)

This is the largest change. The accept loop becomes:

1. Create `CancellationToken` (parent)
2. Create `Arc<Semaphore>` with `max_connections` permits
3. Spawn signal handler task that calls `shutdown.cancel()` on SIGINT/SIGTERM
4. Accept loop uses `tokio::select!` with `biased;`:
   - Shutdown branch → break out of accept loop
   - Accept branch → try_acquire_owned semaphore permit → if full, send SERVER_FULL_MSG and close; if acquired, spawn connection task holding permit
5. After loop breaks: drain with `tokio::time::timeout(drain_secs, join_set)` or similar

**Signal handler:**
```rust
#[cfg(unix)]
async fn wait_for_shutdown_signal() { /* sigint + sigterm via tokio::signal::unix */ }

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() { /* tokio::signal::ctrl_c */ }
```

**Drain strategy:** Use a `JoinSet` to track spawned connection tasks. After shutdown, `timeout(drain_duration, async { while join_set.join_next().await.is_some() {} })`. Stragglers after timeout are abandoned.

**Logging:**
- `info!("shutdown requested")` on signal
- `info!("closed listener")` after breaking accept loop
- `info!(drained = n, "drained connections")` or `warn!(remaining = n, "drain timeout, abandoning connections")`
- `info!("shutdown complete")`

Tests in `listener.rs` (or a new `tests/` directory under `liaozhai-server`):
- `max_connections_rejects_excess` — set max=2, open 2, attempt 3rd → receives `SERVER_FULL_MSG` and is closed.
- `semaphore_released_on_disconnect` — set max=1, open 1, close it, open another → succeeds (verifies RAII drop releases the permit).
- `shutdown_signal_drains_active_connections` — open 2 connections, signal shutdown, verify both receive the shutdown message and the listener exits within the drain timeout.
- `shutdown_drain_timeout_abandons_stuck_clients` — open a connection that doesn't read, configure low drain timeout (e.g., 200ms), signal shutdown, verify the listener exits after the timeout even though the client is unresponsive.

---

## Step 7: Example Config Update

### `liaozhai.example.toml` (MODIFY)

Add:
```toml
[server]
shutdown_drain_secs = 10

[auth]
rate_limiter_max_entries = 10000
```

---

## Step 8: CI Workflow

### `.github/workflows/check.yml` (CREATE)

```yaml
name: CI
on: [push, pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: make check
```

---

## Step 9: Documentation Polish

### README.md — add Quick Start section

After the existing content, add a "Quick start" section with clone → setup → build → run commands per the design plan.

### Per-crate lib.rs docs — expand one-liners to brief paragraphs

Each crate gets a module-level doc that names the crate's role, lists its key public types, and notes how it fits into the workspace dependency graph. Suggested content:

**`liaozhai-core/src/lib.rs`:**
```rust
//! Foundation crate for Liaozhai MUX.
//!
//! Provides shared types ([`id::AccountId`], [`id::WorldId`],
//! [`id::ConnectionId`]), the project-wide [`error::Error`] enum, and
//! workspace-wide [`constants`]. Every other workspace crate depends
//! on this one. Contains no I/O, no async code, no platform assumptions.
```

**`liaozhai-auth/src/lib.rs`:**
```rust
//! Account management and authentication for Liaozhai MUX.
//!
//! [`store::AccountStore`] holds accounts in SQLite with argon2id-hashed
//! passwords; [`rate_limiter::AuthRateLimiter`] tracks failed-login
//! attempts per IP for sliding-window throttling; [`params::Argon2Params`]
//! encapsulates the hashing parameters loaded from configuration.
```

**`liaozhai-net/src/lib.rs`:**
```rust
//! Network protocol and connection handling for Liaozhai MUX.
//!
//! [`codec::TelnetLineCodec`] strips telnet IAC sequences and produces
//! line-oriented input; [`connection::handle_connection`] drives a
//! single TCP session through the [`session::Session`] state machine
//! (banner → auth → world selection → goodbye); [`output::LineWriter`]
//! provides atomic line-and-CRLF writes; [`context::SessionContext`]
//! bundles the per-connection dependencies.
```

**`liaozhai-worlds/src/lib.rs`:**
```rust
//! World registry for Liaozhai MUX.
//!
//! [`registry::WorldRegistry`] holds a list of [`metadata::WorldMetadata`]
//! loaded from a TOML file at startup. v0.1 worlds are display-only
//! (slug, name, short description); per-world ECS state arrives in v0.2+.
```

**`liaozhai/src/lib.rs`:** (umbrella crate)
```rust
//! Liaozhai MUX umbrella crate.
//!
//! Re-exports the curated public API of the workspace. Most consumers
//! should depend on this crate rather than the individual workspace
//! members.
```

### `cargo doc --workspace --no-deps` clean

Run `cargo doc --workspace --no-deps 2>&1 | tee doc-output` and fix any warnings that surface (typically: missing intra-doc links, broken `[Type]` references, undocumented public items if `#![deny(missing_docs)]` is enabled — currently it isn't).

### README quick-start placement

Insert the "Quick start" section directly after the project's tagline / one-line description (before "About") so it's immediately discoverable to a contributor landing on the README. The existing About / Etymology / Architecture sections follow.

---

## Step 10: Coverage + Targeted Tests

Run `cargo llvm-cov --workspace` (if installed) or estimate from test coverage. Fill gaps with targeted tests focused on:
- Error/warn branches in connection handler
- CLI edge cases not yet covered
- Any untested validation paths

---

## Step 11: Final Verification + Release

The release sequence (run in order, halt if any step fails):

1. **Workspace version bump.** Edit `Cargo.toml`'s `[workspace.package].version` from `"0.0.1"` to `"0.1.0"`. Member crates inherit via `version.workspace = true`, so this is a single line. Run `cargo build --workspace` to update `Cargo.lock`. Commit with message `Release v0.1.0`.

2. **Quality gates.**
   - `make check` (build + clippy + fmt + test) is green.
   - `cargo doc --workspace --no-deps` produces no warnings.
   - `cargo llvm-cov --workspace` reports ≥80% on the four library crates.

3. **Manual acceptance demo.** From a fresh clone, follow the README quick-start: build, create an account, run the server, connect via telnet, walk the full demo flow, verify graceful shutdown on Ctrl-C. This is the definition-of-done gate.

4. **Tag.**
   ```bash
   git tag -a v0.1.0 -m "Liaozhai MUX v0.1.0 — connect, authenticate, list worlds"
   git push origin v0.1.0
   # Or for all remotes:
   make push
   ```
   Annotated tag carries the release-note summary.

5. **Crates.io publish.** Per Rust skill CG-PUB-02 pre-publish checklist:
   ```bash
   cd crates/liaozhai
   cargo publish --dry-run     # verify package builds and includes the right files
   cargo publish               # publishes to crates.io
   ```
   The umbrella crate `liaozhai` at v0.1.0 supersedes the v0.0.1 placeholder published at project start. Every release goes to crates.io — no hold-back path; if `cargo publish` fails, halt the release and resolve the underlying issue.

---

## File Inventory

### CREATE (1 file)
| File | Purpose |
|------|---------|
| `.github/workflows/check.yml` | CI workflow |

### MODIFY (~12 files)
| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Bump version 0.0.1→0.1.0; add `sync` feature to `tokio-util` |
| `crates/liaozhai-core/src/constants.rs` | `SERVER_FULL_MSG`, `SHUTDOWN_MSG` (or omit if reusing `GOODBYE_MSG`), defaults |
| `crates/liaozhai-auth/src/rate_limiter.rs` | Bounded HashMap with eviction; new tests |
| `crates/liaozhai-server/src/config.rs` | `shutdown_drain_secs`, `rate_limiter_max_entries` |
| `crates/liaozhai-net/src/context.rs` | Add `CancellationToken` |
| `crates/liaozhai-net/src/connection.rs` | `tokio::select!` with shutdown; new tests |
| `crates/liaozhai-server/src/listener.rs` | Semaphore + shutdown + drain via `JoinSet`; new tests |
| `liaozhai.example.toml` | New config fields |
| `README.md` | Quick start section near the top |
| Per-crate `lib.rs` files (5) | Expanded module docs per the suggested content above |

---

## Test Coverage Target

- **`liaozhai-core`**: 90%+ (mostly constants and an Error enum; trivial to cover comprehensively).
- **`liaozhai-auth`**: 85%+ (store + params + rate_limiter all well-testable; the eviction tests added in M6 close the last gap).
- **`liaozhai-worlds`**: 85%+ (already at this level after M5).
- **`liaozhai-net`**: 80%+ (connection module is I/O-heavy; the integration tests added in M6 for shutdown push it over the line).
- **`liaozhai-server`**: 70%+ (binary crate; CLI tests cover the main subcommand surface, the listener has new tests for semaphore + drain).
- **Workspace overall**: 80%+ — meets ADR-0011's M6 target.

## Risks

**Risk: shutdown drain timeout strands stuck clients.** A client that's mid-typing or unresponsive gets cut off after the configured drain window. *Mitigation:* the timeout is configurable; the 10-second default is generous for normal use and aggressive enough that a misbehaving client can't hold the server hostage.

**Risk: `tokio::select!` ordering — without `biased;`, shutdown can starve.** Under a steady stream of incoming connections (or input bytes on a session), the select macro's randomized branch order can repeatedly pick the I/O branch and never observe the shutdown signal. *Mitigation:* `biased;` in both the listener accept loop and the connection I/O loop. Tests verify shutdown happens promptly even under load.

**Risk: semaphore permit leak.** If a connection task panics between accept and permit-drop, the permit count stays accurate (Rust drops on unwind). *Mitigation:* RAII via `OwnedSemaphorePermit`; no manual permit management.

**Risk: `tokio-util` `sync` feature missing.** If the `Cargo.toml` update is forgotten, the build fails with "cannot find type `CancellationToken`" — clear and fast to diagnose. *Mitigation:* the update is the first item in step 4's instructions.

**Risk: rate limiter eviction's O(n) walk under sustained attack.** Each `record_failure` past the cap walks the entire map. At 10,000 entries this is microseconds. *Mitigation:* none needed at v0.1 scale; documented as O(n) for future awareness.

**Risk: cargo publish of v0.1.0 cannot be undone.** A bad publish lives on crates.io forever. *Mitigation:* `cargo publish --dry-run` first per the release sequence in step 11. Verify package contents (no accidental `.env` files, secrets, or large binary artifacts).

**Risk: GitHub Actions YAML fragility.** Indentation or field naming errors break CI silently or noisily. *Mitigation:* the workflow is short and uses well-known actions. Test on a feature branch (or commit to `main` and watch the first run) before relying on it.

## Definition of done for M6

- All M6 acceptance criteria pass.
- `make check` (build + clippy + fmt + test) is green.
- `cargo doc --workspace --no-deps` produces no warnings.
- `cargo llvm-cov --workspace` shows ≥80% on the four library crates.
- README quick-start works from a fresh clone (manually verified).
- CI workflow runs and passes on `main`.
- Workspace version is `0.1.0` in `Cargo.toml`.
- `git tag -a v0.1.0` is annotated and pushed.
- Crates.io publish complete: `cargo publish -p liaozhai` succeeded.

## Verification

```bash
make check                          # build + clippy + fmt + test
cargo doc --workspace --no-deps     # no warnings

# Manual graceful shutdown:
cargo run --bin liaozhai-server -- run &
# In another terminal: telnet 127.0.0.1 4444 → log in → start typing
# Ctrl-C the server → telnet client sees the shutdown message and connection closes

# Manual max_connections check:
# Edit liaozhai.toml: set max_connections = 2
# Open 2 telnet sessions; attempt a 3rd → "Server is full. Try again later."

# Rate limiter bounding:
# Covered by unit tests; manual exercise impractical (20,000+ distinct IPs)

# Full acceptance demo end-to-end:
mkdir -p data && cp worlds.example.toml data/worlds.toml
printf 'secret\nsecret\n' | cargo run --bin liaozhai-server -- account create alice
cargo run --bin liaozhai-server -- run &
SERVER_PID=$!
sleep 1
{ printf 'alice\r\n'; sleep 0.2; printf 'secret\r\n'; sleep 0.2; printf '1\r\n'; } | nc 127.0.0.1 4444
kill $SERVER_PID

# Coverage report:
cargo llvm-cov --workspace --html
# Open target/llvm-cov/html/index.html; verify ≥80% on library crates

# CI verification (after pushing the workflow):
git push origin <feature-branch>
# Watch the GitHub Actions run; ensure it passes
```
