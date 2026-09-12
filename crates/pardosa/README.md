# pardosa

Deterministic append-only event store foundation with schema descriptors, codecs, container formats, and fiber lifecycle management.

Pardosa implements [fiber semantics](https://github.com/acje/fiber-semantics) in Rust, enforcing event-driven correctness, auditability, linearizability, and deletion policy for Event Carried State Transfer (ECST).

Each domain entity's history is a **fiber** — a singly linked list of immutable events — interleaved into an append-only line. A strict per-fiber state machine governs lifecycle transitions.

## Architecture Overview

- **Containers and Frames**: Binary container format starting with 8-byte magic `PARDOSA\x01`, 32-bit format version, rolling BLAKE3 commitment chains, and frame-level CRC32C verification.
- **Schema Descriptors & Identities**: Strongly-typed AST descriptors (`schema::DescriptorNode`) and 32-byte BLAKE3 schema fingerprints (`schema::SchemaIdentity`) validating payload and envelope integrity.
- **Fiber Lifecycle & State Machine**: Strict 5-state progression (`Undefined`, `Defined`, `Detached`, `Purged`, `Locked`) with 10 legal transitions governing event admission, cursor progression, and deterministic recovery.
- **Single-Writer Ownership Fencing**: Monotonic epochs, machine/boot/process identity vectors, and fencing guarantees ensuring at most one active writer session per artefact.
- **Online Migration & Cutover**: Chase/freeze/cutover lifecycle with caller-selected migration policies (`Keep`, `Purge`, `LockAndPrune`) and immutable predecessor/successor generation pointers.

## Key Types & Primitives

- `StorageEngine`: Minimal I/O trait for storage drivers (`carried_epoch`, `check_authority`, `append_block`, `append_batch`, `read_all`).
- `FileEngine`: Reference filesystem implementation managing `.pgno` container storage with durable write/sync barriers.
- `Store<E: StorageEngine>`: Unified storage pipeline owning fiber handles, caching, and state verification (`open_writer`, `open_reader`, `fiber`, `append_batch`).
- `FiberHandle`: Handle to a specific entity fiber governing sequence numbers, parent event IDs, transitions, and appending events.
- `append_batch`: Batch write primitive on `StorageEngine`, `FileEngine`, and `Store` ensuring sequential rolling commitment progression across batched blocks. Writes in a batch are executed sequentially with per-item landing durability; if an error or ambiguity occurs mid-batch, prior blocks in the batch are known to have landed and are reported via `BatchLandingVerdict`. Callers reconcile landed prefixes without automatic ambiguous retry per C5.12 and C5.16.
- `OperationFailure`: Closed failure model with named `FailureCondition` variants and domain-neutral inspection predicates:
  - `is_already_exists()` (`FailureCondition::StoreAlreadyExists`)
  - `is_not_found()` (`FailureCondition::NoArtefactExists`)
  - `is_concurrency_conflict()` (`FailureCondition::ConcurrencyConflict`)
  - `is_stale_epoch()` (`FailureCondition::StaleEpoch`)

## Usage Example

```rust
use pardosa::prelude::*;

// Define container path and ownership claim
let adapter = FileStorageAdapter::new("data/orders");
let claim = OwnershipClaimRecord {
    epoch: 1,
    machine_id: [1u8; 16],
    boot_id: [2u8; 16],
    process_id: std::process::id() as u64,
    process_start_time_ns: 1_000_000,
    claim_time_ns: 2_000_000,
    operator_label: "service-worker".to_string(),
};

// Open or create writer session
let mut writer = adapter.create(&claim)?;

// Derive deterministic fiber ID from domain aggregate identity
let fiber_id = derive_fiber_id("ord-12345");
let event_id = [1u8; 16]; // 16-byte UUIDv7
let event_payload = b"{\"status\":\"created\"}";

// Append event to fiber
let verdict = writer.append_to_fiber(fiber_id, event_id, event_payload)?;
if let WriteLandingVerdict::Landed(envelope) = verdict {
    println!("Landed event ID {:?}", envelope.header.event_id);
}
```

## Error Handling & Per-Condition Remedies

Pardosa reports typed error conditions with unambiguous remedies:
- `PrecursorChainBroken`: Frame CRC32C checksum or length check failed. Remedy: Verify storage media integrity; restore from replica.
- `SchemaMismatch`: Payload descriptor does not match expected schema identity. Remedy: Update consumer types or apply a migration.
- `EnvelopeMismatch`: Container envelope format version differs from expected. Remedy: Ensure client matches major line.
- `StaleEpoch`: Another writer acquired authority with a higher monotonic epoch. Remedy: Relinquish session; re-establish mutual exclusion.
- `OwnershipUnestablished`: Writer session cannot prove current ownership. Remedy: Refresh ownership claim or inspect fence state.
- `InvariantBreakingConfiguration`: Unrecoverable storage violation encountered. Remedy: Close store; inspect details and execute rescue recovery.

## Cargo Features

- `default = ["uuid"]`: Enables UUID payload codecs.
- `uuid`: Enables `uuid::Uuid` type support.
- `nats`: Reserved feature flag for NATS ecosystem alignment (use `pardosa-nats` for the official JetStream adapter).
- `unstable-test-support`: Reserved feature flag for internal test fixtures.

## Security & Maintenance

- **Security Reporting**: Report vulnerabilities privately via GitHub Security Advisories at [https://github.com/acje/pardosa/security/advisories](https://github.com/acje/pardosa/security/advisories) or contact `security@pardosa.dev`.
- **Withdrawal Posture**: Published releases are yanked strictly for correctness or safety defects.

## License

Licensed under either of [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) or [MIT License](https://opensource.org/licenses/MIT) at your option.
