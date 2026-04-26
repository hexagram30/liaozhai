---
number: 9
title: "Workspace crate layout"
author: "Duncan McGreggor"
component: All
tags: [change-me]
created: 2026-04-25
updated: 2026-04-25
state: Draft
supersedes: null
superseded-by: null
version: 1.0
---

# Workspace crate layout

**Status:** Draft
**Date:** 2026-04-25
**Type:** ADR

## Context

Liaozhai MUX is a multi-faceted project: a server, a builder client (TUI), shared world-model code, soft-code integration, persistence, networking, and (eventually) generative-text infrastructure. Holding all of this in one crate would slow compile times, blur module boundaries, and complicate dependency management — `bevy_ecs` server-side wants different deps than `bevy + bevy_ratatui` client-side, for instance.

The project already has a Cargo workspace structure with the umbrella `liaozhai` crate at `crates/liaozhai/`. We need a layout that scales as v0.1 → v1.0 work progresses and keeps the boundaries between concerns clean.

The original architectural research recommends a workspace structure; this ADR fixes the specific shape we're committing to, with explicit notes about which crates exist now versus which arrive at later versions.

## Decision

The workspace lives at `crates/` with focused crates. Cross-cutting concerns (logging, error types, version metadata) live in a small `liaozhai-core` (or `liaozhai-shared`) crate that everything depends on. The umbrella `liaozhai` crate re-exports a curated subset for ergonomic consumers.

Initial layout (v0.1 scope, with later additions noted):

```
liaozhai/
├── Cargo.toml                       # workspace manifest
├── crates/
│   ├── liaozhai/                    # umbrella; published; re-exports curated public API
│   ├── liaozhai-core/               # shared types, error enum, IDs, version constants
│   ├── liaozhai-world/              # ECS world model, components, relationships, Cells, Things
│   ├── liaozhai-net/                # network protocol, codec, side-channel; client-agnostic
│   ├── liaozhai-server/             # the server binary; ties net + world + auth + worlds
│   ├── liaozhai-auth/               # account model, authentication; SQLite-backed
│   ├── liaozhai-worlds/             # world registry, world lifecycle, cross-world channels
│   ├── liaozhai-cmd/                # command parser, dispatch, builder/player command set
│   └── liaozhai-persist/            # template loading (RON/JSON), snapshot save/load (bevy_save)
│
│   # arrives at v0.4 (Lykn integration):
│   ├── liaozhai-script/             # Lykn AST consumer, walking interpreter, MUX stdlib
│
│   # arrives at v0.5+ (generative layer):
│   ├── liaozhai-text/               # description assembly, salience matching, fragment store
│
│   # arrives at v0.6+ (technique components):
│   ├── liaozhai-pingdian/           # named-technique component library
│
│   # arrives when ratatui builder UI is built (v0.3+ exploratory, v0.4+ real):
│   └── liaozhai-build/              # ratatui builder client; full Bevy + bevy_ratatui
```

Crate naming follows the `liaozhai-*` convention to keep the namespace consistent. The umbrella `liaozhai` crate has no `liaozhai-` prefix; it's the public top-level name.

## Crate responsibilities

The boundaries are deliberate. Each crate has a primary concern and a narrow public API.

`liaozhai-core` is the foundation crate — shared types like `WorldId`, `AccountId`, `ConnectionId`, the project-wide error enum, version metadata, common feature flags. Every other crate depends on it. Keeps `liaozhai-core` small; it's a coupling point.

`liaozhai-world` defines the ECS components, relationships, and systems that compose a Cell, a Thing, an Avatar. Knows nothing about networking or persistence. Server-side depends on this directly; the builder client also depends on it (for shared component definitions).

`liaozhai-net` handles protocol encoding/decoding, side-channel routing, the connection state machine. Knows nothing about specific worlds or accounts — it speaks bytes and structured frames.

`liaozhai-server` is the binary crate that wires `liaozhai-net`, `liaozhai-auth`, `liaozhai-worlds`, `liaozhai-cmd`, and `liaozhai-persist` into a runnable daemon. Owns `main.rs`, the tokio runtime setup, configuration loading.

`liaozhai-auth` owns account state — the SQLite schema for accounts, authentication primitives, role resolution. Doesn't know about specific worlds.

`liaozhai-worlds` owns the world registry, world lifecycle (start/stop/save), and cross-world channels. The `WorldHandle` lives here.

`liaozhai-cmd` owns the player and builder command sets, parsing, dispatch. Depends on `liaozhai-world` (to act on world state). The eventual Lykn-defined commands plug into this crate's dispatch surface.

`liaozhai-persist` owns serialization concerns: RON/JSON template formats, `bevy_save` snapshot integration, file layout, migration utilities.

Later additions (`liaozhai-script`, `liaozhai-text`, `liaozhai-pingdian`, `liaozhai-build`) plug into the existing surface area; they're added as their version arrives, not pre-emptively scaffolded.

## Dependencies and the bevy_ecs vs bevy split

Per ADR-0003, the **server-side crates** (`liaozhai-world`, `liaozhai-server`, etc.) depend on `bevy_ecs` only — not the full `bevy` engine. The builder client (`liaozhai-build`, when it lands) depends on full `bevy` plus `bevy_ratatui`.

`liaozhai-world` is the boundary crate: its component definitions must compile under both contexts. In practice this is trivial — components are plain Rust structs/enums with `bevy_ecs::Component` derives — but it's a constraint to keep in mind when adding component types.

## Versioning and publishing

The umbrella `liaozhai` crate is the published crate, currently at 0.1.0. Internal crates are unpublished workspace members initially. They may graduate to published crates as the project matures (especially `liaozhai-world` and `liaozhai-script` if reuse outside MUX becomes plausible).

Workspace-level `Cargo.toml` defines `[workspace.package]` with shared `version`, `edition`, `license`, `description`. Member crates inherit via `version.workspace = true`.

## Consequences

**Positive:**

- Compilation parallelism. Independent crates compile independently; touching `liaozhai-cmd` doesn't recompile `liaozhai-net`.
- Module boundaries are enforceable. Crate-level visibility (only `pub` items cross crate boundaries) forces interface design.
- Dependency surfaces stay narrow. The server doesn't pull in `bevy_ratatui`; the builder client doesn't pull in tokio if not needed.
- Future open-sourcing or reuse of individual crates is easier (e.g., publishing `liaozhai-world` for other text-world projects to use).

**Negative:**

- More crates means more `Cargo.toml` files to keep in sync.
- Cross-crate refactors are heavier than cross-module ones.
- Initial development sometimes wants tighter coupling than crate boundaries allow; we may need to reorganize once or twice as v0.1 work clarifies what actually wants to live where.

## Alternatives considered

- **Single crate with modules** — simplest. Rejected because the ECS-server vs full-Bevy-client split alone justifies separate crates, and compile times in a project this size benefit from crate-level parallelism.
- **Crate per logical feature (auth, worlds, cells, things…)** — too granular. Many small crates with cyclic-dependency risk. Rejected.
- **Mono-crate now, split later** — workable but creates a second migration. Rejected because the crate boundaries are foreseeable enough to draw them up front.

## Related

- ADR-0002 — Architecture Overview (system layers)
- ADR-0003 — `bevy_ecs` as the core data model (informs the server vs client split)
- ADR-0008 — Lykn as the soft-code language (`liaozhai-script` crate)
