# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Liaozhai MUX is a Rust-based multi-user text-world server in the PennMUSH/TinyMUSH lineage.
See `./README.md` for the full project description and `./docs/design/` for ADRs.

## Build Commands

`make help` provides the full list of commands, but here's a summary:

```bash
make build            # Debug build (all workspace crates)
make release          # Release build
make test             # Run all tests
make lint             # Clippy + format check
make format           # Auto-format with rustfmt
make check            # Build + lint + test
make check-all        # Build + lint + coverage
make coverage         # Text coverage report (cargo llvm-cov)
make docs             # Generate API docs
make run              # Run the server (default port 4444)
make run-config       # Run with config file (default liaozhai.toml)
```

Running a single test:

```bash
cargo test test_name
cargo test -p liaozhai-core test_name
```

Running the server:

```bash
cargo run --bin liaozhai-server -- run --port 4444
cargo run --bin liaozhai-server -- run --config liaozhai.toml
RUST_LOG=debug cargo run --bin liaozhai-server -- run --port 4444
```

## Architecture

The workspace contains six crates:

- `liaozhai-core` — shared types, IDs, error enum, constants (library, no I/O)
- `liaozhai-net` — telnet codec, connection handling (library)
- `liaozhai-auth` — account model, authentication (library)
- `liaozhai-worlds` — world registry, metadata (library)
- `liaozhai-server` — binary crate, CLI, TCP listener, runtime wiring
- `liaozhai` — umbrella crate, re-exports public API

Dependency flow: `liaozhai-core` is the root; all other crates depend on it.
The server binary depends on all four library crates.

See `docs/design/01-draft/0009-workspace-crate-layout.md` for the layout ADR.
See `docs/design/05-active/0011-v0.1-implementation-plan.md` for the implementation plan.

## Writing Code

### Rust Quality Guidelines

1. **`assets/ai/rust/SKILL.md`** — advanced Rust programming skill (**use this**)
2. **`assets/ai/rust/guides/`** — comprehensive Rust guidelines referenced by the skill
3. **`assets/ai/CLAUDE-CODE-COVERAGE.md`** — test coverage guide (general practices). The project's actual coverage targets are defined in ADR-0011 (80% by M6) and the M1 plan (60–70% at M1).

**Important:** `assets/ai/rust` may be a symlink. If it doesn't resolve, ask for assistance.

### Key Conventions

- Edition 2024, toolchain 1.85+
- `thiserror` in library crates, `anyhow` in the server binary
- `tracing` for structured logging — never `println!` or `eprintln!` (except CLI user output)
- `clap` derive for CLI argument parsing
- `?` for error propagation; no `unwrap()` on non-invariant paths
- `&str` not `&String`, `&[T]` not `&Vec<T>` in function signatures
- `#[derive(Debug, Clone, PartialEq)]` on all public types
- `#[non_exhaustive]` on public enums that may grow
- Newtypes for domain IDs (`AccountId`, `WorldId`, `ConnectionId`)
- Field-named getters without `get_` prefix (e.g., `fn username(&self) -> &str`)
- `#[expect(...)]` not `#[allow(...)]` for lint suppression
- `unsafe_code = "deny"` via workspace lints
- Always run `make format` after changes and `make lint` before committing

### Configuration

Server config is TOML. See `liaozhai.example.toml` for the schema.
CLI flags override config file values. `RUST_LOG` overrides the logging filter.

## Project Plan

See `docs/design/05-active/0011-v0.1-implementation-plan.md` for the v0.1 plan.
See `docs/dev/0001-liaozhai-mux-v0.1-m1-detailed-implementation-plan.md` for the M1 detail.

## Git Remotes

Pushes go to: macpro, github, codeberg (via `make push`)
