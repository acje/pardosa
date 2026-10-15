//! NATS JetStream storage adapter for Pardosa.
//!
//! Implements container storage abstractions over JetStream:
//! - Dual stream topologies mapping `{stem}_meta` and `{stem}_data` streams to hierarchical
//!   `{prefix}.meta` and `{prefix}.data` subjects derived from a validated [`NatsLocator`].
//! - Single-writer linearizable ownership via sequence expectations and monotonic epoch fences.
//! - Synchronous facade sessions ([`NatsReaderSession`], [`NatsWriterSession`]) over async JetStream.
//!
//! Deployments enforce a single-writer topology per stream stem; authority divergence marks
//! the session uncertain and halts further appends until external reconciliation.

#![deny(missing_docs)]
#![allow(clippy::pedantic)]

pub mod adapter;
#[cfg(any(test, feature = "unstable-test-support"))]
pub mod test_support;

pub use adapter::{NatsLocator, NatsReaderSession, NatsStorageAdapter, NatsWriterSession};
