use serde::{Deserialize, Serialize};
use std::fmt;

use crate::error::PardosaError;

/// Position in the append-only line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Index(u64);

impl Index {
    pub const ZERO: Index = Index(0);

    pub fn new(v: u64) -> Self {
        Index(v)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    /// Returns the next index, or `IndexOverflow` if at `u64::MAX`.
    pub fn checked_next(self) -> Result<Index, PardosaError> {
        self.0
            .checked_add(1)
            .map(Index)
            .ok_or(PardosaError::IndexOverflow)
    }
}

impl fmt::Display for Index {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a domain entity / fiber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DomainId(u64);

impl DomainId {
    pub fn new(v: u64) -> Self {
        DomainId(v)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    pub fn checked_next(self) -> Result<DomainId, PardosaError> {
        self.0
            .checked_add(1)
            .map(DomainId)
            .ok_or(PardosaError::DomainIdOverflow)
    }
}

impl fmt::Display for DomainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An immutable event in the append-only line.
///
/// - `timestamp`: Unix epoch in milliseconds.
/// - `detached`: `true` when this event records a soft-delete (Detach operation).
/// - `precursor`: Index of the previous event in the same fiber (`None` for the first event).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event<T> {
    pub timestamp: i64,
    pub domain_id: DomainId,
    pub detached: bool,
    pub precursor: Option<Index>,
    pub domain_event: T,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_checked_next() {
        let i = Index::new(0);
        assert_eq!(i.checked_next().unwrap().value(), 1);
    }

    #[test]
    fn index_overflow() {
        let i = Index::new(u64::MAX);
        assert!(i.checked_next().is_err());
    }

    #[test]
    fn domain_id_checked_next() {
        let d = DomainId::new(0);
        assert_eq!(d.checked_next().unwrap().value(), 1);
    }

    #[test]
    fn domain_id_overflow() {
        let d = DomainId::new(u64::MAX);
        assert!(d.checked_next().is_err());
    }

    #[test]
    fn index_roundtrip() {
        let i = Index::new(42);
        assert_eq!(i.value(), 42);
    }

    #[test]
    fn event_serde_roundtrip() {
        let event = Event {
            timestamp: 1700000000000,
            domain_id: DomainId::new(1),
            detached: false,
            precursor: None,
            domain_event: "created".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: Event<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.domain_id, event.domain_id);
        assert_eq!(back.domain_event, "created");
    }
}
