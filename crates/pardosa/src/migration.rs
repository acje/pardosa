//! Migration lifecycle, recovery, cutover, and dense re-chaining per C5.63, C6.16-20, and C12.2.

use crate::encoding::{EnvelopeHeader, EventEnvelope, OwnershipRecord};
pub use crate::encoding::{
    InboundPointerRecord, MigrationEndRecord, MigrationStartRecord, MigrationStatus,
    OutboundPointerRecord, RescuePolicy, RescuePolicyChoiceRecord,
};
use crate::file::{FileStorageAdapter, MetaRecords};
use crate::schema::SchemaDescriptor;
use crate::store::{CausalChainError, FailureCondition, FiberMigrationPolicy, OperationFailure};
use std::collections::HashMap;
use std::fmt;

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
        self.record_outbound_pointer(pointer)
    }

    fn record_meta(&self, record: &OwnershipRecord) -> Result<(), OperationFailure> {
        self.record_meta_record(record)
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
        self.record_meta_record(record)
    }

    fn set_schema_descriptor(
        &mut self,
        descriptor: &SchemaDescriptor,
    ) -> Result<(), OperationFailure> {
        let epoch = self.current_epoch()?;
        let mut writer = self.open_write(epoch)?;
        writer.set_schema_descriptor(descriptor)
    }
}

#[allow(dead_code)]
fn mint_fresh_identity(generation: u32, original_id: &[u8; 16], counter: u64) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"PARDOSA_GENERATION_BOUNDARY_MINT_V1");
    hasher.update(&generation.to_le_bytes());
    hasher.update(original_id);
    hasher.update(&counter.to_le_bytes());
    let hash = hasher.finalize();
    let mut id = [0u8; 16];
    id.copy_from_slice(&hash.as_bytes()[0..16]);
    id
}

/// Migration manager coordinating chase, freeze, transformation, and cutover per C5.63 and C6.16-20.
pub struct MigrationManager<S, T, F> {
    source: S,
    target: T,
    default_fiber_policy: FiberMigrationPolicy,
    fiber_policies: HashMap<[u8; 16], FiberMigrationPolicy>,
    rescue_policy: RescuePolicy,
    rescue_policy_parameters: Vec<u8>,
    broken_chain_election: BrokenChainElection,
    #[allow(dead_code)]
    transformer: F,
    source_generation: u32,
    target_generation: u32,
    phase: MigrationPhase,
    processed_source_count: usize,
    target_event_counter: u64,
    fiber_identity_map: HashMap<[u8; 16], [u8; 16]>,
    fiber_chain_state: HashMap<[u8; 16], ([u8; 16], [u8; 32])>,
    known_source_commitments: HashMap<[u8; 16], ([u8; 16], [u8; 32])>,
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
    /// because live migration is disabled per C5.18.
    pub fn new(source: S, target: T) -> Result<Self, OperationFailure> {
        let _ = (source, target);
        Err(OperationFailure::new(
            FailureCondition::InvariantBreakingConfiguration,
            "live migration is disabled in this release per C5.18; use offline administrative migration",
        ))
    }

    #[doc(hidden)]
    #[must_use]
    pub fn new_for_test(source: S, target: T) -> Self {
        Self {
            source,
            target,
            default_fiber_policy: FiberMigrationPolicy::Keep,
            fiber_policies: HashMap::new(),
            rescue_policy: RescuePolicy::Strict,
            rescue_policy_parameters: Vec::new(),
            broken_chain_election: BrokenChainElection::RefuseOnBreak,
            transformer: identity_transformer,
            source_generation: 1,
            target_generation: 2,
            phase: MigrationPhase::Initial,
            processed_source_count: 0,
            target_event_counter: 0,
            fiber_identity_map: HashMap::new(),
            fiber_chain_state: HashMap::new(),
            known_source_commitments: HashMap::new(),
        }
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
            processed_source_count: self.processed_source_count,
            target_event_counter: self.target_event_counter,
            fiber_identity_map: self.fiber_identity_map,
            fiber_chain_state: self.fiber_chain_state,
            known_source_commitments: self.known_source_commitments,
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

    #[allow(dead_code)]
    fn record_start_if_needed(&self) -> Result<(), OperationFailure> {
        let start_record = MigrationStartRecord {
            source_generation: self.source_generation,
            target_generation: self.target_generation,
            start_time_ns: 1_000_000_000,
            rescue_policy_tag: self.rescue_policy.to_u8(),
        };
        self.target
            .record_meta(&OwnershipRecord::MigrationStart(start_record.clone()))?;
        self.source
            .record_meta(&OwnershipRecord::MigrationStart(start_record))
    }

    #[allow(dead_code)]
    fn drain_batch(&mut self) -> Result<usize, OperationFailure> {
        let all_source_envelopes = self.source.read_envelopes()?;
        if all_source_envelopes.len() <= self.processed_source_count {
            return Ok(0);
        }

        let new_source_slice = &all_source_envelopes[self.processed_source_count..];

        let mut last_event_indices: HashMap<[u8; 16], usize> = HashMap::new();
        for (idx, env) in all_source_envelopes.iter().enumerate() {
            let fiber_id = env.header.fiber_id;
            let policy = self
                .fiber_policies
                .get(&fiber_id)
                .copied()
                .unwrap_or(self.default_fiber_policy);
            if policy == FiberMigrationPolicy::LockAndPrune {
                last_event_indices.insert(fiber_id, idx);
            }
        }

        let mut envelopes_to_migrate = Vec::new();
        let mut staged_known_source_commitments = self.known_source_commitments.clone();
        for (relative_idx, env) in new_source_slice.iter().enumerate() {
            let global_idx = self.processed_source_count + relative_idx;
            let fiber_id = env.header.fiber_id;
            let policy = self
                .fiber_policies
                .get(&fiber_id)
                .copied()
                .unwrap_or(self.default_fiber_policy);

            let comm = env.commitment();
            staged_known_source_commitments.insert(env.header.event_id, (fiber_id, comm));

            match policy {
                FiberMigrationPolicy::Purge => {}
                FiberMigrationPolicy::Keep => {
                    envelopes_to_migrate.push(env.clone());
                }
                FiberMigrationPolicy::LockAndPrune => {
                    if last_event_indices.get(&fiber_id) == Some(&global_idx) {
                        envelopes_to_migrate.push(env.clone());
                    }
                }
            }
        }

        let mut target_envelopes = Vec::with_capacity(envelopes_to_migrate.len());
        let mut staged_target_event_counter = self.target_event_counter;
        let mut staged_fiber_identity_map = self.fiber_identity_map.clone();
        let mut staged_fiber_chain_state = self.fiber_chain_state.clone();

        for env in envelopes_to_migrate {
            let old_event_id = env.header.event_id;
            let old_fiber_id = env.header.fiber_id;
            let policy = self
                .fiber_policies
                .get(&old_fiber_id)
                .copied()
                .unwrap_or(self.default_fiber_policy);

            staged_target_event_counter += 1;
            let target_fiber_id = *staged_fiber_identity_map
                .entry(old_fiber_id)
                .or_insert_with(|| mint_fresh_identity(self.target_generation, &old_fiber_id, 0));
            let target_event_id = mint_fresh_identity(
                self.target_generation,
                &old_event_id,
                staged_target_event_counter,
            );

            let new_payload = match (self.transformer)(&env.payload) {
                Ok(p) => p,
                Err(err) => {
                    return Err(OperationFailure::new(
                        FailureCondition::TransformationRefused,
                        format!("transformation refused: {err}"),
                    ));
                }
            };

            let is_source_genesis =
                env.header.precursor == [0u8; 16] && env.header.precursor_hash == [0u8; 32];

            let (precursor, precursor_hash) = match staged_fiber_chain_state.get(&old_fiber_id) {
                None => ([0u8; 16], [0u8; 32]),
                Some(&(prev_target_id, prev_target_comm)) => {
                    if is_source_genesis {
                        ([0u8; 16], [0u8; 32])
                    } else {
                        let source_precursor_id = env.header.precursor;
                        let predecessor_valid = match staged_known_source_commitments
                            .get(&source_precursor_id)
                        {
                            Some(&(pred_fiber, pred_comm)) => {
                                pred_fiber == old_fiber_id && pred_comm == env.header.precursor_hash
                            }
                            None => false,
                        };

                        if predecessor_valid {
                            (prev_target_id, prev_target_comm)
                        } else {
                            match self.broken_chain_election {
                                BrokenChainElection::RefuseOnBreak => {
                                    return Err(OperationFailure::new(
                                        FailureCondition::PrecursorChainBroken(Some(
                                            CausalChainError::PrecursorOutOfRange,
                                        )),
                                        "precursor chain broken in source; refused per C5.28",
                                    ));
                                }
                                BrokenChainElection::PermitBrokenHistory => ([0u8; 16], [0u8; 32]),
                            }
                        }
                    }
                }
            };

            let detached = if policy == FiberMigrationPolicy::LockAndPrune {
                true
            } else {
                env.header.detached
            };

            let new_envelope = EventEnvelope {
                header: EnvelopeHeader {
                    event_id: target_event_id,
                    fiber_id: target_fiber_id,
                    detached,
                    precursor,
                    precursor_hash,
                },
                payload: new_payload,
            };

            let new_comm = new_envelope.commitment();
            staged_fiber_chain_state.insert(old_fiber_id, (target_event_id, new_comm));
            target_envelopes.push(new_envelope);
        }

        let migrated_count = target_envelopes.len();
        if !target_envelopes.is_empty() {
            self.target.append_envelopes(&target_envelopes)?;
        }
        self.target_event_counter = staged_target_event_counter;
        self.fiber_identity_map = staged_fiber_identity_map;
        self.fiber_chain_state = staged_fiber_chain_state;
        self.known_source_commitments = staged_known_source_commitments;
        self.processed_source_count = all_source_envelopes.len();

        Ok(migrated_count)
    }

    /// Performs the chase phase: ingests events currently available in source while source appends.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// because live migration is disabled per C5.18.
    pub fn chase(&mut self) -> Result<usize, OperationFailure> {
        Err(OperationFailure::new(
            FailureCondition::InvariantBreakingConfiguration,
            "live migration is disabled in this release per C5.18; use offline administrative migration",
        ))
    }

    /// Performs the freeze phase: drains remainder of events from source while source takes no appends.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// because live migration is disabled per C5.18.
    pub fn freeze(&mut self) -> Result<usize, OperationFailure> {
        Err(OperationFailure::new(
            FailureCondition::InvariantBreakingConfiguration,
            "live migration is disabled in this release per C5.18; use offline administrative migration",
        ))
    }

    /// Performs cutover: final synchronization, generation records, and permanent source retirement per C5.63.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// because live migration is disabled per C5.18.
    pub fn cutover(mut self) -> Result<CutoverSummary, OperationFailure> {
        let _ = &mut self;
        Err(OperationFailure::new(
            FailureCondition::InvariantBreakingConfiguration,
            "live migration is disabled in this release per C5.18; use offline administrative migration",
        ))
    }

    /// Executes chase, freeze, and cutover end-to-end.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::InvariantBreakingConfiguration`]
    /// because live migration is disabled per C5.18.
    pub fn run_all(mut self) -> Result<CutoverSummary, OperationFailure> {
        let _ = &mut self;
        Err(OperationFailure::new(
            FailureCondition::InvariantBreakingConfiguration,
            "live migration is disabled in this release per C5.18; use offline administrative migration",
        ))
    }
}
