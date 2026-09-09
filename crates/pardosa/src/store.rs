//! Store runtime vocabulary and state machines.

/// State of a fiber within Pardosa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberState {
    /// Domain identifier has never existed.
    Undefined,
    /// Fiber is active and key exists.
    Defined,
    /// Fiber is soft-deleted.
    Detached,
    /// Fiber is purged from line and retained on optional audit trail.
    Purged,
    /// Fiber is locked from line and key cannot be reused.
    Locked,
}

/// Migration policy applied to a fiber during migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberMigrationPolicy {
    /// Keep fiber across migration.
    Keep,
    /// Purge fiber during migration.
    Purge,
    /// Lock and prune fiber during migration.
    LockAndPrune,
}

/// Rescue policy for locked fibers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockedRescuePolicy {
    /// Preserve audit trail upon rescue.
    PreserveAuditTrail,
    /// Accept data loss upon rescue.
    AcceptDataLoss,
}

/// Actions on a fiber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FiberAction {
    /// Create a new fiber.
    Create,
    /// Update an existing fiber.
    Update,
    /// Detach an existing fiber.
    Detach,
    /// Rescue a detached or locked fiber.
    Rescue,
    /// Migrate a fiber with the given policy.
    Migrate(FiberMigrationPolicy),
}
