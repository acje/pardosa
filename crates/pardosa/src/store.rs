//! Store runtime vocabulary, admission logic, and state machines.

use crate::encoding::{
    EnvelopeIdentity, EventEnvelope, OwnershipClaimRecord, Uuid, ValueConstraint,
};
use crate::schema::{SchemaDescriptor, SchemaIdentity};
use std::fmt;

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

/// External name for an artefact's dragline per C6.5.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtefactLocator(String);

impl ArtefactLocator {
    /// Creates a new artefact locator from a string name.
    #[must_use]
    pub fn new(locator: impl Into<String>) -> Self {
        Self(locator.into())
    }

    /// Returns the string slice of the locator.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ArtefactLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ArtefactLocator {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for ArtefactLocator {
    fn from(s: String) -> Self {
        Self::new(s)
    }
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

/// Typed lifecycle transition attempted on a fiber per C6.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptedTransition {
    /// Creation transition (Undefined -> Defined, Purged -> Defined).
    Create,
    /// Update transition (Defined -> Defined).
    Update,
    /// Detach transition (Defined -> Detached).
    Detach,
    /// Rescue transition (Detached -> Defined, Locked -> Defined).
    Rescue,
    /// Migration transition with selected migration policy.
    Migrate(FiberMigrationPolicy),
}

impl fmt::Display for AttemptedTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create => write!(f, "create"),
            Self::Update => write!(f, "update"),
            Self::Detach => write!(f, "detach"),
            Self::Rescue => write!(f, "rescue"),
            Self::Migrate(policy) => write!(f, "migrate({:?})", policy),
        }
    }
}

/// Error returned when a transition is rejected by the fiber lifecycle state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IllegalStateTransition {
    /// Source state before the attempted transition.
    pub from: FiberState,
    /// Transition that was rejected.
    pub attempted: AttemptedTransition,
}

impl fmt::Display for IllegalStateTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "illegal fiber state transition from {:?} via {}",
            self.from, self.attempted
        )
    }
}

impl std::error::Error for IllegalStateTransition {}

impl FiberState {
    /// Applies a create transition per C6.1 (Undefined -> Defined, Purged -> Defined).
    ///
    /// # Errors
    ///
    /// Returns [`IllegalStateTransition`] if the transition is not permitted.
    pub fn create(&self) -> Result<Self, IllegalStateTransition> {
        self.apply_action(AttemptedTransition::Create)
    }

    /// Applies an update transition per C6.1 (Defined -> Defined).
    ///
    /// # Errors
    ///
    /// Returns [`IllegalStateTransition`] if the transition is not permitted.
    pub fn update(&self) -> Result<Self, IllegalStateTransition> {
        self.apply_action(AttemptedTransition::Update)
    }

    /// Applies a detach transition per C6.1 (Defined -> Detached).
    ///
    /// # Errors
    ///
    /// Returns [`IllegalStateTransition`] if the transition is not permitted.
    pub fn detach(&self) -> Result<Self, IllegalStateTransition> {
        self.apply_action(AttemptedTransition::Detach)
    }

    /// Applies a rescue transition per C6.1 (Detached -> Defined, Locked -> Defined).
    ///
    /// # Errors
    ///
    /// Returns [`IllegalStateTransition`] if the transition is not permitted.
    pub fn rescue(&self) -> Result<Self, IllegalStateTransition> {
        self.apply_action(AttemptedTransition::Rescue)
    }

    /// Applies a migration transition with the given policy per C6.1.
    ///
    /// # Errors
    ///
    /// Returns [`IllegalStateTransition`] if the transition is not permitted.
    pub fn migrate(&self, policy: FiberMigrationPolicy) -> Result<Self, IllegalStateTransition> {
        self.apply_action(AttemptedTransition::Migrate(policy))
    }

    pub(crate) fn apply_action(
        &self,
        action: AttemptedTransition,
    ) -> Result<Self, IllegalStateTransition> {
        match (self, &action) {
            (Self::Undefined, AttemptedTransition::Create) => Ok(Self::Defined),
            (Self::Defined, AttemptedTransition::Update) => Ok(Self::Defined),
            (Self::Defined, AttemptedTransition::Detach) => Ok(Self::Detached),
            (Self::Detached, AttemptedTransition::Rescue) => Ok(Self::Defined),
            (Self::Detached, AttemptedTransition::Migrate(FiberMigrationPolicy::Keep)) => {
                Ok(Self::Detached)
            }
            (Self::Detached, AttemptedTransition::Migrate(FiberMigrationPolicy::LockAndPrune)) => {
                Ok(Self::Locked)
            }
            (Self::Detached, AttemptedTransition::Migrate(FiberMigrationPolicy::Purge)) => {
                Ok(Self::Purged)
            }
            (Self::Locked, AttemptedTransition::Rescue) => Ok(Self::Defined),
            (Self::Locked, AttemptedTransition::Migrate(FiberMigrationPolicy::Purge)) => {
                Ok(Self::Purged)
            }
            (Self::Purged, AttemptedTransition::Create) => Ok(Self::Defined),
            _ => Err(IllegalStateTransition {
                from: *self,
                attempted: action,
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn can_apply_action(&self, action: &AttemptedTransition) -> bool {
        self.apply_action(*action).is_ok()
    }

    /// Rescues a locked fiber with the specified rescue policy per C6.1 and C6.19.
    ///
    /// # Errors
    ///
    /// Returns [`IllegalStateTransition`] if the fiber is not in `Locked` state.
    pub fn rescue_locked(
        &self,
        _policy: LockedRescuePolicy,
    ) -> Result<Self, IllegalStateTransition> {
        match self {
            Self::Locked => Ok(Self::Defined),
            _ => Err(IllegalStateTransition {
                from: *self,
                attempted: AttemptedTransition::Rescue,
            }),
        }
    }

    /// Returns `true` if this state is permissible when reopening an existing store per C6.2.
    #[must_use]
    pub fn is_reopened_valid(&self) -> bool {
        match self {
            Self::Locked => false,
            Self::Undefined | Self::Defined | Self::Detached | Self::Purged => true,
        }
    }
}

/// Reopened fiber state admissible on a reopened artefact per C6.2.
///
/// Structurally excludes `Locked` state which is reachable only within a running migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReopenedFiberState {
    /// Domain identifier has never existed.
    Undefined,
    /// Fiber is active and key exists.
    Defined,
    /// Fiber is soft-deleted.
    Detached,
    /// Fiber is purged from line and retained on optional audit trail.
    Purged,
}

impl ReopenedFiberState {
    /// Converts this reopened fiber state to the general 5-state [`FiberState`].
    #[must_use]
    pub fn to_fiber_state(self) -> FiberState {
        match self {
            Self::Undefined => FiberState::Undefined,
            Self::Defined => FiberState::Defined,
            Self::Detached => FiberState::Detached,
            Self::Purged => FiberState::Purged,
        }
    }
}

/// Validated state boundary for a reopened store per C6.2.
///
/// Enforces that a reopened artefact yields no locked fibers, steady migration mode,
/// and an empty set of removed fiber identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReopenedStoreBoundary {
    fiber_states: Vec<(Uuid, ReopenedFiberState)>,
    mode: MigrationMode,
}

impl ReopenedStoreBoundary {
    /// Validates and constructs a reopened store boundary per C6.2.
    ///
    /// # Errors
    ///
    /// - [`IllegalOpenCombination::LockedStateOnReopen`] if any fiber is in `Locked` state.
    /// - [`IllegalOpenCombination::MigratingModeOnReopen`] if the artefact is in `Migrating` mode.
    /// - [`IllegalOpenCombination::RemovedIdentitiesOnReopen`] if removed fiber identities are non-empty.
    pub fn validate(
        raw_fiber_states: &[(Uuid, FiberState)],
        mode: MigrationMode,
        removed_fiber_identities: &[Uuid],
    ) -> Result<Self, IllegalOpenCombination> {
        if mode == MigrationMode::Migrating {
            return Err(IllegalOpenCombination::MigratingModeOnReopen);
        }
        if !removed_fiber_identities.is_empty() {
            return Err(IllegalOpenCombination::RemovedIdentitiesOnReopen);
        }
        let mut reopened_states = Vec::with_capacity(raw_fiber_states.len());
        for (id, state) in raw_fiber_states {
            let reopened_state = match state {
                FiberState::Locked => return Err(IllegalOpenCombination::LockedStateOnReopen),
                FiberState::Undefined => ReopenedFiberState::Undefined,
                FiberState::Defined => ReopenedFiberState::Defined,
                FiberState::Detached => ReopenedFiberState::Detached,
                FiberState::Purged => ReopenedFiberState::Purged,
            };
            reopened_states.push((*id, reopened_state));
        }
        Ok(Self {
            fiber_states: reopened_states,
            mode: MigrationMode::Steady,
        })
    }

    /// Returns the validated reopened fiber states.
    #[must_use]
    pub fn fiber_states(&self) -> &[(Uuid, ReopenedFiberState)] {
        &self.fiber_states
    }

    /// Returns the migration mode (always `Steady` on reopen per C6.2).
    #[must_use]
    pub fn mode(&self) -> MigrationMode {
        self.mode
    }
}

/// Dragline-local resume cursor valid by construction per C5.22.
///
/// Cursors cannot be constructed with arbitrary unanchored values,
/// cannot be forged, cannot be mixed across distinct draglines or reader
/// sessions at compile-time, and are regenerated during migrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeCursor<'brand> {
    position: u64,
    event_id: [u8; 16],
    locator: ArtefactLocator,
    _brand: std::marker::PhantomData<fn(&'brand ()) -> &'brand ()>,
}

impl<'brand> ResumeCursor<'brand> {
    /// Returns the stream position of the cursor.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Returns the event identifier at this cursor position.
    #[must_use]
    pub fn event_id(&self) -> &[u8; 16] {
        &self.event_id
    }

    /// Returns the locator of the artefact this cursor belongs to.
    #[must_use]
    pub fn locator(&self) -> &ArtefactLocator {
        &self.locator
    }
}

/// Maximum active tracked fibers per reader session per C5.22 and C6.7.
pub const MAX_ACTIVE_FIBERS: usize = 65_536;

/// Maximum stream items ingested per reader session per C5.22.
pub const MAX_STREAM_ITEMS: usize = 65_536;

/// Maximum queued stream payload bytes tracked per reader session.
///
/// # Resource Contract
/// - Boundary: Per-reader session in-memory queued stream.
/// - Accounting: Formula covers per-reader queued payload capacities plus a nominal 85-byte header charge per envelope. This numerical limit (64 MiB) is an implementation policy constraint (Moltke policy per pardosa-3hek.10 / pardosa-3hek.12), not an architectural limit defined by C5.22.
/// - Exclusions: Excludes reader-owned Vec/VecDeque container slot allocations and spare capacity, BTreeMap index nodes, current wire frame and transient frame/decode overlap during `with_frames`, caller iterator internals, locators, returned results, standard library allocator/runtime overhead, and concurrent readers.
/// - Lifetimes: On terminal error or capacity refusal, unread payload allocations are immediately dropped via `stream.clear()` and `retained_bytes` is reset to 0, while VecDeque backing capacity and index nodes remain allocated until the reader session is dropped.
pub const MAX_STREAM_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct FiberTracking {
    last_event_id: [u8; 16],
    last_commitment_hash: [u8; 32],
}

#[derive(Debug, PartialEq, Eq)]
struct ReaderState {
    observed_position: u64,
    fibers: std::collections::BTreeMap<[u8; 16], FiberTracking>,
    observed_events: std::collections::BTreeMap<[u8; 16], [u8; 16]>,
    stream: std::collections::VecDeque<crate::encoding::EventEnvelope>,
    terminally_failed: bool,
    capacity_refused: bool,
    byte_capacity_refused: bool,
    retained_bytes: usize,
}

impl ReaderState {
    fn mark_terminally_failed(&mut self) {
        self.terminally_failed = true;
        self.stream.clear();
        self.retained_bytes = 0;
    }
}

/// Branded event observation from an opened reader session per C5.22.
///
/// Observations can only be produced by an active [`ArtefactReader`] observing
/// a valid genesis or chained event envelope.
#[derive(Debug, PartialEq, Eq)]
pub struct ReaderObservation<'brand> {
    position: u64,
    event_id: [u8; 16],
    fiber_id: [u8; 16],
    locator: ArtefactLocator,
    is_genesis: bool,
    _brand: std::marker::PhantomData<fn(&'brand ()) -> &'brand ()>,
}

impl<'brand> ReaderObservation<'brand> {
    /// Mints a dragline-local resume cursor bound to this observation per C5.22.
    #[must_use]
    pub fn cursor(&self) -> ResumeCursor<'brand> {
        ResumeCursor {
            position: self.position,
            event_id: self.event_id,
            locator: self.locator.clone(),
            _brand: std::marker::PhantomData,
        }
    }

    /// Returns the stream position of this observation.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Returns the event identifier of this observation.
    #[must_use]
    pub fn event_id(&self) -> &[u8; 16] {
        &self.event_id
    }

    /// Returns the fiber identifier of this observation.
    #[must_use]
    pub fn fiber_id(&self) -> &[u8; 16] {
        &self.fiber_id
    }

    /// Returns the locator of the artefact this observation was made in.
    #[must_use]
    pub fn locator(&self) -> &ArtefactLocator {
        &self.locator
    }

    /// Returns whether this observation represents a genesis event.
    #[must_use]
    pub fn is_genesis(&self) -> bool {
        self.is_genesis
    }
}

/// Reader session bound to an opened artefact with a generative brand per C5.22.
///
/// Not [`Clone`] to prevent desynchronized cloned counters.
#[derive(Debug, PartialEq, Eq)]
pub struct ArtefactReader<'brand> {
    locator: ArtefactLocator,
    state: std::cell::RefCell<ReaderState>,
    _brand: std::marker::PhantomData<fn(&'brand ()) -> &'brand ()>,
}

impl ArtefactReader<'_> {
    /// Opens a scoped reader session with a generative brand per C5.22 and C6.5.
    ///
    /// The rank-2 closure parameter ensures that the lifetime `'brand` is unique
    /// to this session invocation and cannot be unified with any other session.
    pub fn with_reader<L, F, R>(locator: L, f: F) -> R
    where
        L: Into<ArtefactLocator>,
        F: for<'brand> FnOnce(ArtefactReader<'brand>) -> R,
    {
        Self::with_stream(locator, std::iter::empty(), f)
    }

    /// Opens a scoped reader session with an event envelope stream per C5.22.
    ///
    /// Ingestion is eager: envelopes are drawn from the provided stream during session setup up
    /// to [`MAX_STREAM_ITEMS`] and [`MAX_STREAM_BYTES`]. The input iterator is dropped before
    /// invoking `f`.
    ///
    /// # Resource Contract
    /// - Boundary: Per-reader session in-memory queued stream.
    /// - Accounting: Formula covers per-reader queued payload capacities plus a nominal 85-byte header charge per envelope across all currently queued envelopes.
    /// - Exclusions: Excludes reader-owned Vec/VecDeque container slot allocations and spare capacity, BTreeMap index nodes, current wire frame and transient frame/decode overlap during `with_frames`, caller iterator internals, locators, returned results, standard library allocator/runtime overhead, and concurrent readers.
    /// - Lifetimes: On terminal error or capacity refusal, unread payload allocations are immediately dropped via `stream.clear()` and `retained_bytes` is reset to 0, while VecDeque backing capacity and index nodes remain allocated until the reader session is dropped.
    ///
    /// If item count exceeds [`MAX_STREAM_ITEMS`] or total retained capacity exceeds [`MAX_STREAM_BYTES`]
    /// (Moltke implementation policy), ingestion stops, unread payload allocations are immediately dropped,
    /// and the refusal is recorded in reader state; the first call to [`read_event`](Self::read_event)
    /// will return [`FailureCondition::ValueConstraintViolated`] with [`ValueConstraint::TooLong`]
    /// and enter a terminal failure state.
    ///
    /// The rank-2 closure parameter ensures that the lifetime `'brand` is unique
    /// to this session invocation and cannot be unified with any other session.
    pub fn with_stream<L, I, F, R>(locator: L, stream: I, f: F) -> R
    where
        L: Into<ArtefactLocator>,
        I: IntoIterator<Item = crate::encoding::EventEnvelope>,
        F: for<'brand> FnOnce(ArtefactReader<'brand>) -> R,
    {
        let mut iter = stream.into_iter();
        let mut stream_deque = std::collections::VecDeque::new();
        let mut capacity_refused = false;
        let mut byte_capacity_refused = false;
        let mut retained_bytes: usize = 0;
        let mut count: usize = 0;

        for env in iter.by_ref() {
            count = count.saturating_add(1);
            if count <= MAX_STREAM_ITEMS {
                let env_size = match 85usize.checked_add(env.payload.capacity()) {
                    Some(s) => s,
                    None => {
                        byte_capacity_refused = true;
                        break;
                    }
                };
                match retained_bytes.checked_add(env_size) {
                    Some(new_retained) if new_retained <= MAX_STREAM_BYTES => {
                        retained_bytes = new_retained;
                        stream_deque.push_back(env);
                    }
                    _ => {
                        byte_capacity_refused = true;
                        break;
                    }
                }
            } else {
                capacity_refused = true;
                break;
            }
        }
        drop(iter);

        let mut reader_state = ReaderState {
            observed_position: 0,
            fibers: std::collections::BTreeMap::new(),
            observed_events: std::collections::BTreeMap::new(),
            stream: stream_deque,
            terminally_failed: false,
            capacity_refused,
            byte_capacity_refused,
            retained_bytes,
        };
        if reader_state.capacity_refused || reader_state.byte_capacity_refused {
            reader_state.mark_terminally_failed();
        }

        let artefact_reader = ArtefactReader {
            locator: locator.into(),
            state: std::cell::RefCell::new(reader_state),
            _brand: std::marker::PhantomData,
        };
        f(artefact_reader)
    }

    /// Opens a scoped reader session from framed container entries per C5.22 and C4.19.
    ///
    /// Decodes each container frame payload into an event envelope.
    ///
    /// # Resource Contract
    /// Governed by the [`with_stream`](Self::with_stream) queued payload capacity contract.
    /// Transient decode overlap between the current wire frame payload allocation and the
    /// newly decoded envelope payload during `with_frames` is excluded from the queued accounting formula.
    ///
    /// # Errors
    ///
    /// Returns [`OperationFailure`] if frame item count exceeds [`MAX_STREAM_ITEMS`],
    /// byte count exceeds [`MAX_STREAM_BYTES`], or any frame payload fails envelope decoding or CRC validation.
    pub fn with_frames<L, I, F, R>(locator: L, frames: I, f: F) -> Result<R, OperationFailure>
    where
        L: Into<ArtefactLocator>,
        I: IntoIterator<Item = crate::file::ContainerFrame>,
        F: for<'brand> FnOnce(ArtefactReader<'brand>) -> R,
    {
        let mut envelopes = Vec::new();
        let mut frame_count: usize = 0;
        let mut total_bytes: usize = 0;

        for frame in frames {
            frame_count = match frame_count.checked_add(1) {
                Some(c) => c,
                None => {
                    return Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: crate::encoding::ValueConstraint::TooLong,
                        },
                        "frame item limit exceeded",
                    ));
                }
            };
            if frame_count > MAX_STREAM_ITEMS {
                return Err(OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: crate::encoding::ValueConstraint::TooLong,
                    },
                    "frame item limit exceeded",
                ));
            }
            match total_bytes.checked_add(frame.payload.len()) {
                Some(b) if b <= MAX_STREAM_BYTES => (),
                _ => {
                    return Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: crate::encoding::ValueConstraint::TooLong,
                        },
                        "frame byte limit exceeded",
                    ));
                }
            }
            let computed = crc32c::crc32c(&frame.payload);
            if frame.checksum != computed {
                return Err(OperationFailure::new(
                    FailureCondition::EnvelopeMismatch,
                    format!(
                        "frame decode error: {}",
                        crate::encoding::DecodeError::ChecksumMismatch {
                            computed,
                            recorded: frame.checksum,
                        }
                    ),
                ));
            }
            let (env, consumed) = match crate::encoding::EventEnvelope::decode(&frame.payload) {
                Ok(v) => v,
                Err(err) => {
                    return Err(OperationFailure::new(
                        FailureCondition::EnvelopeMismatch,
                        format!("frame decode error: {err}"),
                    ));
                }
            };
            if consumed != frame.payload.len() {
                return Err(OperationFailure::new(
                    FailureCondition::EnvelopeMismatch,
                    format!(
                        "frame decode error: {}",
                        crate::encoding::DecodeError::TruncatedPayload {
                            expected: frame.payload.len(),
                            available: consumed,
                        }
                    ),
                ));
            }
            let env_size = match 85usize.checked_add(env.payload.capacity()) {
                Some(s) => s,
                None => {
                    return Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: crate::encoding::ValueConstraint::TooLong,
                        },
                        "frame byte limit exceeded",
                    ));
                }
            };
            total_bytes = match total_bytes.checked_add(env_size) {
                Some(b) if b <= MAX_STREAM_BYTES => b,
                _ => {
                    return Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: crate::encoding::ValueConstraint::TooLong,
                        },
                        "frame byte limit exceeded",
                    ));
                }
            };
            envelopes.push(env);
        }
        Ok(Self::with_stream(locator, envelopes, f))
    }
}

impl<'brand> ArtefactReader<'brand> {
    /// Returns the locator of the opened artefact.
    #[must_use]
    pub fn locator(&self) -> &ArtefactLocator {
        &self.locator
    }

    /// Reads and observes the next event envelope from the reader session stream per C5.22.
    ///
    /// Consumes envelopes from the authorized stream, decrements retained byte capacity accounting
    /// by 85 bytes header overhead plus `payload.capacity()`, validates genesis or precursor link
    /// integrity against session fiber state and BLAKE3 commitment history, and mints a
    /// branded observation.
    ///
    /// # Resource Contract
    /// - Boundary: Per-reader session in-memory queued stream.
    /// - Accounting: Formula covers per-reader queued payload capacities plus a nominal 85-byte header charge per envelope.
    /// - Exclusions: Excludes reader-owned Vec/VecDeque container slot allocations and spare capacity, BTreeMap index nodes, current wire frame and transient frame/decode overlap during `with_frames`, caller iterator internals, locators, returned results, standard library allocator/runtime overhead, and concurrent readers.
    /// - Lifetimes: On terminal error or capacity refusal, unread payload allocations are immediately dropped via `stream.clear()` and `retained_bytes` is reset to 0, while VecDeque backing capacity and index nodes remain allocated until the reader session is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`OperationFailure`] with [`FailureCondition::PrecursorChainBroken`] if precursor
    /// link fields, fiber continuity, or commitment hashes are broken or invalid.
    /// Returns [`OperationFailure`] with [`FailureCondition::ValueConstraintViolated`] if:
    /// - Stream item count exceeded [`MAX_STREAM_ITEMS`] during eager intake,
    /// - Stream retained byte capacity exceeded [`MAX_STREAM_BYTES`] (Moltke policy) during eager intake,
    /// - Active tracked fibers exceed [`MAX_ACTIVE_FIBERS`] per C6.7, or
    /// - The observed position counter overflows [`u64::MAX`].
    pub fn read_event(&self) -> Option<Result<ReaderObservation<'brand>, OperationFailure>> {
        let envelope = {
            let mut state = self.state.borrow_mut();
            if state.capacity_refused {
                state.mark_terminally_failed();
                state.capacity_refused = false;
                return Some(Err(OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: crate::encoding::ValueConstraint::TooLong,
                    },
                    "stream item limit exceeded",
                )));
            }
            if state.byte_capacity_refused {
                state.mark_terminally_failed();
                state.byte_capacity_refused = false;
                return Some(Err(OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: crate::encoding::ValueConstraint::TooLong,
                    },
                    "stream byte limit exceeded",
                )));
            }
            if state.terminally_failed {
                return None;
            }
            let env = state.stream.pop_front()?;
            let env_size = 85usize.saturating_add(env.payload.capacity());
            state.retained_bytes = state.retained_bytes.saturating_sub(env_size);
            if state.observed_events.contains_key(&env.header.event_id) {
                state.mark_terminally_failed();
                return Some(Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    "duplicate event ID observed across reader session per C5.61",
                )));
            }
            env
        };

        if envelope.header.precursor == [0u8; 16] {
            if envelope.header.precursor_hash != [0u8; 32] {
                let mut state = self.state.borrow_mut();
                state.mark_terminally_failed();
                return Some(Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    "genesis envelope has non-zero precursor commitment hash per C4.19",
                )));
            }
            {
                let state = self.state.borrow();
                if state.fibers.len() >= MAX_ACTIVE_FIBERS {
                    drop(state);
                    let mut state = self.state.borrow_mut();
                    state.mark_terminally_failed();
                    return Some(Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: crate::encoding::ValueConstraint::TooLong,
                        },
                        "active tracked fibers capacity exceeded per C6.7",
                    )));
                }
                if state.fibers.contains_key(&envelope.header.fiber_id) {
                    drop(state);
                    let mut state = self.state.borrow_mut();
                    state.mark_terminally_failed();
                    return Some(Err(OperationFailure::new(
                        FailureCondition::PrecursorChainBroken(None),
                        "duplicate genesis on same fiber per C5.22",
                    )));
                }
            }
            let obs = match self.observe_genesis(&envelope) {
                Some(o) => o,
                None => {
                    let mut state = self.state.borrow_mut();
                    state.mark_terminally_failed();
                    return Some(Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: crate::encoding::ValueConstraint::TooLong,
                        },
                        "observed position counter overflow",
                    )));
                }
            };
            Some(Ok(obs))
        } else {
            {
                let state = self.state.borrow();
                let precursor_fiber = state.observed_events.get(&envelope.header.precursor);
                match precursor_fiber {
                    None => {
                        drop(state);
                        let mut state = self.state.borrow_mut();
                        state.mark_terminally_failed();
                        return Some(Err(OperationFailure::new(
                            FailureCondition::PrecursorChainBroken(Some(
                                CausalChainError::PrecursorOutOfRange,
                            )),
                            "precursor event ID is not resident in reader history per C6.12",
                        )));
                    }
                    Some(&prev_fiber) if prev_fiber != envelope.header.fiber_id => {
                        drop(state);
                        let mut state = self.state.borrow_mut();
                        state.mark_terminally_failed();
                        return Some(Err(OperationFailure::new(
                            FailureCondition::PrecursorChainBroken(Some(
                                CausalChainError::PrecursorWrongFiber,
                            )),
                            "precursor event belongs to another resident fiber per C6.12",
                        )));
                    }
                    Some(_) => {
                        let fiber = match state.fibers.get(&envelope.header.fiber_id) {
                            Some(f) => f,
                            None => {
                                drop(state);
                                let mut state = self.state.borrow_mut();
                                state.mark_terminally_failed();
                                return Some(Err(OperationFailure::new(
                                    FailureCondition::PrecursorChainBroken(Some(
                                        CausalChainError::PrecursorOutOfRange,
                                    )),
                                    "fiber has no observed genesis in this artefact per C6.12",
                                )));
                            }
                        };
                        if fiber.last_event_id != envelope.header.precursor {
                            drop(state);
                            let mut state = self.state.borrow_mut();
                            state.mark_terminally_failed();
                            return Some(Err(OperationFailure::new(
                                FailureCondition::PrecursorChainBroken(None),
                                "precursor is a stale non-immediate event on the same fiber per C5.22",
                            )));
                        }
                        if fiber.last_commitment_hash != envelope.header.precursor_hash {
                            drop(state);
                            let mut state = self.state.borrow_mut();
                            state.mark_terminally_failed();
                            return Some(Err(OperationFailure::new(
                                FailureCondition::PrecursorChainBroken(None),
                                "precursor commitment hash does not match predecessor commitment per C5.40",
                            )));
                        }
                    }
                }
            }
            let obs = match self.observe_event(&envelope) {
                Some(o) => o,
                None => {
                    let mut state = self.state.borrow_mut();
                    state.mark_terminally_failed();
                    return Some(Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: crate::encoding::ValueConstraint::TooLong,
                        },
                        "observed position counter overflow",
                    )));
                }
            };
            Some(Ok(obs))
        }
    }

    #[must_use]
    pub(crate) fn observe_genesis(
        &self,
        envelope: &crate::encoding::EventEnvelope,
    ) -> Option<ReaderObservation<'brand>> {
        if envelope.header.precursor != [0u8; 16] || envelope.header.precursor_hash != [0u8; 32] {
            return None;
        }
        let mut state = self.state.borrow_mut();
        if state
            .observed_events
            .contains_key(&envelope.header.event_id)
        {
            return None;
        }
        if state.fibers.len() >= MAX_ACTIVE_FIBERS {
            return None;
        }
        if state.fibers.contains_key(&envelope.header.fiber_id) {
            return None;
        }
        let position = state.observed_position;
        let next_pos = state.observed_position.checked_add(1)?;
        state.observed_position = next_pos;
        let commitment_hash =
            crate::encoding::compute_envelope_commitment(&envelope.header, &envelope.payload);
        state.fibers.insert(
            envelope.header.fiber_id,
            FiberTracking {
                last_event_id: envelope.header.event_id,
                last_commitment_hash: commitment_hash,
            },
        );
        state
            .observed_events
            .insert(envelope.header.event_id, envelope.header.fiber_id);
        Some(ReaderObservation {
            position,
            event_id: envelope.header.event_id,
            fiber_id: envelope.header.fiber_id,
            locator: self.locator.clone(),
            is_genesis: true,
            _brand: std::marker::PhantomData,
        })
    }

    #[must_use]
    pub(crate) fn observe_event(
        &self,
        envelope: &crate::encoding::EventEnvelope,
    ) -> Option<ReaderObservation<'brand>> {
        if envelope.header.precursor == [0u8; 16] {
            return None;
        }
        let mut state = self.state.borrow_mut();
        if state
            .observed_events
            .contains_key(&envelope.header.event_id)
        {
            return None;
        }
        {
            let fiber = state.fibers.get(&envelope.header.fiber_id)?;
            if fiber.last_event_id != envelope.header.precursor {
                return None;
            }
            if fiber.last_commitment_hash != envelope.header.precursor_hash {
                return None;
            }
        }
        let position = state.observed_position;
        let next_pos = state.observed_position.checked_add(1)?;
        state.observed_position = next_pos;
        let commitment_hash =
            crate::encoding::compute_envelope_commitment(&envelope.header, &envelope.payload);
        let fiber = state.fibers.get_mut(&envelope.header.fiber_id)?;
        fiber.last_event_id = envelope.header.event_id;
        fiber.last_commitment_hash = commitment_hash;
        state
            .observed_events
            .insert(envelope.header.event_id, envelope.header.fiber_id);
        Some(ReaderObservation {
            position,
            event_id: envelope.header.event_id,
            fiber_id: envelope.header.fiber_id,
            locator: self.locator.clone(),
            is_genesis: false,
            _brand: std::marker::PhantomData,
        })
    }

    /// Resumes reading from a cursor issued by this reader session.
    #[must_use]
    pub fn resume_from(&self, cursor: &ResumeCursor<'brand>) -> u64 {
        cursor.position
    }

    /// Regenerates a cursor for a migration target dragline per C5.22.
    ///
    /// Cursors have no meaning across migrations and are regenerated for
    /// the target reader session at the target observation's stream position from a
    /// validated target genesis observation.
    /// Requires distinct source cursor (different locator) and returns `None`
    /// if the target observation is not genesis or if attempted within the same session.
    #[must_use]
    pub fn regenerate_cursor_for_migration(
        &self,
        source_cursor: &ResumeCursor<'_>,
        target_observation: &ReaderObservation<'brand>,
    ) -> Option<ResumeCursor<'brand>> {
        if self.locator == source_cursor.locator {
            return None;
        }
        if !target_observation.is_genesis {
            return None;
        }
        if target_observation.event_id == source_cursor.event_id {
            return None;
        }
        Some(target_observation.cursor())
    }
}

/// Closed precursor-walk error sub-domain per C6.12 and C6.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalChainError {
    /// Recorded precursor event ID is outside the artefact.
    PrecursorOutOfRange,
    /// Recorded precursor event belongs to another fiber.
    PrecursorWrongFiber,
}

impl fmt::Display for CausalChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrecursorOutOfRange => write!(f, "precursor event ID is outside the artefact"),
            Self::PrecursorWrongFiber => write!(f, "precursor event belongs to another fiber"),
        }
    }
}

impl std::error::Error for CausalChainError {}

/// Closed death-proof sub-domain per C6.7 and C2.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathProof {
    /// Machine hosting the owner rebooted.
    MachineReboot,
    /// Process hosting the owner is absent.
    ProcessAbsence,
    /// Operating system process ID has been reused by another process.
    ProcessIdReuse,
}

/// Closed liveness sub-domain per C6.7 and C2.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessVerdict {
    /// Owner is proven dead with supporting proof.
    ProvenDead {
        /// Supporting proof of owner death.
        proof: DeathProof,
    },
    /// Absence of proof leaves owner liveness indeterminate per C5.14 and C2.5.
    Indeterminate,
}

/// Proof of clean release proving release from any host per C5.14.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanReleaseProof {
    /// Released monotonic epoch.
    pub epoch: u64,
    /// Release timestamp in nanoseconds since Unix epoch.
    pub release_time_ns: u64,
}

/// Closed migration-mode sub-domain per C6.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationMode {
    /// Artefact is operating in steady mode.
    Steady,
    /// Artefact is actively undergoing migration.
    Migrating,
}

/// Outcome of a write operation per C5.16.
///
/// When write landing is undetermined, it belongs neither to success nor to failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteLandingVerdict<T> {
    /// The write successfully landed.
    Landed(T),
    /// Whether the write landed is undetermined per C5.16.
    Undetermined {
        /// Monotonic epoch carried by the write whose landing is undetermined.
        carried_epoch: u64,
    },
}

/// Diagnostic detail reported with an operation failure per C6.9 and C6.11.
///
/// Holds Pardosa-owned contextual information about the failure without
/// exposing backend error types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticDetail {
    message: String,
}

impl DiagnosticDetail {
    /// Creates a new diagnostic detail from a message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the diagnostic detail message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<String> for DiagnosticDetail {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<&str> for DiagnosticDetail {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

impl fmt::Display for DiagnosticDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Closed enumeration of operation failure conditions per C6.7.
///
/// Contains zero catch-alls; every condition is a complete named condition.
/// Applicable remedies are documented under C6.10.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureCondition {
    /// Creation finds an artefact already present (C12.3).
    ///
    /// Remedy: open the existing artefact instead of creating it.
    StoreAlreadyExists,
    /// Strict open refuses because no artefact exists (C5.62, C5.10).
    ///
    /// Remedy: create the artefact first with strict create.
    NoArtefactExists,
    /// A claim on an existing artefact loses compare-and-set (C5.7, C12.3).
    ///
    /// Remedy: stop; another writer took ownership by compare-and-set.
    ConcurrencyConflict,
    /// Stale-epoch write rejection; writer lost ownership, no retry (C5.5, C12.4).
    ///
    /// Remedy: stop; writer lost ownership, do not retry with stale epoch.
    StaleEpoch,
    /// Ownership cannot be established on write open (C5.10, C12.5).
    ///
    /// Remedy: establish ownership or perform read-only open.
    OwnershipUnestablished,
    /// Ownership record unreadable while fencing (C5.12).
    ///
    /// Remedy: re-establish current authority and determine which events landed before resubmitting.
    OwnershipRecordUnreadable,
    /// Required exclusion mechanism is unavailable (C5.6, C5.13).
    ///
    /// Remedy: wait or configure the required platform exclusion mechanism.
    ExclusionUnavailable,
    /// Another owner holds the required exclusion (C5.6, C5.65, C6.41).
    ///
    /// Remedy: wait for the other owner to release exclusion or initiate operator takeover.
    AnotherOwnerHoldsExclusion,
    /// Migration started without exclusive access to its target (C5.17).
    ///
    /// Remedy: obtain exclusive access to target before starting migration.
    MigrationExclusionAbsent,
    /// A second concurrent migration is refused (C5.19).
    ///
    /// Remedy: wait for the running migration to finish.
    MigrationAlreadyRunning,
    /// Discovered break in backward precursor chain (C5.28, C6.12).
    ///
    /// Remedy: enter the broken artefact only through explicit call-site migration election.
    PrecursorChainBroken(Option<CausalChainError>),
    /// Uncovered partition membership; no dragline assigned by inference (C5.56).
    ///
    /// Remedy: configure explicit partition mapping for the fiber.
    UncoveredPartitionMembership,
    /// Payload type differs on an event-yielding path (C5.51, C6.33).
    ///
    /// Remedy: reconcile schema descriptor or migrate payload.
    SchemaMismatch,
    /// Envelope shape differs on any path (C4.18, C6.33).
    ///
    /// Remedy: align envelope header shape.
    EnvelopeMismatch,
    /// Mismatch established, differing subject unestablished (C6.34).
    ///
    /// Remedy: inspect diagnostic detail and provide matching descriptor/envelope.
    MismatchUndeterminedSubject,
    /// The artefact pair does not belong together (C6.7).
    ///
    /// Remedy: inspect diagnostic detail for pairing check failure and supply matching pair.
    ArtefactMismatch,
    /// A value violates a bound or validity constraint (C6.7, C5.53).
    ///
    /// Remedy: adjust value to satisfy the constraint.
    ValueConstraintViolated {
        /// Specific value constraint that was violated.
        constraint: ValueConstraint,
    },
    /// Wire tag is unrecognised by decoder (C4.13).
    ///
    /// Remedy: upgrade reader to recognise the wire tag.
    UnrecognisedWireTag {
        /// Wire tag value encountered.
        tag: u8,
    },
    /// Invariant-breaking configuration requested at open (C5.35).
    ///
    /// Remedy: adjust open configuration to respect invariants.
    InvariantBreakingConfiguration,
    /// No artefact omitting its schema descriptor is admitted (C5.45, C6.25).
    ///
    /// Remedy: supply schema descriptor; no artefact omitting descriptor is admitted.
    MissingSchemaDescriptor,
    /// Caller payload transformation refused during migration (C6.18).
    ///
    /// Remedy: provide valid payload transformation for migration.
    TransformationRefused,
    /// Every append to a retired migration source is rejected (C5.63).
    ///
    /// Remedy: redirect writes to the migration target; source append authority is permanently retired.
    RetiredMigrationSource,
}

impl FailureCondition {
    /// Returns the documented remedy for this failure condition per C6.10.
    #[must_use]
    pub fn remedy(&self) -> &'static str {
        match self {
            Self::StoreAlreadyExists => "open the existing artefact instead of creating it",
            Self::NoArtefactExists => "create the artefact first with strict create",
            Self::ConcurrencyConflict => "stop; another writer took ownership by compare-and-set",
            Self::StaleEpoch => "stop; writer lost ownership, do not retry with stale epoch",
            Self::OwnershipUnestablished => "establish ownership or perform read-only open",
            Self::OwnershipRecordUnreadable => {
                "re-establish current authority and determine which events landed before resubmitting"
            }
            Self::ExclusionUnavailable => {
                "wait or configure the required platform exclusion mechanism"
            }
            Self::AnotherOwnerHoldsExclusion => {
                "wait for other owner to release exclusion or initiate operator takeover"
            }
            Self::MigrationExclusionAbsent => {
                "obtain exclusive access to target before starting migration"
            }
            Self::MigrationAlreadyRunning => "wait for running migration to finish",
            Self::PrecursorChainBroken(_) => {
                "enter broken artefact only through explicit call-site migration election"
            }
            Self::UncoveredPartitionMembership => {
                "configure explicit partition mapping for the fiber"
            }
            Self::SchemaMismatch => "reconcile schema descriptor or migrate payload",
            Self::EnvelopeMismatch => "align envelope header shape",
            Self::MismatchUndeterminedSubject => {
                "inspect diagnostic detail and provide matching descriptor/envelope"
            }
            Self::ArtefactMismatch => {
                "inspect diagnostic detail for pairing check failure and supply matching pair"
            }
            Self::ValueConstraintViolated { .. } => "adjust value to satisfy the constraint",
            Self::UnrecognisedWireTag { .. } => "upgrade reader to recognise the wire tag",
            Self::InvariantBreakingConfiguration => "adjust open configuration to respect invariants",
            Self::MissingSchemaDescriptor => {
                "supply schema descriptor; no artefact omitting descriptor is admitted"
            }
            Self::TransformationRefused => "provide valid payload transformation for migration",
            Self::RetiredMigrationSource => {
                "redirect writes to migration target; source append authority is permanently retired"
            }
        }
    }
}

impl fmt::Display for FailureCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StoreAlreadyExists => write!(f, "store already exists"),
            Self::NoArtefactExists => write!(f, "no artefact exists on open"),
            Self::ConcurrencyConflict => {
                write!(f, "concurrency conflict on claim compare-and-set")
            }
            Self::StaleEpoch => write!(f, "stale-epoch write rejection"),
            Self::OwnershipUnestablished => {
                write!(f, "ownership cannot be established on write open")
            }
            Self::OwnershipRecordUnreadable => {
                write!(f, "ownership record unreadable while fencing")
            }
            Self::ExclusionUnavailable => write!(f, "required exclusion mechanism is unavailable"),
            Self::AnotherOwnerHoldsExclusion => {
                write!(f, "another owner holds the required exclusion")
            }
            Self::MigrationExclusionAbsent => {
                write!(f, "migration started without target exclusion")
            }
            Self::MigrationAlreadyRunning => write!(f, "concurrent migration already running"),
            Self::PrecursorChainBroken(Some(err)) => {
                write!(f, "precursor chain broken: {err}")
            }
            Self::PrecursorChainBroken(None) => write!(f, "precursor chain broken"),
            Self::UncoveredPartitionMembership => write!(f, "uncovered partition membership"),
            Self::SchemaMismatch => write!(f, "schema payload descriptor mismatch"),
            Self::EnvelopeMismatch => write!(f, "envelope header shape mismatch"),
            Self::MismatchUndeterminedSubject => {
                write!(f, "mismatch established with undetermined subject")
            }
            Self::ArtefactMismatch => write!(f, "artefact pair mismatch"),
            Self::ValueConstraintViolated { constraint } => {
                write!(f, "value constraint violated: {constraint:?}")
            }
            Self::UnrecognisedWireTag { tag } => write!(f, "unrecognised wire tag: 0x{tag:02x}"),
            Self::InvariantBreakingConfiguration => {
                write!(f, "invariant-breaking open configuration")
            }
            Self::MissingSchemaDescriptor => write!(f, "missing schema descriptor"),
            Self::TransformationRefused => write!(f, "caller payload transformation refused"),
            Self::RetiredMigrationSource => {
                write!(f, "append to retired migration source rejected")
            }
        }
    }
}

/// Closed failure type carrying a named condition and Pardosa-owned diagnostic detail per C6.7, C6.9, and C6.11.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationFailure {
    condition: FailureCondition,
    diagnostic_detail: DiagnosticDetail,
}

impl OperationFailure {
    /// Creates a new operation failure with a condition and diagnostic detail.
    #[must_use]
    pub fn new(condition: FailureCondition, detail: impl Into<DiagnosticDetail>) -> Self {
        Self {
            condition,
            diagnostic_detail: detail.into(),
        }
    }

    /// Returns the specific failure condition.
    #[must_use]
    pub fn condition(&self) -> &FailureCondition {
        &self.condition
    }

    /// Returns the Pardosa-owned diagnostic detail.
    #[must_use]
    pub fn diagnostic_detail(&self) -> &DiagnosticDetail {
        &self.diagnostic_detail
    }
}

impl fmt::Display for OperationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}",
            self.condition,
            self.diagnostic_detail.message()
        )
    }
}

impl std::error::Error for OperationFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.condition {
            FailureCondition::PrecursorChainBroken(Some(err)) => Some(err),
            _ => None,
        }
    }
}

/// Ownership status reported on artefact open per C5.10 and C6.14.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipStatus {
    /// Artefact has recorded ownership with an active epoch claim.
    Owned {
        /// Active epoch number.
        epoch: u64,
    },
    /// Artefact has no ownership record and is explicitly unowned per C5.10 / C6.14.
    Unowned,
}

/// Generation knowledge of an opened artefact per C6.8 and C6.14.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationKnowledge {
    /// Generation index is known.
    Known(u32),
    /// Generation index is explicitly unknown per C6.8 / C6.14.
    Unknown,
}

/// Supersession status of an opened artefact per C6.8 and C6.15.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupersessionStatus {
    /// Generation is current (not superseded).
    Current,
    /// Generation is known to be superseded per C6.15.
    Superseded {
        /// Successor generation index if known.
        next_generation: Option<u32>,
    },
    /// Supersession state is unknown per C6.8 / C6.15.
    Unknown,
}

/// Directional migration disagreement reported on open per C5.15 and C6.8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationDisagreement {
    /// No migration disagreement detected.
    None,
    /// Source announces outbound migration the target holds no record of (C5.15).
    OutboundWithoutInbound,
    /// Target holds inbound record the source announces nothing of (C5.15).
    InboundWithoutOutbound,
}

/// Established history integrity for an opened history per C5.26-28, C5.40, and C6.15.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryIntegrity {
    /// History has verified precursor link integrity per C5.27 and C5.40,
    /// but is unanchored per C5.26 (distinct from invalid; does not resist full rewrite).
    Unanchored,
    /// History has verified precursor link integrity per C5.40 and is anchored
    /// with external-observation rewrite evidence covering one artefact in one generation per C5.26 and C5.29.
    Anchored,
}

/// Migration-result completeness relative to intended policy per C6.15 and C6.17.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationCompleteness {
    /// Migration result is known complete (e.g. outbound pointer present per C6.17).
    KnownComplete,
    /// Migration result is known incomplete.
    KnownIncomplete,
    /// Migration completeness is unknown (e.g. interrupted or absent pointer per C6.15).
    Unknown,
}

/// Knowledge of append authority for an opened artefact per C6.15 and C5.63.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendAuthority {
    /// Target artefact being read holds append authority.
    HoldsAuthority,
    /// Append authority is held by another generation.
    HeldByGeneration(u32),
    /// Append authority is unknown per C6.15.
    Unknown,
}

/// Error indicating an illegal combination of qualified open facts per C6.8 and C6.15.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IllegalOpenCombination {
    /// An unowned artefact must report generation as unknown per C6.14.
    UnownedMustHaveUnknownGeneration,
    /// An unknown generation cannot be silently current per C6.8.
    UnknownGenerationCannotBeCurrent,
    /// An unowned artefact cannot hold append authority per C5.10.
    UnownedCannotHoldAppendAuthority,
    /// Locked fiber state is unreachable on a reopened store per C6.2.
    LockedStateOnReopen,
    /// Migrating mode is unreachable on a reopened store per C6.2.
    MigratingModeOnReopen,
    /// Non-empty removed fiber identities are unreachable on a reopened store per C6.2.
    RemovedIdentitiesOnReopen,
}

impl fmt::Display for IllegalOpenCombination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnownedMustHaveUnknownGeneration => {
                write!(f, "unowned artefact must report generation as unknown")
            }
            Self::UnknownGenerationCannotBeCurrent => {
                write!(f, "unknown generation cannot be silently current")
            }
            Self::UnownedCannotHoldAppendAuthority => {
                write!(f, "unowned artefact cannot hold append authority")
            }
            Self::LockedStateOnReopen => {
                write!(f, "locked fiber state is unreachable on reopen")
            }
            Self::MigratingModeOnReopen => {
                write!(f, "migrating mode is unreachable on reopen")
            }
            Self::RemovedIdentitiesOnReopen => {
                write!(f, "removed fiber identities must be empty on reopen")
            }
        }
    }
}

impl std::error::Error for IllegalOpenCombination {}

/// Single qualified-result type carrying what Pardosa knows about an opened artefact per C6.7, C6.8, and C6.15.
///
/// All fields are private with read-only projections to prevent construction of illegal states
/// or direct field mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedOpenResult {
    ownership: OwnershipStatus,
    generation: GenerationKnowledge,
    supersession: SupersessionStatus,
    migration_disagreement: MigrationDisagreement,
    history_integrity: HistoryIntegrity,
    migration_completeness: MigrationCompleteness,
    append_authority: AppendAuthority,
}

impl QualifiedOpenResult {
    /// Returns a new builder for constructing a [`QualifiedOpenResult`].
    #[must_use]
    pub fn builder() -> QualifiedOpenResultBuilder {
        QualifiedOpenResultBuilder::new()
    }

    /// Returns the standard qualified read result for an unowned orphan read per C5.10 and C6.14.
    #[must_use]
    pub fn orphan_read() -> Self {
        Self {
            ownership: OwnershipStatus::Unowned,
            generation: GenerationKnowledge::Unknown,
            supersession: SupersessionStatus::Unknown,
            migration_disagreement: MigrationDisagreement::None,
            history_integrity: HistoryIntegrity::Unanchored,
            migration_completeness: MigrationCompleteness::Unknown,
            append_authority: AppendAuthority::Unknown,
        }
    }

    /// Returns the ownership status.
    #[must_use]
    pub fn ownership(&self) -> &OwnershipStatus {
        &self.ownership
    }

    /// Returns the generation knowledge.
    #[must_use]
    pub fn generation(&self) -> GenerationKnowledge {
        self.generation
    }

    /// Returns the supersession status.
    #[must_use]
    pub fn supersession(&self) -> SupersessionStatus {
        self.supersession
    }

    /// Returns the directional migration disagreement.
    #[must_use]
    pub fn migration_disagreement(&self) -> MigrationDisagreement {
        self.migration_disagreement
    }

    /// Returns the history integrity.
    #[must_use]
    pub fn history_integrity(&self) -> HistoryIntegrity {
        self.history_integrity
    }

    /// Returns the migration completeness.
    #[must_use]
    pub fn migration_completeness(&self) -> MigrationCompleteness {
        self.migration_completeness
    }

    /// Returns the append authority knowledge.
    #[must_use]
    pub fn append_authority(&self) -> AppendAuthority {
        self.append_authority
    }
}

/// Builder for constructing and validating a [`QualifiedOpenResult`] per C6.8 and C6.15.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedOpenResultBuilder {
    ownership: OwnershipStatus,
    generation: GenerationKnowledge,
    supersession: SupersessionStatus,
    migration_disagreement: MigrationDisagreement,
    history_integrity: HistoryIntegrity,
    migration_completeness: MigrationCompleteness,
    append_authority: AppendAuthority,
}

impl Default for QualifiedOpenResultBuilder {
    fn default() -> Self {
        Self {
            ownership: OwnershipStatus::Unowned,
            generation: GenerationKnowledge::Unknown,
            supersession: SupersessionStatus::Unknown,
            migration_disagreement: MigrationDisagreement::None,
            history_integrity: HistoryIntegrity::Unanchored,
            migration_completeness: MigrationCompleteness::Unknown,
            append_authority: AppendAuthority::Unknown,
        }
    }
}

impl QualifiedOpenResultBuilder {
    /// Creates a new builder initialized with default unknown facts.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the ownership status.
    #[must_use]
    pub fn ownership(mut self, ownership: OwnershipStatus) -> Self {
        self.ownership = ownership;
        self
    }

    /// Sets the generation knowledge.
    #[must_use]
    pub fn generation(mut self, generation: GenerationKnowledge) -> Self {
        self.generation = generation;
        self
    }

    /// Sets the supersession status.
    #[must_use]
    pub fn supersession(mut self, supersession: SupersessionStatus) -> Self {
        self.supersession = supersession;
        self
    }

    /// Sets the directional migration disagreement.
    #[must_use]
    pub fn migration_disagreement(mut self, disagreement: MigrationDisagreement) -> Self {
        self.migration_disagreement = disagreement;
        self
    }

    /// Sets the history integrity.
    #[must_use]
    pub fn history_integrity(mut self, integrity: HistoryIntegrity) -> Self {
        self.history_integrity = integrity;
        self
    }

    /// Sets the migration completeness.
    #[must_use]
    pub fn migration_completeness(mut self, completeness: MigrationCompleteness) -> Self {
        self.migration_completeness = completeness;
        self
    }

    /// Sets the append authority knowledge.
    #[must_use]
    pub fn append_authority(mut self, authority: AppendAuthority) -> Self {
        self.append_authority = authority;
        self
    }

    /// Builds the [`QualifiedOpenResult`], validating all invariant combinations.
    ///
    /// # Errors
    ///
    /// Returns [`IllegalOpenCombination`] if the combination of facts violates
    /// specification invariants (C6.8, C6.14, C6.15).
    pub fn build(self) -> Result<QualifiedOpenResult, IllegalOpenCombination> {
        match (
            &self.ownership,
            &self.generation,
            &self.supersession,
            &self.append_authority,
        ) {
            (OwnershipStatus::Unowned, GenerationKnowledge::Known(_), _, _) => {
                Err(IllegalOpenCombination::UnownedMustHaveUnknownGeneration)
            }
            (_, GenerationKnowledge::Unknown, SupersessionStatus::Current, _) => {
                Err(IllegalOpenCombination::UnknownGenerationCannotBeCurrent)
            }
            (OwnershipStatus::Unowned, _, _, AppendAuthority::HoldsAuthority) => {
                Err(IllegalOpenCombination::UnownedCannotHoldAppendAuthority)
            }
            _ => Ok(QualifiedOpenResult {
                ownership: self.ownership,
                generation: self.generation,
                supersession: self.supersession,
                migration_disagreement: self.migration_disagreement,
                history_integrity: self.history_integrity,
                migration_completeness: self.migration_completeness,
                append_authority: self.append_authority,
            }),
        }
    }
}

/// Status of an artefact's recorded ownership metadata per C5.8-12.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedOwnership {
    /// Ownership record exists and carries an active claim per C5.8.
    Claimed(OwnershipClaimRecord),
    /// Ownership record exists but carries no claim (unowned) per C5.9.
    Unowned,
    /// Ownership record could not be read while fencing per C5.12.
    Unreadable,
    /// Ownership record is absent per C5.10 and C6.14.
    Absent,
}

/// Fencing validator ensuring writes carry a current epoch per C5.4, C5.5, and C5.12.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnershipFence;

impl OwnershipFence {
    /// Fences a write by comparing the carried epoch against recorded ownership.
    ///
    /// # Errors
    ///
    /// - [`FailureCondition::OwnershipRecordUnreadable`] if the record could not be read (C5.12).
    /// - [`FailureCondition::OwnershipUnestablished`] if the record is absent or unowned on write path (C5.10, C12.5).
    /// - [`FailureCondition::StaleEpoch`] if the carried epoch does not match the recorded epoch (C5.5, C12.4).
    pub fn fence_write(
        carried_epoch: u64,
        recorded: &RecordedOwnership,
    ) -> Result<(), OperationFailure> {
        match recorded {
            RecordedOwnership::Unreadable => Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "ownership record could not be read while fencing write per C5.12",
            )),
            RecordedOwnership::Absent => Err(OperationFailure::new(
                FailureCondition::OwnershipUnestablished,
                "ownership record is absent on write path per C5.10",
            )),
            RecordedOwnership::Unowned => Err(OperationFailure::new(
                FailureCondition::OwnershipUnestablished,
                "artefact is unowned on write path per C5.10",
            )),
            RecordedOwnership::Claimed(claim) => {
                if carried_epoch == claim.epoch {
                    Ok(())
                } else {
                    Err(OperationFailure::new(
                        FailureCondition::StaleEpoch,
                        format!(
                            "carried epoch {carried_epoch} does not match recorded epoch {} per C5.5",
                            claim.epoch
                        ),
                    ))
                }
            }
        }
    }
}

/// Evaluates owner liveness during takeover evaluation per C5.14.
///
/// If a death proof is provided, takeover is proven;
/// otherwise returns an indeterminate verdict requiring operator action.
#[must_use]
pub fn evaluate_owner_liveness(death_proof: Option<DeathProof>) -> LivenessVerdict {
    match death_proof {
        Some(proof) => LivenessVerdict::ProvenDead { proof },
        None => LivenessVerdict::Indeterminate,
    }
}

/// Presence of an artefact's components in storage per C5.10 and C5.62.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtefactPresence {
    /// Neither ownership record nor event data exists.
    None,
    /// Ownership record exists but event data does not (creation incomplete per C5.10).
    OwnershipRecordOnly,
    /// Event data exists but ownership record is absent (orphan per C5.10 and C6.14).
    EventDataOnly,
    /// Both ownership record and event data exist.
    Both,
}

/// Validates artefact creation admission per C5.62 and C12.3.
///
/// # Errors
///
/// Returns [`OperationFailure`] with [`FailureCondition::StoreAlreadyExists`] if the artefact
/// already exists or has components present in storage.
pub fn admit_create(presence: ArtefactPresence) -> Result<(), OperationFailure> {
    match presence {
        ArtefactPresence::None => Ok(()),
        ArtefactPresence::Both
        | ArtefactPresence::OwnershipRecordOnly
        | ArtefactPresence::EventDataOnly => Err(OperationFailure::new(
            FailureCondition::StoreAlreadyExists,
            "artefact already exists in storage; open instead per C12.3",
        )),
    }
}

/// State of an incomplete creation where ownership record is present without event data per C5.10.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncompleteCreationState {
    /// Ownership record is unseeded (unowned) per C5.9 and C5.64.
    Unseeded,
    /// Ownership record carries an established claim per C5.8 and C5.10.
    Claimed(OwnershipClaimRecord),
}

/// Progression of ordered artefact creation per C5.8, C5.10, and C5.64.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreationProgression {
    /// Step 1: Ownership record created exclusively; initially unseeded (unowned) per C5.64.
    UnseededOwnershipRecordCreated,
    /// Step 2: Ownership record seeded with initial ownership claim per C5.8 and C5.9.
    OwnershipClaimSeeded(OwnershipClaimRecord),
    /// Step 3: Event data created following ownership record per C5.10.
    Complete,
}

impl CreationProgression {
    /// Returns the artefact presence for this creation progression step per C5.10.
    #[must_use]
    pub fn presence(&self) -> ArtefactPresence {
        match self {
            Self::UnseededOwnershipRecordCreated | Self::OwnershipClaimSeeded(_) => {
                ArtefactPresence::OwnershipRecordOnly
            }
            Self::Complete => ArtefactPresence::Both,
        }
    }
}

impl From<IncompleteCreationState> for CreationProgression {
    fn from(state: IncompleteCreationState) -> Self {
        match state {
            IncompleteCreationState::Unseeded => Self::UnseededOwnershipRecordCreated,
            IncompleteCreationState::Claimed(claim) => Self::OwnershipClaimSeeded(claim),
        }
    }
}

/// Ordered creation domain protocol enforcing ownership record first, then event data per C5.8, C5.10, and C5.64.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreationPlan;

impl CreationPlan {
    /// Step 1: Validates that neither component exists and admits exclusive creation
    /// of an unseeded ownership record per C5.64.
    ///
    /// # Errors
    /// Returns [`FailureCondition::StoreAlreadyExists`] if any component already exists.
    pub fn begin(presence: ArtefactPresence) -> Result<CreationProgression, OperationFailure> {
        admit_create(presence)?;
        Ok(CreationProgression::UnseededOwnershipRecordCreated)
    }

    /// Step 2: Seeds the unseeded (unowned) record with an initial claim per C5.8 and C5.9.
    ///
    /// # Errors
    /// Returns [`FailureCondition::ConcurrencyConflict`] if the record is already seeded or complete.
    pub fn seed_claim(
        progression: CreationProgression,
        claim: OwnershipClaimRecord,
    ) -> Result<CreationProgression, OperationFailure> {
        match progression {
            CreationProgression::UnseededOwnershipRecordCreated => {
                Ok(CreationProgression::OwnershipClaimSeeded(claim))
            }
            CreationProgression::OwnershipClaimSeeded(_) | CreationProgression::Complete => {
                Err(OperationFailure::new(
                    FailureCondition::ConcurrencyConflict,
                    "ownership record already seeded; cannot re-seed initial claim per C5.8",
                ))
            }
        }
    }

    /// Step 3: Completes creation by creating event data after the ownership record per C5.10.
    ///
    /// Requires that the ownership record has an established claim.
    ///
    /// # Errors
    /// - [`FailureCondition::OwnershipUnestablished`] if the ownership record has no claim.
    /// - [`FailureCondition::StoreAlreadyExists`] if artefact creation was already completed.
    pub fn complete_event_data(
        progression: CreationProgression,
    ) -> Result<CreationProgression, OperationFailure> {
        match progression {
            CreationProgression::OwnershipClaimSeeded(_) => Ok(CreationProgression::Complete),
            CreationProgression::UnseededOwnershipRecordCreated => Err(OperationFailure::new(
                FailureCondition::OwnershipUnestablished,
                "ownership record must carry an established claim before event data creation per C5.10",
            )),
            CreationProgression::Complete => Err(OperationFailure::new(
                FailureCondition::StoreAlreadyExists,
                "artefact creation already completed per C5.10",
            )),
        }
    }

    /// Resumes incomplete creation where ownership record exists but event data is absent per C5.10.
    ///
    /// Allows the claimant or a later owner to complete creation by writing event data.
    ///
    /// # Errors
    /// - [`FailureCondition::NoArtefactExists`] if ownership record is absent.
    /// - [`FailureCondition::StoreAlreadyExists`] if artefact is already complete.
    /// - [`FailureCondition::OwnershipUnestablished`] if event data exists without ownership record.
    pub fn resume_incomplete_creation(
        presence: ArtefactPresence,
        claim: Option<OwnershipClaimRecord>,
    ) -> Result<IncompleteCreationState, OperationFailure> {
        match presence {
            ArtefactPresence::OwnershipRecordOnly => match claim {
                Some(claim) => Ok(IncompleteCreationState::Claimed(claim)),
                None => Ok(IncompleteCreationState::Unseeded),
            },
            ArtefactPresence::None => Err(OperationFailure::new(
                FailureCondition::NoArtefactExists,
                "cannot resume creation without ownership record per C5.10",
            )),
            ArtefactPresence::Both => Err(OperationFailure::new(
                FailureCondition::StoreAlreadyExists,
                "artefact already complete; open instead per C5.62",
            )),
            ArtefactPresence::EventDataOnly => Err(OperationFailure::new(
                FailureCondition::OwnershipUnestablished,
                "event data without ownership record cannot complete creation per C5.10",
            )),
        }
    }
}

/// Admitted state on opening an artefact per C5.62, C5.10, and C6.14.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAdmission {
    /// Artefact is complete and ready for ordinary access.
    Ready,
    /// Ownership record is present without event data; open for completion per C5.10.
    IncompleteCreation(IncompleteCreationState),
    /// Read-only orphan read admitted with unowned status and unknown generation per C6.14.
    ReadOnlyOrphan,
}

/// Validates artefact open admission per C5.62, C5.10, and C6.14.
///
/// # Errors
///
/// - [`FailureCondition::NoArtefactExists`] if no artefact exists on open.
/// - [`FailureCondition::OwnershipUnestablished`] if opening event-data-only artefact with write intent.
pub fn admit_open(
    presence: ArtefactPresence,
    claim: Option<OwnershipClaimRecord>,
    write_intent: bool,
) -> Result<OpenAdmission, OperationFailure> {
    match presence {
        ArtefactPresence::None => Err(OperationFailure::new(
            FailureCondition::NoArtefactExists,
            "no artefact exists on open; strict open does not create per C5.62",
        )),
        ArtefactPresence::EventDataOnly => {
            if write_intent {
                Err(OperationFailure::new(
                    FailureCondition::OwnershipUnestablished,
                    "event data present without ownership record refused on write path per C5.10",
                ))
            } else {
                Ok(OpenAdmission::ReadOnlyOrphan)
            }
        }
        ArtefactPresence::OwnershipRecordOnly => {
            let state = match claim {
                Some(claim) => IncompleteCreationState::Claimed(claim),
                None => IncompleteCreationState::Unseeded,
            };
            Ok(OpenAdmission::IncompleteCreation(state))
        }
        ArtefactPresence::Both => Ok(OpenAdmission::Ready),
    }
}

/// Evaluates a compare-and-set claim on an ownership record per C5.7, C5.9, and C12.3.
///
/// # Errors
///
/// - [`FailureCondition::OwnershipRecordUnreadable`] if the record could not be read.
/// - [`FailureCondition::OwnershipUnestablished`] if the record is absent.
/// - [`FailureCondition::ConcurrencyConflict`] if compare-and-set check fails.
pub fn evaluate_claim_cas(
    current_recorded: &RecordedOwnership,
    expected_epoch: Option<u64>,
    candidate_claim: &OwnershipClaimRecord,
) -> Result<u64, OperationFailure> {
    match current_recorded {
        RecordedOwnership::Unreadable => Err(OperationFailure::new(
            FailureCondition::OwnershipRecordUnreadable,
            "ownership record unreadable while evaluating claim per C5.12",
        )),
        RecordedOwnership::Absent => Err(OperationFailure::new(
            FailureCondition::OwnershipUnestablished,
            "cannot claim absent ownership record without creation per C5.10",
        )),
        RecordedOwnership::Unowned => {
            if expected_epoch.is_none() {
                Ok(candidate_claim.epoch)
            } else {
                Err(OperationFailure::new(
                    FailureCondition::ConcurrencyConflict,
                    "expected epoch specified for unowned record per C5.9",
                ))
            }
        }
        RecordedOwnership::Claimed(existing) => {
            if Some(existing.epoch) == expected_epoch && candidate_claim.epoch > existing.epoch {
                Ok(candidate_claim.epoch)
            } else {
                Err(OperationFailure::new(
                    FailureCondition::ConcurrencyConflict,
                    format!(
                        "CAS failed: current epoch {}, expected {:?}, candidate {} per C5.7",
                        existing.epoch, expected_epoch, candidate_claim.epoch
                    ),
                ))
            }
        }
    }
}

/// Validates exact name pairing between ownership record and event data per C5.11 and C6.7.
///
/// # Errors
///
/// Returns [`OperationFailure`] with [`FailureCondition::ArtefactMismatch`] if names do not match.
pub fn validate_name_pairing(
    ownership_record_name: &str,
    event_data_name: &str,
) -> Result<(), OperationFailure> {
    if ownership_record_name == event_data_name {
        Ok(())
    } else {
        Err(OperationFailure::new(
            FailureCondition::ArtefactMismatch,
            format!(
                "ownership record name '{ownership_record_name}' does not match event data name '{event_data_name}' per C5.11"
            ),
        ))
    }
}

/// Proof submitted to justify an ownership takeover attempt per C5.14.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeoverProof {
    /// Proof of clean release from any host per C5.14.
    CleanRelease(CleanReleaseProof),
    /// Proof of owner host/process death per C5.14 and C6.7.
    Death(DeathProof),
}

/// Verdict of evaluating an ownership takeover attempt per C5.14.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeoverVerdict {
    /// Takeover is proven by clean release proof or death proof.
    Proven,
    /// Takeover is indeterminate; requires operator initiation per C5.14.
    Indeterminate,
}

/// Evaluates an ownership takeover attempt against the current claim per C5.14.
#[must_use]
pub fn evaluate_takeover(
    current_claim: &OwnershipClaimRecord,
    proof: Option<&TakeoverProof>,
) -> TakeoverVerdict {
    match proof {
        Some(TakeoverProof::CleanRelease(clean)) => {
            if clean.epoch >= current_claim.epoch {
                TakeoverVerdict::Proven
            } else {
                TakeoverVerdict::Indeterminate
            }
        }
        Some(TakeoverProof::Death(_)) => TakeoverVerdict::Proven,
        None => TakeoverVerdict::Indeterminate,
    }
}

/// Event admission mode and evidence per C5.45, C5.51, C5.52, and C6.33.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventAdmission<'a> {
    /// Ordinary event-yielding path comparing schema identity and envelope identity per C5.51.
    Event {
        /// Schema descriptor present on the artefact per C5.45.
        descriptor: Option<&'a SchemaDescriptor>,
        /// Expected schema identity from consumer payload type.
        expected_schema: &'a SchemaIdentity,
        /// Actual schema identity recorded on the artefact.
        actual_schema: &'a SchemaIdentity,
        /// Expected envelope identity fixed by specification per C4.18.
        expected_envelope: &'a EnvelopeIdentity,
        /// Actual envelope identity recorded on the artefact.
        actual_envelope: &'a EnvelopeIdentity,
    },
    /// Record reader path yielding raw records without schema identity comparison per C5.52.
    RecordReaderOnly {
        /// Schema descriptor present on the artefact per C5.45 and C5.52.
        descriptor: Option<&'a SchemaDescriptor>,
        /// Expected envelope identity fixed by specification per C4.18.
        expected_envelope: &'a EnvelopeIdentity,
        /// Actual envelope identity recorded on the artefact.
        actual_envelope: &'a EnvelopeIdentity,
    },
    /// Mismatch established where descriptor does not yield which subject differed per C6.34.
    MismatchUndeterminedSubject {
        /// Diagnostic detail explaining why subject could not be determined.
        detail: &'static str,
    },
}

/// Validates descriptor presence and schema/envelope identity for event admission per C5.45, C5.51, C5.52, and C6.33.
///
/// # Errors
///
/// - [`FailureCondition::MissingSchemaDescriptor`] if descriptor is absent.
/// - [`FailureCondition::EnvelopeMismatch`] if envelope identities differ.
/// - [`FailureCondition::SchemaMismatch`] if schema identities differ on an event-yielding path.
/// - [`FailureCondition::MismatchUndeterminedSubject`] if mismatch subject cannot be determined per C6.34.
pub fn admit_event(admission: &EventAdmission<'_>) -> Result<(), OperationFailure> {
    match admission {
        EventAdmission::Event {
            descriptor,
            expected_schema,
            actual_schema,
            expected_envelope,
            actual_envelope,
        } => {
            if descriptor.is_none() {
                return Err(OperationFailure::new(
                    FailureCondition::MissingSchemaDescriptor,
                    "artefact omits required schema descriptor per C5.45",
                ));
            }
            if expected_envelope != actual_envelope {
                return Err(OperationFailure::new(
                    FailureCondition::EnvelopeMismatch,
                    format!(
                        "envelope identity mismatch: expected {expected_envelope:?}, found {actual_envelope:?}"
                    ),
                ));
            }
            if expected_schema != actual_schema {
                return Err(OperationFailure::new(
                    FailureCondition::SchemaMismatch,
                    format!(
                        "schema identity mismatch: expected {expected_schema:?}, found {actual_schema:?}"
                    ),
                ));
            }
            Ok(())
        }
        EventAdmission::RecordReaderOnly {
            descriptor,
            expected_envelope,
            actual_envelope,
        } => {
            if descriptor.is_none() {
                return Err(OperationFailure::new(
                    FailureCondition::MissingSchemaDescriptor,
                    "artefact omits required schema descriptor per C5.45 and C5.52",
                ));
            }
            if expected_envelope != actual_envelope {
                return Err(OperationFailure::new(
                    FailureCondition::EnvelopeMismatch,
                    format!(
                        "envelope identity mismatch: expected {expected_envelope:?}, found {actual_envelope:?}"
                    ),
                ));
            }
            Ok(())
        }
        EventAdmission::MismatchUndeterminedSubject { detail } => Err(OperationFailure::new(
            FailureCondition::MismatchUndeterminedSubject,
            *detail,
        )),
    }
}

/// Classification of an event envelope's precursor link per C4.19 and C6.12.
///
/// Encapsulated behind private fields with no public variants, constructed
/// exclusively via [`PrecursorLink::classify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrecursorLink<'a> {
    envelope: &'a EventEnvelope,
    is_genesis: bool,
}

impl<'a> PrecursorLink<'a> {
    /// Classifies an event envelope's precursor link per C4.19 and C6.12.
    ///
    /// Validates that genesis envelopes carry zero precursor and zero precursor hash.
    /// Non-genesis envelopes are classified as chained predecessor events.
    ///
    /// # Errors
    ///
    /// Returns [`OperationFailure`] with [`FailureCondition::PrecursorChainBroken`]
    /// wrapping `None` if genesis fields are malformed per C4.19.
    pub fn classify(envelope: &'a EventEnvelope) -> Result<Self, OperationFailure> {
        let is_zero_precursor = envelope.header.precursor == [0u8; 16];
        let is_zero_hash = envelope.header.precursor_hash == [0u8; 32];
        if is_zero_precursor {
            if is_zero_hash {
                Ok(Self {
                    envelope,
                    is_genesis: true,
                })
            } else {
                Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    "genesis envelope has non-zero precursor commitment hash per C4.19",
                ))
            }
        } else {
            Ok(Self {
                envelope,
                is_genesis: false,
            })
        }
    }

    /// Returns the classified event envelope.
    #[must_use]
    pub fn envelope(&self) -> &'a EventEnvelope {
        self.envelope
    }

    /// Returns whether this link represents a genesis event.
    #[must_use]
    pub fn is_genesis(&self) -> bool {
        self.is_genesis
    }
}

/// Validates precursor linkage for event admission along a fiber per C5.27, C5.28, C5.40, and C6.12.
///
/// Verifies requested precursor ID equality on the resolved predecessor, fiber equality,
/// and commitment digest binding using BLAKE3.
///
/// # Errors
///
/// - [`FailureCondition::PrecursorChainBroken`] with [`CausalChainError::PrecursorWrongFiber`]
///   if the event or its predecessor belongs to another fiber.
/// - [`FailureCondition::PrecursorChainBroken`] with [`CausalChainError::PrecursorOutOfRange`]
///   if the precursor event is not resident in the artefact or if the resolved predecessor ID
///   does not match the requested precursor ID.
/// - [`FailureCondition::PrecursorChainBroken`] with `None` if the precursor commitment hash
///   does not match the predecessor's computed BLAKE3 commitment per C5.40.
pub fn admit_precursor_link<'a>(
    expected_fiber_id: &[u8; 16],
    link: &PrecursorLink<'a>,
    resolve_predecessor: impl Fn(&[u8; 16]) -> Option<&'a EventEnvelope>,
) -> Result<(), OperationFailure> {
    if link.envelope.header.fiber_id != *expected_fiber_id {
        return Err(OperationFailure::new(
            FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorWrongFiber)),
            "event belongs to another fiber per C6.12",
        ));
    }
    if link.is_genesis {
        return Ok(());
    }
    let requested_precursor_id = &link.envelope.header.precursor;
    let predecessor = match resolve_predecessor(requested_precursor_id) {
        Some(p) => p,
        None => {
            return Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorOutOfRange)),
                "precursor event ID is outside the artefact per C6.12",
            ));
        }
    };
    if predecessor.header.event_id != *requested_precursor_id {
        return Err(OperationFailure::new(
            FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorOutOfRange)),
            "resolved predecessor event ID does not match requested precursor ID per C6.12",
        ));
    }
    if predecessor.header.fiber_id != *expected_fiber_id {
        return Err(OperationFailure::new(
            FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorWrongFiber)),
            "predecessor event belongs to another fiber per C6.12",
        ));
    }
    let computed_hash =
        crate::encoding::compute_envelope_commitment(&predecessor.header, &predecessor.payload);
    if link.envelope.header.precursor_hash != computed_hash {
        return Err(OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            "precursor commitment hash does not match predecessor commitment per C5.40",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fiber_lifecycle_all_10_transitions() {
        assert_eq!(FiberState::Undefined.create(), Ok(FiberState::Defined));
        assert_eq!(FiberState::Defined.update(), Ok(FiberState::Defined));
        assert_eq!(FiberState::Defined.detach(), Ok(FiberState::Detached));
        assert_eq!(FiberState::Detached.rescue(), Ok(FiberState::Defined));
        assert_eq!(
            FiberState::Detached.migrate(FiberMigrationPolicy::Keep),
            Ok(FiberState::Detached)
        );
        assert_eq!(
            FiberState::Detached.migrate(FiberMigrationPolicy::LockAndPrune),
            Ok(FiberState::Locked)
        );
        assert_eq!(
            FiberState::Detached.migrate(FiberMigrationPolicy::Purge),
            Ok(FiberState::Purged)
        );
        assert_eq!(FiberState::Locked.rescue(), Ok(FiberState::Defined));
        assert_eq!(
            FiberState::Locked.migrate(FiberMigrationPolicy::Purge),
            Ok(FiberState::Purged)
        );
        assert_eq!(FiberState::Purged.create(), Ok(FiberState::Defined));

        let locator = ArtefactLocator::new("test-locator");
        assert_eq!(locator.as_str(), "test-locator");
    }

    #[test]
    fn test_fiber_reopened_validity() {
        assert!(FiberState::Undefined.is_reopened_valid());
        assert!(FiberState::Defined.is_reopened_valid());
        assert!(FiberState::Detached.is_reopened_valid());
        assert!(FiberState::Purged.is_reopened_valid());
        assert!(!FiberState::Locked.is_reopened_valid());
    }

    #[test]
    fn test_fiber_matrix_all_actions_exhaustive() {
        let all_states = [
            FiberState::Undefined,
            FiberState::Defined,
            FiberState::Detached,
            FiberState::Purged,
            FiberState::Locked,
        ];
        let all_actions = [
            AttemptedTransition::Create,
            AttemptedTransition::Update,
            AttemptedTransition::Detach,
            AttemptedTransition::Rescue,
            AttemptedTransition::Migrate(FiberMigrationPolicy::Keep),
            AttemptedTransition::Migrate(FiberMigrationPolicy::LockAndPrune),
            AttemptedTransition::Migrate(FiberMigrationPolicy::Purge),
        ];

        let mut valid_count = 0;
        let mut invalid_count = 0;

        for state in &all_states {
            for action in &all_actions {
                match state.apply_action(*action) {
                    Ok(_) => {
                        assert!(state.can_apply_action(action));
                        valid_count += 1;
                    }
                    Err(err) => {
                        assert!(!state.can_apply_action(action));
                        assert_eq!(err.from, *state);
                        assert_eq!(err.attempted, *action);
                        invalid_count += 1;
                    }
                }
            }
        }

        assert_eq!(valid_count, 10);
        assert_eq!(invalid_count, 25);
    }

    #[test]
    fn test_rescue_locked_policy() {
        assert_eq!(
            FiberState::Locked.rescue_locked(LockedRescuePolicy::PreserveAuditTrail),
            Ok(FiberState::Defined)
        );
        assert_eq!(
            FiberState::Locked.rescue_locked(LockedRescuePolicy::AcceptDataLoss),
            Ok(FiberState::Defined)
        );
        assert!(FiberState::Defined
            .rescue_locked(LockedRescuePolicy::PreserveAuditTrail)
            .is_err());
    }

    #[test]
    fn test_typed_attempted_transition_and_two_member_causal_chain_error() {
        let err = FiberState::Undefined.update().unwrap_err();
        assert_eq!(err.from, FiberState::Undefined);
        assert_eq!(err.attempted, AttemptedTransition::Update);
        assert_eq!(
            format!("{err}"),
            "illegal fiber state transition from Undefined via update"
        );

        let mig_err = FiberState::Undefined
            .migrate(FiberMigrationPolicy::Keep)
            .unwrap_err();
        assert_eq!(
            mig_err.attempted,
            AttemptedTransition::Migrate(FiberMigrationPolicy::Keep)
        );

        fn check_causal_chain_2_members(e: &CausalChainError) {
            match e {
                CausalChainError::PrecursorOutOfRange => {}
                CausalChainError::PrecursorWrongFiber => {}
            }
        }
        check_causal_chain_2_members(&CausalChainError::PrecursorOutOfRange);
        check_causal_chain_2_members(&CausalChainError::PrecursorWrongFiber);
    }

    #[test]
    fn test_cursor_valid_by_construction_and_migration_regeneration() {
        ArtefactReader::with_reader("source-artefact", |source_reader| {
            let genesis_env = crate::encoding::EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: [9u8; 16],
                    fiber_id: [1u8; 16],
                    detached: false,
                    precursor: [0u8; 16],
                    precursor_hash: [0u8; 32],
                },
                payload: vec![1, 2, 3],
            };
            let non_genesis_env = crate::encoding::EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: [11u8; 16],
                    fiber_id: [1u8; 16],
                    detached: false,
                    precursor: [9u8; 16],
                    precursor_hash: [1u8; 32],
                },
                payload: vec![],
            };
            assert!(source_reader.observe_genesis(&non_genesis_env).is_none());

            let genesis_obs = source_reader
                .observe_genesis(&genesis_env)
                .expect("genesis observation");
            assert_eq!(genesis_obs.position(), 0);
            assert_eq!(genesis_obs.event_id(), &[9u8; 16]);
            let cursor = genesis_obs.cursor();
            assert_eq!(cursor.position(), 0);
            assert_eq!(cursor.event_id(), &[9u8; 16]);
            assert_eq!(source_reader.resume_from(&cursor), 0);

            assert!(source_reader.observe_genesis(&genesis_env).is_none());

            let mut genesis_buf = Vec::with_capacity(81 + genesis_env.payload.len());
            genesis_env.header.encode(&mut genesis_buf);
            genesis_buf.extend_from_slice(&genesis_env.payload);
            let genesis_hash = *blake3::hash(&genesis_buf).as_bytes();

            let event_env = crate::encoding::EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: [10u8; 16],
                    fiber_id: [1u8; 16],
                    detached: false,
                    precursor: [9u8; 16],
                    precursor_hash: genesis_hash,
                },
                payload: vec![4, 5, 6],
            };
            let event_obs = source_reader
                .observe_event(&event_env)
                .expect("event observation");
            assert_eq!(event_obs.position(), 1);
            let cursor2 = event_obs.cursor();
            assert_eq!(cursor2.position(), 1);
            assert_eq!(cursor2.event_id(), &[10u8; 16]);
            assert_eq!(source_reader.resume_from(&cursor2), 1);

            let wrong_precursor_env = crate::encoding::EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: [12u8; 16],
                    fiber_id: [1u8; 16],
                    detached: false,
                    precursor: [99u8; 16],
                    precursor_hash: [1u8; 32],
                },
                payload: vec![],
            };
            assert!(source_reader.observe_event(&wrong_precursor_env).is_none());

            let fiber2_genesis_env = crate::encoding::EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: [30u8; 16],
                    fiber_id: [2u8; 16],
                    detached: false,
                    precursor: [0u8; 16],
                    precursor_hash: [0u8; 32],
                },
                payload: vec![],
            };
            let fiber2_obs = source_reader
                .observe_genesis(&fiber2_genesis_env)
                .expect("fiber 2 genesis");
            assert_eq!(fiber2_obs.position(), 2);

            ArtefactReader::with_reader("target-artefact", |target_reader| {
                let target_env = crate::encoding::EventEnvelope {
                    header: crate::encoding::EnvelopeHeader {
                        event_id: [20u8; 16],
                        fiber_id: [3u8; 16],
                        detached: false,
                        precursor: [0u8; 16],
                        precursor_hash: [0u8; 32],
                    },
                    payload: vec![7, 8, 9],
                };
                let target_obs = target_reader
                    .observe_genesis(&target_env)
                    .expect("target observation");
                let regenerated = target_reader
                    .regenerate_cursor_for_migration(&cursor2, &target_obs)
                    .expect("regenerate cursor");

                assert_eq!(regenerated.position(), 0);
                assert_eq!(regenerated.event_id(), &[20u8; 16]);
                assert_eq!(target_reader.resume_from(&regenerated), 0);
            });
        });
    }

    #[test]
    fn test_artefact_reader_bounds_active_tracked_fibers_and_checked_position() {
        ArtefactReader::with_reader("bounded-reader", |reader| {
            for i in 0..MAX_ACTIVE_FIBERS {
                let mut fiber_id = [0u8; 16];
                fiber_id[0..4].copy_from_slice(&(i as u32).to_le_bytes());
                let env = crate::encoding::EventEnvelope {
                    header: crate::encoding::EnvelopeHeader {
                        event_id: fiber_id,
                        fiber_id,
                        detached: false,
                        precursor: [0u8; 16],
                        precursor_hash: [0u8; 32],
                    },
                    payload: vec![],
                };
                assert!(reader.observe_genesis(&env).is_some());
            }

            let mut overflow_fiber = [0u8; 16];
            overflow_fiber[0..4].copy_from_slice(&(MAX_ACTIVE_FIBERS as u32).to_le_bytes());
            let overflow_env = crate::encoding::EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: overflow_fiber,
                    fiber_id: overflow_fiber,
                    detached: false,
                    precursor: [0u8; 16],
                    precursor_hash: [0u8; 32],
                },
                payload: vec![],
            };
            assert!(reader.observe_genesis(&overflow_env).is_none());
        });
    }

    #[test]
    fn test_artefact_reader_with_stream_and_read_event_authorized_consumption() {
        let fiber_id = [1u8; 16];
        let genesis_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id,
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![10, 20, 30],
        };

        let genesis_commitment = genesis_env.commitment();

        let event_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [2u8; 16],
                fiber_id,
                detached: false,
                precursor: [1u8; 16],
                precursor_hash: genesis_commitment,
            },
            payload: vec![40, 50],
        };

        ArtefactReader::with_stream(
            "test-stream",
            vec![genesis_env.clone(), event_env.clone()],
            |reader| {
                let obs1 = reader
                    .read_event()
                    .expect("first event")
                    .expect("genesis ok");
                assert_eq!(obs1.position(), 0);
                assert_eq!(obs1.event_id(), &[1u8; 16]);
                assert!(obs1.is_genesis());

                let obs2 = reader
                    .read_event()
                    .expect("second event")
                    .expect("event ok");
                assert_eq!(obs2.position(), 1);
                assert_eq!(obs2.event_id(), &[2u8; 16]);
                assert!(!obs2.is_genesis());

                assert!(reader.read_event().is_none());
            },
        );

        let bad_event = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [3u8; 16],
                fiber_id,
                detached: false,
                precursor: [1u8; 16],
                precursor_hash: [0xee; 32],
            },
            payload: vec![],
        };
        ArtefactReader::with_stream(
            "bad-stream",
            vec![genesis_env.clone(), bad_event],
            |reader| {
                assert!(reader.read_event().expect("genesis").is_ok());
                let err = reader.read_event().expect("bad event").unwrap_err();
                assert_eq!(
                    err.condition(),
                    &FailureCondition::PrecursorChainBroken(None)
                );
            },
        );
    }

    #[test]
    fn test_artefact_reader_with_frames_decoding() {
        let genesis_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [10u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![1, 2, 3],
        };
        let mut env_bytes = Vec::new();
        genesis_env.encode(&mut env_bytes);
        let frame = crate::file::ContainerFrame::new(env_bytes);

        let res = ArtefactReader::with_frames("frame-reader", vec![frame], |reader| {
            let obs = reader
                .read_event()
                .expect("frame event")
                .expect("genesis ok");
            assert_eq!(obs.position(), 0);
            assert_eq!(obs.event_id(), &[10u8; 16]);
            obs.position()
        });
        assert_eq!(res.unwrap(), 0);
    }

    #[test]
    fn test_h2_with_frames_corrupted_checksum() {
        let genesis_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [10u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![1, 2, 3],
        };
        let mut env_bytes = Vec::new();
        genesis_env.encode(&mut env_bytes);
        let mut frame = crate::file::ContainerFrame::new(env_bytes);
        frame.checksum ^= 0xffff_ffff;

        let res = ArtefactReader::with_frames("frame-reader", vec![frame], |_reader| 42);
        let err = res.unwrap_err();
        assert_eq!(err.condition(), &FailureCondition::EnvelopeMismatch);
        assert!(err
            .diagnostic_detail()
            .message()
            .contains("ChecksumMismatch"));
    }

    #[test]
    fn test_h2_with_frames_trailing_bytes_rejected() {
        let genesis_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [10u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![1, 2, 3],
        };
        let mut env_bytes = Vec::new();
        genesis_env.encode(&mut env_bytes);
        env_bytes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        let frame = crate::file::ContainerFrame::new(env_bytes.clone());

        let res = ArtefactReader::with_frames("frame-reader", vec![frame], |_reader| 42);
        let err = res.unwrap_err();
        assert_eq!(err.condition(), &FailureCondition::EnvelopeMismatch);
        assert!(err
            .diagnostic_detail()
            .message()
            .contains("TruncatedPayload"));
    }

    #[test]
    fn test_h2_with_frames_truncated_header_rejected() {
        let frame = crate::file::ContainerFrame::new(vec![1, 2, 3, 4, 5]);
        let res = ArtefactReader::with_frames("frame-reader", vec![frame], |_reader| 42);
        let err = res.unwrap_err();
        assert_eq!(err.condition(), &FailureCondition::EnvelopeMismatch);
        assert!(err
            .diagnostic_detail()
            .message()
            .contains("TruncatedHeader"));
    }

    #[test]
    fn test_l2_with_frames_truncated_payload_rejected() {
        let genesis_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [10u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![1, 2, 3, 4, 5],
        };
        let mut env_bytes = Vec::new();
        genesis_env.header.encode(&mut env_bytes);
        assert_eq!(env_bytes.len(), 81);
        env_bytes.extend_from_slice(&(100u32).to_le_bytes());
        env_bytes.extend_from_slice(&[1, 2, 3]);
        let decode_res = crate::encoding::EventEnvelope::decode(&env_bytes);
        assert_eq!(
            decode_res.unwrap_err(),
            crate::encoding::DecodeError::TruncatedPayload {
                expected: 81 + 4 + 100,
                available: 81 + 4 + 3,
            }
        );
        let frame = crate::file::ContainerFrame::new(env_bytes);
        let mut callback_invoked = false;
        let res = ArtefactReader::with_frames("frame-reader", vec![frame], |_reader| {
            callback_invoked = true;
            42
        });
        assert!(!callback_invoked);
        let err = res.unwrap_err();
        assert_eq!(err.condition(), &FailureCondition::EnvelopeMismatch);
        assert!(err
            .diagnostic_detail()
            .message()
            .contains("TruncatedPayload"));
    }

    #[test]
    fn test_h1_terminal_refusal_on_discovered_break() {
        let env_a = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![1],
        };
        let env_b_broken = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [2u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0xee; 32],
            },
            payload: vec![2],
        };
        let env_c = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [3u8; 16],
                fiber_id: [2u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![3],
        };

        ArtefactReader::with_stream(
            "test-stream-terminal",
            vec![env_a, env_b_broken, env_c],
            |reader| {
                let obs_a = reader.read_event().expect("first event").expect("ok");
                assert_eq!(obs_a.position(), 0);
                assert_eq!(obs_a.event_id(), &[1u8; 16]);

                let err_b = reader.read_event().expect("second event").unwrap_err();
                assert_eq!(
                    err_b.condition(),
                    &FailureCondition::PrecursorChainBroken(None)
                );

                assert!(reader.read_event().is_none());
                assert!(reader.read_event().is_none());
            },
        );
    }

    #[test]
    fn test_m2_lawful_mapping_in_read_event() {
        let genesis_fiber1 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![10],
        };
        let g1_commitment = genesis_fiber1.commitment();

        let event_fiber1 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [2u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [1u8; 16],
                precursor_hash: g1_commitment,
            },
            payload: vec![20],
        };
        let _e1_commitment = event_fiber1.commitment();

        let genesis_fiber2 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [3u8; 16],
                fiber_id: [2u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![30],
        };

        let wrong_fiber_event = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [4u8; 16],
                fiber_id: [2u8; 16],
                detached: false,
                precursor: [1u8; 16],
                precursor_hash: g1_commitment,
            },
            payload: vec![40],
        };

        ArtefactReader::with_stream(
            "stream-wrong-fiber",
            vec![
                genesis_fiber1.clone(),
                genesis_fiber2.clone(),
                wrong_fiber_event,
            ],
            |reader| {
                assert!(reader.read_event().expect("first").is_ok());
                assert!(reader.read_event().expect("second").is_ok());
                let err = reader.read_event().expect("third").unwrap_err();
                assert_eq!(
                    err.condition(),
                    &FailureCondition::PrecursorChainBroken(Some(
                        CausalChainError::PrecursorWrongFiber
                    ))
                );
            },
        );

        let out_of_range_event = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [5u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [99u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![50],
        };

        ArtefactReader::with_stream(
            "stream-out-of-range",
            vec![genesis_fiber1.clone(), out_of_range_event],
            |reader| {
                assert!(reader.read_event().expect("first").is_ok());
                let err = reader.read_event().expect("second").unwrap_err();
                assert_eq!(
                    err.condition(),
                    &FailureCondition::PrecursorChainBroken(Some(
                        CausalChainError::PrecursorOutOfRange
                    ))
                );
            },
        );

        let stale_event = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [6u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [1u8; 16],
                precursor_hash: g1_commitment,
            },
            payload: vec![60],
        };

        ArtefactReader::with_stream(
            "stream-stale",
            vec![genesis_fiber1.clone(), event_fiber1.clone(), stale_event],
            |reader| {
                assert!(reader.read_event().expect("first").is_ok());
                assert!(reader.read_event().expect("second").is_ok());
                let err = reader.read_event().expect("third").unwrap_err();
                assert_eq!(
                    err.condition(),
                    &FailureCondition::PrecursorChainBroken(None)
                );
            },
        );

        let bad_hash_event = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [7u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [2u8; 16],
                precursor_hash: [0xff; 32],
            },
            payload: vec![70],
        };

        ArtefactReader::with_stream(
            "stream-bad-hash",
            vec![genesis_fiber1, event_fiber1, bad_hash_event],
            |reader| {
                assert!(reader.read_event().expect("first").is_ok());
                assert!(reader.read_event().expect("second").is_ok());
                let err = reader.read_event().expect("third").unwrap_err();
                assert_eq!(
                    err.condition(),
                    &FailureCondition::PrecursorChainBroken(None)
                );
            },
        );
    }

    #[test]
    fn test_l1_observed_position_counter_overflow_boundary() {
        let genesis_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [100u8; 16],
                fiber_id: [100u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![1, 2, 3],
        };
        ArtefactReader::with_stream("overflow-stream", vec![genesis_env], |reader| {
            reader.state.borrow_mut().observed_position = u64::MAX;
            let err = reader.read_event().expect("read attempted").unwrap_err();
            assert_eq!(
                err.condition(),
                &FailureCondition::ValueConstraintViolated {
                    constraint: crate::encoding::ValueConstraint::TooLong,
                }
            );
            assert_eq!(reader.state.borrow().observed_position, u64::MAX);
            assert!(reader.read_event().is_none());
        });
    }

    #[test]
    fn test_l1_public_capacity_refusal_via_read_event() {
        let mut fibers_map = std::collections::BTreeMap::new();
        for i in 0..MAX_ACTIVE_FIBERS {
            let mut fid = [0u8; 16];
            fid[0..4].copy_from_slice(&(i as u32).to_le_bytes());
            fibers_map.insert(
                fid,
                FiberTracking {
                    last_event_id: fid,
                    last_commitment_hash: [0u8; 32],
                },
            );
        }

        let overflow_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0xff; 16],
                fiber_id: [0xff; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![],
        };

        ArtefactReader::with_stream("capacity-stream", vec![overflow_env], |reader| {
            reader.state.borrow_mut().fibers = fibers_map;
            let err = reader.read_event().expect("read attempted").unwrap_err();
            assert_eq!(
                err.condition(),
                &FailureCondition::ValueConstraintViolated {
                    constraint: crate::encoding::ValueConstraint::TooLong,
                }
            );
            assert_eq!(reader.state.borrow().observed_position, 0);
            assert!(reader.read_event().is_none());
        });
    }

    #[test]
    fn test_l3_later_genesis_migration_target_position() {
        let source_genesis = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![],
        };
        let target_g1 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [10u8; 16],
                fiber_id: [10u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![],
        };
        let target_g2 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [20u8; 16],
                fiber_id: [20u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![],
        };

        ArtefactReader::with_stream("source", vec![source_genesis], |source_reader| {
            let source_obs = source_reader.read_event().unwrap().unwrap();
            let source_cursor = source_obs.cursor();

            ArtefactReader::with_stream("target", vec![target_g1, target_g2], |target_reader| {
                let _obs1 = target_reader.read_event().unwrap().unwrap();
                let obs2 = target_reader.read_event().unwrap().unwrap();
                assert_eq!(obs2.position(), 1);

                let regenerated = target_reader
                    .regenerate_cursor_for_migration(&source_cursor, &obs2)
                    .expect("regenerated cursor for later genesis");
                assert_eq!(regenerated.position(), 1);
                assert_eq!(regenerated.event_id(), &[20u8; 16]);
                assert_eq!(target_reader.resume_from(&regenerated), 1);
            });
        });
    }

    #[test]
    fn test_qualified_open_result_valid_and_illegal_combinations() {
        let orphan = QualifiedOpenResult::orphan_read();
        assert_eq!(*orphan.ownership(), OwnershipStatus::Unowned);
        assert_eq!(orphan.generation(), GenerationKnowledge::Unknown);
        assert_eq!(orphan.supersession(), SupersessionStatus::Unknown);
        assert_eq!(orphan.append_authority(), AppendAuthority::Unknown);

        let coexisting = QualifiedOpenResult::builder()
            .ownership(OwnershipStatus::Owned { epoch: 5 })
            .generation(GenerationKnowledge::Known(1))
            .supersession(SupersessionStatus::Superseded {
                next_generation: Some(2),
            })
            .migration_disagreement(MigrationDisagreement::OutboundWithoutInbound)
            .history_integrity(HistoryIntegrity::Anchored)
            .migration_completeness(MigrationCompleteness::KnownComplete)
            .append_authority(AppendAuthority::HeldByGeneration(2))
            .build();
        assert!(coexisting.is_ok());
        let res = coexisting.unwrap();
        assert_eq!(
            res.migration_disagreement(),
            MigrationDisagreement::OutboundWithoutInbound
        );
        assert_eq!(
            res.supersession(),
            SupersessionStatus::Superseded {
                next_generation: Some(2)
            }
        );

        let partial_target = QualifiedOpenResult::builder()
            .ownership(OwnershipStatus::Owned { epoch: 10 })
            .generation(GenerationKnowledge::Known(2))
            .supersession(SupersessionStatus::Current)
            .migration_disagreement(MigrationDisagreement::None)
            .history_integrity(HistoryIntegrity::Anchored)
            .migration_completeness(MigrationCompleteness::Unknown)
            .append_authority(AppendAuthority::Unknown)
            .build();
        assert!(partial_target.is_ok());

        let illegal_unowned_gen = QualifiedOpenResult::builder()
            .ownership(OwnershipStatus::Unowned)
            .generation(GenerationKnowledge::Known(1))
            .build();
        assert_eq!(
            illegal_unowned_gen,
            Err(IllegalOpenCombination::UnownedMustHaveUnknownGeneration)
        );

        let illegal_unknown_current = QualifiedOpenResult::builder()
            .ownership(OwnershipStatus::Owned { epoch: 1 })
            .generation(GenerationKnowledge::Unknown)
            .supersession(SupersessionStatus::Current)
            .build();
        assert_eq!(
            illegal_unknown_current,
            Err(IllegalOpenCombination::UnknownGenerationCannotBeCurrent)
        );

        let illegal_unowned_authority = QualifiedOpenResult::builder()
            .ownership(OwnershipStatus::Unowned)
            .generation(GenerationKnowledge::Unknown)
            .append_authority(AppendAuthority::HoldsAuthority)
            .build();
        assert_eq!(
            illegal_unowned_authority,
            Err(IllegalOpenCombination::UnownedCannotHoldAppendAuthority)
        );
    }

    #[test]
    fn test_ownership_fence_and_indeterminate_verdicts() {
        let carried = 5;
        let claim = OwnershipClaimRecord {
            epoch: 5,
            machine_id: [1u8; 16],
            boot_id: [2u8; 16],
            process_id: 100,
            process_start_time_ns: 1000,
            claim_time_ns: 2000,
            operator_label: "test".to_string(),
        };

        assert!(
            OwnershipFence::fence_write(carried, &RecordedOwnership::Claimed(claim.clone()))
                .is_ok()
        );

        let stale = OwnershipFence::fence_write(4, &RecordedOwnership::Claimed(claim));
        assert!(stale.is_err());
        assert_eq!(
            stale.unwrap_err().condition(),
            &FailureCondition::StaleEpoch
        );

        let unreadable = OwnershipFence::fence_write(carried, &RecordedOwnership::Unreadable);
        assert!(unreadable.is_err());
        assert_eq!(
            unreadable.unwrap_err().condition(),
            &FailureCondition::OwnershipRecordUnreadable
        );

        let absent = OwnershipFence::fence_write(carried, &RecordedOwnership::Absent);
        assert!(absent.is_err());
        assert_eq!(
            absent.unwrap_err().condition(),
            &FailureCondition::OwnershipUnestablished
        );

        let unowned = OwnershipFence::fence_write(carried, &RecordedOwnership::Unowned);
        assert!(unowned.is_err());
        assert_eq!(
            unowned.unwrap_err().condition(),
            &FailureCondition::OwnershipUnestablished
        );

        assert_eq!(
            evaluate_owner_liveness(Some(DeathProof::MachineReboot)),
            LivenessVerdict::ProvenDead {
                proof: DeathProof::MachineReboot
            }
        );
        assert_eq!(
            evaluate_owner_liveness(None),
            LivenessVerdict::Indeterminate
        );

        let verdict = WriteLandingVerdict::<u64>::Undetermined { carried_epoch: 5 };
        assert_eq!(
            verdict,
            WriteLandingVerdict::Undetermined { carried_epoch: 5 }
        );

        let clean = CleanReleaseProof {
            epoch: 5,
            release_time_ns: 9999,
        };
        assert_eq!(clean.epoch, 5);
        assert_eq!(clean.release_time_ns, 9999);
    }

    #[test]
    fn test_operation_failure_and_conditions() {
        let condition =
            FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorOutOfRange));
        let failure = OperationFailure::new(condition.clone(), "test diagnostic");
        assert_eq!(failure.condition(), &condition);
        assert_eq!(failure.diagnostic_detail().message(), "test diagnostic");
        assert_eq!(
            format!("{failure}"),
            "precursor chain broken: precursor event ID is outside the artefact: test diagnostic"
        );
        use std::error::Error;
        assert!(failure.source().is_some());

        let broken_none = OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            "integrity break",
        );
        assert!(broken_none.source().is_none());
        assert_eq!(
            format!("{broken_none}"),
            "precursor chain broken: integrity break"
        );
    }

    #[test]
    fn test_event_admission_all_branches() {
        use crate::schema::DescriptorNode;
        let descriptor = SchemaDescriptor::new(1, DescriptorNode::U8);
        let expected_schema = SchemaIdentity::from_raw([1; 32]);
        let other_schema = SchemaIdentity::from_raw([2; 32]);
        let expected_envelope = EnvelopeIdentity::from_raw([10; 32]);
        let other_envelope = EnvelopeIdentity::from_raw([20; 32]);

        let valid_event = EventAdmission::Event {
            descriptor: Some(&descriptor),
            expected_schema: &expected_schema,
            actual_schema: &expected_schema,
            expected_envelope: &expected_envelope,
            actual_envelope: &expected_envelope,
        };
        assert!(admit_event(&valid_event).is_ok());

        let missing_desc_event = EventAdmission::Event {
            descriptor: None,
            expected_schema: &expected_schema,
            actual_schema: &expected_schema,
            expected_envelope: &expected_envelope,
            actual_envelope: &expected_envelope,
        };
        assert_eq!(
            admit_event(&missing_desc_event).unwrap_err().condition(),
            &FailureCondition::MissingSchemaDescriptor
        );

        let env_mismatch_event = EventAdmission::Event {
            descriptor: Some(&descriptor),
            expected_schema: &expected_schema,
            actual_schema: &expected_schema,
            expected_envelope: &expected_envelope,
            actual_envelope: &other_envelope,
        };
        assert_eq!(
            admit_event(&env_mismatch_event).unwrap_err().condition(),
            &FailureCondition::EnvelopeMismatch
        );

        let schema_mismatch_event = EventAdmission::Event {
            descriptor: Some(&descriptor),
            expected_schema: &expected_schema,
            actual_schema: &other_schema,
            expected_envelope: &expected_envelope,
            actual_envelope: &expected_envelope,
        };
        assert_eq!(
            admit_event(&schema_mismatch_event).unwrap_err().condition(),
            &FailureCondition::SchemaMismatch
        );

        let valid_record = EventAdmission::RecordReaderOnly {
            descriptor: Some(&descriptor),
            expected_envelope: &expected_envelope,
            actual_envelope: &expected_envelope,
        };
        assert!(admit_event(&valid_record).is_ok());

        let missing_desc_record = EventAdmission::RecordReaderOnly {
            descriptor: None,
            expected_envelope: &expected_envelope,
            actual_envelope: &expected_envelope,
        };
        assert_eq!(
            admit_event(&missing_desc_record).unwrap_err().condition(),
            &FailureCondition::MissingSchemaDescriptor
        );

        let env_mismatch_record = EventAdmission::RecordReaderOnly {
            descriptor: Some(&descriptor),
            expected_envelope: &expected_envelope,
            actual_envelope: &other_envelope,
        };
        assert_eq!(
            admit_event(&env_mismatch_record).unwrap_err().condition(),
            &FailureCondition::EnvelopeMismatch
        );

        let undetermined = EventAdmission::MismatchUndeterminedSubject {
            detail: "unestablished subject",
        };
        assert_eq!(
            admit_event(&undetermined).unwrap_err().condition(),
            &FailureCondition::MismatchUndeterminedSubject
        );
    }

    #[test]
    fn test_precursor_link_validation() {
        let fiber_id = [1u8; 16];
        let other_fiber = [2u8; 16];

        let genesis_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [10u8; 16],
                fiber_id,
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![1, 2, 3],
        };
        let genesis_link = PrecursorLink::classify(&genesis_env).expect("genesis link");
        assert!(genesis_link.is_genesis());
        assert_eq!(genesis_link.envelope(), &genesis_env);
        assert!(admit_precursor_link(&fiber_id, &genesis_link, |_| None).is_ok());

        assert_eq!(
            admit_precursor_link(&other_fiber, &genesis_link, |_| None)
                .unwrap_err()
                .condition(),
            &FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorWrongFiber))
        );

        let malformed_genesis = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [10u8; 16],
                fiber_id,
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [99u8; 32],
            },
            payload: vec![],
        };
        assert_eq!(
            PrecursorLink::classify(&malformed_genesis)
                .unwrap_err()
                .condition(),
            &FailureCondition::PrecursorChainBroken(None)
        );

        let mut genesis_buf = Vec::with_capacity(81 + genesis_env.payload.len());
        genesis_env.header.encode(&mut genesis_buf);
        genesis_buf.extend_from_slice(&genesis_env.payload);
        let computed_genesis_hash = *blake3::hash(&genesis_buf).as_bytes();

        let pred_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [20u8; 16],
                fiber_id,
                detached: false,
                precursor: [10u8; 16],
                precursor_hash: computed_genesis_hash,
            },
            payload: vec![4, 5, 6],
        };
        let pred_link = PrecursorLink::classify(&pred_env).expect("pred link");
        assert!(!pred_link.is_genesis());
        assert_eq!(pred_link.envelope(), &pred_env);

        assert!(admit_precursor_link(&fiber_id, &pred_link, |id| {
            if id == &[10u8; 16] {
                Some(&genesis_env)
            } else {
                None
            }
        })
        .is_ok());

        let wrong_id_pred = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [99u8; 16],
                fiber_id,
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![],
        };
        assert_eq!(
            admit_precursor_link(&fiber_id, &pred_link, |_id| { Some(&wrong_id_pred) })
                .unwrap_err()
                .condition(),
            &FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorOutOfRange))
        );

        assert_eq!(
            admit_precursor_link(&other_fiber, &pred_link, |id| {
                if id == &[10u8; 16] {
                    Some(&genesis_env)
                } else {
                    None
                }
            })
            .unwrap_err()
            .condition(),
            &FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorWrongFiber))
        );

        assert_eq!(
            admit_precursor_link(&fiber_id, &pred_link, |_| None)
                .unwrap_err()
                .condition(),
            &FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorOutOfRange))
        );

        let wrong_fiber_pred = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [10u8; 16],
                fiber_id: other_fiber,
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![],
        };
        assert_eq!(
            admit_precursor_link(&fiber_id, &pred_link, |id| {
                if id == &[10u8; 16] {
                    Some(&wrong_fiber_pred)
                } else {
                    None
                }
            })
            .unwrap_err()
            .condition(),
            &FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorWrongFiber))
        );

        let tampered_genesis = crate::encoding::EventEnvelope {
            header: genesis_env.header.clone(),
            payload: vec![1, 2, 99],
        };
        assert_eq!(
            admit_precursor_link(&fiber_id, &pred_link, |id| {
                if id == &[10u8; 16] {
                    Some(&tampered_genesis)
                } else {
                    None
                }
            })
            .unwrap_err()
            .condition(),
            &FailureCondition::PrecursorChainBroken(None)
        );

        let bad_hash_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [20u8; 16],
                fiber_id,
                detached: false,
                precursor: [10u8; 16],
                precursor_hash: [0xfe; 32],
            },
            payload: vec![4, 5, 6],
        };
        let bad_hash_link = PrecursorLink::classify(&bad_hash_env).expect("bad hash link");
        assert_eq!(
            admit_precursor_link(&fiber_id, &bad_hash_link, |id| {
                if id == &[10u8; 16] {
                    Some(&genesis_env)
                } else {
                    None
                }
            })
            .unwrap_err()
            .condition(),
            &FailureCondition::PrecursorChainBroken(None)
        );
    }

    #[test]
    fn test_ordered_creation_domain_protocol() {
        let step1 = CreationPlan::begin(ArtefactPresence::None).expect("begin create");
        assert_eq!(step1, CreationProgression::UnseededOwnershipRecordCreated);

        assert_eq!(
            CreationPlan::complete_event_data(step1.clone())
                .unwrap_err()
                .condition(),
            &FailureCondition::OwnershipUnestablished
        );

        assert_eq!(
            CreationPlan::begin(ArtefactPresence::Both)
                .unwrap_err()
                .condition(),
            &FailureCondition::StoreAlreadyExists
        );

        let claim = OwnershipClaimRecord {
            epoch: 1,
            machine_id: [1; 16],
            boot_id: [2; 16],
            process_id: 10,
            process_start_time_ns: 100,
            claim_time_ns: 200,
            operator_label: "creator".to_string(),
        };

        let step2 = CreationPlan::seed_claim(step1, claim.clone()).expect("seed claim");
        assert_eq!(
            step2,
            CreationProgression::OwnershipClaimSeeded(claim.clone())
        );

        assert_eq!(
            CreationPlan::seed_claim(step2.clone(), claim.clone())
                .unwrap_err()
                .condition(),
            &FailureCondition::ConcurrencyConflict
        );

        let step3 = CreationPlan::complete_event_data(step2).expect("complete event data");
        assert_eq!(step3, CreationProgression::Complete);
        assert_eq!(step3.presence(), ArtefactPresence::Both);

        assert_eq!(
            CreationPlan::complete_event_data(step3)
                .unwrap_err()
                .condition(),
            &FailureCondition::StoreAlreadyExists
        );

        let resumed_unseeded =
            CreationPlan::resume_incomplete_creation(ArtefactPresence::OwnershipRecordOnly, None)
                .expect("resume unseeded");
        assert_eq!(resumed_unseeded, IncompleteCreationState::Unseeded);

        let resumed_claimed = CreationPlan::resume_incomplete_creation(
            ArtefactPresence::OwnershipRecordOnly,
            Some(claim.clone()),
        )
        .expect("resume claimed");
        assert_eq!(
            resumed_claimed,
            IncompleteCreationState::Claimed(claim.clone())
        );

        let step_from_claimed = CreationProgression::from(resumed_claimed);
        assert_eq!(
            CreationPlan::complete_event_data(step_from_claimed).expect("complete from claimed"),
            CreationProgression::Complete
        );

        assert_eq!(
            CreationPlan::resume_incomplete_creation(ArtefactPresence::None, None)
                .unwrap_err()
                .condition(),
            &FailureCondition::NoArtefactExists
        );

        let incomplete_open_unseeded =
            admit_open(ArtefactPresence::OwnershipRecordOnly, None, false).unwrap();
        assert_eq!(
            incomplete_open_unseeded,
            OpenAdmission::IncompleteCreation(IncompleteCreationState::Unseeded)
        );

        let incomplete_open_claimed =
            admit_open(ArtefactPresence::OwnershipRecordOnly, Some(claim), false).unwrap();
        assert_eq!(
            incomplete_open_claimed,
            OpenAdmission::IncompleteCreation(IncompleteCreationState::Claimed(
                OwnershipClaimRecord {
                    epoch: 1,
                    machine_id: [1; 16],
                    boot_id: [2; 16],
                    process_id: 10,
                    process_start_time_ns: 100,
                    claim_time_ns: 200,
                    operator_label: "creator".to_string(),
                }
            ))
        );
    }

    #[test]
    fn test_h1_with_stream_empty_at_limit_over_limit() {
        ArtefactReader::with_stream("empty-stream", std::iter::empty(), |reader| {
            assert!(reader.read_event().is_none());
        });

        let at_limit_stream = (0..MAX_STREAM_ITEMS).map(|i| {
            let mut id = [0u8; 16];
            id[0..4].copy_from_slice(&(i as u32).to_le_bytes());
            crate::encoding::EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: id,
                    fiber_id: id,
                    detached: false,
                    precursor: [0u8; 16],
                    precursor_hash: [0u8; 32],
                },
                payload: vec![],
            }
        });
        ArtefactReader::with_stream("at-limit-stream", at_limit_stream, |reader| {
            let obs = reader
                .read_event()
                .expect("first event")
                .expect("genesis ok");
            assert_eq!(obs.position(), 0);
        });

        let over_limit_stream = (0..=MAX_STREAM_ITEMS).map(|i| {
            let mut id = [0u8; 16];
            id[0..4].copy_from_slice(&(i as u32).to_le_bytes());
            crate::encoding::EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: id,
                    fiber_id: id,
                    detached: false,
                    precursor: [0u8; 16],
                    precursor_hash: [0u8; 32],
                },
                payload: vec![],
            }
        });
        ArtefactReader::with_stream("over-limit-stream", over_limit_stream, |reader| {
            let err = reader.read_event().expect("over-limit event").unwrap_err();
            assert_eq!(
                err.condition(),
                &FailureCondition::ValueConstraintViolated {
                    constraint: crate::encoding::ValueConstraint::TooLong,
                }
            );
            assert_eq!(
                err.diagnostic_detail().message(),
                "stream item limit exceeded"
            );
            assert!(reader.read_event().is_none());
        });
    }

    #[test]
    fn test_h1_with_frames_empty_at_limit_over_limit() {
        let empty_res = ArtefactReader::with_frames(
            "empty-frames",
            Vec::<crate::file::ContainerFrame>::new(),
            |_reader| 42,
        );
        assert_eq!(empty_res.unwrap(), 42);

        let at_limit_frames = (0..MAX_STREAM_ITEMS).map(|i| {
            let mut id = [0u8; 16];
            id[0..4].copy_from_slice(&(i as u32).to_le_bytes());
            let env = crate::encoding::EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: id,
                    fiber_id: id,
                    detached: false,
                    precursor: [0u8; 16],
                    precursor_hash: [0u8; 32],
                },
                payload: vec![],
            };
            let mut buf = Vec::new();
            env.encode(&mut buf);
            crate::file::ContainerFrame::new(buf)
        });
        let at_limit_res =
            ArtefactReader::with_frames("at-limit-frames", at_limit_frames, |_reader| 99);
        assert_eq!(at_limit_res.unwrap(), 99);

        let over_limit_frames = (0..=MAX_STREAM_ITEMS).map(|i| {
            let mut id = [0u8; 16];
            id[0..4].copy_from_slice(&(i as u32).to_le_bytes());
            let env = crate::encoding::EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: id,
                    fiber_id: id,
                    detached: false,
                    precursor: [0u8; 16],
                    precursor_hash: [0u8; 32],
                },
                payload: vec![],
            };
            let mut buf = Vec::new();
            env.encode(&mut buf);
            crate::file::ContainerFrame::new(buf)
        });
        let mut invoked = false;
        let over_limit_res =
            ArtefactReader::with_frames("over-limit-frames", over_limit_frames, |_reader| {
                invoked = true;
                100
            });
        assert!(!invoked);
        let err = over_limit_res.unwrap_err();
        assert_eq!(
            err.condition(),
            &FailureCondition::ValueConstraintViolated {
                constraint: crate::encoding::ValueConstraint::TooLong,
            }
        );
        assert_eq!(
            err.diagnostic_detail().message(),
            "frame item limit exceeded"
        );
    }

    #[test]
    fn test_h2_duplicate_event_id_rejection() {
        let genesis_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![1, 2, 3],
        };
        let g_hash = genesis_env.commitment();
        let dup_same_fiber = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [1u8; 16],
                precursor_hash: g_hash,
            },
            payload: vec![4, 5],
        };

        ArtefactReader::with_stream(
            "dup-same-fiber",
            vec![genesis_env.clone(), dup_same_fiber],
            |reader| {
                let obs1 = reader
                    .read_event()
                    .expect("first event")
                    .expect("genesis ok");
                assert_eq!(obs1.position(), 0);
                assert_eq!(obs1.event_id(), &[1u8; 16]);

                let err = reader.read_event().expect("second event").unwrap_err();
                assert_eq!(
                    err.condition(),
                    &FailureCondition::PrecursorChainBroken(None)
                );
                assert!(err
                    .diagnostic_detail()
                    .message()
                    .contains("duplicate event ID"));
                assert_eq!(reader.state.borrow().observed_position, 1);
                assert!(reader.read_event().is_none());
            },
        );

        let dup_other_fiber = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [2u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![7, 8],
        };

        ArtefactReader::with_stream(
            "dup-cross-fiber",
            vec![genesis_env.clone(), dup_other_fiber],
            |reader| {
                let obs1 = reader
                    .read_event()
                    .expect("first event")
                    .expect("genesis ok");
                assert_eq!(obs1.position(), 0);

                let err = reader.read_event().expect("second event").unwrap_err();
                assert_eq!(
                    err.condition(),
                    &FailureCondition::PrecursorChainBroken(None)
                );
                assert!(err
                    .diagnostic_detail()
                    .message()
                    .contains("duplicate event ID"));
                assert_eq!(
                    reader.state.borrow().observed_events.get(&[1u8; 16]),
                    Some(&[1u8; 16])
                );
                assert_eq!(reader.state.borrow().observed_position, 1);
                assert!(reader.read_event().is_none());
            },
        );
    }

    #[test]
    fn test_m1_stream_byte_budget_enforcement() {
        let big_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![0u8; MAX_STREAM_BYTES],
        };
        ArtefactReader::with_stream("over-bytes-stream", vec![big_env], |reader| {
            let err = reader.read_event().expect("over-budget event").unwrap_err();
            assert_eq!(
                err.condition(),
                &FailureCondition::ValueConstraintViolated {
                    constraint: crate::encoding::ValueConstraint::TooLong,
                }
            );
            assert_eq!(
                err.diagnostic_detail().message(),
                "stream byte limit exceeded"
            );
            assert!(reader.read_event().is_none());
        });

        let env1 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![1, 2, 3],
        };
        let c1 = env1.commitment();
        let env2 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [2u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [1u8; 16],
                precursor_hash: c1,
            },
            payload: vec![4, 5, 6, 7],
        };
        ArtefactReader::with_stream("byte-release-stream", vec![env1, env2], |reader| {
            let initial_retained = reader.state.borrow().retained_bytes;
            assert_eq!(initial_retained, (85 + 3) + (85 + 4));

            let _ = reader.read_event().expect("first").unwrap();
            let after_first = reader.state.borrow().retained_bytes;
            assert_eq!(after_first, 85 + 4);

            let _ = reader.read_event().expect("second").unwrap();
            let after_second = reader.state.borrow().retained_bytes;
            assert_eq!(after_second, 0);
        });
    }

    #[test]
    fn test_m1_high_capacity_zero_length_payload_refusal() {
        let high_cap_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: Vec::with_capacity(MAX_STREAM_BYTES + 1),
        };
        assert_eq!(high_cap_env.payload.len(), 0);
        assert!(high_cap_env.payload.capacity() > MAX_STREAM_BYTES);

        ArtefactReader::with_stream("high-cap-stream", vec![high_cap_env], |reader| {
            let err = reader
                .read_event()
                .expect("refusal event")
                .expect_err("high capacity must be refused");
            assert_eq!(
                err.condition(),
                &FailureCondition::ValueConstraintViolated {
                    constraint: crate::encoding::ValueConstraint::TooLong,
                }
            );
            assert_eq!(
                err.diagnostic_detail().message(),
                "stream byte limit exceeded"
            );
            assert!(reader.read_event().is_none());
        });
    }

    #[test]
    fn test_m1_with_frames_byte_budget_refusal_before_decode_copy() {
        let frame = crate::file::ContainerFrame {
            checksum: 0xdead_beef,
            payload: vec![0u8; MAX_STREAM_BYTES + 1],
        };
        let mut invoked = false;
        let res = ArtefactReader::with_frames("over-budget-frame", vec![frame], |_reader| {
            invoked = true;
            42
        });
        assert!(!invoked);
        let err = res.unwrap_err();
        assert_eq!(
            err.condition(),
            &FailureCondition::ValueConstraintViolated {
                constraint: crate::encoding::ValueConstraint::TooLong,
            }
        );
        assert_eq!(
            err.diagnostic_detail().message(),
            "frame byte limit exceeded"
        );
    }

    #[test]
    fn test_m1_with_frames_exact_limit_accepted() {
        let env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![0u8; MAX_STREAM_BYTES - 85],
        };
        let mut encoded = Vec::with_capacity(MAX_STREAM_BYTES);
        env.encode(&mut encoded);
        assert_eq!(encoded.len(), MAX_STREAM_BYTES);
        let frame = crate::file::ContainerFrame::new(encoded);
        let res = ArtefactReader::with_frames("exact-64mib-frame", vec![frame], |reader| {
            assert_eq!(reader.state.borrow().retained_bytes, MAX_STREAM_BYTES);
            let obs = reader
                .read_event()
                .expect("some observation")
                .expect("valid genesis observation");
            assert_eq!(obs.position(), 0);
            assert!(obs.is_genesis());
            assert_eq!(obs.locator().as_str(), "exact-64mib-frame");
            assert_eq!(reader.state.borrow().retained_bytes, 0);
            assert!(reader.read_event().is_none());
            42
        });
        assert_eq!(res.unwrap(), 42);
    }

    #[test]
    fn test_m1_with_frames_one_over_limit_refused() {
        let frame = crate::file::ContainerFrame {
            checksum: 0x1234_5678,
            payload: vec![0u8; MAX_STREAM_BYTES + 1],
        };
        let res = ArtefactReader::with_frames("one-over-frame", vec![frame], |_reader| 42);
        let err = res.unwrap_err();
        assert_eq!(
            err.condition(),
            &FailureCondition::ValueConstraintViolated {
                constraint: crate::encoding::ValueConstraint::TooLong,
            }
        );
        assert_eq!(
            err.diagnostic_detail().message(),
            "frame byte limit exceeded"
        );
    }

    #[test]
    fn test_m1_with_frames_multi_envelope_boundary() {
        let p1_len = 1000usize;
        let env1 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![0u8; p1_len],
        };
        let c1 = env1.commitment();
        let mut enc1 = Vec::new();
        env1.encode(&mut enc1);
        assert_eq!(enc1.len(), 85 + p1_len);
        let frame1 = crate::file::ContainerFrame::new(enc1);

        let p2_len = MAX_STREAM_BYTES - (85 + p1_len) - 85;
        let env2 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [2u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [1u8; 16],
                precursor_hash: c1,
            },
            payload: vec![0u8; p2_len],
        };
        let mut enc2 = Vec::new();
        env2.encode(&mut enc2);
        assert_eq!(enc2.len(), 85 + p2_len);
        let frame2 = crate::file::ContainerFrame::new(enc2);

        let res = ArtefactReader::with_frames(
            "multi-frame-exact",
            vec![frame1.clone(), frame2],
            |reader| {
                assert_eq!(reader.state.borrow().retained_bytes, MAX_STREAM_BYTES);
                let _ = reader.read_event().expect("some").unwrap();
                let _ = reader.read_event().expect("some").unwrap();
                assert_eq!(reader.state.borrow().retained_bytes, 0);
                assert!(reader.read_event().is_none());
            },
        );
        assert!(res.is_ok());

        let over_frame2 = crate::file::ContainerFrame {
            checksum: 0xfeed_beef,
            payload: vec![0u8; (85 + p2_len) + 1],
        };
        let over_res = ArtefactReader::with_frames(
            "multi-frame-over",
            vec![frame1, over_frame2],
            |_reader| {},
        );
        let err = over_res.unwrap_err();
        assert_eq!(
            err.condition(),
            &FailureCondition::ValueConstraintViolated {
                constraint: crate::encoding::ValueConstraint::TooLong,
            }
        );
        assert_eq!(
            err.diagnostic_detail().message(),
            "frame byte limit exceeded"
        );
    }

    #[test]
    fn test_m1_with_stream_exact_limit_and_one_over() {
        let exact_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![0u8; MAX_STREAM_BYTES - 85],
        };
        ArtefactReader::with_stream("stream-exact", vec![exact_env], |reader| {
            assert_eq!(reader.state.borrow().retained_bytes, MAX_STREAM_BYTES);
            let obs = reader.read_event().expect("some").unwrap();
            assert_eq!(obs.position(), 0);
            assert_eq!(reader.state.borrow().retained_bytes, 0);
            assert!(reader.read_event().is_none());
        });

        let over_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![0u8; MAX_STREAM_BYTES - 85 + 1],
        };
        ArtefactReader::with_stream("stream-one-over", vec![over_env], |reader| {
            assert_eq!(reader.state.borrow().retained_bytes, 0);
            let err = reader.read_event().expect("some").unwrap_err();
            assert_eq!(
                err.condition(),
                &FailureCondition::ValueConstraintViolated {
                    constraint: crate::encoding::ValueConstraint::TooLong,
                }
            );
            assert_eq!(
                err.diagnostic_detail().message(),
                "stream byte limit exceeded"
            );
            assert!(reader.read_event().is_none());
        });
    }

    #[test]
    fn test_m2_terminal_failure_and_refusal_clears_queue_immediately() {
        let valid_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![1, 2, 3],
        };
        let high_cap_env = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [2u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [1u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: Vec::with_capacity(MAX_STREAM_BYTES + 1),
        };
        ArtefactReader::with_stream("intake-refusal", vec![valid_env, high_cap_env], |reader| {
            assert!(reader.state.borrow().stream.is_empty());
            assert_eq!(reader.state.borrow().stream.len(), 0);
            assert_eq!(reader.state.borrow().retained_bytes, 0);
            assert!(reader.state.borrow().terminally_failed);
            let err = reader.read_event().expect("some").expect_err("refused");
            assert_eq!(
                err.condition(),
                &FailureCondition::ValueConstraintViolated {
                    constraint: crate::encoding::ValueConstraint::TooLong,
                }
            );
            assert_eq!(
                err.diagnostic_detail().message(),
                "stream byte limit exceeded"
            );
            assert!(reader.state.borrow().stream.is_empty());
            assert_eq!(reader.state.borrow().stream.len(), 0);
            assert_eq!(reader.state.borrow().retained_bytes, 0);
            assert!(reader.read_event().is_none());
        });

        let item_envs = (0..=MAX_STREAM_ITEMS).map(|i| {
            let mut id = [0u8; 16];
            id[..8].copy_from_slice(&(i as u64).to_be_bytes());
            crate::encoding::EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: id,
                    fiber_id: [1u8; 16],
                    detached: false,
                    precursor: [0u8; 16],
                    precursor_hash: [0u8; 32],
                },
                payload: Vec::new(),
            }
        });
        ArtefactReader::with_stream("item-intake-refusal", item_envs, |reader| {
            assert!(reader.state.borrow().stream.is_empty());
            assert_eq!(reader.state.borrow().stream.len(), 0);
            assert_eq!(reader.state.borrow().retained_bytes, 0);
            assert!(reader.state.borrow().terminally_failed);
            let err = reader.read_event().expect("some").expect_err("refused");
            assert_eq!(
                err.condition(),
                &FailureCondition::ValueConstraintViolated {
                    constraint: crate::encoding::ValueConstraint::TooLong,
                }
            );
            assert_eq!(
                err.diagnostic_detail().message(),
                "stream item limit exceeded"
            );
            assert!(reader.state.borrow().stream.is_empty());
            assert_eq!(reader.state.borrow().stream.len(), 0);
            assert_eq!(reader.state.borrow().retained_bytes, 0);
            assert!(reader.read_event().is_none());
        });

        let env1 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![1, 2, 3],
        };
        let env2 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [2u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [9u8; 32],
            },
            payload: vec![4, 5, 6],
        };
        let env3 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [3u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![7, 8, 9],
        };
        ArtefactReader::with_stream("terminal-error-cleanup", vec![env1, env2, env3], |reader| {
            assert_eq!(reader.state.borrow().stream.len(), 3);
            assert!(reader.state.borrow().retained_bytes > 0);

            let obs = reader.read_event().expect("some").expect("ok genesis");
            assert_eq!(obs.position(), 0);
            assert_eq!(reader.state.borrow().stream.len(), 2);
            assert!(reader.state.borrow().retained_bytes > 0);

            let err = reader
                .read_event()
                .expect("some")
                .expect_err("terminal error");
            assert_eq!(
                err.condition(),
                &FailureCondition::PrecursorChainBroken(None)
            );

            assert_eq!(reader.state.borrow().stream.len(), 0);
            assert_eq!(reader.state.borrow().retained_bytes, 0);
            assert!(reader.state.borrow().terminally_failed);

            assert!(reader.read_event().is_none());
        });
    }

    #[test]
    fn test_m1_multi_envelope_aggregate_capacity_tracking_and_release() {
        let mut p1 = Vec::with_capacity(500);
        p1.extend_from_slice(&[1, 2, 3]);
        let env1 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [1u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: p1,
        };
        let c1 = env1.commitment();

        let mut p2 = Vec::with_capacity(1000);
        p2.extend_from_slice(&[4, 5, 6, 7]);
        let env2 = crate::encoding::EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [2u8; 16],
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [1u8; 16],
                precursor_hash: c1,
            },
            payload: p2,
        };

        ArtefactReader::with_stream("cap-release-stream", vec![env1, env2], |reader| {
            let initial_retained = reader.state.borrow().retained_bytes;
            assert_eq!(initial_retained, (85 + 500) + (85 + 1000));

            let _ = reader.read_event().expect("first").unwrap();
            let after_first = reader.state.borrow().retained_bytes;
            assert_eq!(after_first, 85 + 1000);

            let _ = reader.read_event().expect("second").unwrap();
            let after_second = reader.state.borrow().retained_bytes;
            assert_eq!(after_second, 0);
        });
    }
}
