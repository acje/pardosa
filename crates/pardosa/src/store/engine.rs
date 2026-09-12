//! Minimal storage engine I/O contract.

use crate::encoding::{
    InboundPointerRecord, MigrationEndRecord, MigrationStartRecord, OutboundPointerRecord,
    OwnershipClaimRecord, OwnershipRecord, RescuePolicyChoiceRecord,
};
use crate::schema::SchemaDescriptor;
use crate::store::{
    BatchLandingVerdict, NextAttemptStatus, OpenAdmission, OperationFailure, WriteLandingVerdict,
};

/// Callback invoked for each recovered frame during incremental recovery.
pub type FrameRecoveryCallback<'a> = dyn FnMut(u64, &[u8]) -> Result<(), OperationFailure> + 'a;

/// Minimal I/O contract for storage drivers.
pub trait StorageEngine {
    /// Returns the carried epoch for this engine session.
    fn carried_epoch(&self) -> u64;

    /// Verifies authority to append, enforcing epoch fencing, retirement, and uncertainty checks.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`crate::store::FailureCondition::StaleEpoch`] if carried epoch is superseded.
    /// Returns [`OperationFailure`] with [`crate::store::FailureCondition::RetiredMigrationSource`] if retired.
    /// Returns [`OperationFailure`] with [`crate::store::FailureCondition::OwnershipRecordUnreadable`] if uncertain or unreadable.
    fn check_authority(&self) -> Result<(), OperationFailure> {
        Ok(())
    }

    /// Appends a raw frame block, returning the write landing verdict with sequence or frame count.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if storage write or synchronization fails.
    fn append_block(&mut self, block: &[u8]) -> Result<WriteLandingVerdict<u64>, OperationFailure>;

    /// Appends a batch of raw frame blocks, returning detailed landing progress.
    fn append_batch_detailed(&mut self, blocks: &[&[u8]]) -> BatchLandingVerdict<u64> {
        if let Err(error) = self.check_authority() {
            return BatchLandingVerdict::PreAttemptRefusal {
                error,
                unattempted_count: blocks.len(),
            };
        }
        if blocks.is_empty() {
            return BatchLandingVerdict::LandedAll {
                final_position: 0,
                landed_count: 0,
            };
        }
        let mut last_position = 0;

        for (i, block) in blocks.iter().enumerate() {
            match self.append_block(block) {
                Ok(WriteLandingVerdict::Landed(seq)) => {
                    last_position = seq;
                }
                Ok(WriteLandingVerdict::Undetermined { carried_epoch }) => {
                    return BatchLandingVerdict::PartialProgress {
                        landed_count: i,
                        next_attempt: NextAttemptStatus::Undetermined { carried_epoch },
                        unattempted_count: blocks.len().saturating_sub(i + 1),
                    };
                }
                Err(error) => {
                    return BatchLandingVerdict::PartialProgress {
                        landed_count: i,
                        next_attempt: NextAttemptStatus::Rejected(error),
                        unattempted_count: blocks.len().saturating_sub(i + 1),
                    };
                }
            }
        }

        BatchLandingVerdict::LandedAll {
            final_position: last_position,
            landed_count: blocks.len(),
        }
    }

    /// Appends a batch of raw frame blocks, returning the write landing verdict with sequence or frame count.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if storage write or synchronization fails.
    fn append_batch(
        &mut self,
        blocks: &[&[u8]],
    ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        match self.append_batch_detailed(blocks) {
            BatchLandingVerdict::LandedAll { final_position, .. } => {
                Ok(WriteLandingVerdict::Landed(final_position))
            }
            BatchLandingVerdict::PreAttemptRefusal { error, .. } => Err(error),
            BatchLandingVerdict::PartialProgress {
                next_attempt: NextAttemptStatus::Undetermined { carried_epoch },
                ..
            } => Ok(WriteLandingVerdict::Undetermined { carried_epoch }),
            BatchLandingVerdict::PartialProgress {
                next_attempt: NextAttemptStatus::Rejected(err),
                ..
            } => Err(err),
        }
    }

    /// Reads a single block at the specified sequence or index.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails.
    fn read_block(&mut self, index: u64) -> Result<Vec<u8>, OperationFailure> {
        let mut chunk = self.read_chunk(index, 1)?;
        chunk.pop().ok_or_else(|| {
            OperationFailure::new(
                crate::store::FailureCondition::ValueConstraintViolated {
                    constraint: crate::encoding::ValueConstraint::TooLong,
                },
                "block index out of bounds",
            )
        })
    }

    /// Reads all raw blocks in container order.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails.
    fn read_all(&mut self) -> Result<Vec<Vec<u8>>, OperationFailure>;

    /// Reads a chunk of raw blocks starting from sequence or index offset up to `max_items`.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails.
    fn read_chunk(
        &mut self,
        start_index: u64,
        max_items: usize,
    ) -> Result<Vec<Vec<u8>>, OperationFailure> {
        if max_items == 0 {
            return Ok(Vec::new());
        }
        let all = self.read_all()?;
        let start = usize::try_from(start_index).unwrap_or(usize::MAX);
        if start >= all.len() {
            return Ok(Vec::new());
        }
        let end = start.saturating_add(max_items).min(all.len());
        Ok(all[start..end].to_vec())
    }

    /// Recovers frames incrementally in chunks, invoking `on_frame` for each frame.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading or frame processing fails.
    fn recover_frames(
        &mut self,
        chunk_size: usize,
        on_frame: &mut FrameRecoveryCallback<'_>,
    ) -> Result<u64, OperationFailure> {
        let chunk_size = chunk_size.max(1);
        let mut start_index: u64 = 0;
        let mut total_frames: u64 = 0;
        loop {
            let chunk = self.read_chunk(start_index, chunk_size)?;
            if chunk.is_empty() {
                break;
            }
            let chunk_len = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
            for frame in &chunk {
                on_frame(total_frames, frame)?;
                total_frames = total_frames.saturating_add(1);
            }
            start_index = start_index.saturating_add(chunk_len);
            if chunk.len() < chunk_size {
                break;
            }
        }
        Ok(total_frames)
    }

    /// Acquires exclusion on the underlying storage.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if exclusion cannot be acquired.
    fn acquire_exclusion(&mut self) -> Result<(), OperationFailure> {
        Ok(())
    }

    /// Releases exclusion on the underlying storage.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if releasing exclusion fails.
    fn release_exclusion(&mut self) -> Result<(), OperationFailure> {
        Ok(())
    }

    /// Returns `true` if this storage medium is permanently retired as a migration source.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if checking retirement status fails.
    fn is_retired(&self) -> Result<bool, OperationFailure>;

    /// Synchronizes storage buffers to durable medium.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if synchronization fails.
    fn sync(&mut self) -> Result<(), OperationFailure>;

    /// Configures the publish timeout duration if supported by the driver.
    fn set_publish_timeout(&mut self, _timeout: std::time::Duration) {}

    /// Configures whether to simulate an indeterminate write landing per C5.16 if supported by the driver.
    fn set_simulate_indeterminate(&mut self, _simulate: bool) {}

    /// Returns diagnostic detail if the engine entered an uncertain write state.
    fn uncertain_diagnostic(&self) -> Option<&str>;

    /// Returns the active ownership claim record, if established.
    fn claim(&self) -> Option<&OwnershipClaimRecord>;

    /// Returns the open admission mode for this engine.
    fn admission(&self) -> &OpenAdmission {
        &OpenAdmission::Ready
    }

    /// Returns the schema descriptor attached to this artefact, if present.
    fn schema_descriptor(&self) -> Option<&SchemaDescriptor>;

    /// Attaches or updates the schema descriptor on this artefact.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if writing descriptor fails.
    fn set_schema_descriptor(
        &mut self,
        descriptor: &SchemaDescriptor,
    ) -> Result<(), OperationFailure>;

    /// Records an ownership record into metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    fn record_meta_record(&mut self, record: &OwnershipRecord) -> Result<(), OperationFailure>;

    /// Returns the outbound pointer record, if recorded.
    fn outbound_pointer(&self) -> Option<&OutboundPointerRecord>;

    /// Returns the inbound pointer record, if recorded.
    fn inbound_pointer(&self) -> Option<&InboundPointerRecord>;

    /// Returns the migration start record, if recorded.
    fn migration_start(&self) -> Option<&MigrationStartRecord>;

    /// Returns the migration end record, if recorded.
    fn migration_end(&self) -> Option<&MigrationEndRecord>;

    /// Returns the rescue policy choice record, if recorded.
    fn rescue_policy_choice(&self) -> Option<&RescuePolicyChoiceRecord>;

    /// Appends an outbound pointer record to metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    fn record_outbound_pointer(
        &mut self,
        pointer: &OutboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::OutboundPointer(pointer.clone()))
    }

    /// Appends an inbound pointer record to metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    fn record_inbound_pointer(
        &mut self,
        pointer: &InboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::InboundPointer(pointer.clone()))
    }

    /// Appends a migration start record to metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    fn record_migration_start(
        &mut self,
        start: &MigrationStartRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::MigrationStart(start.clone()))
    }

    /// Appends a migration end record to metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    fn record_migration_end(&mut self, end: &MigrationEndRecord) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::MigrationEnd(end.clone()))
    }

    /// Appends a rescue policy choice record to metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    fn record_rescue_policy_choice(
        &mut self,
        choice: &RescuePolicyChoiceRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::RescuePolicyChoice(choice.clone()))
    }
}
