//! NATS JetStream storage adapter for Pardosa.
//!
//! # Positive Definitions
//!
//! Implements Pardosa's container storage abstractions on top of NATS JetStream:
//! - **Dual Stream Topologies**: Maps each artefact stem to `{stem}_meta` (key-value and message store for
//!   ownership records, epochs, and generation pointers) and `{stem}_data` (ordered append-only event stream).
//! - **Single-Writer Fencing**: Enforces linearizable ownership using JetStream sequence expectations and
//!   monotonic epoch fences.
//! - **Synchronous Session Bridge**: Implements synchronous facade sessions ([`NatsReaderSession`], [`NatsWriterSession`])
//!   over asynchronous JetStream operations.
//!
//! # Per-Condition Remedies
//!
//! When operations fail, typed error conditions indicate specific remedies:
//! - `StaleEpoch`: Writer session epoch was superseded by another claimant.
//!   Remedy: Relinquish session; do not retry writes without renewing exclusive lease.
//! - `StoreAlreadyExists`: Attempted exclusive create on an existing stream stem.
//!   Remedy: Use strict open instead of create, or specify a distinct artefact stem.
//! - `NoArtefactExists`: Artefact streams do not exist in JetStream.
//!   Remedy: Verify stream stem name and ensure artefact was initialized via create.
//! - `OwnershipRecordUnreadable`: Metadata stream read/write failed or timed out.
//!   Remedy: Check NATS cluster connectivity and JetStream availability; retry.
//!
//! # Truthful Seal Limits (S5)
//!
//! Per C4.9, C4.10, and C6.44, storage adapter traits in Pardosa are sealed. This crate provides the official
//! NATS JetStream adapter. Conformance against Pardosa's storage and exclusion invariants is verified by the
//! test suite; external implementations are not admitted at 1.0.
//!
//! # Shipped Producer Inventory (S10)
//!
//! This crate produces storage adapters, not schema descriptors. It relies exclusively on descriptors produced
//! by `pardosa` and `pardosa-derive`.
//!
//! # Security and Maintenance Disclosure
//!
//! - **Single Maintainer (C6.32)**: Maintained on a best-effort basis without SLA.
//! - **Security Reporting (C5.38)**: Disclose security vulnerabilities via GitHub Advisories at
//!   `https://github.com/acje/pardosa/security/advisories` or contact `security@pardosa.dev`.
//! - **Withdrawal Posture (C5.38)**: Yanks occur strictly for correctness or safety defects.
//!
//! # Major-Line Read Limits
//!
//! Per C4.21, an artefact is read only by the major line that wrote it. Cross-major data export is the operator's
//! responsibility via the migration subsystem.

#![deny(missing_docs)]

pub mod adapter;
pub mod test_support;

pub use adapter::{NatsReaderSession, NatsStorageAdapter, NatsWriterSession};
