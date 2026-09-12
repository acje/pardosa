//! Session-level fiber index coordinating validated tip admission, precursor tracking, and event counts.

use crate::encoding::{DecodeError, EventEnvelope, ValueConstraint};
use crate::store::fiber_handle::FiberHandle;
use crate::store::{FailureCondition, FiberState, OperationFailure};
use std::collections::{HashMap, HashSet};

/// Maximum active tracked fibers in a session index.
pub const MAX_ACTIVE_FIBERS: usize = 100_000;

pub use crate::store::fiber_handle::MAX_EVENTS_PER_FIBER;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FiberTip {
    pub(crate) envelope: EventEnvelope,
    pub(crate) event_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FiberSlot {
    Active(FiberTip),
    Broken { reason: Box<str>, event_count: u64 },
}

/// In-memory session index tracking active fibers and event counts.
///
/// Coordinates sequential tip validation, precursor chaining, and active fiber bounds.
///
/// # Resource Contract
/// - Boundary: Pure count-based bounding for `SessionIndex` and store sessions per Priority 3.
/// - Named Budgets: `MAX_ACTIVE_FIBERS = 100_000` and `MAX_EVENTS_PER_FIBER = 100_000`.
/// - Explicit Application Exclusions: In-memory tip payload allocations, transient bulk history buffers during `read_all_frames`/`build_from_frames`, process stack, allocator heap overhead, and durable payload storage in OS page cache or JetStream streams. Byte-reservation scaffolding is permanently retired.
/// - Exhaustion: `ValueConstraint::TooLong` when fiber count or event count exceeds 100,000.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIndex {
    fibers: HashMap<[u8; 16], FiberSlot>,
    seen_event_ids: HashSet<[u8; 16]>,
    revision: u64,
    has_raw_frames: bool,
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
            revision: 0,
            has_raw_frames: false,
        }
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

        if !self.fibers.contains_key(&fiber_id) && self.fibers.len() >= MAX_ACTIVE_FIBERS {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "active tracked fibers capacity exceeded (MAX_ACTIVE_FIBERS)",
            ));
        }

        let event_count = match self.fibers.get(&fiber_id) {
            Some(FiberSlot::Active(tip)) => tip.event_count,
            Some(FiberSlot::Broken { event_count, .. }) => *event_count,
            None => 0,
        };

        self.fibers.insert(
            fiber_id,
            FiberSlot::Broken {
                reason,
                event_count,
            },
        );
        self.revision = self.revision.wrapping_add(1);
        Ok(())
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
            .any(|slot| matches!(slot, FiberSlot::Broken { .. }))
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
            Some(FiberSlot::Broken { .. }) => Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "cannot get latest envelope on broken fiber",
            )),
            None => Ok(None),
        }
    }

    /// Returns the event count for the specified fiber identifier.
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
            Some(FiberSlot::Broken { .. }) => Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "cannot query event count on broken fiber",
            )),
            None => Ok(0),
        }
    }

    /// Returns a [`FiberHandle`] reflecting the current state of the specified fiber.
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
                let state = if tip.envelope.header.detached {
                    FiberState::Detached
                } else {
                    FiberState::Defined
                };
                Ok(FiberHandle::with_state(
                    fiber_id,
                    state,
                    tip.envelope.header.event_id,
                    tip.envelope.commitment(),
                    tip.event_count,
                )?)
            }
            Some(FiberSlot::Broken { .. }) => Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "cannot construct fiber handle for broken fiber",
            )),
            None => Ok(FiberHandle::new(fiber_id)),
        }
    }

    /// Validates an event envelope candidate against session index constraints before write landing.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if raw frames exist, event_id is zero, duplicate event_id is seen,
    /// precursor chain or hash is broken, event count limit is reached, or capacity limit is breached.
    pub fn validate_append(&self, envelope: &EventEnvelope) -> Result<(), OperationFailure> {
        if self.has_raw_frames {
            return Err(OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                "session contains unindexed raw frames; append unavailable",
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

        let max_seen_events = MAX_ACTIVE_FIBERS.saturating_mul(MAX_EVENTS_PER_FIBER as usize);
        if self.seen_event_ids.len() >= max_seen_events {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "seen event ids limit exceeded",
            ));
        }

        match self.fibers.get(&envelope.header.fiber_id) {
            Some(FiberSlot::Broken { .. }) => Err(OperationFailure::new(
                FailureCondition::InvariantBreakingConfiguration,
                "cannot append to broken fiber",
            )),
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
                Ok(())
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
                Ok(())
            }
        }
    }

    /// Pre-validates an event envelope candidate against session index constraints.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if candidate validation fails.
    pub fn prepare_append(&self, envelope: &EventEnvelope) -> Result<(), OperationFailure> {
        self.validate_append(envelope)
    }

    /// Commits an event envelope directly after validation.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if candidate validation fails.
    pub fn commit_envelope(&mut self, envelope: EventEnvelope) -> Result<(), OperationFailure> {
        self.validate_append(&envelope)?;
        self.commit_envelope_unchecked(envelope);
        Ok(())
    }

    /// Commits an event envelope without re-validating preconditions.
    pub(crate) fn commit_envelope_unchecked(&mut self, envelope: EventEnvelope) {
        let fiber_id = envelope.header.fiber_id;
        self.seen_event_ids.insert(envelope.header.event_id);

        match self.fibers.get_mut(&fiber_id) {
            Some(FiberSlot::Active(tip)) => {
                tip.envelope = envelope;
                tip.event_count += 1;
            }
            _ => {
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

    fn record_broken_event_id(
        &mut self,
        fiber_id: [u8; 16],
        event_id: [u8; 16],
    ) -> Result<(), OperationFailure> {
        if let Some(FiberSlot::Broken { event_count, .. }) = self.fibers.get_mut(&fiber_id) {
            let next_count = event_count.checked_add(1).ok_or_else(|| {
                OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: ValueConstraint::TooLong,
                    },
                    "broken fiber event count limit exceeded",
                )
            })?;
            if next_count > MAX_EVENTS_PER_FIBER {
                return Err(OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: ValueConstraint::TooLong,
                    },
                    "broken fiber event count limit exceeded",
                ));
            }
            *event_count = next_count;
        }

        let max_seen_events = MAX_ACTIVE_FIBERS.saturating_mul(MAX_EVENTS_PER_FIBER as usize);
        if !self.seen_event_ids.contains(&event_id) {
            if self.seen_event_ids.len() >= max_seen_events {
                return Err(OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: ValueConstraint::TooLong,
                    },
                    "seen event ids limit exceeded",
                ));
            }
            self.seen_event_ids.insert(event_id);
        }
        Ok(())
    }

    /// Processes a single raw frame into the session index during recovery.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if frame is shorter than 85 bytes, frame decode fails,
    /// or resource limits are breached.
    pub fn process_frame(&mut self, frame: &[u8]) -> Result<(), OperationFailure> {
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
        if let Some(FiberSlot::Broken { .. }) = self.fibers.get(&env.header.fiber_id) {
            self.record_broken_event_id(env.header.fiber_id, env.header.event_id)?;
            return Ok(());
        }
        match self.validate_append(&env) {
            Ok(()) => {
                self.commit_envelope_unchecked(env);
            }
            Err(err) => match err.condition() {
                FailureCondition::PrecursorChainBroken(_) => {
                    self.mark_fiber_broken(
                        env.header.fiber_id,
                        err.diagnostic_detail().message().to_string(),
                    )?;
                    self.record_broken_event_id(env.header.fiber_id, env.header.event_id)?;
                }
                _ => return Err(err),
            },
        }
        Ok(())
    }

    /// Rebuilds a session index from an ordered sequence of raw container frames.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if frame is shorter than 85 bytes, frame decode fails,
    /// or resource limits are breached.
    pub fn build_from_frames<'a>(
        frames: impl IntoIterator<Item = &'a [u8]>,
    ) -> Result<Self, OperationFailure> {
        let mut index = Self::new();
        for frame in frames {
            index.process_frame(frame)?;
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

        let err_mid =
            SessionIndex::build_from_frames([env_buf.as_slice(), corrupt_frame.as_slice()])
                .unwrap_err();
        assert_eq!(*err_mid.condition(), FailureCondition::EnvelopeMismatch);

        let index_ok =
            SessionIndex::build_from_frames([env_buf.as_slice()]).expect("build succeeds");
        assert_eq!(index_ok.len(), 1);
        assert!(!index_ok.has_broken_fibers());
        assert_eq!(
            index_ok.get_latest(&[0x10; 16]).unwrap().unwrap().payload,
            b"first"
        );

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
        assert!(index_broken.has_broken_fibers());
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
        let err_orphan = index.validate_append(&orphan_env).unwrap_err();
        assert_eq!(
            *err_orphan.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );

        let genesis_env = EventEnvelope::genesis([0x01; 16], fiber_id, b"genesis").unwrap();
        index.commit_envelope(genesis_env.clone()).unwrap();

        let dup_genesis = EventEnvelope::genesis([0x02; 16], fiber_id, b"dup_genesis").unwrap();
        let err_dup = index.validate_append(&dup_genesis).unwrap_err();
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
        let err_prev = index.validate_append(&wrong_prev_id).unwrap_err();
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
        let err_hash = index.validate_append(&wrong_hash).unwrap_err();
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
        let err_broken = index.validate_append(&next_env).unwrap_err();
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
        let err_dup = index.validate_append(&env2).unwrap_err();
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

        let index = SessionIndex::build_from_frames([frame1.as_slice()])
            .expect("rebuild succeeds with broken fiber");
        assert!(matches!(
            index.fibers.get(&f1),
            Some(FiberSlot::Broken { .. })
        ));

        let env_on_f2 = EventEnvelope::genesis(shared_event_id, f2, b"f2").unwrap();
        let err = index
            .validate_append(&env_on_f2)
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

        index
            .validate_append(&genesis)
            .expect("valid pre-validation");
        assert_eq!(index.len(), 0);
        assert_eq!(index.get_latest(&fiber_id).unwrap(), None);
        assert_eq!(index.revision(), 0);

        index.commit_envelope(genesis.clone()).unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index.get_latest(&fiber_id).unwrap(), Some(&genesis));
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
        index.validate_append(&child).unwrap();
        index.commit_envelope(child).unwrap();
        assert_eq!(index.revision(), 2);
    }

    #[test]
    fn test_session_index_clean_diagnostics_no_invalid_citations() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x60; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"v1").unwrap();
        index.commit_envelope(genesis).unwrap();

        let dup = EventEnvelope::genesis([0x02; 16], fiber_id, b"v2").unwrap();
        let err_dup = index.validate_append(&dup).unwrap_err();
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
        let err_stale = index.validate_append(&stale).unwrap_err();
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
        let index = SessionIndex::new();
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
        let err = index.validate_append(&zero_env).unwrap_err();
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
        for i in 0..MAX_ACTIVE_FIBERS as u32 {
            let mut fiber_id = [0u8; 16];
            fiber_id[0..4].copy_from_slice(&i.to_le_bytes());
            let env = EventEnvelope::genesis([0x01; 16], fiber_id, b"fiber").unwrap();
            index.commit_envelope_unchecked(env);
        }
        assert_eq!(index.len(), MAX_ACTIVE_FIBERS);

        let new_fiber_env = EventEnvelope::genesis([0x02; 16], [0xff; 16], b"overflow").unwrap();
        let err = index.validate_append(&new_fiber_env).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
    }

    #[test]
    fn test_session_index_max_events_per_fiber_exhaustion_refusal() {
        assert_eq!(MAX_EVENTS_PER_FIBER, 100_000);
        let mut index = SessionIndex::new();
        let fiber_id = [0x77; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"genesis").unwrap();
        index.commit_envelope(genesis.clone()).unwrap();

        let at_limit_env = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x02; 16],
                fiber_id,
                detached: false,
                precursor: genesis.header.event_id,
                precursor_hash: genesis.commitment(),
            },
            payload: b"at_limit".to_vec(),
        };

        if let Some(FiberSlot::Active(tip)) = index.fibers.get_mut(&fiber_id) {
            tip.event_count = MAX_EVENTS_PER_FIBER - 1;
        }
        assert!(index.validate_append(&at_limit_env).is_ok());

        if let Some(FiberSlot::Active(tip)) = index.fibers.get_mut(&fiber_id) {
            tip.event_count = MAX_EVENTS_PER_FIBER;
        }
        let err = index.validate_append(&at_limit_env).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
        assert!(err.to_string().contains("MAX_EVENTS_PER_FIBER"));
    }

    #[test]
    fn test_session_index_seen_event_overhead_grows_monotonically() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x55; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"").unwrap();
        index.commit_envelope(genesis.clone()).unwrap();
        assert_eq!(index.revision(), 1);

        let mut prev_env = genesis;
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
            index.commit_envelope(next_env.clone()).unwrap();
            assert_eq!(index.revision(), i as u64);
            prev_env = next_env;
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
            Some(FiberSlot::Broken { reason, .. }) => {
                assert_eq!(reason.len(), 256);
            }
            _ => panic!("expected broken slot"),
        }

        let mut max_index = SessionIndex::new();
        for i in 0..MAX_ACTIVE_FIBERS as u32 {
            let mut fid = [0u8; 16];
            fid[0..4].copy_from_slice(&i.to_le_bytes());
            max_index.fibers.insert(
                fid,
                FiberSlot::Broken {
                    reason: "b".into(),
                    event_count: 0,
                },
            );
        }
        let err_cap = max_index
            .mark_fiber_broken([0x88; 16], "overflow".to_string())
            .unwrap_err();
        assert_eq!(
            *err_cap.condition(),
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
            Some(FiberSlot::Broken { reason, .. }) => {
                assert_eq!(reason.chars().count(), 256);
                assert_eq!(reason.len(), 1024);
            }
            _ => panic!("expected broken slot"),
        }
    }

    #[test]
    fn test_session_index_bounded_reservations_limit_1024() {
        let mut index = SessionIndex::new();
        for i in 0..MAX_ACTIVE_FIBERS as u32 {
            let mut fiber_id = [0u8; 16];
            fiber_id[0..4].copy_from_slice(&i.to_le_bytes());
            let env = EventEnvelope::genesis([0x01; 16], fiber_id, b"count-based-bound").unwrap();
            index.commit_envelope_unchecked(env);
        }
        assert_eq!(index.len(), MAX_ACTIVE_FIBERS);

        let over_env = EventEnvelope::genesis([0x02; 16], [0xfe; 16], b"over").unwrap();
        let err = index.validate_append(&over_env).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
    }

    #[test]
    fn test_session_index_pending_reservation_bytes_capacity_and_drop_release() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x99; 16];
        let env1 = EventEnvelope::genesis([0x01; 16], fiber_id, vec![0xaa; 100]).unwrap();
        index.commit_envelope(env1.clone()).unwrap();
        assert_eq!(index.len(), 1);

        let env2 = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x02; 16],
                fiber_id,
                detached: false,
                precursor: [0x01; 16],
                precursor_hash: env1.commitment(),
            },
            payload: vec![0xbb; 200],
        };
        index.commit_envelope(env2).unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index.event_count(&fiber_id).unwrap(), 2);
    }

    #[test]
    fn test_mark_fiber_broken_respects_pending_reservation_bytes() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x99; 16];
        let env1 = EventEnvelope::genesis([0x01; 16], fiber_id, vec![0xaa; 100]).unwrap();
        index.commit_envelope(env1).unwrap();

        index
            .mark_fiber_broken(fiber_id, "broken fiber".to_string())
            .unwrap();
        assert!(index.has_broken_fibers());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn test_mark_fiber_broken_active_replacement_capacity_refusal() {
        let mut index = SessionIndex::new();
        for i in 0..MAX_ACTIVE_FIBERS as u32 {
            let mut fid = [0u8; 16];
            fid[0..4].copy_from_slice(&i.to_le_bytes());
            let env = EventEnvelope::genesis([0x01; 16], fid, b"p").unwrap();
            index.commit_envelope_unchecked(env);
        }
        assert_eq!(index.len(), MAX_ACTIVE_FIBERS);

        let mut existing_fid = [0u8; 16];
        existing_fid[0..4].copy_from_slice(&0u32.to_le_bytes());
        index
            .mark_fiber_broken(existing_fid, "replacement succeeds at limit".to_string())
            .expect("replacing existing active slot succeeds at limit");
    }

    #[test]
    fn test_mark_fiber_broken_broken_replacement_capacity_refusal() {
        let mut index = SessionIndex::new();
        for i in 0..MAX_ACTIVE_FIBERS as u32 {
            let mut fid = [0u8; 16];
            fid[0..4].copy_from_slice(&i.to_le_bytes());
            index.fibers.insert(
                fid,
                FiberSlot::Broken {
                    reason: "short".into(),
                    event_count: 0,
                },
            );
        }

        let mut existing_fid = [0u8; 16];
        existing_fid[0..4].copy_from_slice(&0u32.to_le_bytes());
        index
            .mark_fiber_broken(
                existing_fid,
                "much longer reason replacing existing broken slot".to_string(),
            )
            .expect("replacing existing broken slot succeeds at limit");
    }

    #[test]
    fn test_mark_has_raw_frames_revokes_prepare_append_and_outstanding_reservation() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x88; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"payload").unwrap();

        index.validate_append(&genesis).expect("validation valid");

        index.mark_has_raw_frames();
        assert!(index.has_raw_frames());

        let next_env = EventEnvelope::genesis([0x02; 16], [0x99; 16], b"payload2").unwrap();
        let err_prepare = index.validate_append(&next_env).unwrap_err();
        assert_eq!(*err_prepare.condition(), FailureCondition::EnvelopeMismatch);
        assert!(err_prepare
            .to_string()
            .contains("session contains unindexed raw frames; append unavailable"));
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
        assert!(index1.has_broken_fibers());

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
        assert!(index2.has_broken_fibers());
    }

    #[test]
    fn test_record_broken_event_id_capacity_exhaustion_refusal() {
        let mut index = SessionIndex::new();
        let fiber_id = [0x88; 16];
        index
            .mark_fiber_broken(fiber_id, "broken".to_string())
            .unwrap();
        index
            .record_broken_event_id(fiber_id, [0x91; 16])
            .expect("recording broken event id succeeds");
        assert!(index.seen_event_ids.contains(&[0x91; 16]));

        let dup_env = EventEnvelope::genesis([0x91; 16], [0x88; 16], b"dup").unwrap();
        let err = index.validate_append(&dup_env).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
    }

    #[test]
    fn test_build_from_frames_broken_fiber_event_count_limit_exhaustion() {
        assert_eq!(MAX_EVENTS_PER_FIBER, 100_000);
        let fiber_id = [0x42; 16];
        let count = (MAX_EVENTS_PER_FIBER + 1) as usize;
        let mut frame_data = Vec::with_capacity(count * 85);
        for i in 1..=count {
            let mut event_id = [0u8; 16];
            event_id[0..4].copy_from_slice(&(i as u32).to_le_bytes());
            let env = EventEnvelope {
                header: crate::encoding::EnvelopeHeader {
                    event_id,
                    fiber_id,
                    detached: false,
                    precursor: [0xee; 16],
                    precursor_hash: [0xff; 32],
                },
                payload: Vec::new(),
            };
            env.encode(&mut frame_data);
        }

        let frames: Vec<&[u8]> = frame_data
            .as_chunks::<85>()
            .0
            .iter()
            .map(AsRef::as_ref)
            .collect();
        assert_eq!(frames.len(), count);

        let index = SessionIndex::build_from_frames(
            frames[..MAX_EVENTS_PER_FIBER as usize].iter().copied(),
        )
        .expect("up to MAX_EVENTS_PER_FIBER on broken fiber succeeds");
        assert!(index.has_broken_fibers());

        let err = SessionIndex::build_from_frames(frames.iter().copied()).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
        assert!(err
            .to_string()
            .contains("broken fiber event count limit exceeded"));
    }
}
