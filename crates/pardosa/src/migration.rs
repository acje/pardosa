//! Migration lifecycle, recovery, cutover, and dense re-chaining per C5.63, C6.16-20, and C12.2.

use crate::encoding::{EventEnvelope, OwnershipRecord};
pub use crate::encoding::{
    InboundPointerRecord, MigrationEndRecord, MigrationStartRecord, MigrationStatus,
    OutboundPointerRecord, RescuePolicy, RescuePolicyChoiceRecord,
};
use crate::file::{FileStorageAdapter, MetaRecords};
use crate::schema::{AdmittedDescriptor, PardosaSchema, SchemaDescriptor};
use crate::store::{FailureCondition, FiberMigrationPolicy, OperationFailure};
use std::collections::HashMap;
use std::fmt;

mod private {
    pub trait Sealed {}
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct InternalSeal;
    impl Sealed for InternalSeal {}
}

/// Sealed marker trait implemented only by officially witnessed migration contracts.
pub trait SealedMigrationWitness: private::Sealed {}

/// Compile-time witness proving that `Target` schema version is strictly `Source` version + 1.
///
/// # Compile-time Guarantees
/// This witness statically verifies the exact declared source/target schema version pairing
/// (`n -> n+1`) and transformer type. It does **not** guarantee semantic transform correctness,
/// backend stream identity, or external writer exclusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationWitness<Source: PardosaSchema, Target: PardosaSchema> {
    _marker: std::marker::PhantomData<(Source, Target)>,
    _seal: private::InternalSeal,
}

impl<Source: PardosaSchema, Target: PardosaSchema> private::Sealed
    for MigrationWitness<Source, Target>
{
}

impl<Source: PardosaSchema, Target: PardosaSchema> SealedMigrationWitness
    for MigrationWitness<Source, Target>
{
}

impl<Source: PardosaSchema, Target: PardosaSchema> Default for MigrationWitness<Source, Target> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Source: PardosaSchema, Target: PardosaSchema> MigrationWitness<Source, Target> {
    /// Compile-time constant assertion that `Target` is adjacent (`n -> n+1`) to `Source`.
    pub const ADJACENCY_ASSERTION: () = {
        assert!(
            Source::SCHEMA_VERSION < u32::MAX,
            "Source schema version must be less than u32::MAX"
        );
        assert!(
            Target::SCHEMA_VERSION == Source::SCHEMA_VERSION + 1,
            "Migration must be strict n -> n+1"
        );
    };

    /// Enforces at compile time that `Target` schema version is strictly `Source` version + 1.
    pub const fn assert_adjacent() {
        let () = Self::ADJACENCY_ASSERTION;
    }

    /// Creates a sealed migration witness after statically asserting version adjacency.
    #[must_use]
    pub const fn new() -> Self {
        const {
            Self::assert_adjacent();
        }
        Self {
            _marker: std::marker::PhantomData,
            _seal: private::InternalSeal,
        }
    }
}

/// Typed migration contract transforming events from [`Self::SourceEvent`] to [`Self::TargetEvent`].
///
/// # Compile-time Guarantees
/// Implementations declare typed source and target schema types. Compile-time guarantees
/// exact declared source/target schema/version pairing (`n -> n+1`) and transformer type,
/// NOT semantic transform correctness, backend stream identity, or external writer exclusion.
pub trait VersionMigration {
    /// Source event schema type.
    type SourceEvent: PardosaSchema;
    /// Target event schema type.
    type TargetEvent: PardosaSchema;
    /// Error returned if event transformation fails.
    type Error;

    /// Transforms a source event into a target event.
    ///
    /// # Errors
    /// Returns `Self::Error` if transformation fails.
    fn transform(&self, event: &Self::SourceEvent) -> Result<Self::TargetEvent, Self::Error>;
}

/// Wrapper around a [`VersionMigration`] whose source and target versions are verified
/// adjacent (`n -> n+1`) at compile time.
///
/// # Compile-time Guarantees
/// Construction enforces that `M::TargetEvent::SCHEMA_VERSION == M::SourceEvent::SCHEMA_VERSION + 1`
/// via [`MigrationWitness`]. Compile-time guarantees exact declared source/target schema/version pairing
/// (`n -> n+1`) and transformer type, NOT semantic transform correctness, backend stream identity,
/// or external writer exclusion.
#[derive(Debug, Clone)]
pub struct AdjacentMigration<M: VersionMigration> {
    migration: M,
    _witness: MigrationWitness<M::SourceEvent, M::TargetEvent>,
}

impl<M: VersionMigration> AdjacentMigration<M> {
    /// Creates a new adjacent migration wrapper, enforcing `MigrationWitness` adjacency at compile time.
    #[must_use]
    pub fn new(migration: M) -> Self {
        const {
            MigrationWitness::<M::SourceEvent, M::TargetEvent>::assert_adjacent();
        }
        Self {
            migration,
            _witness: MigrationWitness::new(),
        }
    }

    /// Returns a reference to the inner migration.
    #[must_use]
    pub fn migration(&self) -> &M {
        &self.migration
    }

    /// Consumes the wrapper and returns the inner migration.
    #[must_use]
    pub fn into_inner(self) -> M {
        self.migration
    }

    /// Transforms a source event into a target event via the inner migration.
    ///
    /// # Errors
    /// Returns `M::Error` if transformation fails.
    pub fn transform(&self, event: &M::SourceEvent) -> Result<M::TargetEvent, M::Error> {
        self.migration.transform(event)
    }
}

/// Caller election on broken precursor chain handling per C5.28 and T3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokenChainElection {
    /// Discovery of a broken precursor chain refuses migration with [`FailureCondition::PrecursorChainBroken`].
    RefuseOnBreak,
    /// Caller elects to enter broken history; broken or unanchored events are re-chained as genesis events.
    PermitBrokenHistory,
}

/// Lifecycle phase of an active or completed migration per C5.63.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationPhase {
    /// Initial phase before batch transfer.
    Initial,
    /// Chase phase: transferring events while source continues to accept appends.
    Chase,
    /// Freeze phase: source append authority is paused/frozen, remaining events transferred.
    Freeze,
    /// Cutover phase: generation records written and source append authority permanently retired.
    Completed,
}

/// Summary of a successfully completed migration and cutover per C5.63, C6.16, and C6.17.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutoverSummary {
    /// Total number of events transferred and retained in target.
    pub total_migrated_events: usize,
    /// Number of distinct fibers surviving into target.
    pub surviving_fibers: usize,
    /// Source locator identifier.
    pub source_locator_id: [u8; 16],
    /// Target locator identifier.
    pub target_locator_id: [u8; 16],
    /// Prior generation epoch from source.
    pub prior_generation_epoch: u64,
    /// Cutover epoch on target.
    pub cutover_epoch: u64,
}

/// Storage adapter capable of serving as a source for migration.
pub trait MigrationSource {
    /// Returns the 16-byte locator identifier for this source artefact.
    fn locator_id(&self) -> [u8; 16];
    /// Returns the current monotonic epoch for this source artefact.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if epoch metadata cannot be read.
    fn current_epoch(&self) -> Result<u64, OperationFailure>;
    /// Reads all event envelopes from this source artefact in dragline order.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails.
    fn read_envelopes(&self) -> Result<Vec<EventEnvelope>, OperationFailure>;
    /// Appends an outbound pointer record to this source metadata per C6.17 and C5.63.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    fn record_outbound_pointer(
        &self,
        pointer: &OutboundPointerRecord,
    ) -> Result<(), OperationFailure>;
    /// Appends an arbitrary ownership record to this source metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    fn record_meta(&self, record: &OwnershipRecord) -> Result<(), OperationFailure>;
    /// Reads all meta records from this source artefact.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails.
    fn read_meta(&self) -> Result<MetaRecords, OperationFailure>;
}

/// Storage adapter capable of serving as a target for migration.
pub trait MigrationTarget {
    /// Returns the 16-byte locator identifier for this target artefact.
    fn locator_id(&self) -> [u8; 16];
    /// Returns the current monotonic epoch for this target artefact.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if epoch metadata cannot be read.
    fn current_epoch(&self) -> Result<u64, OperationFailure>;
    /// Appends event envelopes to the target artefact in dragline order.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if appending fails.
    fn append_envelopes(&mut self, envelopes: &[EventEnvelope]) -> Result<(), OperationFailure>;
    /// Appends an inbound pointer record to this target metadata per C6.16.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    fn record_inbound_pointer(
        &self,
        pointer: &InboundPointerRecord,
    ) -> Result<(), OperationFailure>;
    /// Appends an arbitrary ownership record to this target metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    fn record_meta(&self, record: &OwnershipRecord) -> Result<(), OperationFailure>;
    /// Attaches a schema descriptor to this target artefact per C8.2.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if attaching fails.
    fn set_schema_descriptor(
        &mut self,
        descriptor: &SchemaDescriptor,
    ) -> Result<(), OperationFailure>;
}

impl MigrationSource for FileStorageAdapter {
    fn locator_id(&self) -> [u8; 16] {
        self.locator_id()
    }

    fn current_epoch(&self) -> Result<u64, OperationFailure> {
        self.current_epoch()
    }

    fn read_envelopes(&self) -> Result<Vec<EventEnvelope>, OperationFailure> {
        let mut reader = self.open_read()?;
        reader.read_all_envelopes_for_migration()
    }

    fn record_outbound_pointer(
        &self,
        pointer: &OutboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record_internal(&OwnershipRecord::OutboundPointer(pointer.clone()))
    }

    fn record_meta(&self, record: &OwnershipRecord) -> Result<(), OperationFailure> {
        self.record_meta_record_internal(record)
    }

    fn read_meta(&self) -> Result<MetaRecords, OperationFailure> {
        self.read_meta_records()
    }
}

impl MigrationTarget for FileStorageAdapter {
    fn locator_id(&self) -> [u8; 16] {
        self.locator_id()
    }

    fn current_epoch(&self) -> Result<u64, OperationFailure> {
        self.current_epoch()
    }

    fn append_envelopes(&mut self, envelopes: &[EventEnvelope]) -> Result<(), OperationFailure> {
        let epoch = self.current_epoch()?;
        let mut writer = self.open_write(epoch)?;
        for env in envelopes {
            let mut env_buf = Vec::new();
            env.encode(&mut env_buf);
            writer.append_unvalidated_frame(&env_buf)?;
        }
        writer.sync()
    }

    fn record_inbound_pointer(
        &self,
        pointer: &InboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_inbound_pointer(pointer)
    }

    fn record_meta(&self, record: &OwnershipRecord) -> Result<(), OperationFailure> {
        self.record_meta_record_internal(record)
    }

    fn set_schema_descriptor(
        &mut self,
        descriptor: &SchemaDescriptor,
    ) -> Result<(), OperationFailure> {
        let epoch = self.current_epoch()?;
        let mut writer = self.open_write(epoch)?;
        let admitted = AdmittedDescriptor::try_from_descriptor(descriptor.clone())?;
        writer.set_schema_descriptor(&admitted)
    }
}

/// Migration manager coordinating chase, freeze, transformation, and cutover per C5.63 and C6.16-20.
#[allow(dead_code)]
pub struct MigrationManager<S, T, F> {
    source: S,
    target: T,
    default_fiber_policy: FiberMigrationPolicy,
    fiber_policies: HashMap<[u8; 16], FiberMigrationPolicy>,
    rescue_policy: RescuePolicy,
    rescue_policy_parameters: Vec<u8>,
    broken_chain_election: BrokenChainElection,
    transformer: F,
    source_generation: u32,
    target_generation: u32,
    phase: MigrationPhase,
}

impl<S: fmt::Debug, T: fmt::Debug, F> fmt::Debug for MigrationManager<S, T, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MigrationManager")
            .field("source", &self.source)
            .field("target", &self.target)
            .field("phase", &self.phase)
            .finish()
    }
}

#[allow(dead_code)]
fn identity_transformer(payload: &[u8]) -> Result<Vec<u8>, OperationFailure> {
    Ok(payload.to_vec())
}

impl<S: MigrationSource, T: MigrationTarget>
    MigrationManager<S, T, fn(&[u8]) -> Result<Vec<u8>, OperationFailure>>
{
    /// Creates a new migration manager with default identity payload transformation.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// because live migration is disabled per approved constrained release decision.
    pub fn new(source: S, target: T) -> Result<Self, OperationFailure> {
        let _ = (source, target);
        Err(OperationFailure::new(
            FailureCondition::InvariantBreakingConfiguration,
            "live migration is disabled in this release per approved constrained release decision; use offline administrative migration",
        ))
    }
}

impl<S: MigrationSource, T: MigrationTarget, F> MigrationManager<S, T, F>
where
    F: FnMut(&[u8]) -> Result<Vec<u8>, OperationFailure>,
{
    /// Configures a custom payload transformation closure per C6.18.
    pub fn with_transformer<F2>(self, transformer: F2) -> MigrationManager<S, T, F2>
    where
        F2: FnMut(&[u8]) -> Result<Vec<u8>, OperationFailure>,
    {
        MigrationManager {
            source: self.source,
            target: self.target,
            default_fiber_policy: self.default_fiber_policy,
            fiber_policies: self.fiber_policies,
            rescue_policy: self.rescue_policy,
            rescue_policy_parameters: self.rescue_policy_parameters,
            broken_chain_election: self.broken_chain_election,
            transformer,
            source_generation: self.source_generation,
            target_generation: self.target_generation,
            phase: self.phase,
        }
    }

    /// Sets the default migration policy for fibers not explicitly configured.
    #[must_use]
    pub fn with_default_fiber_policy(mut self, policy: FiberMigrationPolicy) -> Self {
        self.default_fiber_policy = policy;
        self
    }

    /// Configures an explicit migration policy for a specific fiber per C12.2.
    #[must_use]
    pub fn with_fiber_policy(mut self, fiber_id: [u8; 16], policy: FiberMigrationPolicy) -> Self {
        self.fiber_policies.insert(fiber_id, policy);
        self
    }

    /// Sets the rescue policy choice and parameters per C4.13 and C6.19.
    #[must_use]
    pub fn with_rescue_policy(mut self, policy: RescuePolicy, parameters: Vec<u8>) -> Self {
        self.rescue_policy = policy;
        self.rescue_policy_parameters = parameters;
        self
    }

    /// Sets the broken-chain election per C5.28 and T3.
    #[must_use]
    pub fn with_broken_chain_election(mut self, election: BrokenChainElection) -> Self {
        self.broken_chain_election = election;
        self
    }

    /// Sets the source and target generation numbers.
    #[must_use]
    pub fn with_generations(mut self, source_generation: u32, target_generation: u32) -> Self {
        self.source_generation = source_generation;
        self.target_generation = target_generation;
        self
    }

    /// Returns the current lifecycle phase of the migration.
    #[must_use]
    pub fn phase(&self) -> MigrationPhase {
        self.phase
    }

    /// Performs the freeze phase: drains remainder of events from source while source takes no appends.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// because live migration is disabled per approved constrained release decision.
    pub fn freeze(&mut self) -> Result<usize, OperationFailure> {
        Err(OperationFailure::new(
            FailureCondition::InvariantBreakingConfiguration,
            "live migration is disabled in this release per approved constrained release decision; use offline administrative migration",
        ))
    }

    /// Performs cutover: final synchronization, generation records, and permanent source retirement per C5.63.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// because live migration is disabled per approved constrained release decision.
    pub fn cutover(mut self) -> Result<CutoverSummary, OperationFailure> {
        let _ = &mut self;
        Err(OperationFailure::new(
            FailureCondition::InvariantBreakingConfiguration,
            "live migration is disabled in this release per approved constrained release decision; use offline administrative migration",
        ))
    }

    /// Executes chase, freeze, and cutover end-to-end.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// because live migration is disabled per approved constrained release decision.
    pub fn run_all(mut self) -> Result<CutoverSummary, OperationFailure> {
        let _ = &mut self;
        Err(OperationFailure::new(
            FailureCondition::InvariantBreakingConfiguration,
            "live migration is disabled in this release per approved constrained release decision; use offline administrative migration",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_manager_stub_failures() {
        let mut mgr = MigrationManager {
            source: FileStorageAdapter::new(std::path::PathBuf::from("/tmp/test_src")),
            target: FileStorageAdapter::new(std::path::PathBuf::from("/tmp/test_dst")),
            default_fiber_policy: FiberMigrationPolicy::Keep,
            fiber_policies: HashMap::new(),
            rescue_policy: RescuePolicy::Strict,
            rescue_policy_parameters: Vec::new(),
            broken_chain_election: BrokenChainElection::RefuseOnBreak,
            transformer: identity_transformer,
            source_generation: 1,
            target_generation: 2,
            phase: MigrationPhase::Initial,
        };

        let err_freeze = mgr.freeze().unwrap_err();
        assert_eq!(
            *err_freeze.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );
        assert!(err_freeze.to_string().contains("live migration is disabled in this release per approved constrained release decision; use offline administrative migration"));

        let err_cutover = mgr.cutover().unwrap_err();
        assert_eq!(
            *err_cutover.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );
        assert!(err_cutover.to_string().contains("live migration is disabled in this release per approved constrained release decision; use offline administrative migration"));

        let mgr2 = MigrationManager {
            source: FileStorageAdapter::new(std::path::PathBuf::from("/tmp/test_src")),
            target: FileStorageAdapter::new(std::path::PathBuf::from("/tmp/test_dst")),
            default_fiber_policy: FiberMigrationPolicy::Keep,
            fiber_policies: HashMap::new(),
            rescue_policy: RescuePolicy::Strict,
            rescue_policy_parameters: Vec::new(),
            broken_chain_election: BrokenChainElection::RefuseOnBreak,
            transformer: identity_transformer,
            source_generation: 1,
            target_generation: 2,
            phase: MigrationPhase::Initial,
        };
        let err_run = mgr2.run_all().unwrap_err();
        assert_eq!(
            *err_run.condition(),
            FailureCondition::InvariantBreakingConfiguration
        );
        assert!(err_run.to_string().contains("live migration is disabled in this release per approved constrained release decision; use offline administrative migration"));
    }
}
