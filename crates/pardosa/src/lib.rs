//! Pardosa foundation crate providing schema descriptors, codecs, container formats, and fiber lifecycle.
//!
//! # Positive Definitions
//!
//! Pardosa is a constrained foundation for deterministic, append-only, linearizable event storage.
//! It defines:
//! - **Containers and Frames**: Binary container format starting with 8-byte magic `PARDOSA\x01`,
//!   32-bit format version, rolling BLAKE3 commitment hash chains, and frame-level CRC32C verification.
//! - **Schema Descriptors and Identities**: Strongly-typed AST descriptors ([`schema::DescriptorNode`])
//!   and 32-byte BLAKE3 schema fingerprints ([`schema::SchemaIdentity`]) validating payload and envelope integrity.
//! - **Fiber Lifecycle and State Machine**: Strict five-state progression (`Initial`, `Admitted`, `Locked`,
//!   `Concluded`, `Retired`) governing event admission, cursor progression, and deterministic recovery.
//! - **Single-Writer Ownership Fencing**: Monotonic epochs, machine/boot/process identity vectors, and fencing
//!   guarantees ensuring at most one active writer session per artefact.
//! - **Migration and Cutover**: Online chase/freeze/cutover lifecycle with caller-selected migration policies
//!   and immutable predecessor/successor generation pointers.
//!
//! # Per-Condition Remedies
//!
//! When operations fail, Pardosa reports typed error conditions with unambiguous remedies:
//! - [`store::FailureCondition::UnreadableRecord`]: Frame CRC32C checksum or length check failed.
//!   Remedy: Verify storage media integrity; restore from backup or uncorrupted replica.
//! - [`store::FailureCondition::SchemaMismatch`]: Payload schema descriptor does not match expected schema identity.
//!   Remedy: Update consumer types to match the artefact's schema version or apply a migration.
//! - [`store::FailureCondition::EnvelopeMismatch`]: Container envelope format version or standard fields differ from expected.
//!   Remedy: Ensure client matches the container major format line (1.0).
//! - [`store::FailureCondition::StaleEpoch`]: Another writer acquired authority with a higher monotonic epoch.
//!   Remedy: Relinquish writer session; do not retry without re-establishing mutual exclusion.
//! - [`store::FailureCondition::OwnershipUnestablished`]: Writer session cannot conclusively prove current ownership.
//!   Remedy: Refresh ownership claim or inspect operator fence state before retrying.
//! - [`store::FailureCondition::TerminalFailure`]: Unrecoverable protocol or storage violation encountered.
//!   Remedy: Close the store; inspect error details and execute migration or rescue recovery per C4.7.
//!
//! # Truthful Seal Limits (S5)
//!
//! Per C4.24 and C6.35:
//! - Pardosa establishes that a schema descriptor was produced by one of the fixed producers recognized by this
//!   specification (the derive macro or internal shipped implementations).
//! - Crucially, whether a descriptor describes the events an artefact holds *faithfully* stands outside what
//!   Pardosa establishes. Pardosa states this boundary honestly wherever descriptors are consumed: structural
//!   validity and producer authenticity are verified, but runtime semantic fidelity of user fields cannot be
//!   proven by the storage engine.
//! - Storage adapter traits remain sealed (C4.9, C4.10): Only internal adapters are admitted at 1.0. Third parties
//!   establish conformance against public obligations rather than providing arbitrary external adapter implementations.
//!
//! # Shipped Producer Inventory (S10)
//!
//! Per C4.24, the set of schema descriptor producers recognized at 1.0 is fixed:
//! 1. `pardosa_derive::PardosaSchema` procedural macro published with Pardosa.
//! 2. Zero hand-written implementations within published crates (the inventory of hand-written producers is empty).
//!
//! Admitting any further producer is an addition within a major line.
//!
//! # Security and Maintenance Disclosure
//!
//! - **Single Maintainer (C6.32)**: Pardosa has one maintainer. Issue triage and support are provided on a
//!   best-effort basis without a committed response-time SLA.
//! - **Security Policy (C5.38)**: Report vulnerabilities privately via GitHub Security Advisories at
//!   `https://github.com/acje/pardosa/security/advisories` or contact `security@pardosa.dev`. There is no SLA for
//!   remedy delivery.
//! - **Non-Rust Advisory Monitoring (C5.38)**: Pardosa directly monitors advisories for its non-Rust dependency edges
//!   (e.g., cryptographic assembly and compression libraries).
//! - **Withdrawal Posture (C5.38)**: Published releases are withdrawn (yanked) only for correctness or safety defects.
//!
//! # Major-Line Read Limits
//!
//! Per C4.21, an artefact is read only by the major line that wrote it. Cross-major data export is the operator's
//! responsibility via the migration subsystem.

#![deny(missing_docs)]

pub mod encoding;
pub mod file;
pub mod migration;
pub mod prelude;
pub mod schema;
pub mod store;

pub use pardosa_derive::PardosaSchema;
