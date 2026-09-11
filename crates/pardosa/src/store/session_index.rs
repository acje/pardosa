//! Session-level fiber index coordinating validated tip admission, precursor tracking, and event counts.

use crate::encoding::{DecodeError, EventEnvelope, ValueConstraint};
use crate::store::fiber_handle::{FiberHandle, MAX_EVENTS_PER_FIBER};
use crate::store::{FailureCondition, FiberState, OperationFailure};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

static NEXT_INDEX_ID: AtomicU64 = AtomicU64::new(1);

/// Maximum active tracked fibers in a session index.
pub const MAX_ACTIVE_FIBERS: usize = 100_000;

/// Maximum active append reservations in a session index.
pub const MAX_ACTIVE_RESERVATIONS: usize = 1024;

/// Maximum retained index bytes in a session index (64 MiB).
pub const MAX_INDEX_BYTES: usize = 64 * 1024 * 1024;

const FIBER_ENTRY_OVERHEAD: usize = 128;
const SEEN_EVENT_OVERHEAD: usize = 32;
const BROKEN_MARKER_OVERHEAD: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FiberTip {
    pub(crate) envelope: EventEnvelope,
    pub(crate) event_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FiberSlot {
    Active(FiberTip),
    Broken(Box<str>),
}

/// Non-cloneable reservation certifying that an event envelope has been validated against a session index.
#[derive(Debug)]
pub struct AppendReservation {
    pub(crate) index_id: u64,
    pub(crate) expected_revision: u64,
    pub(crate) fiber_id: [u8; 16],
    pub(crate) event_id: [u8; 16],
    pub(crate) envelope: Option<EventEnvelope>,
    pub(crate) active_reservations: Arc<AtomicUsize>,
    pub(crate) pending_reservation_bytes: Arc<AtomicUsize>,
    pub(crate) entry_bytes: usize,
    pub(crate) committed: bool,
}

impl Drop for AppendReservation {
    fn drop(&mut self) {
        if !self.committed {
            self.active_reservations.fetch_sub(1, Ordering::Relaxed);
            self.pending_reservation_bytes
                .fetch_sub(self.entry_bytes, Ordering::Relaxed);
        }
    }
}

impl PartialEq for AppendReservation {
    fn eq(&self, other: &Self) -> bool {
        self.index_id == other.index_id
            && self.expected_revision == other.expected_revision
            && self.fiber_id == other.fiber_id
            && self.event_id == other.event_id
            && self.envelope == other.envelope
            && self.entry_bytes == other.entry_bytes
    }
}

impl Eq for AppendReservation {}

impl AppendReservation {
    /// Returns a reference to the reserved event envelope.
    #[must_use]
    pub fn envelope(&self) -> &EventEnvelope {
        self.envelope
            .as_ref()
            .expect("reservation envelope present")
    }

    /// Returns the fiber identifier targeted by the reservation.
    #[must_use]
    pub fn fiber_id(&self) -> [u8; 16] {
        self.fiber_id
    }

    /// Returns the event identifier targeted by the reservation.
    #[must_use]
    pub fn event_id(&self) -> [u8; 16] {
        self.event_id
    }

    /// Returns the expected index revision for this reservation.
    #[must_use]
    pub fn expected_revision(&self) -> u64 {
        self.expected_revision
    }
}

/// In-memory session index tracking active fibers and event counts.
///
/// # Resource Contract
///
/// - **Boundary**: Per-instance logical accounting ceiling for a single [`SessionIndex`] managing active fiber tips and seen event IDs. Each [`SessionIndex`] clone constitutes an independent instance with its own budget; there is no aggregate cross-session bound.
/// - **Workload**: Append-only event streams and point-lookups up to 100,000 active fibers.
/// - **Named Budgets**:
///   - `MAX_ACTIVE_FIBERS = 100_000` fibers (all slots including broken).
///   - `MAX_INDEX_BYTES = 64 * 1024 * 1024` (64 MiB) is the per-instance logical accounting ceiling for retained and pending logical charges of a single `SessionIndex` (128B entry overhead, 85B envelope overhead, 32B event ID overhead, 64B broken marker + reason string capped at 256 Unicode scalars (at most 1024 UTF-8 bytes), plus payload bytes).
///   - `MAX_ACTIVE_RESERVATIONS = 1024` concurrent in-flight uncommitted append reservations.
/// - **Accounting & Ownership**: RAII drop deduction, zero-copy payload moves on commit.
/// - **Exclusions**: Explicit exclusions from the 64 MiB logical index budget include: separate writer session `seen_events` sets, spare collection capacities in HashMaps/HashSets, transient frame acquisition/serialization buffers (such as entire-file buffers in `read_container_frames`, frame vectors in `read_all_frames`, and transient envelope decode/encode allocations), OS kernel file buffers and page cache, TCP/JetStream socket buffers, allocator heap metadata, process stack, and independent [`SessionIndex`] clones.
/// - **Exhaustion Policy**: Returns `Err(OperationFailure::new(FailureCondition::ValueConstraintViolated { constraint: ValueConstraint::TooLong }, ...))`.
#[derive(Debug)]
pub struct SessionIndex {
    fibers: HashMap<[u8; 16], FiberSlot>,
    seen_event_ids: HashSet<[u8; 16]>,
    retained_bytes: usize,
    index_id: u64,
    revision: u64,
    active_reservations: Arc<AtomicUsize>,
    pending_reservation_bytes: Arc<AtomicUsize>,
    has_raw_frames: bool,
}

impl PartialEq for SessionIndex {
    fn eq(&self, other: &Self) -> bool {
        self.fibers == other.fibers
            && self.seen_event_ids == other.seen_event_ids
            && self.retained_bytes == other.retained_bytes
            && self.index_id == other.index_id
            && self.revision == other.revision
            && self.active_reservations.load(Ordering::Relaxed)
                == other.active_reservations.load(Ordering::Relaxed)
            && self.pending_reservation_bytes.load(Ordering::Relaxed)
                == other.pending_reservation_bytes.load(Ordering::Relaxed)
            && self.has_raw_frames == other.has_raw_frames
    }
}

impl Eq for SessionIndex {}

impl Clone for SessionIndex {
    fn clone(&self) -> Self {
        Self {
            fibers: self.fibers.clone(),
            seen_event_ids: self.seen_event_ids.clone(),
            retained_bytes: self.retained_bytes,
            index_id: NEXT_INDEX_ID.fetch_add(1, Ordering::Relaxed),
            revision: self.revision,
            active_reservations: Arc::new(AtomicUsize::new(0)),
            pending_reservation_bytes: Arc::new(AtomicUsize::new(0)),
            has_raw_frames: self.has_raw_frames,
        }
    }
}

impl Default for SessionIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionIndex {
    /// Creates an empty session index.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fibers: HashMap::new(),
            seen_event_ids: HashSet::new(),
            retained_bytes: 0,
            index_id: NEXT_INDEX_ID.fetch_add(1, Ordering::Relaxed),
            revision: 0,
            active_reservations: Arc::new(AtomicUsize::new(0)),
            pending_reservation_bytes: Arc::new(AtomicUsize::new(0)),
            has_raw_frames: false,
        }
    }

    /// Returns the number of currently active append reservations.
    #[must_use]
    pub fn active_reservations(&self) -> usize {
        self.active_reservations.load(Ordering::Relaxed)
    }

    /// Returns the number of bytes currently reserved across active reservations.
    #[must_use]
    pub fn pending_reservation_bytes(&self) -> usize {
        self.pending_reservation_bytes.load(Ordering::Relaxed)
    }

    /// Marks that the index has unindexed raw frames.
    pub fn mark_has_raw_frames(&mut self) {
        self.has_raw_frames = true;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Returns `true` if the session index has recorded raw frames.
    #[must_use]
    pub fn has_raw_frames(&self) -> bool {
        self.has_raw_frames
    }

    /// Marks a fiber identifier as broken, recording the break in index state.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if index capacity limits are exceeded.
    pub fn mark_fiber_broken(
        &mut self,
        fiber_id: [u8; 16],
        reason: String,
    ) -> Result<(), OperationFailure> {
        let reason: Box<str> = reason
            .chars()
            .take(256)
            .collect::<String>()
            .into_boxed_str();

        let broken_bytes = FIBER_ENTRY_OVERHEAD + BROKEN_MARKER_OVERHEAD + reason.len();
        let pending = self.pending_reservation_bytes.load(Ordering::Relaxed);

        if let Some(slot) = self.fibers.get_mut(&fiber_id) {
            match slot {
                FiberSlot::Active(tip) => {
                    let old_bytes = FIBER_ENTRY_OVERHEAD + 85 + tip.envelope.payload.len();
                    let projected_bytes = self
                        .retained_bytes
                        .saturating_sub(old_bytes)
                        .checked_add(broken_bytes)
                        .ok_or_else(|| {
                            OperationFailure::new(
                                FailureCondition::ValueConstraintViolated {
                                    constraint: ValueConstraint::TooLong,
                                },
                                "retained bytes calculation overflow",
                            )
                        })?;
                    let total_bytes = projected_bytes.checked_add(pending).ok_or_else(|| {
                        OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::TooLong,
                            },
                            "retained bytes calculation overflow",
                        )
                    })?;
                    if total_bytes > MAX_INDEX_BYTES {
                        return Err(OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::TooLong,
                            },
                            "session index memory capacity exceeded (MAX_INDEX_BYTES)",
                        ));
                    }
                    self.retained_bytes = projected_bytes;
                    *slot = FiberSlot::Broken(reason);
                    self.revision = self.revision.wrapping_add(1);
                    Ok(())
                }
                FiberSlot::Broken(old_reason) => {
                    let old_bytes =
                        FIBER_ENTRY_OVERHEAD + BROKEN_MARKER_OVERHEAD + old_reason.len();
                    let projected_bytes = self
                        .retained_bytes
                        .saturating_sub(old_bytes)
                        .checked_add(broken_bytes)
                        .ok_or_else(|| {
                            OperationFailure::new(
                                FailureCondition::ValueConstraintViolated {
                                    constraint: ValueConstraint::TooLong,
                                },
                                "retained bytes calculation overflow",
                            )
                        })?;
                    let total_bytes = projected_bytes.checked_add(pending).ok_or_else(|| {
                        OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::TooLong,
                            },
                            "retained bytes calculation overflow",
                        )
                    })?;
                    if total_bytes > MAX_INDEX_BYTES {
                        return Err(OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::TooLong,
                            },
                            "session index memory capacity exceeded (MAX_INDEX_BYTES)",
                        ));
                    }
                    self.retained_bytes = projected_bytes;
                    *slot = FiberSlot::Broken(reason);
                    self.revision = self.revision.wrapping_add(1);
                    Ok(())
                }
            }
        } else {
            if self.fibers.len() >= MAX_ACTIVE_FIBERS {
                return Err(OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: ValueConstraint::TooLong,
                    },
                    "active tracked fibers capacity exceeded (MAX_ACTIVE_FIBERS)",
                ));
            }
            let projected_bytes =
                self.retained_bytes
                    .checked_add(broken_bytes)
                    .ok_or_else(|| {
                        OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::TooLong,
                            },
                            "retained bytes calculation overflow",
                        )
                    })?;
            let total_bytes = projected_bytes.checked_add(pending).ok_or_else(|| {
                OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: ValueConstraint::TooLong,
                    },
                    "retained bytes calculation overflow",
                )
            })?;
            if total_bytes > MAX_INDEX_BYTES {
                return Err(OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: ValueConstraint::TooLong,
                    },
                    "session index memory capacity exceeded (MAX_INDEX_BYTES)",
                ));
            }
            self.retained_bytes = projected_bytes;
            self.fibers.insert(fiber_id, FiberSlot::Broken(reason));
            self.revision = self.revision.wrapping_add(1);
            Ok(())
        }
    }

    /// Invalidates a fiber identifier by marking it broken.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if index capacity limits are exceeded.
    pub fn invalidate_fiber(&mut self, fiber_id: &[u8; 16]) -> Result<(), OperationFailure> {
        self.mark_fiber_broken(*fiber_id, "invalidated fiber".to_string())
    }

    /// Returns `true` if any fiber in the session index is marked broken.
    #[must_use]
    pub fn has_broken_fibers(&self) -> bool {
        self.fibers
            .values()
            .any(|slot| matches!(slot, FiberSlot::Broken(_)))
    }

    /// Returns the number of distinct active fibers currently tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fibers
            .values()
            .filter(|slot| matches!(slot, FiberSlot::Active(_)))
            .count()
    }

    /// Returns `true` if no active fibers are currently tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the total bytes currently accounted for in the session index.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Returns the current revision of the session index.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns a reference to the latest event envelope for the specified fiber identifier.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::EnvelopeMismatch`] if raw frames exist.
    /// Returns [`OperationFailure`] with [`FailureCondition::PrecursorChainBroken`] if the fiber is broken.
    pub fn get_latest(
        &self,
        fiber_id: &[u8; 16],
    ) -> Result<Option<&EventEnvelope>, OperationFailure> {
        if self.has_raw_frames {
            return Err(OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                "session contains unindexed raw frames; point lookup unavailable",
            ));
        }
        match self.fibers.get(fiber_id) {
            Some(FiberSlot::Active(tip)) => Ok(Some(&tip.envelope)),
            Some(FiberSlot::Broken(_)) => Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "discovered break in precursor chain",
            )),
            None => Ok(None),
        }
    }

    /// Returns the total event count recorded for the specified fiber identifier.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::EnvelopeMismatch`] if raw frames exist.
    /// Returns [`OperationFailure`] with [`FailureCondition::PrecursorChainBroken`] if the fiber is broken.
    pub fn event_count(&self, fiber_id: &[u8; 16]) -> Result<u64, OperationFailure> {
        if self.has_raw_frames {
            return Err(OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                "session contains unindexed raw frames; point lookup unavailable",
            ));
        }
        match self.fibers.get(fiber_id) {
            Some(FiberSlot::Active(tip)) => Ok(tip.event_count),
            Some(FiberSlot::Broken(_)) => Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "discovered break in precursor chain",
            )),
            None => Ok(0),
        }
    }

    /// Returns a [`FiberHandle`] for the specified fiber identifier.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::EnvelopeMismatch`] if raw frames exist.
    /// Returns [`OperationFailure`] with [`FailureCondition::PrecursorChainBroken`] if the fiber is broken.
    pub fn fiber(&self, fiber_id: [u8; 16]) -> Result<FiberHandle, OperationFailure> {
        if self.has_raw_frames {
            return Err(OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                "session contains unindexed raw frames; point lookup unavailable",
            ));
        }
        match self.fibers.get(&fiber_id) {
            Some(FiberSlot::Active(tip)) => {
                let state = match tip.envelope.header.detached {
                    true => FiberState::Detached,
                    false => FiberState::Defined,
                };
                FiberHandle::with_state(
                    fiber_id,
                    state,
                    tip.envelope.header.event_id,
                    tip.envelope.commitment(),
                    tip.event_count,
                )
            }
            Some(FiberSlot::Broken(_)) => Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "discovered break in precursor chain",
            )),
            None => Ok(FiberHandle::new(fiber_id)),
        }
    }

    /// Prepares an append reservation certifying that the envelope is valid against the current index revision.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::EnvelopeMismatch`] if raw frames exist in the session.
    /// Returns [`OperationFailure`] if the envelope violates precursor chain consistency,
    /// precursor hash binding, genesis rules, duplicate event identities, or resource capacity limits.
    pub fn prepare_append(
        &mut self,
        envelope: &EventEnvelope,
    ) -> Result<AppendReservation, OperationFailure> {
        if self.has_raw_frames {
            return Err(OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                "session contains unindexed raw frames; append unavailable",
            ));
        }

        if self.active_reservations.load(Ordering::Relaxed) >= MAX_ACTIVE_RESERVATIONS {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "active reservations limit exceeded (1024)",
            ));
        }

        if envelope.header.event_id == [0u8; 16] {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::Empty,
                },
                "event_id cannot be all-zeroes",
            ));
        }

        if self.seen_event_ids.contains(&envelope.header.event_id) {
            return Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "duplicate event ID observed across session fibers per C5.61",
            ));
        }

        let entry_bytes = FIBER_ENTRY_OVERHEAD + 85 + envelope.payload.len() + SEEN_EVENT_OVERHEAD;
        let pending = self.pending_reservation_bytes.load(Ordering::Relaxed);
        let projected_bytes = self
            .retained_bytes
            .checked_add(pending)
            .and_then(|sum| sum.checked_add(entry_bytes))
            .ok_or_else(|| {
                OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: ValueConstraint::TooLong,
                    },
                    "retained bytes calculation overflow",
                )
            })?;
        if projected_bytes > MAX_INDEX_BYTES {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "session index memory capacity exceeded (MAX_INDEX_BYTES)",
            ));
        }

        match self.fibers.get(&envelope.header.fiber_id) {
            Some(FiberSlot::Broken(_)) => {
                return Err(OperationFailure::new(
                    FailureCondition::InvariantBreakingConfiguration,
                    "cannot append to broken fiber",
                ));
            }
            Some(FiberSlot::Active(tip)) => {
                if envelope.header.precursor == [0u8; 16] {
                    return Err(OperationFailure::new(
                        FailureCondition::PrecursorChainBroken(None),
                        "duplicate genesis on same fiber",
                    ));
                }
                if envelope.header.precursor != tip.envelope.header.event_id {
                    return Err(OperationFailure::new(
                        FailureCondition::PrecursorChainBroken(None),
                        "precursor does not match predecessor event ID per C5.40",
                    ));
                }
                if envelope.header.precursor_hash != tip.envelope.commitment() {
                    return Err(OperationFailure::new(
                        FailureCondition::PrecursorChainBroken(None),
                        "precursor commitment hash does not match predecessor commitment per C5.40",
                    ));
                }
                if tip.event_count >= MAX_EVENTS_PER_FIBER {
                    return Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: ValueConstraint::TooLong,
                        },
                        "event count boundary reached (MAX_EVENTS_PER_FIBER)",
                    ));
                }
            }
            None => {
                if self.fibers.len() >= MAX_ACTIVE_FIBERS {
                    return Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: ValueConstraint::TooLong,
                        },
                        "active tracked fibers capacity exceeded (MAX_ACTIVE_FIBERS)",
                    ));
                }
                if envelope.header.precursor != [0u8; 16]
                    || envelope.header.precursor_hash != [0u8; 32]
                {
                    return Err(OperationFailure::new(
                        FailureCondition::PrecursorChainBroken(None),
                        "initial envelope on fiber must carry genesis precursor zeroes per C4.19 and C5.40",
                    ));
                }
            }
        }

        self.active_reservations.fetch_add(1, Ordering::Relaxed);
        self.pending_reservation_bytes
            .fetch_add(entry_bytes, Ordering::Relaxed);
        Ok(AppendReservation {
            index_id: self.index_id,
            expected_revision: self.revision,
            fiber_id: envelope.header.fiber_id,
            event_id: envelope.header.event_id,
            envelope: Some(envelope.clone()),
            active_reservations: Arc::clone(&self.active_reservations),
            pending_reservation_bytes: Arc::clone(&self.pending_reservation_bytes),
            entry_bytes,
            committed: false,
        })
    }

    /// Commits an append reservation to the session index, verifying index ownership and revision.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// if reservation belongs to a different index or revision has advanced.
    pub fn commit_append(
        &mut self,
        mut reservation: AppendReservation,
    ) -> Result<(), OperationFailure> {
        if self.index_id != reservation.index_id {
            return Err(OperationFailure::new(
                FailureCondition::InvariantBreakingConfiguration,
                "append reservation belongs to a different session index",
            ));
        }
        if self.revision != reservation.expected_revision {
            return Err(OperationFailure::new(
                FailureCondition::InvariantBreakingConfiguration,
                "session index revision mismatch: index was mutated since reservation was prepared",
            ));
        }

        reservation.committed = true;
        self.active_reservations.fetch_sub(1, Ordering::Relaxed);
        self.pending_reservation_bytes
            .fetch_sub(reservation.entry_bytes, Ordering::Relaxed);

        let fiber_id = reservation.fiber_id;
        let envelope = reservation
            .envelope
            .take()
            .expect("reservation envelope present");
        self.seen_event_ids.insert(envelope.header.event_id);

        match self.fibers.get_mut(&fiber_id) {
            Some(FiberSlot::Active(tip)) => {
                let old_bytes = FIBER_ENTRY_OVERHEAD + 85 + tip.envelope.payload.len();
                let new_bytes =
                    FIBER_ENTRY_OVERHEAD + 85 + envelope.payload.len() + SEEN_EVENT_OVERHEAD;
                self.retained_bytes = self.retained_bytes.saturating_sub(old_bytes) + new_bytes;
                tip.envelope = envelope;
                tip.event_count += 1;
            }
            _ => {
                let entry_bytes =
                    FIBER_ENTRY_OVERHEAD + 85 + envelope.payload.len() + SEEN_EVENT_OVERHEAD;
                self.retained_bytes += entry_bytes;
                self.fibers.insert(
                    fiber_id,
                    FiberSlot::Active(FiberTip {
                        envelope,
                        event_count: 1,
                    }),
                );
            }
        }

        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    /// Commits an event envelope directly by preparing and committing an append reservation.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if candidate validation fails.
    pub fn commit_envelope(&mut self, envelope: EventEnvelope) -> Result<(), OperationFailure> {
        let reservation = self.prepare_append(&envelope)?;
        self.commit_append(reservation)
    }

    /// Decodes a container frame payload and verifies full frame consumption.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::EnvelopeMismatch`]
    /// if envelope decoding fails or if trailing unconsumed bytes remain.
    pub fn decode_and_validate_frame(frame: &[u8]) -> Result<EventEnvelope, OperationFailure> {
        let (env, consumed) = EventEnvelope::decode(frame).map_err(|err| {
            OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                format!("frame decode error: {err}"),
            )
        })?;
        if consumed != frame.len() {
            return Err(OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                format!(
                    "frame decode error: {}",
                    DecodeError::TruncatedPayload {
                        expected: frame.len(),
                        available: consumed,
                    }
                ),
            ));
        }
        Ok(env)
    }

    fn record_broken_event_id(&mut self, event_id: [u8; 16]) -> Result<(), OperationFailure> {
        if self.seen_event_ids.contains(&event_id) {
            return Ok(());
        }
        let pending = self.pending_reservation_bytes.load(Ordering::Relaxed);
        let projected = self
            .retained_bytes
            .checked_add(SEEN_EVENT_OVERHEAD)
            .ok_or_else(|| {
                OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: ValueConstraint::TooLong,
                    },
                    "retained bytes calculation overflow",
                )
            })?;
        let total = projected.checked_add(pending).ok_or_else(|| {
            OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "retained bytes calculation overflow",
            )
        })?;
        if total > MAX_INDEX_BYTES {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "session index memory capacity exceeded (MAX_INDEX_BYTES)",
            ));
        }
        self.retained_bytes = projected;
        self.seen_event_ids.insert(event_id);
        Ok(())
    }

    /// Rebuilds a session index from an ordered sequence of raw container frames.
    ///
    /// Requires exact envelope decoding; refuses frames shorter than 85 bytes or malformed envelopes per M2.
    /// Envelopes with broken precursor chains mark the fiber as broken rather than failing rebuild.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if frame is shorter than 85 bytes, frame decode fails,
    /// or resource limits are breached.
    pub fn build_from_frames<'a>(
        frames: impl IntoIterator<Item = &'a [u8]>,
    ) -> Result<Self, OperationFailure> {
        let mut index = Self::new();
        for frame in frames {
            if frame.len() < 85 {
                return Err(OperationFailure::new(
                    FailureCondition::EnvelopeMismatch,
                    "frame length < 85 bytes or malformed envelope in ordinary session index",
                ));
            }
            let env = Self::decode_and_validate_frame(frame).map_err(|_| {
                OperationFailure::new(
                    FailureCondition::EnvelopeMismatch,
                    "frame length < 85 bytes or malformed envelope in ordinary session index",
                )
            })?;
            if let Some(FiberSlot::Broken(_)) = index.fibers.get(&env.header.fiber_id) {
                index.record_broken_event_id(env.header.event_id)?;
                continue;
            }
            match index.prepare_append(&env) {
                Ok(reservation) => {
                    index.commit_append(reservation)?;
                }
                Err(err) => match err.condition() {
                    FailureCondition::PrecursorChainBroken(_) => {
                        index.mark_fiber_broken(
                            env.header.fiber_id,
                            err.diagnostic_detail().message().to_string(),
                        )?;
                        index.record_broken_event_id(env.header.event_id)?;
                    }
                    _ => return Err(err),
                },
            }
        }
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_index_build_from_frames_zero_error_swallowing() {
        let mut env_buf = Vec::new();
        let env1 = EventEnvelope::genesis([0x01; 16], [0x10; 16], b"first").expect("genesis");
        env1.encode(&mut env_buf);

        let corrupt_short = b"short_frame";
        let err_short = SessionIndex::build_from_frames([corrupt_short.as_slice()]).unwrap_err();
        assert_eq!(*err_short.condition(), FailureCondition::EnvelopeMismatch);
        assert!(err_short
            .to_string()
            .contains("frame length < 85 bytes or malformed envelope in ordinary session index"));

        let mut corrupt_frame = Vec::new();
        env1.header.encode(&mut corrupt_frame);
        corrupt_frame.extend_from_slice(&(100u32).to_le_bytes());
        corrupt_frame.extend_from_slice(&[1, 2, 3]);
        let err_corrupt = SessionIndex::build_from_frames([corrupt_frame.as_slice()]).unwrap_err();
        assert_eq!(*err_corrupt.condition(), FailureCondition::EnvelopeMismatch);

        let mut trailing_frame = env_buf.clone();
        trailing_frame.extend_from_slice(b"extra_bytes");
        let err_trailing =
            SessionIndex::build_from_frames([trailing_frame.as_slice()]).unwrap_err();
        assert_eq!(
            *err_trailing.condition(),
            FailureCondition::EnvelopeMismatch
        );

        let err_mixed =
            SessionIndex::build_from_frames([env_buf.as_slice(), corrupt_frame.as_slice()])
                .unwrap_err();
        assert_eq!(*err_mixed.condition(), FailureCondition::EnvelopeMismatch);

        let ok_index =
            SessionIndex::build_from_frames([env_buf.as_slice()]).expect("build succeeds");
        assert_eq!(ok_index.len(), 1);
        assert_eq!(ok_index.event_count(&[0x10; 16]).unwrap(), 1);

        let broken_env = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x02; 16],
                fiber_id: [0x22; 16],
                detached: false,
                precursor: [0x99; 16],
                precursor_hash: [0xaa; 32],
            },
            payload: vec![],
        };
        let mut broken_buf = Vec::new();
        broken_env.encode(&mut broken_buf);
        let index_broken =
            SessionIndex::build_from_frames([env_buf.as_slice(), broken_buf.as_slice()]).unwrap();
        assert_eq!(index_broken.len(), 1);
        assert!(index_broken.get_latest(&[0x22; 16]).is_err());
        assert!(index_broken.fiber([0x22; 16]).is_err());
    }

    #[test]
    fn test_session_index_rejects_orphan_precursor_and_wrong_fiber() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x20; 16];

        let orphan_env = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x01; 16],
                fiber_id,
                detached: false,
                precursor: [0x99; 16],
                precursor_hash: [0xaa; 32],
            },
            payload: vec![],
        };
        let err_orphan = index.prepare_append(&orphan_env).unwrap_err();
        assert_eq!(
            *err_orphan.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );

        let genesis_env = EventEnvelope::genesis([0x01; 16], fiber_id, b"genesis").unwrap();
        let reservation = index.prepare_append(&genesis_env).unwrap();
        index.commit_append(reservation).unwrap();

        let dup_genesis = EventEnvelope::genesis([0x02; 16], fiber_id, b"dup_genesis").unwrap();
        let err_dup = index.prepare_append(&dup_genesis).unwrap_err();
        assert_eq!(
            *err_dup.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );

        let wrong_prev_id = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x03; 16],
                fiber_id,
                detached: false,
                precursor: [0x88; 16],
                precursor_hash: genesis_env.commitment(),
            },
            payload: vec![],
        };
        let err_prev = index.prepare_append(&wrong_prev_id).unwrap_err();
        assert_eq!(
            *err_prev.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );

        let wrong_hash = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x04; 16],
                fiber_id,
                detached: false,
                precursor: [0x01; 16],
                precursor_hash: [0xff; 32],
            },
            payload: vec![],
        };
        let err_hash = index.prepare_append(&wrong_hash).unwrap_err();
        assert_eq!(
            *err_hash.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
    }

    #[test]
    fn test_session_index_rejects_broken_fiber_append() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x30; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"genesis").unwrap();
        index.commit_envelope(genesis.clone()).unwrap();
        assert_eq!(index.len(), 1);

        index.invalidate_fiber(&fiber_id).unwrap();
        assert_eq!(
            *index.get_latest(&fiber_id).unwrap_err().condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
        assert_eq!(
            *index.event_count(&fiber_id).unwrap_err().condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
        assert_eq!(
            *index.fiber(fiber_id).unwrap_err().condition(),
            FailureCondition::PrecursorChainBroken(None)
        );

        let next_env = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x02; 16],
                fiber_id,
                detached: false,
                precursor: [0x01; 16],
                precursor_hash: genesis.commitment(),
            },
            payload: vec![],
        };
        let err_broken = index.prepare_append(&next_env).unwrap_err();
        assert_eq!(
            *err_broken.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );
    }

    #[test]
    fn test_session_index_generation_wide_duplicate_event_id() {
        let mut index = SessionIndex::new();
        let f1 = [0x41; 16];
        let f2 = [0x42; 16];
        let shared_event_id = [0x01; 16];

        let env1 = EventEnvelope::genesis(shared_event_id, f1, b"f1").unwrap();
        index.commit_envelope(env1).unwrap();

        let env2 = EventEnvelope::genesis(shared_event_id, f2, b"f2").unwrap();
        let err_dup = index.prepare_append(&env2).unwrap_err();
        assert_eq!(
            *err_dup.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
    }

    #[test]
    fn test_build_from_frames_broken_envelope_event_id_tracked_for_uniqueness() {
        let f1 = [0x71; 16];
        let f2 = [0x72; 16];
        let shared_event_id = [0x88; 16];

        let broken_env = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: shared_event_id,
                fiber_id: f1,
                detached: false,
                precursor: [0x99; 16],
                precursor_hash: [0xaa; 32],
            },
            payload: b"orphan_on_f1".to_vec(),
        };

        let mut frame1 = Vec::new();
        broken_env.encode(&mut frame1);

        let mut index = SessionIndex::build_from_frames([frame1.as_slice()])
            .expect("rebuild succeeds with broken fiber");
        assert!(matches!(index.fibers.get(&f1), Some(FiberSlot::Broken(_))));

        let env_on_f2 = EventEnvelope::genesis(shared_event_id, f2, b"f2").unwrap();
        let err = index
            .prepare_append(&env_on_f2)
            .expect_err("duplicate event ID from broken fiber must be rejected");
        assert_eq!(
            *err.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
        assert!(err
            .to_string()
            .contains("duplicate event ID observed across session fibers per C5.61"));
    }

    #[test]
    fn test_session_index_two_phase_mutation_contract() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x50; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"payload").unwrap();

        let reservation = index.prepare_append(&genesis).expect("valid reservation");
        assert_eq!(reservation.envelope(), &genesis);
        assert_eq!(reservation.fiber_id(), fiber_id);
        assert_eq!(reservation.event_id(), [0x01; 16]);
        assert_eq!(reservation.expected_revision(), 0);
        assert_eq!(index.len(), 0);
        assert_eq!(index.get_latest(&fiber_id).unwrap(), None);
        assert_eq!(index.retained_bytes(), 0);

        index.commit_append(reservation).unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index.get_latest(&fiber_id).unwrap(), Some(&genesis));
        assert!(index.retained_bytes() > 0);
        assert_eq!(index.revision(), 1);

        let child = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x02; 16],
                fiber_id,
                detached: false,
                precursor: [0x01; 16],
                precursor_hash: genesis.commitment(),
            },
            payload: vec![],
        };
        let res2 = index.prepare_append(&child).unwrap();
        assert_eq!(res2.expected_revision(), 1);

        index.invalidate_fiber(&[0x99; 16]).unwrap();
        assert_eq!(index.revision(), 2);

        let err_stale = index.commit_append(res2).unwrap_err();
        assert_eq!(
            *err_stale.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let mut other_index = SessionIndex::new();
        let foreign_genesis = EventEnvelope::genesis([0x03; 16], [0x77; 16], b"foreign").unwrap();
        let foreign_res = other_index.prepare_append(&foreign_genesis).unwrap();
        let err_foreign = index.commit_append(foreign_res).unwrap_err();
        assert_eq!(
            *err_foreign.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );
    }

    #[test]
    fn test_session_index_clean_diagnostics_no_invalid_citations() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x60; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"v1").unwrap();
        index.commit_envelope(genesis).unwrap();

        let dup = EventEnvelope::genesis([0x02; 16], fiber_id, b"v2").unwrap();
        let err_dup = index.prepare_append(&dup).unwrap_err();
        assert!(
            !err_dup.diagnostic_detail().message().contains("C5.22"),
            "diagnostic must not cite C5.22"
        );

        let stale = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x03; 16],
                fiber_id,
                detached: false,
                precursor: [0x99; 16],
                precursor_hash: [0xaa; 32],
            },
            payload: vec![],
        };
        let err_stale = index.prepare_append(&stale).unwrap_err();
        assert!(
            !err_stale.diagnostic_detail().message().contains("C5.22"),
            "diagnostic must not cite C5.22"
        );
        assert!(
            !err_stale.diagnostic_detail().message().contains("C6.7"),
            "diagnostic must not cite C6.7"
        );
    }

    #[test]
    fn test_session_index_zero_event_id_rejection() {
        let mut index = SessionIndex::new();
        let zero_env = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0u8; 16],
                fiber_id: [0x70; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![],
        };
        let err = index.prepare_append(&zero_env).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::Empty,
            }
        );
    }

    #[test]
    fn test_session_index_resource_bounds_capacity_refusal() {
        let mut index = SessionIndex::new();
        index.retained_bytes = MAX_INDEX_BYTES;

        let env = EventEnvelope::genesis([0x01; 16], [0x80; 16], b"overflow").unwrap();
        let err = index.prepare_append(&env).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
    }

    #[test]
    fn test_session_index_seen_event_overhead_grows_monotonically() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x55; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"").unwrap();
        let r1 = index.prepare_append(&genesis).unwrap();
        index.commit_append(r1).unwrap();
        let bytes_1 = index.retained_bytes();

        let mut prev_env = genesis;
        let mut prev_bytes = bytes_1;
        for i in 2..=10 {
            let next_env = EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id: [i as u8; 16],
                    fiber_id,
                    detached: false,
                    precursor: prev_env.header.event_id,
                    precursor_hash: prev_env.commitment(),
                },
                payload: vec![],
            };
            let r = index.prepare_append(&next_env).unwrap();
            index.commit_append(r).unwrap();
            let current_bytes = index.retained_bytes();
            assert!(
                current_bytes > prev_bytes,
                "event {i}: retained_bytes {current_bytes} must exceed previous {prev_bytes}"
            );
            assert_eq!(current_bytes - prev_bytes, SEEN_EVENT_OVERHEAD);
            prev_env = next_env;
            prev_bytes = current_bytes;
        }
    }

    #[test]
    fn test_mark_fiber_broken_reason_truncation_and_capacity_refusal() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x77; 16];
        let mut huge_capacity_string = String::with_capacity(1024 * 1024);
        huge_capacity_string.push_str(&"a".repeat(500));
        index
            .mark_fiber_broken(fiber_id, huge_capacity_string)
            .unwrap();

        match index.fibers.get(&fiber_id) {
            Some(FiberSlot::Broken(r)) => {
                assert_eq!(r.len(), 256);
            }
            _ => panic!("expected broken slot"),
        }

        let mut max_bytes_index = SessionIndex::new();
        max_bytes_index.retained_bytes = MAX_INDEX_BYTES;
        let err_bytes = max_bytes_index
            .mark_fiber_broken([0x88; 16], "overflow".to_string())
            .unwrap_err();
        assert_eq!(
            *err_bytes.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
    }

    #[test]
    fn test_mark_fiber_broken_unicode_scalar_bound() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x99; 16];
        let emoji_reason = "🦀".repeat(300);
        index.mark_fiber_broken(fiber_id, emoji_reason).unwrap();

        match index.fibers.get(&fiber_id) {
            Some(FiberSlot::Broken(r)) => {
                assert_eq!(r.chars().count(), 256);
                assert_eq!(r.len(), 1024);
            }
            _ => panic!("expected broken slot"),
        }
    }

    #[test]
    fn test_session_index_bounded_reservations_limit_1024() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x42; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"genesis").unwrap();
        index.commit_envelope(genesis.clone()).unwrap();

        let mut reservations = Vec::new();
        for i in 0..1024u32 {
            let mut event_id = [0u8; 16];
            event_id[0..4].copy_from_slice(&(i + 2).to_le_bytes());
            let env = EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id,
                    fiber_id,
                    detached: false,
                    precursor: genesis.header.event_id,
                    precursor_hash: genesis.commitment(),
                },
                payload: vec![],
            };
            let res = index.prepare_append(&env).expect("reservation under limit");
            reservations.push(res);
        }
        assert_eq!(index.active_reservations(), 1024);

        let over_env = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0xff; 16],
                fiber_id,
                detached: false,
                precursor: genesis.header.event_id,
                precursor_hash: genesis.commitment(),
            },
            payload: vec![],
        };
        let err = index.prepare_append(&over_env).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );

        drop(reservations.pop());
        assert_eq!(index.active_reservations(), 1023);

        let res = index
            .prepare_append(&over_env)
            .expect("reservation after drop");
        assert_eq!(index.active_reservations(), 1024);
        drop(res);
        assert_eq!(index.active_reservations(), 1023);
    }

    #[test]
    fn test_session_index_pending_reservation_bytes_capacity_and_drop_release() {
        let mut index = SessionIndex::new();
        assert_eq!(index.pending_reservation_bytes(), 0);
        assert!(!index.has_raw_frames());

        let fiber_id = [0x99; 16];
        let half_cap = 32 * 1024 * 1024;
        let env1 = EventEnvelope::genesis([0x01; 16], fiber_id, vec![0xaa; half_cap]).unwrap();
        let res1 = index.prepare_append(&env1).unwrap();
        let expected_bytes = FIBER_ENTRY_OVERHEAD + 85 + half_cap + SEEN_EVENT_OVERHEAD;
        assert_eq!(index.pending_reservation_bytes(), expected_bytes);

        let env2 =
            EventEnvelope::genesis([0x02; 16], [0xaa; 16], vec![0xbb; half_cap + 1024]).unwrap();
        let err = index.prepare_append(&env2).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );

        drop(res1);
        assert_eq!(index.pending_reservation_bytes(), 0);

        let res2 = index.prepare_append(&env2).unwrap();
        assert_eq!(
            index.pending_reservation_bytes(),
            FIBER_ENTRY_OVERHEAD + 85 + half_cap + 1024 + SEEN_EVENT_OVERHEAD
        );
        index.commit_append(res2).unwrap();
        assert_eq!(index.pending_reservation_bytes(), 0);
        assert!(index.retained_bytes() >= half_cap);
    }

    #[test]
    fn test_mark_fiber_broken_respects_pending_reservation_bytes() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x99; 16];
        let near_cap = MAX_INDEX_BYTES - 100;
        let env1 = EventEnvelope::genesis(
            [0x01; 16],
            fiber_id,
            vec![0xaa; near_cap - (FIBER_ENTRY_OVERHEAD + 85 + SEEN_EVENT_OVERHEAD)],
        )
        .unwrap();
        let res1 = index.prepare_append(&env1).unwrap();
        assert!(index.pending_reservation_bytes() > 0);

        let err = index
            .mark_fiber_broken([0xaa; 16], "exceeds capacity with pending".to_string())
            .unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );

        drop(res1);
        assert_eq!(index.pending_reservation_bytes(), 0);

        index
            .mark_fiber_broken([0xaa; 16], "succeeds after pending dropped".to_string())
            .unwrap();
    }

    #[test]
    fn test_mark_fiber_broken_active_replacement_capacity_refusal() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x55; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"p").unwrap();
        let res = index.prepare_append(&genesis).unwrap();
        index.commit_append(res).unwrap();

        index.retained_bytes = MAX_INDEX_BYTES - 10;
        let err = index
            .mark_fiber_broken(fiber_id, "broken reason pushing over capacity".to_string())
            .unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
    }

    #[test]
    fn test_mark_fiber_broken_broken_replacement_capacity_refusal() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x66; 16];
        index
            .mark_fiber_broken(fiber_id, "short".to_string())
            .unwrap();

        index.retained_bytes = MAX_INDEX_BYTES - 10;
        let err = index
            .mark_fiber_broken(
                fiber_id,
                "much longer reason pushing over capacity".to_string(),
            )
            .unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
    }

    #[test]
    fn test_mark_has_raw_frames_revokes_prepare_append_and_outstanding_reservation() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x88; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"payload").unwrap();

        let reservation = index.prepare_append(&genesis).expect("reservation valid");
        assert_eq!(reservation.expected_revision(), 0);

        index.mark_has_raw_frames();
        assert!(index.has_raw_frames());

        let next_env = EventEnvelope::genesis([0x02; 16], [0x99; 16], b"payload2").unwrap();
        let err_prepare = index.prepare_append(&next_env).unwrap_err();
        assert_eq!(*err_prepare.condition(), FailureCondition::EnvelopeMismatch);
        assert!(err_prepare
            .to_string()
            .contains("session contains unindexed raw frames; append unavailable"));

        let err_commit = index.commit_append(reservation).unwrap_err();
        assert_eq!(
            *err_commit.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );
        assert!(err_commit
            .to_string()
            .contains("session index revision mismatch"));
    }

    #[test]
    fn test_build_from_frames_broken_event_id_capacity_accounting_and_refusal() {
        let f1 = [0x71; 16];
        let broken_env1 = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x11; 16],
                fiber_id: f1,
                detached: false,
                precursor: [0x99; 16],
                precursor_hash: [0xaa; 32],
            },
            payload: b"first_broken".to_vec(),
        };
        let mut frame1 = Vec::new();
        broken_env1.encode(&mut frame1);

        let index1 = SessionIndex::build_from_frames([frame1.as_slice()])
            .expect("build with single broken frame");
        let initial_retained = index1.retained_bytes();
        let expected_reason_len =
            "initial envelope on fiber must carry genesis precursor zeroes per C4.19 and C5.40"
                .len();
        let expected_marker_bytes =
            FIBER_ENTRY_OVERHEAD + BROKEN_MARKER_OVERHEAD + expected_reason_len;
        assert_eq!(
            initial_retained,
            expected_marker_bytes + SEEN_EVENT_OVERHEAD
        );

        let broken_env2 = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x12; 16],
                fiber_id: f1,
                detached: false,
                precursor: [0x98; 16],
                precursor_hash: [0xab; 32],
            },
            payload: b"second_broken".to_vec(),
        };
        let mut frame2 = Vec::new();
        broken_env2.encode(&mut frame2);

        let index2 = SessionIndex::build_from_frames([frame1.as_slice(), frame2.as_slice()])
            .expect("build with two broken frames");
        assert_eq!(
            index2.retained_bytes(),
            initial_retained + SEEN_EVENT_OVERHEAD
        );

        let f_large = [0x72; 16];
        let near_cap = MAX_INDEX_BYTES - initial_retained - (SEEN_EVENT_OVERHEAD / 2);
        let large_payload_len = near_cap - (FIBER_ENTRY_OVERHEAD + 85 + SEEN_EVENT_OVERHEAD);
        let large_env =
            EventEnvelope::genesis([0x20; 16], f_large, vec![0xcc; large_payload_len]).unwrap();
        let mut large_frame = Vec::new();
        large_env.encode(&mut large_frame);

        let err_cap = SessionIndex::build_from_frames([
            large_frame.as_slice(),
            frame1.as_slice(),
            frame2.as_slice(),
        ])
        .unwrap_err();
        assert_eq!(
            *err_cap.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
        assert!(err_cap
            .to_string()
            .contains("session index memory capacity exceeded (MAX_INDEX_BYTES)"));
    }

    #[test]
    fn test_record_broken_event_id_capacity_exhaustion_refusal() {
        let mut index_at_limit = SessionIndex::new();
        index_at_limit.retained_bytes = MAX_INDEX_BYTES - SEEN_EVENT_OVERHEAD;
        index_at_limit
            .record_broken_event_id([0x91; 16])
            .expect("at-limit insertion must succeed");
        assert_eq!(index_at_limit.retained_bytes, MAX_INDEX_BYTES);

        let mut index_over_limit = SessionIndex::new();
        index_over_limit.retained_bytes = MAX_INDEX_BYTES - SEEN_EVENT_OVERHEAD + 1;
        let err = index_over_limit
            .record_broken_event_id([0x92; 16])
            .unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
        assert!(err
            .to_string()
            .contains("session index memory capacity exceeded (MAX_INDEX_BYTES)"));
    }
}
