---
number: 2
title: "Architecture Overview"
author: "source compatibility"
component: All
tags: [change-me]
created: 2026-04-25
updated: 2026-04-25
state: Draft
supersedes: null
superseded-by: null
version: 1.0
---

# Architecture Overview

**Status:** Draft
**Date:** 2026-04-25
**Type:** Architecture overview (synthesizes ADRs 0002–0008)

## Vision

Liaozhai MUX is a multi-user text-world server in the spiritual lineage of PennMUSH and TinyMUSH, written in Rust, designed to evolve into a procedural literary fiction engine grounded in the Chinese *píngdiǎn* (評點) commentary tradition. The first phase of work is deliberately mundane: a server that admins can connect to, browse worlds in, and build cells inside; that players can connect to, enter worlds, and explore through prose. The generative and exegetical layers — the things that make the project *Liaozhai* and not just another MUSH — arrive later, layered onto a foundation that is correct, simple, and runnable first.

The project's animating tension is between two things that are usually treated as separate concerns: the social-extensibility tradition of MUSHes (live worlds, live coding, builder cultures) and the procedural-narrative tradition of modern interactive fiction (salience-matched description, named technique components, fabula/discourse separation). The architecture's job is to make these compose rather than collide.

## Principles

The design is governed by a small number of principles, in tension where they have to be:

**Basics first, magic later.** A v0.1 that lets an admin create a cell and a player walk into it is more valuable than a v1.0 that compiles but doesn't run. The generative layer is layered onto a working substrate, not bolted into an unfinished one.

**Inspired by, not bound by.** The project draws heavily on PennMUSH/TinyMUSH/LambdaMOO/Evennia *as inspiration*. We are not constrained by source compatibility, attribute syntax, dbref idioms, or any other legacy. Where Rust gives us a better tool — sum types, lifetimes, ownership, the type system — we use it. Where modern ECS gives us a better data model than 30-year-old C structs, we use it. (See ADR-0006.)

**ECS as the substrate, not the surface.** The world model is `bevy_ecs` underneath, but the surface presented to builders and to soft-code is MUSH-shaped: things in cells, attributes on things, locks on actions. The ECS provides composition; the surface provides familiarity. (See ADR-0002.)

**One world per `bevy_ecs::World`.** Multi-world hosting is a first-class feature, not a retrofit. Each game-world is its own ECS world with its own entities, its own tick, its own builders and players. The server is a registry of worlds plus the connections between them and the outside. (See ADR-0003.)

**Tokio for I/O, message-passing for everything else.** The server is fundamentally I/O-bound; tokio handles the network. Worlds run their ticks on dedicated tasks. Connections speak to worlds through channels. No shared mutable state across world boundaries. (See ADR-0004.)

**Soft-code is Lykn.** The eventual scripting layer is Duncan's Lykn — a typed, immutable Lisp-flavored language with a Rust toolchain and a macro system. The language is structurally aligned with what we're building (`cell` as primitive, S-expressions as exegetical trees, macros as named lenses) and the project owns the language. (See ADR-0007.)

**Description assembly is salience-based.** When the generative layer arrives, it follows the Valve/Inform 7/Emily Short lineage: tagged fragments, most-specific-match selection, "mentioned" tracking to prevent redundant listing, fall back gracefully. *Píngdiǎn*-named technique components (草蛇灰線, 烘雲托月, etc.) become first-class types in this layer. This is v0.5+ work, not v0.1.

## System layers

```
┌──────────────────────────────────────────────────────────────────┐
│  Clients: telnet, websocket, ratatui builder, web (later)        │
└──────────────────────────────────────────────────────────────────┘
                               │
                               │  text + structured side-channel
                               ▼
┌──────────────────────────────────────────────────────────────────┐
│  Network layer: listeners, sessions, codec, side-channel routing │
└──────────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────┐
│  Auth + World registry: accounts, login, world catalog, role     │
│  resolution, world entry/exit                                    │
└──────────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────┐
│  Per-world domain (one of these per running world):              │
│   ┌────────────────┐   ┌─────────────────┐   ┌────────────────┐  │
│   │ Command parse  │ ► │ ECS world       │ ► │ Description    │  │
│   │ + dispatch     │   │ (entities,      │   │ assembly       │  │
│   │                │   │  components,    │   │ (v0.5+:        │  │
│   │                │   │  systems, tick) │   │  salience)     │  │
│   └────────────────┘   └─────────────────┘   └────────────────┘  │
│                               │                                  │
│                               ▼                                  │
│                       Soft-code runtime                          │
│                       (v0.4+: Lykn interpreter)                  │
└──────────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────┐
│  Persistence: templates (RON/JSON), instances (bevy_save),       │
│  account state (SQLite)                                          │
└──────────────────────────────────────────────────────────────────┘
```

## Core concepts

The following terms have specific, load-bearing meanings throughout the project. Stable vocabulary matters; avoid synonyms.

A **Server** is the running daemon — a single OS process that listens on configured network endpoints, manages accounts, hosts one or more worlds, and routes connections between clients and worlds. Roughly analogous to a PennMUSH `netmush` process, but multi-world.

A **World** is a self-contained game-world hosted by the server. Each world has its own ECS state, its own ticking simulation, its own builders, its own players, its own persisted data. Players in different worlds cannot interact directly; an account can be a builder in one world and a player in another. (See ADR-0003.)

A **Cell** is the primary spatial unit within a world — the equivalent of a "room" in classic MUDs but more neutral in connotation. A Cell can be a room, a clearing, a dream, a moment of weather, a stanza. Cells are connected to other Cells by **Exits**, which carry their own attributes (direction, lock, description). The term `Cell` is also a deliberate echo of Lykn's `cell` mutation primitive — a MUX Cell is a place, a Lykn cell is a place where state can change. (See ADR-0005.)

A **Thing** is anything in a world that isn't a Cell or an Exit — objects, NPCs, characters, abstract entities like factions or weather systems. The MUSH tradition makes a hard categorical distinction between rooms/exits/things/players; we soften this. A Thing is a bundle of components, and what kind of Thing it is is determined by which components it carries.

An **Account** is a persistent user identity at the server level — login credentials, contact info, server-wide preferences. Accounts persist across worlds. An Account can have a different **Avatar** in each world it's joined.

An **Avatar** (working term — open to revision) is the in-world representation of an Account in a particular World. A single account can have one Avatar per world, with different names, descriptions, and roles in each.

A **Connection** is a live network session. A connection has a state machine: pre-auth → authenticated → world-selection → in-world. A connection in the in-world state is bound to a specific (Account, World, Avatar) tuple.

A **Builder** is an Avatar that holds construction privileges in a world — can create Cells, edit descriptions, set attributes, define Exits. The MUSH tradition calls these "wizards" or "gods." Liaozhai MUX uses Builder for the role; the term "wizard" can stay as flavor in user-visible text where appropriate.

A **Component** (in the ECS sense) is a piece of data attached to an entity. Cells, Things, Avatars, and Exits are all entities; they carry components like `Name`, `Description`, `Locked`, `OnFire`, `Hungry`. Components compose freely — a Thing with `Lit` and `Hot` is on fire whether it's a candle or a building.

An **Aspect** (Fate-style; open to revision) is a free-form text descriptor with mechanical weight — "Burning Kitchen" attached to a Cell, "Slightly Drunk" attached to an Avatar. Aspects are simultaneously narrative descriptors and mechanical hooks. They're a specific shape of component.

## Data model

The world model uses `bevy_ecs` (the ECS crate from the Bevy engine, used standalone — see ADR-0002). Entities are opaque IDs; components are typed Rust structs; relationships are first-class via Bevy's relationship system.

Spatial topology lives in two layers. The high-level graph — which cells connect to which — uses `petgraph::StableGraph` for traversal, pathfinding, and serialization. Stable indices survive node removal, which matters when builders delete cells. The fine-grained "what's in this cell right now" relationships use Bevy's relationship components (`InCell { cell: Entity }` paired with `CellContents(Vec<Entity>)`), which the ECS keeps consistent automatically.

A worked example: a Cell named "The Studio at Dusk" might carry components like:

```rust
Cell                              // marker, this is a cell
Name("The Studio at Dusk")
ShortDescription("a small, lamplit room")
LookDescription(/* longer prose */)
TimeOfDay(Hour(19.5))
Weather(Clear)
LampLit                            // marker
Indoors                            // marker
Aspect("Smell of ink and pine")    // free-form descriptor
RegionMember { region: ...id... }  // relationship
```

A new Cell type isn't a new Rust type. It's a new combination of components, possibly with a few new components added to the library. A "haunted clearing" is `Cell + Outdoors + Forest + Aspect("the air remembers something")`. A "burning library" is `Cell + Indoors + Library + OnFire + Aspect("smoke pooling under the rafters")`. The data model adapts to new fictional needs without engine code changes — as long as the description-assembly layer can read the components that determine what to say.

## Network architecture

The server listens on at minimum two protocols: telnet (with structured side-channel, GMCP-inspired) for traditional MUSH clients and tooling, and WebSocket for browser and modern client integration. Both speak the same logical protocol underneath: a text channel for prose output and player input, plus a JSON side-channel for structured updates (room metadata, prompts, status data) that clients render as they choose.

A connection's lifecycle progresses through states: **Connecting** → **Authenticating** → **WorldSelection** → **InWorld**. Each state accepts a different command set. In **InWorld**, the connection is bound to a specific (Account, World, Avatar), and the connection's input is dispatched to that world's command handler.

Multiple client types are first-class: a **terminal text client** (telnet, basic prose), a **ratatui builder client** (rich TUI for admins/builders, separate process), and **future web/native clients**. All speak the same protocol; differences are in rendering, not transport.

## Multi-world isolation

Each game-world is an independent `bevy_ecs::World`. The server holds a `WorldRegistry: HashMap<WorldId, WorldHandle>`, where each `WorldHandle` exposes a command channel, an output broadcast, and lifecycle controls (start, stop, save). Connections in the **InWorld** state hold a reference to a specific `WorldHandle` and route their input through it. (See ADR-0003.)

Cross-world communication is intentionally constrained — a server-level **channel** facility (separate from any single world) lets accounts chat across worlds, but in-world entities cannot reference entities in other worlds. This isolation is what lets us tick worlds at different rates, save them independently, and (later) host them on different machines.

## Concurrency model

The server uses tokio as its async runtime. Each accepted connection becomes its own async task. Each world's tick runs on its own dedicated task, processing a command queue and advancing simulation on a fixed schedule (default 100 ms; configurable per world). Cross-task communication is via tokio channels — connection tasks send commands to world tasks, world tasks send output to connection tasks (and to broadcast channels for room-wide messages). No shared mutable state crosses task boundaries; the model is actor-shaped throughout. (See ADR-0004.)

## Soft-code: Lykn

The soft-code layer is Lykn — Duncan's typed, immutable, S-expression Rust-toolchained language. The fit is structurally deep (Lykn's `cell` primitive, the lisp-as-tree alignment with *píngdiǎn* commentary, macros as named exegetical lenses) and the project owns the language. (See ADR-0007 for the full case.)

Embedding strategy starts with a tree-walking interpreter in Rust that consumes Lykn's AST after macro expansion to kernel forms. The interpreter is deliberately simple — performance demands are low (sub-100ms response to player actions is luxurious) and hot-reload, sandboxing, and ECS integration matter more than execution speed. If/when a use case emerges that needs more performance, Lykn gains a bytecode target and MUX gains a bytecode VM. The interpreter ships in v0.4 of MUX; the VM is later.

Two-way evolution is explicit: Lykn improves to support MUX use cases (kernel form stabilization, AST export API, Rust runtime hooks); MUX uses Lykn idioms where they fit (Cells as Lykn cells, the `define-cell` / `before-enter` / `lock-predicate` macro family).

## Description assembly (v0.5+)

The generative-fiction layer is the project's eventual differentiator and is deferred to v0.5+. Its architecture follows the Valve/Inform 7/Emily Short lineage: description fragments are tagged with predicates over component state, the most-specifically-matching fragment is selected at render time, and an Inform 7-style "mentioned" flag prevents the same entity from being listed twice across nested descriptions. Fragments compose by layering — a Cell's base description, plus weather overlay, plus time-of-day overlay, plus current-event overlay, all assembled per-look.

*Píngdiǎn* named techniques (草蛇灰線 *cǎoshé huīxiàn* foreshadowing, 烘雲托月 *hōngyún tuōyuè* peripheral description, 橫雲斷山 *héngyún duànshān* scene transition, others) become typed components in this layer. Builders attach them to Cells, Things, and event triggers; the description-assembly system reads them and adjusts what gets said when. This is the layer that earns the project's name.

Everything in this section is directional. Concrete decisions get their own ADRs as we approach v0.5.

## Persistence

Two stores, two tempos. **Templates** — Cell archetypes, Thing blueprints, base description corpora, lock-predicate libraries — live as RON or JSON files, version-controlled with the world they belong to, loaded at world startup. **Instances** — actual entities with current state, who's where, what's on fire — are saved via `bevy_save` snapshots on a configurable cadence and on graceful shutdown. **Account state** (login credentials, server-wide preferences, cross-world identity) lives in SQLite at the server level, separate from any single world.

This split mirrors the zone-file/player-file separation that every successful MUD shipped. Templates are designable, diffable, and reviewable; instances are operational state. Both speak Rust's `serde` ecosystem, so format choice is reversible.

## Iteration roadmap

Each version below should be runnable, tested, and demonstrably useful before the next begins. Versions are scope, not calendar.

**v0.1 — Connect, authenticate, list worlds.** Server accepts telnet connections, drives the connect → authenticate → world-selection flow, lists worlds from a hardcoded registry. A successful "world selection" prints "you would now be in world X" and disconnects. No actual in-world simulation.

**v0.2 — Enter a world, look around.** Per-world ECS state is real. A demo world hardcodes one Cell with a description. Entering the world places the avatar in the Cell and fires `look`. A `quit` command returns to world selection. No movement yet.

**v0.3 — Build cells, walk between them.** Builder commands let an admin create Cells, set their descriptions, and connect them via Exits. Player commands include `look`, `go <direction>`, and `who`. World state is persisted between server restarts. This is the first version that meets Duncan's "admin builds, player explores" definition.

**v0.4 — Things, attributes, basic interactivity, Lykn integration.** Things exist in Cells and inventories. Attributes can be set on Cells and Things. The Lykn AST-walker interpreter ships; the first soft-code layer (basic triggers — `on-enter`, `on-look`, `on-touch`) is Lykn-authored. Locks become Lykn predicates.

**v0.5 — Salience-based description assembly.** Description fragments gain predicate tags; the salience-matching system selects fragments at render time. Cells and Things grow from "single description string" to "stack of tagged fragments." Inform 7-style "mentioned" tracking lands. The first weather/time/lighting overlays demonstrate the system end-to-end.

**v0.6+ — *Píngdiǎn* technique components.** Named-technique components ship as a starter library. Foreshadowing, peripheral description, parallel structure all become attachable to entities and recognized by the description system. This is when the project's title earns its weight.

**v1.0 and beyond.** Stability, performance work, web client, broader builder tooling, distributed/multi-server deployment if desired. Open-ended.

## Open questions

Several questions are deliberately unresolved at this stage; each will be answered by a future ADR when the answer materially affects ongoing work.

The Lykn embedding strategy beyond the initial AST walker — when do we need a bytecode VM, what does the Rust hosting API look like — gets a future ADR (likely 0009) when v0.4 work begins. The wire protocol details — exact GMCP-style namespaces, side-channel payload schemas — get an ADR when v0.1 implementation is concrete enough to pin them down. The persistence cadence and snapshotting strategy gets an ADR when v0.3 work begins. The exact list of `píngdiǎn` techniques shipped in v0.6 gets an ADR (or possibly several) as the description-assembly system stabilizes and we know what shapes are useful.

What we are *not* going to revisit casually: the choice of Rust + bevy_ecs (ADR-0002), the multi-world architecture (ADR-0003), the actor-model concurrency (ADR-0004), the inspired-by stance toward prior MUSHes (ADR-0006), or the Lykn-as-soft-code commitment (ADR-0007). Those are foundational; revisiting them is a major project event.

## Related decisions

| ADR | Topic |
|-----|-------|
| 0002 | `bevy_ecs` (not full Bevy) as the core data model |
| 0003 | Multi-world server: one ECS world per game-world |
| 0004 | Tokio + actor-model concurrency |
| 0005 | "Cells" as the primary spatial unit |
| 0006 | Inspired by, not bound by, prior MUSH/MUD systems |
| 0007 | Lykn as the soft-code language |
| 0008 | Workspace crate layout |
