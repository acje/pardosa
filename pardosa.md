# Pardosa — Research & Design Notes

Pardosa is an in-memory EDA storage layer implementing **fiber semantics** in Rust. It provides Event Carried State Transfer (ECST) with correctness, auditability, and deletion policy as first-class concerns.

## Origin

- **fiber-semantics** repo: defines the conceptual model — fibers, lines, draglines, migrations, and the per-fiber state machine (5 states: Undefined, Defined, Detached, Purged, Locked).
- **web-service-gin** repo: Go prototype implementing `pardosa.Server[T]` with generics, a `Dragline[T]` append-only log backed by `[]Event[T]`, and a `map[DomainIdentity]Fiber` lookup. NATS/JetStream persistence is stubbed but not wired.

## Core Concepts (from fiber-semantics)

| Concept | Description |
|---------|-------------|
| **Event** | Immutable fact with header: `Timestamp`, `DomainId`, `Detached`, `Precursor`, `DomainEvent` payload |
| **Fiber** | Singly linked list of events sharing a `DomainId` — the history of one entity/activity |
| **Line** | Append-only array of interleaved fibers. Locked between migrations |
| **Dragline** | A protected Line with a `LookupFiber` index for O(1) fiber head access |
| **Migration** | Version transition: enables schema upgrades, deletion policies (Keep, LockAndPrune, Purge) |

### Fiber State Machine

5 states, 10 transitions. Application operations and migration operations are separated by design.

#### States

| State | Description |
|-------|-------------|
| **Undefined** | DomainId has never existed |
| **Defined** | Fiber is active, key exists |
| **Detached** | Fiber is soft-deleted |
| **Purged** | Fiber only exists on optional audit trail. Key can be reused |
| **Locked** | Fiber only exists on optional audit trail. Key can NOT be reused |

#### Transitions

```
Undefined  --Create-->                Defined
Defined    --Update-->                Defined
Defined    --Detach-->                Detached
Detached   --Rescue-->                Defined
Detached   --Migrate(Keep)-->         Detached
Detached   --Migrate(Purge)-->        Purged
Detached   --Migrate(LockAndPrune)--> Locked
Purged     --Create-->                Defined
Locked     --Rescue-->                Defined
Locked     --Migrate(Purge)-->        Purged
```

#### Application operations (between migrations)

- **Create**: Undefined → Defined, Purged → Defined
- **Update**: Defined → Defined
- **Detach**: Defined → Detached (soft delete)
- **Rescue**: Detached → Defined, Locked → Defined (history lost on Locked, fresh start)

#### Migration operations (during migration only)

- **Migrate(Keep)**: Detached → Detached (fiber survives migration unchanged, remains soft-deleted)
- **Migrate(Purge)**: Detached → Purged, Locked → Purged (fiber removed from line, retained on optional audit trail, key reusable)
- **Migrate(LockAndPrune)**: Detached → Locked (fiber pruned to last event, removed from line, retained on optional audit trail, key not reusable except via Rescue)

#### Semantics

- **Defined fibers are implicitly kept** during migrations — no explicit Migrate(Keep) needed.
- **Locked vs Purged**: Both remove the fiber from the line. Locked prevents key reuse via Create but allows Rescue (original entity revival, history lost). Purged allows key reuse via Create (new entity).
- **Locked → Rescue**: History is lost. The rescued fiber starts fresh with no precursor from pruned events.
- **Locked → Migrate(Purge)**: Escalation path. A locked fiber can be fully purged in a subsequent migration, making the key reusable.

#### Test matrix

5 states × 7 action types (Create, Update, Detach, Rescue, Migrate(Keep), Migrate(Purge), Migrate(LockAndPrune)) = 35 pairs. 10 valid, 25 invalid.

#### Notes

- **Undefined is implicit absence** — no fiber entry exists in LookupFiber. Not a stored state.
- **Migrations are per-fiber decisions within a line-wide migration pass.** Each detached or locked fiber gets an individual migration policy applied during the pass. Defined fibers are implicitly kept. Undefined entries are skipped.

### Operations

- **Mutating**: Create, Update, Detach, Rescue
- **Migration**: Migrate(Keep), Migrate(Purge), Migrate(LockAndPrune)
- **Read**: Read, ReadWithDeleted, List, ListWithDeleted, History, ReadLine

## Go Prototype Analysis

Key types from `web-service-gin/pkg/pardosa/`:

```go
type Server[T comparable] struct {
    domainIdCounter DomainIdentity
    dragline        Dragline[T]
}

type Dragline[T comparable] struct {
    Line        []Event[T]
    LookupFiber map[DomainIdentity]Fiber
}

type Event[T comparable] struct {
    Timestamp   int64
    DomainId    DomainIdentity
    Detached    bool
    Precursor   Index
    DomainEvent T
}

type Fiber struct {
    Anchor  Index
    Len     uint64
    Current Index
}
```

### Known issues in Go prototype
- `List`/`ListWithDeleted` assume monotonically increasing DomainId — broken by design
- No concurrency (TODO: RWMutex)
- No stream persistence (TODO: NATS/JetStream write)
- Anchor always at start of fiber, should be at `Len % n`
- No migration implementation yet

## Rust Implementation Plan

### Type Mapping

| Go | Rust |
|----|------|
| `Server[T comparable]` | `Server<T: Clone + PartialEq>` |
| `Dragline[T]` | `Dragline<T>` with `Vec<Event<T>>` + `HashMap<DomainId, Fiber>` |
| `Index int64` | `type Index = i64` or newtype `Index(i64)` |
| `DomainIdentity uint64` | `type DomainId = u64` or newtype |
| `Event[T]` | `struct Event<T>` with `serde::Serialize + Deserialize` |
| RWMutex | `RwLock<Dragline<T>>` or `tokio::sync::RwLock` for async |

### Crate Dependencies (candidates)

| Crate | Purpose | Notes |
|-------|---------|-------|
| **`async-nats`** | NATS/JetStream persistence | Official client, v0.47+, Tokio-based. Replaces deprecated sync `nats` crate |
| **`serde` + `serde_json`** | Serialization | For DomainEvent payloads and persistence |
| **`tokio`** | Async runtime | Required by async-nats |
| **`thiserror`** | Error types | For `TransitionError` enum |

State machine is hand-rolled with exhaustive enum matching — no external crate. This keeps the state machine as inspectable data: a single `TRANSITIONS` table drives both runtime logic and DOT/Graphviz visualization.

### Architecture Sketch

```
pardosa/
├── Cargo.toml
├── src/
│   ├── lib.rs          # public API: Server<T>
│   ├── event.rs        # Event<T>, DomainId, Index types
│   ├── fiber.rs        # Fiber struct
│   ├── fiber_state.rs  # FiberState, FiberAction, transition(), TRANSITIONS table
│   ├── dot.rs          # DOT/Graphviz output from TRANSITIONS table
│   ├── dragline.rs     # Dragline<T>: Line + LookupFiber
│   ├── migration.rs    # MigrationContext, migration-only API surface
│   └── persistence.rs  # NATS/JetStream adapter (async-nats)
```

### Implementation Priorities

1. **Fiber state machine** — FiberState (5 variants), FiberAction (Create, Update, Detach, Rescue, Migrate(policy)), transition function, TRANSITIONS table, DOT visualization
2. **Core types** — Event, Fiber, DomainId, Index (newtype over u64, Option for no-precursor), Dragline
3. **Server API** — Create, Read, Update, Detach, Rescue, History, ReadLine
4. **Concurrency** — `RwLock<Dragline<T>>`
5. **Fix List operations** — iterate `LookupFiber` keys instead of assuming monotonic IDs
6. **Migrations** — MigrationContext gating, Keep/LockAndPrune/Purge with line reindexing
7. **Persistence** — async-nats JetStream integration

### Relevant Rust Ecosystem

**Event sourcing crates** (for reference, not direct use):
- `cqrs-es` — lightweight CQRS+ES framework, serverless-oriented
- `esrs` — Postgres-backed ES by Prima.it
- `thalo` — ES with Postgres+Kafka, includes schema DSL

**Append-only log patterns**:
- Bitcask pattern: append-only write log + in-memory HashMap index
- `nebari` — transactional append-only KV in pure Rust
- Segmented log pattern: https://arindas.github.io/blog/segmented-log-rust/

**State machines** (reference, not used — hand-rolled approach chosen for inspectability and DOT visualization):
- `statig` — hierarchical, generic, async, `no_std`
- `rust-fsm` — simpler DSL macro approach
- `sm` — compile-time validated, less maintained

## Open Questions

- DomainId as `u64` or `String`? Go prototype has TODO to convert to string
- Protocol buffers for DomainEvent serialization? (noted in Go TODO)
- Anchor stride: what value of `n` for `Len % n` anchoring?
- Should the Rust version be a library crate, a binary, or both?
- Migration versioning scheme for dragline versions
- MigrationContext struct contents — version number? policy list per fiber?
- Whether Purged/Locked fibers retain metadata (purge timestamp, original create timestamp) on the audit trail
