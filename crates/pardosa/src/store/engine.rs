//! Minimal storage engine I/O contract.

use crate::encoding::{
    InboundPointerRecord, MigrationEndRecord, MigrationStartRecord, OutboundPointerRecord,
    OwnershipClaimRecord, OwnershipRecord, RescuePolicyChoiceRecord,
};
use crate::schema::SchemaDescriptor;
use crate::store::{OpenAdmission, OperationFailure, WriteLandingVerdict};

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

    /// Appends a batch of raw frame blocks, returning the write landing verdict with sequence or frame count.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if storage write or synchronization fails.
    fn append_batch(
        &mut self,
        blocks: &[&[u8]],
    ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        self.check_authority()?;
        let mut last_verdict = WriteLandingVerdict::Landed(0);
        for block in blocks {
            match self.append_block(block)? {
                WriteLandingVerdict::Landed(seq) => {
                    last_verdict = WriteLandingVerdict::Landed(seq);
                }
                WriteLandingVerdict::Undetermined { carried_epoch } => {
                    return Ok(WriteLandingVerdict::Undetermined { carried_epoch });
                }
            }
        }
        Ok(last_verdict)
    }

    /// Reads a single block at the specified sequence or index.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails.
    fn read_block(&mut self, index: u64) -> Result<Vec<u8>, OperationFailure> {
        let all = self.read_all()?;
        let idx = usize::try_from(index).map_err(|_| {
            OperationFailure::new(
                crate::store::FailureCondition::ValueConstraintViolated {
                    constraint: crate::encoding::ValueConstraint::TooLong,
                },
                "block index out of bounds",
            )
        })?;
        all.into_iter().nth(idx).ok_or_else(|| {
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
