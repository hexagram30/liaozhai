---
number: 3
title: "`bevy_ecs` as the core data model"
author: "Bevy ECS"
component: All
tags: [change-me]
created: 2026-04-25
updated: 2026-04-25
state: Draft
supersedes: null
superseded-by: null
version: 1.0
---

# `bevy_ecs` as the core data model

**Status:** Draft
**Date:** 2026-04-25
**Type:** ADR

## Context

The world model needs a composable, performant, well-supported data structure. The original architectural research (`Architecting a Text World Engine: From MUD Heritage to Bevy ECS`) made the case in detail: ECS composition cleanly captures what MUSH attribute-keyed objects already approximate, what Caves of Qud's Parts system implements directly, and what GURPS advantages and Fate aspects model in tabletop terms.

The question is which ECS implementation, used how. The Rust ECS landscape includes:

- **`bevy_ecs`** — used standalone, the ECS crate from the Bevy game engine. Mature, fast, well-documented. Bevy 0.16's relationship system handles graph-shaped containment and adjacency natively. Active development, large user base. Can be used without the rest of Bevy.
- **Full `bevy`** — the engine. Includes rendering, audio, input, asset pipeline. Most of which a server doesn't need.
- **`hecs`** — minimal ECS. Smaller surface area, less feature-rich, no built-in relationships.
- **`legion`** — was a contender; effectively superseded by Bevy ECS in the Rust ecosystem.
- **`specs`** — older, less actively developed.

For the **server**, we need a fast, composable ECS without the rendering/audio/input apparatus of a full game engine. For the **TUI builder client** (a separate process — see ADR-0009), we want full Bevy because `bevy_ratatui` integrates cleanly and the Bevy schedule model fits a TUI's update loop.

## Decision

The Liaozhai MUX **server** uses `bevy_ecs` as a standalone dependency — not the full `bevy` engine.

The Liaozhai MUX **builder client** (a separate binary) uses full Bevy plus `bevy_ratatui` for its TUI.

Both share component definitions through a common crate (see ADR-0009), so a `Cell`, `Name`, or `Aspect` defined once is usable in either context.

## Consequences

**Positive:**

- The server's compile time, binary size, and dependency surface stay small. We're not paying for a renderer we'll never instantiate.
- Bevy's relationship system is available for `InCell`/`CellContents`-shaped data — first-class one-to-many relationships with automatic consistency.
- The component-as-Rust-struct model gives us full type safety and IDE support for world-state code.
- Migration to a future ECS is possible but unlikely; Bevy ECS has the most ecosystem momentum.

**Negative:**

- Bevy's schedule model and system function signatures are idiosyncratic; new contributors need to learn them.
- Some Bevy features (asset hot-reloading, scene serialization) are tied to the full engine; if we want them, we either reach for the relevant crate independently or live without.
- Bevy's API has historically broken across minor versions; we'll need to track upstream and budget for migration work.

**Neutral:**

- The crate split (server uses `bevy_ecs`, client uses `bevy`) means component definitions in the shared crate must compile under both. This is a trivial constraint in practice but worth knowing.

## Alternatives considered

- **`hecs`** — too minimal; we'd reinvent the relationship system Bevy already provides.
- **Roll our own** — explicitly rejected. ECS is solved; spending engineering on a custom one would be churn for no differentiation.
- **Use full `bevy` everywhere** — pulls in rendering and audio crates the server doesn't need. Compile time and binary size penalty without benefit.

## Related

- ADR-0002 — Architecture Overview
- ADR-0004 — Multi-world server: one `bevy_ecs::World` per game-world
- ADR-0009 — Workspace crate layout
