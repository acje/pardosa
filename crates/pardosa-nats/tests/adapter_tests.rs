use pardosa::prelude::*;
use pardosa_nats::test_support::LiveNatsServer;
use pardosa_nats::NatsStorageAdapter;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

fn sample_claim(epoch: u64) -> OwnershipClaimRecord {
    OwnershipClaimRecord {
        epoch,
        machine_id: [1u8; 16],
        boot_id: [2u8; 16],
        process_id: 12345,
        process_start_time_ns: 1_000_000,
        claim_time_ns: 2_000_000,
        operator_label: "operator-nats-test".to_string(),
    }
}

fn sample_envelope(seq: u64) -> EventEnvelope {
    let mut event_id = [0u8; 16];
    event_id[0..8].copy_from_slice(&seq.to_le_bytes());
    let mut fiber_id = [0u8; 16];
    fiber_id[0..8].copy_from_slice(&seq.to_le_bytes());
    EventEnvelope {
        header: EnvelopeHeader {
            event_id,
            fiber_id,
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: format!("event-seq-{seq}").into_bytes(),
    }
}

fn unique_stem(prefix: &str) -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(1);
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("m5_{prefix}_{}_{count}", std::process::id())
}

#[test]
fn test_nats_strict_create_and_refuse_existing() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("strict_create");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");

    assert_eq!(adapter.stem(), stem);
    assert_eq!(adapter.meta_stream_name(), format!("{stem}_meta"));
    assert_eq!(adapter.data_stream_name(), format!("{stem}_data"));
    assert_eq!(adapter.presence(), ArtefactPresence::None);

    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create should succeed");
    assert_eq!(adapter.presence(), ArtefactPresence::Both);
    assert_eq!(writer.carried_epoch(), 1);
    assert_eq!(writer.rolling_commitment().frame_count(), 0);

    let err = adapter.create(&claim).unwrap_err();
    assert_eq!(err.condition(), &FailureCondition::StoreAlreadyExists);

    let frame_count = writer
        .append_raw_frame(b"first-event")
        .expect("append frame");
    assert_eq!(frame_count, 1);
    assert_eq!(writer.rolling_commitment().frame_count(), 1);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_strict_open_refuses_nonexistent() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("strict_open");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");

    let write_err = adapter.open_write(1).unwrap_err();
    assert_eq!(write_err.condition(), &FailureCondition::NoArtefactExists);

    let read_err = adapter.open_read().unwrap_err();
    assert_eq!(read_err.condition(), &FailureCondition::NoArtefactExists);
}

#[test]
fn test_nats_concurrent_create_exactly_one_winner() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("concurrent_create");
    let num_threads = 8;
    let winners = Arc::new(AtomicUsize::new(0));
    let store_exists = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for thread_idx in 0..num_threads {
        let server_url = server.url().to_string();
        let stem_clone = stem.clone();
        let winners_clone = Arc::clone(&winners);
        let store_exists_clone = Arc::clone(&store_exists);
        handles.push(thread::spawn(move || {
            let adapter = NatsStorageAdapter::new(&server_url, &stem_clone).expect("connect");
            let claim = sample_claim(thread_idx as u64 + 1);
            match adapter.create(&claim) {
                Ok(_writer) => {
                    winners_clone.fetch_add(1, Ordering::SeqCst);
                }
                Err(err) => {
                    if err.condition() == &FailureCondition::StoreAlreadyExists {
                        store_exists_clone.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(winners.load(Ordering::SeqCst), 1);
    assert_eq!(store_exists.load(Ordering::SeqCst), num_threads - 1);

    let cleanup_adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let _ = cleanup_adapter.delete_streams();
}

#[test]
fn test_nats_two_writer_occ_collision() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("two_writer_occ");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer1 = adapter.create(&claim).expect("create writer 1");
    let mut writer2 = adapter.open_write(1).expect("open writer 2");

    let count1 = writer1
        .append_raw_frame(b"writer-1-event-1")
        .expect("writer 1 append");
    assert_eq!(count1, 1);

    let err2 = writer2.append_raw_frame(b"writer-2-event-1").unwrap_err();
    assert_eq!(err2.condition(), &FailureCondition::ConcurrencyConflict);

    let count1_b = writer1
        .append_raw_frame(b"writer-1-event-2")
        .expect("writer 1 second append");
    assert_eq!(count1_b, 2);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_per_landing_epoch_verification_and_stale_epoch() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("stale_epoch");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim1 = sample_claim(1);

    let mut writer1 = adapter.create(&claim1).expect("create writer epoch 1");
    assert_eq!(writer1.carried_epoch(), 1);

    let env1 = EventEnvelope::genesis([0x01; 16], [0xaa; 16], b"event-epoch-1").unwrap();
    let mut buf1 = Vec::new();
    env1.encode(&mut buf1);

    let count1 = writer1.append_frame(&buf1).expect("append under epoch 1");
    assert_eq!(count1, 1);

    let claim2 = sample_claim(2);
    adapter
        .record_ownership_claim(&claim2)
        .expect("record new claim at epoch 2");

    let env_stale = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: env1.header.event_id,
            precursor_hash: env1.commitment(),
        },
        payload: b"stale-event".to_vec(),
    };
    let mut buf_stale = Vec::new();
    env_stale.encode(&mut buf_stale);

    let stale_err = writer1.append_frame(&buf_stale).unwrap_err();
    assert_eq!(stale_err.condition(), &FailureCondition::StaleEpoch);

    let mut writer2 = adapter.open_write(2).expect("open writer at epoch 2");
    assert_eq!(writer2.carried_epoch(), 2);

    let env2 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x03; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: env1.header.event_id,
            precursor_hash: env1.commitment(),
        },
        payload: b"event-epoch-2".to_vec(),
    };
    let mut buf2 = Vec::new();
    env2.encode(&mut buf2);

    let count2 = writer2.append_frame(&buf2).expect("append under epoch 2");
    assert_eq!(count2, 2);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_indeterminate_landing_verdict() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("indeterminate");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");

    let env1 = EventEnvelope::genesis([0x01; 16], [0xaa; 16], b"normal-event").unwrap();
    let mut buf1 = Vec::new();
    env1.encode(&mut buf1);

    let verdict = writer
        .append_frame_verdict(&buf1)
        .expect("normal append verdict");
    assert_eq!(verdict, WriteLandingVerdict::Landed(1));

    let mut indet_writer = writer.with_simulate_indeterminate(true);
    let env2 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: env1.header.event_id,
            precursor_hash: env1.commitment(),
        },
        payload: b"uncertain-event".to_vec(),
    };
    let mut buf2 = Vec::new();
    env2.encode(&mut buf2);
    let indet_verdict = indet_writer
        .append_frame_verdict(&buf2)
        .expect("indeterminate append verdict");
    assert_eq!(
        indet_verdict,
        WriteLandingVerdict::Undetermined { carried_epoch: 1 }
    );
    assert!(indet_writer.uncertain_diagnostic().is_some());

    let mut regular_writer = indet_writer.with_simulate_indeterminate(false);
    let env3 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x03; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: env1.header.event_id,
            precursor_hash: env1.commitment(),
        },
        payload: b"another-normal-event".to_vec(),
    };
    let mut buf3 = Vec::new();
    env3.encode(&mut buf3);
    let err = regular_writer
        .append_frame(&buf3)
        .expect_err("subsequent operations must be rejected while uncertain");
    assert_eq!(
        *err.condition(),
        FailureCondition::OwnershipRecordUnreadable
    );
    assert!(err
        .to_string()
        .contains("writer session in uncertain state; reconciliation required"));

    let mut reconciled_writer = adapter
        .open_write(1)
        .expect("reopen writer after reconciliation");
    let res = reconciled_writer.append_frame(&buf3).expect("landed");
    assert_eq!(res, 2);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_read_only_open_permits_concurrent_readers() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("concurrent_readers");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    for seq in 1..=10 {
        let env = sample_envelope(seq);
        writer.append_envelope(&env).expect("append envelope");
    }
    writer.sync().expect("sync");

    let num_readers = 8;
    let mut reader_handles = Vec::new();
    for _ in 0..num_readers {
        let server_url = server.url().to_string();
        let stem_clone = stem.clone();
        reader_handles.push(thread::spawn(move || {
            let r_adapter = NatsStorageAdapter::new(&server_url, &stem_clone).expect("connect");
            let mut reader = r_adapter.open_read().expect("open read");
            assert_eq!(reader.admission(), &OpenAdmission::Ready);
            let envelopes = reader.read_all_envelopes().expect("read envelopes");
            assert_eq!(envelopes.len(), 10);
            for (idx, env) in envelopes.iter().enumerate() {
                assert_eq!(env.payload, format!("event-seq-{}", idx + 1).into_bytes());
            }
        }));
    }

    for h in reader_handles {
        h.join().unwrap();
    }

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_continuous_rolling_commitment() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("rolling_commitment");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    for seq in 1..=5 {
        let env = sample_envelope(seq);
        writer.append_envelope(&env).expect("append envelope");
    }

    let writer_count = writer.rolling_commitment().frame_count();
    let writer_digest = writer.rolling_commitment().current_commitment();
    assert_eq!(writer_count, 5);

    let mut reader = adapter.open_read().expect("open reader");
    let _ = reader.read_all_frames().expect("read frames");
    assert_eq!(reader.rolling_commitment().frame_count(), 5);
    assert_eq!(
        reader.rolling_commitment().current_commitment(),
        writer_digest
    );

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_incomplete_creation_c5_10() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("incomplete_creation");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    adapter
        .create_incomplete_meta_only(&claim)
        .expect("create incomplete meta");
    assert_eq!(adapter.presence(), ArtefactPresence::OwnershipRecordOnly);

    let reader = adapter.open_read().expect("open read on incomplete");
    assert_eq!(
        reader.admission(),
        &OpenAdmission::IncompleteCreation(IncompleteCreationState::Claimed(claim.clone()))
    );
    assert_eq!(reader.claim(), Some(&claim));

    let wrong_epoch_err = adapter.open_write(999).unwrap_err();
    assert_eq!(wrong_epoch_err.condition(), &FailureCondition::StaleEpoch);

    let mut writer = adapter
        .open_write(1)
        .expect("complete creation via open_write");
    assert_eq!(adapter.presence(), ArtefactPresence::Both);
    let count = writer.append_raw_frame(b"completed-event").expect("append");
    assert_eq!(count, 1);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_complete_creation_validation_and_error_propagation() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("complete_creation_val");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim_a = sample_claim(1);

    adapter
        .create_incomplete_meta_only(&claim_a)
        .expect("create incomplete meta");
    assert_eq!(adapter.presence(), ArtefactPresence::OwnershipRecordOnly);

    let inbound = InboundPointerRecord {
        prior_generation_locator_id: [0x55; 16],
        prior_generation_epoch: 0,
    };
    adapter
        .record_inbound_pointer(&inbound)
        .expect("record inbound pointer on incomplete meta");

    let mut claim_b = claim_a.clone();
    claim_b.epoch = 2;
    claim_b.operator_label = "operator-b".to_string();

    let err_mismatch = adapter
        .complete_creation(&claim_b)
        .expect_err("complete creation with mismatched claim must be refused");
    assert_eq!(
        *err_mismatch.condition(),
        FailureCondition::OwnershipUnestablished
    );

    let reader_after_err = adapter.open_read().expect("open reader after error");
    assert_eq!(reader_after_err.claim(), Some(&claim_a));
    assert_eq!(
        reader_after_err.meta_records().inbound_pointer.as_ref(),
        Some(&inbound)
    );

    let writer = adapter
        .complete_creation(&claim_a)
        .expect("complete creation with matching claim must succeed");
    assert_eq!(writer.claim(), &claim_a);
    assert_eq!(
        writer.meta_records().inbound_pointer.as_ref(),
        Some(&inbound)
    );
    assert_eq!(adapter.presence(), ArtefactPresence::Both);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_complete_creation_corrupt_metadata_refusal() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("complete_creation_corrupt");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim_a = sample_claim(1);

    adapter
        .create_incomplete_meta_only(&claim_a)
        .expect("create incomplete meta");
    assert_eq!(adapter.presence(), ArtefactPresence::OwnershipRecordOnly);

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        let client = async_nats::connect(server.url())
            .await
            .expect("connect async_nats");
        client
            .publish(
                adapter.meta_stream_name().to_string(),
                bytes::Bytes::from_static(b"corrupt_unreadable_metadata_frame_payload"),
            )
            .await
            .expect("publish corrupt frame");
        client.flush().await.expect("flush");
    });

    let err = adapter
        .complete_creation(&claim_a)
        .expect_err("complete creation must refuse when metadata is corrupt or unreadable");
    assert_eq!(
        *err.condition(),
        FailureCondition::OwnershipRecordUnreadable
    );
    assert!(err
        .to_string()
        .contains("failed to decode frame in meta stream"));
    assert_eq!(adapter.presence(), ArtefactPresence::OwnershipRecordOnly);

    let claim_b = sample_claim(2);
    let err_b = adapter
        .complete_creation(&claim_b)
        .expect_err("complete creation must propagate read error with ? before claim evaluation");
    assert_eq!(
        *err_b.condition(),
        FailureCondition::OwnershipRecordUnreadable
    );
    assert_eq!(adapter.presence(), ArtefactPresence::OwnershipRecordOnly);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_interleaved_open_coherent_snapshot() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("interleaved_open");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer1 = adapter.create(&claim).expect("create writer 1");
    let fiber_id = [0x99; 16];
    let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"first").unwrap();
    writer1.append_envelope(&genesis).expect("append 1");

    let mut writer2 = adapter.open_write(1).expect("open writer 2");
    assert_eq!(writer2.last_sequence(), 2);
    let frames2 = writer2.read_all_envelopes().expect("read envelopes");
    assert_eq!(frames2.len(), 1);

    let second = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id,
            detached: false,
            precursor: genesis.header.event_id,
            precursor_hash: genesis.commitment(),
        },
        payload: b"second".to_vec(),
    };
    writer1
        .append_envelope(&second)
        .expect("writer 1 appends second event");
    assert_eq!(writer1.last_sequence(), 3);

    let third = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x03; 16],
            fiber_id,
            detached: false,
            precursor: genesis.header.event_id,
            precursor_hash: genesis.commitment(),
        },
        payload: b"third_from_writer2".to_vec(),
    };
    let occ_err = writer2
        .append_envelope(&third)
        .expect_err("writer 2 must be rejected by OCC conflict due to interleaved append");
    assert_eq!(
        *occ_err.condition(),
        FailureCondition::ConcurrencyConflict,
        "OCC collision must be detected: {:?}",
        occ_err.condition()
    );

    let barrier = Arc::new(std::sync::Barrier::new(2));
    let adapter_clone = adapter.clone();

    let env_a = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x10; 16],
            fiber_id,
            detached: false,
            precursor: second.header.event_id,
            precursor_hash: second.commitment(),
        },
        payload: b"concurrent_a".to_vec(),
    };

    let env_b = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x20; 16],
            fiber_id,
            detached: false,
            precursor: second.header.event_id,
            precursor_hash: second.commitment(),
        },
        payload: b"concurrent_b".to_vec(),
    };

    let handle_a = {
        let barrier = barrier.clone();
        let adapter = adapter_clone.clone();
        let env_a = env_a.clone();
        thread::spawn(move || {
            let mut writer = adapter.open_write(1).expect("thread a open_write");
            assert_eq!(writer.last_sequence(), 3);
            barrier.wait();
            writer.append_envelope(&env_a)
        })
    };

    let handle_b = {
        let barrier = barrier.clone();
        let adapter = adapter_clone;
        let env_b = env_b.clone();
        thread::spawn(move || {
            let mut writer = adapter.open_write(1).expect("thread b open_write");
            assert_eq!(writer.last_sequence(), 3);
            barrier.wait();
            writer.append_envelope(&env_b)
        })
    };

    let res_a = handle_a.join().expect("join thread a");
    let res_b = handle_b.join().expect("join thread b");

    let winner_env = match (res_a, res_b) {
        (Ok(seq_a), Err(err_b))
            if seq_a == 3 && *err_b.condition() == FailureCondition::ConcurrencyConflict =>
        {
            env_a
        }
        (Err(err_a), Ok(seq_b))
            if seq_b == 3 && *err_a.condition() == FailureCondition::ConcurrencyConflict =>
        {
            env_b
        }
        (a, b) => panic!(
            "expected exactly one thread to succeed with Ok(3) and the other to fail with ConcurrencyConflict, got a={:?}, b={:?}",
            a, b
        ),
    };

    let mut writer_reopen = adapter.open_write(1).expect("reopen after concurrent race");
    assert_eq!(writer_reopen.last_sequence(), 4);
    let envelopes_reopen = writer_reopen
        .read_all_envelopes()
        .expect("read envelopes reopen");
    assert_eq!(envelopes_reopen.len(), 3);
    assert_eq!(
        envelopes_reopen[2].header.event_id,
        winner_env.header.event_id
    );
    assert_eq!(envelopes_reopen[2].commitment(), winner_env.commitment());

    let env_reopen = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x21; 16],
            fiber_id,
            detached: false,
            precursor: winner_env.header.event_id,
            precursor_hash: winner_env.commitment(),
        },
        payload: b"reopened_after_occ_conflict".to_vec(),
    };
    let verdict_reopen = writer_reopen
        .append_envelope(&env_reopen)
        .expect("append after reopen");
    assert_eq!(verdict_reopen, 4);
    assert_eq!(writer_reopen.last_sequence(), 5);

    let mut final_reader = adapter.open_read().expect("final read");
    let final_envelopes = final_reader.read_all_envelopes().expect("final envelopes");
    assert_eq!(final_envelopes.len(), 4);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_reader_capability_no_transport_escape() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("reader_no_escape");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    let fiber_id = [0x55; 16];
    let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"first").unwrap();
    writer.append_envelope(&genesis).expect("append");

    let reader = adapter.open_read().expect("open reader");
    assert_eq!(reader.stem(), &stem);
    assert_eq!(reader.meta_stream_name(), format!("{stem}_meta"));
    assert_eq!(reader.data_stream_name(), format!("{stem}_data"));

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_c8_2_schema_completeness() {
    let valid_schema = SchemaDescriptor::new(
        1,
        DescriptorNode::Struct {
            name: "Event".to_string(),
            fields: vec![
                FieldDescriptor {
                    name: "id".to_string(),
                    node: DescriptorNode::U64,
                },
                FieldDescriptor {
                    name: "tag".to_string(),
                    node: DescriptorNode::EventString { max_bytes: 64 },
                },
            ],
        },
    );
    assert!(valid_schema.validate_structural_completeness().is_ok());

    let invalid_zero_version = SchemaDescriptor::new(0, DescriptorNode::U8);
    assert_eq!(
        invalid_zero_version
            .validate_structural_completeness()
            .unwrap_err()
            .condition(),
        &FailureCondition::MissingSchemaDescriptor
    );

    let invalid_empty_enum = SchemaDescriptor::new(
        1,
        DescriptorNode::Enum {
            name: "Empty".to_string(),
            discriminant_width: 1,
            variants: vec![],
        },
    );
    assert_eq!(
        invalid_empty_enum
            .validate_structural_completeness()
            .unwrap_err()
            .condition(),
        &FailureCondition::ValueConstraintViolated {
            constraint: ValueConstraint::Empty,
        }
    );

    let server = LiveNatsServer::acquire();
    let stem = unique_stem("schema_completeness");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    writer
        .set_schema_descriptor(&valid_schema)
        .expect("set valid schema descriptor");

    let reader = adapter.open_read().expect("open reader");
    assert_eq!(reader.schema_descriptor(), Some(&valid_schema));
    assert!(reader.validate_schema_completeness().is_ok());

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_format_vectors_roundtrip() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let vector_path =
        std::path::Path::new(&manifest_dir).join("../../conformance/vectors/envelope.json");
    let content = fs::read_to_string(&vector_path).expect("read envelope.json");
    let json: serde_json::Value = serde_json::from_str(&content).expect("parse json");
    let vectors = json["vectors"].as_array().expect("vectors array");

    let server = LiveNatsServer::acquire();
    let claim = sample_claim(1);
    for (idx, vec) in vectors.iter().enumerate() {
        let expected_outcome = vec["expected_outcome"].as_str().unwrap();
        if expected_outcome == "Success" {
            let bytes_hex = vec["bytes_hex"].as_str().unwrap();
            let bytes = hex::decode(bytes_hex).unwrap();
            let (env, _) = EventEnvelope::decode(&bytes).unwrap();
            let stem = unique_stem(&format!("format_vec_{idx}"));
            let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
            let mut writer = adapter.create(&claim).expect("create writer");
            let mut env_buf = Vec::new();
            env.encode(&mut env_buf);
            writer
                .append_unvalidated_frame(&env_buf)
                .expect("append raw frame");
            drop(writer);

            let mut reader = adapter.open_read().expect("open reader");
            let read_envelopes = reader
                .read_all_envelopes_for_migration()
                .expect("read envelopes");
            assert_eq!(read_envelopes.len(), 1);
            assert_eq!(read_envelopes[0].header, env.header);
            assert_eq!(read_envelopes[0].payload, env.payload);
            adapter.delete_streams().expect("cleanup");
        }
    }
}

#[test]
fn test_nats_adapter_retirement_and_generation_records() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("retired_nats");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    let env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x01; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: b"payload-1".to_vec(),
    };
    writer.append_envelope(&env).expect("append envelope");

    let outbound = OutboundPointerRecord {
        next_generation_locator_id: [42u8; 16],
        cutover_epoch: 1,
    };
    adapter
        .record_outbound_pointer(&outbound)
        .expect("record outbound pointer");

    let err_new_writer = adapter
        .open_write(1)
        .expect_err("new writer must be rejected");
    assert_eq!(
        *err_new_writer.condition(),
        FailureCondition::RetiredMigrationSource
    );

    let err_append = writer
        .append_envelope(&env)
        .expect_err("existing writer append must be rejected");
    assert_eq!(
        *err_append.condition(),
        FailureCondition::RetiredMigrationSource
    );

    let mut reader = adapter.open_read().expect("historical read must succeed");
    assert!(reader.is_retired_source().expect("query retired source"));
    assert_eq!(reader.outbound_pointer(), Some(&outbound));
    let frames = reader.read_all_envelopes().expect("read frames");
    assert_eq!(frames.len(), 1);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_session_fiber_handle_and_point_lookup() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("fiber_kv");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    let fiber_id = [0x77; 16];
    let domain_key = "account:77";
    let derived_id = derive_fiber_id(domain_key);

    assert!(writer.get_latest(fiber_id).expect("get latest").is_none());
    assert!(writer
        .get_latest_with_key(domain_key)
        .expect("get latest with key")
        .is_none());

    let h_initial = writer.fiber(fiber_id).expect("fiber");
    assert_eq!(h_initial.state(), FiberState::Undefined);

    let env1 = match writer
        .append_to_fiber(fiber_id, [0x01; 16], b"nats-payload-1")
        .expect("append to fiber")
    {
        WriteLandingVerdict::Landed(e) => e,
        WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
    };
    assert_eq!(env1.header.fiber_id, fiber_id);
    assert_eq!(env1.header.event_id, [0x01; 16]);

    let latest = writer
        .get_latest(fiber_id)
        .expect("get latest")
        .expect("some");
    assert_eq!(latest.header.event_id, [0x01; 16]);
    assert_eq!(latest.payload, b"nats-payload-1");

    let h_defined = writer.fiber(fiber_id).expect("fiber");
    assert_eq!(h_defined.state(), FiberState::Defined);
    assert_eq!(h_defined.precursor(), [0x01; 16]);
    assert_eq!(h_defined.precursor_hash(), env1.commitment());

    let env_key = match writer
        .append_to_fiber(derived_id, [0x02; 16], b"nats-payload-key")
        .expect("append with key")
    {
        WriteLandingVerdict::Landed(e) => e,
        WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
    };
    let latest_key = writer
        .get_latest_with_key(domain_key)
        .expect("get latest with key")
        .expect("some");
    assert_eq!(latest_key.header.event_id, [0x02; 16]);
    assert_eq!(latest_key.commitment(), env_key.commitment());

    let h_key = writer.fiber_with_key(domain_key).expect("fiber with key");
    assert_eq!(h_key.state(), FiberState::Defined);
    assert_eq!(h_key.fiber_id(), derived_id);

    let env_detach = match writer
        .detach_fiber(fiber_id, [0x03; 16], b"nats-detach")
        .expect("detach fiber")
    {
        WriteLandingVerdict::Landed(e) => e,
        WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
    };
    assert!(env_detach.header.detached);
    let h_detached = writer.fiber(fiber_id).expect("fiber");
    assert_eq!(h_detached.state(), FiberState::Detached);

    let env_rescue = match writer
        .rescue_fiber(fiber_id, [0x04; 16], b"nats-rescue")
        .expect("rescue fiber")
    {
        WriteLandingVerdict::Landed(e) => e,
        WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
    };
    assert!(!env_rescue.header.detached);
    let h_rescued = writer.fiber(fiber_id).expect("fiber");
    assert_eq!(h_rescued.state(), FiberState::Defined);
    assert_eq!(h_rescued.precursor(), [0x04; 16]);

    drop(writer);

    let reader = adapter.open_read().expect("open reader");
    let r_latest = reader
        .get_latest(fiber_id)
        .expect("reader get latest")
        .expect("some");
    assert_eq!(r_latest.header.event_id, [0x04; 16]);

    let r_latest_key = reader
        .get_latest_with_key(domain_key)
        .expect("reader get latest key")
        .expect("some");
    assert_eq!(r_latest_key.header.event_id, [0x02; 16]);

    let r_fiber = reader.fiber(fiber_id).expect("fiber");
    assert_eq!(r_fiber.state(), FiberState::Defined);
    assert_eq!(r_fiber.precursor(), [0x04; 16]);

    let r_fiber_key = reader.fiber_with_key(domain_key).expect("fiber with key");
    assert_eq!(r_fiber_key.state(), FiberState::Defined);
    assert_eq!(r_fiber_key.fiber_id(), derived_id);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_append_to_fiber_rejects_stale_epoch() {
    let server = LiveNatsServer::acquire();
    let stem = format!("stale_fiber_{}_{}", std::process::id(), 99);
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim1 = sample_claim(1);
    let mut writer1 = adapter.create(&claim1).expect("create writer 1");
    let fiber_id = [0x88; 16];

    let verdict1 = writer1
        .append_to_fiber(fiber_id, [0x01; 16], b"first")
        .expect("append 1");
    assert!(matches!(verdict1, WriteLandingVerdict::Landed(_)));

    let claim2 = sample_claim(2);
    adapter
        .record_ownership_claim(&claim2)
        .expect("record claim 2");

    let err = writer1
        .append_to_fiber(fiber_id, [0x02; 16], b"second")
        .expect_err("stale epoch append must be rejected");
    assert_eq!(*err.condition(), FailureCondition::StaleEpoch);

    let writer3 = adapter.open_write(2).expect("open writer 3");
    let mut writer3 = writer3.with_simulate_indeterminate(true);
    let undetermined = writer3
        .append_to_fiber([0x66; 16], [0x11; 16], b"undetermined")
        .expect("undetermined verdict");
    assert_eq!(
        undetermined,
        WriteLandingVerdict::Undetermined { carried_epoch: 2 }
    );

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_append_raw_frame_rejects_stale_epoch() {
    let server = LiveNatsServer::acquire();
    let stem = format!("stale_raw_{}_{}", std::process::id(), 99);
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim1 = sample_claim(1);
    let mut writer1 = adapter.create(&claim1).expect("create writer 1");

    let count1 = writer1.append_raw_frame(b"raw-1").expect("append raw 1");
    assert_eq!(count1, 1);

    let claim2 = sample_claim(2);
    adapter
        .record_ownership_claim(&claim2)
        .expect("record claim 2");

    let err = writer1
        .append_raw_frame(b"raw-2")
        .expect_err("stale epoch append_raw_frame must be rejected");
    assert_eq!(*err.condition(), FailureCondition::StaleEpoch);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_append_unvalidated_frame_rejects_stale_epoch() {
    let server = LiveNatsServer::acquire();
    let stem = format!("stale_unvalidated_{}_{}", std::process::id(), 101);
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim1 = sample_claim(1);
    let mut writer1 = adapter.create(&claim1).expect("create writer 1");

    let count1 = writer1
        .append_unvalidated_frame(b"raw-1")
        .expect("append raw 1");
    assert_eq!(count1, 1);

    let claim2 = sample_claim(2);
    adapter
        .record_ownership_claim(&claim2)
        .expect("record claim 2");

    let err = writer1
        .append_unvalidated_frame(b"raw-2")
        .expect_err("stale epoch append_unvalidated_frame must be rejected");
    assert_eq!(*err.condition(), FailureCondition::StaleEpoch);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_append_frame_rejects_malformed_boolean_discriminant() {
    let server = LiveNatsServer::acquire();
    let stem = format!("malformed_bool_nats_{}", std::process::id());
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");
    let fiber_id = [0x55; 16];
    let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"initial").unwrap();
    let mut bytes = Vec::new();
    genesis.encode(&mut bytes);
    assert!(bytes.len() >= 85);
    bytes[32] = 2;
    let err = writer
        .append_frame(&bytes)
        .expect_err("malformed boolean discriminant in envelope payload must be rejected");
    assert_eq!(*err.condition(), FailureCondition::EnvelopeMismatch);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_reader_retains_broken_slot_after_read_all_envelopes_error() {
    let server = LiveNatsServer::acquire();
    let stem = format!("reader_retains_broken_nats_{}", std::process::id());
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");
    let fiber_id = [0x77; 16];
    let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"initial").unwrap();
    writer.append_envelope(&genesis).expect("append genesis");

    let mut reader = adapter.open_read().expect("open reader");
    let handle = reader.fiber(fiber_id).expect("reader initial point lookup");
    assert_eq!(handle.state(), FiberState::Defined);

    let broken_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id,
            detached: false,
            precursor: [0x99; 16],
            precursor_hash: [0xaa; 32],
        },
        payload: b"broken-precursor".to_vec(),
    };
    let mut broken_bytes = Vec::new();
    broken_env.encode(&mut broken_bytes);
    writer
        .append_unvalidated_frame(&broken_bytes)
        .expect("append raw frame");

    let read_err = reader.read_all_envelopes().unwrap_err();
    assert_eq!(
        *read_err.condition(),
        FailureCondition::PrecursorChainBroken(None)
    );

    let lookup_err = reader
        .fiber(fiber_id)
        .expect_err("reader must retain broken fiber state");
    assert_eq!(
        *lookup_err.condition(),
        FailureCondition::PrecursorChainBroken(None)
    );

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_append_frame_rejects_short_payload_under_85_bytes() {
    let server = LiveNatsServer::acquire();
    let stem = format!("short_payload_nats_{}", std::process::id());
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");
    let short_payload = [0u8; 84];
    let err = writer
        .append_frame(&short_payload)
        .expect_err("84-byte payload must be rejected by append_frame per H2");
    assert_eq!(*err.condition(), FailureCondition::EnvelopeMismatch);
    assert!(err
        .to_string()
        .contains("payload too short for event envelope: 84"));

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_open_write_watermark_binds_to_consumed_sequence() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("h4_watermark_seq");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer1 = adapter.create(&claim).expect("create writer 1");
    let fiber_id = [0x42; 16];
    let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"first").unwrap();
    writer1.append_envelope(&genesis).expect("append 1");
    let child = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id,
            detached: false,
            precursor: genesis.header.event_id,
            precursor_hash: genesis.commitment(),
        },
        payload: b"second".to_vec(),
    };
    writer1.append_envelope(&child).expect("append 2");

    let writer2 = adapter.open_write(1).expect("open writer 2");
    assert_eq!(writer2.last_sequence(), 3);

    drop(writer2);
    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_append_raw_frame_envelope_pre_landing_admission_and_refusal() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("h3_raw_nats");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    let fiber_id = [0x88; 16];
    let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"initial").unwrap();
    let mut gen_bytes = Vec::new();
    genesis.encode(&mut gen_bytes);

    writer
        .append_raw_frame(&gen_bytes)
        .expect("append raw genesis");
    let seq_before = writer.last_sequence();

    let dup_genesis = EventEnvelope::genesis([0x02; 16], fiber_id, b"dup").unwrap();
    let mut dup_bytes = Vec::new();
    dup_genesis.encode(&mut dup_bytes);

    let err = writer.append_raw_frame(&dup_bytes).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::PrecursorChainBroken(None)
    );
    assert_eq!(writer.last_sequence(), seq_before);

    let child = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id,
            detached: false,
            precursor: genesis.header.event_id,
            precursor_hash: genesis.commitment(),
        },
        payload: b"child".to_vec(),
    };
    let mut child_bytes = Vec::new();
    child.encode(&mut child_bytes);

    writer
        .append_raw_frame(&child_bytes)
        .expect("append raw child");
    assert!(writer.last_sequence() > seq_before);
    let handle = writer.fiber(fiber_id).unwrap();
    assert_eq!(handle.event_count(), 2);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_unvalidated_frame_revokes_point_lookup_and_append() {
    let server = LiveNatsServer::acquire();
    let stem = format!("unvalidated_revokes_nats_{}", std::process::id());
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");
    let fiber_id = [0x55; 16];

    writer
        .append_unvalidated_frame(b"unvalidated-nats-frame")
        .expect("append unvalidated frame");

    let err_latest = writer.get_latest(fiber_id).unwrap_err();
    assert_eq!(*err_latest.condition(), FailureCondition::EnvelopeMismatch);
    assert!(err_latest
        .to_string()
        .contains("session contains unindexed raw frames; point lookup unavailable"));

    let err_fiber = writer.fiber(fiber_id).unwrap_err();
    assert_eq!(*err_fiber.condition(), FailureCondition::EnvelopeMismatch);
    assert!(err_fiber
        .to_string()
        .contains("session contains unindexed raw frames; point lookup unavailable"));

    let err_append_fiber = writer
        .append_to_fiber(fiber_id, [0x01; 16], b"payload")
        .unwrap_err();
    assert_eq!(
        *err_append_fiber.condition(),
        FailureCondition::EnvelopeMismatch
    );

    let env = EventEnvelope::genesis([0x02; 16], fiber_id, b"envelope").unwrap();
    let err_append_env = writer.append_envelope(&env).unwrap_err();
    assert_eq!(
        *err_append_env.condition(),
        FailureCondition::EnvelopeMismatch
    );
    assert!(err_append_env
        .to_string()
        .contains("session contains unindexed raw frames; append unavailable"));

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_writer_retains_uncertain_diagnostic_on_undetermined_landing() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("uncertain_diag");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    assert_eq!(writer.uncertain_diagnostic(), None);

    let env1 = EventEnvelope::genesis([0x01; 16], [0xaa; 16], b"normal-event").unwrap();
    let mut buf1 = Vec::new();
    env1.encode(&mut buf1);
    writer
        .append_frame_verdict(&buf1)
        .expect("normal append verdict");
    assert_eq!(writer.uncertain_diagnostic(), None);

    let mut indet_writer = writer.with_simulate_indeterminate(true);
    let env2 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: env1.header.event_id,
            precursor_hash: env1.commitment(),
        },
        payload: b"uncertain-event".to_vec(),
    };
    let mut buf2 = Vec::new();
    env2.encode(&mut buf2);
    let verdict = indet_writer
        .append_frame_verdict(&buf2)
        .expect("indeterminate verdict");
    assert_eq!(
        verdict,
        WriteLandingVerdict::Undetermined { carried_epoch: 1 }
    );
    let diag = indet_writer
        .uncertain_diagnostic()
        .expect("uncertain diagnostic must be retained on undetermined landing");
    assert!(diag.contains("undetermined"));

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_presend_refusal_does_not_poison_session() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("presend_refusal");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");
    assert_eq!(writer.uncertain_diagnostic(), None);

    let env1 = EventEnvelope::genesis([0x01; 16], [0xaa; 16], b"normal-event").unwrap();
    let mut buf1 = Vec::new();
    env1.encode(&mut buf1);
    writer
        .append_frame_verdict(&buf1)
        .expect("normal append verdict");
    assert_eq!(writer.uncertain_diagnostic(), None);

    let oversized_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: env1.header.event_id,
            precursor_hash: env1.commitment(),
        },
        payload: vec![0x42; 2 * 1024 * 1024],
    };
    let mut oversized_buf = Vec::new();
    oversized_env.encode(&mut oversized_buf);
    let _err = writer
        .append_frame_verdict(&oversized_buf)
        .expect_err("oversized envelope must be refused pre-send");
    assert_eq!(writer.uncertain_diagnostic(), None);

    let env3 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x03; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: env1.header.event_id,
            precursor_hash: env1.commitment(),
        },
        payload: b"recovery-event".to_vec(),
    };
    let mut buf3 = Vec::new();
    env3.encode(&mut buf3);
    writer
        .append_frame_verdict(&buf3)
        .expect("subsequent small valid append must succeed after pre-send refusal");
    assert_eq!(writer.uncertain_diagnostic(), None);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_metadata_presend_refusal_does_not_poison_session() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("metadata_presend_refusal");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");
    assert_eq!(writer.uncertain_diagnostic(), None);

    let env1 = EventEnvelope::genesis([0x01; 16], [0xaa; 16], b"normal-event").unwrap();
    let mut buf1 = Vec::new();
    env1.encode(&mut buf1);
    let verdict1 = writer
        .append_frame_verdict(&buf1)
        .expect("normal append verdict");
    assert!(matches!(verdict1, WriteLandingVerdict::Landed(_)));
    assert_eq!(writer.uncertain_diagnostic(), None);

    let oversized_record = OwnershipRecord::SchemaDescriptor {
        schema_version: 1,
        descriptor_bytes: vec![0x42; 2 * 1024 * 1024],
    };
    let err = writer
        .record_ownership_record(&oversized_record)
        .expect_err("oversized metadata must be refused pre-send");
    assert_eq!(
        *err.condition(),
        FailureCondition::OwnershipRecordUnreadable
    );
    assert!(
        err.to_string().contains("max payload size exceeded")
            || err.to_string().contains("failed to publish meta frame")
    );
    assert_eq!(
        writer.uncertain_diagnostic(),
        None,
        "pre-send metadata refusal must not poison writer session"
    );

    let env2 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: env1.header.event_id,
            precursor_hash: env1.commitment(),
        },
        payload: b"recovery-event".to_vec(),
    };
    let mut buf2 = Vec::new();
    env2.encode(&mut buf2);
    let verdict2 = writer
        .append_frame_verdict(&buf2)
        .expect("subsequent small valid append must succeed after pre-send metadata refusal");
    assert!(matches!(verdict2, WriteLandingVerdict::Landed(_)));
    assert_eq!(writer.uncertain_diagnostic(), None);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_metadata_authority_checks_and_borrowed_claim() {
    let server = LiveNatsServer::acquire();
    let stem = format!("meta_auth_nats_{}", std::process::id());
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");

    let borrowed_claim: &OwnershipClaimRecord = writer.claim();
    assert_eq!(borrowed_claim.epoch, 1);
    assert_eq!(
        writer.meta_records().latest_claim.as_ref().map(|c| c.epoch),
        Some(1)
    );

    let claim2 = sample_claim(2);
    adapter
        .record_ownership_record(&OwnershipRecord::OwnershipClaim(claim2))
        .expect("append epoch 2");

    let choice = RescuePolicyChoiceRecord {
        policy_tag: 0,
        parameter_payload: vec![],
    };
    let err_meta = writer
        .record_ownership_record(&OwnershipRecord::RescuePolicyChoice(choice))
        .unwrap_err();
    assert_eq!(*err_meta.condition(), FailureCondition::StaleEpoch);

    let descriptor = SchemaDescriptor::new(1, DescriptorNode::U64);
    let err_schema = writer.set_schema_descriptor(&descriptor).unwrap_err();
    assert_eq!(*err_schema.condition(), FailureCondition::StaleEpoch);

    let sync_err = writer.sync().unwrap_err();
    assert_eq!(*sync_err.condition(), FailureCondition::StaleEpoch);

    let reader = adapter.open_read().expect("open reader");
    let claim_opt: Option<&OwnershipClaimRecord> = reader.claim();
    assert_eq!(claim_opt.map(|c| c.epoch), Some(2));

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_read_all_frames_does_not_advance_occ_watermark() {
    let server = LiveNatsServer::acquire();
    let stem = format!("occ_sync_nats_{}", std::process::id());
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim = sample_claim(1);
    let mut writer_a = adapter.create(&claim).expect("create writer A");
    let mut writer_b = adapter.open_write(1).expect("open writer B");

    let fiber_id = [0xbb; 16];
    let genesis_a = EventEnvelope::genesis([0x01; 16], fiber_id, b"genesis_a").unwrap();
    writer_a
        .append_envelope(&genesis_a)
        .expect("writer A appends genesis");

    let frames = writer_b.read_all_frames().expect("writer B reads frames");
    assert_eq!(frames.len(), 1);

    let genesis_b = EventEnvelope::genesis([0x02; 16], fiber_id, b"genesis_b").unwrap();
    let err_b = writer_b.append_envelope(&genesis_b).unwrap_err();
    assert_eq!(*err_b.condition(), FailureCondition::ConcurrencyConflict);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_migration_read_rejects_malformed_envelope() {
    let server = LiveNatsServer::acquire();
    let stem = format!("migration_malformed_nats_{}", std::process::id());
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");
    writer
        .append_unvalidated_frame(b"short-payload")
        .expect("append short frame");

    let mut reader = adapter.open_read().expect("open reader");
    let err = reader.read_all_envelopes_for_migration().unwrap_err();
    assert_eq!(*err.condition(), FailureCondition::EnvelopeMismatch);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_replay_constants_and_contract() {
    use pardosa_nats::adapter::{MAX_CONCURRENT_FETCHES, MAX_REPLAY_BYTES};
    assert_eq!(MAX_CONCURRENT_FETCHES, 32);
    assert_eq!(MAX_REPLAY_BYTES, 64 * 1024 * 1024);
}

#[test]
fn test_nats_replay_bounded_batching_exact_order_and_commitment() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("replay_batching_order");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");

    let frame_count = 75u64;
    let mut expected_payloads = Vec::with_capacity(frame_count as usize);

    for seq in 1..=frame_count {
        let env = sample_envelope(seq);
        let mut encoded = Vec::new();
        env.encode(&mut encoded);
        let appended_seq = writer.append_envelope(&env).expect("append envelope");
        assert_eq!(appended_seq, seq);
        expected_payloads.push(encoded);
    }

    let initial_commitment = writer.rolling_commitment().clone();
    assert_eq!(initial_commitment.frame_count(), frame_count);

    let mut replayed_writer = adapter.open_write(1).expect("open write replay");
    assert_eq!(
        replayed_writer.rolling_commitment().frame_count(),
        frame_count
    );
    assert_eq!(
        replayed_writer.rolling_commitment().current_commitment(),
        initial_commitment.current_commitment()
    );

    let replayed_frames = replayed_writer.read_all_frames().expect("read frames");
    assert_eq!(replayed_frames.len(), frame_count as usize);
    for (idx, (actual, expected)) in replayed_frames
        .iter()
        .zip(expected_payloads.iter())
        .enumerate()
    {
        assert_eq!(actual, expected, "frame mismatch at index {idx}");
    }

    let mut reader = adapter.open_read().expect("open read");
    let reader_frames = reader.read_all_frames().expect("reader frames");
    assert_eq!(reader_frames.len(), frame_count as usize);
    for (idx, (actual, expected)) in reader_frames
        .iter()
        .zip(expected_payloads.iter())
        .enumerate()
    {
        assert_eq!(actual, expected, "reader frame mismatch at index {idx}");
    }

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_nats_replay_capacity_exhaustion_refusal() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("replay_cap_refusal");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");

    for _seq in 1..=10 {
        let payload = vec![0xaa; 100];
        writer.append_raw_frame(&payload).expect("append frame");
    }

    let rt = adapter.runtime().clone();
    let js = adapter.jetstream().clone();
    let data_stream = adapter.data_stream_name().to_string();

    let err = rt
        .block_on(async {
            pardosa_nats::adapter::read_data_frames_async_bounded(&js, &data_stream, 32, 500).await
        })
        .expect_err("500 byte limit must be refused on 1000 byte total stream");

    assert_eq!(*err.condition(), FailureCondition::RefusalDueToCapacity);
    assert!(err
        .diagnostic_detail()
        .message()
        .contains("replay byte limit exceeded"));

    adapter.delete_streams().expect("cleanup");
}
