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
    EventEnvelope {
        header: EnvelopeHeader {
            event_id,
            fiber_id: [1u8; 16],
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

    let frame_count = writer.append_frame(b"first-event").expect("append frame");
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
        .append_frame(b"writer-1-event-1")
        .expect("writer 1 append");
    assert_eq!(count1, 1);

    let err2 = writer2.append_frame(b"writer-2-event-1").unwrap_err();
    assert_eq!(err2.condition(), &FailureCondition::ConcurrencyConflict);

    let count1_b = writer1
        .append_frame(b"writer-1-event-2")
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

    let count1 = writer1
        .append_frame(b"event-epoch-1")
        .expect("append under epoch 1");
    assert_eq!(count1, 1);

    let claim2 = sample_claim(2);
    adapter
        .record_ownership_claim(&claim2)
        .expect("record new claim at epoch 2");

    let stale_err = writer1.append_frame(b"stale-event").unwrap_err();
    assert_eq!(stale_err.condition(), &FailureCondition::StaleEpoch);

    let mut writer2 = adapter.open_write(2).expect("open writer at epoch 2");
    assert_eq!(writer2.carried_epoch(), 2);
    let count2 = writer2
        .append_frame(b"event-epoch-2")
        .expect("append under epoch 2");
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

    let verdict = writer
        .append_frame_verdict(b"normal-event")
        .expect("normal append verdict");
    assert_eq!(verdict, WriteLandingVerdict::Landed(1));

    let mut indet_writer = writer.with_simulate_indeterminate(true);
    let indet_verdict = indet_writer
        .append_frame_verdict(b"uncertain-event")
        .expect("indeterminate append verdict");
    assert_eq!(
        indet_verdict,
        WriteLandingVerdict::Undetermined { carried_epoch: 1 }
    );

    let mut regular_writer = indet_writer.with_simulate_indeterminate(false);
    let res = regular_writer
        .append_frame(b"another-normal-event")
        .expect("landed");
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
    let count = writer.append_frame(b"completed-event").expect("append");
    assert_eq!(count, 1);

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
    let stem = unique_stem("format_vectors");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    let mut expected_envelopes = Vec::new();

    for vec in vectors {
        let expected_outcome = vec["expected_outcome"].as_str().unwrap();
        if expected_outcome == "Success" {
            let bytes_hex = vec["bytes_hex"].as_str().unwrap();
            let bytes = hex::decode(bytes_hex).unwrap();
            let (env, _) = EventEnvelope::decode(&bytes).unwrap();
            writer.append_envelope(&env).expect("append envelope");
            expected_envelopes.push(env);
        }
    }

    let mut reader = adapter.open_read().expect("open reader");
    let read_envelopes = reader.read_all_envelopes().expect("read envelopes");
    assert_eq!(read_envelopes.len(), expected_envelopes.len());
    for (read, exp) in read_envelopes.iter().zip(expected_envelopes.iter()) {
        assert_eq!(read.header, exp.header);
        assert_eq!(read.payload, exp.payload);
    }

    adapter.delete_streams().expect("cleanup");
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
        payload: b"sample nats payload".to_vec(),
    };
    writer.append_envelope(&env).expect("append envelope");

    let outbound = OutboundPointerRecord {
        next_generation_locator_id: [99u8; 16],
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
    assert!(reader.is_retired_source());
    assert_eq!(reader.outbound_pointer(), Some(&outbound));
    let frames = reader.read_all_envelopes().expect("read frames");
    assert_eq!(frames.len(), 1);

    adapter.delete_streams().expect("cleanup");
}
