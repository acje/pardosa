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
    let mut writer = source
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create source");
    let target_claim = sample_claim(1);
    target
        .create(&target_claim, &AdmittedDescriptor::default_for_test())
        .expect("create target");

    let env1 = sample_genesis_envelope(1, 0x55, b"nats_e1");
    let comm1 = env1.commitment();
    let env2 = sample_chained_envelope(2, 0x55, 1, comm1, b"nats_e2");
    writer.append_envelope_verdict(&env1).expect("append 1");
    writer.append_envelope_verdict(&env2).expect("append 2");

    let err_mgr = MigrationManager::new(source.clone(), target.clone()).unwrap_err();
    assert_eq!(
        *err_mgr.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(err_mgr.to_string().contains("live migration is disabled in this release per approved constrained release decision; use offline administrative migration"));

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

    assert!(source.is_retired_source().expect("source retired"));

    let err_new = source.open_write(1).expect_err("source must be retired");
    assert_eq!(
        *err_new.condition(),
        FailureCondition::RetiredMigrationSource
    );

    let err_append = writer
        .append_envelope_verdict(&env1)
        .expect_err("existing writer retired");
    assert_eq!(
        *err_append.condition(),
        FailureCondition::RetiredMigrationSource
    );

    let reader = target.open_read().expect("open target reader");
    assert_eq!(
        reader
            .inbound_pointer()
            .unwrap()
            .prior_generation_locator_id,
        source.locator_id()
    );

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
    source
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create source");
    target
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create target");

    let mut writer = source.open_write(1).expect("open write source");
    let env1 = sample_genesis_envelope(1, 0x77, b"cross_data");
    writer.append_envelope_verdict(&env1).expect("append cross");

    let err_mgr = MigrationManager::new(source.clone(), target.clone()).unwrap_err();
    assert_eq!(
        *err_mgr.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(err_mgr.to_string().contains("live migration is disabled in this release per approved constrained release decision; use offline administrative migration"));

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
    let mut writer = source
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create source");
    let target_claim = sample_claim(1);
    target
        .create(&target_claim, &AdmittedDescriptor::default_for_test())
        .expect("create target");

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
    writer.append_envelope_verdict(&env1).expect("append env1");
    let mut broken_buf = Vec::new();
    broken_env.encode(&mut broken_buf).unwrap();
    drop(writer);
    source
        .append_unvalidated_frame_for_test(&broken_buf)
        .expect("append broken raw frame");

    let mut src_reader = source.open_read().expect("open source reader");
    let src_ordinary_err = src_reader
        .read_all_envelopes()
        .expect_err("ordinary reader on nats source with broken history must refuse");
    assert!(matches!(
        src_ordinary_err.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));

    let err_refuse = MigrationManager::new(source.clone(), target.clone()).unwrap_err();
    assert_eq!(
        *err_refuse.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(!source.is_retired_source().expect("source not retired"));

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
    let mut writer = source
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create source");
    let target_claim = sample_claim(1);
    target
        .create(&target_claim, &AdmittedDescriptor::default_for_test())
        .expect("create target");

    let a1 = sample_genesis_envelope(1, 0x44, b"nats_k1");
    let comm_a1 = a1.commitment();
    let a2 = sample_chained_envelope(2, 0x44, 1, comm_a1, b"nats_k2");

    writer.append_envelope_verdict(&a1).expect("append a1");
    writer.append_envelope_verdict(&a2).expect("append a2");

    let err_mgr = MigrationManager::new(source.clone(), target.clone()).unwrap_err();
    assert_eq!(
        *err_mgr.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(err_mgr.to_string().contains("live migration is disabled in this release per approved constrained release decision; use offline administrative migration"));

    source.delete_streams().expect("cleanup src");
    target.delete_streams().expect("cleanup dst");
}
