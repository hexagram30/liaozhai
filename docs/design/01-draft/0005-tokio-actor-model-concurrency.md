---
number: 5
title: "Tokio + actor-model concurrency"
author: "exactly one"
component: All
tags: [change-me]
created: 2026-04-25
updated: 2026-04-25
state: Draft
supersedes: null
superseded-by: null
version: 1.0
---

# Tokio + actor-model concurrency

**Status:** Draft
**Date:** 2026-04-25
**Type:** ADR

## Context

The Liaozhai MUX server has two kinds of concurrent work to manage:

1. **I/O-heavy work** — accepting network connections, reading client input, writing server output, persisting to disk. Many concurrent operations, each doing little compute, often blocked on I/O.
2. **CPU-light, state-heavy work** — advancing the simulation of each world on a tick, processing the command queue, computing description-assembly outputs. Few concurrent operations, each touching a coherent block of state (one world's ECS), running periodically.

These have different optimal concurrency strategies. The I/O work wants async tasks — many of them, cheap to spawn, scheduled cooperatively. The simulation work wants exclusive access to its ECS world during a tick, with no contention from other workers, in a predictable schedule.

The MUSH tradition handled this with a single-threaded event loop (the C `select()` model in PennMUSH/TinyMUSH). Modern Rust gives us better tools: tokio's async runtime for I/O, dedicated tasks for CPU-bound work, and channels for safe communication between them.

## Decision

**Tokio is the async runtime.** All I/O — network listeners, client sessions, file persistence — uses tokio's async APIs. The runtime is multi-threaded by default; the work-stealing scheduler distributes async tasks across worker threads.

**Each connection is an async task.** When a client connects, an async task is spawned to handle that connection's lifecycle: read input, parse, dispatch, write output. The task ends when the connection closes.

**Each world is an actor.** A world's ECS state is owned by exactly one task. That task receives commands on an `mpsc` channel, processes them in order against its world, advances the world's tick on a timer, and broadcasts output on a `broadcast` channel that connection tasks subscribe to. No other task touches that world's `bevy_ecs::World` directly.

**Channels carry everything across boundaries.** Connection task → world task is `mpsc::Sender<WorldCommand>`. World task → connection tasks is `broadcast::Sender<WorldOutput>`, with each connection holding its own `Receiver`. World task → persistence task is a separate `mpsc`. Server-level channels (chat, system messages, world registry events) follow the same pattern.

**No shared mutable state across task boundaries.** What looks like shared state (the world registry, the account database) is mediated by tasks that own those resources, accessed through their own command channels.

## Consequences

**Positive:**

- The data ownership story is clean — every piece of mutable state has exactly one task that can mutate it.
- No locks, no `Arc<Mutex<...>>` smell. Channels are the synchronization primitive.
- World ticks are predictable. The world's task wakes on its tick interval, drains its command queue, advances simulation, broadcasts output. No interleaving with other worlds, no contention.
- Tokio's tracing/instrumentation maps well onto the actor-shaped graph; each task gets a span, each channel send is observable.
- Async cancellation works naturally — drop a task to stop a world, drop a connection to disconnect a client.

**Negative:**

- The actor model has structural costs: serializing commands as messages, copying data across channels (or wrapping in `Arc` when copying is too expensive), thinking about buffering and back-pressure.
- Bevy ECS schedule abstractions are designed for in-process system parallelism, but our worlds are single-task-each. We're not benefiting from Bevy's parallel scheduler within a world. (In practice this is fine — text-world simulation is light enough that single-threaded ticks are not a bottleneck.)
- Cross-task debugging is harder than cross-function debugging. Tracing infrastructure becomes important earlier than it would in a synchronous design.

## Tick rate

World ticks default to 100 ms (10 Hz) — fast enough for fluid command response, slow enough to keep CPU and serialization cost low. Per-world override allows narrative-pace worlds (slower) and combat-heavy worlds (faster).

A world's tick processes its command queue, advances any time-driven systems (weather, NPCs, scheduled events), and broadcasts queued output. There is no global tick — each world ticks independently on its own schedule.

## Connection lifecycle in actor terms

```
client connects
  └─► server spawns connection task
       ├─ handshake, auth, world selection
       └─ enters InWorld state
           ├─ subscribes to world's broadcast::Receiver<WorldOutput>
           ├─ holds mpsc::Sender<WorldCommand> for that world
           ├─ loop:
           │   ┌─ read input from socket → parse → send to world via mpsc
           │   └─ recv from broadcast → write to socket
           └─ on disconnect: drop subscription, drop sender, end task
```

## Alternatives considered

- **`async-std`** — tokio is the de facto standard; the ecosystem (database drivers, network protocols, instrumentation) overwhelmingly assumes tokio.
- **Single-threaded event loop (PennMUSH-style)** — workable but gives up parallelism for free across worlds. Multi-world is a designed feature; we want worlds to make progress independently.
- **`Arc<Mutex<World>>` instead of actor model** — invites contention and lock-ordering bugs. The actor model is the better discipline.
- **`crossbeam` channels with thread-per-world** — possible, but mixes two concurrency models (threads for worlds, tokio for I/O) when one (tokio everywhere) suffices.

## Related

- [0001 — Architecture overview](./0001-architecture-overview.md)
- [0003 — Multi-world server](./0003-multi-world-server.md) (each world is an actor under this scheme)
