//! NATS JetStream storage adapter for Pardosa.

#![deny(missing_docs)]

pub mod adapter;
pub mod test_support;

pub use adapter::{NatsReaderSession, NatsStorageAdapter, NatsWriterSession};
