//! Pardosa provides deterministic, append-only, linearizable event storage.
//!
//! # Architecture
//!
//! - **Containers**: Binary framing with BLAKE3 commitments and CRC32C verification.
//! - **Schemas**: Strongly typed AST descriptors and BLAKE3 schema fingerprints.
//! - **Fibers**: Five-state lifecycle governing event admission and deterministic recovery.
//! - **Fencing**: Monotonic epochs ensuring single-writer ownership.
//!
#![deny(missing_docs)]

pub mod encoding;
pub mod file;
pub mod migration;
pub mod prelude;
pub mod schema;
pub mod store;

pub use file::TornTailInfo;

#[cfg(any(test, feature = "unstable-test-support"))]
pub use file::QuotaWriter;

pub use pardosa_derive::{PardosaSchema, PardosaType};
