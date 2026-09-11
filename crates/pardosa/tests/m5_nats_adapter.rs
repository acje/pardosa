#![cfg(feature = "nats")]

use pardosa::prelude::*;
use pardosa_nats::test_support::LiveNatsServer;
use pardosa_nats::NatsStorageAdapter;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

fn sample_claim(epoch: u64) -> OwnershipClaimRecord {
    OwnershipClaimRecord {
        epoch,
        machine_id: [1u8; 16],
        boot_id: [2u8; 16],
        process_id: 12345,
        process_start_time_ns: 1_000_000,
        claim_time_ns: 2_000_000,
        operator_label: "operator-m5-test".to_string(),
    }
}

fn unique_stem(prefix: &str) -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(1);
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("m5_facade_{prefix}_{}_{count}", std::process::id())
}

#[test]
fn test_m5_c8_2_schema_completeness_on_nats_adapter() {
    let valid_schema = SchemaDescriptor::new(
        1,
        DescriptorNode::Struct {
            name: "OrderPlaced".to_string(),
            fields: vec![
                FieldDescriptor {
                    name: "order_id".to_string(),
                    node: DescriptorNode::U64,
                },
                FieldDescriptor {
                    name: "customer_id".to_string(),
                    node: DescriptorNode::U64,
                },
                FieldDescriptor {
                    name: "status".to_string(),
                    node: DescriptorNode::Enum {
                        name: "OrderStatus".to_string(),
                        discriminant_width: 1,
                        variants: vec![
                            VariantDescriptor {
                                discriminant: 0,
                                name: "Pending".to_string(),
                                payload: None,
                            },
                            VariantDescriptor {
                                discriminant: 1,
                                name: "Completed".to_string(),
                                payload: None,
                            },
                        ],
                    },
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

    let invalid_zero_bound_string =
        SchemaDescriptor::new(1, DescriptorNode::EventString { max_bytes: 0 });
    assert_eq!(
        invalid_zero_bound_string
            .validate_structural_completeness()
            .unwrap_err()
            .condition(),
        &FailureCondition::ValueConstraintViolated {
            constraint: ValueConstraint::Empty,
        }
    );

    let server = LiveNatsServer::acquire();
    let stem = unique_stem("schema_adapter");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect adapter");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    writer
        .set_schema_descriptor(&valid_schema)
        .expect("set schema descriptor");
    drop(writer);

    let reader = adapter.open_read().expect("open read");
    assert_eq!(reader.schema_descriptor(), Some(&valid_schema));
    assert!(reader.validate_schema_completeness().is_ok());

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_m5_format_vectors_roundtrip_on_nats_adapter() {
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
fn test_m5_nats_strict_create_and_open() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("strict_ops");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");

    assert_eq!(
        adapter.open_write(1).unwrap_err().condition(),
        &FailureCondition::NoArtefactExists
    );
    assert_eq!(
        adapter.open_read().unwrap_err().condition(),
        &FailureCondition::NoArtefactExists
    );

    let claim = sample_claim(1);
    let writer = adapter.create(&claim).expect("create writer");
    assert_eq!(writer.carried_epoch(), 1);

    let dup_err = adapter.create(&claim).unwrap_err();
    assert_eq!(dup_err.condition(), &FailureCondition::StoreAlreadyExists);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_m5_nats_two_writer_occ_exclusion() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("occ_exclusion");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim = sample_claim(1);

    let mut writer1 = adapter.create(&claim).expect("create writer 1");
    let mut writer2 = adapter.open_write(1).expect("open writer 2");

    let count1 = writer1
        .append_raw_frame(b"event-from-w1")
        .expect("w1 append");
    assert_eq!(count1, 1);

    let conflict = writer2.append_raw_frame(b"event-from-w2").unwrap_err();
    assert_eq!(conflict.condition(), &FailureCondition::ConcurrencyConflict);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_m5_nats_per_landing_epoch_verification() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("epoch_fence");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim1 = sample_claim(1);

    let mut writer1 = adapter.create(&claim1).expect("create writer 1");
    let _ = writer1.append_raw_frame(b"event-1").expect("append 1");

    let claim2 = sample_claim(2);
    adapter
        .record_ownership_claim(&claim2)
        .expect("supersede with epoch 2");

    let stale = writer1.append_raw_frame(b"event-2").unwrap_err();
    assert_eq!(stale.condition(), &FailureCondition::StaleEpoch);

    adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_m5_nats_indeterminate_landing_verdict() {
    let server = LiveNatsServer::acquire();
    let stem = unique_stem("indeterminate_verdict");
    let adapter = NatsStorageAdapter::new(server.url(), &stem).expect("connect");
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    let env1 = EventEnvelope::genesis([0x01; 16], [0xaa; 16], b"event-1").unwrap();
    let mut buf1 = Vec::new();
    env1.encode(&mut buf1);
    let landed = writer.append_frame_verdict(&buf1).expect("verdict");
    assert_eq!(landed, WriteLandingVerdict::Landed(1));

    let mut writer_sim = writer.with_simulate_indeterminate(true);
    let env2 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: [0x01; 16],
            precursor_hash: env1.commitment(),
        },
        payload: b"event-2".to_vec(),
    };
    let mut buf2 = Vec::new();
    env2.encode(&mut buf2);
    let indet = writer_sim.append_frame_verdict(&buf2).expect("verdict");
    assert_eq!(
        indet,
        WriteLandingVerdict::Undetermined { carried_epoch: 1 }
    );

    adapter.delete_streams().expect("cleanup");
}
