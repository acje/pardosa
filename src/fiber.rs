use crate::event::Index;

/// Tracks the position and length of a fiber within the line.
///
/// Invariants: `len >= 1`, `current >= anchor`.
#[derive(Debug, Clone)]
pub struct Fiber {
    anchor: Index,
    len: u64,
    current: Index,
}

impl Fiber {
    /// Create a new fiber. `current` must be >= `anchor` (by value).
    pub fn new(anchor: Index, len: u64, current: Index) -> Self {
        debug_assert!(len >= 1, "fiber len must be >= 1");
        debug_assert!(
            current.value() >= anchor.value(),
            "current must be >= anchor"
        );
        Fiber {
            anchor,
            len,
            current,
        }
    }

    pub fn anchor(&self) -> Index {
        self.anchor
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn current(&self) -> Index {
        self.current
    }

    /// Update fiber after appending a new event at `new_current`.
    pub fn advance(&mut self, new_current: Index) {
        self.current = new_current;
        self.len += 1;
    }
}
