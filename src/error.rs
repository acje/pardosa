use crate::event::DomainId;
use crate::fiber_state::{FiberAction, FiberState};

/// All errors produced by pardosa operations.
#[derive(Debug, thiserror::Error)]
pub enum PardosaError {
    #[error("invalid transition: state {state:?} + action {action:?}")]
    InvalidTransition {
        state: FiberState,
        action: FiberAction,
    },

    #[error("NATS connection unavailable")]
    NatsUnavailable,

    #[error("migration in progress — application operations rejected")]
    MigrationInProgress,

    #[error("Locked→Rescue requires acknowledge_data_loss = true")]
    AcknowledgmentRequired,

    #[error("domain ID {0:?} is not in Purged state — cannot reuse")]
    IdNotPurged(DomainId),

    #[error("domain ID {0:?} already exists")]
    IdAlreadyExists(DomainId),

    #[error("fiber not found for domain ID {0:?}")]
    FiberNotFound(DomainId),

    #[error("index overflow")]
    IndexOverflow,

    #[error("domain ID counter overflow")]
    DomainIdOverflow,
}
