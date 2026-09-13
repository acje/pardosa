use pardosa::file::FileStorageAdapter;
use pardosa::migration::*;
use pardosa::prelude::*;
use std::collections::HashMap;

fn sample_claim(epoch: u64) -> OwnershipClaimRecord {
    OwnershipClaimRecord {
        epoch,
        machine_id: [1u8; 16],
        boot_id: [2u8; 16],
        process_id: 12345,
        process_start_time_ns: 1_000_000,
        claim_time_ns: 2_000_000,
        operator_label: "operator-m6-test".to_string(),
    }
}

fn sample_genesis_envelope(event_id_byte: u8, fiber_id_byte: u8, payload: &[u8]) -> EventEnvelope {
    EventEnvelope {
        header: EnvelopeHeader {
            event_id: [event_id_byte; 16],
            fiber_id: [fiber_id_byte; 16],
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: payload.to_vec(),
    }
}

fn sample_chained_envelope(
    event_id_byte: u8,
    fiber_id_byte: u8,
    precursor_id_byte: u8,
    precursor_commitment: [u8; 32],
    payload: &[u8],
) -> EventEnvelope {
    EventEnvelope {
        header: EnvelopeHeader {
            event_id: [event_id_byte; 16],
            fiber_id: [fiber_id_byte; 16],
            detached: false,
            precursor: [precursor_id_byte; 16],
            precursor_hash: precursor_commitment,
        },
        payload: payload.to_vec(),
    }
}

#[test]
fn test_m6_live_migration_manager_and_freeze_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source"));
    let target = FileStorageAdapter::new(dir.path().join("target"));

    let err_new = MigrationManager::new(source.clone(), target.clone()).unwrap_err();
    assert_eq!(
        *err_new.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(err_new.to_string().contains("live migration is disabled in this release per C5.18; use offline administrative migration"));

    let mut manager = MigrationManager::new_for_test(source, target);
    let err_freeze = manager.freeze().unwrap_err();
    assert_eq!(
        *err_freeze.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(err_freeze.to_string().contains("live migration is disabled in this release per C5.18; use offline administrative migration"));
}

#[test]
fn test_m6_migration_basic_lifecycle_and_cutover() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source_path = dir.path().join("source");
    let target_path = dir.path().join("target");

    let source = FileStorageAdapter::new(&source_path);
    let target = FileStorageAdapter::new(&target_path);

    let claim1 = sample_claim(1);
    source.create(&claim1).expect("create source");
    let claim2 = sample_claim(1);
    target.create(&claim2).expect("create target");

    let mut writer = source.open_write(1).expect("open write source");
    let env1 = sample_genesis_envelope(1, 0xaa, b"payload1");
    let comm1 = env1.commitment();
    let env2 = sample_chained_envelope(2, 0xaa, 1, comm1, b"payload2");
    writer.append_envelope_verdict(&env1).expect("append 1");
    writer.append_envelope_verdict(&env2).expect("append 2");

    let err_mgr = MigrationManager::new(source.clone(), target.clone()).unwrap_err();
    assert_eq!(
        *err_mgr.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(err_mgr.to_string().contains("live migration is disabled in this release per C5.18; use offline administrative migration"));

    let source_locator = source.locator_id();
    let target_locator = target.locator_id();
    let source_epoch = source.current_epoch().expect("source epoch");

    let inbound = InboundPointerRecord {
        prior_generation_locator_id: source_locator,
        prior_generation_epoch: source_epoch,
    };
    target
        .record_inbound_pointer(&inbound)
        .expect("record inbound");

    let end_record = MigrationEndRecord {
        source_generation: 1,
        target_generation: 2,
        end_time_ns: 2_000_000_000,
        status: MigrationStatus::Complete,
    };
    target
        .record_meta_record_for_test(&OwnershipRecord::MigrationEnd(end_record))
        .expect("record migration end");

    let rescue_choice = RescuePolicyChoiceRecord {
        policy_tag: RescuePolicy::Strict.to_u8(),
        parameter_payload: Vec::new(),
    };
    target
        .record_meta_record_for_test(&OwnershipRecord::RescuePolicyChoice(rescue_choice))
        .expect("record rescue choice");

    let outbound = OutboundPointerRecord {
        next_generation_locator_id: target_locator,
        cutover_epoch: source_epoch,
    };
    source
        .record_outbound_pointer_for_test(&outbound)
        .expect("record outbound");

    assert!(source.is_retired_source().expect("query retired"));

    let err_append = writer
        .append_envelope_verdict(&env1)
        .expect_err("source writer retired");
    assert_eq!(
        *err_append.condition(),
        FailureCondition::RetiredMigrationSource
    );

    drop(writer);

    let err_new = source.open_write(1).expect_err("source must be retired");
    assert_eq!(
        *err_new.condition(),
        FailureCondition::RetiredMigrationSource
    );

    let target_reader = target.open_read().expect("open target reader");
    assert_eq!(
        target_reader
            .inbound_pointer()
            .unwrap()
            .prior_generation_locator_id,
        source.locator_id()
    );

    let meta = target.read_meta_records().expect("read meta records");
    assert!(meta.inbound_pointer.is_some());
    assert!(meta.migration_end.is_some());
    assert!(meta.rescue_policy_choice.is_some());
}

#[test]
fn test_m6_caller_transformation_closure_and_refusal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_tx"));
    let target = FileStorageAdapter::new(dir.path().join("target_tx"));

    let claim = sample_claim(1);
    source.create(&claim).expect("create source");
    target.create(&claim).expect("create target");

    let mut writer = source.open_write(1).expect("open write source");
    let env1 = sample_genesis_envelope(1, 0xaa, b"input_data");
    writer.append_envelope_verdict(&env1).expect("append 1");

    let err = MigrationManager::new(source.clone(), target.clone()).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(!source.is_retired_source().expect("query retired"));
    let env2 = sample_chained_envelope(2, 0xaa, 1, env1.commitment(), b"more_data");
    writer
        .append_envelope_verdict(&env2)
        .expect("existing writer still accepts appends");
    drop(writer);
    let mut writer_new = source.open_write(1).expect("new writer can be opened");
    let env3 = sample_chained_envelope(3, 0xaa, 2, env2.commitment(), b"even_more_data");
    writer_new
        .append_envelope_verdict(&env3)
        .expect("new writer accepts appends");
}

#[test]
fn test_m6_per_fiber_policies_keep_purge_lock_and_prune() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_policies"));
    let target = FileStorageAdapter::new(dir.path().join("target_policies"));

    let claim = sample_claim(1);
    source.create(&claim).expect("create source");
    target.create(&claim).expect("create target");

    let mut writer = source.open_write(1).expect("open write source");

    let a1 = sample_genesis_envelope(1, 0x11, b"a1");
    let comm_a1 = a1.commitment();
    let a2 = sample_chained_envelope(2, 0x11, 1, comm_a1, b"a2");
    let comm_a2 = a2.commitment();
    let a3 = sample_chained_envelope(3, 0x11, 2, comm_a2, b"a3");

    writer.append_envelope_verdict(&a1).expect("append a1");
    writer.append_envelope_verdict(&a2).expect("append a2");
    writer.append_envelope_verdict(&a3).expect("append a3");

    let err = MigrationManager::new(source.clone(), target.clone()).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(!source.is_retired_source().expect("query retired"));
}

#[test]
fn test_m6_dense_rechaining_and_pairwise_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_order"));
    let target = FileStorageAdapter::new(dir.path().join("target_order"));

    let claim = sample_claim(1);
    source.create(&claim).expect("create source");
    target.create(&claim).expect("create target");

    let mut writer = source.open_write(1).expect("open write source");

    let a1 = sample_genesis_envelope(1, 0x0a, b"A1");
    let comm_a1 = a1.commitment();
    let b1 = sample_genesis_envelope(2, 0x0b, b"B1");
    let comm_b1 = b1.commitment();
    let a2 = sample_chained_envelope(3, 0x0a, 1, comm_a1, b"A2");
    let comm_a2 = a2.commitment();
    let b2 = sample_chained_envelope(4, 0x0b, 2, comm_b1, b"B2");
    let a3 = sample_chained_envelope(5, 0x0a, 3, comm_a2, b"A3");

    writer.append_envelope_verdict(&a1).expect("append");
    writer.append_envelope_verdict(&b1).expect("append");
    writer.append_envelope_verdict(&a2).expect("append");
    writer.append_envelope_verdict(&b2).expect("append");
    writer.append_envelope_verdict(&a3).expect("append");

    let err = MigrationManager::new(source.clone(), target.clone()).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(!source.is_retired_source().expect("query retired"));
}

#[test]
fn test_m6_broken_chain_election_refuse_vs_permit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source_refuse = FileStorageAdapter::new(dir.path().join("source_refuse"));
    let target_refuse = FileStorageAdapter::new(dir.path().join("target_refuse"));

    let claim = sample_claim(1);
    source_refuse.create(&claim).expect("create source refuse");
    target_refuse.create(&claim).expect("create target refuse");

    let mut writer = source_refuse.open_write(1).expect("open write source");
    let env1 = sample_genesis_envelope(1, 0xaa, b"valid1");
    let broken_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [2u8; 16],
            fiber_id: [0xaau8; 16],
            detached: false,
            precursor: [1u8; 16],
            precursor_hash: [0xffu8; 32],
        },
        payload: b"broken_event".to_vec(),
    };
    writer.append_envelope_verdict(&env1).expect("append env1");
    let mut broken_buf = Vec::new();
    broken_env.encode(&mut broken_buf);
    writer
        .append_unvalidated_frame(&broken_buf)
        .expect("append broken raw frame");

    let err_refuse =
        MigrationManager::new(source_refuse.clone(), target_refuse.clone()).unwrap_err();
    assert_eq!(
        *err_refuse.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(!source_refuse.is_retired_source().expect("query retired"));

    let source_permit = FileStorageAdapter::new(dir.path().join("source_permit"));
    let target_permit = FileStorageAdapter::new(dir.path().join("target_permit"));
    source_permit.create(&claim).expect("create source permit");
    target_permit.create(&claim).expect("create target permit");

    let mut writer_permit = source_permit
        .open_write(1)
        .expect("open write source permit");
    writer_permit
        .append_envelope_verdict(&env1)
        .expect("append env1");
    writer_permit
        .append_unvalidated_frame(&broken_buf)
        .expect("append broken raw frame");

    let mut src_reader = source_permit.open_read().expect("open source reader");
    let ordinary_err = src_reader
        .read_all_envelopes()
        .expect_err("ordinary reader on source must refuse broken chain");
    assert!(matches!(
        ordinary_err.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));
    let src_envelopes = src_reader
        .read_all_envelopes_for_migration()
        .expect("migration reader on source permits broken history");
    let mut src_map = HashMap::new();
    for env in &src_envelopes {
        src_map.insert(env.header.event_id, env);
    }
    let broken_link = PrecursorLink::classify(&src_envelopes[1]).expect("classify broken link");
    let ordinary_err =
        admit_precursor_link(&src_envelopes[1].header.fiber_id, &broken_link, |id| {
            src_map.get(id).copied()
        })
        .expect_err("ordinary reader on source must refuse broken chain");
    assert!(matches!(
        ordinary_err.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));

    let err_permit =
        MigrationManager::new(source_permit.clone(), target_permit.clone()).unwrap_err();
    assert_eq!(
        *err_permit.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
}

#[test]
fn test_m6_chase_phase_concurrent_source_appends() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_chase"));
    let target = FileStorageAdapter::new(dir.path().join("target_chase"));

    let err = MigrationManager::new(source.clone(), target.clone()).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(err.to_string().contains("live migration is disabled in this release per C5.18; use offline administrative migration"));
}

#[test]
fn test_m6_transform_failure_retry_does_not_leak_staged_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_retry"));
    let target = FileStorageAdapter::new(dir.path().join("target_retry"));

    let err = MigrationManager::new(source.clone(), target.clone()).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(err.to_string().contains("live migration is disabled in this release per C5.18; use offline administrative migration"));
}
