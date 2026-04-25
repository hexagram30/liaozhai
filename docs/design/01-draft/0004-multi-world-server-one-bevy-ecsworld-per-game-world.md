---
number: 4
title: "Multi-world server: one `bevy_ecs::World` per game-world"
author: "running multiple"
component: All
tags: [change-me]
created: 2026-04-25
updated: 2026-04-25
state: Draft
supersedes: null
superseded-by: null
version: 1.0
---

# Multi-world server: one `bevy_ecs::World` per game-world

**Status:** Draft
**Date:** 2026-04-25
**Type:** ADR

## Context

PennMUSH and TinyMUSH (and most of their derivatives) host a single game-world per server process. One `netmush` runs one MUSH. Multi-world setups are achieved by running multiple processes, each with its own database, port, and config.

Liaozhai MUX is designed to host **multiple worlds in one server process**, with shared accounts across worlds. An admin connects, sees a list of worlds, picks one, enters as builder. A player connects, sees the same list, picks one, enters as player. Account identity is server-level; in-world identity (Avatar) is per-world.

This is closer to Evennia's "you can run multiple games on the same server" posture than to PennMUSH's "one server one world." It implies architectural choices about isolation, persistence, and concurrency.

The question: how is a world implemented in code, and how strictly is it isolated from other worlds running in the same process?

## Decision

**Each game-world is its own `bevy_ecs::World` instance.** No entity in world A can hold a reference to an entity in world B; queries in world A cannot see world B's components; ticks in world A and world B are independent.

The server holds a `WorldRegistry`, roughly:

```rust
struct WorldRegistry {
    worlds: HashMap<WorldId, WorldHandle>,
}

struct WorldHandle {
    id: WorldId,
    metadata: WorldMetadata,
    command_tx: mpsc::Sender<WorldCommand>,
    output_rx: broadcast::Receiver<WorldOutput>,
    // lifecycle controls: start, stop, save, etc.
}
```

A connection in the **InWorld** state holds a `WorldHandle` for its current world. Player input is forwarded to that world's command channel. Output the world produces for that connection comes back via a per-connection subscription on the output broadcast.

Cross-world communication is narrow and explicit: a server-level **channel** facility (the chat/comm layer, separate from any single world) lets accounts converse across worlds. No game-state references cross.

## Consequences

**Positive:**

- Isolation is total. Bugs in one world cannot corrupt another. Crashes in a world's tick can be caught and recovered without affecting other worlds.
- Worlds can tick at different rates. A slow-paced narrative world might tick every 500 ms; a faster-paced one every 50 ms.
- Worlds save and load independently. Backup and migration are per-world.
- Future horizontal scaling is plausible: a world could be moved to a different OS process or different machine without changing the world's code.
- Account identity vs. in-world identity is clean: one `Account`, many `Avatar`s, separation enforced by the architecture.

**Negative:**

- Cross-world features require explicit machinery — they can't be implemented by "just querying both worlds." This is the cost of the isolation guarantee.
- Per-world overhead is real: each world holds its own ECS state, its own task, its own command queue. Fifty empty worlds is fifty empty tasks. This is fine in practice but worth knowing.
- Builder tooling that wants to introspect or edit worlds must explicitly route through the world's command channel.

## Cross-world communication

The deliberately limited cross-world facilities are:

- **Server-level channels:** chat, OOC, system announcements. Account-scoped; no entity references.
- **World registry queries:** "list of worlds available to this account." Read-only metadata; no in-world state.
- **Account-level state:** preferences, friend lists, admin role. Stored at the server, accessible from any world's context.

## Alternatives considered

- **One `bevy_ecs::World`, with a `WorldId` component on every entity.** Simpler to implement; lets some queries span worlds with filters. Rejected because the isolation guarantee is the point — losing it makes per-world tick rates, independent saves, and future scaling much harder.
- **One server process per world (the PennMUSH model).** Simplest possible isolation. Rejected because the multi-world hosting feature is a designed goal, not an emergent need; running fifty processes to host fifty worlds is operationally heavier than one process holding fifty `World` instances.

## Related

- [0001 — Architecture overview](./0001-architecture-overview.md)
- [0002 — `bevy_ecs` as core data model](./0002-bevy-ecs-data-model.md)
- [0004 — Tokio + actor-model concurrency](./0004-tokio-actor-concurrency.md) (each world's tick is an actor)
