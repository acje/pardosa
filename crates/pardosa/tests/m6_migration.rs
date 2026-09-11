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
    writer.append_envelope(&env1).expect("append 1");
    writer.append_envelope(&env2).expect("append 2");

    let mut manager = MigrationManager::new(source.clone(), target.clone());
    let chased = manager.chase().expect("chase");
    assert_eq!(chased, 2);
    assert_eq!(manager.phase(), MigrationPhase::Chase);

    let frozen = manager.freeze().expect("freeze");
    assert_eq!(frozen, 0);
    assert_eq!(manager.phase(), MigrationPhase::Freeze);

    let summary = manager.cutover().expect("cutover");
    assert_eq!(summary.total_migrated_events, 2);
    assert_eq!(summary.surviving_fibers, 1);

    let err_new = source.open_write(1).expect_err("source must be retired");
    assert_eq!(
        *err_new.condition(),
        FailureCondition::RetiredMigrationSource
    );

    let err_append = writer
        .append_envelope(&env1)
        .expect_err("source writer retired");
    assert_eq!(
        *err_append.condition(),
        FailureCondition::RetiredMigrationSource
    );

    let mut target_reader = target.open_read().expect("open target reader");
    assert_eq!(
        target_reader
            .inbound_pointer()
            .unwrap()
            .prior_generation_locator_id,
        source.locator_id()
    );
    let migrated = target_reader.read_all_envelopes().expect("read migrated");
    assert_eq!(migrated.len(), 2);
    assert_ne!(migrated[0].header.event_id, env1.header.event_id);
    assert_ne!(migrated[0].header.fiber_id, env1.header.fiber_id);
    assert_eq!(migrated[0].header.precursor, [0u8; 16]);
    assert_eq!(migrated[0].header.precursor_hash, [0u8; 32]);
    assert_eq!(migrated[1].header.precursor, migrated[0].header.event_id);
    assert_eq!(migrated[1].header.precursor_hash, migrated[0].commitment());
    assert_eq!(migrated[0].payload, b"payload1");
    assert_eq!(migrated[1].payload, b"payload2");
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
    writer.append_envelope(&env1).expect("append 1");

    let manager_success = MigrationManager::new(source.clone(), target.clone()).with_transformer(
        |payload: &[u8]| {
            let mut transformed = payload.to_vec();
            transformed.extend_from_slice(b"_v2");
            Ok(transformed)
        },
    );

    let summary = manager_success.run_all().expect("run all migration");
    assert_eq!(summary.total_migrated_events, 1);

    let mut reader = target.open_read().expect("open target reader");
    let migrated = reader.read_all_envelopes().expect("read envelopes");
    assert_eq!(migrated[0].payload, b"input_data_v2");

    let source_fail = FileStorageAdapter::new(dir.path().join("source_tx_fail"));
    let target_fail = FileStorageAdapter::new(dir.path().join("target_tx_fail"));
    source_fail.create(&claim).expect("create source");
    target_fail.create(&claim).expect("create target");

    let mut writer_fail = source_fail.open_write(1).expect("open write source");
    writer_fail.append_envelope(&env1).expect("append");

    let manager_fail = MigrationManager::new(source_fail.clone(), target_fail.clone())
        .with_transformer(|_payload: &[u8]| {
            Err(OperationFailure::new(
                FailureCondition::TransformationRefused,
                "schema upcast rejected payload",
            ))
        });

    let err = manager_fail
        .run_all()
        .expect_err("transformation refusal must abort migration");
    assert_eq!(*err.condition(), FailureCondition::TransformationRefused);

    assert!(!source_fail.is_retired_source().expect("query retired"));
    let env2 = sample_chained_envelope(2, 0xaa, 1, env1.commitment(), b"more_data");
    writer_fail
        .append_envelope(&env2)
        .expect("existing writer still accepts appends");
    drop(writer_fail);
    let mut writer_new = source_fail.open_write(1).expect("new writer can be opened");
    let env3 = sample_chained_envelope(3, 0xaa, 2, env2.commitment(), b"even_more_data");
    writer_new
        .append_envelope(&env3)
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

    let fiber_keep = [0x11u8; 16];
    let fiber_purge = [0x22u8; 16];
    let fiber_lock = [0x33u8; 16];

    let mut writer = source.open_write(1).expect("open write source");

    let a1 = sample_genesis_envelope(1, 0x11, b"a1");
    let comm_a1 = a1.commitment();
    let a2 = sample_chained_envelope(2, 0x11, 1, comm_a1, b"a2");
    let comm_a2 = a2.commitment();
    let a3 = sample_chained_envelope(3, 0x11, 2, comm_a2, b"a3");

    let b1 = sample_genesis_envelope(10, 0x22, b"b1");
    let comm_b1 = b1.commitment();
    let b2 = sample_chained_envelope(11, 0x22, 10, comm_b1, b"b2");

    let c1 = sample_genesis_envelope(20, 0x33, b"c1");
    let comm_c1 = c1.commitment();
    let c2 = sample_chained_envelope(21, 0x33, 20, comm_c1, b"c2");
    let comm_c2 = c2.commitment();
    let c3 = sample_chained_envelope(22, 0x33, 21, comm_c2, b"c3");

    writer.append_envelope(&a1).expect("append a1");
    writer.append_envelope(&b1).expect("append b1");
    writer.append_envelope(&c1).expect("append c1");
    writer.append_envelope(&a2).expect("append a2");
    writer.append_envelope(&c2).expect("append c2");
    writer.append_envelope(&b2).expect("append b2");
    writer.append_envelope(&a3).expect("append a3");
    writer.append_envelope(&c3).expect("append c3");

    let manager = MigrationManager::new(source.clone(), target.clone())
        .with_fiber_policy(fiber_keep, FiberMigrationPolicy::Keep)
        .with_fiber_policy(fiber_purge, FiberMigrationPolicy::Purge)
        .with_fiber_policy(fiber_lock, FiberMigrationPolicy::LockAndPrune);

    let summary = manager.run_all().expect("run all migration");
    assert_eq!(summary.total_migrated_events, 4);
    assert_eq!(summary.surviving_fibers, 2);

    let mut reader = target.open_read().expect("open target reader");
    let migrated = reader.read_all_envelopes().expect("read envelopes");
    assert_eq!(migrated.len(), 4);

    assert_eq!(migrated[0].payload, b"a1");
    assert_eq!(migrated[1].payload, b"a2");
    assert_eq!(migrated[2].payload, b"a3");
    assert_eq!(migrated[3].payload, b"c3");

    assert!(migrated[3].header.detached);
    assert_eq!(migrated[3].header.precursor, [0u8; 16]);
    assert_eq!(migrated[3].header.precursor_hash, [0u8; 32]);

    for env in &migrated {
        assert_ne!(env.header.fiber_id, fiber_purge);
        assert_ne!(env.payload, b"b1");
        assert_ne!(env.payload, b"b2");
    }
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

    writer.append_envelope(&a1).expect("append");
    writer.append_envelope(&b1).expect("append");
    writer.append_envelope(&a2).expect("append");
    writer.append_envelope(&b2).expect("append");
    writer.append_envelope(&a3).expect("append");

    let manager = MigrationManager::new(source.clone(), target.clone());
    let summary = manager.run_all().expect("run all migration");
    assert_eq!(summary.total_migrated_events, 5);

    let mut reader = target.open_read().expect("open target reader");
    let migrated = reader.read_all_envelopes().expect("read envelopes");

    let payloads: Vec<&[u8]> = migrated.iter().map(|e| e.payload.as_slice()).collect();
    assert_eq!(payloads, vec![b"A1", b"B1", b"A2", b"B2", b"A3"]);

    let target_a_fiber = migrated[0].header.fiber_id;
    let target_b_fiber = migrated[1].header.fiber_id;
    assert_ne!(target_a_fiber, target_b_fiber);

    let a_events: Vec<&EventEnvelope> = migrated
        .iter()
        .filter(|e| e.header.fiber_id == target_a_fiber)
        .collect();
    assert_eq!(a_events.len(), 3);
    assert_eq!(a_events[0].header.precursor, [0u8; 16]);
    assert_eq!(a_events[0].header.precursor_hash, [0u8; 32]);
    assert_eq!(a_events[1].header.precursor, a_events[0].header.event_id);
    assert_eq!(a_events[1].header.precursor_hash, a_events[0].commitment());
    assert_eq!(a_events[2].header.precursor, a_events[1].header.event_id);
    assert_eq!(a_events[2].header.precursor_hash, a_events[1].commitment());

    let mut env_map = HashMap::new();
    for env in &migrated {
        env_map.insert(env.header.event_id, env);
    }

    for env in &migrated {
        let link = PrecursorLink::classify(env).expect("classify target link");
        admit_precursor_link(&env.header.fiber_id, &link, |id| env_map.get(id).copied())
            .expect("target precursor link validation");
    }
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
    writer.append_envelope(&env1).expect("append env1");
    let mut broken_buf = Vec::new();
    broken_env.encode(&mut broken_buf);
    writer
        .append_unvalidated_frame(&broken_buf)
        .expect("append broken raw frame");

    let manager_refuse = MigrationManager::new(source_refuse.clone(), target_refuse.clone())
        .with_broken_chain_election(BrokenChainElection::RefuseOnBreak);

    let err = manager_refuse
        .run_all()
        .expect_err("must refuse on broken chain");
    assert!(matches!(
        err.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));
    assert!(!source_refuse.is_retired_source().expect("query retired"));

    let source_permit = FileStorageAdapter::new(dir.path().join("source_permit"));
    let target_permit = FileStorageAdapter::new(dir.path().join("target_permit"));
    source_permit.create(&claim).expect("create source permit");
    target_permit.create(&claim).expect("create target permit");

    let mut writer_permit = source_permit
        .open_write(1)
        .expect("open write source permit");
    writer_permit.append_envelope(&env1).expect("append env1");
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

    let manager_permit = MigrationManager::new(source_permit.clone(), target_permit.clone())
        .with_broken_chain_election(BrokenChainElection::PermitBrokenHistory);

    let summary = manager_permit
        .run_all()
        .expect("permit broken history migration");
    assert_eq!(summary.total_migrated_events, 2);

    let mut target_reader = target_permit.open_read().expect("open target reader");
    let ordinary_target_err = target_reader
        .read_all_envelopes()
        .expect_err("ordinary reader on target with broken history must refuse");
    assert!(matches!(
        ordinary_target_err.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));
    let migrated = target_reader
        .read_all_envelopes_for_migration()
        .expect("migration reader on target reads broken history");
    assert_eq!(migrated.len(), 2);

    assert_eq!(migrated[0].header.precursor, [0u8; 16]);
    assert_eq!(migrated[0].header.precursor_hash, [0u8; 32]);
    assert_eq!(migrated[1].header.precursor, [0u8; 16]);
    assert_eq!(migrated[1].header.precursor_hash, [0u8; 32]);

    let mut target_map = HashMap::new();
    for env in &migrated {
        target_map.insert(env.header.event_id, env);
    }

    for env in &migrated {
        let link = PrecursorLink::classify(env).expect("classify link");
        admit_precursor_link(&env.header.fiber_id, &link, |id| {
            target_map.get(id).copied()
        })
        .expect("target reader validates successfully");
    }
}

#[cfg(feature = "nats")]
#[test]
fn test_m6_migration_on_nats_adapter() {
    let server = pardosa_nats::test_support::LiveNatsServer::acquire();
    let src_stem = format!("m6_nats_src_{}_{}", std::process::id(), 1);
    let dst_stem = format!("m6_nats_dst_{}_{}", std::process::id(), 2);

    let source =
        pardosa_nats::NatsStorageAdapter::new(server.url(), &src_stem).expect("connect src");
    let target =
        pardosa_nats::NatsStorageAdapter::new(server.url(), &dst_stem).expect("connect dst");

    let claim = sample_claim(1);
    let mut writer = source.create(&claim).expect("create source");
    let target_claim = sample_claim(1);
    target.create(&target_claim).expect("create target");

    let env1 = sample_genesis_envelope(1, 0x55, b"nats_e1");
    let comm1 = env1.commitment();
    let env2 = sample_chained_envelope(2, 0x55, 1, comm1, b"nats_e2");
    writer.append_envelope(&env1).expect("append 1");
    writer.append_envelope(&env2).expect("append 2");

    let manager = MigrationManager::new(source.clone(), target.clone()).with_transformer(
        |payload: &[u8]| {
            let mut out = payload.to_vec();
            out.extend_from_slice(b"_migrated");
            Ok(out)
        },
    );

    let summary = manager.run_all().expect("run nats migration");
    assert_eq!(summary.total_migrated_events, 2);
    assert_eq!(summary.surviving_fibers, 1);

    let err_new = source.open_write(1).expect_err("source must be retired");
    assert_eq!(
        *err_new.condition(),
        FailureCondition::RetiredMigrationSource
    );

    let err_append = writer
        .append_envelope(&env1)
        .expect_err("existing writer retired");
    assert_eq!(
        *err_append.condition(),
        FailureCondition::RetiredMigrationSource
    );

    let mut reader = target.open_read().expect("open target reader");
    let envelopes = reader
        .read_all_envelopes()
        .expect("read migrated envelopes");
    assert_eq!(envelopes.len(), 2);
    assert_eq!(envelopes[0].payload, b"nats_e1_migrated");
    assert_eq!(envelopes[1].payload, b"nats_e2_migrated");

    source.delete_streams().expect("cleanup src");
    target.delete_streams().expect("cleanup dst");
}

#[cfg(feature = "nats")]
#[test]
fn test_m6_migration_cross_adapter_file_to_nats() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_cross"));

    let server = pardosa_nats::test_support::LiveNatsServer::acquire();
    let dst_stem = format!("m6_cross_dst_{}_{}", std::process::id(), 3);
    let target =
        pardosa_nats::NatsStorageAdapter::new(server.url(), &dst_stem).expect("connect dst");

    let claim = sample_claim(1);
    source.create(&claim).expect("create source");
    target.create(&claim).expect("create target");

    let mut writer = source.open_write(1).expect("open write source");
    let env1 = sample_genesis_envelope(1, 0x77, b"cross_data");
    writer.append_envelope(&env1).expect("append cross");

    let manager = MigrationManager::new(source.clone(), target.clone());
    let summary = manager.run_all().expect("run cross migration");
    assert_eq!(summary.total_migrated_events, 1);

    assert!(source.is_retired_source().expect("source retired"));
    let mut reader = target.open_read().expect("open target reader");
    let envelopes = reader.read_all_envelopes().expect("read target");
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].payload, b"cross_data");

    target.delete_streams().expect("cleanup dst");
}

#[test]
fn test_m6_chase_phase_concurrent_source_appends() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_chase"));
    let target = FileStorageAdapter::new(dir.path().join("target_chase"));

    let claim = sample_claim(1);
    source.create(&claim).expect("create source");
    target.create(&claim).expect("create target");

    let mut writer = source.open_write(1).expect("open write source");
    let env1 = sample_genesis_envelope(1, 0x11, b"chase1");
    let comm1 = env1.commitment();
    let env2 = sample_chained_envelope(2, 0x11, 1, comm1, b"chase2");
    let comm2 = env2.commitment();
    writer.append_envelope(&env1).expect("append env1");
    writer.append_envelope(&env2).expect("append env2");

    let mut manager = MigrationManager::new(source.clone(), target.clone());
    let chased = manager.chase().expect("chase initial batch");
    assert_eq!(chased, 2);
    assert_eq!(manager.phase(), MigrationPhase::Chase);

    let env3 = sample_chained_envelope(3, 0x11, 2, comm2, b"chase3");
    let comm3 = env3.commitment();
    let env4 = sample_chained_envelope(4, 0x11, 3, comm3, b"chase4");
    writer
        .append_envelope(&env3)
        .expect("append env3 during chase");
    writer
        .append_envelope(&env4)
        .expect("append env4 during chase");

    let frozen = manager.freeze().expect("freeze and drain remainder");
    assert_eq!(frozen, 2);
    assert_eq!(manager.phase(), MigrationPhase::Freeze);

    let summary = manager.cutover().expect("cutover");
    assert_eq!(summary.total_migrated_events, 4);
    assert_eq!(summary.surviving_fibers, 1);

    let err = writer
        .append_envelope(&env1)
        .expect_err("source writer permanently retired");
    assert_eq!(*err.condition(), FailureCondition::RetiredMigrationSource);

    let mut target_reader = target.open_read().expect("open target reader");
    let migrated = target_reader
        .read_all_envelopes()
        .expect("read target envelopes");
    assert_eq!(migrated.len(), 4);
    assert_eq!(migrated[0].payload, b"chase1");
    assert_eq!(migrated[1].payload, b"chase2");
    assert_eq!(migrated[2].payload, b"chase3");
    assert_eq!(migrated[3].payload, b"chase4");
}

#[cfg(feature = "nats")]
#[test]
fn test_m6_nats_broken_chain_election() {
    let server = pardosa_nats::test_support::LiveNatsServer::acquire();
    let src_stem = format!("m6_nats_brk_src_{}_{}", std::process::id(), 10);
    let dst_stem = format!("m6_nats_brk_dst_{}_{}", std::process::id(), 11);

    let source =
        pardosa_nats::NatsStorageAdapter::new(server.url(), &src_stem).expect("connect src");
    let target =
        pardosa_nats::NatsStorageAdapter::new(server.url(), &dst_stem).expect("connect dst");

    let claim = sample_claim(1);
    let mut writer = source.create(&claim).expect("create source");
    let target_claim = sample_claim(1);
    target.create(&target_claim).expect("create target");

    let env1 = sample_genesis_envelope(1, 0xbb, b"nats_valid1");
    let broken_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [2u8; 16],
            fiber_id: [0xbbu8; 16],
            detached: false,
            precursor: [1u8; 16],
            precursor_hash: [0xffu8; 32],
        },
        payload: b"nats_broken".to_vec(),
    };
    writer.append_envelope(&env1).expect("append env1");
    let mut broken_buf = Vec::new();
    broken_env.encode(&mut broken_buf);
    writer
        .append_unvalidated_frame(&broken_buf)
        .expect("append broken raw frame");

    let mut src_reader = source.open_read().expect("open source reader");
    let src_ordinary_err = src_reader
        .read_all_envelopes()
        .expect_err("ordinary reader on nats source with broken history must refuse");
    assert!(matches!(
        src_ordinary_err.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));

    let manager_refuse = MigrationManager::new(source.clone(), target.clone())
        .with_broken_chain_election(BrokenChainElection::RefuseOnBreak);

    let err = manager_refuse
        .run_all()
        .expect_err("nats migration must refuse broken chain");
    assert!(matches!(
        err.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));
    assert!(!source.is_retired_source().expect("source not retired"));

    let manager_permit = MigrationManager::new(source.clone(), target.clone())
        .with_broken_chain_election(BrokenChainElection::PermitBrokenHistory);

    let summary = manager_permit
        .run_all()
        .expect("nats permit broken history migration");
    assert_eq!(summary.total_migrated_events, 2);

    let mut target_reader = target.open_read().expect("open target reader");
    let target_ordinary_err = target_reader
        .read_all_envelopes()
        .expect_err("ordinary reader on nats target with broken history must refuse");
    assert!(matches!(
        target_ordinary_err.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));
    let migrated = target_reader
        .read_all_envelopes_for_migration()
        .expect("read target envelopes");
    assert_eq!(migrated.len(), 2);
    assert_eq!(migrated[0].header.precursor, [0u8; 16]);
    assert_eq!(migrated[1].header.precursor, [0u8; 16]);

    source.delete_streams().expect("cleanup src");
    target.delete_streams().expect("cleanup dst");
}

#[cfg(feature = "nats")]
#[test]
fn test_m6_nats_policies_and_dense_rechaining() {
    let server = pardosa_nats::test_support::LiveNatsServer::acquire();
    let src_stem = format!("m6_nats_pol_src_{}_{}", std::process::id(), 20);
    let dst_stem = format!("m6_nats_pol_dst_{}_{}", std::process::id(), 21);

    let source =
        pardosa_nats::NatsStorageAdapter::new(server.url(), &src_stem).expect("connect src");
    let target =
        pardosa_nats::NatsStorageAdapter::new(server.url(), &dst_stem).expect("connect dst");

    let claim = sample_claim(1);
    let mut writer = source.create(&claim).expect("create source");
    let target_claim = sample_claim(1);
    target.create(&target_claim).expect("create target");

    let fiber_keep = [0x44u8; 16];
    let fiber_purge = [0x55u8; 16];
    let fiber_lock = [0x66u8; 16];

    let a1 = sample_genesis_envelope(1, 0x44, b"nats_k1");
    let comm_a1 = a1.commitment();
    let a2 = sample_chained_envelope(2, 0x44, 1, comm_a1, b"nats_k2");

    let b1 = sample_genesis_envelope(10, 0x55, b"nats_p1");

    let c1 = sample_genesis_envelope(20, 0x66, b"nats_l1");
    let comm_c1 = c1.commitment();
    let c2 = sample_chained_envelope(21, 0x66, 20, comm_c1, b"nats_l2");

    writer.append_envelope(&a1).expect("append a1");
    writer.append_envelope(&b1).expect("append b1");
    writer.append_envelope(&c1).expect("append c1");
    writer.append_envelope(&a2).expect("append a2");
    writer.append_envelope(&c2).expect("append c2");

    let manager = MigrationManager::new(source.clone(), target.clone())
        .with_fiber_policy(fiber_keep, FiberMigrationPolicy::Keep)
        .with_fiber_policy(fiber_purge, FiberMigrationPolicy::Purge)
        .with_fiber_policy(fiber_lock, FiberMigrationPolicy::LockAndPrune);

    let summary = manager.run_all().expect("run nats policies migration");
    assert_eq!(summary.total_migrated_events, 3);
    assert_eq!(summary.surviving_fibers, 2);

    let mut target_reader = target.open_read().expect("open target reader");
    let migrated = target_reader.read_all_envelopes().expect("read target");
    assert_eq!(migrated.len(), 3);
    assert_eq!(migrated[0].payload, b"nats_k1");
    assert_eq!(migrated[1].payload, b"nats_k2");
    assert_eq!(migrated[2].payload, b"nats_l2");
    assert!(migrated[2].header.detached);

    source.delete_streams().expect("cleanup src");
    target.delete_streams().expect("cleanup dst");
}
