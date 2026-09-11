# Consumer Adapter Blueprint: Demarcation & Implementation Guide

## Table of Responsibilities

| Dimension | Pardosa Core Owns | Consumer-Side Adapter Owns |
|---|---|---|
| **Identity & Indexing** | `FiberId` and `EventId` minting via Dragline; causal precursor BLAKE3 hash chain. | Domain Aggregate IDs (e.g. `IssueId`, `RepoId`), entity keys, in-memory `FiberIndex` mapping. |
| **Sequencing & Ordering** | Same-fiber total order; Dragline global append log order; backward window. | Global projection ordering across fibers; CQRS causal tracking; cross-aggregate timelines. |
| **Serialization & Types** | Binary AST schema descriptors (tag `0x09`), canonical codec, `GenomeSafe` constrained types. | Serde `Serialize`/`Deserialize` DTOs, JSON/MessagePack representations, web/GraphQL models. |
| **Storage & Engines** | Sealed `StorageEngine`, `.meta` / `.pgno` file layout, NATS JetStream OCC headers. | Storage path configuration, NATS connection credentials, environment variable ingestion. |
| **Concurrency & Fencing** | Single-writer CAS ownership, epoch verification, `ConcurrencyConflict` error generation. | Retry backoff policies, transaction boundary rollback, user notification, command re-evaluation. |
| **Lifecycle & Bootstrap** | Strict `create()` and `open()`; non-destructive initialisation; ordered `.meta` -> `.pgno` creation. | Two-arm bootstrap orchestration (`create()` else `open()`); deployment migration provisioning. |
| **Runtime & Execution** | Synchronous API facade; internal thread blocking; internal runtime isolation. | Tokio task scheduling; `tokio::task::spawn_blocking` wrappers; worker pool bounding. |
| **Projections & Queries** | Raw event stream rehydration via `replay_all()` and single-fiber forward cursors. | CQRS read-model projectors; SQL/SQLite tables; query engines; in-memory materialized views. |
| **Telemetry & Context** | Envelope 5-field header (no custom fields); internal logging. | W3C Traceparent, span context, correlation IDs (embedded inside event payload `T`). |

## Anti-Pattern / Prohibited Drift Inventory

1. **Leaking Domain Keys / AggregateId into Pardosa Core**:
   - *Violation*: Adding domain keys or `expected_version` to `EventStore` append or indexing APIs.
   - *Remedy*: Keep `FiberIndex` purely in consumer adapter memory. Pass only `FiberId` and payload `T` to Pardosa.
2. **`open_or_create()` or Destructive Overwrite Flags**:
   - *Violation*: Adding auto-create convenience constructors or `truncate(true)` initialisers.
   - *Remedy*: Require consumer to explicitly match on `OperationFailure::is_already_exists()` and fall back to `open()`.
3. **Public Serde Derives or Untyped JSON**:
   - *Violation*: Adding `#[derive(Serialize, Deserialize)]` to Pardosa core types or supporting arbitrary JSON trees.
   - *Remedy*: Adhere to Zero Public Serde (jn1.8 D6). Translate between JSON DTOs and `GenomeSafe` enums at adapter boundary.
4. **Public Async Facade APIs**:
   - *Violation*: Exposing `async fn` on `EventStore`, `StoreWriter`, or `StoreReader`.
   - *Remedy*: Keep the 1.0 facade strictly synchronous per PGN-0010. Offload via `tokio::task::spawn_blocking` in consumer adapters.
5. **In-Band Resync and Retry on Concurrency Conflict**:
   - *Violation*: Catching `ConcurrencyConflict` inside the store and blindly re-reading tip to re-apply writes.
   - *Remedy*: Surface `ConcurrencyConflict` immediately to the caller; the consumer must reload aggregate state and re-validate business invariants.
6. **Cross-Fiber Secondary Indexing or Queries**:
   - *Violation*: Building global event-kind indices or cross-fiber search queries into storage engines.
   - *Remedy*: Refuse cross-fiber queries in core (Spec C5.39). Projections belong in consumer-side read models.
7. **Unbounded Collections in Schemas**:
   - *Violation*: Using raw `Vec`, `String`, or `HashMap` in event schemas.
   - *Remedy*: Enforce `EventVec<T, MAX>`, `EventString`, and `NonEmptyEventString` via `#[derive(PardosaSchema)]`.

## Downstream Consumer Blueprint (Adapter Implementation Guide)

Any downstream consumer (such as `gh-report`) must structure its Pardosa integration as a dedicated adapter module adhering to four patterns:

### 1. Threading & Runtime Integration
Because Pardosa provides a synchronous facade and uses internal blocking bridges for async backends (such as NATS), calling Pardosa directly from an async Tokio worker thread can panic or starve the runtime (pardosa-jn1.14). The adapter must wrap all store interactions in `spawn_blocking`:

```rust
pub async fn append_event_async<E: EventSafe>(
    store: Arc<EventStore<E>>,
    fiber_id: FiberId,
    event: E,
) -> Result<EventId, AdapterError> {
    tokio::task::spawn_blocking(move || {
        store.append(fiber_id, event)
    })
    .await
    .map_err(|join_err| AdapterError::WorkerPanicked(join_err))?
    .map_err(AdapterError::from)
}
```

### 2. Bootstrap Orchestration
Consumers must not rely on `open_or_create`. They must write an explicit two-arm bootstrap match to handle first-run container creation safely:

```rust
pub fn bootstrap_store<E: EventSafe>(path: &Path) -> Result<EventStore<E>, AdapterError> {
    match EventStore::create(path) {
        Ok(store) => Ok(store),
        Err(err) if err.is_already_exists() => {
            EventStore::open(path).map_err(AdapterError::from)
        }
        Err(err) => Err(AdapterError::from(err)),
    }
}
```

### 3. Domain Mapping & Schema Definition
1. **Aggregate ID to Fiber ID**: Maintain an in-memory `FiberIndex<AggregateId>` within the adapter. On the first event for an aggregate, call `store.begin(initial_event)` to mint a fresh `FiberId` and register it in the index. For subsequent events, resolve `FiberId` from the index and call `store.append(fiber_id, next_event)`.
2. **Schema Definition**: Define events using a root enumeration annotated with `#[derive(PardosaSchema)]`, using `GenomeSafe` bounded types, explicit discriminants, and a tombstone variant:

```rust
#[derive(Debug, Clone, PartialEq, Eq, PardosaSchema)]
#[repr(u16)]
pub enum GhReportEvent {
    #[tombstone]
    Tombstone = 0,
    IssueOpened {
        issue_id: u64,
        title: EventString,
    } = 1,
    IssueClosed {
        issue_id: u64,
        closed_at: u64,
    } = 2,
}
```

### 4. Concurrency Conflict Handling
When a write encounters `FailureCondition::ConcurrencyConflict` or `FailureCondition::StaleEpoch`, the adapter must never retry blind in-band. The error indicates that another writer updated the stream or took ownership. The consumer must abort the current unit of work, reload its in-memory aggregate state from the store, re-run business logic validation, and re-attempt the write if still valid.
