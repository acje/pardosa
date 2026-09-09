//! Pardosa foundation crate providing schema descriptors, codecs, and container formats.

#![deny(missing_docs)]

pub mod encoding;
pub mod file;
pub mod prelude;
pub mod schema;
pub mod store;

pub use pardosa_derive::PardosaSchema;
