---
number: 8
title: "Lykn as the soft-code language"
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

# Lykn as the soft-code language

**Status:** Draft
**Date:** 2026-04-25
**Type:** ADR

## Context

The MUSH tradition's defining feature is **soft-code** — the ability for in-world builders and trusted users to write game logic that the server executes without recompilation or restart. PennMUSH has MUSHcode, LambdaMOO has MOO programs, LPMud has LPC, Evennia has Python typeclasses with hot-reload. Whichever language fills this role becomes a defining property of the project: it shapes builder ergonomics, sets the ceiling for what soft-code can do, and constrains the engine's API surface.

The choice space includes:

- **MUSHcode (reimplemented in Rust)** — preserves community continuity. Notoriously difficult to maintain. String-based, intentionally terse, decades of accumulated idioms.
- **Lua via mlua** — mature, ubiquitous, well-supported in Rust. Familiar to game developers. Untyped. Not Lisp.
- **Steel** — Rust-native Scheme. Lisp. Reasonably mature.
- **Rhai** — Rust-native scripting language. Simpler than the alternatives. Untyped. Less expressive.
- **Rune** — Rust-native, Rust-syntax-aligned. Smaller community.
- **Embedded JavaScript (boa, QuickJS, deno_core)** — large ecosystem, heavy runtime, FFI overhead.
- **Lykn** — Duncan's typed, immutable, S-expression Lisp-flavored language. Has a Rust-based compiler, linter, and formatter. Currently targets Deno via JS compilation. Macro system used to define its own surface forms.

The Lykn option is unusual. It's a language Duncan owns, not a third-party dependency. Its current runtime target is Deno (JS), so server-side Rust execution requires either a new compilation target or a Rust-side interpreter. The case for Lykn has to be strong enough to justify that engineering work.

## The case for Lykn

Three resonances make Lykn structurally aligned with Liaozhai MUX, not just convenient.

**The `cell` pun.** Lykn's mutation primitive is `cell`. Liaozhai MUX's primary spatial unit is **Cell** (ADR-0005). A MUX Cell *is* a place. A Lykn cell *is* how state changes. Soft-code authored in Lykn manipulates MUX Cells via Lykn cells. The terminology compresses cleanly. This isn't a stretch — both terms emerged independently and they align.

**S-expressions as exegetical trees.** The *píngdiǎn* commentary tradition that animates the project (草蛇灰線 inside 烘雲托月 inside the narrative arc) treats narrative as a tree of nested techniques. S-expressions are tree structures. Lisp is the language form that already understands what literary exegesis is *doing*. Steel or Rhai would also work; Lykn is structurally aligned with the project's intellectual content.

**Macros as named lenses.** Lykn's macro system lets the surface language grow into the application's domain. A *píngdiǎn* critic adds named lenses to a fixed text; a Lykn programmer adds named surface forms that compile to kernel forms. Both are augmenting the readable surface without changing the substrate. MUX-specific surface forms (`define-cell`, `before-enter`, `lock-predicate`, `aspect`) can be defined as Lykn macros — the Rust side only ever sees kernel forms, dramatically reducing the interpreter surface.

Beyond the resonances, the practical advantages: Duncan owns the language and can evolve it in response to MUX needs. Vendor risk is internalized rather than externalized. The bi-directional pressure (Lykn improves to support MUX, MUX adopts Lykn idioms) is a feature, not a bug — both projects gain.

## Decision

**Liaozhai MUX adopts Lykn as the soft-code language.**

The first cut of soft-code support (planned for v0.4 — see ADR-0001's roadmap) ships as a **tree-walking interpreter in Rust** that consumes Lykn's AST after macro expansion to kernel forms. The interpreter is intentionally simple. Performance demands are low: sub-100 ms response to player actions is luxurious, and most soft-code runs in response to discrete events, not in hot loops.

The MUX-side Lykn integration ships as its own crate (likely `liaozhai-script` or `liaozhai-lykn` — see ADR-0008). It exposes:

- A function to compile a Lykn source file or AST into an interpretable representation.
- A function to evaluate that representation against a sandboxed environment, with hooks into the world's ECS state.
- A capability/permission system that limits what soft-code can read and write (no escaping its sandbox into arbitrary world or filesystem state).
- A standard library of MUX-specific Lykn macros (`define-cell`, `before-enter`, etc.) that compile to kernel forms the interpreter understands.

If/when soft-code performance becomes a bottleneck, Lykn gains a bytecode target and MUX gains a bytecode VM. The interpreter and VM share the same surface semantics, so soft-code authors don't see the change. This evolution gets its own ADR when it becomes relevant.

## Two-way evolution

Lykn-as-MUX-soft-code is a **bi-directional commitment**. MUX use cases will drive Lykn improvements; Lykn idioms will shape MUX surface forms.

Anticipated Lykn-side work (over time, not a v0.4 prerequisite):

- A stable AST export API consumable from Rust without going through JS compilation.
- Stabilization of the kernel form set (so the MUX interpreter has a fixed target).
- Possibly a Rust-targeted compilation or interpretation backend (long term).
- Hooks for host-defined types and effects, so MUX can expose ECS components and side-effecting commands as first-class Lykn values.

Anticipated MUX-side work:

- The interpreter (v0.4).
- The Lykn standard-library shape for MUX (Cell-aware macros, attribute access patterns, lock predicates, trigger registration).
- Documentation that teaches Lykn-for-MUX-builders without requiring prior Lisp experience.

## Consequences

**Positive:**

- The project's identity is internally coherent: Liaozhai (brand), MUX (lineage), Lykn (soft-code) — three layers, all bearing Duncan's hand. That kind of coherence is rare and load-bearing.
- The aesthetic alignment with *píngdiǎn* and exegesis is genuine. The choice strengthens the project's literary-tradition framing rather than working against it.
- Vendor risk is internal: bugs, missing features, and breaking changes are project decisions, not external constraints.
- Macro extensibility means MUX can grow its own surface vocabulary without compiler changes.
- Lykn gains real-world pressure to improve, which Duncan has indicated is desirable.

**Negative:**

- Lykn is younger than Lua/Steel/Rhai; battle-testing is shallower. Project owns the bugs.
- Adding a Rust-side Lykn runtime is real engineering work: roughly 2–4 focused weeks for the v0.4 walking interpreter, more for the eventual bytecode VM.
- Two projects become coupled. Lykn's evolution is now also MUX's evolution; planning has to account for both.
- Documentation burden compounds: MUX builders must learn MUX *and* Lykn together. Onboarding is heavier than if Lykn were already widely known.

## Alternatives considered

- **Steel (Rust-native Scheme)** — was the alternative-universe answer. Lisp-flavored, mature embedded story, no MUX-side runtime work needed. Rejected because the Lykn resonances (cell pun, project unity, macro alignment) are real and Steel offers none of them. Steel remains the fallback if Lykn integration encounters something unworkable.
- **Lua via mlua** — most ergonomic third-party option. Rejected because Lua isn't a Lisp, and the project's literary-exegetical framing wants a Lisp.
- **MUSHcode** — rejected by ADR-0006 (no source compatibility). Even if compatibility were a goal, MUSHcode is a poor language for new authoring.
- **Embedded JS (boa, QuickJS, deno_core)** — would let us reuse Lykn's existing JS compilation target unchanged. Rejected because the runtime weight, FFI complexity, and sandboxing difficulty are too high for the value.

## Related

- [0001 — Architecture overview](./0001-architecture-overview.md) (soft-code section)
- [0005 — Cells as primary spatial unit](./0005-cells-as-spatial-unit.md) (the `cell` pun)
- [0006 — Inspired by, not bound by, prior MUSHes](./0006-inspired-by-not-bound-by.md) (why not MUSHcode)
- *Future ADR-0009* — Lykn embedding strategy (when v0.4 work begins)
