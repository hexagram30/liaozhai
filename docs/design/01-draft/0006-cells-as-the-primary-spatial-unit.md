---
number: 6
title: "\"Cells\" as the primary spatial unit"
author: "prior tooling"
component: All
tags: [change-me]
created: 2026-04-25
updated: 2026-04-25
state: Draft
supersedes: null
superseded-by: null
version: 1.0
---

# "Cells" as the primary spatial unit

**Status:** Draft
**Date:** 2026-04-25
**Type:** ADR

## Context

The MUD/MUSH tradition has a single dominant term for "the place an entity is in": **room**. A DikuMUD has rooms, a PennMUSH has rooms, a LambdaMOO has rooms, an Evennia has rooms. The term carries clear semantics — a bounded interior space with descriptive text, exits to other rooms, things and characters inside.

The term is also constraining. A "room" connotes architecture: walls, ceiling, doors. It strains when the space is a forest clearing, a stretch of road, the surface of a pond, a moment of weather, a stanza of a poem, a dream. Existing MUSHes accept the strain ("you wouldn't *call* it a room, but it's a room") because consistency with prior tooling matters more than vocabulary precision.

Liaozhai MUX is unconstrained by prior tooling (see ADR-0007). It's also a project that explicitly wants to support spaces that are not rooms — the *píngdiǎn* commentary tradition treats narrative as a tree of nested techniques, some of which are spatial in only the most metaphorical sense, and the project's eventual generative layer should be able to talk about a "moment of dusk in the studio" as a place an Avatar can be.

A second consideration: Lykn — the project's chosen soft-code language (ADR-0008) — already uses **`cell`** as its mutation primitive. A Lykn `cell` is a place where state can change. Naming the project's spatial primitive **Cell** creates a deliberate pun: in MUX, a Cell is a place; in Lykn, a cell is a place state changes; soft-code authored in Lykn manipulates MUX Cells via Lykn cells. The terminology compresses cleanly and the resonance is genuine, not forced.

## Decision

**Cell** is the primary spatial unit in Liaozhai MUX.

A Cell is an entity in a world's ECS that carries a `Cell` marker component. Cells can be connected to other Cells via Exits (themselves entities, with their own attributes). Things and Avatars are "in" Cells via Bevy relationship components (`InCell { cell: Entity }` paired with `CellContents(Vec<Entity>)`).

The term *room* is **not** banned from user-visible text. A particular Cell can be called "The Studio" or "the parlor" in its display name; description text can refer to "the room" naturally. The term *Cell* is for the engine, the API, the soft-code, the ADRs, and the developer documentation. User-facing text uses whatever term fits the fiction.

## Consequences

**Positive:**

- Spaces that aren't rooms get the same first-class treatment without category friction. A Cell can be a forest clearing, a moment, a poem, a dream.
- The Lykn pun establishes a stable association between language primitive and engine primitive. Builders thinking in Lykn ("this `cell` is the player's location") get a coherent mental model.
- The term is short, neutral, and Google-able — a search for "MUSH room" returns thirty years of tradition we'd be referencing; a search for "Liaozhai MUX Cell" is unambiguous.
- The biological metaphor (cells, organisms, organs) is available if/when we want to talk about composition: a world is composed of cells, a cell is composed of components, etc.

**Negative:**

- New terminology is a tax on documentation, on contributor onboarding, and on community translation work. Every "what's a Cell?" question costs.
- The term has prior associations — prison cells, biological cells, spreadsheet cells — that may distract or mislead.
- Search collisions with the Lykn `cell` primitive will need disambiguation in technical contexts. We'll use **Cell** (capitalized, MUX) and **cell** (lowercase, Lykn) consistently.

## Capitalization conventions

To keep the disambiguation tractable:

- **Cell** (capitalized) — the MUX spatial entity. Used in prose, ADRs, code comments referring to the engine concept, and user-visible text where the engine concept is named.
- `Cell` (code formatting) — the Rust component or type.
- **cell** (lowercase, in italics or code where context demands) — the Lykn mutation primitive.
- "rooms," "places," "spaces" — fine in user-facing prose where the engine concept isn't being directly named. The fiction takes priority over the API.

## Alternatives considered

- **Room** — the dominant tradition. Clear, immediately understood by anyone with MUSH experience. Rejected because the connotation is too constraining and we already have license to choose freshly.
- **Place** — neutral, broad, but generic to the point of carrying no flavor. Hard to talk about specifically ("a place is a place").
- **Node** — accurate (Cells are graph nodes) but deeply technical; loses the inhabited quality.
- **Locale** — workable but quietly geographical; pulls toward "outdoor area."
- **Zone** — already taken by Fate (and by Caves of Qud, used differently). Conflicts.

## Related

- ADR-0002 — Architecture Overview (Cell concept introduced)
- ADR-0008 — Lykn as the soft-code language (Lykn `cell` primitive)
