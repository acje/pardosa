use pardosa::file::FileStorageAdapter;
use pardosa::prelude::*;
use pardosa_nats::test_support::LiveNatsServer;
use pardosa_nats::NatsStorageAdapter;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(prefix: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(1);
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "pardosa_m7_test_{}_{}_{}",
            std::process::id(),
            prefix,
            count
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create test dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn sample_claim(epoch: u64) -> OwnershipClaimRecord {
    OwnershipClaimRecord {
        epoch,
        machine_id: [1u8; 16],
        boot_id: [2u8; 16],
        process_id: 12345,
        process_start_time_ns: 1_000_000,
        claim_time_ns: 2_000_000,
        operator_label: "operator-m7-assurance".to_string(),
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

fn unique_nats_stem(prefix: &str) -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(1);
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("m7_assurance_{prefix}_{}_{count}", std::process::id())
}

#[test]
fn test_m7_symmetric_schema_completeness() {
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

    let dir = TestDir::new("sym_schema_file");
    let file_adapter = FileStorageAdapter::new(dir.path().join("file_schema_store"));
    let claim = sample_claim(1);

    let mut file_writer = file_adapter.create(&claim).expect("create file writer");
    file_writer
        .set_schema_descriptor(&valid_schema)
        .expect("set schema file");
    drop(file_writer);

    let file_reader = file_adapter.open_read().expect("open file reader");
    assert_eq!(file_reader.schema_descriptor(), Some(&valid_schema));
    assert!(file_reader.validate_schema_completeness().is_ok());

    let server = LiveNatsServer::acquire();
    let nats_stem = unique_nats_stem("sym_schema_nats");
    let nats_adapter =
        NatsStorageAdapter::new(server.url(), &nats_stem).expect("connect nats adapter");

    let mut nats_writer = nats_adapter.create(&claim).expect("create nats writer");
    nats_writer
        .set_schema_descriptor(&valid_schema)
        .expect("set schema nats");
    drop(nats_writer);

    let nats_reader = nats_adapter.open_read().expect("open nats reader");
    assert_eq!(nats_reader.schema_descriptor(), Some(&valid_schema));
    assert!(nats_reader.validate_schema_completeness().is_ok());

    nats_adapter.delete_streams().expect("cleanup nats");
}

#[test]
fn test_m7_symmetric_format_vectors_roundtrip() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let vector_path =
        std::path::Path::new(&manifest_dir).join("../../conformance/vectors/envelope.json");
    let content = fs::read_to_string(&vector_path).expect("read envelope.json");
    let json: serde_json::Value = serde_json::from_str(&content).expect("parse json");
    let vectors = json["vectors"].as_array().expect("vectors array");

    let mut valid_envelopes = Vec::new();
    for vec in vectors {
        let expected_outcome = vec["expected_outcome"].as_str().unwrap();
        if expected_outcome == "Success" {
            let bytes_hex = vec["bytes_hex"].as_str().unwrap();
            let bytes = hex::decode(bytes_hex).unwrap();
            let (env, _) = EventEnvelope::decode(&bytes).unwrap();
            valid_envelopes.push(env);
        }
    }
    assert!(!valid_envelopes.is_empty());

    let claim = sample_claim(1);

    let dir = TestDir::new("sym_vectors_file");
    let file_adapter = FileStorageAdapter::new(dir.path().join("vectors_store"));
    let mut file_writer = file_adapter.create(&claim).expect("create file writer");
    for env in &valid_envelopes {
        let mut env_buf = Vec::new();
        env.encode(&mut env_buf);
        file_writer
            .append_unvalidated_frame(&env_buf)
            .expect("append file raw frame");
    }
    drop(file_writer);

    let mut file_reader = file_adapter.open_read().expect("open file reader");
    let file_read_envelopes = file_reader
        .read_all_envelopes_for_migration()
        .expect("read file envelopes");
    assert_eq!(file_read_envelopes.len(), valid_envelopes.len());
    for (read, exp) in file_read_envelopes.iter().zip(valid_envelopes.iter()) {
        assert_eq!(read.header, exp.header);
        assert_eq!(read.payload, exp.payload);
    }

    let server = LiveNatsServer::acquire();
    let nats_stem = unique_nats_stem("sym_vectors_nats");
    let nats_adapter =
        NatsStorageAdapter::new(server.url(), &nats_stem).expect("connect nats adapter");
    let mut nats_writer = nats_adapter.create(&claim).expect("create nats writer");
    for env in &valid_envelopes {
        let mut env_buf = Vec::new();
        env.encode(&mut env_buf);
        nats_writer
            .append_unvalidated_frame(&env_buf)
            .expect("append nats raw frame");
    }
    drop(nats_writer);

    let mut nats_reader = nats_adapter.open_read().expect("open nats reader");
    let nats_read_envelopes = nats_reader
        .read_all_envelopes_for_migration()
        .expect("read nats envelopes");
    assert_eq!(nats_read_envelopes.len(), valid_envelopes.len());
    for (read, exp) in nats_read_envelopes.iter().zip(valid_envelopes.iter()) {
        assert_eq!(read.header, exp.header);
        assert_eq!(read.payload, exp.payload);
    }

    assert_eq!(file_read_envelopes, nats_read_envelopes);
    nats_adapter.delete_streams().expect("cleanup nats");
}

#[test]
fn test_m7_symmetric_strict_create_and_open_refusal() {
    let claim = sample_claim(1);

    let dir = TestDir::new("sym_strict_file");
    let file_nonexistent = FileStorageAdapter::new(dir.path().join("nonexistent_file"));
    let file_write_err = file_nonexistent.open_write(1).unwrap_err();
    let file_read_err = file_nonexistent.open_read().unwrap_err();

    let server = LiveNatsServer::acquire();
    let nats_stem_nonexistent = unique_nats_stem("sym_strict_nonexistent");
    let nats_nonexistent =
        NatsStorageAdapter::new(server.url(), &nats_stem_nonexistent).expect("connect nats");
    let nats_write_err = nats_nonexistent.open_write(1).unwrap_err();
    let nats_read_err = nats_nonexistent.open_read().unwrap_err();

    assert_eq!(
        file_write_err.condition(),
        &FailureCondition::NoArtefactExists
    );
    assert_eq!(
        file_read_err.condition(),
        &FailureCondition::NoArtefactExists
    );
    assert_eq!(
        nats_write_err.condition(),
        &FailureCondition::NoArtefactExists
    );
    assert_eq!(
        nats_read_err.condition(),
        &FailureCondition::NoArtefactExists
    );
    assert_eq!(file_write_err.condition(), nats_write_err.condition());
    assert_eq!(file_read_err.condition(), nats_read_err.condition());

    let file_store_path = dir.path().join("strict_file_store");
    let file_adapter = FileStorageAdapter::new(&file_store_path);
    let file_writer = file_adapter.create(&claim).expect("create file");
    assert_eq!(file_writer.carried_epoch(), 1);
    let file_dup_err = file_adapter.create(&claim).unwrap_err();

    let nats_stem = unique_nats_stem("sym_strict_store");
    let nats_adapter = NatsStorageAdapter::new(server.url(), &nats_stem).expect("connect nats");
    let nats_writer = nats_adapter.create(&claim).expect("create nats");
    assert_eq!(nats_writer.carried_epoch(), 1);
    let nats_dup_err = nats_adapter.create(&claim).unwrap_err();

    assert_eq!(
        file_dup_err.condition(),
        &FailureCondition::StoreAlreadyExists
    );
    assert_eq!(
        nats_dup_err.condition(),
        &FailureCondition::StoreAlreadyExists
    );
    assert_eq!(file_dup_err.condition(), nats_dup_err.condition());

    nats_adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_m7_dimension_1_concurrent_create_and_incomplete_creation() {
    let claim = sample_claim(1);

    let dir = TestDir::new("dim1_concurrent_file");
    let store_path = dir.path().join("raced_file");
    let num_threads = 8;
    let file_winners = Arc::new(AtomicUsize::new(0));
    let file_already_exists = Arc::new(AtomicUsize::new(0));
    let mut file_handles = Vec::new();

    for _ in 0..num_threads {
        let path_clone = store_path.clone();
        let winners = Arc::clone(&file_winners);
        let exists = Arc::clone(&file_already_exists);
        file_handles.push(thread::spawn(move || {
            let adapter = FileStorageAdapter::new(&path_clone);
            let claim = sample_claim(1);
            match adapter.create(&claim) {
                Ok(_) => {
                    winners.fetch_add(1, Ordering::SeqCst);
                }
                Err(err) if err.condition() == &FailureCondition::StoreAlreadyExists => {
                    exists.fetch_add(1, Ordering::SeqCst);
                }
                Err(err) => panic!("unexpected file create error: {:?}", err),
            }
        }));
    }
    for h in file_handles {
        h.join().expect("join file thread");
    }
    assert_eq!(file_winners.load(Ordering::SeqCst), 1);
    assert_eq!(file_already_exists.load(Ordering::SeqCst), num_threads - 1);

    let server = LiveNatsServer::acquire();
    let nats_stem = unique_nats_stem("dim1_concurrent_nats");
    let nats_winners = Arc::new(AtomicUsize::new(0));
    let nats_already_exists = Arc::new(AtomicUsize::new(0));
    let mut nats_handles = Vec::new();

    for _ in 0..num_threads {
        let url = server.url().to_string();
        let stem = nats_stem.clone();
        let winners = Arc::clone(&nats_winners);
        let exists = Arc::clone(&nats_already_exists);
        nats_handles.push(thread::spawn(move || {
            let adapter = NatsStorageAdapter::new(&url, &stem).expect("connect nats");
            let claim = sample_claim(1);
            match adapter.create(&claim) {
                Ok(_) => {
                    winners.fetch_add(1, Ordering::SeqCst);
                }
                Err(err) if err.condition() == &FailureCondition::StoreAlreadyExists => {
                    exists.fetch_add(1, Ordering::SeqCst);
                }
                Err(err) => panic!("unexpected nats create error: {:?}", err),
            }
        }));
    }
    for h in nats_handles {
        h.join().expect("join nats thread");
    }
    assert_eq!(nats_winners.load(Ordering::SeqCst), 1);
    assert_eq!(nats_already_exists.load(Ordering::SeqCst), num_threads - 1);

    let file_incomplete_path = dir.path().join("incomplete_file");
    let file_incomplete_adapter = FileStorageAdapter::new(&file_incomplete_path);
    assert_eq!(file_incomplete_adapter.presence(), ArtefactPresence::None);
    file_incomplete_adapter
        .create_incomplete_meta_only(&claim)
        .expect("create file meta-only");
    assert_eq!(
        file_incomplete_adapter.presence(),
        ArtefactPresence::OwnershipRecordOnly
    );
    let file_reader = file_incomplete_adapter
        .open_read()
        .expect("open read on incomplete file");
    assert!(matches!(
        file_reader.admission(),
        OpenAdmission::IncompleteCreation(_)
    ));
    let mut file_completed = file_incomplete_adapter
        .complete_creation(&claim)
        .expect("complete file creation");
    assert_eq!(file_incomplete_adapter.presence(), ArtefactPresence::Both);
    file_completed
        .append_raw_frame(b"completed-frame")
        .expect("append after completion");
    drop(file_completed);

    let orphan_path = dir.path().join("orphan_file");
    let orphan_adapter = FileStorageAdapter::new(&orphan_path);
    let mut orphan_writer = orphan_adapter.create(&claim).expect("create orphan store");
    orphan_writer
        .append_raw_frame(b"orphan-frame")
        .expect("append orphan frame");
    drop(orphan_writer);
    fs::remove_file(orphan_adapter.meta_path()).expect("remove meta to make orphan");
    assert_eq!(orphan_adapter.presence(), ArtefactPresence::EventDataOnly);
    let file_orphan_err = orphan_adapter.open_write(1).unwrap_err();
    assert_eq!(
        file_orphan_err.condition(),
        &FailureCondition::OwnershipUnestablished
    );

    let nats_incomplete_stem = unique_nats_stem("dim1_incomplete_nats");
    let nats_incomplete_adapter =
        NatsStorageAdapter::new(server.url(), &nats_incomplete_stem).expect("connect nats");
    assert_eq!(nats_incomplete_adapter.presence(), ArtefactPresence::None);
    nats_incomplete_adapter
        .create_incomplete_meta_only(&claim)
        .expect("create nats meta-only");
    assert_eq!(
        nats_incomplete_adapter.presence(),
        ArtefactPresence::OwnershipRecordOnly
    );
    let nats_reader = nats_incomplete_adapter
        .open_read()
        .expect("open read on incomplete nats");
    assert!(matches!(
        nats_reader.admission(),
        OpenAdmission::IncompleteCreation(_)
    ));
    let nats_stale_open_err = nats_incomplete_adapter.open_write(999).unwrap_err();
    assert_eq!(
        nats_stale_open_err.condition(),
        &FailureCondition::StaleEpoch
    );
    let mut nats_completed = nats_incomplete_adapter
        .complete_creation(&claim)
        .expect("complete nats creation");
    assert_eq!(nats_incomplete_adapter.presence(), ArtefactPresence::Both);
    nats_completed
        .append_raw_frame(b"completed-frame")
        .expect("append after completion");

    let cas_absent_err = evaluate_claim_cas(&RecordedOwnership::Absent, None, &claim).unwrap_err();
    assert_eq!(
        cas_absent_err.condition(),
        &FailureCondition::OwnershipUnestablished
    );
    assert_eq!(file_orphan_err.condition(), cas_absent_err.condition());

    nats_incomplete_adapter.delete_streams().expect("cleanup");
    let cleanup_adapter =
        NatsStorageAdapter::new(server.url(), &nats_stem).expect("connect cleanup");
    cleanup_adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_m7_dimension_2_overlapping_writers_and_epoch_fencing() {
    let claim1 = sample_claim(1);

    let dir = TestDir::new("dim2_writers_file");
    let file_store_path = dir.path().join("writers_store");
    let file_adapter = FileStorageAdapter::new(&file_store_path);
    let mut file_writer1 = file_adapter.create(&claim1).expect("create file writer 1");
    let file_adapter2 = FileStorageAdapter::new(&file_store_path);
    let file_exclusion_err = file_adapter2.open_write(1).unwrap_err();
    assert_eq!(
        file_exclusion_err.condition(),
        &FailureCondition::AnotherOwnerHoldsExclusion
    );

    let _ = file_writer1
        .append_raw_frame(b"file-frame-1")
        .expect("append 1");
    let claim2 = sample_claim(2);
    file_adapter
        .record_ownership_claim(&claim2)
        .expect("supersede file claim");
    let file_stale_err = file_writer1.append_raw_frame(b"file-frame-2").unwrap_err();
    assert_eq!(file_stale_err.condition(), &FailureCondition::StaleEpoch);

    let server = LiveNatsServer::acquire();
    let nats_stem = unique_nats_stem("dim2_writers_nats");
    let nats_adapter = NatsStorageAdapter::new(server.url(), &nats_stem).expect("connect nats");
    let mut nats_writer1 = nats_adapter.create(&claim1).expect("create nats writer 1");
    let mut nats_writer2 = nats_adapter.open_write(1).expect("open nats writer 2");

    let count1 = nats_writer1
        .append_raw_frame(b"nats-frame-1")
        .expect("append 1");
    assert_eq!(count1, 1);
    let nats_occ_err = nats_writer2.append_raw_frame(b"nats-frame-2").unwrap_err();
    assert_eq!(
        nats_occ_err.condition(),
        &FailureCondition::ConcurrencyConflict
    );

    nats_adapter
        .record_ownership_claim(&claim2)
        .expect("supersede nats claim");
    let nats_stale_err = nats_writer1.append_raw_frame(b"nats-frame-3").unwrap_err();
    assert_eq!(nats_stale_err.condition(), &FailureCondition::StaleEpoch);

    assert_eq!(file_stale_err.condition(), nats_stale_err.condition());

    nats_adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_m7_dimension_3_unreadable_ownership_and_indeterminate_verdict() {
    let server = LiveNatsServer::acquire();
    let nats_stem = unique_nats_stem("dim3_indeterminate");
    let nats_adapter = NatsStorageAdapter::new(server.url(), &nats_stem).expect("connect nats");
    let claim = sample_claim(1);

    let mut nats_writer = nats_adapter.create(&claim).expect("create nats writer");
    let env1 = EventEnvelope::genesis([0x01; 16], [0x11; 16], b"clean-frame").unwrap();
    let mut buf1 = Vec::new();
    env1.encode(&mut buf1);
    let landed = nats_writer.append_frame_verdict(&buf1).expect("verdict");
    assert_eq!(landed, WriteLandingVerdict::Landed(1));

    let mut indet_writer = nats_writer.with_simulate_indeterminate(true);
    let env2 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id: [0x11; 16],
            detached: false,
            precursor: env1.header.event_id,
            precursor_hash: env1.commitment(),
        },
        payload: b"indet-frame".to_vec(),
    };
    let mut buf2 = Vec::new();
    env2.encode(&mut buf2);
    let undetermined = indet_writer.append_frame_verdict(&buf2).expect("verdict");
    assert_eq!(
        undetermined,
        WriteLandingVerdict::Undetermined { carried_epoch: 1 }
    );

    let dir = TestDir::new("dim3_corrupt_file");
    let file_store_path = dir.path().join("corrupt_meta_store");
    let file_adapter = FileStorageAdapter::new(&file_store_path);
    let mut file_writer = file_adapter.create(&claim).expect("create file writer");
    file_writer.append_raw_frame(b"frame-1").expect("append 1");

    fs::write(file_adapter.meta_path(), b"garbage_corrupted_meta").expect("corrupt meta");
    let file_unreadable_err = file_writer.append_raw_frame(b"frame-2").unwrap_err();
    assert_eq!(
        file_unreadable_err.condition(),
        &FailureCondition::OwnershipRecordUnreadable
    );

    let recorded_unreadable = RecordedOwnership::Unreadable;
    let cas_err = evaluate_claim_cas(&recorded_unreadable, Some(1), &sample_claim(2)).unwrap_err();
    assert_eq!(
        cas_err.condition(),
        &FailureCondition::OwnershipRecordUnreadable
    );

    nats_adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_m7_dimension_4_migration_cutover_and_permanent_source_retirement() {
    let dir = TestDir::new("dim4_cutover_file");
    let source_path = dir.path().join("source_file");
    let target_path = dir.path().join("target_file");
    let file_source = FileStorageAdapter::new(&source_path);
    let file_target = FileStorageAdapter::new(&target_path);

    let claim = sample_claim(1);
    file_source.create(&claim).expect("create source");
    file_target.create(&claim).expect("create target");

    let mut file_writer = file_source.open_write(1).expect("open write source");
    let env1 = sample_genesis_envelope(1, 0x11, b"data1");
    file_writer.append_envelope(&env1).expect("append 1");

    let file_manager = MigrationManager::new(file_source.clone(), file_target.clone());
    let summary = file_manager.run_all().expect("run file cutover");
    assert_eq!(summary.total_migrated_events, 1);
    assert_eq!(summary.surviving_fibers, 1);

    assert!(file_source.is_retired_source().expect("query retired"));
    let file_writer_err = file_writer.append_envelope(&env1).unwrap_err();
    assert_eq!(
        file_writer_err.condition(),
        &FailureCondition::RetiredMigrationSource
    );
    let file_new_writer_err = file_source.open_write(1).unwrap_err();
    assert_eq!(
        file_new_writer_err.condition(),
        &FailureCondition::RetiredMigrationSource
    );

    let server = LiveNatsServer::acquire();
    let nats_src_stem = unique_nats_stem("dim4_nats_src");
    let nats_dst_stem = unique_nats_stem("dim4_nats_dst");
    let nats_source =
        NatsStorageAdapter::new(server.url(), &nats_src_stem).expect("connect nats src");
    let nats_target =
        NatsStorageAdapter::new(server.url(), &nats_dst_stem).expect("connect nats dst");

    nats_source.create(&claim).expect("create nats source");
    nats_target.create(&claim).expect("create nats target");

    let mut nats_writer = nats_source.open_write(1).expect("open write nats source");
    nats_writer.append_envelope(&env1).expect("append 1");

    let nats_manager = MigrationManager::new(nats_source.clone(), nats_target.clone());
    let nats_summary = nats_manager.run_all().expect("run nats cutover");
    assert_eq!(nats_summary.total_migrated_events, 1);
    assert_eq!(nats_summary.surviving_fibers, 1);

    assert!(nats_source.is_retired_source().expect("query retired"));
    let nats_writer_err = nats_writer.append_envelope(&env1).unwrap_err();
    assert_eq!(
        nats_writer_err.condition(),
        &FailureCondition::RetiredMigrationSource
    );
    let nats_new_writer_err = nats_source.open_write(1).unwrap_err();
    assert_eq!(
        nats_new_writer_err.condition(),
        &FailureCondition::RetiredMigrationSource
    );

    assert_eq!(file_writer_err.condition(), nats_writer_err.condition());
    assert_eq!(
        file_new_writer_err.condition(),
        nats_new_writer_err.condition()
    );

    let cross_fn_file_src = FileStorageAdapter::new(dir.path().join("cross_fn_src"));
    cross_fn_file_src.create(&claim).expect("create cross src");
    let mut cross_fn_writer = cross_fn_file_src
        .open_write(1)
        .expect("open cross src writer");
    cross_fn_writer
        .append_envelope(&env1)
        .expect("append cross src");

    let cross_fn_nats_dst_stem = unique_nats_stem("dim4_cross_fn_dst");
    let cross_fn_nats_dst =
        NatsStorageAdapter::new(server.url(), &cross_fn_nats_dst_stem).expect("connect cross dst");
    cross_fn_nats_dst.create(&claim).expect("create cross dst");

    let cross_fn_manager =
        MigrationManager::new(cross_fn_file_src.clone(), cross_fn_nats_dst.clone());
    let cross_fn_summary = cross_fn_manager
        .run_all()
        .expect("cross file-to-nats cutover");
    assert_eq!(cross_fn_summary.total_migrated_events, 1);
    assert!(cross_fn_file_src
        .is_retired_source()
        .expect("cross src retired"));

    let cross_nf_nats_src_stem = unique_nats_stem("dim4_cross_nf_src");
    let cross_nf_nats_src =
        NatsStorageAdapter::new(server.url(), &cross_nf_nats_src_stem).expect("connect nf src");
    cross_nf_nats_src.create(&claim).expect("create nf src");
    let mut cross_nf_writer = cross_nf_nats_src.open_write(1).expect("open nf src writer");
    cross_nf_writer
        .append_envelope(&env1)
        .expect("append nf src");

    let cross_nf_file_dst = FileStorageAdapter::new(dir.path().join("cross_nf_dst"));
    cross_nf_file_dst.create(&claim).expect("create nf dst");

    let cross_nf_manager =
        MigrationManager::new(cross_nf_nats_src.clone(), cross_nf_file_dst.clone());
    let cross_nf_summary = cross_nf_manager
        .run_all()
        .expect("cross nats-to-file cutover");
    assert_eq!(cross_nf_summary.total_migrated_events, 1);
    assert!(cross_nf_nats_src
        .is_retired_source()
        .expect("cross nf src retired"));

    nats_source.delete_streams().expect("cleanup");
    nats_target.delete_streams().expect("cleanup");
    cross_fn_nats_dst.delete_streams().expect("cleanup");
    cross_nf_nats_src.delete_streams().expect("cleanup");
}

#[test]
fn test_m7_dimension_5_partial_target_reads_and_superseded_generation() {
    let claim = sample_claim(1);
    let dir = TestDir::new("dim5_partial_file");
    let file_src = FileStorageAdapter::new(dir.path().join("partial_src_file"));
    let file_target = FileStorageAdapter::new(dir.path().join("partial_target_file"));
    file_src.create(&claim).expect("create src file");
    file_target.create(&claim).expect("create target file");

    let mut file_writer = file_src.open_write(1).expect("open src writer");
    let env1 = sample_genesis_envelope(1, 0xaa, b"partial_1");
    let comm1 = env1.commitment();
    let env2 = sample_chained_envelope(2, 0xaa, 1, comm1, b"partial_2");
    file_writer.append_envelope(&env1).expect("append 1");
    file_writer.append_envelope(&env2).expect("append 2");

    let mut file_manager = MigrationManager::new(file_src.clone(), file_target.clone());
    let chased = file_manager.chase().expect("chase");
    assert_eq!(chased, 2);
    assert_eq!(file_manager.phase(), MigrationPhase::Chase);

    let mut file_target_reader = file_target
        .open_read()
        .expect("open target reader during chase");
    let partial_migrated = file_target_reader
        .read_all_envelopes()
        .expect("read target envelopes during chase");
    assert_eq!(partial_migrated.len(), 2);
    assert_eq!(partial_migrated[0].payload, b"partial_1");
    assert_eq!(partial_migrated[1].payload, b"partial_2");
    assert!(file_src.outbound_pointer().unwrap().is_none());

    file_manager.freeze().expect("freeze");
    file_manager.cutover().expect("cutover");
    assert!(file_src.is_retired_source().expect("query retired"));

    let mut file_src_reader = file_src
        .open_read()
        .expect("open retired source for reading");
    let historical = file_src_reader
        .read_all_envelopes()
        .expect("historical read on retired generation");
    assert_eq!(historical.len(), 2);
    assert_eq!(historical[0].payload, b"partial_1");
    assert_eq!(historical[1].payload, b"partial_2");

    let server = LiveNatsServer::acquire();
    let nats_src_stem = unique_nats_stem("dim5_partial_nats_src");
    let nats_target_stem = unique_nats_stem("dim5_partial_nats_target");
    let nats_src = NatsStorageAdapter::new(server.url(), &nats_src_stem).expect("connect nats src");
    let nats_target =
        NatsStorageAdapter::new(server.url(), &nats_target_stem).expect("connect nats target");
    nats_src.create(&claim).expect("create nats src");
    nats_target.create(&claim).expect("create nats target");

    let mut nats_writer = nats_src.open_write(1).expect("open nats writer");
    nats_writer.append_envelope(&env1).expect("append 1");
    nats_writer.append_envelope(&env2).expect("append 2");

    let mut nats_manager = MigrationManager::new(nats_src.clone(), nats_target.clone());
    let nats_chased = nats_manager.chase().expect("chase");
    assert_eq!(nats_chased, 2);

    let mut nats_target_reader = nats_target.open_read().expect("open nats target reader");
    let nats_partial = nats_target_reader
        .read_all_envelopes()
        .expect("read nats target envelopes");
    assert_eq!(nats_partial.len(), 2);
    assert_eq!(nats_partial[0].payload, b"partial_1");
    assert_eq!(nats_partial[1].payload, b"partial_2");

    nats_manager.freeze().expect("freeze");
    nats_manager.cutover().expect("cutover");
    assert!(nats_src.is_retired_source().expect("query retired"));

    let mut nats_src_reader = nats_src.open_read().expect("open retired nats source");
    let nats_historical = nats_src_reader
        .read_all_envelopes()
        .expect("historical read on retired nats");
    assert_eq!(nats_historical.len(), 2);
    assert_eq!(nats_historical[0].payload, b"partial_1");
    assert_eq!(nats_historical[1].payload, b"partial_2");

    nats_src.delete_streams().expect("cleanup");
    nats_target.delete_streams().expect("cleanup");
}

#[test]
fn test_m7_dimension_6_transformation_refusal_and_fiber_policies() {
    let claim = sample_claim(1);
    let dir = TestDir::new("dim6_policies_file");

    let file_src_fail = FileStorageAdapter::new(dir.path().join("tx_fail_src_file"));
    let file_target_fail = FileStorageAdapter::new(dir.path().join("tx_fail_target_file"));
    file_src_fail.create(&claim).expect("create file src fail");
    file_target_fail
        .create(&claim)
        .expect("create file target fail");

    let mut file_writer_fail = file_src_fail.open_write(1).expect("open writer");
    let env1 = sample_genesis_envelope(1, 0xbb, b"tx_payload");
    file_writer_fail.append_envelope(&env1).expect("append 1");

    let file_manager_fail = MigrationManager::new(file_src_fail.clone(), file_target_fail.clone())
        .with_transformer(|_payload: &[u8]| {
            Err(OperationFailure::new(
                FailureCondition::TransformationRefused,
                "payload refused by schema policy",
            ))
        });
    let file_tx_err = file_manager_fail
        .run_all()
        .expect_err("transformation refusal must fail");
    assert_eq!(
        file_tx_err.condition(),
        &FailureCondition::TransformationRefused
    );
    assert!(!file_src_fail.is_retired_source().expect("query retired"));

    let server = LiveNatsServer::acquire();
    let nats_src_fail_stem = unique_nats_stem("dim6_nats_tx_fail_src");
    let nats_target_fail_stem = unique_nats_stem("dim6_nats_tx_fail_target");
    let nats_src_fail =
        NatsStorageAdapter::new(server.url(), &nats_src_fail_stem).expect("connect nats src");
    let nats_target_fail =
        NatsStorageAdapter::new(server.url(), &nats_target_fail_stem).expect("connect nats target");
    nats_src_fail.create(&claim).expect("create nats src");
    nats_target_fail.create(&claim).expect("create nats target");

    let mut nats_writer_fail = nats_src_fail.open_write(1).expect("open nats writer");
    nats_writer_fail.append_envelope(&env1).expect("append 1");

    let nats_manager_fail = MigrationManager::new(nats_src_fail.clone(), nats_target_fail.clone())
        .with_transformer(|_payload: &[u8]| {
            Err(OperationFailure::new(
                FailureCondition::TransformationRefused,
                "payload refused by schema policy",
            ))
        });
    let nats_tx_err = nats_manager_fail
        .run_all()
        .expect_err("transformation refusal must fail");
    assert_eq!(
        nats_tx_err.condition(),
        &FailureCondition::TransformationRefused
    );
    assert!(!nats_src_fail.is_retired_source().expect("query retired"));

    assert_eq!(file_tx_err.condition(), nats_tx_err.condition());

    let fiber_keep = [0x11u8; 16];
    let fiber_purge = [0x22u8; 16];
    let fiber_lock = [0x33u8; 16];

    let a1 = sample_genesis_envelope(1, 0x11, b"a1");
    let b1 = sample_genesis_envelope(10, 0x22, b"b1");
    let c1 = sample_genesis_envelope(20, 0x33, b"c1");

    let nats_src_pol_stem = unique_nats_stem("dim6_nats_pol_src");
    let nats_target_pol_stem = unique_nats_stem("dim6_nats_pol_target");
    let nats_src_pol =
        NatsStorageAdapter::new(server.url(), &nats_src_pol_stem).expect("connect nats pol src");
    let nats_target_pol = NatsStorageAdapter::new(server.url(), &nats_target_pol_stem)
        .expect("connect nats pol target");
    nats_src_pol.create(&claim).expect("create pol src");
    nats_target_pol.create(&claim).expect("create pol target");

    let mut nats_pol_writer = nats_src_pol.open_write(1).expect("open pol writer");
    nats_pol_writer.append_envelope(&a1).expect("append a1");
    nats_pol_writer.append_envelope(&b1).expect("append b1");
    nats_pol_writer.append_envelope(&c1).expect("append c1");

    let nats_pol_manager = MigrationManager::new(nats_src_pol.clone(), nats_target_pol.clone())
        .with_fiber_policy(fiber_keep, FiberMigrationPolicy::Keep)
        .with_fiber_policy(fiber_purge, FiberMigrationPolicy::Purge)
        .with_fiber_policy(fiber_lock, FiberMigrationPolicy::LockAndPrune);
    let pol_summary = nats_pol_manager.run_all().expect("run pol migration");
    assert_eq!(pol_summary.total_migrated_events, 2);
    assert_eq!(pol_summary.surviving_fibers, 2);

    let mut nats_pol_reader = nats_target_pol.open_read().expect("open pol reader");
    let migrated_envs = nats_pol_reader
        .read_all_envelopes()
        .expect("read pol envelopes");
    assert_eq!(migrated_envs.len(), 2);
    assert_eq!(migrated_envs[0].payload, b"a1");
    assert_eq!(migrated_envs[1].payload, b"c1");
    assert!(migrated_envs[1].header.detached);

    nats_src_fail.delete_streams().expect("cleanup");
    nats_target_fail.delete_streams().expect("cleanup");
    nats_src_pol.delete_streams().expect("cleanup");
    nats_target_pol.delete_streams().expect("cleanup");
}

#[test]
fn test_m7_dimension_7_chain_breaks_and_mismatch_refusal() {
    let claim = sample_claim(1);
    let dir = TestDir::new("dim7_chain_file");

    let file_src = FileStorageAdapter::new(dir.path().join("chain_break_file_src"));
    let file_target = FileStorageAdapter::new(dir.path().join("chain_break_file_target"));
    file_src.create(&claim).expect("create file src");
    file_target.create(&claim).expect("create file target");

    let mut file_writer = file_src.open_write(1).expect("open file writer");
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
    file_writer.append_envelope(&env1).expect("append env1");
    let mut broken_buf = Vec::new();
    broken_env.encode(&mut broken_buf);
    file_writer
        .append_unvalidated_frame(&broken_buf)
        .expect("append broken");

    let file_manager_refuse = MigrationManager::new(file_src.clone(), file_target.clone())
        .with_broken_chain_election(BrokenChainElection::RefuseOnBreak);
    let file_break_err = file_manager_refuse
        .run_all()
        .expect_err("refuse on broken chain");
    assert!(matches!(
        file_break_err.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));

    let server = LiveNatsServer::acquire();
    let nats_src_stem = unique_nats_stem("dim7_nats_chain_src");
    let nats_target_stem = unique_nats_stem("dim7_nats_chain_target");
    let nats_src = NatsStorageAdapter::new(server.url(), &nats_src_stem).expect("connect nats src");
    let nats_target =
        NatsStorageAdapter::new(server.url(), &nats_target_stem).expect("connect nats target");
    nats_src.create(&claim).expect("create nats src");
    nats_target.create(&claim).expect("create nats target");

    let mut nats_writer = nats_src.open_write(1).expect("open nats writer");
    nats_writer.append_envelope(&env1).expect("append env1");
    nats_writer
        .append_unvalidated_frame(&broken_buf)
        .expect("append broken");

    let nats_manager_refuse = MigrationManager::new(nats_src.clone(), nats_target.clone())
        .with_broken_chain_election(BrokenChainElection::RefuseOnBreak);
    let nats_break_err = nats_manager_refuse
        .run_all()
        .expect_err("refuse on broken chain");
    assert!(matches!(
        nats_break_err.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));

    assert_eq!(
        std::mem::discriminant(file_break_err.condition()),
        std::mem::discriminant(nats_break_err.condition())
    );

    let file_target_permit = FileStorageAdapter::new(dir.path().join("chain_permit_file_target"));
    file_target_permit
        .create(&claim)
        .expect("create permit target");
    let file_manager_permit = MigrationManager::new(file_src.clone(), file_target_permit.clone())
        .with_broken_chain_election(BrokenChainElection::PermitBrokenHistory);
    let file_permit_summary = file_manager_permit
        .run_all()
        .expect("permit broken history migration");
    assert_eq!(file_permit_summary.total_migrated_events, 2);

    let nats_target_permit_stem = unique_nats_stem("dim7_nats_chain_permit");
    let nats_target_permit = NatsStorageAdapter::new(server.url(), &nats_target_permit_stem)
        .expect("connect nats permit target");
    nats_target_permit
        .create(&claim)
        .expect("create permit nats target");
    let nats_manager_permit = MigrationManager::new(nats_src.clone(), nats_target_permit.clone())
        .with_broken_chain_election(BrokenChainElection::PermitBrokenHistory);
    let nats_permit_summary = nats_manager_permit
        .run_all()
        .expect("permit broken history migration");
    assert_eq!(nats_permit_summary.total_migrated_events, 2);

    nats_src.delete_streams().expect("cleanup");
    nats_target.delete_streams().expect("cleanup");
    nats_target_permit.delete_streams().expect("cleanup");
}

#[test]
fn test_m7_dimension_8_unanchored_history_and_rolling_commitment() {
    let claim = sample_claim(1);
    let dir = TestDir::new("dim8_rolling_file");

    let file_adapter = FileStorageAdapter::new(dir.path().join("rolling_file_store"));
    let mut file_writer = file_adapter.create(&claim).expect("create file writer");

    let mut expected_commitment = RollingCommitment::new();
    for i in 1..=5 {
        let payload = format!("rolling_frame_{i}").into_bytes();
        let mut frame_buf = Vec::new();
        ContainerFrame::encode_payload(&payload, &mut frame_buf);
        expected_commitment.update_frame(&frame_buf);
        file_writer
            .append_raw_frame(&payload)
            .expect("append frame");
    }

    assert_eq!(
        file_writer.rolling_commitment().current_commitment(),
        expected_commitment.current_commitment()
    );
    assert_eq!(file_writer.rolling_commitment().frame_count(), 5);

    let mut file_reader = file_adapter.open_read().expect("open file reader");
    let file_frames = file_reader.read_all_frames().expect("read file frames");
    assert_eq!(file_frames.len(), 5);
    assert_eq!(
        file_reader.rolling_commitment().current_commitment(),
        expected_commitment.current_commitment()
    );
    assert_eq!(file_reader.rolling_commitment().frame_count(), 5);

    let server = LiveNatsServer::acquire();
    let nats_stem = unique_nats_stem("dim8_rolling_nats");
    let nats_adapter = NatsStorageAdapter::new(server.url(), &nats_stem).expect("connect nats");
    let mut nats_writer = nats_adapter.create(&claim).expect("create nats writer");

    for i in 1..=5 {
        let payload = format!("rolling_frame_{i}").into_bytes();
        nats_writer
            .append_raw_frame(&payload)
            .expect("append frame");
    }

    assert_eq!(
        nats_writer.rolling_commitment().current_commitment(),
        expected_commitment.current_commitment()
    );
    assert_eq!(nats_writer.rolling_commitment().frame_count(), 5);

    let mut nats_reader = nats_adapter.open_read().expect("open nats reader");
    let nats_frames = nats_reader.read_all_frames().expect("read nats frames");
    assert_eq!(nats_frames.len(), 5);
    assert_eq!(
        nats_reader.rolling_commitment().current_commitment(),
        expected_commitment.current_commitment()
    );
    assert_eq!(nats_reader.rolling_commitment().frame_count(), 5);

    assert_eq!(
        file_reader.rolling_commitment().current_commitment(),
        nats_reader.rolling_commitment().current_commitment()
    );

    nats_adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_m7_strict_cross_adapter_error_condition_parity() {
    let claim1 = sample_claim(1);
    let claim2 = sample_claim(2);
    let dir = TestDir::new("parity_suite");
    let server = LiveNatsServer::acquire();

    let file_stem = dir.path().join("parity_store");
    let file_adapter = FileStorageAdapter::new(&file_stem);
    let nats_stem = unique_nats_stem("parity_store");
    let nats_adapter = NatsStorageAdapter::new(server.url(), &nats_stem).expect("connect nats");

    let file_err_no_art = file_adapter.open_write(1).unwrap_err();
    let nats_err_no_art = nats_adapter.open_write(1).unwrap_err();
    assert_eq!(
        file_err_no_art.condition(),
        &FailureCondition::NoArtefactExists
    );
    assert_eq!(
        nats_err_no_art.condition(),
        &FailureCondition::NoArtefactExists
    );
    assert_eq!(file_err_no_art.condition(), nats_err_no_art.condition());

    let mut file_writer = file_adapter.create(&claim1).expect("create file");
    let mut nats_writer = nats_adapter.create(&claim1).expect("create nats");

    let file_err_exists = file_adapter.create(&claim1).unwrap_err();
    let nats_err_exists = nats_adapter.create(&claim1).unwrap_err();
    assert_eq!(
        file_err_exists.condition(),
        &FailureCondition::StoreAlreadyExists
    );
    assert_eq!(
        nats_err_exists.condition(),
        &FailureCondition::StoreAlreadyExists
    );
    assert_eq!(file_err_exists.condition(), nats_err_exists.condition());

    let initial_env = sample_genesis_envelope(1, 0x10, b"initial_event");
    let mut nats_writer2 = nats_adapter.open_write(1).expect("open write 2");
    let _ = nats_writer.append_envelope(&initial_env).expect("append 1");
    let nats_err_conflict = nats_writer2.append_envelope(&initial_env).unwrap_err();
    assert_eq!(
        nats_err_conflict.condition(),
        &FailureCondition::ConcurrencyConflict
    );

    let recorded = RecordedOwnership::Claimed(claim1);
    let cas_conflict = evaluate_claim_cas(&recorded, Some(99), &claim2).unwrap_err();
    assert_eq!(
        cas_conflict.condition(),
        &FailureCondition::ConcurrencyConflict
    );
    assert_eq!(cas_conflict.condition(), nats_err_conflict.condition());

    file_writer
        .append_envelope(&initial_env)
        .expect("append initial file env");
    file_adapter
        .record_ownership_claim(&claim2)
        .expect("supersede file claim");
    nats_adapter
        .record_ownership_claim(&claim2)
        .expect("supersede nats claim");

    let file_err_stale = file_writer.append_envelope(&initial_env).unwrap_err();
    let nats_err_stale = nats_writer.append_envelope(&initial_env).unwrap_err();
    assert_eq!(file_err_stale.condition(), &FailureCondition::StaleEpoch);
    assert_eq!(nats_err_stale.condition(), &FailureCondition::StaleEpoch);
    assert_eq!(file_err_stale.condition(), nats_err_stale.condition());

    drop(file_writer);
    drop(nats_writer);
    drop(nats_writer2);

    let target_file = FileStorageAdapter::new(dir.path().join("parity_target_file"));
    target_file.create(&claim2).expect("create target file");
    let target_nats_stem = unique_nats_stem("parity_target_nats");
    let target_nats =
        NatsStorageAdapter::new(server.url(), &target_nats_stem).expect("connect target nats");
    target_nats.create(&claim2).expect("create target nats");

    let broken_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [2u8; 16],
            fiber_id: [0x10u8; 16],
            detached: false,
            precursor: [1u8; 16],
            precursor_hash: [0xffu8; 32],
        },
        payload: b"broken".to_vec(),
    };
    let mut file_w_broken = file_adapter.open_write(2).expect("open w2 file");
    let mut broken_buf = Vec::new();
    broken_env.encode(&mut broken_buf);
    file_w_broken
        .append_unvalidated_frame(&broken_buf)
        .expect("append broken file");
    let mut nats_w_broken = nats_adapter.open_write(2).expect("open w2 nats");
    nats_w_broken
        .append_unvalidated_frame(&broken_buf)
        .expect("append broken nats");
    drop(file_w_broken);
    drop(nats_w_broken);

    let manager_file_break = MigrationManager::new(file_adapter.clone(), target_file)
        .with_broken_chain_election(BrokenChainElection::RefuseOnBreak);
    let file_err_broken = manager_file_break.run_all().unwrap_err();

    let manager_nats_break = MigrationManager::new(nats_adapter.clone(), target_nats.clone())
        .with_broken_chain_election(BrokenChainElection::RefuseOnBreak);
    let nats_err_broken = manager_nats_break.run_all().unwrap_err();

    assert!(matches!(
        file_err_broken.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));
    assert!(matches!(
        nats_err_broken.condition(),
        FailureCondition::PrecursorChainBroken(_)
    ));
    assert_eq!(
        std::mem::discriminant(file_err_broken.condition()),
        std::mem::discriminant(nats_err_broken.condition())
    );

    let clean_target_file = FileStorageAdapter::new(dir.path().join("parity_clean_target_file"));
    clean_target_file
        .create(&claim2)
        .expect("create clean file");
    let clean_target_nats_stem = unique_nats_stem("parity_clean_nats_target");
    let clean_target_nats = NatsStorageAdapter::new(server.url(), &clean_target_nats_stem)
        .expect("connect clean target");
    clean_target_nats
        .create(&claim2)
        .expect("create clean nats");

    let manager_file_permit =
        MigrationManager::new(file_adapter.clone(), clean_target_file.clone())
            .with_broken_chain_election(BrokenChainElection::PermitBrokenHistory);
    manager_file_permit.run_all().expect("cutover file permit");

    let manager_nats_permit =
        MigrationManager::new(nats_adapter.clone(), clean_target_nats.clone())
            .with_broken_chain_election(BrokenChainElection::PermitBrokenHistory);
    manager_nats_permit.run_all().expect("cutover nats permit");

    let file_err_retired = file_adapter.open_write(2).unwrap_err();
    let nats_err_retired = nats_adapter.open_write(2).unwrap_err();
    assert_eq!(
        file_err_retired.condition(),
        &FailureCondition::RetiredMigrationSource
    );
    assert_eq!(
        nats_err_retired.condition(),
        &FailureCondition::RetiredMigrationSource
    );
    assert_eq!(file_err_retired.condition(), nats_err_retired.condition());

    nats_adapter.delete_streams().expect("cleanup");
    target_nats.delete_streams().expect("cleanup");
    clean_target_nats.delete_streams().expect("cleanup");
}

#[test]
fn test_m7_resource_contracts_under_saturation() {
    let claim = sample_claim(1);
    let dir = TestDir::new("saturation_suite");
    let server = LiveNatsServer::acquire();

    let file_adapter = FileStorageAdapter::new(dir.path().join("saturation_file"));
    let mut file_writer = file_adapter.create(&claim).expect("create file");

    let nats_stem = unique_nats_stem("saturation_nats");
    let nats_adapter = NatsStorageAdapter::new(server.url(), &nats_stem).expect("connect nats");
    let mut nats_writer = nats_adapter.create(&claim).expect("create nats");

    let saturation_batch_count = 100u64;
    for seq in 1..=saturation_batch_count {
        let payload = format!("saturation_payload_data_item_{seq}").into_bytes();
        let file_seq = file_writer
            .append_raw_frame(&payload)
            .expect("append frame file");
        let nats_seq = nats_writer
            .append_raw_frame(&payload)
            .expect("append frame nats");
        assert_eq!(file_seq, seq);
        assert_eq!(nats_seq, seq);
    }

    assert_eq!(
        file_writer.rolling_commitment().frame_count(),
        saturation_batch_count
    );
    assert_eq!(
        nats_writer.rolling_commitment().frame_count(),
        saturation_batch_count
    );
    assert_eq!(
        file_writer.rolling_commitment().current_commitment(),
        nats_writer.rolling_commitment().current_commitment()
    );

    let high_capacity_env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0xfe; 16],
            fiber_id: [0x01; 16],
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: Vec::with_capacity(MAX_STREAM_BYTES + 1),
    };
    assert!(high_capacity_env.payload.capacity() > MAX_STREAM_BYTES);

    ArtefactReader::with_stream("saturation_cap_test", vec![high_capacity_env], |reader| {
        let err = reader
            .read_event()
            .expect("some event")
            .expect_err("high capacity must be refused");
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

    nats_adapter.delete_streams().expect("cleanup");
}

#[test]
fn test_m7_clean_cancellation_and_shutdown_lock_release() {
    let claim = sample_claim(1);
    let dir = TestDir::new("shutdown_release_suite");
    let server = LiveNatsServer::acquire();

    let env1 = EventEnvelope::genesis([0x01; 16], [0x22; 16], b"shutdown_frame_1").unwrap();
    let mut buf1 = Vec::new();
    env1.encode(&mut buf1);

    let env2 = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x02; 16],
            fiber_id: [0x22; 16],
            detached: false,
            precursor: env1.header.event_id,
            precursor_hash: env1.commitment(),
        },
        payload: b"shutdown_frame_2".to_vec(),
    };
    let mut buf2 = Vec::new();
    env2.encode(&mut buf2);

    let file_adapter = FileStorageAdapter::new(dir.path().join("shutdown_file"));
    let mut file_writer_1 = file_adapter.create(&claim).expect("create file writer 1");
    file_writer_1.append_frame(&buf1).expect("append frame 1");
    let file_comm_1 = file_writer_1.rolling_commitment().current_commitment();
    drop(file_writer_1);

    let mut file_writer_2 = file_adapter
        .open_write(1)
        .expect("open file writer 2 after writer 1 drop");
    file_writer_2.append_frame(&buf2).expect("append frame 2");
    drop(file_writer_2);

    let mut file_reader = file_adapter.open_read().expect("open file reader");
    let file_frames = file_reader.read_all_frames().expect("read all file frames");
    assert_eq!(file_frames.len(), 2);
    assert_eq!(file_frames[0], buf1);
    assert_eq!(file_frames[1], buf2);

    let nats_stem = unique_nats_stem("shutdown_nats");
    let nats_adapter = NatsStorageAdapter::new(server.url(), &nats_stem).expect("connect nats");
    let mut nats_writer_1 = nats_adapter.create(&claim).expect("create nats writer 1");
    nats_writer_1.append_frame(&buf1).expect("append frame 1");
    let nats_comm_1 = nats_writer_1.rolling_commitment().current_commitment();
    assert_eq!(file_comm_1, nats_comm_1);
    drop(nats_writer_1);

    let mut nats_writer_2 = nats_adapter
        .open_write(1)
        .expect("open nats writer 2 after writer 1 drop");
    nats_writer_2.append_frame(&buf2).expect("append frame 2");
    drop(nats_writer_2);

    let mut nats_reader = nats_adapter.open_read().expect("open nats reader");
    let nats_frames = nats_reader.read_all_frames().expect("read all nats frames");
    assert_eq!(nats_frames.len(), 2);
    assert_eq!(nats_frames[0], buf1);
    assert_eq!(nats_frames[1], buf2);

    assert_eq!(
        file_reader.rolling_commitment().current_commitment(),
        nats_reader.rolling_commitment().current_commitment()
    );

    nats_adapter.delete_streams().expect("cleanup");
}
