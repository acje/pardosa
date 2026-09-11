//! Unified storage pipeline owning fiber handles, caching, and state verification per C5 and C6.

use crate::encoding::{
    EventEnvelope, InboundPointerRecord, MigrationEndRecord, MigrationStartRecord,
    OutboundPointerRecord, OwnershipClaimRecord, OwnershipRecord, RescuePolicyChoiceRecord,
};
use crate::file::{ContainerFrame, RollingCommitment};
use crate::schema::{derive_fiber_id, SchemaDescriptor};
use crate::store::engine::StorageEngine;
use crate::store::fiber_handle::FiberHandle;
use crate::store::session_index::SessionIndex;
use crate::store::{FailureCondition, OpenAdmission, OperationFailure, WriteLandingVerdict};
use std::fmt;

/// Unified storage pipeline owning fiber handles, caching, and state verification per C5 and C6.
pub struct Store<E: StorageEngine> {
    pub(crate) engine: E,
    pub(crate) rolling_commitment: RollingCommitment,
    pub(crate) fiber_index: SessionIndex,
    pub(crate) broken_reader_cause: Option<FailureCondition>,
}

impl<E: StorageEngine + fmt::Debug> fmt::Debug for Store<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store")
            .field("engine", &self.engine)
            .field("rolling_commitment", &self.rolling_commitment)
            .field("fiber_index", &self.fiber_index)
            .field("broken_reader_cause", &self.broken_reader_cause)
            .finish()
    }
}

impl<E: StorageEngine> Store<E> {
    /// Opens a writer storage pipeline by deriving state from the engine.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading existing frames or rebuilding session index fails.
    pub fn open_writer(mut engine: E) -> Result<Self, OperationFailure> {
        let frames = engine.read_all()?;
        let fiber_index = SessionIndex::build_from_frames(frames.iter().map(|f| f.as_slice()))?;
        let mut rolling_commitment = RollingCommitment::new();
        for frame in &frames {
            let mut frame_buf = Vec::new();
            ContainerFrame::encode_payload(frame, &mut frame_buf);
            rolling_commitment.update_frame(&frame_buf);
        }
        Ok(Self {
            engine,
            rolling_commitment,
            fiber_index,
            broken_reader_cause: None,
        })
    }

    /// Opens a reader storage pipeline by deriving state from the engine.
    #[must_use]
    pub fn open_reader(mut engine: E) -> Self {
        let frames_res = engine.read_all();
        match frames_res {
            Ok(frames) => {
                let mut rolling = RollingCommitment::new();
                for frame in &frames {
                    let mut frame_buf = Vec::new();
                    ContainerFrame::encode_payload(frame, &mut frame_buf);
                    rolling.update_frame(&frame_buf);
                }
                match SessionIndex::build_from_frames(frames.iter().map(|f| f.as_slice())) {
                    Ok(fiber_index) => Self {
                        engine,
                        rolling_commitment: rolling,
                        fiber_index,
                        broken_reader_cause: None,
                    },
                    Err(err) => Self {
                        engine,
                        rolling_commitment: rolling,
                        fiber_index: SessionIndex::new(),
                        broken_reader_cause: Some(err.condition().clone()),
                    },
                }
            }
            Err(err) => Self {
                engine,
                rolling_commitment: RollingCommitment::new(),
                fiber_index: SessionIndex::new(),
                broken_reader_cause: Some(err.condition().clone()),
            },
        }
    }

    /// Returns a reference to the underlying storage engine.
    #[must_use]
    pub fn engine(&self) -> &E {
        &self.engine
    }

    /// Configures the publish timeout duration on the underlying engine if supported.
    pub fn set_publish_timeout(&mut self, timeout: std::time::Duration) {
        self.engine.set_publish_timeout(timeout);
    }

    /// Configures whether to simulate an indeterminate write landing on the underlying engine if supported.
    pub fn set_simulate_indeterminate(&mut self, simulate: bool) {
        self.engine.set_simulate_indeterminate(simulate);
    }

    /// Returns the carried epoch for this session.
    #[must_use]
    pub fn carried_epoch(&self) -> u64 {
        self.engine.carried_epoch()
    }

    /// Returns the diagnostic detail string if the engine entered an uncertain write state.
    #[must_use]
    pub fn uncertain_diagnostic(&self) -> Option<&str> {
        self.engine.uncertain_diagnostic()
    }

    /// Returns the active ownership claim record, if established.
    #[must_use]
    pub fn claim(&self) -> Option<&OwnershipClaimRecord> {
        self.engine.claim()
    }

    /// Returns a reference to the rolling commitment tracker.
    #[must_use]
    pub fn rolling_commitment(&self) -> &RollingCommitment {
        &self.rolling_commitment
    }

    /// Returns the open admission mode for this session.
    #[must_use]
    pub fn admission(&self) -> &OpenAdmission {
        self.engine.admission()
    }

    /// Returns `true` if this artefact has been retired as a migration source.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading retirement status fails.
    pub fn is_retired_source(&self) -> Result<bool, OperationFailure> {
        self.engine.is_retired()
    }

    /// Returns the schema descriptor attached to this artefact, if present.
    #[must_use]
    pub fn schema_descriptor(&self) -> Option<&SchemaDescriptor> {
        self.engine.schema_descriptor()
    }

    /// Validates the attached schema descriptor's structural completeness per C8.2.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if descriptor is absent or structurally incomplete.
    pub fn validate_schema_completeness(&self) -> Result<(), OperationFailure> {
        match self.schema_descriptor() {
            Some(descriptor) => descriptor.validate_structural_completeness(),
            None => Err(OperationFailure::new(
                FailureCondition::MissingSchemaDescriptor,
                "artefact schema descriptor is absent per C8.2",
            )),
        }
    }

    /// Attaches and validates a schema descriptor to the artefact per C8.2.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if authority verification or storage write fails.
    pub fn set_schema_descriptor(
        &mut self,
        descriptor: &SchemaDescriptor,
    ) -> Result<(), OperationFailure> {
        descriptor.validate_structural_completeness()?;
        self.engine.check_authority()?;
        self.engine.set_schema_descriptor(descriptor)
    }

    /// Returns the outbound pointer record, if recorded.
    #[must_use]
    pub fn outbound_pointer(&self) -> Option<&OutboundPointerRecord> {
        self.engine.outbound_pointer()
    }

    /// Returns the inbound pointer record, if recorded.
    #[must_use]
    pub fn inbound_pointer(&self) -> Option<&InboundPointerRecord> {
        self.engine.inbound_pointer()
    }

    /// Returns the migration start record, if recorded.
    #[must_use]
    pub fn migration_start(&self) -> Option<&MigrationStartRecord> {
        self.engine.migration_start()
    }

    /// Returns the migration end record, if recorded.
    #[must_use]
    pub fn migration_end(&self) -> Option<&MigrationEndRecord> {
        self.engine.migration_end()
    }

    /// Returns the rescue policy choice record, if recorded.
    #[must_use]
    pub fn rescue_policy_choice(&self) -> Option<&RescuePolicyChoiceRecord> {
        self.engine.rescue_policy_choice()
    }

    /// Records an arbitrary ownership record into metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if authority verification or recording fails.
    pub fn record_meta_record(&mut self, record: &OwnershipRecord) -> Result<(), OperationFailure> {
        self.engine.check_authority()?;
        self.engine.record_meta_record(record)
    }

    /// Records an outbound pointer record into metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    pub fn record_outbound_pointer(
        &mut self,
        pointer: &OutboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.engine.record_outbound_pointer(pointer)
    }

    /// Records an inbound pointer record into metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    pub fn record_inbound_pointer(
        &mut self,
        pointer: &InboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.engine.record_inbound_pointer(pointer)
    }

    /// Records a migration start record into metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    pub fn record_migration_start(
        &mut self,
        start: &MigrationStartRecord,
    ) -> Result<(), OperationFailure> {
        self.engine.record_migration_start(start)
    }

    /// Records a migration end record into metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    pub fn record_migration_end(
        &mut self,
        end: &MigrationEndRecord,
    ) -> Result<(), OperationFailure> {
        self.engine.record_migration_end(end)
    }

    /// Records a rescue policy choice record into metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    pub fn record_rescue_policy_choice(
        &mut self,
        choice: &RescuePolicyChoiceRecord,
    ) -> Result<(), OperationFailure> {
        self.engine.record_rescue_policy_choice(choice)
    }

    /// Synchronizes storage buffers to durable medium.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if synchronization fails.
    pub fn sync(&mut self) -> Result<(), OperationFailure> {
        self.engine.sync()
    }

    /// Returns a [`FiberHandle`] for the specified fiber identifier.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reader has failed or fiber is broken.
    pub fn fiber(&self, fiber_id: [u8; 16]) -> Result<FiberHandle, OperationFailure> {
        if self.engine.uncertain_diagnostic().is_some() {
            return Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "writer session in uncertain state; reconciliation required",
            ));
        }
        if let Some(ref cause) = self.broken_reader_cause {
            return Err(OperationFailure::new(
                cause.clone(),
                "reader session retained broken state; point lookups unavailable",
            ));
        }
        self.fiber_index.fiber(fiber_id)
    }

    /// Returns a [`FiberHandle`] by deriving its identifier from a domain key string.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reader has failed or fiber is broken.
    pub fn fiber_with_key(&self, domain_key: &str) -> Result<FiberHandle, OperationFailure> {
        self.fiber(derive_fiber_id(domain_key))
    }

    /// Returns the latest committed [`EventEnvelope`] on the specified fiber.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reader has failed or fiber is broken.
    pub fn get_latest(
        &self,
        fiber_id: [u8; 16],
    ) -> Result<Option<&EventEnvelope>, OperationFailure> {
        if self.engine.uncertain_diagnostic().is_some() {
            return Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "writer session in uncertain state; reconciliation required",
            ));
        }
        if let Some(ref cause) = self.broken_reader_cause {
            return Err(OperationFailure::new(
                cause.clone(),
                "reader session retained broken state; point lookups unavailable",
            ));
        }
        self.fiber_index.get_latest(&fiber_id)
    }

    /// Returns the latest committed [`EventEnvelope`] by deriving its identifier from a domain key string.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reader has failed or fiber is broken.
    pub fn get_latest_with_key(
        &self,
        domain_key: &str,
    ) -> Result<Option<&EventEnvelope>, OperationFailure> {
        self.get_latest(derive_fiber_id(domain_key))
    }

    fn append_frame_raw(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        let mut frame_buf = Vec::new();
        ContainerFrame::encode_payload(payload, &mut frame_buf);
        let verdict = self.engine.append_block(&frame_buf)?;
        match verdict {
            WriteLandingVerdict::Landed(_) => {
                self.rolling_commitment.update_frame(&frame_buf);
                Ok(self.rolling_commitment.frame_count())
            }
            WriteLandingVerdict::Undetermined { .. } => Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "write landing undetermined",
            )),
        }
    }

    /// Appends a raw frame payload without session index pre-admission validation.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if write or sync fails.
    pub fn append_raw_frame(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        self.engine.check_authority()?;
        if payload.len() >= 85 {
            if let Ok((env, consumed)) = EventEnvelope::decode(payload) {
                if consumed == payload.len() {
                    self.fiber_index.validate_append(&env)?;
                    let count = self.append_frame_raw(payload)?;
                    self.fiber_index.commit_envelope_unchecked(env);
                    return Ok(count);
                }
            }
        }
        let count = self.append_frame_raw(payload)?;
        self.fiber_index.mark_has_raw_frames();
        Ok(count)
    }

    /// Appends an unvalidated raw frame directly to the container for testing or migration recovery.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if authority verification or storage write fails.
    #[doc(hidden)]
    pub fn append_unvalidated_frame(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        self.engine.check_authority()?;
        let count = self.append_frame_raw(payload)?;
        self.fiber_index.mark_has_raw_frames();
        Ok(count)
    }

    /// Appends a framed payload byte slice, returning a [`WriteLandingVerdict`].
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if validation or storage write fails.
    pub fn append_frame_verdict(
        &mut self,
        payload: &[u8],
    ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        self.engine.check_authority()?;
        if payload.len() < 85 {
            return Err(OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                format!("payload too short for event envelope: {}", payload.len()),
            ));
        }
        let (env, consumed) = EventEnvelope::decode(payload).map_err(|err| {
            OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                format!("failed to decode envelope: {err}"),
            )
        })?;
        if consumed != payload.len() {
            return Err(OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                format!(
                    "frame decode error: {}",
                    crate::encoding::DecodeError::TruncatedPayload {
                        expected: payload.len(),
                        available: consumed,
                    }
                ),
            ));
        }

        self.fiber_index.validate_append(&env)?;

        let mut frame_buf = Vec::new();
        ContainerFrame::encode_payload(payload, &mut frame_buf);

        let verdict = self.engine.append_block(&frame_buf)?;
        match verdict {
            WriteLandingVerdict::Landed(_) => {
                self.rolling_commitment.update_frame(&frame_buf);
                self.fiber_index.commit_envelope_unchecked(env);
                Ok(WriteLandingVerdict::Landed(
                    self.rolling_commitment.frame_count(),
                ))
            }
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                Ok(WriteLandingVerdict::Undetermined { carried_epoch })
            }
        }
    }

    /// Appends a framed payload byte slice, unwrapping the landing verdict.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if validation, storage write, or landing fails.
    pub fn append_frame(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        match self.append_frame_verdict(payload)? {
            WriteLandingVerdict::Landed(count) => Ok(count),
            WriteLandingVerdict::Undetermined { .. } => Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "write landing undetermined: operation may or may not have landed",
            )),
        }
    }

    /// Appends an event envelope returning a [`WriteLandingVerdict`].
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if validation or storage write fails.
    pub fn append_envelope_verdict(
        &mut self,
        envelope: &EventEnvelope,
    ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        let mut env_buf = Vec::new();
        envelope.encode(&mut env_buf);
        self.append_frame_verdict(&env_buf)
    }

    /// Appends an event envelope to the container after pre-landing validation.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if validation, storage write, or landing fails.
    pub fn append_envelope(&mut self, envelope: &EventEnvelope) -> Result<u64, OperationFailure> {
        match self.append_envelope_verdict(envelope)? {
            WriteLandingVerdict::Landed(count) => Ok(count),
            WriteLandingVerdict::Undetermined { .. } => Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "write landing undetermined: operation may or may not have landed",
            )),
        }
    }

    /// Appends a batch of framed payload byte slices, returning a [`WriteLandingVerdict`].
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if validation or storage write fails.
    pub fn append_batch_verdict(
        &mut self,
        payloads: &[&[u8]],
    ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        self.engine.check_authority()?;
        if payloads.is_empty() {
            return Ok(WriteLandingVerdict::Landed(
                self.rolling_commitment.frame_count(),
            ));
        }

        let mut scratch_index = self.fiber_index.clone();
        let mut frame_buffers = Vec::with_capacity(payloads.len());

        for payload in payloads {
            if payload.len() < 85 {
                return Err(OperationFailure::new(
                    FailureCondition::EnvelopeMismatch,
                    format!("payload too short for event envelope: {}", payload.len()),
                ));
            }
            let (env, consumed) = EventEnvelope::decode(payload).map_err(|err| {
                OperationFailure::new(
                    FailureCondition::EnvelopeMismatch,
                    format!("failed to decode envelope: {err}"),
                )
            })?;
            if consumed != payload.len() {
                return Err(OperationFailure::new(
                    FailureCondition::EnvelopeMismatch,
                    format!(
                        "frame decode error: {}",
                        crate::encoding::DecodeError::TruncatedPayload {
                            expected: payload.len(),
                            available: consumed,
                        }
                    ),
                ));
            }

            scratch_index.validate_append(&env)?;
            scratch_index.commit_envelope_unchecked(env);

            let mut frame_buf = Vec::new();
            ContainerFrame::encode_payload(payload, &mut frame_buf);
            frame_buffers.push(frame_buf);
        }

        let block_refs: Vec<&[u8]> = frame_buffers.iter().map(|f| f.as_slice()).collect();
        let verdict = self.engine.append_batch(&block_refs)?;

        match verdict {
            WriteLandingVerdict::Landed(_) => {
                for frame_buf in &frame_buffers {
                    self.rolling_commitment.update_frame(frame_buf);
                }
                self.fiber_index = scratch_index;
                Ok(WriteLandingVerdict::Landed(
                    self.rolling_commitment.frame_count(),
                ))
            }
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                Ok(WriteLandingVerdict::Undetermined { carried_epoch })
            }
        }
    }

    /// Appends a batch of framed payload byte slices, unwrapping the landing verdict.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if validation, storage write, or landing fails.
    pub fn append_batch(&mut self, payloads: &[&[u8]]) -> Result<u64, OperationFailure> {
        match self.append_batch_verdict(payloads)? {
            WriteLandingVerdict::Landed(count) => Ok(count),
            WriteLandingVerdict::Undetermined { .. } => Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "write landing undetermined: operation may or may not have landed",
            )),
        }
    }

    /// Appends a batch of event envelopes returning a [`WriteLandingVerdict`].
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if validation or storage write fails.
    pub fn append_batch_envelopes_verdict(
        &mut self,
        envelopes: &[EventEnvelope],
    ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        let mut encoded_payloads = Vec::with_capacity(envelopes.len());
        for env in envelopes {
            let mut env_buf = Vec::new();
            env.encode(&mut env_buf);
            encoded_payloads.push(env_buf);
        }
        let payload_refs: Vec<&[u8]> = encoded_payloads.iter().map(|p| p.as_slice()).collect();
        self.append_batch_verdict(&payload_refs)
    }

    /// Appends a batch of event envelopes to the container after pre-landing validation.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if validation, storage write, or landing fails.
    pub fn append_batch_envelopes(
        &mut self,
        envelopes: &[EventEnvelope],
    ) -> Result<u64, OperationFailure> {
        match self.append_batch_envelopes_verdict(envelopes)? {
            WriteLandingVerdict::Landed(count) => Ok(count),
            WriteLandingVerdict::Undetermined { .. } => Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "write landing undetermined: operation may or may not have landed",
            )),
        }
    }

    /// Appends an event to the specified fiber.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if append is rejected by fiber lifecycle or storage fails.
    pub fn append_to_fiber(
        &mut self,
        fiber_id: [u8; 16],
        event_id: [u8; 16],
        payload: impl Into<Vec<u8>>,
    ) -> Result<WriteLandingVerdict<EventEnvelope>, OperationFailure> {
        let mut handle = self.fiber(fiber_id)?;
        let envelope = handle.append(event_id, payload)?;
        let mut env_buf = Vec::new();
        envelope.encode(&mut env_buf);
        let verdict = self.append_frame_verdict(&env_buf)?;
        match verdict {
            WriteLandingVerdict::Landed(_) => Ok(WriteLandingVerdict::Landed(envelope)),
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                Ok(WriteLandingVerdict::Undetermined { carried_epoch })
            }
        }
    }

    /// Detaches the specified fiber.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if detach is rejected by fiber lifecycle or storage fails.
    pub fn detach_fiber(
        &mut self,
        fiber_id: [u8; 16],
        event_id: [u8; 16],
        payload: impl Into<Vec<u8>>,
    ) -> Result<WriteLandingVerdict<EventEnvelope>, OperationFailure> {
        let mut handle = self.fiber(fiber_id)?;
        let envelope = handle.detach(event_id, payload)?;
        let mut env_buf = Vec::new();
        envelope.encode(&mut env_buf);
        let verdict = self.append_frame_verdict(&env_buf)?;
        match verdict {
            WriteLandingVerdict::Landed(_) => Ok(WriteLandingVerdict::Landed(envelope)),
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                Ok(WriteLandingVerdict::Undetermined { carried_epoch })
            }
        }
    }

    /// Rescues the specified locked or detached fiber.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if rescue is rejected by fiber lifecycle or storage fails.
    pub fn rescue_fiber(
        &mut self,
        fiber_id: [u8; 16],
        event_id: [u8; 16],
        payload: impl Into<Vec<u8>>,
    ) -> Result<WriteLandingVerdict<EventEnvelope>, OperationFailure> {
        let mut handle = self.fiber(fiber_id)?;
        let envelope = handle.rescue(event_id, payload)?;
        let mut env_buf = Vec::new();
        envelope.encode(&mut env_buf);
        let verdict = self.append_frame_verdict(&env_buf)?;
        match verdict {
            WriteLandingVerdict::Landed(_) => Ok(WriteLandingVerdict::Landed(envelope)),
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                Ok(WriteLandingVerdict::Undetermined { carried_epoch })
            }
        }
    }

    /// Reads all raw frame payloads from the container.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails.
    pub fn read_all_frames(&mut self) -> Result<Vec<Vec<u8>>, OperationFailure> {
        let frames = match self.engine.read_all() {
            Ok(f) => f,
            Err(err) => {
                self.broken_reader_cause = Some(err.condition().clone());
                return Err(err);
            }
        };
        let mut rolling = RollingCommitment::new();
        for frame in &frames {
            let mut frame_buf = Vec::new();
            ContainerFrame::encode_payload(frame, &mut frame_buf);
            rolling.update_frame(&frame_buf);
        }
        self.rolling_commitment = rolling;
        Ok(frames)
    }

    fn read_all_envelopes_inner(
        &mut self,
        for_migration: bool,
    ) -> Result<Vec<EventEnvelope>, OperationFailure> {
        if !for_migration {
            if let Some(ref cause) = self.broken_reader_cause {
                return Err(OperationFailure::new(
                    cause.clone(),
                    "reader session retained broken state; point lookups unavailable",
                ));
            }
        }
        let res = (|| {
            let frames = self.read_all_frames()?;
            for frame in &frames {
                if frame.len() < 85 {
                    return Err(OperationFailure::new(
                        FailureCondition::EnvelopeMismatch,
                        format!("frame length < 85 bytes: {}", frame.len()),
                    ));
                }
            }
            let idx = SessionIndex::build_from_frames(frames.iter().map(|f| f.as_slice()))?;
            if !for_migration && idx.has_broken_fibers() {
                self.fiber_index = idx;
                return Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    "discovered break in precursor chain",
                ));
            }
            let envelopes = frames
                .iter()
                .map(|frame| SessionIndex::decode_and_validate_frame(frame))
                .collect::<Result<Vec<_>, _>>()?;
            self.fiber_index = idx;
            Ok(envelopes)
        })();

        if let Err(ref err) = res {
            self.broken_reader_cause = Some(err.condition().clone());
        }
        res
    }

    /// Reads and decodes all event envelopes from the container.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if any frame is invalid or history contains broken fibers.
    pub fn read_all_envelopes(&mut self) -> Result<Vec<EventEnvelope>, OperationFailure> {
        self.read_all_envelopes_inner(false)
    }

    /// Reads all event envelopes for migration, tolerating broken fiber history.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading frames fails.
    pub fn read_all_envelopes_for_migration(
        &mut self,
    ) -> Result<Vec<EventEnvelope>, OperationFailure> {
        self.read_all_envelopes_inner(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::FiberState;

    #[derive(Debug)]
    struct InMemoryEngine {
        epoch: u64,
        blocks: Vec<Vec<u8>>,
        retired: bool,
        uncertain: bool,
        uncertain_diagnostic: Option<String>,
        claim: Option<OwnershipClaimRecord>,
        admission: OpenAdmission,
        meta_records: crate::file::MetaRecords,
    }

    impl Default for InMemoryEngine {
        fn default() -> Self {
            Self {
                epoch: 0,
                blocks: Vec::new(),
                retired: false,
                uncertain: false,
                uncertain_diagnostic: None,
                claim: None,
                admission: OpenAdmission::Ready,
                meta_records: crate::file::MetaRecords::default(),
            }
        }
    }

    impl StorageEngine for InMemoryEngine {
        fn carried_epoch(&self) -> u64 {
            self.epoch
        }

        fn check_authority(&self) -> Result<(), OperationFailure> {
            if self.uncertain {
                return Err(OperationFailure::new(
                    FailureCondition::OwnershipRecordUnreadable,
                    "writer session in uncertain state; reconciliation required",
                ));
            }
            if self.retired {
                return Err(OperationFailure::new(
                    FailureCondition::RetiredMigrationSource,
                    "artefact append authority permanently retired via outbound pointer per C5.63",
                ));
            }
            Ok(())
        }

        fn append_block(
            &mut self,
            block: &[u8],
        ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
            self.check_authority()?;
            self.blocks.push(block.to_vec());
            Ok(WriteLandingVerdict::Landed(self.blocks.len() as u64))
        }

        fn append_batch(
            &mut self,
            blocks: &[&[u8]],
        ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
            self.check_authority()?;
            for block in blocks {
                self.blocks.push(block.to_vec());
            }
            Ok(WriteLandingVerdict::Landed(self.blocks.len() as u64))
        }

        fn read_all(&mut self) -> Result<Vec<Vec<u8>>, OperationFailure> {
            let mut frames = Vec::new();
            for block in &self.blocks {
                let (payload, _) = ContainerFrame::decode(block).map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::PrecursorChainBroken(None),
                        format!("frame decode error: {err}"),
                    )
                })?;
                frames.push(payload);
            }
            Ok(frames)
        }

        fn is_retired(&self) -> Result<bool, OperationFailure> {
            Ok(self.retired)
        }

        fn sync(&mut self) -> Result<(), OperationFailure> {
            Ok(())
        }

        fn uncertain_diagnostic(&self) -> Option<&str> {
            self.uncertain_diagnostic.as_deref()
        }

        fn claim(&self) -> Option<&OwnershipClaimRecord> {
            self.claim.as_ref()
        }

        fn admission(&self) -> &OpenAdmission {
            &self.admission
        }

        fn schema_descriptor(&self) -> Option<&SchemaDescriptor> {
            self.meta_records.schema_descriptor.as_ref()
        }

        fn set_schema_descriptor(
            &mut self,
            descriptor: &SchemaDescriptor,
        ) -> Result<(), OperationFailure> {
            self.meta_records.schema_descriptor = Some(descriptor.clone());
            Ok(())
        }

        fn record_meta_record(&mut self, record: &OwnershipRecord) -> Result<(), OperationFailure> {
            match record {
                OwnershipRecord::OwnershipClaim(claim) => {
                    self.meta_records.latest_claim = Some(claim.clone());
                }
                OwnershipRecord::OutboundPointer(pointer) => {
                    self.meta_records.outbound_pointer = Some(pointer.clone());
                    self.retired = true;
                }
                _ => {}
            }
            Ok(())
        }

        fn outbound_pointer(&self) -> Option<&OutboundPointerRecord> {
            self.meta_records.outbound_pointer.as_ref()
        }

        fn inbound_pointer(&self) -> Option<&InboundPointerRecord> {
            self.meta_records.inbound_pointer.as_ref()
        }

        fn migration_start(&self) -> Option<&MigrationStartRecord> {
            self.meta_records.migration_start.as_ref()
        }

        fn migration_end(&self) -> Option<&MigrationEndRecord> {
            self.meta_records.migration_end.as_ref()
        }

        fn rescue_policy_choice(&self) -> Option<&RescuePolicyChoiceRecord> {
            self.meta_records.rescue_policy_choice.as_ref()
        }
    }

    #[test]
    fn test_store_in_memory_engine_append_and_read() {
        let engine = InMemoryEngine {
            epoch: 1,
            ..Default::default()
        };
        let mut store = Store::open_writer(engine).expect("open writer");

        let fiber_id = [0x11; 16];
        let event_id1 = [0x01; 16];
        let verdict1 = store
            .append_to_fiber(fiber_id, event_id1, b"first-event")
            .expect("append to fiber succeeds");
        assert_eq!(store.rolling_commitment().frame_count(), 1);

        match verdict1 {
            WriteLandingVerdict::Landed(env) => {
                assert_eq!(env.header.event_id, event_id1);
                assert_eq!(env.header.fiber_id, fiber_id);
            }
            WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
        }

        let handle1 = store.fiber(fiber_id).expect("fiber handle");
        assert_eq!(handle1.state(), FiberState::Defined);
        assert_eq!(handle1.event_count(), 1);
        assert_eq!(handle1.precursor(), event_id1);

        let latest = store
            .get_latest(fiber_id)
            .expect("get latest succeeds")
            .expect("some envelope");
        assert_eq!(latest.header.event_id, event_id1);
        assert_eq!(latest.payload, b"first-event");

        let event_id2 = [0x02; 16];
        let verdict2 = store
            .append_to_fiber(fiber_id, event_id2, b"second-event")
            .expect("second append succeeds");
        assert_eq!(store.rolling_commitment().frame_count(), 2);

        match verdict2 {
            WriteLandingVerdict::Landed(env) => {
                assert_eq!(env.header.event_id, event_id2);
                assert_eq!(env.header.fiber_id, fiber_id);
            }
            WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
        }

        let handle2 = store.fiber(fiber_id).expect("fiber handle");
        assert_eq!(handle2.state(), FiberState::Defined);
        assert_eq!(handle2.event_count(), 2);
        assert_eq!(handle2.precursor(), event_id2);

        let envelopes = store
            .read_all_envelopes()
            .expect("read all envelopes succeeds");
        assert_eq!(envelopes.len(), 2);
        assert_eq!(envelopes[0].payload, b"first-event");
        assert_eq!(envelopes[1].payload, b"second-event");
    }

    #[test]
    fn test_store_in_memory_engine_uncertainty_rejection() {
        let mut engine = InMemoryEngine {
            epoch: 1,
            ..Default::default()
        };
        engine.uncertain = true;
        engine.uncertain_diagnostic = Some("simulated uncertainty".to_string());

        let mut store = Store::open_writer(engine).expect("open writer");

        let err = store
            .append_to_fiber([0x11; 16], [0x01; 16], b"payload")
            .unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::OwnershipRecordUnreadable
        );
        assert!(err
            .to_string()
            .contains("writer session in uncertain state; reconciliation required"));
    }

    #[test]
    fn test_store_in_memory_engine_retirement_rejection() {
        let engine = InMemoryEngine {
            epoch: 1,
            retired: true,
            ..Default::default()
        };
        let mut store = Store::open_writer(engine).expect("open writer");

        let err = store
            .append_to_fiber([0x11; 16], [0x01; 16], b"payload")
            .unwrap_err();
        assert_eq!(*err.condition(), FailureCondition::RetiredMigrationSource);
        assert!(store.is_retired_source().unwrap());
    }

    #[test]
    fn test_store_in_memory_engine_detach_and_rescue_lifecycle() {
        let engine = InMemoryEngine {
            epoch: 1,
            ..Default::default()
        };
        let mut store = Store::open_writer(engine).expect("open writer");

        let fiber_id = [0x22; 16];
        store
            .append_to_fiber(fiber_id, [0x01; 16], b"init")
            .expect("init append");

        let detach_verdict = store
            .detach_fiber(fiber_id, [0x02; 16], b"soft-delete")
            .expect("detach");
        match detach_verdict {
            WriteLandingVerdict::Landed(env) => {
                assert!(env.header.detached);
            }
            WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
        }

        let handle_detached = store.fiber(fiber_id).expect("fiber handle");
        assert_eq!(handle_detached.state(), FiberState::Detached);

        let rescue_verdict = store
            .rescue_fiber(fiber_id, [0x03; 16], b"rescue:preserve_audit_trail")
            .expect("rescue");
        match rescue_verdict {
            WriteLandingVerdict::Landed(env) => {
                assert!(!env.header.detached);
            }
            WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
        }

        let handle_rescued = store.fiber(fiber_id).expect("fiber handle");
        assert_eq!(handle_rescued.state(), FiberState::Defined);
    }

    #[test]
    fn test_store_coherent_reopen_from_engine_snapshot() {
        let engine = InMemoryEngine {
            epoch: 1,
            ..Default::default()
        };
        let mut initial_store = Store::open_writer(engine).expect("open writer");
        let fiber_id = [0x33; 16];
        initial_store
            .append_to_fiber(fiber_id, [0x01; 16], b"genesis")
            .expect("append genesis");

        let reopened_engine = initial_store.engine;
        let mut reopened_writer =
            Store::open_writer(reopened_engine).expect("open writer coherent");
        assert_eq!(reopened_writer.rolling_commitment().frame_count(), 1);

        let handle = reopened_writer.fiber(fiber_id).expect("fiber handle");
        assert_eq!(handle.state(), FiberState::Defined);
        assert_eq!(handle.event_count(), 1);

        let duplicate_id_err = reopened_writer
            .append_to_fiber(fiber_id, [0x01; 16], b"duplicate_id")
            .unwrap_err();
        assert_eq!(
            *duplicate_id_err.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
    }

    #[test]
    fn test_store_metadata_authority_checks_and_terminal_reader_refusal() {
        let mut engine = InMemoryEngine {
            epoch: 1,
            ..Default::default()
        };
        engine.retired = true;
        let mut store = Store::open_writer(engine).expect("open writer");

        let err_meta = store
            .record_meta_record(&OwnershipRecord::RescuePolicyChoice(
                RescuePolicyChoiceRecord {
                    policy_tag: 0,
                    parameter_payload: vec![],
                },
            ))
            .unwrap_err();
        assert_eq!(
            *err_meta.condition(),
            FailureCondition::RetiredMigrationSource
        );

        let descriptor = SchemaDescriptor::new(1, crate::schema::DescriptorNode::U64);
        let err_schema = store.set_schema_descriptor(&descriptor).unwrap_err();
        assert_eq!(
            *err_schema.condition(),
            FailureCondition::RetiredMigrationSource
        );

        store.broken_reader_cause = Some(FailureCondition::PrecursorChainBroken(None));
        let err_point = store.get_latest([0x11; 16]).unwrap_err();
        assert_eq!(
            *err_point.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
    }

    #[test]
    fn test_store_append_batch_rolling_commitment_matches_single_append() {
        let fiber1 = [0x11; 16];
        let fiber2 = [0x22; 16];
        let env1 = EventEnvelope::genesis([0x01; 16], fiber1, b"f1-genesis".to_vec())
            .expect("env1 genesis");
        let env2 =
            EventEnvelope::chain(&env1, [0x02; 16], b"f1-event2".to_vec()).expect("env2 chain");
        let env3 = EventEnvelope::genesis([0x03; 16], fiber2, b"f2-genesis".to_vec())
            .expect("env3 genesis");

        let mut store_single = Store::open_writer(InMemoryEngine {
            epoch: 1,
            ..Default::default()
        })
        .expect("open single store");

        store_single.append_envelope(&env1).expect("append env1");
        store_single.append_envelope(&env2).expect("append env2");
        store_single.append_envelope(&env3).expect("append env3");

        let mut store_batch = Store::open_writer(InMemoryEngine {
            epoch: 1,
            ..Default::default()
        })
        .expect("open batch store");

        let batch_count = store_batch
            .append_batch_envelopes(&[env1, env2, env3])
            .expect("append batch envelopes");

        assert_eq!(batch_count, 3);
        assert_eq!(
            store_batch.rolling_commitment().frame_count(),
            store_single.rolling_commitment().frame_count()
        );
        assert_eq!(
            store_batch.rolling_commitment().current_commitment(),
            store_single.rolling_commitment().current_commitment()
        );

        let h1 = store_batch.fiber(fiber1).expect("fiber1");
        assert_eq!(h1.event_count(), 2);
        let h2 = store_batch.fiber(fiber2).expect("fiber2");
        assert_eq!(h2.event_count(), 1);
    }
}
