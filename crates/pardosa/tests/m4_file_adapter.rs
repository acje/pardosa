use pardosa::prelude::*;
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
            "pardosa_m4_test_{}_{}_{}",
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
        operator_label: "operator-test".to_string(),
    }
}

#[test]
fn test_readme_file_storage_adapter_example_compiles() {
    let dir = TestDir::new("readme_example");
    let adapter = FileStorageAdapter::new(dir.path().join("orders"));
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");

    let fiber_id = derive_fiber_id("ord-12345");
    let event_id = [1u8; 16];
    let event_payload = b"{\"status\":\"created\"}";
    let verdict = writer
        .append_to_fiber(fiber_id, event_id, event_payload)
        .expect("append to fiber");
    if let WriteLandingVerdict::Landed(envelope) = verdict {
        assert_eq!(envelope.header.event_id, event_id);
    } else {
        panic!("expected landed verdict");
    }
}

#[test]
fn test_m4_strict_create_and_refuse_existing() {
    let dir = TestDir::new("strict_create");
    let store_path = dir.path().join("demo_store");
    let adapter = FileStorageAdapter::new(&store_path);

    assert_eq!(adapter.stem(), "demo_store");
    assert_eq!(adapter.meta_path(), store_path.with_extension("meta"));
    assert_eq!(adapter.pgno_path(), store_path.with_extension("pgno"));
    assert_eq!(adapter.presence(), ArtefactPresence::None);

    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create should succeed");
    assert_eq!(adapter.presence(), ArtefactPresence::Both);
    assert_eq!(writer.carried_epoch(), 1);
    assert_eq!(writer.rolling_commitment().frame_count(), 0);

    let err = adapter.create(&claim).unwrap_err();
    assert_eq!(err.condition(), &FailureCondition::StoreAlreadyExists);

    let _ = writer.append_raw_frame(b"first-event");
}

#[test]
fn test_m4_strict_open_refuses_nonexistent() {
    let dir = TestDir::new("strict_open");
    let store_path = dir.path().join("nonexistent_store");
    let adapter = FileStorageAdapter::new(&store_path);

    let write_err = adapter.open_write(1).unwrap_err();
    assert_eq!(write_err.condition(), &FailureCondition::NoArtefactExists);

    let read_err = adapter.open_read().unwrap_err();
    assert_eq!(read_err.condition(), &FailureCondition::NoArtefactExists);
}

#[test]
fn test_m4_concurrent_create_exactly_one_winner() {
    let dir = TestDir::new("concurrent_create");
    let store_path = dir.path().join("raced_store");
    let num_threads = 8;
    let winners = Arc::new(AtomicUsize::new(0));
    let store_exists = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for thread_idx in 0..num_threads {
        let store_path_clone = store_path.clone();
        let winners_clone = Arc::clone(&winners);
        let store_exists_clone = Arc::clone(&store_exists);
        handles.push(thread::spawn(move || {
            let adapter = FileStorageAdapter::new(&store_path_clone);
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
}

#[test]
fn test_m4_writer_exclusion_and_policy() {
    let dir = TestDir::new("exclusion");
    let store_path = dir.path().join("locked_store");
    let adapter = FileStorageAdapter::new(&store_path);
    let claim = sample_claim(1);

    let writer1 = adapter
        .create(&claim)
        .expect("first writer creates and locks");

    let adapter2 = FileStorageAdapter::new(&store_path);
    let err = adapter2.open_write(1).unwrap_err();
    assert_eq!(
        err.condition(),
        &FailureCondition::AnotherOwnerHoldsExclusion
    );

    let unavail_adapter = FileStorageAdapter::new(&store_path)
        .with_exclusion_policy(FileExclusionPolicy::SimulateUnavailable);
    let unavail_err = unavail_adapter.open_write(1).unwrap_err();
    assert_eq!(
        unavail_err.condition(),
        &FailureCondition::ExclusionUnavailable
    );

    let held_adapter = FileStorageAdapter::new(&store_path)
        .with_exclusion_policy(FileExclusionPolicy::SimulateHeldByOther);
    let held_err = held_adapter.open_write(1).unwrap_err();
    assert_eq!(
        held_err.condition(),
        &FailureCondition::AnotherOwnerHoldsExclusion
    );

    drop(writer1);

    let writer2 = adapter
        .open_write(1)
        .expect("open write after lock released");
    drop(writer2);
}

#[test]
fn test_m4_readonly_open_concurrent_with_writer() {
    let dir = TestDir::new("readonly_concurrent");
    let store_path = dir.path().join("swmr_store");
    let adapter = FileStorageAdapter::new(&store_path);
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    writer.append_raw_frame(b"frame-1").expect("append frame 1");
    writer.append_raw_frame(b"frame-2").expect("append frame 2");

    let mut reader1 = adapter
        .open_read()
        .expect("first reader opens while writer holds lock");
    let mut reader2 = adapter
        .open_read()
        .expect("second reader opens concurrently");

    let frames1 = reader1.read_all_frames().expect("reader1 read all");
    let frames2 = reader2.read_all_frames().expect("reader2 read all");

    assert_eq!(frames1.len(), 2);
    assert_eq!(frames1[0], b"frame-1");
    assert_eq!(frames1[1], b"frame-2");
    assert_eq!(frames2, frames1);

    assert_eq!(reader1.rolling_commitment().frame_count(), 2);
    assert_eq!(
        reader1.rolling_commitment().current_commitment(),
        writer.rolling_commitment().current_commitment()
    );
}

#[test]
fn test_m4_incomplete_creation_and_orphan() {
    let dir = TestDir::new("incomplete_creation");
    let store_path = dir.path().join("incomplete_store");
    let adapter = FileStorageAdapter::new(&store_path);
    let claim = sample_claim(1);

    adapter
        .create_incomplete_meta_only(&claim)
        .expect("create incomplete meta only");
    assert_eq!(adapter.presence(), ArtefactPresence::OwnershipRecordOnly);

    let reader = adapter
        .open_read()
        .expect("reader opens incomplete creation");
    assert!(matches!(
        reader.admission(),
        OpenAdmission::IncompleteCreation(_)
    ));

    let mut writer = adapter
        .complete_creation(&claim)
        .expect("complete creation");
    assert_eq!(adapter.presence(), ArtefactPresence::Both);
    writer
        .append_raw_frame(b"after-completion")
        .expect("append after completion");
    drop(writer);

    let orphan_path = dir.path().join("orphan_store");
    let orphan_adapter = FileStorageAdapter::new(&orphan_path);
    let mut orphan_writer = orphan_adapter.create(&claim).expect("create normal");
    orphan_writer
        .append_raw_frame(b"orphan-data")
        .expect("append");
    drop(orphan_writer);

    fs::remove_file(orphan_adapter.meta_path()).expect("remove meta to make orphan");
    assert_eq!(orphan_adapter.presence(), ArtefactPresence::EventDataOnly);

    let write_err = orphan_adapter.open_write(1).unwrap_err();
    assert_eq!(
        write_err.condition(),
        &FailureCondition::OwnershipUnestablished
    );

    let mut orphan_reader = orphan_adapter.open_read().expect("read orphan");
    assert_eq!(orphan_reader.admission(), &OpenAdmission::ReadOnlyOrphan);
    let orphan_frames = orphan_reader
        .read_all_frames()
        .expect("read frames from orphan");
    assert_eq!(orphan_frames.len(), 1);
    assert_eq!(orphan_frames[0], b"orphan-data");
}

#[test]
fn test_m4_per_landing_epoch_verification() {
    let dir = TestDir::new("epoch_verification");
    let store_path = dir.path().join("epoch_store");
    let adapter = FileStorageAdapter::new(&store_path);
    let claim1 = sample_claim(1);

    let mut writer = adapter.create(&claim1).expect("create writer epoch 1");
    writer
        .append_raw_frame(b"frame-at-epoch-1")
        .expect("first append at epoch 1");

    let claim2 = sample_claim(2);
    adapter
        .record_ownership_claim(&claim2)
        .expect("superseding claim in meta");

    let stale_err = writer
        .append_raw_frame(b"frame-with-stale-epoch")
        .unwrap_err();
    assert_eq!(stale_err.condition(), &FailureCondition::StaleEpoch);

    let mut reader = adapter.open_read().expect("reader opens");
    let frames = reader.read_all_frames().expect("read frames");
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0], b"frame-at-epoch-1");

    fs::write(adapter.meta_path(), b"truncated").expect("corrupt meta");
    let unreadable_err = writer
        .append_raw_frame(b"frame-with-corrupt-meta")
        .unwrap_err();
    assert_eq!(
        unreadable_err.condition(),
        &FailureCondition::OwnershipRecordUnreadable
    );
}

#[test]
fn test_m4_continuous_rolling_commitment_and_crc32c() {
    let dir = TestDir::new("commitment_and_crc");
    let store_path = dir.path().join("crc_store");
    let adapter = FileStorageAdapter::new(&store_path);
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    assert_eq!(writer.rolling_commitment().frame_count(), 0);

    writer
        .append_raw_frame(b"payload-alpha")
        .expect("append alpha");
    assert_eq!(writer.rolling_commitment().frame_count(), 1);
    let digest1 = writer.rolling_commitment().current_commitment();

    writer
        .append_raw_frame(b"payload-beta")
        .expect("append beta");
    assert_eq!(writer.rolling_commitment().frame_count(), 2);
    let digest2 = writer.rolling_commitment().current_commitment();
    assert_ne!(digest1, digest2);

    let mut reader = adapter.open_read().expect("reader");
    let frames = reader.read_all_frames().expect("read frames");
    assert_eq!(frames.len(), 2);
    assert_eq!(reader.rolling_commitment().current_commitment(), digest2);

    let mut file_bytes = fs::read(adapter.pgno_path()).expect("read pgno");
    let last_byte_idx = file_bytes.len() - 1;
    file_bytes[last_byte_idx] ^= 0xFF;
    fs::write(adapter.pgno_path(), file_bytes).expect("write corrupt pgno");

    let err = adapter.open_read().unwrap_err();
    assert_eq!(
        err.condition(),
        &FailureCondition::PrecursorChainBroken(None)
    );
}

#[test]
fn test_m4_c8_2_schema_structural_completeness() {
    let valid_schema = SchemaDescriptor::new(
        1,
        DescriptorNode::Struct {
            name: "OrderPayload".to_string(),
            fields: vec![
                FieldDescriptor {
                    name: "id".to_string(),
                    node: DescriptorNode::Uuid,
                },
                FieldDescriptor {
                    name: "amount".to_string(),
                    node: DescriptorNode::U64,
                },
                FieldDescriptor {
                    name: "note".to_string(),
                    node: DescriptorNode::EventString { max_bytes: 256 },
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

    let dir = TestDir::new("schema_adapter");
    let store_path = dir.path().join("schema_store");
    let adapter = FileStorageAdapter::new(&store_path);
    let claim = sample_claim(1);

    let mut writer = adapter.create(&claim).expect("create writer");
    writer
        .set_schema_descriptor(&valid_schema)
        .expect("set schema descriptor");
    drop(writer);

    let reader = adapter.open_read().expect("open read");
    assert_eq!(reader.schema_descriptor(), Some(&valid_schema));
    assert!(reader.validate_schema_completeness().is_ok());
}

#[test]
fn test_m4_format_vectors_roundtrip_on_filesystem_adapter() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let vector_path =
        std::path::Path::new(&manifest_dir).join("../../conformance/vectors/envelope.json");
    let content = fs::read_to_string(&vector_path).expect("read envelope.json");
    let json: serde_json::Value = serde_json::from_str(&content).expect("parse json");
    let vectors = json["vectors"].as_array().expect("vectors array");

    let dir = TestDir::new("format_vectors");
    let claim = sample_claim(1);

    for (idx, vec) in vectors.iter().enumerate() {
        let expected_outcome = vec["expected_outcome"].as_str().unwrap();
        if expected_outcome == "Success" {
            let bytes_hex = vec["bytes_hex"].as_str().unwrap();
            let bytes = hex::decode(bytes_hex).unwrap();
            let (env, _) = EventEnvelope::decode(&bytes).unwrap();
            let store_path = dir.path().join(format!("vector_store_{idx}"));
            let adapter = FileStorageAdapter::new(&store_path);
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
        }
    }
}

#[test]
fn test_m4_adapter_retirement_and_generation_records() {
    let dir = tempfile::tempdir().expect("tempdir");
    let base_path = dir.path().join("retired_source");
    let adapter = FileStorageAdapter::new(&base_path);
    let claim = sample_claim(1);
    adapter.create(&claim).expect("create");

    let mut writer = adapter.open_write(1).expect("open write");
    let env = EventEnvelope {
        header: EnvelopeHeader {
            event_id: [0x01; 16],
            fiber_id: [0xaa; 16],
            detached: false,
            precursor: [0u8; 16],
            precursor_hash: [0u8; 32],
        },
        payload: b"sample payload".to_vec(),
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
}

#[test]
fn test_m4_incremental_recovery_open_writer_and_reader_identical_state() {
    let dir = TestDir::new("m4_incremental_recovery");
    let stem = dir.path().join("store");
    let adapter = FileStorageAdapter::new(&stem);
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");

    let fiber1 = [0x11; 16];
    let fiber2 = [0x22; 16];
    let fiber3 = [0x33; 16];

    for i in 1..=5u8 {
        let event_id = [i; 16];
        let payload = format!("fiber1-event-{i}").into_bytes();
        writer
            .append_to_fiber(fiber1, event_id, payload)
            .expect("append fiber1");
    }

    for i in 6..=10u8 {
        let event_id = [i; 16];
        let payload = format!("fiber2-event-{i}").into_bytes();
        writer
            .append_to_fiber(fiber2, event_id, payload)
            .expect("append fiber2");
    }

    for i in 11..=15u8 {
        let event_id = [i; 16];
        let payload = format!("fiber3-event-{i}").into_bytes();
        writer
            .append_to_fiber(fiber3, event_id, payload)
            .expect("append fiber3");
    }

    let orig_digest = writer.rolling_commitment().current_commitment();
    let orig_count = writer.rolling_commitment().frame_count();
    assert_eq!(orig_count, 15);

    let orig_h1_count = writer.fiber(fiber1).expect("h1").event_count();
    let orig_h1_prec = writer.fiber(fiber1).expect("h1").precursor();
    let orig_h2_count = writer.fiber(fiber2).expect("h2").event_count();
    let orig_h2_prec = writer.fiber(fiber2).expect("h2").precursor();
    let orig_h3_count = writer.fiber(fiber3).expect("h3").event_count();
    let orig_h3_prec = writer.fiber(fiber3).expect("h3").precursor();

    let orig_id1 = writer
        .get_latest(fiber1)
        .expect("latest1")
        .unwrap()
        .header
        .event_id;
    let orig_id2 = writer
        .get_latest(fiber2)
        .expect("latest2")
        .unwrap()
        .header
        .event_id;
    let orig_id3 = writer
        .get_latest(fiber3)
        .expect("latest3")
        .unwrap()
        .header
        .event_id;
    drop(writer);

    let reopened_writer = adapter.open_write(1).expect("reopen writer");
    assert_eq!(
        reopened_writer.rolling_commitment().current_commitment(),
        orig_digest
    );
    assert_eq!(
        reopened_writer.rolling_commitment().frame_count(),
        orig_count
    );
    let w_h1 = reopened_writer.fiber(fiber1).expect("w h1");
    let w_h2 = reopened_writer.fiber(fiber2).expect("w h2");
    let w_h3 = reopened_writer.fiber(fiber3).expect("w h3");
    assert_eq!(w_h1.event_count(), orig_h1_count);
    assert_eq!(w_h1.precursor(), orig_h1_prec);
    assert_eq!(w_h2.event_count(), orig_h2_count);
    assert_eq!(w_h2.precursor(), orig_h2_prec);
    assert_eq!(w_h3.event_count(), orig_h3_count);
    assert_eq!(w_h3.precursor(), orig_h3_prec);

    let w_latest1 = reopened_writer.get_latest(fiber1).expect("w l1").unwrap();
    let w_latest2 = reopened_writer.get_latest(fiber2).expect("w l2").unwrap();
    let w_latest3 = reopened_writer.get_latest(fiber3).expect("w l3").unwrap();
    assert_eq!(w_latest1.header.event_id, orig_id1);
    assert_eq!(w_latest2.header.event_id, orig_id2);
    assert_eq!(w_latest3.header.event_id, orig_id3);

    let reopened_reader = adapter.open_read().expect("open reader");
    assert_eq!(
        reopened_reader.rolling_commitment().current_commitment(),
        orig_digest
    );
    assert_eq!(
        reopened_reader.rolling_commitment().frame_count(),
        orig_count
    );
    let r_h1 = reopened_reader.fiber(fiber1).expect("r h1");
    let r_h2 = reopened_reader.fiber(fiber2).expect("r h2");
    let r_h3 = reopened_reader.fiber(fiber3).expect("r h3");
    assert_eq!(r_h1.event_count(), orig_h1_count);
    assert_eq!(r_h1.precursor(), orig_h1_prec);
    assert_eq!(r_h2.event_count(), orig_h2_count);
    assert_eq!(r_h2.precursor(), orig_h2_prec);
    assert_eq!(r_h3.event_count(), orig_h3_count);
    assert_eq!(r_h3.precursor(), orig_h3_prec);

    let r_latest1 = reopened_reader.get_latest(fiber1).expect("r l1").unwrap();
    let r_latest2 = reopened_reader.get_latest(fiber2).expect("r l2").unwrap();
    let r_latest3 = reopened_reader.get_latest(fiber3).expect("r l3").unwrap();
    assert_eq!(r_latest1.header.event_id, orig_id1);
    assert_eq!(r_latest2.header.event_id, orig_id2);
    assert_eq!(r_latest3.header.event_id, orig_id3);
}

#[test]
fn test_m4_fold_and_for_each_envelopes_borrowed_views() {
    let dir = tempfile::tempdir().expect("tempdir");
    let base_path = dir.path().join("fold_views");
    let adapter = FileStorageAdapter::new(&base_path);
    let claim = sample_claim(1);
    let mut writer = adapter.create(&claim).expect("create writer");

    let fiber = [0x77; 16];
    let mut expected_envelopes = Vec::new();
    for i in 1..=20u8 {
        let event_id = [i; 16];
        let payload = format!("fold-payload-{i}").into_bytes();
        let verdict = writer
            .append_to_fiber(fiber, event_id, payload)
            .expect("append");
        if let WriteLandingVerdict::Landed(env) = verdict {
            expected_envelopes.push(env);
        }
    }
    drop(writer);

    let mut reader = adapter.open_read().expect("open reader");

    let folded_items = reader
        .fold_envelopes(Vec::new(), |mut acc, env_ref| {
            acc.push((env_ref.header.event_id, env_ref.payload.to_vec()));
            Ok(acc)
        })
        .expect("fold envelopes");
    assert_eq!(folded_items.len(), 20);
    for (idx, (event_id, payload)) in folded_items.iter().enumerate() {
        assert_eq!(event_id, &expected_envelopes[idx].header.event_id);
        assert_eq!(payload, &expected_envelopes[idx].payload);
    }

    let mut visited_count = 0;
    reader
        .for_each_envelope(|env_ref| {
            assert_eq!(
                env_ref.header.event_id,
                expected_envelopes[visited_count].header.event_id
            );
            assert_eq!(
                env_ref.payload,
                expected_envelopes[visited_count].payload.as_slice()
            );
            visited_count += 1;
            Ok(())
        })
        .expect("for_each_envelope");
    assert_eq!(visited_count, 20);
}

#[test]
fn test_file_reopen_same_stream_append_continuation_matches_batch() {
    let dir = TestDir::new("reopen_continuation");
    let stem_a = dir.path().join("store_a");
    let stem_b = dir.path().join("store_b");

    let adapter_a = FileStorageAdapter::new(&stem_a);
    let adapter_b = FileStorageAdapter::new(&stem_b);
    let claim = sample_claim(1);

    let fiber1 = [0x11; 16];
    let fiber2 = [0x22; 16];
    let fiber3 = [0x33; 16];

    let mut events_n = Vec::new();
    let mut event_num = 1u8;
    for _round in 0..4 {
        events_n.push((fiber1, [event_num; 16], format!("payload-{event_num}").into_bytes()));
        event_num += 1;
        events_n.push((fiber2, [event_num; 16], format!("payload-{event_num}").into_bytes()));
        event_num += 1;
        events_n.push((fiber3, [event_num; 16], format!("payload-{event_num}").into_bytes()));
        event_num += 1;
    }
    assert_eq!(events_n.len(), 12);

    let mut events_m = Vec::new();
    for _round in 0..3 {
        events_m.push((fiber1, [event_num; 16], format!("payload-{event_num}").into_bytes()));
        event_num += 1;
        events_m.push((fiber2, [event_num; 16], format!("payload-{event_num}").into_bytes()));
        event_num += 1;
        events_m.push((fiber3, [event_num; 16], format!("payload-{event_num}").into_bytes()));
        event_num += 1;
    }
    assert_eq!(events_m.len(), 9);

    let mut writer_a = adapter_a.create(&claim).expect("create writer a");
    for (fiber_id, event_id, payload) in &events_n {
        writer_a
            .append_to_fiber(*fiber_id, *event_id, payload.clone())
            .expect("append to writer a");
    }
    drop(writer_a);

    let mut reader_a = adapter_a.open_read().expect("open reader a mid");
    let mid_envelopes = reader_a.read_all_envelopes().expect("read all mid");
    assert_eq!(mid_envelopes.len(), 12);
    drop(reader_a);

    let mut writer_a = adapter_a.open_write(1).expect("reopen writer a");
    for (fiber_id, event_id, payload) in &events_m {
        writer_a
            .append_to_fiber(*fiber_id, *event_id, payload.clone())
            .expect("append continuation to writer a");
    }
    drop(writer_a);

    let mut writer_b = adapter_b.create(&claim).expect("create writer b");
    for (fiber_id, event_id, payload) in &events_n {
        writer_b
            .append_to_fiber(*fiber_id, *event_id, payload.clone())
            .expect("batch append n to writer b");
    }
    for (fiber_id, event_id, payload) in &events_m {
        writer_b
            .append_to_fiber(*fiber_id, *event_id, payload.clone())
            .expect("batch append m to writer b");
    }
    drop(writer_b);

    let final_writer_a = adapter_a.open_write(1).expect("final open write a");
    let final_writer_b = adapter_b.open_write(1).expect("final open write b");

    assert_eq!(
        final_writer_a.rolling_commitment().current_commitment(),
        final_writer_b.rolling_commitment().current_commitment()
    );
    assert_eq!(
        final_writer_a.rolling_commitment().frame_count(),
        final_writer_b.rolling_commitment().frame_count()
    );
    assert_eq!(final_writer_a.rolling_commitment().frame_count(), 21);

    for fiber_id in [fiber1, fiber2, fiber3] {
        let handle_a = final_writer_a.fiber(fiber_id).expect("fiber handle a");
        let handle_b = final_writer_b.fiber(fiber_id).expect("fiber handle b");
        assert_eq!(handle_a.event_count(), handle_b.event_count());
        assert_eq!(handle_a.precursor(), handle_b.precursor());
        assert_eq!(handle_a.state(), handle_b.state());

        let latest_a = final_writer_a.get_latest(fiber_id).expect("latest a").unwrap();
        let latest_b = final_writer_b.get_latest(fiber_id).expect("latest b").unwrap();
        assert_eq!(latest_a.header.event_id, latest_b.header.event_id);
        assert_eq!(latest_a.header.precursor, latest_b.header.precursor);
        assert_eq!(latest_a.header.fiber_id, latest_b.header.fiber_id);
        assert_eq!(latest_a.payload, latest_b.payload);
    }
    drop(final_writer_a);
    drop(final_writer_b);

    let mut final_reader_a = adapter_a.open_read().expect("final open read a");
    let mut final_reader_b = adapter_b.open_read().expect("final open read b");
    let envs_a = final_reader_a.read_all_envelopes().expect("read all a");
    let envs_b = final_reader_b.read_all_envelopes().expect("read all b");
    assert_eq!(envs_a.len(), 21);
    assert_eq!(envs_b.len(), 21);

    for (ea, eb) in envs_a.iter().zip(envs_b.iter()) {
        assert_eq!(ea.header.event_id, eb.header.event_id);
        assert_eq!(ea.header.fiber_id, eb.header.fiber_id);
        assert_eq!(ea.header.precursor, eb.header.precursor);
        assert_eq!(ea.header.precursor_hash, eb.header.precursor_hash);
        assert_eq!(ea.header.detached, eb.header.detached);
        assert_eq!(ea.payload, eb.payload);
    }
}

#[test]
fn test_file_read_meta_records_missing_seq1_fails_closed() {
    let dir = TestDir::new("meta_missing_seq1");
    let meta_path = dir.path().join("corrupted.meta");
    fs::write(&meta_path, b"not_a_header").expect("write invalid header");

    let err = pardosa::file::read_meta_records(&meta_path).expect_err("must fail closed");
    assert_eq!(*err.condition(), FailureCondition::OwnershipRecordUnreadable);

    let empty_meta_path = dir.path().join("empty.meta");
    fs::write(&empty_meta_path, b"").expect("write empty meta");
    let err_empty = pardosa::file::read_meta_records(&empty_meta_path).expect_err("must fail closed on empty");
    assert_eq!(*err_empty.condition(), FailureCondition::OwnershipRecordUnreadable);
}

#[test]
fn test_file_open_write_concurrent_writer_rejected_while_holding_lock() {
    let dir = TestDir::new("open_write_concurrent");
    let base_path = dir.path().join("store");
    let adapter = FileStorageAdapter::new(&base_path);
    let claim = sample_claim(1);

    let writer = adapter.create(&claim).expect("create");
    drop(writer);

    let writer1 = adapter.open_write(1).expect("first open write holds exclusion");
    let err = adapter.open_write(1).expect_err("concurrent open write must be rejected");
    assert_eq!(*err.condition(), FailureCondition::AnotherOwnerHoldsExclusion);

    drop(writer1);

    let writer2 = adapter.open_write(1).expect("open write succeeds after lock released");
    drop(writer2);
}
