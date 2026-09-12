use pardosa::prelude::*;
use std::process::Command;

#[test]
fn test_c6_1_fiber_lifecycle_transitions() {
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

    let all_states = [
        FiberState::Undefined,
        FiberState::Defined,
        FiberState::Detached,
        FiberState::Purged,
        FiberState::Locked,
    ];

    let mut valid = 0;
    let mut invalid = 0;
    for state in &all_states {
        let results = [
            state.create(),
            state.update(),
            state.detach(),
            state.rescue(),
            state.migrate(FiberMigrationPolicy::Keep),
            state.migrate(FiberMigrationPolicy::LockAndPrune),
            state.migrate(FiberMigrationPolicy::Purge),
        ];
        for res in results {
            match res {
                Ok(_) => valid += 1,
                Err(_) => invalid += 1,
            }
        }
    }
    assert_eq!(valid, 10);
    assert_eq!(invalid, 25);
}

#[test]
fn test_c6_2_reopened_store_limits() {
    assert!(FiberState::Undefined.is_reopened_valid());
    assert!(FiberState::Defined.is_reopened_valid());
    assert!(FiberState::Detached.is_reopened_valid());
    assert!(FiberState::Purged.is_reopened_valid());
    assert!(!FiberState::Locked.is_reopened_valid());

    let fiber_1 = Uuid::from_bytes([1u8; 16]);
    let fiber_2 = Uuid::from_bytes([2u8; 16]);

    let valid_states = vec![
        (fiber_1, FiberState::Defined),
        (fiber_2, FiberState::Detached),
    ];
    let boundary = ReopenedStoreBoundary::validate(&valid_states, MigrationMode::Steady, &[]);
    assert!(boundary.is_ok());
    let b = boundary.unwrap();
    assert_eq!(b.mode(), MigrationMode::Steady);
    assert_eq!(b.fiber_states().len(), 2);
    assert_eq!(b.fiber_states()[0].1, ReopenedFiberState::Defined);
    assert_eq!(b.fiber_states()[0].1.to_fiber_state(), FiberState::Defined);

    let locked_states = vec![(fiber_1, FiberState::Locked)];
    assert_eq!(
        ReopenedStoreBoundary::validate(&locked_states, MigrationMode::Steady, &[]),
        Err(IllegalOpenCombination::LockedStateOnReopen)
    );

    assert_eq!(
        ReopenedStoreBoundary::validate(&valid_states, MigrationMode::Migrating, &[]),
        Err(IllegalOpenCombination::MigratingModeOnReopen)
    );

    assert_eq!(
        ReopenedStoreBoundary::validate(&valid_states, MigrationMode::Steady, &[fiber_1]),
        Err(IllegalOpenCombination::RemovedIdentitiesOnReopen)
    );
}

#[test]
fn test_c5_22_dragline_local_cursor_and_regeneration() {
    let genesis_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x01; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: vec![],
    };

    let mut genesis_buf = Vec::with_capacity(81 + genesis_env.payload.len());
    genesis_env.header.encode(&mut genesis_buf);
    genesis_buf.extend_from_slice(&genesis_env.payload);
    let genesis_hash = *blake3::hash(&genesis_buf).as_bytes();

    let event_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: [0x01; 16],
            precursor_hash: genesis_hash,
        },
        payload: vec![],
    };

    ArtefactReader::with_stream(
        "dragline-alpha",
        vec![genesis_env, event_env],
        |alpha_reader| {
            let genesis_obs = alpha_reader
                .read_event()
                .expect("genesis stream item")
                .expect("genesis observation");
            let genesis_cursor = genesis_obs.cursor();
            assert_eq!(genesis_cursor.position(), 0);
            assert_eq!(genesis_cursor.event_id(), &[0x01; 16]);
            assert_eq!(alpha_reader.resume_from(&genesis_cursor), 0);

            let event_obs = alpha_reader
                .read_event()
                .expect("event stream item")
                .expect("event observation");
            let event_cursor = event_obs.cursor();
            assert_eq!(event_cursor.position(), 1);
            assert_eq!(event_cursor.event_id(), &[0x02; 16]);
            assert_eq!(alpha_reader.resume_from(&event_cursor), 1);

            let target_env = EventEnvelope {
                header: EnvelopeHeader {
                    event_id: [0x03; 16],
                    fiber_id: [0xbb; 16],
                    detached: false,
                    precursor: [0u8; 16],
                    precursor_hash: [0u8; 32],
                },
                payload: vec![],
            };

            ArtefactReader::with_stream("dragline-beta", vec![target_env], |beta_reader| {
                let target_obs = beta_reader
                    .read_event()
                    .expect("target stream item")
                    .expect("target observation");
                let target_cursor = beta_reader
                    .regenerate_cursor_for_migration(&event_cursor, &target_obs)
                    .expect("regenerate cursor");
                assert_eq!(target_cursor.position(), 0);
                assert_eq!(target_cursor.event_id(), &[0x03; 16]);
                assert_eq!(beta_reader.resume_from(&target_cursor), 0);
            });

            assert!(alpha_reader
                .regenerate_cursor_for_migration(&event_cursor, &genesis_obs)
                .is_none());
        },
    );
}

#[test]
fn test_c6_7_operation_failure_taxonomy_exhaustive() {
    fn check_condition_exhaustive(condition: &FailureCondition) {
        match condition {
            FailureCondition::StoreAlreadyExists => {}
            FailureCondition::NoArtefactExists => {}
            FailureCondition::ConcurrencyConflict => {}
            FailureCondition::StaleEpoch => {}
            FailureCondition::OwnershipUnestablished => {}
            FailureCondition::OwnershipRecordUnreadable => {}
            FailureCondition::ExclusionUnavailable => {}
            FailureCondition::AnotherOwnerHoldsExclusion => {}
            FailureCondition::MigrationExclusionAbsent => {}
            FailureCondition::MigrationAlreadyRunning => {}
            FailureCondition::PrecursorChainBroken(_) => {}
            FailureCondition::UncoveredPartitionMembership => {}
            FailureCondition::SchemaMismatch => {}
            FailureCondition::EnvelopeMismatch => {}
            FailureCondition::MismatchUndeterminedSubject => {}
            FailureCondition::ArtefactMismatch => {}
            FailureCondition::ValueConstraintViolated { .. } => {}
            FailureCondition::UnrecognisedWireTag { .. } => {}
            FailureCondition::InvariantBreakingConfiguration => {}
            FailureCondition::MissingSchemaDescriptor => {}
            FailureCondition::TransformationRefused => {}
            FailureCondition::RetiredMigrationSource => {}
            FailureCondition::TransportUnavailable => {}
        }
    }

    let conditions = [
        FailureCondition::StoreAlreadyExists,
        FailureCondition::NoArtefactExists,
        FailureCondition::ConcurrencyConflict,
        FailureCondition::StaleEpoch,
        FailureCondition::OwnershipUnestablished,
        FailureCondition::OwnershipRecordUnreadable,
        FailureCondition::ExclusionUnavailable,
        FailureCondition::AnotherOwnerHoldsExclusion,
        FailureCondition::MigrationExclusionAbsent,
        FailureCondition::MigrationAlreadyRunning,
        FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorOutOfRange)),
        FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorWrongFiber)),
        FailureCondition::PrecursorChainBroken(None),
        FailureCondition::UncoveredPartitionMembership,
        FailureCondition::SchemaMismatch,
        FailureCondition::EnvelopeMismatch,
        FailureCondition::MismatchUndeterminedSubject,
        FailureCondition::ArtefactMismatch,
        FailureCondition::ValueConstraintViolated {
            constraint: ValueConstraint::TooLong,
        },
        FailureCondition::UnrecognisedWireTag { tag: 0xee },
        FailureCondition::InvariantBreakingConfiguration,
        FailureCondition::MissingSchemaDescriptor,
        FailureCondition::TransformationRefused,
        FailureCondition::RetiredMigrationSource,
        FailureCondition::TransportUnavailable,
    ];

    for condition in &conditions {
        check_condition_exhaustive(condition);
        let remedy = condition.remedy();
        assert!(!remedy.is_empty());
        let failure = OperationFailure::new(condition.clone(), "diagnostic message");
        assert_eq!(failure.condition(), condition);
        assert_eq!(failure.diagnostic_detail().message(), "diagnostic message");
        let formatted = format!("{failure}");
        assert!(!formatted.is_empty());
    }

    let broken_chain = OperationFailure::new(
        FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorWrongFiber)),
        "fiber link broken",
    );
    use std::error::Error;
    assert!(broken_chain.source().is_some());

    let broken_chain_none = OperationFailure::new(
        FailureCondition::PrecursorChainBroken(None),
        "precursor commitment broken",
    );
    assert!(broken_chain_none.source().is_none());
}

#[test]
fn test_c6_8_and_c6_15_qualified_open_results() {
    let orphan = QualifiedOpenResult::orphan_read();
    assert_eq!(*orphan.ownership(), OwnershipStatus::Unowned);
    assert_eq!(orphan.generation(), GenerationKnowledge::Unknown);
    assert_eq!(orphan.supersession(), SupersessionStatus::Unknown);
    assert_eq!(orphan.migration_disagreement(), MigrationDisagreement::None);
    assert_eq!(orphan.history_integrity(), HistoryIntegrity::Unanchored);
    assert_eq!(
        orphan.migration_completeness(),
        MigrationCompleteness::Unknown
    );
    assert_eq!(orphan.append_authority(), AppendAuthority::Unknown);

    let coexisting = QualifiedOpenResult::builder()
        .ownership(OwnershipStatus::Owned { epoch: 100 })
        .generation(GenerationKnowledge::Known(1))
        .supersession(SupersessionStatus::Superseded {
            next_generation: Some(2),
        })
        .migration_disagreement(MigrationDisagreement::InboundWithoutOutbound)
        .history_integrity(HistoryIntegrity::Anchored)
        .migration_completeness(MigrationCompleteness::KnownComplete)
        .append_authority(AppendAuthority::HeldByGeneration(2))
        .build();
    assert!(coexisting.is_ok());
    let res = coexisting.unwrap();
    assert_eq!(
        res.supersession(),
        SupersessionStatus::Superseded {
            next_generation: Some(2)
        }
    );
    assert_eq!(
        res.migration_disagreement(),
        MigrationDisagreement::InboundWithoutOutbound
    );

    let partial_target = QualifiedOpenResult::builder()
        .ownership(OwnershipStatus::Owned { epoch: 50 })
        .generation(GenerationKnowledge::Known(2))
        .supersession(SupersessionStatus::Current)
        .migration_disagreement(MigrationDisagreement::None)
        .history_integrity(HistoryIntegrity::Anchored)
        .migration_completeness(MigrationCompleteness::Unknown)
        .append_authority(AppendAuthority::Unknown)
        .build();
    assert!(partial_target.is_ok());

    let unowned_known_gen = QualifiedOpenResult::builder()
        .ownership(OwnershipStatus::Unowned)
        .generation(GenerationKnowledge::Known(1))
        .build();
    assert_eq!(
        unowned_known_gen,
        Err(IllegalOpenCombination::UnownedMustHaveUnknownGeneration)
    );

    let unknown_gen_current = QualifiedOpenResult::builder()
        .ownership(OwnershipStatus::Owned { epoch: 1 })
        .generation(GenerationKnowledge::Unknown)
        .supersession(SupersessionStatus::Current)
        .build();
    assert_eq!(
        unknown_gen_current,
        Err(IllegalOpenCombination::UnknownGenerationCannotBeCurrent)
    );

    let unowned_authority = QualifiedOpenResult::builder()
        .ownership(OwnershipStatus::Unowned)
        .generation(GenerationKnowledge::Unknown)
        .append_authority(AppendAuthority::HoldsAuthority)
        .build();
    assert_eq!(
        unowned_authority,
        Err(IllegalOpenCombination::UnownedCannotHoldAppendAuthority)
    );
}

#[test]
fn test_c5_5_through_16_ownership_and_domain_admissions() {
    let claim = OwnershipClaimRecord {
        epoch: 42,
        machine_id: [1u8; 16],
        boot_id: [2u8; 16],
        process_id: 1234,
        process_start_time_ns: 5000,
        claim_time_ns: 6000,
        operator_label: "test-operator".to_string(),
    };

    assert!(OwnershipFence::fence_write(42, &RecordedOwnership::Claimed(claim.clone())).is_ok());
    let stale = OwnershipFence::fence_write(41, &RecordedOwnership::Claimed(claim.clone()));
    assert_eq!(
        stale.unwrap_err().condition(),
        &FailureCondition::StaleEpoch
    );

    let unreadable = OwnershipFence::fence_write(42, &RecordedOwnership::Unreadable);
    assert_eq!(
        unreadable.unwrap_err().condition(),
        &FailureCondition::OwnershipRecordUnreadable
    );

    let absent = OwnershipFence::fence_write(42, &RecordedOwnership::Absent);
    assert_eq!(
        absent.unwrap_err().condition(),
        &FailureCondition::OwnershipUnestablished
    );

    let unowned = OwnershipFence::fence_write(42, &RecordedOwnership::Unowned);
    assert_eq!(
        unowned.unwrap_err().condition(),
        &FailureCondition::OwnershipUnestablished
    );

    assert_eq!(
        evaluate_owner_liveness(Some(DeathProof::ProcessAbsence)),
        LivenessVerdict::ProvenDead {
            proof: DeathProof::ProcessAbsence
        }
    );
    assert_eq!(
        evaluate_owner_liveness(None),
        LivenessVerdict::Indeterminate
    );

    let clean = CleanReleaseProof {
        epoch: 42,
        release_time_ns: 7000,
    };
    assert_eq!(clean.epoch, 42);
    assert_eq!(clean.release_time_ns, 7000);

    let landing = WriteLandingVerdict::<()>::Undetermined { carried_epoch: 42 };
    match landing {
        WriteLandingVerdict::Undetermined { carried_epoch } => assert_eq!(carried_epoch, 42),
        WriteLandingVerdict::Landed(()) => panic!("expected undetermined"),
    }

    assert!(admit_create(ArtefactPresence::None).is_ok());
    assert_eq!(
        admit_create(ArtefactPresence::Both)
            .unwrap_err()
            .condition(),
        &FailureCondition::StoreAlreadyExists
    );
    assert_eq!(
        admit_create(ArtefactPresence::OwnershipRecordOnly)
            .unwrap_err()
            .condition(),
        &FailureCondition::StoreAlreadyExists
    );

    assert_eq!(
        admit_open(ArtefactPresence::None, None, false)
            .unwrap_err()
            .condition(),
        &FailureCondition::NoArtefactExists
    );
    assert_eq!(
        admit_open(ArtefactPresence::Both, None, true).unwrap(),
        OpenAdmission::Ready
    );
    assert_eq!(
        admit_open(ArtefactPresence::OwnershipRecordOnly, None, true).unwrap(),
        OpenAdmission::IncompleteCreation(IncompleteCreationState::Unseeded)
    );
    assert_eq!(
        admit_open(ArtefactPresence::EventDataOnly, None, false).unwrap(),
        OpenAdmission::ReadOnlyOrphan
    );
    assert_eq!(
        admit_open(ArtefactPresence::EventDataOnly, None, true)
            .unwrap_err()
            .condition(),
        &FailureCondition::OwnershipUnestablished
    );

    let candidate = OwnershipClaimRecord {
        epoch: 43,
        machine_id: [1u8; 16],
        boot_id: [2u8; 16],
        process_id: 1234,
        process_start_time_ns: 5000,
        claim_time_ns: 7000,
        operator_label: "claimant".to_string(),
    };
    assert_eq!(
        evaluate_claim_cas(
            &RecordedOwnership::Claimed(claim.clone()),
            Some(42),
            &candidate
        )
        .unwrap(),
        43
    );
    assert_eq!(
        evaluate_claim_cas(
            &RecordedOwnership::Claimed(claim.clone()),
            Some(41),
            &candidate
        )
        .unwrap_err()
        .condition(),
        &FailureCondition::ConcurrencyConflict
    );
    assert_eq!(
        evaluate_claim_cas(&RecordedOwnership::Unowned, None, &candidate).unwrap(),
        43
    );

    assert!(validate_name_pairing("dataset_1", "dataset_1").is_ok());
    let mismatch = validate_name_pairing("dataset_1", "dataset_2");
    assert_eq!(
        mismatch.unwrap_err().condition(),
        &FailureCondition::ArtefactMismatch
    );

    let clean_proof = TakeoverProof::CleanRelease(clean);
    assert_eq!(
        evaluate_takeover(&claim, Some(&clean_proof)),
        TakeoverVerdict::Proven
    );
    let stale_clean = TakeoverProof::CleanRelease(CleanReleaseProof {
        epoch: 40,
        release_time_ns: 1000,
    });
    assert_eq!(
        evaluate_takeover(&claim, Some(&stale_clean)),
        TakeoverVerdict::Indeterminate
    );
    let death_proof = TakeoverProof::Death(DeathProof::MachineReboot);
    assert_eq!(
        evaluate_takeover(&claim, Some(&death_proof)),
        TakeoverVerdict::Proven
    );
    assert_eq!(
        evaluate_takeover(&claim, None),
        TakeoverVerdict::Indeterminate
    );

    let descriptor = SchemaDescriptor::new(1, DescriptorNode::U8);
    let schema_id = SchemaIdentity::from_raw([3u8; 32]);
    let wrong_schema_id = SchemaIdentity::from_raw([4u8; 32]);
    let env_id = EnvelopeIdentity::from_raw([5u8; 32]);
    let wrong_env_id = EnvelopeIdentity::from_raw([6u8; 32]);

    assert_eq!(
        admit_event(&EventAdmission::Event {
            descriptor: None,
            expected_schema: &schema_id,
            actual_schema: &schema_id,
            expected_envelope: &env_id,
            actual_envelope: &env_id,
        })
        .unwrap_err()
        .condition(),
        &FailureCondition::MissingSchemaDescriptor
    );
    assert_eq!(
        admit_event(&EventAdmission::Event {
            descriptor: Some(&descriptor),
            expected_schema: &schema_id,
            actual_schema: &wrong_schema_id,
            expected_envelope: &env_id,
            actual_envelope: &env_id,
        })
        .unwrap_err()
        .condition(),
        &FailureCondition::SchemaMismatch
    );
    assert_eq!(
        admit_event(&EventAdmission::Event {
            descriptor: Some(&descriptor),
            expected_schema: &schema_id,
            actual_schema: &schema_id,
            expected_envelope: &env_id,
            actual_envelope: &wrong_env_id,
        })
        .unwrap_err()
        .condition(),
        &FailureCondition::EnvelopeMismatch
    );
    assert!(admit_event(&EventAdmission::Event {
        descriptor: Some(&descriptor),
        expected_schema: &schema_id,
        actual_schema: &schema_id,
        expected_envelope: &env_id,
        actual_envelope: &env_id,
    })
    .is_ok());

    let fiber_a = [7u8; 16];
    let fiber_b = [8u8; 16];

    let genesis_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [10u8; 16],
            fiber_id: fiber_a,
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: vec![],
    };
    let genesis_link = PrecursorLink::classify(&genesis_env).expect("genesis link");
    assert!(genesis_link.is_genesis());
    assert_eq!(genesis_link.envelope(), &genesis_env);
    assert!(admit_precursor_link(&fiber_a, &genesis_link, |_| None).is_ok());

    let mut genesis_buf = Vec::with_capacity(81 + genesis_env.payload.len());
    genesis_env.header.encode(&mut genesis_buf);
    genesis_buf.extend_from_slice(&genesis_env.payload);
    let genesis_commitment = *blake3::hash(&genesis_buf).as_bytes();

    let pred_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [20u8; 16],
            fiber_id: fiber_a,
            detached: false,
            precursor: [10u8; 16],
            precursor_hash: genesis_commitment,
        },
        payload: vec![],
    };
    let pred_link = PrecursorLink::classify(&pred_env).expect("pred link");
    assert!(!pred_link.is_genesis());
    assert_eq!(pred_link.envelope(), &pred_env);

    assert!(admit_precursor_link(&fiber_a, &pred_link, |id| {
        if id == &[10u8; 16] {
            Some(&genesis_env)
        } else {
            None
        }
    })
    .is_ok());

    let wrong_id_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [99u8; 16],
            fiber_id: fiber_a,
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: vec![],
    };
    assert_eq!(
        admit_precursor_link(&fiber_a, &pred_link, |_id| { Some(&wrong_id_env) })
            .unwrap_err()
            .condition(),
        &FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorOutOfRange))
    );

    assert_eq!(
        admit_precursor_link(&fiber_b, &pred_link, |id| {
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
        admit_precursor_link(&fiber_a, &pred_link, |_| None)
            .unwrap_err()
            .condition(),
        &FailureCondition::PrecursorChainBroken(Some(CausalChainError::PrecursorOutOfRange))
    );

    let tampered_genesis = EventEnvelope {
        header: genesis_env.header.clone(),
        payload: vec![99, 98, 97],
    };
    assert_eq!(
        admit_precursor_link(&fiber_a, &pred_link, |_| Some(&tampered_genesis))
            .unwrap_err()
            .condition(),
        &FailureCondition::PrecursorChainBroken(None)
    );

    let bad_hash_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [20u8; 16],
            fiber_id: fiber_a,
            detached: false,
            precursor: [10u8; 16],
            precursor_hash: [0xff; 32],
        },
        payload: vec![],
    };
    let bad_hash_link = PrecursorLink::classify(&bad_hash_env).expect("bad hash link");
    assert_eq!(
        admit_precursor_link(&fiber_a, &bad_hash_link, |_| Some(&genesis_env))
            .unwrap_err()
            .condition(),
        &FailureCondition::PrecursorChainBroken(None)
    );

    let malformed_genesis = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [10u8; 16],
            fiber_id: fiber_a,
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [1u8; 32],
        },
        payload: vec![],
    };
    assert_eq!(
        PrecursorLink::classify(&malformed_genesis)
            .unwrap_err()
            .condition(),
        &FailureCondition::PrecursorChainBroken(None)
    );
}

fn run_rustc(code: &str) -> (bool, String) {
    use std::io::Write;

    let deps_dir = std::env::current_exe()
        .expect("current test executable")
        .parent()
        .expect("parent deps directory")
        .to_path_buf();

    let entries = std::fs::read_dir(&deps_dir)
        .unwrap_or_else(|e| panic!("failed to read deps dir {}: {e}", deps_dir.display()));
    let mut rlibs = Vec::new();
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|e| panic!("failed to read entry in {}: {e}", deps_dir.display()));
        let p = entry.path();
        if let Some(s) = p.file_name().and_then(|n| n.to_str()) {
            if s.starts_with("libpardosa-") && s.ends_with(".rlib") {
                rlibs.push(p.clone());
            }
        }
    }
    let rlib = match rlibs.len() {
        0 => panic!(
            "could not locate libpardosa rlib in deps directory: {}",
            deps_dir.display()
        ),
        1 => rlibs.remove(0),
        _ => {
            rlibs.sort_by_key(|p| {
                std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            });
            rlibs.pop().unwrap()
        }
    };

    let rustc_cmd = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let mut cmd = Command::new(&rustc_cmd);
    cmd.arg("--edition")
        .arg("2021")
        .arg("-L")
        .arg(&deps_dir)
        .arg("--extern")
        .arg(format!("pardosa={}", rlib.display()));
    let mut child = cmd
        .arg("--crate-type")
        .arg("lib")
        .arg("--emit")
        .arg("mir=-")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn rustc");

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(code.as_bytes())
            .expect("write code to rustc stdin");
    }
    let output = child.wait_with_output().expect("wait rustc");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (output.status.success(), stderr)
}

#[test]
fn test_compile_fail_and_positive_controls_external_boundary() {
    let positive_code = r#"
        use pardosa::prelude::*;
        pub fn run_test() {
            let env = EventEnvelope {
                header: EnvelopeHeader {
                    event_id: [1; 16],
                    fiber_id: [2; 16],
                    detached: false,
                    precursor: [0; 16],
                    precursor_hash: [0; 32],
                },
                payload: vec![],
            };
            ArtefactReader::with_stream("locator", vec![env.clone()], |reader| {
                let obs = reader.read_event().unwrap().unwrap();
                let cursor = obs.cursor();
                let _pos = reader.resume_from(&cursor);
            });
            let _link = PrecursorLink::classify(&env).unwrap();
            let _res = QualifiedOpenResult::builder()
                .ownership(OwnershipStatus::Unowned)
                .generation(GenerationKnowledge::Unknown)
                .build();
        }
    "#;
    let (pos_ok, pos_stderr) = run_rustc(positive_code);
    assert!(
        pos_ok,
        "positive control must compile cleanly:\n{pos_stderr}"
    );

    let cross_dragline_mixing_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid() {
            let env = EventEnvelope {
                header: EnvelopeHeader {
                    event_id: [1; 16],
                    fiber_id: [2; 16],
                    detached: false,
                    precursor: [0; 16],
                    precursor_hash: [0; 32],
                },
                payload: vec![],
            };
            ArtefactReader::with_stream("alpha", vec![env.clone()], |reader_a| {
                ArtefactReader::with_stream("beta", vec![env.clone()], |reader_b| {
                    let obs_a = reader_a.read_event().unwrap().unwrap();
                    let cursor_a = obs_a.cursor();
                    reader_b.resume_from(&cursor_a);
                });
            });
        }
    "#;
    let (mix_ok, mix_stderr) = run_rustc(cross_dragline_mixing_code);
    assert!(
        !mix_ok,
        "cross-dragline cursor mixing must fail compilation (H1)"
    );
    assert!(
        mix_stderr.contains("lifetime")
            || mix_stderr.contains("E0308")
            || mix_stderr.contains("E0521")
            || mix_stderr.contains("escapes"),
        "rejection must be lifetime/type mismatch: {mix_stderr}"
    );

    let direct_cursor_construction_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid() {
            let _cursor = ResumeCursor {
                position: 0,
                event_id: [0; 16],
                locator: ArtefactLocator::new("alpha"),
                _brand: std::marker::PhantomData,
            };
        }
    "#;
    let (cursor_ok, cursor_stderr) = run_rustc(direct_cursor_construction_code);
    assert!(
        !cursor_ok,
        "direct cursor construction must fail compilation (H1)"
    );
    assert!(
        cursor_stderr.contains("private"),
        "rejection must be private fields: {cursor_stderr}"
    );

    let direct_observation_construction_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid() {
            let _obs = ReaderObservation {
                position: 0,
                event_id: [0; 16],
                fiber_id: [0; 16],
                locator: ArtefactLocator::new("alpha"),
                is_genesis: true,
                _brand: std::marker::PhantomData,
            };
        }
    "#;
    let (obs_ok, obs_stderr) = run_rustc(direct_observation_construction_code);
    assert!(
        !obs_ok,
        "direct observation construction must fail compilation (H1)"
    );
    assert!(
        obs_stderr.contains("private"),
        "rejection must be private fields: {obs_stderr}"
    );

    let clone_reader_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid() {
            ArtefactReader::with_reader("locator", |reader| {
                let _cloned = reader.clone();
            });
        }
    "#;
    let (clone_ok, clone_stderr) = run_rustc(clone_reader_code);
    assert!(
        !clone_ok,
        "cloning ArtefactReader must fail compilation (H1)"
    );
    assert!(
        clone_stderr.contains("clone"),
        "rejection must be missing Clone: {clone_stderr}"
    );

    let direct_precursor_link_construction_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid() {
            let env = EventEnvelope {
                header: EnvelopeHeader {
                    event_id: [1; 16],
                    fiber_id: [2; 16],
                    detached: false,
                    precursor: [0; 16],
                    precursor_hash: [0; 32],
                },
                payload: vec![],
            };
            let _link = PrecursorLink {
                envelope: &env,
                is_genesis: true,
            };
        }
    "#;
    let (link_ok, link_stderr) = run_rustc(direct_precursor_link_construction_code);
    assert!(
        !link_ok,
        "direct PrecursorLink construction must fail compilation (H2)"
    );
    assert!(
        link_stderr.contains("private"),
        "rejection must be private fields: {link_stderr}"
    );

    let call_observe_genesis_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid() {
            let env = EventEnvelope {
                header: EnvelopeHeader {
                    event_id: [1; 16],
                    fiber_id: [2; 16],
                    detached: false,
                    precursor: [0; 16],
                    precursor_hash: [0; 32],
                },
                payload: vec![],
            };
            ArtefactReader::with_reader("locator", |reader| {
                let _ = reader.observe_genesis(&env);
            });
        }
    "#;
    let (gen_ok, gen_stderr) = run_rustc(call_observe_genesis_code);
    assert!(
        !gen_ok,
        "calling observe_genesis directly must fail compilation (H1/L1)"
    );
    assert!(
        gen_stderr.contains("private"),
        "rejection must cite private method: {gen_stderr}"
    );

    let cross_reader_regeneration_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid() {
            let env = EventEnvelope {
                header: EnvelopeHeader {
                    event_id: [1; 16],
                    fiber_id: [2; 16],
                    detached: false,
                    precursor: [0; 16],
                    precursor_hash: [0; 32],
                },
                payload: vec![],
            };
            ArtefactReader::with_stream("alpha", vec![env.clone()], |reader_a| {
                ArtefactReader::with_stream("beta", vec![env.clone()], |reader_b| {
                    let obs_a = reader_a.read_event().unwrap().unwrap();
                    let cursor_a = obs_a.cursor();
                    let obs_b = reader_b.read_event().unwrap().unwrap();
                    reader_a.regenerate_cursor_for_migration(&cursor_a, &obs_b);
                });
            });
        }
    "#;
    let (cross_reg_ok, cross_reg_stderr) = run_rustc(cross_reader_regeneration_code);
    assert!(
        !cross_reg_ok,
        "regenerating with foreign observation brand must fail compilation (H1)"
    );
    assert!(
        cross_reg_stderr.contains("lifetime")
            || cross_reg_stderr.contains("E0308")
            || cross_reg_stderr.contains("E0521"),
        "rejection must be lifetime/type mismatch: {cross_reg_stderr}"
    );

    let contradictory_open_admission_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid() {
            let _adm = OpenAdmission::IncompleteCreation(CreationProgression::Complete);
        }
    "#;
    let (adm_ok, adm_stderr) = run_rustc(contradictory_open_admission_code);
    assert!(
        !adm_ok,
        "constructing IncompleteCreation with Complete must fail compilation (M1)"
    );
    assert!(
        adm_stderr.contains("E0308") || adm_stderr.contains("mismatched types"),
        "rejection must be type mismatch: {adm_stderr}"
    );

    let direct_qualified_mutation_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid() {
            let mut res = QualifiedOpenResult::orphan_read();
            res.ownership = OwnershipStatus::Owned { epoch: 10 };
        }
    "#;
    let (mut_ok, mut_stderr) = run_rustc(direct_qualified_mutation_code);
    assert!(
        !mut_ok,
        "direct QualifiedOpenResult mutation must fail compilation (H2)"
    );
    assert!(
        mut_stderr.contains("private"),
        "rejection must be private fields: {mut_stderr}"
    );

    let direct_qualified_literal_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid() {
            let _res = QualifiedOpenResult {
                ownership: OwnershipStatus::Unowned,
                generation: GenerationKnowledge::Unknown,
                supersession: SupersessionStatus::Unknown,
                migration_disagreement: MigrationDisagreement::None,
                history_integrity: HistoryIntegrity::Unanchored,
                migration_completeness: MigrationCompleteness::Unknown,
                append_authority: AppendAuthority::Unknown,
            };
        }
    "#;
    let (lit_ok, lit_stderr) = run_rustc(direct_qualified_literal_code);
    assert!(
        !lit_ok,
        "direct QualifiedOpenResult literal construction must fail compilation (M3/H2)"
    );
    assert!(
        lit_stderr.contains("private"),
        "rejection must be private fields: {lit_stderr}"
    );

    let file_reader_append_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid(mut reader: FileReaderSession) {
            let _ = reader.append_frame(b"illegal-append-on-reader");
        }
    "#;
    let (reader_append_ok, reader_append_stderr) = run_rustc(file_reader_append_code);
    assert!(
        !reader_append_ok,
        "FileReaderSession must not expose append_frame (H2)"
    );
    assert!(
        reader_append_stderr.contains("DerefMut")
            || reader_append_stderr.contains("E0596")
            || reader_append_stderr.contains("no method named `append_frame`")
            || reader_append_stderr.contains("E0599"),
        "rejection must be missing DerefMut or missing method: {reader_append_stderr}"
    );

    let file_reader_set_schema_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid(mut reader: FileReaderSession, desc: &SchemaDescriptor) {
            let _ = reader.set_schema_descriptor(desc);
        }
    "#;
    let (reader_schema_ok, reader_schema_stderr) = run_rustc(file_reader_set_schema_code);
    assert!(
        !reader_schema_ok,
        "FileReaderSession must not expose set_schema_descriptor (H2)"
    );
    assert!(
        reader_schema_stderr.contains("DerefMut")
            || reader_schema_stderr.contains("E0596")
            || reader_schema_stderr.contains("no method named `set_schema_descriptor`")
            || reader_schema_stderr.contains("E0599"),
        "rejection must be missing DerefMut or missing method: {reader_schema_stderr}"
    );

    let file_reader_meta_record_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid(mut reader: FileReaderSession, record: &OwnershipRecord) {
            let _ = reader.record_meta_record(record);
        }
    "#;
    let (reader_meta_ok, reader_meta_stderr) = run_rustc(file_reader_meta_record_code);
    assert!(
        !reader_meta_ok,
        "FileReaderSession must not expose record_meta_record (H2)"
    );
    assert!(
        reader_meta_stderr.contains("DerefMut")
            || reader_meta_stderr.contains("E0596")
            || reader_meta_stderr.contains("no method named `record_meta_record`")
            || reader_meta_stderr.contains("E0599"),
        "rejection must be missing DerefMut or missing method: {reader_meta_stderr}"
    );

    let file_writer_mutation_positive_code = r#"
        use pardosa::prelude::*;
        pub fn run_valid(mut writer: FileWriterSession, env: &EventEnvelope, desc: &SchemaDescriptor, record: &OwnershipRecord) {
            let _ = writer.append_envelope(env);
            let _ = writer.set_schema_descriptor(desc);
            let _ = writer.record_meta_record(record);
            let _claim: &OwnershipClaimRecord = writer.claim();
            let _ = writer.release_exclusion();
        }
    "#;
    let (writer_mut_ok, writer_mut_stderr) = run_rustc(file_writer_mutation_positive_code);
    assert!(
        writer_mut_ok,
        "FileWriterSession must expose valid writer controls (H2):\n{writer_mut_stderr}"
    );

    let store_from_parts_code = r#"
        use pardosa::store::{Store, StorageEngine};
        pub fn run_invalid<E: StorageEngine>(engine: E) {
            let _ = Store::from_parts(engine, todo!(), todo!());
        }
    "#;
    let (from_parts_ok, from_parts_stderr) = run_rustc(store_from_parts_code);
    assert!(
        !from_parts_ok,
        "Store::from_parts must not exist (R2-H1, R2-M1)"
    );
    assert!(
        from_parts_stderr.contains("no function or associated item named `from_parts`")
            || from_parts_stderr.contains("E0599"),
        "rejection must be missing from_parts: {from_parts_stderr}"
    );

    let store_engine_mut_code = r#"
        use pardosa::store::{Store, StorageEngine};
        pub fn run_invalid<E: StorageEngine>(mut store: Store<E>) {
            let _ = store.engine_mut();
        }
    "#;
    let (engine_mut_ok, engine_mut_stderr) = run_rustc(store_engine_mut_code);
    assert!(!engine_mut_ok, "Store::engine_mut must not exist (R2-H1)");
    assert!(
        engine_mut_stderr.contains("no method named `engine_mut`")
            || engine_mut_stderr.contains("E0599"),
        "rejection must be missing engine_mut: {engine_mut_stderr}"
    );

    let file_writer_engine_mut_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid(mut writer: FileWriterSession) {
            let _ = writer.engine_mut();
        }
    "#;
    let (fw_engine_mut_ok, fw_engine_mut_stderr) = run_rustc(file_writer_engine_mut_code);
    assert!(
        !fw_engine_mut_ok,
        "FileWriterSession must not expose engine_mut (R2-H1)"
    );
    assert!(
        fw_engine_mut_stderr.contains("no method named `engine_mut`")
            || fw_engine_mut_stderr.contains("E0599"),
        "rejection must be missing engine_mut: {fw_engine_mut_stderr}"
    );

    let file_writer_reuse_after_release_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid(writer: FileWriterSession, env: &EventEnvelope) {
            let _ = writer.release_exclusion();
            let _ = writer.append_envelope(env);
        }
    "#;
    let (fw_reuse_ok, fw_reuse_stderr) = run_rustc(file_writer_reuse_after_release_code);
    assert!(
        !fw_reuse_ok,
        "FileWriterSession must not be reusable after release_exclusion (R2-H1)"
    );
    assert!(
        fw_reuse_stderr.contains("use of moved value") || fw_reuse_stderr.contains("E0382"),
        "rejection must be use of moved value: {fw_reuse_stderr}"
    );
}

#[test]
fn test_h3_blake3_known_answer_golden_vector() {
    let header = EnvelopeHeader {
        event_id: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        fiber_id: [
            17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32,
        ],
        detached: false,
        precursor: [0u8; 16],
        precursor_hash: [0u8; 32],
    };
    let payload = b"Pardosa BLAKE3 golden commitment test payload";
    assert_eq!(payload.len(), 45);
    let mut preimage = Vec::with_capacity(81 + 45);
    header.encode(&mut preimage);
    preimage.extend_from_slice(payload);
    assert_eq!(preimage.len(), 126);

    let commitment = compute_envelope_commitment(&header, payload);
    let expected: [u8; 32] = [
        0xaf, 0x14, 0x56, 0x68, 0xe0, 0x47, 0xba, 0x94, 0xef, 0x43, 0x23, 0x90, 0x1d, 0xe8, 0x09,
        0xd1, 0xf5, 0x5d, 0x4f, 0x82, 0x68, 0x2f, 0x91, 0xb0, 0xfd, 0xa5, 0x43, 0xfb, 0x6e, 0x13,
        0xa7, 0x9e,
    ];
    assert_eq!(commitment, expected);

    let env = EventEnvelope {
        header: header.clone(),
        payload: payload.to_vec(),
    };
    assert_eq!(env.commitment(), expected);
}

#[test]
fn test_compile_fail_observe_event_private() {
    let call_observe_event_code = r#"
        use pardosa::prelude::*;
        pub fn run_invalid() {
            let env = EventEnvelope {
                header: EnvelopeHeader {
                    event_id: [2; 16],
                    fiber_id: [2; 16],
                    detached: false,
                    precursor: [1; 16],
                    precursor_hash: [0; 32],
                },
                payload: vec![],
            };
            ArtefactReader::with_reader("locator", |reader| {
                let _ = reader.observe_event(&env);
            });
        }
    "#;
    let (evt_ok, evt_stderr) = run_rustc(call_observe_event_code);
    assert!(
        !evt_ok,
        "calling observe_event directly must fail compilation (M2)"
    );
    assert!(
        evt_stderr.contains("private"),
        "rejection must cite private method: {evt_stderr}"
    );
}

#[test]
fn test_l1_blake3_known_answer_golden_vector_detached() {
    let header = EnvelopeHeader {
        event_id: [
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
            0x1f, 0x20,
        ],
        fiber_id: [
            0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e,
            0x2f, 0x30,
        ],
        detached: true,
        precursor: [
            0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e,
            0x3f, 0x40,
        ],
        precursor_hash: [
            0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e,
            0x4f, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x5b, 0x5c,
            0x5d, 0x5e, 0x5f, 0x60,
        ],
    };
    let payload = b"Pardosa detached chained event with non-zero precursor and hash";
    assert_eq!(payload.len(), 63);
    let mut preimage = Vec::with_capacity(81 + 63);
    header.encode(&mut preimage);
    preimage.extend_from_slice(payload);
    assert_eq!(preimage.len(), 144);

    let commitment = compute_envelope_commitment(&header, payload);
    let expected: [u8; 32] = [
        0x34, 0xa5, 0x90, 0x6b, 0x24, 0x8a, 0x6c, 0x54, 0xea, 0x69, 0x97, 0x40, 0x9f, 0x2c, 0x05,
        0x60, 0x61, 0x5e, 0xc4, 0xc8, 0x7d, 0xf5, 0xc3, 0x3a, 0xcf, 0xf4, 0x47, 0xb4, 0x3b, 0x5c,
        0x51, 0x40,
    ];
    assert_eq!(commitment, expected);

    let env = EventEnvelope {
        header: header.clone(),
        payload: payload.to_vec(),
    };
    assert_eq!(env.commitment(), expected);
}

#[test]
fn test_m1_integration_high_capacity_zero_length_refusal() {
    let env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [1u8; 16],
            fiber_id: [1u8; 16],
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: Vec::with_capacity(MAX_STREAM_BYTES + 1),
    };
    assert_eq!(env.payload.len(), 0);
    assert!(env.payload.capacity() > MAX_STREAM_BYTES);

    ArtefactReader::with_stream("integration-high-cap", vec![env], |reader| {
        let err = reader
            .read_event()
            .expect("refusal event")
            .expect_err("high capacity zero-length payload must be refused");
        assert_eq!(
            err.condition(),
            &FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
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
fn test_m1_integration_with_frames_byte_budget_refusal_before_decode() {
    let frame = ContainerFrame {
        checksum: 0xbaad_f00d,
        payload: vec![0u8; MAX_STREAM_BYTES + 1],
    };
    let mut invoked = false;
    let res = ArtefactReader::with_frames("integration-over-budget", vec![frame], |_reader| {
        invoked = true;
        99
    });
    assert!(!invoked);
    let err = res.unwrap_err();
    assert_eq!(
        err.condition(),
        &FailureCondition::ValueConstraintViolated {
            constraint: ValueConstraint::TooLong,
        }
    );
    assert_eq!(
        err.diagnostic_detail().message(),
        "frame byte limit exceeded"
    );
}

#[test]
fn test_m1_integration_with_frames_exact_limit_accepted() {
    let env = EventEnvelope {
        header: EnvelopeHeader {
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
    let frame = ContainerFrame::new(encoded);
    let res = ArtefactReader::with_frames("integration-exact-64mib", vec![frame], |reader| {
        let obs = reader
            .read_event()
            .expect("some event")
            .expect("valid genesis");
        assert_eq!(obs.position(), 0);
        assert!(obs.is_genesis());
        assert!(reader.read_event().is_none());
        77
    });
    assert_eq!(res.unwrap(), 77);
}

#[test]
fn test_m2_integration_terminal_failure_lifecycle_and_refusal() {
    let valid_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [1u8; 16],
            fiber_id: [1u8; 16],
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: vec![1, 2, 3],
    };
    let high_cap_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [2u8; 16],
            fiber_id: [1u8; 16],
            detached: false,
            precursor: [1u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: Vec::with_capacity(MAX_STREAM_BYTES + 1),
    };
    ArtefactReader::with_stream(
        "integration-intake-refusal",
        vec![valid_env, high_cap_env],
        |reader| {
            let err = reader.read_event().expect("some").expect_err("refused");
            assert_eq!(
                err.condition(),
                &FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                }
            );
            assert_eq!(
                err.diagnostic_detail().message(),
                "stream byte limit exceeded"
            );
            assert!(reader.read_event().is_none());
        },
    );

    let item_envs = (0..=MAX_STREAM_ITEMS).map(|i| {
        let mut id = [0u8; 16];
        id[..8].copy_from_slice(&(i as u64).to_be_bytes());
        EventEnvelope {
            header: EnvelopeHeader {
                event_id: id,
                fiber_id: [1u8; 16],
                detached: false,
                precursor: [0u8; 16],
                precursor_hash: [0u8; 32],
            },
            payload: Vec::new(),
        }
    });
    ArtefactReader::with_stream("integration-item-intake-refusal", item_envs, |reader| {
        let err = reader.read_event().expect("some").expect_err("refused");
        assert_eq!(
            err.condition(),
            &FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
        assert_eq!(
            err.diagnostic_detail().message(),
            "stream item limit exceeded"
        );
        assert!(reader.read_event().is_none());
    });

    let env1 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [1u8; 16],
            fiber_id: [1u8; 16],
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: vec![1, 2, 3],
    };
    let env2 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [2u8; 16],
            fiber_id: [1u8; 16],
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [9u8; 32],
        },
        payload: vec![4, 5, 6],
    };
    let env3 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [3u8; 16],
            fiber_id: [1u8; 16],
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: vec![7, 8, 9],
    };
    ArtefactReader::with_stream(
        "integration-terminal-cleanup",
        vec![env1, env2, env3],
        |reader| {
            let obs = reader.read_event().expect("some").expect("ok genesis");
            assert_eq!(obs.position(), 0);

            let err = reader
                .read_event()
                .expect("some")
                .expect_err("terminal error");
            assert_eq!(
                err.condition(),
                &FailureCondition::PrecursorChainBroken(None)
            );
            assert!(reader.read_event().is_none());
        },
    );
}

#[test]
fn test_m1_integration_multi_envelope_capacity_tracking_and_release() {
    let mut p1 = Vec::with_capacity(400);
    p1.extend_from_slice(&[1, 2, 3]);
    let env1 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [1u8; 16],
            fiber_id: [1u8; 16],
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: p1,
    };
    let c1 = env1.commitment();

    let mut p2 = Vec::with_capacity(800);
    p2.extend_from_slice(&[4, 5, 6, 7]);
    let env2 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [2u8; 16],
            fiber_id: [1u8; 16],
            detached: false,
            precursor: [1u8; 16],
            precursor_hash: c1,
        },
        payload: p2,
    };

    ArtefactReader::with_stream("integration-cap-release", vec![env1, env2], |reader| {
        let obs1 = reader.read_event().expect("first event").unwrap();
        assert_eq!(obs1.position(), 0);
        let obs2 = reader.read_event().expect("second event").unwrap();
        assert_eq!(obs2.position(), 1);
        assert!(reader.read_event().is_none());
    });
}

#[derive(Debug, Default)]
struct MockEngine {
    blocks: Vec<Vec<u8>>,
    fail_at_block: Option<usize>,
    undetermined_at_block: Option<usize>,
    read_failure: Option<OperationFailure>,
    fail_reads_with_transport_unavailable: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl StorageEngine for MockEngine {
    fn carried_epoch(&self) -> u64 {
        42
    }

    fn append_block(&mut self, block: &[u8]) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        let idx = self.blocks.len();
        if self.undetermined_at_block == Some(idx) {
            return Ok(WriteLandingVerdict::Undetermined { carried_epoch: 42 });
        }
        if self.fail_at_block == Some(idx) {
            return Err(OperationFailure::new(
                FailureCondition::ConcurrencyConflict,
                "simulated write conflict",
            ));
        }
        self.blocks.push(block.to_vec());
        Ok(WriteLandingVerdict::Landed(self.blocks.len() as u64))
    }

    fn read_all(&mut self) -> Result<Vec<Vec<u8>>, OperationFailure> {
        if self
            .fail_reads_with_transport_unavailable
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(OperationFailure::new(
                FailureCondition::TransportUnavailable,
                "transient connection timeout",
            ));
        }
        if let Some(ref err) = self.read_failure {
            return Err(err.clone());
        }
        Ok(self.blocks.clone())
    }

    fn is_retired(&self) -> Result<bool, OperationFailure> {
        Ok(false)
    }

    fn sync(&mut self) -> Result<(), OperationFailure> {
        Ok(())
    }

    fn uncertain_diagnostic(&self) -> Option<&str> {
        None
    }

    fn claim(&self) -> Option<&OwnershipClaimRecord> {
        None
    }

    fn schema_descriptor(&self) -> Option<&SchemaDescriptor> {
        None
    }

    fn set_schema_descriptor(
        &mut self,
        _descriptor: &SchemaDescriptor,
    ) -> Result<(), OperationFailure> {
        Ok(())
    }

    fn record_meta_record(&mut self, _record: &OwnershipRecord) -> Result<(), OperationFailure> {
        Ok(())
    }

    fn outbound_pointer(&self) -> Option<&OutboundPointerRecord> {
        None
    }

    fn inbound_pointer(&self) -> Option<&InboundPointerRecord> {
        None
    }

    fn migration_start(&self) -> Option<&MigrationStartRecord> {
        None
    }

    fn migration_end(&self) -> Option<&MigrationEndRecord> {
        None
    }

    fn rescue_policy_choice(&self) -> Option<&RescuePolicyChoiceRecord> {
        None
    }
}

#[test]
fn test_c5_12_and_c5_16_bounded_batch_landing_verdicts() {
    let fiber = [0x99; 16];
    let e1 = EventEnvelope::genesis([0x01; 16], fiber, b"event-1".to_vec()).expect("genesis");
    let e2 = EventEnvelope::chain(&e1, [0x02; 16], b"event-2".to_vec()).expect("chain 2");
    let e3 = EventEnvelope::chain(&e2, [0x03; 16], b"event-3".to_vec()).expect("chain 3");
    let e4 = EventEnvelope::chain(&e3, [0x04; 16], b"event-4".to_vec()).expect("chain 4");
    let batch = [e1.clone(), e2.clone(), e3.clone(), e4.clone()];

    let mut store_all = Store::open_writer(MockEngine::default()).expect("store open");
    let v_all = store_all
        .append_batch_envelopes_detailed(&batch)
        .expect("verdict all");
    assert_eq!(
        v_all,
        BatchLandingVerdict::LandedAll {
            final_position: 4,
            landed_count: 4,
        }
    );
    assert_eq!(v_all.landed_count(), 4);
    assert_eq!(v_all.unattempted_count(), 0);
    assert_eq!(v_all.total_count(), 4);
    assert_eq!(store_all.rolling_commitment().frame_count(), 4);
    assert_eq!(
        store_all
            .session_index()
            .expect("idx")
            .event_count(&fiber)
            .unwrap(),
        4
    );

    let mut store_undet = Store::open_writer(MockEngine {
        undetermined_at_block: Some(2),
        ..Default::default()
    })
    .expect("store open");
    let v_undet = store_undet
        .append_batch_envelopes_detailed(&batch)
        .expect("verdict undet");
    match &v_undet {
        BatchLandingVerdict::PartialProgress {
            landed_count,
            next_attempt,
            unattempted_count,
        } => {
            assert_eq!(*landed_count, 2);
            assert_eq!(
                *next_attempt,
                NextAttemptStatus::Undetermined { carried_epoch: 42 }
            );
            assert_eq!(*unattempted_count, 1);
        }
        other => panic!("expected PartialProgress, got {other:?}"),
    }
    assert_eq!(v_undet.landed_count(), 2);
    assert_eq!(v_undet.unresolved_count(), 1);
    assert_eq!(v_undet.unattempted_count(), 1);
    assert_eq!(v_undet.total_count(), 4);
    assert_eq!(store_undet.rolling_commitment().frame_count(), 2);
    assert_eq!(
        store_undet
            .session_index()
            .expect("idx")
            .event_count(&fiber)
            .unwrap(),
        2
    );
    assert_eq!(
        store_undet
            .session_index()
            .expect("idx")
            .get_latest(&fiber)
            .unwrap()
            .unwrap()
            .header
            .event_id,
        e2.header.event_id
    );

    let mut store_fail = Store::open_writer(MockEngine {
        fail_at_block: Some(1),
        ..Default::default()
    })
    .expect("store open");
    let v_fail = store_fail
        .append_batch_envelopes_detailed(&batch)
        .expect("verdict fail");
    match &v_fail {
        BatchLandingVerdict::PartialProgress {
            landed_count,
            next_attempt,
            unattempted_count,
        } => {
            assert_eq!(*landed_count, 1);
            match next_attempt {
                NextAttemptStatus::Rejected(error) => {
                    assert_eq!(*error.condition(), FailureCondition::ConcurrencyConflict);
                }
                other => panic!("expected Rejected, got {other:?}"),
            }
            assert_eq!(*unattempted_count, 2);
        }
        other => panic!("expected PartialProgress, got {other:?}"),
    }
    assert_eq!(v_fail.landed_count(), 1);
    assert_eq!(v_fail.rejected_count(), 1);
    assert_eq!(v_fail.unattempted_count(), 2);
    assert_eq!(v_fail.total_count(), 4);
    assert_eq!(store_fail.rolling_commitment().frame_count(), 1);
    assert_eq!(
        store_fail
            .session_index()
            .expect("idx")
            .event_count(&fiber)
            .unwrap(),
        1
    );
    assert_eq!(
        store_fail
            .session_index()
            .expect("idx")
            .get_latest(&fiber)
            .unwrap()
            .unwrap()
            .header
            .event_id,
        e1.header.event_id
    );
}

#[test]
fn test_for_each_envelope_refuses_broken_chain_and_stops_delivery() {
    let mut engine = MockEngine::default();
    let fiber = [0x99; 16];
    let e1 = EventEnvelope::genesis([1u8; 16], fiber, b"payload-1").unwrap();
    let mut e2_broken = EventEnvelope::genesis([2u8; 16], fiber, b"payload-2").unwrap();
    e2_broken.header.precursor = [99u8; 16];

    let mut env1_bytes = Vec::new();
    e1.encode(&mut env1_bytes);

    let mut env2_bytes = Vec::new();
    e2_broken.encode(&mut env2_bytes);

    engine.append_block(&env1_bytes).unwrap();
    engine.append_block(&env2_bytes).unwrap();

    let mut store = Store::open_reader(engine);
    let mut delivered_events = Vec::new();

    let res = store.for_each_envelope(|env_ref| {
        delivered_events.push(env_ref.header.event_id);
        Ok(())
    });

    assert!(res.is_err(), "broken chain must return error");
    assert_eq!(
        *res.unwrap_err().condition(),
        FailureCondition::PrecursorChainBroken(None)
    );
    assert_eq!(delivered_events, Vec::<[u8; 16]>::new());
}

#[test]
fn test_for_each_envelope_consumer_callback_error_does_not_poison_reader_session() {
    let mut engine = MockEngine::default();
    let fiber = [0x88; 16];
    let e1 = EventEnvelope::genesis([1u8; 16], fiber, b"payload-1").unwrap();
    let e2 = EventEnvelope::chain(&e1, [2u8; 16], b"payload-2").unwrap();

    let mut env1_bytes = Vec::new();
    e1.encode(&mut env1_bytes);
    let mut env2_bytes = Vec::new();
    e2.encode(&mut env2_bytes);

    engine.append_block(&env1_bytes).unwrap();
    engine.append_block(&env2_bytes).unwrap();

    let mut store = Store::open_reader(engine);

    let res = store.for_each_envelope(|env| {
        if env.header.event_id == [2u8; 16] {
            return Err(OperationFailure::new(
                FailureCondition::TransformationRefused,
                "consumer explicitly aborted iteration",
            ));
        }
        Ok(())
    });

    assert_eq!(
        *res.unwrap_err().condition(),
        FailureCondition::TransformationRefused
    );

    let mut replay_count = 0;
    let rerun = store.for_each_envelope(|_env| {
        replay_count += 1;
        Ok(())
    });
    assert!(
        rerun.is_ok(),
        "session must remain healthy after consumer callback abort"
    );
    assert_eq!(replay_count, 2);

    let fiber_handle = store.fiber(fiber);
    assert!(
        fiber_handle.is_ok(),
        "fiber point lookup must remain available"
    );
}

#[test]
fn test_for_each_envelope_transport_unavailable_does_not_poison_reader_session() {
    let fail_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut engine = MockEngine {
        fail_reads_with_transport_unavailable: fail_flag.clone(),
        ..Default::default()
    };
    let fiber = [0x77; 16];
    let e1 = EventEnvelope::genesis([1u8; 16], fiber, b"payload-1").unwrap();
    let mut env1_bytes = Vec::new();
    e1.encode(&mut env1_bytes);
    engine.append_block(&env1_bytes).unwrap();

    let mut store = Store::open_reader(engine);

    fail_flag.store(true, std::sync::atomic::Ordering::SeqCst);

    let res = store.for_each_envelope(|_env| Ok(()));
    assert_eq!(
        *res.unwrap_err().condition(),
        FailureCondition::TransportUnavailable
    );

    fail_flag.store(false, std::sync::atomic::Ordering::SeqCst);

    let mut count = 0;
    let rerun = store.for_each_envelope(|_env| {
        count += 1;
        Ok(())
    });
    assert!(
        rerun.is_ok(),
        "session must not be poisoned by transient transport unavailable"
    );
    assert_eq!(count, 1);
}

#[test]
fn test_for_each_envelope_positive_corruption_poisons_reader_session_terminally() {
    let mut engine = MockEngine::default();
    let fiber = [0x66; 16];
    let e1 = EventEnvelope::genesis([1u8; 16], fiber, b"payload-1").unwrap();
    let mut env1_bytes = Vec::new();
    e1.encode(&mut env1_bytes);
    engine.append_block(&env1_bytes).unwrap();
    engine.append_block(b"short-corrupt-frame").unwrap();

    let mut store = Store::open_reader(engine);

    let res = store.for_each_envelope(|_env| Ok(()));
    assert!(res.is_err());
    assert_eq!(
        *res.unwrap_err().condition(),
        FailureCondition::EnvelopeMismatch
    );

    let subsequent = store.for_each_envelope(|_env| Ok(()));
    let err = subsequent.unwrap_err();
    assert_eq!(*err.condition(), FailureCondition::EnvelopeMismatch);
    assert!(err
        .diagnostic_detail()
        .message()
        .contains("retained broken state"));

    let fiber_res = store.fiber(fiber);
    let fiber_err = fiber_res.unwrap_err();
    assert_eq!(*fiber_err.condition(), FailureCondition::EnvelopeMismatch);
    assert!(fiber_err
        .diagnostic_detail()
        .message()
        .contains("retained broken state"));
}

#[test]
fn test_m4_impossible_engine_batch_count_is_refused_by_store() {
    #[derive(Debug, Default)]
    struct RogueBatchEngine {
        epoch: u64,
    }
    impl StorageEngine for RogueBatchEngine {
        fn carried_epoch(&self) -> u64 {
            self.epoch
        }
        fn append_block(
            &mut self,
            _b: &[u8],
        ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
            Ok(WriteLandingVerdict::Landed(1))
        }
        fn append_batch_detailed(&mut self, _blocks: &[&[u8]]) -> BatchLandingVerdict<u64> {
            BatchLandingVerdict::PartialProgress {
                landed_count: 5,
                next_attempt: NextAttemptStatus::Undetermined {
                    carried_epoch: self.epoch,
                },
                unattempted_count: 0,
            }
        }
        fn read_all(&mut self) -> Result<Vec<Vec<u8>>, OperationFailure> {
            Ok(Vec::new())
        }
        fn is_retired(&self) -> Result<bool, OperationFailure> {
            Ok(false)
        }
        fn sync(&mut self) -> Result<(), OperationFailure> {
            Ok(())
        }
        fn uncertain_diagnostic(&self) -> Option<&str> {
            None
        }
        fn claim(&self) -> Option<&OwnershipClaimRecord> {
            None
        }
        fn schema_descriptor(&self) -> Option<&SchemaDescriptor> {
            None
        }
        fn set_schema_descriptor(&mut self, _d: &SchemaDescriptor) -> Result<(), OperationFailure> {
            Ok(())
        }
        fn record_meta_record(&mut self, _r: &OwnershipRecord) -> Result<(), OperationFailure> {
            Ok(())
        }
        fn outbound_pointer(&self) -> Option<&OutboundPointerRecord> {
            None
        }
        fn inbound_pointer(&self) -> Option<&InboundPointerRecord> {
            None
        }
        fn migration_start(&self) -> Option<&MigrationStartRecord> {
            None
        }
        fn migration_end(&self) -> Option<&MigrationEndRecord> {
            None
        }
        fn rescue_policy_choice(&self) -> Option<&RescuePolicyChoiceRecord> {
            None
        }
    }

    let fiber = [0x55; 16];
    let e1 = EventEnvelope::genesis([1u8; 16], fiber, b"valid-1").unwrap();
    let mut e1_buf = Vec::new();
    e1.encode(&mut e1_buf);

    let mut store = Store::open_writer(RogueBatchEngine { epoch: 1 }).unwrap();
    let err = store.append_batch_detailed(&[&e1_buf]).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::PrecursorChainBroken(None)
    );
    assert!(err.diagnostic_detail().message().contains(
        "inconsistent batch partition: landed 5 + next 1 + unattempted 0 != batch length 1"
    ));
}

#[test]
fn test_m4_inconsistent_suffix_partition_rejected() {
    struct SuffixRogueEngine {
        epoch: u64,
    }
    impl StorageEngine for SuffixRogueEngine {
        fn carried_epoch(&self) -> u64 {
            self.epoch
        }
        fn append_block(
            &mut self,
            _b: &[u8],
        ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
            Ok(WriteLandingVerdict::Landed(1))
        }
        fn append_batch_detailed(&mut self, _blocks: &[&[u8]]) -> BatchLandingVerdict<u64> {
            BatchLandingVerdict::PartialProgress {
                landed_count: 0,
                next_attempt: NextAttemptStatus::Undetermined {
                    carried_epoch: self.epoch,
                },
                unattempted_count: 99,
            }
        }
        fn read_all(&mut self) -> Result<Vec<Vec<u8>>, OperationFailure> {
            Ok(Vec::new())
        }
        fn is_retired(&self) -> Result<bool, OperationFailure> {
            Ok(false)
        }
        fn sync(&mut self) -> Result<(), OperationFailure> {
            Ok(())
        }
        fn uncertain_diagnostic(&self) -> Option<&str> {
            None
        }
        fn claim(&self) -> Option<&OwnershipClaimRecord> {
            None
        }
        fn schema_descriptor(&self) -> Option<&SchemaDescriptor> {
            None
        }
        fn set_schema_descriptor(&mut self, _d: &SchemaDescriptor) -> Result<(), OperationFailure> {
            Ok(())
        }
        fn record_meta_record(&mut self, _r: &OwnershipRecord) -> Result<(), OperationFailure> {
            Ok(())
        }
        fn outbound_pointer(&self) -> Option<&OutboundPointerRecord> {
            None
        }
        fn inbound_pointer(&self) -> Option<&InboundPointerRecord> {
            None
        }
        fn migration_start(&self) -> Option<&MigrationStartRecord> {
            None
        }
        fn migration_end(&self) -> Option<&MigrationEndRecord> {
            None
        }
        fn rescue_policy_choice(&self) -> Option<&RescuePolicyChoiceRecord> {
            None
        }
    }

    let fiber = [0x55; 16];
    let e1 = EventEnvelope::genesis([1u8; 16], fiber, b"valid-1").unwrap();
    let mut e1_buf = Vec::new();
    e1.encode(&mut e1_buf);

    let mut store = Store::open_writer(SuffixRogueEngine { epoch: 1 }).unwrap();
    let err = store.append_batch_detailed(&[&e1_buf]).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::PrecursorChainBroken(None)
    );
    assert!(err.diagnostic_detail().message().contains(
        "inconsistent batch partition: landed 0 + next 1 + unattempted 99 != batch length 1"
    ));
}

#[test]
fn test_m3_consumer_callback_returning_precursor_chain_broken_does_not_poison_reader() {
    let mut engine = MockEngine::default();
    let fiber = [0x44; 16];
    let e1 = EventEnvelope::genesis([1u8; 16], fiber, b"payload-1").unwrap();
    let mut env1_bytes = Vec::new();
    e1.encode(&mut env1_bytes);
    engine.append_block(&env1_bytes).unwrap();

    let mut store = Store::open_reader(engine);

    let res = store.for_each_envelope(|_env| {
        Err(OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            "consumer callback returned precursor break for internal domain reason",
        ))
    });
    assert_eq!(
        *res.unwrap_err().condition(),
        FailureCondition::PrecursorChainBroken(None)
    );

    let rerun = store.for_each_envelope(|_env| Ok(()));
    assert!(
        rerun.is_ok(),
        "reader session must not be poisoned when callback returns precursor broken"
    );
    assert!(store.fiber(fiber).is_ok());
}

#[test]
fn test_m14_session_index_refuses_when_reader_retains_broken_state() {
    let mut engine = MockEngine::default();
    let fiber = [0x33; 16];
    let e1 = EventEnvelope::genesis([1u8; 16], fiber, b"payload-1").unwrap();
    let mut env1_bytes = Vec::new();
    e1.encode(&mut env1_bytes);
    engine.append_block(&env1_bytes).unwrap();
    engine.append_block(b"broken-frame").unwrap();

    let mut store = Store::open_reader(engine);
    store
        .for_each_envelope(|_env| Ok(()))
        .expect_err("visitor must fail on corrupt frame");

    let idx_err = store.session_index().unwrap_err();
    assert_eq!(*idx_err.condition(), FailureCondition::EnvelopeMismatch);
    assert!(idx_err
        .diagnostic_detail()
        .message()
        .contains("retained broken state"));
}

#[test]
fn test_h1_open_reader_with_transport_unavailable_refuses_point_lookups_and_recovers_atomically() {
    let fail_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let fiber = [0x22; 16];
    let e1 = EventEnvelope::genesis([1u8; 16], fiber, b"payload-h1").unwrap();
    let mut env1_bytes = Vec::new();
    e1.encode(&mut env1_bytes);

    let engine = MockEngine {
        blocks: vec![env1_bytes],
        fail_reads_with_transport_unavailable: fail_flag.clone(),
        ..Default::default()
    };

    let mut store = Store::open_reader(engine);

    let get_err = store.get_latest(fiber).unwrap_err();
    assert_eq!(*get_err.condition(), FailureCondition::TransportUnavailable);

    let fiber_err = store.fiber(fiber).unwrap_err();
    assert_eq!(
        *fiber_err.condition(),
        FailureCondition::TransportUnavailable
    );

    let idx_err = store.session_index().unwrap_err();
    assert_eq!(*idx_err.condition(), FailureCondition::TransportUnavailable);

    fail_flag.store(false, std::sync::atomic::Ordering::SeqCst);
    let mut seen = Vec::new();
    let res = store.for_each_envelope(|env| {
        seen.push(env.header.event_id);
        Ok(())
    });
    assert!(
        res.is_ok(),
        "for_each_envelope must succeed after transport recovers"
    );
    assert_eq!(seen, vec![[1u8; 16]]);

    let latest = store
        .get_latest(fiber)
        .expect("get_latest must succeed")
        .expect("envelope present");
    assert_eq!(latest.header.event_id, [1u8; 16]);

    let handle = store.fiber(fiber).expect("fiber must succeed");
    assert_eq!(handle.event_count(), 1);

    let idx = store.session_index().expect("session_index must succeed");
    assert_eq!(idx.event_count(&fiber).unwrap(), 1);

    assert_eq!(store.rolling_commitment().frame_count(), 1);
}
