---
number: 7
title: "Inspired by, not bound by, prior MUSH/MUD systems"
author: "trusted users"
component: All
tags: [change-me]
created: 2026-04-25
updated: 2026-04-25
state: Draft
supersedes: null
superseded-by: null
version: 1.0
---

# Inspired by, not bound by, prior MUSH/MUD systems

**Status:** Draft
**Date:** 2026-04-25
**Type:** ADR

## Context

Liaozhai MUX descends conceptually from a long lineage: DikuMUD, LPMud, LambdaMOO, TinyMUSH, PennMUSH, and Evennia. The original architectural research treats this lineage seriously and extracts what's load-bearing from each — the attribute-keyed object model from MUSH, the prototype inheritance from LPMud, the typeclass dynamism from Evennia, the soft-code extensibility tradition that runs through all of them.

There's a fork in the road for any project that draws on this lineage: aim for **source compatibility** (load existing PennMUSH databases, run existing MUSHcode unchanged, accept the constraints that come with that) versus **conceptual compatibility** (preserve the patterns that work, modernize freely, accept that existing content doesn't port directly).

Source compatibility has clear appeal — it offers a migration path for existing MUSHes, validates the project to the existing community, and inherits decades of tested behavior. It also imposes substantial costs: 30-year-old idioms and idiosyncrasies become engineering constraints. MUSHcode's string-based programming model, dbref recycling semantics, attribute-tree backtick parsing, the lock language's specific operator precedence — every one of these is a decision someone made for a different era's tools, and source compatibility means honoring every one. The result tends to be a project that ports the past faithfully and innovates carefully around the edges.

Liaozhai MUX is going somewhere the prior tradition didn't aim. The procedural-narrative layer (salience-based assembly, *píngdiǎn* technique components, fabula/discourse separation) is genuinely new. The multi-world hosting model is closer to Evennia's posture than to PennMUSH's. The soft-code language is Lykn — a typed, immutable, Lisp-flavored language with a Rust toolchain — not MUSHcode. We have access to Rust's type system, modern ECS, contemporary async I/O, and decades of subsequent design research. The cost-benefit of source compatibility tilts the wrong way.

## Decision

**Liaozhai MUX is inspired by, not bound by, prior MUSH/MUD systems.** No goals around source compatibility. No commitment to PennMUSH database format, MUSHcode, dbref idioms, or any specific lock-expression syntax. The lineage is intellectual and aesthetic, not operational.

What we **do** preserve from the lineage:

- The **builder culture** — the idea that worlds grow through live, in-situ construction by trusted users, not through compile-redeploy cycles.
- The **soft-code tradition** — that game logic can be authored without restarting the server, and that authoring is a separate discipline from engine development. Implemented via Lykn rather than MUSHcode (see ADR-0007).
- The **attribute model conceptually** — entities carry arbitrary named data accessible to soft-code. Implemented as ECS components and Lykn-accessible attributes rather than MUSHcode's string-keyed map.
- The **lock pattern** — predicate-based access control on actions, attributes, and entry. Implemented as Lykn expressions rather than MUSHcode lock-language.
- The **room/exit graph topology** — places connected by named transitions. Implemented as Cells and Exits with the graph stored in `petgraph::StableGraph` (see ADR-0001 for the data model).
- The **separation of templates from instances** — designed content (Cell archetypes, Thing blueprints) versus runtime state (who's where, what's on fire). Implemented per ADR-0001's persistence section.

What we **do not** preserve:

- MUSHcode as a language. Lykn replaces it.
- Dbref syntax (`#1234`) as the canonical entity reference. Internally, entities are `bevy_ecs::Entity`; externally, they have human-readable names and machine-stable UUIDs.
- The specific PennMUSH/TinyMUSH command syntax (`@create`, `@dig`, `@open`, etc.). Liaozhai MUX commands are designed fresh; if some happen to match the tradition because the tradition got them right, fine, but no commitment.
- Database file formats. Persistence is `bevy_save` for instances, RON/JSON for templates, SQLite for accounts.
- The four-category type system (rooms / exits / things / players). Replaced by ECS composition; an entity is what its components say it is.

## Consequences

**Positive:**

- Architectural freedom. Decisions are made on their merits in the modern Rust + ECS context, not relative to PennMUSH's compatibility surface.
- The codebase doesn't carry decades of "this is here for compatibility" baggage.
- Soft-code, world model, persistence, and protocol can all be designed coherently rather than glued onto a legacy substrate.
- The eventual generative layer (description assembly, *píngdiǎn* components) doesn't have to fight legacy assumptions about what a "room description" is.

**Negative:**

- No migration path for existing MUSHes. Anyone with an existing PennMUSH/TinyMUSH world cannot move it to Liaozhai MUX without manual rework. This is a real cost; it eliminates a possible community on-ramp.
- We give up "instant familiarity" for users coming from existing MUSHes. New terminology, new commands, new soft-code language. Documentation and onboarding must do more work.
- Some patterns may need re-discovery. Three decades of MUSH community knowledge encoded "what works" — we'll learn some of it the hard way by having freedom to do otherwise.

## Mitigation

To make the lineage intelligible to people arriving from existing MUSHes, the documentation will include explicit "if you're coming from PennMUSH" mappings — a glossary that translates concepts (dbref → entity ID, MUSHcode attribute → Lykn-accessible attribute, etc.). This isn't compatibility; it's translation. It costs us a documentation chapter, not an engineering compromise.

## Alternatives considered

- **Source-compatible (load existing PennMUSH databases)** — rejected. The compatibility cost is high, the pull-through value low for a project aiming at procedural narrative. Existing MUSHes have many fewer than the project's eventual reach.
- **Pattern-compatible (same model, modern syntax)** — was a strong candidate. Rejected when Lykn-as-soft-code was selected; "modern syntax with PennMUSH's data model" is incoherent if soft-code is a typed Lisp.

## Related

- [0001 — Architecture overview](./0001-architecture-overview.md)
- [0007 — Lykn as soft-code language](./0007-lykn-soft-code-language.md) (the most consequential break from the lineage)
