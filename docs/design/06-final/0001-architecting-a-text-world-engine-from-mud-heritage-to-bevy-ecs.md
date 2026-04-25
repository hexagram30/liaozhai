---
number: 1
title: "Architecting a text world engine: from MUD heritage to Bevy ECS"
author: "splitting infrastructure"
component: All
tags: [change-me]
created: 2026-04-25
updated: 2026-04-25
state: Final
supersedes: null
superseded-by: null
version: 1.0
---

# Architecting a text world engine: from MUD heritage to Bevy ECS

**The strongest foundation for a Rust/Bevy/ratatui text world engine lies at the intersection of three traditions**: the graph-based spatial models and live-coding extensibility of classic MUDs, the data-driven component composition of modern roguelikes like Caves of Qud and Cataclysm DDA, and the salience-based text assembly pioneered by Valve's dynamic dialogue and Emily Short's procedural text research. This report synthesizes decades of design knowledge across these traditions into concrete architectural guidance. The core insight is that ECS composition maps almost perfectly onto how both tabletop RPGs and classic MUDs already decompose world state — advantages, aspects, attributes, and properties all translate naturally into components — while modern narrative research provides the missing layer for turning that flat component data into readable prose.

---

## How five generations of text worlds organized their data

The evolution from DikuMUD (1990) to Evennia (2006+) traces a clear arc from rigid, compiled data structures toward dynamic, runtime-composable object models — an arc that ECS completes.

**DikuMUD** defined the template: three core types (`struct room_data`, `struct char_data`, `struct obj_data`) with rooms connected by a fixed 6-direction exit array (N/S/E/W/U/D), each exit storing a destination room virtual number (vnum), door flags, and a key vnum. The world loads from flat text files (`.wld`, `.mob`, `.obj`, `.zon`) at boot into RAM arrays, with `real_room()` mapping vnums to array indices. Zone files encode reset commands — "load mobile M at room R, equip it with object O" — providing the template-instance pattern. DikuMUD's simplicity drove massive proliferation (inspiring EverQuest and WoW), but its hard-coded C structs meant **any new object property required recompilation**. No inheritance, no scripting, no live editing.

**LPMud** solved extensibility by splitting infrastructure into a C driver (the VM) and a LPC mudlib (the game framework). The revolutionary insight was that the driver knows nothing about game concepts — rooms, weapons, and NPCs are defined entirely in LPC code. Objects use prototype-based inheritance via the `inherit` keyword, with containment modeled through `environment()` (what contains me) and `move_object()`. A room is just a `.c` file inheriting `/lib/room.c` that calls `set_short()`, `set_long()`, and `add_exit()`. Wizards coded new content while the game ran. The tradeoff: **no standardized mudlib**, so each MUD reinvented its world model.

**LambdaMOO** unified everything into numbered objects (`#0`, `#1`, `#958`) with properties (named data slots) and verbs (named programs). The parent chain provides prototype inheritance — `$room` is the ancestor of all rooms, `$thing` of all things. Eight built-in properties (`name`, `owner`, `location`, `contents`, `programmer`, `wizard`, `r`, `w`) plus arbitrary user-defined properties. Command parsing maps player input to verb calls with direct/indirect objects and prepositions. This model was conceptually elegant — everything is an object — but **scaling to complex simulations was difficult** because the entire database was a single file, and the MOO language lacked features like regular expressions until later forks like ToastStunt.

**MUSH/PennMUSH** adopted an attribute-based model: four object types (rooms, exits, things, players) identified by recyclable dbrefs (`#1234`), each carrying arbitrary string-keyed attributes. MUSHcode — the programming language — lives inside attributes, creating an unusual code-as-data architecture. PennMUSH's attribute trees (backtick-delimited: `BRANCH`DATA`) provide hierarchical organization with access control propagation. The lock system supports complex boolean permission expressions (`@lock exit=(job:janitor&sex:male)|sex:female`). MUSHes achieved remarkable flexibility for social/RP worlds, but **MUSHcode is notoriously difficult to maintain** — everything is string-based, and the language is intentionally terse.

**Evennia** (modern Python) bridges these traditions using Django ORM plus a typeclass system. Only **four database tables** exist (`AccountDB`, `ObjectDB`, `ScriptDB`, `ChannelDB`), with `db_typeclass_path` storing a Python dotted path that dynamically applies the correct class at load time. Attributes are separate Django model objects supporting arbitrary pickled Python values. Tags provide lightweight categorization for fast filtering. The Idmapper caching layer keeps typeclass instances in memory. This gives MOO-like flexibility with proper Python tooling — tests, version control, pip packages, and Django migrations for schema changes.

### What the roguelikes added to the model

Roguelikes extended MUD-style world models with deep simulation layers that text worlds traditionally lacked.

**Dwarf Fortress** models geology through deterministic multi-stage generation: elevation (1–400), rainfall (0–100), temperature (10–70), drainage (0–100), and volcanism (0–100) drive biome assignment. Below the surface, accurate geological layers — sedimentary, metamorphic, igneous — determine mineral placement. The fortress map is a 3D tile grid organized by **z-levels** (typically ~50 of land + 15 of sky), each tile storing terrain type, material, water level (0–7), temperature, and references to occupying entities. DFHack's reverse-engineered structures (over 5,362 commits of XML describing C++ types) reveal hundreds of entity types including `historical_figure`, `artifact_record`, `creature_raw`, and `inorganic_raw`. The multi-century history simulation — tracking individual figures, wars, artifacts, and civilizations — creates genuinely emergent narrative context for gameplay.

**Caves of Qud** uses an ECS architecture where **Parts are components** attached to objects defined in `ObjectBlueprints.xml` (22,000+ lines). The inheritance tree (`Object → PhysicalObject → InorganicObject → Item → Armor → BaseHelmet → KnollwormSkull`) means children inherit all parts from parents. Critically, any Part can be attached to any object — adding a `Brain` part to a table creates animated furniture. Parts communicate via an **event propagation system**: events fire on entities and propagate to all parts unless handled or prevented. The spatial model uses parasangs (3×3 screens), with 80×25-tile zones generated lazily via a modular builder pipeline that combines `SolidEarth`, `CreateVoids`, Perlin noise maps, and Wave Function Collapse. This two-phase approach — world-level placement followed by zone-level fabrication — balances authored structure with procedural content.

**Cataclysm DDA** is perhaps the purest example of data-driven design. Nearly all content lives in JSON files with a `"type"` field (`TOOL`, `ARMOR`, `GUN`, `MONSTER`, `mapgen`). The `"copy-from"` field provides JSON-level inheritance, allowing new items to extend existing definitions. Hundreds of flags (`SEES`, `HEARS`, `SMELLS`, `ACID_IMMUNE`) create a rich attribute system. This architecture enables massive community modding — hundreds of mods — without touching C++. The tradeoff: **JSON syntax errors prevent the entire game from loading**, and complex items require deeply nested field structures.

**NetHack** takes the opposite approach: monolithic C arrays. The `mons[]` array in `monst.c` defines every monster as a `struct permonst` with speed, AC, magic resistance, attacks, generation flags, and resistances. Objects use type-specific interpretation of four generic integer value fields. The `levl[x][y]` 2D array stores terrain per tile. It's extremely memory-efficient (bitfields everywhere) and the entire game content is greppable, but **adding content requires source modification** — only NetHack 3.7's introduction of Lua scripting began addressing this.

---

## ECS composition patterns that unify these traditions

Bevy's relationship system (introduced in 0.16) provides the architectural primitive that connects MUD-style spatial models with ECS data composition.

**Custom relationships** model containment and spatial adjacency directly:

```rust
#[derive(Component)]
#[relationship(relationship_target = RoomContents)]
struct InRoom(pub Entity);

#[derive(Component)]
#[relationship_target(relationship = InRoom)]
struct RoomContents(Vec<Entity>);

#[derive(Component)]
struct Exits(HashMap<Direction, Entity>);  // room → room connections
```

Relationships are **non-fragmenting** (entities pointing to different targets share the same archetype), **immutable** (inserting a new component replaces the old one and hooks update the target), and support `linked_spawn` for cascading despawn. They currently support one-to-many patterns. For graph-based world topology, `petgraph::StableGraph` (Rust's most popular graph library, 2.1M+ downloads) complements ECS relationships well — stable indices remain valid after node removal, and `serde-1` support enables serialization.

**Flecs** pioneered a more radical approach with entity pairs — a 64-bit ID encoding both relationship and target. This enables queries like `(Eats, *)` (wildcard) and built-in cleanup properties for cascading deletes. The tradeoff is **archetype fragmentation**: different pairs create different archetypes, requiring `DontFragment` mitigation. Bevy's relationship system achieves similar expressiveness without this fragmentation cost.

The decomposition of complex world state into composable components follows a clear pattern. A "forest room in winter at night during rain" becomes orthogonal components: a `Room` marker, `Terrain { biome: Forest }`, `Season { current: Winter }`, `TimeOfDay { hour: 22.0 }`, `Weather { condition: Rain }`, and `Outdoors` as a zero-size tag. The distinction matters: **tag/marker components** (zero-size, binary flags like `Hostile`, `Lit`, `Outdoors`) differentiate archetypes at no storage cost; **data components** (parameterized state like `Health(u32)`, `Temperature(f32)`) carry fields accessed by systems; **relationship components** model entity-to-entity links.

### Tabletop RPG systems as component blueprints

Tabletop systems already decompose world state into composable units that map directly to ECS components. **GURPS advantages** are the clearest example: each is a discrete, point-costed unit — `Combat Reflexes` (15 CP) becomes a zero-size tag component, `Wealth(WealthLevel)` becomes a parameterized data component, and the hundreds of advantages in the GURPS sourcebooks mirror an ECS component library.

**Fate's aspects** — free-form text strings with mechanical weight — introduce a more fluid model. An aspect like "Burning Kitchen" attached to a zone entity is simultaneously a narrative descriptor and a mechanical trigger (invokable for +2 to fire-related actions). The **Fate Fractal** principle — that anything can be treated as a character with aspects, skills, and stress — maps perfectly to ECS, where any entity can carry any components. Fate's **zones** (abstract spatial regions defined by narrative boundaries, not measurements) are particularly relevant to text worlds: a zone is "close enough to interact directly," with aspects like "Ladder Access Only" on zone boundaries replacing precise distance calculations.

**Blades in the Dark progress clocks** — 4/6/8-segment trackers named for obstacles ("Perimeter Security," "Ritual Complete") — translate directly to ECS data components. Linked clocks (filling one triggers another) model cascading world state changes. Faction clocks ticked during downtime drive world dynamism — a pattern that maps cleanly to Bevy's `FixedUpdate` schedule for background world simulation.

---

## Assembling natural prose from component data

The central challenge of a text world engine is transforming flat ECS component data into prose that reads as authored narrative rather than database output. Five decades of interactive fiction research provide tested solutions.

**Inform 7's activity pipeline** is the gold standard for layered description assembly. The carry-out-looking rulebook triggers a cascade: room name → room description → "writing a paragraph about" rules (author-provided custom paragraphs for specific entity configurations) → initial appearance text for new objects → "listing nondescript items" (the automated "You can see X, Y, and Z here"). The critical mechanism is the **"mentioned" flag**: when a custom paragraph describes Mr. Wickham and lists the women in the room, all of those entities get tagged as "mentioned" and won't appear redundantly in the nondescript items list. This prevents the concatenated-fragment problem that plagues naive component assembly.

Inform 7's **adaptive text system** handles grammatical agreement automatically: `"[The actor] [put] [the noun] on [the second noun]."` produces "You put the revolver on the table" or "General Lee puts the revolver on the table" depending on the actor, managing person, number, tense, and contractions. Text variation uses `[one of]…[at random]`, `[one of]…[as decreasingly likely options]`, and conditional `[if condition]…[otherwise]…[end if]` blocks. The `visited` property enables different first-visit versus return descriptions — a basic but essential form of novelty detection.

**Emily Short's procedural text taxonomy** organizes techniques by sophistication:

- **Template systems** with variable substitution (the baseline)
- **Generative grammars** like Tracery — recursive context-free expansion from JSON rule sets, with `.capitalize`, `.s`, `.ed`, `.a` modifiers handling English morphology
- **Salience-based systems** that select the most specifically matching variant for the current world state (Valve's approach)
- **Tagged generative grammars** combining all three — every expansion indexed with metadata (emotional state, world knowledge, relationship status), with salience selecting the best match and fallback to progressively less specific variants

The tagged-grammar-plus-salience approach, refined in Short's work at Spirit AI's Character Engine, is the most promising architecture for a component-driven text world. Each descriptive fragment is tagged with the component states it's appropriate for; the system selects the most specific match and falls back gracefully.

### Valve's dynamic dialogue as an architectural template

**Elan Ruskin's GDC 2012 talk** on "AI-driven Dynamic Dialog through Fuzzy Pattern Matching" describes the system used across Left 4 Dead, Portal, DOTA, and all Source engine games. The system tracks **hundreds of world-state facts** uniformly, then fuzzy-matches against a database of thousands of possible lines. Each line is tagged with criteria (character identity, health level, nearby allies, recent events, weather). The most specific matching line wins — more conditions matched equals more salient equals preferred. Writers add special cases, running gags, and track additional facts without programmer involvement. Firewatch adopted this system wholesale for its dynamic Henry/Delilah dialogue.

For a Bevy implementation, this translates to: ECS components are the "facts," description fragments are tagged with component-query predicates, and a description-assembly system selects the most specifically matching fragments. A room's weather overlay might have variants tagged `{season: Winter, weather: Rain}` ("freezing rain drips from skeletal branches"), `{weather: Rain}` ("rain patters on the canopy"), and a default ("the forest stands quiet"). The system picks the most specific match.

### Montfort's Curveship and the fabula/discourse split

Nick Montfort's **Curveship** demonstrates that the same underlying events can be narrated with radically different discourse strategies — varying order (flashback, flashforward), frequency (tell once or repeatedly), speed (summary or scene), focalization (whose perspective), and even naming conventions ("the father" vs "a man" for parable-like register). Curveship achieves this by cleanly separating the simulator (first-order action representations) from the narrator/teller module (which applies a "spin" parameter). For a Bevy engine, this suggests maintaining event logs alongside current state, so description systems can reference not just what exists now but what happened and in what order.

**James Ryan's dissertation** ("Curating Simulated Storyworlds," UC Santa Cruz, 2018) adds a crucial reframing: emergent narrative works more like nonfiction than fiction — stories "actually happen" in simulation. The system's job is **curation**, not invention. Ryan identifies that simulation causality can be too diffuse for good storytelling; solutions include explicit causal chains (contingent unlocking with causal bookkeeping) and modular, recombinant simulation elements. His *Bad News* project at SFMOMA demonstrated this with a richly simulated town whose emergent stories were performed live.

---

## What to show, when, and how: filtering and presentation

The gap between complete world state and what a player should see at any moment is the information architecture problem. Text games face this acutely — there's no peripheral vision, no ambient audio, only deliberately surfaced text.

### Layered perception and granularity scaling

Classic MUDs established a hierarchy that modern TUI games should preserve. **Discworld MUD** exemplifies the layered approach: room descriptions include short description → long description → dynamic atmospheric lines (crowd density on streets, background magic visible only to wizards/witches) → weather line (outdoor only) → living things → objects. Each layer is conditional on character class, skill level, and lighting. A dark room returns only "Something. It's dark here, isn't it?" while lit rooms show full descriptions. The `scan` command extends this spatially: full detail for the current room, entity names for adjacent rooms, counts only at 2–3 rooms distance.

Granularity scaling follows a **glance → look → examine → search** hierarchy. In most MUDs, `brief` mode shows room name and exits only; `look` gives the full description plus entities present; `examine` combines `look <item>` with `look in <item>` and reveals properties; `search` is skill-gated active investigation. Discworld adds sensory commands (`smell`, `taste`, `listen`) each returning different text. For ECS, this means storing description data at multiple granularities — `GlanceDescription`, `LookDescription`, `ExamineDescription` — or using a single rich description with perception-threshold-tagged details that filter at render time.

**Roguelike FOV algorithms** adapt to text worlds for map rendering. Symmetric shadowcasting (by Albert Ford) guarantees bidirectional visibility — if A sees B, B sees A — using rational numbers to eliminate floating-point artifacts. For a text-world TUI map panel, the explored/visible/unknown trichotomy (explored tiles shown dimmed, visible tiles bright, unknown tiles blank) extends naturally to text: full detail for visible rooms, "remembered" summaries for explored-but-not-visible, nothing for unknown.

### Separating data from display via structured protocols

**GMCP (Generic MUD Communication Protocol)** is the clearest precedent for data/presentation separation in text worlds. Evolved from Achaea's ATCP (2008), it sends JSON over Telnet subnegotiation — separate from the text stream. Data organized into subscribable namespaces: `char.vitals`, `room.info`, `room.map`, `comm.channel`. Crucially, **data is only sent when values actually change** (efficient delta updates). The same `char.vitals {"hp": 450, "maxhp": 500}` renders as a text prompt, a colored gauge bar, or a numeric HUD panel depending on the client.

In Bevy, this pattern becomes multiple rendering systems reading the same components. A `TextDescriptionSystem` queries `Position + Description + Visibility` to generate prose. A `MapRenderSystem` queries `Position + MapGlyph + Visibility` for the ASCII map panel. A `StatusBarSystem` reads `Health + Stamina + Equipment` for the HUD. All share the same ECS world. The `bevy_ratatui` crate provides the integration layer, and `zgrow/spacegame` on GitHub demonstrates this Bevy+ratatui pattern in a sci-fi roguelike.

### Terminal UI information architecture

**Cogmind** (by Kyzrati) is the landmark reference for terminal game UI. Operating on a **160×60 character grid**, it keeps "all or almost all important information necessary for decision-making visible at all times." The layout places the message log top-left, map center (minimum 50×50 game tiles using 2-wide characters), core stats top-right, equipment list with condition bars, and a multi-function window that switches between combat calculations, nearby ally status, and extended log. When screen space is limited, Cogmind offers three layout presets: full 60-row (everything visible), semi-modal 45-row (inventory hides when full map shown), and fully modal 45-row (multiple panes hidden/shown on demand). This is **progressive disclosure applied to game UI**.

For a ratatui implementation, the recommended layout uses horizontal constraints splitting ~70/30 between main content and sidebar, with nested vertical layouts within each:

The **main content area** should contain a scrollable narrative pane (the largest element — this *is* the game for text-heavy experiences), an optional ASCII map view below it, and a mode-switching context panel at the bottom (combat details during fights, exploration info during travel, dialogue choices during conversations). The **sidebar** holds persistent status (HP/MP gauges, key stats), equipment summary, and a minimap or compass. Pop-up overlays handle inventory management and detailed inspection — modal windows rendered via `Clear` + redraw on top of main content.

### Adaptive UI that restructures by activity

The interface should reconfigure based on game mode. A `GameMode` enum (Exploration, Combat, Dialogue, Crafting, Inventory) drives layout weights: exploration emphasizes the map (60%) with narrative alongside (25%); combat emphasizes the combat log (30%) with a tactical map (40%) and action menu (15%); dialogue maximizes the conversation pane with NPC information and response choices. Context-sensitive controls bind the same keys to different actions per mode, reducing memorization burden — the same key means "interact" in exploration, "attack" in combat, "select option" in dialogue.

Player customization follows Cogmind's proven three-tier approach: auto-detect terminal size and apply a layout preset (compact for <120 columns, standard for 120–180, expanded for 180+), expose a "Display Preset" dropdown with 3–4 options, and only unlock granular pane sizing/visibility toggles if the player selects "Custom." Research on the paradox of choice (Iyengar & Lepper's jam study: 60% stopped at 24 options but only 30% bought; 40% stopped at 6 options but 60% bought) confirms that **limiting initial choices to 5–7 dramatically improves engagement**.

---

## Cross-cutting architecture: time, space, events, and persistence

### Event sourcing turns game events into narrative

Event sourcing — recording all state changes as an immutable event stream — is uniquely valuable for text games because **the event log doubles as the narrative log**. Each `PlayerMoved { from, to }`, `DamageDealt { source, target, amount }`, or `WeatherChanged { old, new }` naturally produces descriptive text while also enabling replay, undo, and debugging. Bevy provides two event mechanisms: **Messages** (`MessageWriter`/`MessageReader`) for frame-buffered broadcast suitable for fire-and-forget game events, and **Observers** (`Event` + `On<T>`) for immediate reactive triggers ideal for cascading effects like trap chains.

CQRS maps naturally onto this architecture: commands (player input → validation → world mutation) flow through write-side systems, while multiple read-side systems project the same world state into different views — room descriptions, map rendering, combat logs, NPC AI perceptions. Bevy's automatic parallel scheduling based on data access patterns naturally enforces read/write separation.

### Spatial data for graph worlds with coordinate extensions

For the primary world topology, **`petgraph::StableGraph<RoomData, ExitData>`** provides graph-based room/exit modeling with stable indices that survive node removal, Dijkstra pathfinding, BFS/DFS traversal, and serde serialization. For wilderness areas needing proximity queries, layer **`rstar`** (R*-tree spatial index, N-dimensional, optimized for nearest-neighbor) or a simple `HashMap<(i32, i32), Entity>` spatial hash. The `bevy_spatial` crate integrates spatial indexing directly with Bevy's entity system.

A hybrid approach works well: interior/authored spaces use the room graph; outdoor/procedural spaces overlay a coordinate grid with named region entities. Caves of Qud's parasang→zone→cell hierarchy provides a tested model for this layering.

### The world tick: energy systems meet Bevy scheduling

Classic MUDs used pulse-based ticking: DikuMUD's `PULSE_SEC` constant drives different subsystems at different rates — combat every 2–3 seconds, regeneration every 30–60 seconds, weather every 5–10 minutes. LPMud's per-object `heart_beat()` let objects independently opt into periodic updates.

Roguelikes solved the turn-based fairness problem with **energy-based scheduling**: every entity accumulates `energy += speed` each tick; when `energy >= threshold` (typically 1000), the entity acts and `energy -= action_cost`. Different actions have different costs (move: 1000, attack: 1200, cast spell: 2000), and arbitrary speed ratios emerge naturally (speed 102 vs 103 is distinguishable). Bob Nystrom's implementation in his Dart roguelike "Hauberk" adds the refinement that failed actions (wall-bumps) don't consume energy.

For Bevy, the recommended architecture uses `FixedUpdate` with a configurable timestep as the world tick, player input systems in `Update`, and `run_condition` to gate systems on game state. An energy system as a Bevy `Resource` manages the turn queue:

```rust
#[derive(Resource)]
struct TurnScheduler {
    queue: BinaryHeap<(i32, Entity)>,  // (energy, entity)
}
```

Systems chain in explicit order: input → command resolution → world update → description generation → output. Bevy's `.before()`, `.after()`, and `.chain()` enforce this ordering, while `SystemSet` grouping enables different update rates for different simulation layers.

### Saving and loading complex world state

The `bevy_save` crate (by hankjordan) provides the most comprehensive save/load solution: `World::save()` / `World::load()` for full snapshots, `World::capture()` / `World::apply()` for in-memory snapshots, and **`World::checkpoint()` / `World::rollback()`** for undo/replay — directly supporting event sourcing patterns. Only types registered with `register_saveable::<T>()` are included, with support for custom formats (binary default, JSON available) and middleware (compression, encryption). Migration support via `ReflectMigrate` handles type changes across versions.

The recommended architecture separates **template data** (room definitions, item blueprints, NPC prototypes — loaded from RON/JSON at startup like MUD area files) from **instance data** (player inventory, NPC state, world modifications — saved via `bevy_save` snapshots). This mirrors the zone-file/player-file split that every successful MUD used, while `petgraph` with its `serde-1` feature flag enables serializing the world graph alongside ECS state.

---

## Conclusion: the architecture that emerges

The synthesis across these traditions points to a specific architecture. **World topology** should use `petgraph::StableGraph` for room/exit graphs with Bevy relationship components (`InRoom`, `RoomContents`) for entity-room membership. **Entity composition** should follow GURPS/Fate patterns — zero-size markers for binary traits, data components for parameterized state, relationships for entity-entity links. **Description generation** should implement Valve's salience-based fuzzy matching: tag description fragments with component-query predicates, select the most specific match, fall back gracefully. Inform 7's "mentioned" flag pattern prevents redundant listing, and the glance→look→examine hierarchy gates detail through perception thresholds.

The deepest lesson from this research is that **the separation of concerns already exists in the source traditions** — it just needs to be recognized and mapped. MUD zone files are ECS archetypes. MUSH attributes are components. Fate zones are spatial relationship entities. Progress clocks are timer components. Inform 7's activity pipeline is a system chain. Curveship's fabula/discourse split is the event-log/rendering-system distinction. The engine doesn't need to invent new patterns so much as faithfully translate proven ones into Bevy's idiom, using `petgraph` for topology, salience matching for text, energy scheduling for turns, and `bevy_save` for persistence. The result should be a system where adding a new weather type means adding a JSON definition and a few tagged description fragments — not modifying engine code.
