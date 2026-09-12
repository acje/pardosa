use pardosa::file::FileStorageAdapter;
use pardosa::migration::*;
use pardosa::prelude::*;
use pardosa_nats::test_support::LiveNatsServer;
use pardosa_nats::NatsStorageAdapter;

fn sample_claim(epoch: u64) -> OwnershipClaimRecord {
    OwnershipClaimRecord {
        epoch,
        machine_id: [1u8; 16],
        boot_id: [2u8; 16],
        process_id: 12345,
        process_start_time_ns: 1_000_000,
        claim_time_ns: 2_000_000,
        operator_label: "operator-m6-nats-test".to_string(),
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
fn test_m6_migration_on_nats_adapter() {
    let server = LiveNatsServer::acquire();
    let src_stem = format!("m6_nats_src_{}_{}", std::process::id(), 1);
    let dst_stem = format!("m6_nats_dst_{}_{}", std::process::id(), 2);

    let source = NatsStorageAdapter::new(server.url(), &src_stem).expect("connect src");
    let target = NatsStorageAdapter::new(server.url(), &dst_stem).expect("connect dst");

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

#[test]
fn test_m6_migration_cross_adapter_file_to_nats() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_cross"));

    let server = LiveNatsServer::acquire();
    let dst_stem = format!("m6_cross_dst_{}_{}", std::process::id(), 3);
    let target = NatsStorageAdapter::new(server.url(), &dst_stem).expect("connect dst");

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
fn test_m6_nats_broken_chain_election() {
    let server = LiveNatsServer::acquire();
    let src_stem = format!("m6_nats_brk_src_{}_{}", std::process::id(), 10);
    let dst_stem = format!("m6_nats_brk_dst_{}_{}", std::process::id(), 11);

    let source = NatsStorageAdapter::new(server.url(), &src_stem).expect("connect src");
    let target = NatsStorageAdapter::new(server.url(), &dst_stem).expect("connect dst");

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

#[test]
fn test_m6_nats_policies_and_dense_rechaining() {
    let server = LiveNatsServer::acquire();
    let src_stem = format!("m6_nats_pol_src_{}_{}", std::process::id(), 20);
    let dst_stem = format!("m6_nats_pol_dst_{}_{}", std::process::id(), 21);

    let source = NatsStorageAdapter::new(server.url(), &src_stem).expect("connect src");
    let target = NatsStorageAdapter::new(server.url(), &dst_stem).expect("connect dst");

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
