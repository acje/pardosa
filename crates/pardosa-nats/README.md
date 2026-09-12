# pardosa-nats

NATS JetStream storage adapter for [Pardosa](https://crates.io/crates/pardosa).

`pardosa-nats` implements Pardosa's deterministic append-only container storage abstractions over distributed NATS JetStream streams.

## Core Capabilities

- **Dual Stream Topology**: Maps each artefact stem to:
  - `{stem}_meta`: Key-value and message stream for ownership claims, monotonic epochs, and generation cutover pointers.
  - `{stem}_data`: Ordered, append-only event stream recording Pardosa container frames.
- **Single-Writer Fencing & OCC**: Enforces linearizable single-writer ownership using JetStream sequence expectations and monotonic epoch fences. Stale writes and concurrent writers are rejected deterministically via optimistic concurrency control (OCC).
- **Subject Namespaces**: Supports custom publish and routing subjects via `.with_subjects(meta_subject, data_subject)`, enabling flexible subject hierarchies, cluster partitions, and wildcard routing independent of stream names.
- **Synchronous Session Facade**: Bridges asynchronous JetStream operations into Pardosa's synchronous pipeline contracts via `NatsWriterSession` and `NatsReaderSession`.

## Usage Example

```rust
use pardosa::prelude::*;
use pardosa_nats::NatsStorageAdapter;

// Connect to NATS JetStream with custom subject routing
let adapter = NatsStorageAdapter::new("nats://127.0.0.1:4222", "tenant_orders")?
    .with_subjects("tenant.orders.meta", "tenant.orders.data");

// Create writer session with single-writer fencing epoch
let claim = OwnershipClaimRecord {
    epoch: 1,
    machine_id: [1u8; 16],
    boot_id: [2u8; 16],
    process_id: std::process::id() as u64,
    process_start_time_ns: 1_000_000,
    claim_time_ns: 2_000_000,
    operator_label: "service-worker".to_string(),
};
let mut writer = adapter.create(&claim)?;

// Derive deterministic fiber ID and append event
let fiber_id = derive_fiber_id("order-9876");
let event_id = [1u8; 16]; // 16-byte UUIDv7
let payload = b"{\"status\":\"confirmed\"}";
let verdict = writer.append_to_fiber(fiber_id, event_id, payload)?;
if let WriteLandingVerdict::Landed(envelope) = verdict {
    println!("Landed event ID {:?}", envelope.header.event_id);
}
```

## Error Handling & Remedies

When operations fail, typed error conditions indicate specific remedies:
- `StaleEpoch`: Writer session epoch was superseded by another claimant. Remedy: Relinquish session; renew exclusive lease before retrying.
- `StoreAlreadyExists`: Attempted exclusive create on an existing stream stem. Remedy: Use strict open instead of create, or specify a distinct artefact stem.
- `NoArtefactExists`: Artefact streams do not exist in JetStream. Remedy: Verify stream stem name and ensure artefact was initialized via create.
- `OwnershipRecordUnreadable`: Metadata stream read/write failed or timed out. Remedy: Check NATS cluster connectivity and JetStream health; retry.

## Cargo Features

- `default = []`
- `unstable-test-support`: Exposes internal test fixtures (activates `tempfile`).

## Security & Maintenance

- **Security Reporting**: Report vulnerabilities privately via GitHub Security Advisories at [https://github.com/acje/pardosa/security/advisories](https://github.com/acje/pardosa/security/advisories) or contact `security@pardosa.dev`.
- **Withdrawal Posture**: Published releases are yanked strictly for correctness or safety defects.

## License

Licensed under either of [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) or [MIT License](https://opensource.org/licenses/MIT) at your option.
