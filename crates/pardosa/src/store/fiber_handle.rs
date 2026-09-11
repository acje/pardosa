//! First-class fiber handle encapsulating the 5-state lifecycle and precursor chaining.

use crate::encoding::{EnvelopeHeader, EventEnvelope, ValueConstraint};
use crate::store::{CausalChainError, FailureCondition, FiberState, OperationFailure};

/// Maximum number of events allowed per fiber.
pub const MAX_EVENTS_PER_FIBER: u64 = 100_000;

/// Active entity handle encapsulating fiber state, precursor tracking, and event counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiberHandle {
    fiber_id: [u8; 16],
    state: FiberState,
    precursor: [u8; 16],
    precursor_hash: [u8; 32],
    event_count: u64,
}

impl FiberHandle {
    /// Creates a new fiber handle for a previously undefined fiber.
    #[must_use]
    pub fn new(fiber_id: [u8; 16]) -> Self {
        Self {
            fiber_id,
            state: FiberState::Undefined,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
            event_count: 0,
        }
    }

    /// Creates a fiber handle with an explicit lifecycle state, precursor, precursor hash, and event count.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// if the state and tip combinations are contradictory or event count is outside 1..=[`MAX_EVENTS_PER_FIBER`].
    pub fn with_state(
        fiber_id: [u8; 16],
        state: FiberState,
        precursor: [u8; 16],
        precursor_hash: [u8; 32],
        event_count: u64,
    ) -> Result<Self, OperationFailure> {
        match state {
            FiberState::Undefined | FiberState::Purged => {
                if precursor != [0u8; 16] || precursor_hash != [0u8; 32] || event_count != 0 {
                    return Err(OperationFailure::new(
                        FailureCondition::InvariantBreakingConfiguration,
                        "undefined or purged fiber must carry zero precursor, zero precursor hash, and event count 0",
                    ));
                }
            }
            FiberState::Defined | FiberState::Detached | FiberState::Locked => {
                if precursor == [0u8; 16]
                    || precursor_hash == [0u8; 32]
                    || event_count == 0
                    || event_count > MAX_EVENTS_PER_FIBER
                {
                    return Err(OperationFailure::new(
                        FailureCondition::InvariantBreakingConfiguration,
                        "active fiber must carry non-zero precursor, precursor hash, and 1 <= event_count <= MAX_EVENTS_PER_FIBER",
                    ));
                }
            }
        }
        Ok(Self {
            fiber_id,
            state,
            precursor,
            precursor_hash,
            event_count,
        })
    }

    /// Reconstitutes a fiber handle from an ordered sequence of recorded event envelopes.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::PrecursorChainBroken`]
    /// if an envelope has a mismatched fiber identifier, broken precursor link, or duplicate event identifier.
    /// Returns [`OperationFailure`] with [`FailureCondition::ValueConstraintViolated`]
    /// if an envelope has an all-zeroes event identifier or if envelope sequence exceeds [`MAX_EVENTS_PER_FIBER`].
    pub fn from_envelopes(
        fiber_id: [u8; 16],
        envelopes: &[EventEnvelope],
    ) -> Result<Self, OperationFailure> {
        if envelopes.is_empty() {
            return Ok(Self::new(fiber_id));
        }

        if envelopes.len() as u64 > MAX_EVENTS_PER_FIBER {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "envelope sequence length exceeds MAX_EVENTS_PER_FIBER",
            ));
        }

        let mut current_state = FiberState::Undefined;
        let mut prev_event_id = [0u8; 16];
        let mut prev_commitment = [0u8; 32];
        let mut seen_ids = std::collections::HashSet::new();

        for (idx, env) in envelopes.iter().enumerate() {
            if env.header.event_id == [0u8; 16] {
                return Err(OperationFailure::new(
                    FailureCondition::ValueConstraintViolated {
                        constraint: ValueConstraint::Empty,
                    },
                    "envelope event_id cannot be all-zeroes",
                ));
            }
            if !seen_ids.insert(env.header.event_id) {
                return Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    "duplicate event ID observed in envelope sequence per C5.61",
                ));
            }
            if env.header.fiber_id != fiber_id {
                return Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(Some(
                        CausalChainError::PrecursorWrongFiber,
                    )),
                    "envelope fiber_id does not match target fiber",
                ));
            }

            match idx {
                0 => {
                    if env.header.precursor != [0u8; 16] || env.header.precursor_hash != [0u8; 32] {
                        return Err(OperationFailure::new(
                            FailureCondition::PrecursorChainBroken(None),
                            "initial envelope must carry genesis precursor zeroes",
                        ));
                    }
                }
                _ => {
                    if env.header.precursor != prev_event_id
                        || env.header.precursor_hash != prev_commitment
                    {
                        return Err(OperationFailure::new(
                            FailureCondition::PrecursorChainBroken(None),
                            "precursor link does not match predecessor commitment",
                        ));
                    }
                }
            }

            prev_event_id = env.header.event_id;
            prev_commitment = env.commitment();
            current_state = match env.header.detached {
                true => FiberState::Detached,
                false => FiberState::Defined,
            };
        }

        Ok(Self {
            fiber_id,
            state: current_state,
            precursor: prev_event_id,
            precursor_hash: prev_commitment,
            event_count: envelopes.len() as u64,
        })
    }

    /// Returns the fiber identifier.
    #[must_use]
    pub fn fiber_id(&self) -> [u8; 16] {
        self.fiber_id
    }

    /// Returns the current lifecycle state of the fiber.
    #[must_use]
    pub fn state(&self) -> FiberState {
        self.state
    }

    /// Returns the precursor event identifier.
    #[must_use]
    pub fn precursor(&self) -> [u8; 16] {
        self.precursor
    }

    /// Returns the 32-byte BLAKE3 precursor hash commitment.
    #[must_use]
    pub fn precursor_hash(&self) -> [u8; 32] {
        self.precursor_hash
    }

    /// Returns the total number of events recorded for this fiber handle.
    #[must_use]
    pub fn event_count(&self) -> u64 {
        self.event_count
    }

    /// Returns `true` if the fiber is active in [`FiberState::Defined`].
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state == FiberState::Defined
    }

    /// Returns `true` if the fiber is in [`FiberState::Detached`].
    #[must_use]
    pub fn is_detached(&self) -> bool {
        self.state == FiberState::Detached
    }

    /// Returns `true` if the fiber is in [`FiberState::Locked`].
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.state == FiberState::Locked
    }

    /// Returns `true` if the fiber is in [`FiberState::Purged`].
    #[must_use]
    pub fn is_purged(&self) -> bool {
        self.state == FiberState::Purged
    }

    /// Returns `true` if the fiber is in [`FiberState::Undefined`].
    #[must_use]
    pub fn is_undefined(&self) -> bool {
        self.state == FiberState::Undefined
    }

    /// Appends an event to the fiber, automatically minting an [`EventEnvelope`].
    ///
    /// Creates a genesis envelope if the fiber is [`FiberState::Undefined`] or [`FiberState::Purged`].
    /// Chains precursor and hash if the fiber is [`FiberState::Defined`].
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::ValueConstraintViolated`] if `event_id` is zero
    /// or if event count boundary is reached ([`MAX_EVENTS_PER_FIBER`]).
    /// Returns [`OperationFailure`] with [`FailureCondition::PrecursorChainBroken`] if `event_id` matches precursor.
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// if the fiber is in [`FiberState::Detached`] or [`FiberState::Locked`] without prior rescue.
    /// Handle fields are unchanged on error.
    pub fn append(
        &mut self,
        event_id: [u8; 16],
        payload: impl Into<Vec<u8>>,
    ) -> Result<EventEnvelope, OperationFailure> {
        if event_id == [0u8; 16] {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::Empty,
                },
                "event_id cannot be all-zeroes",
            ));
        }
        if event_id == self.precursor {
            return Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "event ID matches precursor event ID per C5.61",
            ));
        }
        if self.event_count >= MAX_EVENTS_PER_FIBER {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "event count boundary reached (MAX_EVENTS_PER_FIBER)",
            ));
        }
        let next_count = self.event_count + 1;

        let payload_bytes = payload.into();
        match self.state {
            FiberState::Undefined | FiberState::Purged => {
                let envelope = EventEnvelope::genesis(event_id, self.fiber_id, payload_bytes)
                    .map_err(|_| {
                        OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::Empty,
                            },
                            "failed to construct genesis envelope",
                        )
                    })?;
                self.precursor = event_id;
                self.precursor_hash = envelope.commitment();
                self.state = FiberState::Defined;
                self.event_count = next_count;
                Ok(envelope)
            }
            FiberState::Defined => {
                let envelope = EventEnvelope {
                    header: EnvelopeHeader {
                        event_id,
                        fiber_id: self.fiber_id,
                        detached: false,
                        precursor: self.precursor,
                        precursor_hash: self.precursor_hash,
                    },
                    payload: payload_bytes,
                };
                self.precursor = event_id;
                self.precursor_hash = envelope.commitment();
                self.event_count = next_count;
                Ok(envelope)
            }
            FiberState::Detached | FiberState::Locked => Err(OperationFailure::new(
                FailureCondition::InvariantBreakingConfiguration,
                format!(
                    "cannot append to fiber in {:?} state without rescue",
                    self.state
                ),
            )),
        }
    }

    /// Detaches the fiber, emitting a detached [`EventEnvelope`].
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::ValueConstraintViolated`] if `event_id` is zero
    /// or if event count boundary is reached ([`MAX_EVENTS_PER_FIBER`]).
    /// Returns [`OperationFailure`] with [`FailureCondition::PrecursorChainBroken`] if `event_id` matches precursor.
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// if the fiber is not in [`FiberState::Defined`] state.
    /// Handle fields are unchanged on error.
    pub fn detach(
        &mut self,
        event_id: [u8; 16],
        payload: impl Into<Vec<u8>>,
    ) -> Result<EventEnvelope, OperationFailure> {
        if event_id == [0u8; 16] {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::Empty,
                },
                "event_id cannot be all-zeroes",
            ));
        }
        if event_id == self.precursor {
            return Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "event ID matches precursor event ID per C5.61",
            ));
        }
        if self.event_count >= MAX_EVENTS_PER_FIBER {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "event count boundary reached (MAX_EVENTS_PER_FIBER)",
            ));
        }
        let next_count = self.event_count + 1;

        let payload_bytes = payload.into();
        match self.state {
            FiberState::Defined => {
                let envelope = EventEnvelope {
                    header: EnvelopeHeader {
                        event_id,
                        fiber_id: self.fiber_id,
                        detached: true,
                        precursor: self.precursor,
                        precursor_hash: self.precursor_hash,
                    },
                    payload: payload_bytes,
                };
                self.precursor = event_id;
                self.precursor_hash = envelope.commitment();
                self.state = FiberState::Detached;
                self.event_count = next_count;
                Ok(envelope)
            }
            _ => Err(OperationFailure::new(
                FailureCondition::InvariantBreakingConfiguration,
                format!(
                    "cannot detach fiber in {:?} state; detach is only valid from Defined",
                    self.state
                ),
            )),
        }
    }

    /// Rescues a detached or locked fiber, returning the fiber to [`FiberState::Defined`].
    ///
    /// Chaining continues from the prior precursor if [`FiberState::Detached`].
    /// Resets precursor to genesis if [`FiberState::Locked`].
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::ValueConstraintViolated`] if `event_id` is zero
    /// or if event count boundary is reached ([`MAX_EVENTS_PER_FIBER`]).
    /// Returns [`OperationFailure`] with [`FailureCondition::PrecursorChainBroken`] if `event_id` matches precursor.
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// if the fiber is not in [`FiberState::Detached`] or [`FiberState::Locked`] state.
    /// Handle fields are unchanged on error.
    pub fn rescue(
        &mut self,
        event_id: [u8; 16],
        payload: impl Into<Vec<u8>>,
    ) -> Result<EventEnvelope, OperationFailure> {
        if event_id == [0u8; 16] {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::Empty,
                },
                "event_id cannot be all-zeroes",
            ));
        }
        if event_id == self.precursor {
            return Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "event ID matches precursor event ID per C5.61",
            ));
        }
        if self.event_count >= MAX_EVENTS_PER_FIBER {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "event count boundary reached (MAX_EVENTS_PER_FIBER)",
            ));
        }
        let next_count = self.event_count + 1;

        let payload_bytes = payload.into();
        match self.state {
            FiberState::Detached => {
                let envelope = EventEnvelope {
                    header: EnvelopeHeader {
                        event_id,
                        fiber_id: self.fiber_id,
                        detached: false,
                        precursor: self.precursor,
                        precursor_hash: self.precursor_hash,
                    },
                    payload: payload_bytes,
                };
                self.precursor = event_id;
                self.precursor_hash = envelope.commitment();
                self.state = FiberState::Defined;
                self.event_count = next_count;
                Ok(envelope)
            }
            FiberState::Locked => {
                let envelope = EventEnvelope::genesis(event_id, self.fiber_id, payload_bytes)
                    .map_err(|_| {
                        OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::Empty,
                            },
                            "failed to construct genesis envelope on rescue locked",
                        )
                    })?;
                self.precursor = event_id;
                self.precursor_hash = envelope.commitment();
                self.state = FiberState::Defined;
                self.event_count = next_count;
                Ok(envelope)
            }
            _ => Err(OperationFailure::new(
                FailureCondition::InvariantBreakingConfiguration,
                format!(
                    "cannot rescue fiber in {:?} state; rescue is only valid from Detached or Locked",
                    self.state
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fiber_handle_initial_append() {
        let mut handle = FiberHandle::new([0x01; 16]);
        assert_eq!(handle.state(), FiberState::Undefined);
        assert!(handle.is_undefined());
        assert!(!handle.is_active());
        assert_eq!(handle.event_count(), 0);

        let env1 = handle
            .append([0x02; 16], b"genesis")
            .expect("append genesis");
        assert_eq!(handle.state(), FiberState::Defined);
        assert!(handle.is_active());
        assert_eq!(handle.precursor(), [0x02; 16]);
        assert_eq!(handle.precursor_hash(), env1.commitment());
        assert_eq!(handle.event_count(), 1);

        assert_eq!(env1.header.fiber_id, [0x01; 16]);
        assert_eq!(env1.header.event_id, [0x02; 16]);
        assert_eq!(env1.header.precursor, [0u8; 16]);
        assert_eq!(env1.header.precursor_hash, [0u8; 32]);
        assert!(!env1.header.detached);
    }

    #[test]
    fn test_fiber_handle_chaining_and_commitment_equality() {
        let mut handle = FiberHandle::new([0x10; 16]);
        let env1 = handle.append([0x01; 16], b"first").expect("env1");
        let env2 = handle.append([0x02; 16], b"second").expect("env2");

        assert_eq!(env2.header.precursor, [0x01; 16]);
        assert_eq!(env2.header.precursor_hash, env1.commitment());
        assert_eq!(handle.precursor(), [0x02; 16]);
        assert_eq!(handle.precursor_hash(), env2.commitment());
        assert_eq!(handle.event_count(), 2);
    }

    #[test]
    fn test_fiber_handle_detach_and_rescue_lifecycle() {
        let mut handle = FiberHandle::new([0x20; 16]);
        let env1 = handle.append([0x01; 16], b"v1").expect("env1");
        assert!(handle.is_active());

        let env_detach = handle.detach([0x02; 16], b"soft-delete").expect("detach");
        assert_eq!(handle.state(), FiberState::Detached);
        assert!(handle.is_detached());
        assert!(!handle.is_active());
        assert_eq!(env_detach.header.precursor, [0x01; 16]);
        assert_eq!(env_detach.header.precursor_hash, env1.commitment());
        assert!(env_detach.header.detached);

        let err = handle.append([0x03; 16], b"illegal").unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err_detach = handle.detach([0x03; 16], b"double-detach").unwrap_err();
        assert_eq!(
            *err_detach.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let env_rescue = handle.rescue([0x03; 16], b"restore").expect("rescue");
        assert_eq!(handle.state(), FiberState::Defined);
        assert!(handle.is_active());
        assert_eq!(env_rescue.header.precursor, [0x02; 16]);
        assert_eq!(env_rescue.header.precursor_hash, env_detach.commitment());
        assert!(!env_rescue.header.detached);
        assert_eq!(handle.event_count(), 3);

        let env4 = handle.append([0x04; 16], b"v2").expect("env4");
        assert_eq!(env4.header.precursor, [0x03; 16]);
        assert_eq!(env4.header.precursor_hash, env_rescue.commitment());
    }

    #[test]
    fn test_fiber_handle_rescue_locked_resets_genesis_precursor() {
        let mut handle =
            FiberHandle::with_state([0x30; 16], FiberState::Locked, [0x99; 16], [0xee; 32], 1)
                .expect("valid locked handle");
        assert!(handle.is_locked());
        assert!(!handle.is_active());

        let err = handle.append([0x01; 16], b"append locked").unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let env_rescue = handle
            .rescue([0x01; 16], b"genesis after lock")
            .expect("rescue locked");
        assert_eq!(handle.state(), FiberState::Defined);
        assert!(handle.is_active());
        assert_eq!(env_rescue.header.precursor, [0u8; 16]);
        assert_eq!(env_rescue.header.precursor_hash, [0u8; 32]);
        assert_eq!(handle.precursor(), [0x01; 16]);
        assert_eq!(handle.precursor_hash(), env_rescue.commitment());
    }

    #[test]
    fn test_fiber_handle_purged_append_creates_genesis() {
        let mut handle =
            FiberHandle::with_state([0x40; 16], FiberState::Purged, [0u8; 16], [0u8; 32], 0)
                .expect("valid purged handle");
        assert!(handle.is_purged());
        assert!(!handle.is_active());

        let env_genesis = handle
            .append([0x01; 16], b"fresh genesis")
            .expect("append purged");
        assert_eq!(handle.state(), FiberState::Defined);
        assert!(handle.is_active());
        assert_eq!(env_genesis.header.precursor, [0u8; 16]);
        assert_eq!(env_genesis.header.precursor_hash, [0u8; 32]);
        assert_eq!(handle.precursor(), [0x01; 16]);
        assert_eq!(handle.precursor_hash(), env_genesis.commitment());
    }

    #[test]
    fn test_fiber_handle_zero_event_id_rejection() {
        let mut handle = FiberHandle::new([0x50; 16]);
        let err = handle.append([0u8; 16], b"zero").unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::Empty,
            }
        );

        let env1 = handle.append([0x01; 16], b"ok").expect("env1");
        assert_eq!(handle.event_count(), 1);

        let err_detach = handle.detach([0u8; 16], b"zero").unwrap_err();
        assert_eq!(
            *err_detach.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::Empty,
            }
        );

        let _ = handle.detach([0x02; 16], b"detached").expect("detach");
        let err_rescue = handle.rescue([0u8; 16], b"zero").unwrap_err();
        assert_eq!(
            *err_rescue.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::Empty,
            }
        );
        let _ = env1;
    }

    #[test]
    fn test_fiber_handle_from_envelopes_reconstitution() {
        let fiber_id = [0x60; 16];
        let mut original = FiberHandle::new(fiber_id);
        let env1 = original.append([0x01; 16], b"one").expect("one");
        let env2 = original.append([0x02; 16], b"two").expect("two");
        let env3 = original.detach([0x03; 16], b"three").expect("three");

        let reconstituted =
            FiberHandle::from_envelopes(fiber_id, &[env1.clone(), env2.clone(), env3.clone()])
                .expect("reconstituted");
        assert_eq!(reconstituted.fiber_id(), fiber_id);
        assert_eq!(reconstituted.state(), FiberState::Detached);
        assert_eq!(reconstituted.precursor(), [0x03; 16]);
        assert_eq!(reconstituted.precursor_hash(), env3.commitment());
        assert_eq!(reconstituted.event_count(), 3);

        let wrong_fiber =
            FiberHandle::from_envelopes([0x99; 16], std::slice::from_ref(&env1)).unwrap_err();
        assert_eq!(
            *wrong_fiber.condition(),
            FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorWrongFiber))
        );

        let broken_env = EventEnvelope {
            header: EnvelopeHeader {
                event_id: [0x04; 16],
                fiber_id,
                detached: false,
                precursor: [0xaa; 16],
                precursor_hash: [0xbb; 32],
            },
            payload: vec![],
        };
        let broken = FiberHandle::from_envelopes(fiber_id, &[env1, env2, broken_env]).unwrap_err();
        assert_eq!(
            *broken.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );

        let empty = FiberHandle::from_envelopes(fiber_id, &[]).expect("empty envelopes");
        assert_eq!(empty.state(), FiberState::Undefined);
        assert_eq!(empty.event_count(), 0);
    }

    #[test]
    fn test_fiber_handle_with_state_contradictory_combinations() {
        let f = [0x70; 16];

        let err1 =
            FiberHandle::with_state(f, FiberState::Defined, [0u8; 16], [0x11; 32], 1).unwrap_err();
        assert_eq!(
            *err1.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err2 =
            FiberHandle::with_state(f, FiberState::Defined, [0x01; 16], [0u8; 32], 1).unwrap_err();
        assert_eq!(
            *err2.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err3 =
            FiberHandle::with_state(f, FiberState::Defined, [0x01; 16], [0x11; 32], 0).unwrap_err();
        assert_eq!(
            *err3.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err4 = FiberHandle::with_state(f, FiberState::Undefined, [0x01; 16], [0u8; 32], 0)
            .unwrap_err();
        assert_eq!(
            *err4.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err5 = FiberHandle::with_state(f, FiberState::Undefined, [0u8; 16], [0x11; 32], 0)
            .unwrap_err();
        assert_eq!(
            *err5.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err6 =
            FiberHandle::with_state(f, FiberState::Undefined, [0u8; 16], [0u8; 32], 1).unwrap_err();
        assert_eq!(
            *err6.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err7 =
            FiberHandle::with_state(f, FiberState::Detached, [0u8; 16], [0x11; 32], 1).unwrap_err();
        assert_eq!(
            *err7.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err8 =
            FiberHandle::with_state(f, FiberState::Detached, [0x01; 16], [0u8; 32], 1).unwrap_err();
        assert_eq!(
            *err8.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err9 = FiberHandle::with_state(f, FiberState::Detached, [0x01; 16], [0x11; 32], 0)
            .unwrap_err();
        assert_eq!(
            *err9.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err10 =
            FiberHandle::with_state(f, FiberState::Locked, [0x01; 16], [0x11; 32], 0).unwrap_err();
        assert_eq!(
            *err10.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err11 =
            FiberHandle::with_state(f, FiberState::Locked, [0u8; 16], [0u8; 32], 1).unwrap_err();
        assert_eq!(
            *err11.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let ok_def =
            FiberHandle::with_state(f, FiberState::Defined, [0x01; 16], [0x11; 32], 1).unwrap();
        assert_eq!(ok_def.state(), FiberState::Defined);
        assert_eq!(ok_def.event_count(), 1);

        let ok_undef =
            FiberHandle::with_state(f, FiberState::Undefined, [0u8; 16], [0u8; 32], 0).unwrap();
        assert_eq!(ok_undef.state(), FiberState::Undefined);
        assert_eq!(ok_undef.event_count(), 0);
    }

    #[test]
    fn test_fiber_handle_from_envelopes_zero_event_id_and_duplicates() {
        let fiber_id = [0x80; 16];
        let mut original = FiberHandle::new(fiber_id);
        let env1 = original.append([0x01; 16], b"one").expect("one");

        let zero_env = EventEnvelope {
            header: EnvelopeHeader {
                event_id: [0u8; 16],
                fiber_id,
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: vec![],
        };
        let err_zero = FiberHandle::from_envelopes(fiber_id, &[zero_env]).unwrap_err();
        assert_eq!(
            *err_zero.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::Empty,
            }
        );

        let env2 = original.append([0x02; 16], b"two").expect("two");
        let duplicate_seq = vec![env1.clone(), env2, env1];
        let err_dup = FiberHandle::from_envelopes(fiber_id, &duplicate_seq).unwrap_err();
        assert_eq!(
            *err_dup.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
    }

    #[test]
    fn test_fiber_handle_duplicate_event_id_rejection() {
        let mut handle = FiberHandle::new([0x85; 16]);
        let _ = handle.append([0x01; 16], b"first").expect("first");

        let err = handle.append([0x01; 16], b"duplicate").unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
    }

    #[test]
    fn test_fiber_handle_exhaustive_illegal_transitions() {
        let fiber_id = [0x90; 16];

        let mut h_undef = FiberHandle::new(fiber_id);
        let err_undef_detach = h_undef.detach([0x01; 16], b"x").unwrap_err();
        assert_eq!(
            *err_undef_detach.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );
        let err_undef_rescue = h_undef.rescue([0x01; 16], b"x").unwrap_err();
        assert_eq!(
            *err_undef_rescue.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let mut h_defined = FiberHandle::new(fiber_id);
        h_defined.append([0x01; 16], b"genesis").unwrap();
        let err_def_rescue = h_defined.rescue([0x02; 16], b"x").unwrap_err();
        assert_eq!(
            *err_def_rescue.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let mut h_detached = FiberHandle::new(fiber_id);
        h_detached.append([0x01; 16], b"genesis").unwrap();
        h_detached.detach([0x02; 16], b"detach").unwrap();
        let err_det_append = h_detached.append([0x03; 16], b"x").unwrap_err();
        assert_eq!(
            *err_det_append.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );
        let err_det_detach = h_detached.detach([0x03; 16], b"x").unwrap_err();
        assert_eq!(
            *err_det_detach.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let mut h_locked =
            FiberHandle::with_state(fiber_id, FiberState::Locked, [0x01; 16], [0x11; 32], 1)
                .unwrap();
        let err_lock_append = h_locked.append([0x02; 16], b"x").unwrap_err();
        assert_eq!(
            *err_lock_append.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );
        let err_lock_detach = h_locked.detach([0x02; 16], b"x").unwrap_err();
        assert_eq!(
            *err_lock_detach.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let mut h_purged =
            FiberHandle::with_state(fiber_id, FiberState::Purged, [0u8; 16], [0u8; 32], 0).unwrap();
        let err_purge_detach = h_purged.detach([0x02; 16], b"x").unwrap_err();
        assert_eq!(
            *err_purge_detach.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );
        let err_purge_rescue = h_purged.rescue([0x02; 16], b"x").unwrap_err();
        assert_eq!(
            *err_purge_rescue.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );
    }

    #[test]
    fn test_fiber_handle_h4_max_count_boundary_and_purged_rules() {
        let fiber_id = [0xa0; 16];

        let err_max_def = FiberHandle::with_state(
            fiber_id,
            FiberState::Defined,
            [0x01; 16],
            [0x11; 32],
            u64::MAX,
        )
        .unwrap_err();
        assert_eq!(
            *err_max_def.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err_max_det = FiberHandle::with_state(
            fiber_id,
            FiberState::Detached,
            [0x01; 16],
            [0x11; 32],
            u64::MAX,
        )
        .unwrap_err();
        assert_eq!(
            *err_max_det.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err_max_lock = FiberHandle::with_state(
            fiber_id,
            FiberState::Locked,
            [0x01; 16],
            [0x11; 32],
            u64::MAX,
        )
        .unwrap_err();
        assert_eq!(
            *err_max_lock.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let err_purged_nonzero =
            FiberHandle::with_state(fiber_id, FiberState::Purged, [0x01; 16], [0x11; 32], 1)
                .unwrap_err();
        assert_eq!(
            *err_purged_nonzero.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );

        let ok_purged =
            FiberHandle::with_state(fiber_id, FiberState::Purged, [0u8; 16], [0u8; 32], 0)
                .expect("purged with zeroes must succeed");
        assert_eq!(ok_purged.state(), FiberState::Purged);
        assert_eq!(ok_purged.event_count(), 0);

        let mut h = FiberHandle::with_state(
            fiber_id,
            FiberState::Defined,
            [0x01; 16],
            [0x11; 32],
            MAX_EVENTS_PER_FIBER - 1,
        )
        .expect("MAX_EVENTS_PER_FIBER - 1 must succeed");
        assert_eq!(h.event_count(), MAX_EVENTS_PER_FIBER - 1);

        let env_max = h.append([0x02; 16], b"reach max").expect("reach max");
        assert_eq!(h.event_count(), MAX_EVENTS_PER_FIBER);
        assert_eq!(h.precursor(), [0x02; 16]);
        assert_eq!(h.precursor_hash(), env_max.commitment());

        let h_reconstituted = FiberHandle::with_state(
            fiber_id,
            FiberState::Defined,
            [0x02; 16],
            env_max.commitment(),
            MAX_EVENTS_PER_FIBER,
        )
        .expect("reconstituting handle with MAX_EVENTS_PER_FIBER must succeed");
        assert_eq!(h_reconstituted, h);

        let snapshot_before = h.clone();
        let err_overflow = h.append([0x03; 16], b"overflow").unwrap_err();
        assert_eq!(
            *err_overflow.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
        assert_eq!(h, snapshot_before);

        let err_detach_overflow = h.detach([0x03; 16], b"detach overflow").unwrap_err();
        assert_eq!(
            *err_detach_overflow.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
        assert_eq!(h, snapshot_before);

        let mut h_detached_max = FiberHandle::with_state(
            fiber_id,
            FiberState::Detached,
            [0x02; 16],
            env_max.commitment(),
            MAX_EVENTS_PER_FIBER,
        )
        .expect("detached at max");
        let snapshot_det = h_detached_max.clone();
        let err_rescue_overflow = h_detached_max
            .rescue([0x03; 16], b"rescue overflow")
            .unwrap_err();
        assert_eq!(
            *err_rescue_overflow.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
        assert_eq!(h_detached_max, snapshot_det);
    }
}
