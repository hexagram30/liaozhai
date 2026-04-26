---
number: 10
title: "Graph topology and query architecture"
author: "their nature"
component: All
tags: [change-me]
created: 2026-04-25
updated: 2026-04-25
state: Draft
supersedes: null
superseded-by: null
version: 1.0
---

# Graph topology and query architecture

**Status:** Draft
**Date:** 2026-04-25
**Type:** ADR
**Related ADRs:** 0002 (Architecture overview), 0003 (`bevy_ecs` data model), 0004 (Multi-world server), 0006 (Cells)

## Context

Liaozhai MUX worlds are graphs by their nature: Cells are nodes, Exits are edges. Many of the operations a MUSH wants are graph-theoretic: shortest path between Cells, reachability checks, articulation points (chokepoints whose removal disconnects regions), connected component analysis, distance-bounded traversal for event propagation. These are useful for players (auto-routing, "how far is the library"), for builders (connectivity validation, orphan detection, "if I delete this Cell, what becomes unreachable"), for game internals (NPC pathfinding, sound/scent/light propagation, visibility scoping), and for the future generative-text layer (distance-aware description, region-level overlays, path-aware narration).

ADR-0002 sketched a two-layer model — `petgraph::StableGraph` for topology, Bevy ECS for entity data — but did not commit to how the two stay consistent or what API surface is exposed. The design space, on inspection, has three real shapes:

**A. Petgraph as canonical store, ECS as data layer, with sync.** Two stores hold different aspects of the same truth; a sync layer keeps them aligned on every Cell or Exit creation/deletion. Inherits petgraph's tested algorithms but introduces a class of consistency bugs and a non-trivial sync surface to maintain.

**B. ECS as canonical store, hand-rolled algorithms over component queries.** Single source of truth, no sync, but reimplements a small library of well-known graph algorithms (BFS, DFS, Dijkstra, articulation points, connected components, SCC) on top of Bevy's query API. Algorithms are well-documented; the cost is that we maintain them.

**C. ECS as canonical store, petgraph's algorithms reused unchanged via trait implementations.** A wrapper type implements petgraph's graph traits (`GraphBase`, `IntoNeighbors`, `Visitable`, etc.) over a Bevy `SystemParam` that holds the relevant queries. Petgraph's algorithm catalog runs against that wrapper exactly as it would against `StableGraph`. Single source of truth; no sync; no reimplementation; we use Rust's trait system for what it was designed for.

Approach C is what petgraph's design supports explicitly. The library exposes algorithms generic over traits precisely so different graph implementations can plug in. Bevy 0.16's relationship system, separately, provides automatically-maintained one-to-many entity links — exactly what an "outgoing exits" or "incoming exits" index needs. The two systems compose cleanly: Bevy maintains the adjacency lists; petgraph traits expose them as graph operations; algorithms run.

## Decision

**Liaozhai MUX adopts Approach C: ECS-canonical with petgraph trait implementations over a Bevy SystemParam wrapper.**

There is no separate `petgraph::StableGraph` instance per world. Topology lives entirely in the ECS — Cells and Exits are entities; connectivity is encoded by relationship components. A `WorldGraph` `SystemParam` (with read-only access to the relevant queries) implements the petgraph traits that algorithms require. `petgraph::algo::*` operations run directly against `WorldGraph`.

### Component model

The topology is encoded by four component types and their automatic Bevy 0.16 relationship maintenance:

```rust
/// Marker on every Cell entity.
#[derive(Component)]
struct CellTag;

/// On every Exit entity. Identifies the source Cell.
#[derive(Component)]
#[relationship(relationship_target = OutgoingExits)]
struct ExitFrom(pub Entity);

/// On every Exit entity. Identifies the destination Cell.
#[derive(Component)]
#[relationship(relationship_target = IncomingExits)]
struct ExitTo(pub Entity);

/// On every Cell entity. Auto-maintained list of outgoing Exit entities.
/// Bevy populates this when an Exit is spawned with ExitFrom(this_cell).
#[derive(Component)]
#[relationship_target(relationship = ExitFrom)]
struct OutgoingExits(Vec<Entity>);

/// On every Cell entity. Auto-maintained list of incoming Exit entities.
#[derive(Component)]
#[relationship_target(relationship = ExitTo)]
struct IncomingExits(Vec<Entity>);

/// On every Exit entity. The graph-edge data.
#[derive(Component)]
struct Connects {
    pub from: Entity,
    pub to: Entity,
}

/// Optional cost on an Exit. Absence means weight = 1.
#[derive(Component)]
struct TravelCost(pub f32);
```

When a builder creates an Exit with `ExitFrom(source_cell)` and `ExitTo(dest_cell)`, Bevy automatically appends the Exit's `Entity` to `source_cell.OutgoingExits` and `dest_cell.IncomingExits`. When the Exit despawns, Bevy removes it from both. The adjacency lists are structurally consistent; we never write sync code.

### The `WorldGraph` SystemParam

A read-only graph view over the world's ECS:

```rust
#[derive(SystemParam)]
struct WorldGraph<'w, 's> {
    cells:    Query<'w, 's, Entity, With<CellTag>>,
    outgoing: Query<'w, 's, &'static OutgoingExits>,
    incoming: Query<'w, 's, &'static IncomingExits>,
    exits:    Query<'w, 's, (&'static Connects, Option<&'static TravelCost>)>,
}

impl<'w, 's> WorldGraph<'w, 's> {
    pub fn neighbors(&self, cell: Entity) -> impl Iterator<Item = Entity> + '_ {
        self.outgoing
            .get(cell)
            .into_iter()
            .flat_map(|outs| outs.iter().copied())
            .filter_map(|exit| self.exits.get(exit).ok().map(|(c, _)| c.to))
    }

    // ... incoming(), edge_weight(), etc.
}
```

The `SystemParam` derive lets Bevy inject the right queries into any system that needs graph operations. Multiple systems can hold concurrent `WorldGraph` views because all queries are read-only.

A separate `WorldGraphMut` `SystemParam` exists for mutating operations (creating/deleting Cells and Exits); it requires exclusive write access and is held only by topology-mutating systems.

### Petgraph traits implemented

We implement the minimum trait set needed for our chosen algorithm catalog:

| Trait | Purpose | Required for |
|-------|---------|--------------|
| `GraphBase` | Defines `NodeId = Entity`, `EdgeId = Entity` | All algorithms |
| `Visitable` | Visited-set tracking via `HashSet<Entity>` | BFS, DFS, Dijkstra, articulation points |
| `IntoNeighbors` | Forward-adjacency iteration | BFS, DFS, reachability |
| `IntoNeighborsDirected` | Directional adjacency (incoming/outgoing) | SCC, directional traversals |
| `IntoEdges` | Edge iteration with weights | Dijkstra, A*, weighted algorithms |
| `IntoEdgeReferences` | Edge iteration with full references | All-pairs algorithms |
| `Data` | Exposes node/edge weight types | Weighted algorithms |
| `NodeIndexable` | Node-to-usize for matrix algorithms | Floyd-Warshall, some clustering |
| `IntoNodeIdentifiers` | Iterate all nodes | Whole-graph algorithms (SCC, components) |
| `NodeCount` / `EdgeCount` | Counts | Density, capacity hints |

The implementations are mechanical — each is 5–20 lines of Rust delegating to the appropriate Bevy query.

## Query API catalog

Algorithms are organized by who calls them and at what permission level. The list below is the v0.3+ commitment; not all need to land at once.

### Pathfinding

`shortest_path_unweighted(from, to) -> Option<Path>` — BFS-based; step-counted distance. Available to all roles.

`shortest_path(from, to) -> Option<Path>` — Dijkstra over `TravelCost`; falls back to step-counted if no costs are set. Available to builders by default; player visibility configurable per-world.

`shortest_path_with(from, to, predicate) -> Option<Path>` — Dijkstra with a predicate that excludes edges or Cells (e.g., locked exits, hostile Cells).

`a_star(from, to, heuristic) -> Option<Path>` — heuristic-guided pathfinding for large worlds where Dijkstra is wasteful.

`paths_within(from, max_cost) -> Iterator<(Entity, f32)>` — all Cells reachable within a bounded cost, with their distances. Used by event propagation, NPC perception, "Cells within N steps."

### Reachability

`reachable(from, to) -> bool` — boolean answer; faster than computing the full path.

`reachable_set(from) -> HashSet<Entity>` — all Cells reachable from a starting Cell. Useful for connectivity validation.

`reachable_within(from, max_steps) -> HashSet<Entity>` — depth-bounded reachable set.

### Topology analysis (builder/admin operations)

`articulation_points() -> Vec<Entity>` — Cells whose removal would disconnect the graph. The "if I delete this, what breaks" query.

`bridges() -> Vec<Entity>` — Exits whose removal would disconnect the graph.

`connected_components() -> Vec<Vec<Entity>>` — undirected component decomposition. Identifies isolated regions.

`strongly_connected_components() -> Vec<Vec<Entity>>` — directed component decomposition. Identifies regions with full mutual reachability (vs one-way pockets).

`orphans() -> Vec<Entity>` — Cells with no exits in either direction.

`one_way_in() -> Vec<Entity>` / `one_way_out() -> Vec<Entity>` — Cells reachable but not exitable, or exitable but not reachable.

`cycles() -> Vec<Vec<Entity>>` — simple cycles (capped count for large worlds; full enumeration is expensive).

### Aggregate metrics

`diameter() -> usize` — longest shortest-path. Useful for understanding world scale.

`density() -> f32` — edges divided by max-possible. Useful for visualization scaling.

### Predicate-driven traversal

`find_cells_within(from, max_steps, predicate: impl Fn(Entity) -> bool) -> Vec<Entity>` — Cells within a bounded distance whose data satisfies a predicate. Used both internally (locating game-state-relevant Cells) and exposed to soft-code.

### Mutation

`add_cell() -> Entity` — spawns a new Cell entity.

`add_exit(from: Entity, to: Entity, cost: Option<f32>) -> Entity` — spawns an Exit; `OutgoingExits`/`IncomingExits` are auto-maintained by Bevy.

`remove_cell(cell: Entity)` — despawns a Cell and all Exits connected to it (cascading cleanup via Bevy's relationship cleanup hooks).

`remove_exit(exit: Entity)` — despawns an Exit; both adjacency lists are auto-updated.

## Edge directionality

**Edges are directed.** Every Exit is a one-way edge from a source Cell to a destination Cell. A "normal" two-way passage is two Exit entities — one in each direction. This matches the MUSH-tradition mental model (you `@open north` and separately `@open south`), supports asymmetric cases naturally (one-way portals, holes-you-fall-down, doors-locked-from-one-side), and keeps the graph model uniform.

A `BidirectionalExit` *helper* (a one-shot spawn function that creates the matching pair) can hide the boilerplate at the builder layer. Internally, two directed edges; externally, one builder gesture.

## Edge weighting

**Default weight is 1 (step-counted). Optional `TravelCost(f32)` component overrides.**

Algorithms come in two variants where it matters: an `_unweighted` variant that ignores `TravelCost` and treats every edge as 1 (BFS), and a default variant that uses `TravelCost` if present (Dijkstra over weighted edges). The unweighted variant is strictly faster and is the default for "how many rooms away" queries.

Multi-criterion routing (fastest vs. safest vs. scenic) is deferred. If/when it materializes, it gets its own ADR; the trait implementations support it via additional `TravelCost`-shaped components and per-call edge-weight closures (`shortest_path_with_weight: impl Fn(Entity) -> f32`).

## Soft-code (Lykn) exposure

The soft-code surface is deliberately minimal at v0.4 introduction and grows with use. The initial primitives:

```lykn
;; Movement (the most common operation)
(go-exit avatar exit)                    ;; move along an Exit; emits events

;; Path and distance queries
(path-to from-cell to-cell)              ;; -> Path or nil
(distance from-cell to-cell)             ;; -> number or nil
(reachable? from-cell to-cell)           ;; -> boolean

;; Local topology
(neighbors cell)                         ;; -> list of Cells
(exits-of cell)                          ;; -> list of Exit entities
(within-steps cell n)                    ;; -> list of Cells

;; Predicate-driven (the interesting one)
(find-cells-where cell n predicate)      ;; -> list of Cells matching `predicate`
                                         ;;    within `n` steps from `cell`
```

The `find-cells-where` primitive composes graph traversal with Lykn predicate evaluation — "find every Cell within 3 steps that has the `lit` aspect," "find the nearest Cell whose `kind` is `library`." It's the primitive builders will reach for constantly once it exists, and getting its semantics right early avoids ad-hoc reinvention. The implementation runs Lykn predicates inside the petgraph traversal callback.

Topology-analysis queries (`articulation_points`, `cycles`, etc.) are exposed only to admin/builder roles, accessed via the builder command surface, not via Lykn primitives — at least initially. This keeps the soft-code API focused on play-relevant operations.

## Performance considerations

For text-world scales — worlds with hundreds to low thousands of Cells — the trait-based approach has acceptable overhead. The hot path in graph algorithms is `IntoNeighbors::neighbors`, which under our scheme is a Bevy `Query::get` lookup followed by an iteration over the `OutgoingExits` vector. Both are O(1) with small constants. The visited-set tracking uses `HashSet<Entity>` instead of petgraph's `bitvec`; the difference is roughly 2–5× per insert/contains, which sounds dramatic but multiplied across an entire algorithm is still microseconds-to-milliseconds for our scales.

If a world ever pushes into pathological size and benchmarks find the `HashSet` to be the bottleneck, the `Visitable::Map` type can be swapped for an `Entity::index()`-keyed bitset without changing any other code. The trait's purpose is exactly this kind of pluggability.

For algorithms that need all-pairs shortest paths (Floyd-Warshall, O(V³)), worlds beyond a few hundred Cells should consider precomputed indexes rather than on-demand queries. That's a v0.5+ concern, addressed when actual scale-pressure surfaces.

## Consequences

**Positive:**

- Single source of truth. ECS holds everything; there is no second representation.
- Reuses petgraph's tested algorithm catalog without reimplementing.
- No sync layer means no consistency-bug class.
- Bevy maintains adjacency lists structurally via the relationship system.
- Mutations are immediately visible to subsequent graph queries; soft-code can interleave reads and writes naturally.
- Idiomatic — uses Rust's trait system as designed.
- Test surface is concentrated: trait impls are mechanical; algorithm correctness is petgraph's responsibility.

**Negative:**

- Initial implementation cost: roughly 8–12 trait impls plus the `WorldGraph` and `WorldGraphMut` `SystemParam` definitions. Mechanical but real work, on the order of 1–2 days of focused effort.
- Maintenance: petgraph's trait API has been stable but is not frozen; major version bumps may require trait-impl updates.
- Performance ceiling slightly lower than a dedicated `StableGraph` for very large worlds (HashSet vs. bitvec for visited tracking). Unlikely to matter at our scales but worth knowing.
- Learning curve for contributors: graph operations look like ECS queries with petgraph algorithms layered on top, which is unusual. Documentation and examples have to compensate.

**Neutral:**

- Bevy 0.16's relationship system is the linchpin. Bevy 0.15 and earlier did not have it; we depend on 0.16+ in the workspace.
- Soft-code primitives are designed deliberately rather than emerging organically. The `find-cells-where` shape commits us to a specific composition of traversal + predicate evaluation.

## Alternatives considered

**A. Petgraph as canonical, ECS as data, with sync layer.** Two stores; sync system on every topology mutation. Inherits petgraph's data structure performance characteristics directly. Rejected because the sync layer is non-trivial to maintain and introduces a consistency-bug class that the trait-based approach avoids entirely. The performance benefit doesn't materialize at our scales.

**B. ECS as canonical, hand-rolled algorithms over component queries.** No petgraph dependency at all; we implement BFS, DFS, Dijkstra, articulation points, connected components, SCC ourselves. Rejected because the implementations are well-known but real work, and we'd be on the hook for correctness in perpetuity. Petgraph is a healthy crate; let it carry the algorithmic load.

**D. Cache adjacency lists in a Resource separate from ECS.** Hybrid where `OutgoingExits`/`IncomingExits` are not components but a centralized resource. Rejected because Bevy 0.16's relationship system already provides exactly this — automatic adjacency maintenance — at the component level, with all the benefits of ECS-native access.

## What this enables for future work

- **v0.3 (movement, builder construction):** the trait impls land; basic pathfinding, reachability, and connectivity validation are usable.
- **v0.4 (Lykn integration):** soft-code primitives wrap the trait-based queries; `find-cells-where` becomes builder-accessible.
- **v0.5 (description assembly):** distance-aware fragments use `paths_within` to determine relevance; region-level overlays use connected components.
- **v0.6+ (píngdiǎn techniques):** narrative-level techniques like `cǎoshé huīxiàn` (foreshadowing) can plant tagged fragments along a player's actual path, which requires path-tracking the queries enable.

## Implementation notes

The ADR commits to the architecture; specific implementation details are deferred to code review when the work lands.

Worth flagging for implementation:

- The `Visitable::Map` type starts as `HashSet<Entity>`; if benchmarks show it bottlenecking, swap for `bevy_utils::HashSet` (FxHash-backed) or an `Entity::index()`-keyed bitset.
- Topology-mutating systems must hold `WorldGraphMut` (exclusive write); read-only systems use `WorldGraph` (read-only, parallel-friendly). Bevy's scheduler enforces the discipline.
- Cascading despawn (deleting a Cell removes its Exits) uses Bevy 0.16's `linked_spawn` cascading cleanup feature. Verify behavior in tests.
- Path representations: a `Path` is `Vec<(Entity, Entity)>` (Cell pairs along the path) plus a `total_cost: f32`. Representing as Cells alone loses the per-edge weight information; representing as Exits alone loses the start Cell. Pairs are the cleanest middle.

## Related

- ADR-0002 — Architecture Overview (mentions petgraph; this ADR resolves the underspecification)
- ADR-0003 — `bevy_ecs` as the core data model (the Bevy 0.16 relationship system is load-bearing here)
- ADR-0004 — Multi-world server: one `bevy_ecs::World` per game-world (each world has its own `WorldGraph`)
- ADR-0006 — "Cells" as the primary spatial unit (Cells are the graph nodes)
- ADR-0008 — Lykn as the soft-code language (the Lykn primitives section commits to specific surface forms)
- ADR-0011 — v0.1 Implementation Plan (v0.1 doesn't use the graph; this ADR is preparation for v0.2/v0.3)
