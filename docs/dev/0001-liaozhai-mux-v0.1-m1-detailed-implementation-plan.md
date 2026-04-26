# Liaozhai MUX v0.1 — M1 Detailed Implementation Plan

## Context

The Liaozhai MUX project has a comprehensive set of ADRs (0001-0011) defining the architecture, crate layout, concurrency model, and a six-milestone v0.1 implementation plan. The workspace currently has one placeholder crate (`crates/liaozhai/`), an outdated CLAUDE.md (from a prior project), and no tooling configuration (no rustfmt.toml, clippy config, Makefile, or rust-toolchain.toml).

This plan covers **Milestone 1 (M1): Workspace skeleton + tokio listener** — creating five new crates, wiring the tokio runtime, accepting TCP connections with a banner, and establishing the project's tooling and configuration foundation.

**Source documents:**
- `docs/design/05-active/0011-v0.1-implementation-plan.md` (milestones, deps, acceptance criteria)
- `docs/design/01-draft/0009-workspace-crate-layout.md` (crate responsibilities)
- `docs/design/01-draft/0005-tokio-actor-model-concurrency.md` (connection-as-task pattern)
- `assets/ai/rust/SKILL.md` + guides (Rust idioms, error handling, async patterns, observability)

---

## Dependency Graph

```
liaozhai-core          (no internal deps)
    ^
    |--- liaozhai-net      (depends on liaozhai-core)
    |--- liaozhai-auth     (depends on liaozhai-core)  [skeleton]
    |--- liaozhai-worlds   (depends on liaozhai-core)  [skeleton]
    |--- liaozhai (umbrella) (depends on liaozhai-core, re-exports)
    |
    +--- liaozhai-server   (depends on core, net, auth, worlds)
```

## Implementation Order

Build sequentially — each step depends only on prior steps:

1. Tooling config files (rust-toolchain.toml, rustfmt.toml)
2. Root Cargo.toml (add workspace.dependencies, workspace.lints, internal path deps)
3. `liaozhai-core` — fully implemented (types, errors, constants)
4. `liaozhai-net` — M1 partial (banner-then-close connection handler)
5. `liaozhai-auth` — skeleton (Account struct, no DB)
6. `liaozhai-worlds` — skeleton (WorldMetadata, empty WorldRegistry)
7. `liaozhai-server` — fully implemented for M1 (clap CLI, config, listener)
8. `liaozhai` umbrella — update to re-export liaozhai-core
9. .gitignore update (add data/, liaozhai.toml)
10. Example config file (liaozhai.example.toml)
11. Makefile
12. CLAUDE.md — full rewrite

---

## Step 1: Tooling Configuration

### `rust-toolchain.toml` (CREATE)
```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy", "llvm-tools-preview"]
```
`llvm-tools-preview` enables `cargo-llvm-cov` for M6 coverage. Installed toolchain is 1.93.

### `rustfmt.toml` (CREATE)
```toml
edition = "2024"
max_width = 100
use_field_init_shorthand = true
```

---

## Step 2: Root Cargo.toml (MODIFY)

Full replacement contents:

```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "Apache-2.0"
description = "A Rust-based multi-user exegesis for procedural literary fiction and world-building, in the PennMUSH/TinyMUSH lineage"
rust-version = "1.85"

[workspace.dependencies]
tokio              = { version = "1", features = ["full"] }
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
serde              = { version = "1", features = ["derive"] }
toml               = "0.8"
clap               = { version = "4", features = ["derive"] }
thiserror          = "2"
anyhow             = "1"
bytes              = "1"
uuid               = { version = "1", features = ["v4", "serde"] }
rusqlite           = { version = "0.31", features = ["bundled"] }
argon2             = "0.5"
rpassword          = "7"

# Internal crate dependencies
liaozhai-core   = { path = "crates/liaozhai-core" }
liaozhai-net    = { path = "crates/liaozhai-net" }
liaozhai-auth   = { path = "crates/liaozhai-auth" }
liaozhai-worlds = { path = "crates/liaozhai-worlds" }

[workspace.lints.rust]
unsafe_code = "deny"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
must_use_candidate = "allow"
dbg_macro = "warn"
print_stdout = "warn"
print_stderr = "warn"
```

**Note on `thiserror = "2"`**: thiserror 2.x has been current and stable since late 2024. The derive API is identical to 1.x. Toolchain 1.93 supports it. ADR-0011 has been updated to pin `"2"` so the plan and the ADR stay in sync.

---

## Step 3: `liaozhai-core` (FULLY IMPLEMENTED)

### `crates/liaozhai-core/Cargo.toml` (CREATE)
```toml
[package]
name = "liaozhai-core"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Shared types, IDs, error enum, and constants for Liaozhai MUX"
rust-version.workspace = true

[dependencies]
thiserror.workspace = true
uuid.workspace = true
serde.workspace = true

[lints]
workspace = true
```

### `crates/liaozhai-core/src/lib.rs` (CREATE)
```rust
//! Shared types, IDs, error enum, and constants for Liaozhai MUX.
//!
//! This crate is the foundation layer. Every other crate in the workspace
//! depends on it. It contains no I/O and no async code.

pub mod constants;
pub mod error;
pub mod id;
```

### `crates/liaozhai-core/src/id.rs` (CREATE)

Three newtype wrappers:

```rust
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorldId(Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(Uuid);
```

Each gets:
- `pub fn new() -> Self` — creates via `Uuid::new_v4()` (random v4)
- `pub fn from_uuid(uuid: Uuid) -> Self` — constructs from an existing UUID. Used by M4's SQLite loader and any future code that reconstructs IDs from persisted data; adding it now avoids surprise during M4.
- `pub fn uuid(&self) -> Uuid` — returns the inner UUID **by value**. `Uuid` is `Copy`, so a borrow-returning getter (`&Uuid`) would be unidiomatic and slightly less ergonomic for callers.
- `impl fmt::Display` — delegates to inner UUID

Tests: uniqueness, Display format, serde JSON roundtrip, `from_uuid` reconstructs an ID with the same `uuid()` value.

### `crates/liaozhai-core/src/error.rs` (CREATE)

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("authentication error: {0}")]
    Auth(String),

    #[error("network error: {0}")]
    Net(String),

    #[error("world error: {0}")]
    World(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

Tests: Display formatting, `From<io::Error>` conversion.

### `crates/liaozhai-core/src/constants.rs` (CREATE)

```rust
/// Project version. Picked up from `Cargo.toml` at compile time via `env!`,
/// so the banner stays in sync with the published version automatically.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Connection banner shown to clients on accept. Compile-time `concat!`
/// embeds the version directly; callers receive a `&'static str` with no
/// runtime allocation.
pub const BANNER: &str = concat!(
    "\r\n  Liaozhai MUX \u{804a}\u{9f4b} \u{2014} v",
    env!("CARGO_PKG_VERSION"),
    "\r\n  Multi-User eXegesis\r\n\r\n",
);

pub const DEFAULT_PORT: u16 = 4444;
pub const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1";
pub const DEFAULT_MAX_CONNECTIONS: usize = 100;
pub const DEFAULT_LOG_FILTER: &str = "info";
```

Tests: `BANNER` contains the project version and the "Multi-User eXegesis" subtitle; `VERSION` is non-empty.

---

## Step 4: `liaozhai-net` (M1 PARTIAL)

M1 scope: send banner, close connection. Codec and state machine arrive in M2/M3.

### `crates/liaozhai-net/Cargo.toml` (CREATE)
```toml
[package]
name = "liaozhai-net"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Telnet codec and connection handling for Liaozhai MUX"
rust-version.workspace = true

[dependencies]
liaozhai-core.workspace = true
tokio.workspace = true
tracing.workspace = true
bytes.workspace = true

[lints]
workspace = true
```

### `crates/liaozhai-net/src/lib.rs` (CREATE)
```rust
//! Network protocol, codec, and connection handling for Liaozhai MUX.

pub mod connection;
```

### `crates/liaozhai-net/src/connection.rs` (CREATE)

```rust
use liaozhai_core::constants;
use liaozhai_core::id::ConnectionId;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use std::net::SocketAddr;

pub async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
) -> liaozhai_core::error::Result<()>
```

Implementation:
1. Generate `ConnectionId::new()`
2. `info!(%conn_id, %peer, "connection accepted")`
3. Write `constants::BANNER` bytes to stream
4. Shut down the stream
5. `info!(%conn_id, %peer, "connection closed")`
6. Return `Ok(())`; warn on write/shutdown errors

Test: bind ephemeral port, connect, read stream to completion, assert banner present. Use `#[tokio::test]`.

---

## Step 5: `liaozhai-auth` (SKELETON)

### `crates/liaozhai-auth/Cargo.toml` (CREATE)
```toml
[package]
name = "liaozhai-auth"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Account model and authentication for Liaozhai MUX"
rust-version.workspace = true

[dependencies]
liaozhai-core.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

Only `liaozhai-core` and `thiserror` for now. `rusqlite`, `argon2`, `rpassword`, `tokio` are added in M4.

### `crates/liaozhai-auth/src/lib.rs` (CREATE)
```rust
//! Account model, authentication, and password hashing for Liaozhai MUX.

pub mod account;
```

### `crates/liaozhai-auth/src/account.rs` (CREATE)

```rust
use liaozhai_core::id::AccountId;

#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    id: AccountId,
    username: String,
}
```

Methods: `new(username: String) -> Self`, `id() -> AccountId`, `username() -> &str`

Test: create Account, verify getters.

---

## Step 6: `liaozhai-worlds` (SKELETON)

### `crates/liaozhai-worlds/Cargo.toml` (CREATE)
```toml
[package]
name = "liaozhai-worlds"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "World registry and world metadata for Liaozhai MUX"
rust-version.workspace = true

[dependencies]
liaozhai-core.workspace = true
serde.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

### `crates/liaozhai-worlds/src/lib.rs` (CREATE)
```rust
//! World registry, metadata, and lifecycle for Liaozhai MUX.

pub mod metadata;
pub mod registry;
```

### `crates/liaozhai-worlds/src/metadata.rs` (CREATE)

```rust
use liaozhai_core::id::WorldId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldMetadata {
    id: WorldId,
    slug: String,
    name: String,
    short_description: String,
}
```

Methods:
- `new(slug, name, short_description) -> Self`
- `id() -> WorldId` — returns by value; `WorldId` is `Copy`.
- `slug() -> &str`, `name() -> &str`, `short_description() -> &str` — borrow-returning getters for the string fields.

### `crates/liaozhai-worlds/src/registry.rs` (CREATE)

```rust
use crate::metadata::WorldMetadata;

#[derive(Debug, Clone, Default)]
pub struct WorldRegistry {
    worlds: Vec<WorldMetadata>,
}
```

Methods: `new() -> Self`, `len() -> usize`, `is_empty() -> bool`, `worlds() -> &[WorldMetadata]`

Test: empty registry has len 0 and is_empty true.

---

## Step 7: `liaozhai-server` (FULLY IMPLEMENTED FOR M1)

### `crates/liaozhai-server/Cargo.toml` (CREATE)
```toml
[package]
name = "liaozhai-server"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "The Liaozhai MUX server binary"
rust-version.workspace = true

[[bin]]
name = "liaozhai-server"
path = "src/main.rs"

[dependencies]
liaozhai-core.workspace = true
liaozhai-net.workspace = true
liaozhai-auth.workspace = true
liaozhai-worlds.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
clap.workspace = true
serde.workspace = true
toml.workspace = true
anyhow.workspace = true

[lints]
workspace = true
```

### `crates/liaozhai-server/src/main.rs` (CREATE)

**Clap CLI structure:**

```rust
#[derive(Debug, Parser)]
#[command(name = "liaozhai-server", version, about = "Liaozhai MUX server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the server.
    Run {
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        bind: Option<String>,
    },
    /// Account management (M4 placeholder).
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
}

#[derive(Debug, Subcommand)]
enum AccountAction {
    Create { username: String },
    List,
}
```

**`main()` function:**
- Parse `Cli::parse()`
- For `Command::Run`: load config via `config::load()`, init tracing, log startup, build tokio `Runtime::new()`, `block_on(listener::run(&cfg))`
- For `Command::Account`: print M4 placeholder messages to stderr

**Design note:** Builds tokio `Runtime` manually (not `#[tokio::main]`) for explicit control. `main() -> anyhow::Result<()>`.

### `crates/liaozhai-server/src/config.rs` (CREATE)

**Config types (all `#[derive(Debug, Clone, Deserialize)]` with `#[serde(default)]`):**

```rust
pub struct AppConfig {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub worlds: WorldsConfig,
    pub logging: LoggingConfig,
}

pub struct ServerConfig {
    pub bind_address: String,   // default "127.0.0.1"
    pub port: u16,              // default 4444
    pub max_connections: usize, // default 100
}

pub struct AuthConfig {
    pub db_path: String,        // default "./data/accounts.db"
}

pub struct WorldsConfig {
    pub registry_path: String,  // default "./data/worlds.toml"
}

pub struct LoggingConfig {
    pub default_filter: String, // default "info"
}
```

**Functions:**

```rust
pub fn load(
    config_path: Option<&Path>,
    port_override: Option<u16>,
    bind_override: Option<String>,
) -> anyhow::Result<AppConfig>
```

Precedence: CLI flags > TOML file > compiled defaults. Uses `std::fs::read_to_string` + `toml::from_str`.

```rust
pub fn init_tracing(logging: &LoggingConfig)
```

Sets up `tracing_subscriber::fmt()` with `EnvFilter`. Respects `RUST_LOG` if set, otherwise uses config's `default_filter`. Use the **non-panicking** `try_from_default_env` variant so a malformed `RUST_LOG` falls back gracefully:

```rust
let filter = EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| EnvFilter::new(&logging.default_filter));
```

Avoid `EnvFilter::from_default_env()` (panicking variant) — a typo in `RUST_LOG` should not crash the server at startup.

Tests: default config values, CLI overrides, TOML deserialization from hardcoded string.

### `crates/liaozhai-server/src/listener.rs` (CREATE)

```rust
pub async fn run(cfg: &AppConfig) -> anyhow::Result<()>
```

Implementation:
1. Bind `TcpListener` to `cfg.server.bind_address:cfg.server.port`
2. `info!(%addr, "listening for connections")`
3. Loop: `listener.accept().await` → `tokio::spawn` per connection → call `liaozhai_net::connection::handle_connection`
4. Log errors from accept and from individual connections (don't crash)

**Note:** No graceful shutdown in M1. The loop runs until Ctrl-C kills the process. M6 adds signal handling.

---

## Step 8: `liaozhai` umbrella (MODIFY)

### `crates/liaozhai/Cargo.toml` (MODIFY)
Add `liaozhai-core.workspace = true` to `[dependencies]` and `[lints] workspace = true`.

### `crates/liaozhai/src/lib.rs` (MODIFY)
Replace placeholder with:
```rust
//! Liaozhai MUX — umbrella crate.
//!
//! Re-exports the public API from workspace crates for convenient access.

pub use liaozhai_core::constants;
pub use liaozhai_core::error;
pub use liaozhai_core::id;
```

---

## Step 9: .gitignore Update (MODIFY)

Append:
```
# Runtime data
data/

# Local config (example is liaozhai.example.toml)
liaozhai.toml
```

---

## Step 10: Example Configuration (CREATE)

### `liaozhai.example.toml`
```toml
# Liaozhai MUX — example configuration file.
# Copy to liaozhai.toml and adjust as needed.

[server]
bind_address = "127.0.0.1"
port = 4444
max_connections = 100

[auth]
db_path = "./data/accounts.db"

[worlds]
registry_path = "./data/worlds.toml"

[logging]
default_filter = "info"
```

---

## Step 11: Makefile (CREATE)

```makefile
.PHONY: help build release test lint format check check-all docs run clean coverage push

CARGO  := cargo
PORT   ?= 4444
CONFIG ?= liaozhai.toml

help:           ## Show this help.
build:          ## Build all crates (debug).
release:        ## Build all crates (release).
test:           ## Run all tests.
lint:           ## Clippy + format check.
format:         ## Auto-format all sources.
check:          ## build + lint + test
check-all:      ## check + coverage
docs:           ## Generate API docs.
run:            ## Run server on PORT (default 4444).
run-config:     ## Run server with CONFIG file.
clean:          ## Remove build artifacts.
coverage:       ## Text coverage report (cargo-llvm-cov).
push:           ## Push to all remotes (macpro, github, codeberg).
```

(Full recipes specified in implementation.)

---

## Step 12: CLAUDE.md (FULL REWRITE)

Replace entire contents. Key sections:
- **Project Overview** — pointer to README.md, link to docs/design/
- **Build Commands** — make targets, cargo run examples, RUST_LOG usage
- **Architecture** — six crates, dependency flow, ADR pointers
- **Writing Code** — SKILL.md reference, key conventions (edition 2024, thiserror/anyhow split, tracing, clap derive, `?` propagation, borrowed types in signatures, field-named getters, `#[expect]`, `unsafe_code = "deny"`)
- **Configuration** — TOML schema, CLI override precedence, RUST_LOG
- **Git Remotes** — macpro, github, codeberg

---

## Complete File Inventory

### Files to CREATE (24 files)

| File | Purpose |
|------|---------|
| `rust-toolchain.toml` | Pin stable + components |
| `rustfmt.toml` | Format settings |
| `crates/liaozhai-core/Cargo.toml` | Core manifest |
| `crates/liaozhai-core/src/lib.rs` | Core crate root |
| `crates/liaozhai-core/src/id.rs` | AccountId, WorldId, ConnectionId newtypes |
| `crates/liaozhai-core/src/error.rs` | Project-wide Error enum |
| `crates/liaozhai-core/src/constants.rs` | Banner, defaults |
| `crates/liaozhai-net/Cargo.toml` | Net manifest |
| `crates/liaozhai-net/src/lib.rs` | Net crate root |
| `crates/liaozhai-net/src/connection.rs` | Banner-then-close handler |
| `crates/liaozhai-auth/Cargo.toml` | Auth manifest (skeleton) |
| `crates/liaozhai-auth/src/lib.rs` | Auth crate root |
| `crates/liaozhai-auth/src/account.rs` | Account struct (placeholder) |
| `crates/liaozhai-worlds/Cargo.toml` | Worlds manifest (skeleton) |
| `crates/liaozhai-worlds/src/lib.rs` | Worlds crate root |
| `crates/liaozhai-worlds/src/metadata.rs` | WorldMetadata struct |
| `crates/liaozhai-worlds/src/registry.rs` | WorldRegistry (empty) |
| `crates/liaozhai-server/Cargo.toml` | Server binary manifest |
| `crates/liaozhai-server/src/main.rs` | Entry point + clap CLI |
| `crates/liaozhai-server/src/config.rs` | Config types + TOML loading |
| `crates/liaozhai-server/src/listener.rs` | TCP accept loop |
| `liaozhai.example.toml` | Example config |
| `Makefile` | Build automation |

### Files to MODIFY (5 files)

| File | Change |
|------|--------|
| `Cargo.toml` (root) | Add workspace.dependencies, workspace.lints, rust-version, path deps |
| `crates/liaozhai/Cargo.toml` | Add liaozhai-core dep, lints, rust-version |
| `crates/liaozhai/src/lib.rs` | Replace placeholder with re-exports |
| `CLAUDE.md` | Full rewrite for Liaozhai MUX |
| `.gitignore` | Add data/, liaozhai.toml |

---

## M1 Status Summary

| Crate | M1 Status | What's Real |
|-------|-----------|-------------|
| `liaozhai-core` | **Full** | All ID newtypes (with `from_uuid` constructors), Error enum, constants (`VERSION`, `BANNER`, defaults) |
| `liaozhai-net` | **Partial** | handle_connection (banner + close). No codec, no state machine. |
| `liaozhai-auth` | **Skeleton** | Account struct with constructor/getters. No DB, no hashing. |
| `liaozhai-worlds` | **Skeleton** | WorldMetadata struct, empty WorldRegistry. No TOML loading. |
| `liaozhai-server` | **Full for M1** | Clap CLI (run + account stubs), config loading, tracing init, TCP listener. |
| `liaozhai` (umbrella) | **Updated** | Re-exports from liaozhai-core. |

---

## Verification (M1 Acceptance Criteria)

1. `cargo build --workspace` — succeeds with zero warnings
2. `cargo clippy --workspace -- -D warnings` — clean
3. `cargo fmt --all -- --check` — clean
4. `cargo test --workspace` — all tests pass
5. `cargo run --bin liaozhai-server -- run --port 4444` — opens port 4444, logs "listening for connections"
6. `telnet 127.0.0.1 4444` — connects, shows banner with "Liaozhai MUX 聊齋" and version, closes
7. `RUST_LOG=debug cargo run --bin liaozhai-server -- run --port 4444` — shows debug-level structured logs with connection ID and peer address
8. `cargo run --bin liaozhai-server -- run --config liaozhai.example.toml` — reads config, binds correctly
9. `cargo run --bin liaozhai-server -- run --config liaozhai.example.toml --port 5555` — CLI override wins, binds to 5555
10. `cargo run --bin liaozhai-server -- account create alice` — prints M4 placeholder message

---

## Test Coverage Target

M1 is mostly scaffolding — config structs, ID newtypes, banner constant, a banner-then-close connection handler. The `liaozhai-auth` and `liaozhai-worlds` crates are intentionally too thin to be meaningfully tested at M1; their substantive coverage lands at M4 and M5 respectively.

Realistic M1 coverage target: **60–70%** on the testable surface (`liaozhai-core`, `liaozhai-net::connection`, `liaozhai-server::config`). ADR-0011's 80%-by-M6 target stays on track but is not enforced at M1; expecting 80% at M1 would create noise about untestable skeletons rather than catching real gaps.

## Cargo.lock Policy

`Cargo.lock` is **committed** to version control. `liaozhai-server` is a binary crate, and committing the lockfile is the standard convention for binary projects — it ensures reproducible builds across contributor machines and CI. The library crates in the workspace inherit the same lockfile; this is fine because none of them are independently published from `liaozhai-server`'s release cycle.

## Resolved Decisions

- **`thiserror` version**: Using `"2"` (current stable, identical derive API). ADR-0011 has been updated to match.
- **Repository URL**: Including `repository = "https://github.com/hexagram30/liaozhai"` in `[workspace.package]`.
