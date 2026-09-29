#![allow(clippy::pedantic)]

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

    let err_new = MigrationManager::new(source, target).unwrap_err();
    assert_eq!(
        *err_new.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(err_new.to_string().contains("live migration is disabled in this release per approved constrained release decision; use offline administrative migration"));
}

#[test]
fn test_compile_fail_migration_new_for_test_removed() {
    let code = r#"
#![allow(clippy::pedantic)]

use pardosa::file::FileStorageAdapter;
        use pardosa::migration::MigrationManager;

        pub fn check() {
            let dir = std::path::PathBuf::from("/tmp");
            let source = FileStorageAdapter::new(dir.join("src"));
            let target = FileStorageAdapter::new(dir.join("dst"));
            let _ = MigrationManager::new_for_test(source, target);
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(!ok, "MigrationManager::new_for_test must not exist");
    assert!(
        stderr.contains("new_for_test")
            && (stderr.contains("not found") || stderr.contains("no associated function")),
        "stderr must indicate new_for_test is absent:\n{stderr}"
    );
}

#[test]
fn test_m6_migration_basic_lifecycle_and_cutover() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source_path = dir.path().join("source");
    let target_path = dir.path().join("target");

    let source = FileStorageAdapter::new(&source_path);
    let target = FileStorageAdapter::new(&target_path);

    let claim1 = sample_claim(1);
    source
        .create(&claim1, &AdmittedDescriptor::default_for_test())
        .expect("create source");
    let claim2 = sample_claim(1);
    target
        .create(&claim2, &AdmittedDescriptor::default_for_test())
        .expect("create target");

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
    source
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create source");
    target
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create target");

    let mut writer = source.open_write(1).expect("open write source");
    let env1 = sample_genesis_envelope(1, 0xaa, b"input_data");
    writer.append_envelope_verdict(&env1).expect("append 1");

    let err = MigrationManager::new(source.clone(), target).unwrap_err();
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
fn test_m6_live_migration_refused_with_populated_single_fiber() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_policies"));
    let target = FileStorageAdapter::new(dir.path().join("target_policies"));

    let claim = sample_claim(1);
    source
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create source");
    target
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create target");

    let mut writer = source.open_write(1).expect("open write source");

    let a1 = sample_genesis_envelope(1, 0x11, b"a1");
    let comm_a1 = a1.commitment();
    let a2 = sample_chained_envelope(2, 0x11, 1, comm_a1, b"a2");
    let comm_a2 = a2.commitment();
    let a3 = sample_chained_envelope(3, 0x11, 2, comm_a2, b"a3");

    writer.append_envelope_verdict(&a1).expect("append a1");
    writer.append_envelope_verdict(&a2).expect("append a2");
    writer.append_envelope_verdict(&a3).expect("append a3");

    let err = MigrationManager::new(source.clone(), target).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(!source.is_retired_source().expect("query retired"));
}

#[test]
fn test_m6_live_migration_refused_with_interleaved_fibers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_order"));
    let target = FileStorageAdapter::new(dir.path().join("target_order"));

    let claim = sample_claim(1);
    source
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create source");
    target
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create target");

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

    let err = MigrationManager::new(source.clone(), target).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(!source.is_retired_source().expect("query retired"));
}

#[test]
fn test_m6_broken_history_readable_for_migration_but_live_manager_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source_refuse = FileStorageAdapter::new(dir.path().join("source_refuse"));
    let target_refuse = FileStorageAdapter::new(dir.path().join("target_refuse"));

    let claim = sample_claim(1);
    source_refuse
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create source refuse");
    target_refuse
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create target refuse");

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
    broken_env.encode(&mut broken_buf).unwrap();
    drop(writer);
    source_refuse
        .append_unvalidated_frame_for_test(&broken_buf)
        .expect("append broken raw frame");

    let err_refuse = MigrationManager::new(source_refuse.clone(), target_refuse).unwrap_err();
    assert_eq!(
        *err_refuse.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(!source_refuse.is_retired_source().expect("query retired"));

    let source_permit = FileStorageAdapter::new(dir.path().join("source_permit"));
    let target_permit = FileStorageAdapter::new(dir.path().join("target_permit"));
    source_permit
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create source permit");
    target_permit
        .create(&claim, &AdmittedDescriptor::default_for_test())
        .expect("create target permit");

    let mut writer_permit = source_permit
        .open_write(1)
        .expect("open write source permit");
    writer_permit
        .append_envelope_verdict(&env1)
        .expect("append env1");
    drop(writer_permit);
    source_permit
        .append_unvalidated_frame_for_test(&broken_buf)
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

    let err_permit = MigrationManager::new(source_permit, target_permit).unwrap_err();
    assert_eq!(
        *err_permit.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
}

#[test]
fn test_m6_live_migration_refusal_recommends_offline_administration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_chase"));
    let target = FileStorageAdapter::new(dir.path().join("target_chase"));

    let err = MigrationManager::new(source, target).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(err.to_string().contains("live migration is disabled in this release per approved constrained release decision; use offline administrative migration"));
}

#[test]
fn test_m6_live_migration_refused_before_source_or_target_creation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = FileStorageAdapter::new(dir.path().join("source_retry"));
    let target = FileStorageAdapter::new(dir.path().join("target_retry"));

    let err = MigrationManager::new(source, target).unwrap_err();
    assert_eq!(
        *err.condition(),
        FailureCondition::InvariantBreakingConfiguration
    );
    assert!(err.to_string().contains("live migration is disabled in this release per approved constrained release decision; use offline administrative migration"));
}

fn run_rustc(code: &str) -> (bool, String) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;
    static ARTIFACT: OnceLock<std::path::PathBuf> = OnceLock::new();
    let artifact = ARTIFACT.get_or_init(|| {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let mut command = Command::new(env!("CARGO"));
        command
            .args([
                "build",
                "--locked",
                "--lib",
                "--message-format=json",
                "--manifest-path",
            ])
            .arg(&manifest)
            .args(["-p", env!("CARGO_PKG_NAME")]);
        if cfg!(not(debug_assertions)) {
            command.arg("--release");
        }
        let output = command.output().expect("build package proof artifact");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let records: Vec<serde_json::Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).expect("Cargo JSON"))
            .collect();
        let matches: Vec<_> = records
            .iter()
            .filter(|r| {
                r["reason"] == "compiler-artifact"
                    && r["manifest_path"].as_str() == manifest.to_str()
                    && r["target"]["name"] == "pardosa"
                    && r["target"]["kind"] == serde_json::json!(["lib"])
                    && r["features"] == serde_json::json!(["default", "uuid"])
            })
            .collect();
        assert_eq!(matches.len(), 1, "exact package/target/features");
        let paths: Vec<_> = matches[0]["filenames"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str())
            .filter(|p| p.ends_with(".rlib"))
            .collect();
        assert_eq!(paths.len(), 1);
        paths[0].into()
    });
    let mut child = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
        .args(["--edition=2021", "--crate-type=lib", "--emit=mir=-", "-"])
        .arg("--extern")
        .arg(format!("pardosa={}", artifact.display()))
        .arg("-L")
        .arg(format!(
            "dependency={}",
            artifact.parent().unwrap().join("deps").display()
        ))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rustc");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(code.as_bytes())
        .expect("write source");
    let output = child.wait_with_output().expect("wait rustc");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn test_compile_success_strict_adjacent_migration() {
    let code = r#"
        use pardosa::migration::{AdjacentMigration, MigrationWitness, VersionMigration};
        use pardosa::schema::{DescriptorNode, PardosaSchema};

        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct EventV1 {
            pub value: u32,
        }

        impl PardosaSchema for EventV1 {
            const SCHEMA_VERSION: u32 = 1;
            fn schema_descriptor() -> DescriptorNode {
                DescriptorNode::U32
            }
            fn encode_payload(&self, buf: &mut Vec<u8>) -> Result<(), pardosa::encoding::EncodeError> {
                buf.extend_from_slice(&self.value.to_le_bytes());
                Ok(())
            }
            fn decode_payload(buf: &[u8]) -> Result<Self, pardosa::encoding::DecodeError> {
                if buf.len() < 4 {
                    return Err(pardosa::encoding::DecodeError::TruncatedPayload {
                        expected: 4,
                        available: buf.len(),
                    });
                }
                let value = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
                Ok(Self { value })
            }
        }

        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct EventV2 {
            pub value: u64,
        }

        impl PardosaSchema for EventV2 {
            const SCHEMA_VERSION: u32 = 2;
            fn schema_descriptor() -> DescriptorNode {
                DescriptorNode::U64
            }
            fn encode_payload(&self, buf: &mut Vec<u8>) -> Result<(), pardosa::encoding::EncodeError> {
                buf.extend_from_slice(&self.value.to_le_bytes());
                Ok(())
            }
            fn decode_payload(buf: &[u8]) -> Result<Self, pardosa::encoding::DecodeError> {
                if buf.len() < 8 {
                    return Err(pardosa::encoding::DecodeError::TruncatedPayload {
                        expected: 8,
                        available: buf.len(),
                    });
                }
                let value = u64::from_le_bytes([
                    buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
                ]);
                Ok(Self { value })
            }
        }

        pub struct MigrateV1ToV2;

        impl VersionMigration for MigrateV1ToV2 {
            type SourceEvent = EventV1;
            type TargetEvent = EventV2;
            type Error = std::convert::Infallible;

            fn transform(&self, event: &Self::SourceEvent) -> Result<Self::TargetEvent, Self::Error> {
                Ok(EventV2 {
                    value: event.value as u64,
                })
            }
        }

        pub fn check() {
            MigrationWitness::<EventV1, EventV2>::assert_adjacent();
            let migration = AdjacentMigration::new(MigrateV1ToV2);
            let v1 = EventV1 { value: 42 };
            let v2 = migration.transform(&v1).unwrap();
            assert_eq!(v2.value, 42);
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(ok, "strict n->n+1 positive control must compile:\n{stderr}");
}

#[test]
fn test_compile_fail_migration_skip_version_n_to_n_plus_2() {
    let code = r#"
        use pardosa::migration::{AdjacentMigration, VersionMigration};
        use pardosa::schema::{DescriptorNode, PardosaSchema};

        pub struct EventV1;
        impl PardosaSchema for EventV1 {
            const SCHEMA_VERSION: u32 = 1;
            fn schema_descriptor() -> DescriptorNode { DescriptorNode::U8 }
            fn encode_payload(&self, _: &mut Vec<u8>) -> Result<(), pardosa::encoding::EncodeError> { Ok(()) }
            fn decode_payload(_: &[u8]) -> Result<Self, pardosa::encoding::DecodeError> { Ok(Self) }
        }

        pub struct EventV3;
        impl PardosaSchema for EventV3 {
            const SCHEMA_VERSION: u32 = 3;
            fn schema_descriptor() -> DescriptorNode { DescriptorNode::U8 }
            fn encode_payload(&self, _: &mut Vec<u8>) -> Result<(), pardosa::encoding::EncodeError> { Ok(()) }
            fn decode_payload(_: &[u8]) -> Result<Self, pardosa::encoding::DecodeError> { Ok(Self) }
        }

        pub struct SkipMigration;
        impl VersionMigration for SkipMigration {
            type SourceEvent = EventV1;
            type TargetEvent = EventV3;
            type Error = std::convert::Infallible;
            fn transform(&self, _: &Self::SourceEvent) -> Result<Self::TargetEvent, Self::Error> { Ok(EventV3) }
        }

        pub fn check() {
            let _ = AdjacentMigration::new(SkipMigration);
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(!ok, "n -> n+2 migration must fail compilation");
    assert!(
        stderr.contains("Migration must be strict n -> n+1"),
        "stderr must cite strict n -> n+1 invariant:\n{stderr}"
    );
}

#[test]
fn test_compile_fail_migration_backwards_version() {
    let code = r#"
        use pardosa::migration::{AdjacentMigration, VersionMigration};
        use pardosa::schema::{DescriptorNode, PardosaSchema};

        pub struct EventV2;
        impl PardosaSchema for EventV2 {
            const SCHEMA_VERSION: u32 = 2;
            fn schema_descriptor() -> DescriptorNode { DescriptorNode::U8 }
            fn encode_payload(&self, _: &mut Vec<u8>) -> Result<(), pardosa::encoding::EncodeError> { Ok(()) }
            fn decode_payload(_: &[u8]) -> Result<Self, pardosa::encoding::DecodeError> { Ok(Self) }
        }

        pub struct EventV1;
        impl PardosaSchema for EventV1 {
            const SCHEMA_VERSION: u32 = 1;
            fn schema_descriptor() -> DescriptorNode { DescriptorNode::U8 }
            fn encode_payload(&self, _: &mut Vec<u8>) -> Result<(), pardosa::encoding::EncodeError> { Ok(()) }
            fn decode_payload(_: &[u8]) -> Result<Self, pardosa::encoding::DecodeError> { Ok(Self) }
        }

        pub struct BackwardMigration;
        impl VersionMigration for BackwardMigration {
            type SourceEvent = EventV2;
            type TargetEvent = EventV1;
            type Error = std::convert::Infallible;
            fn transform(&self, _: &Self::SourceEvent) -> Result<Self::TargetEvent, Self::Error> { Ok(EventV1) }
        }

        pub fn check() {
            let _ = AdjacentMigration::new(BackwardMigration);
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(!ok, "backwards migration must fail compilation");
    assert!(
        stderr.contains("Migration must be strict n -> n+1"),
        "stderr must cite strict n -> n+1 invariant:\n{stderr}"
    );
}

#[test]
fn test_compile_fail_migration_same_version() {
    let code = r#"
        use pardosa::migration::{AdjacentMigration, VersionMigration};
        use pardosa::schema::{DescriptorNode, PardosaSchema};

        pub struct EventV1;
        impl PardosaSchema for EventV1 {
            const SCHEMA_VERSION: u32 = 1;
            fn schema_descriptor() -> DescriptorNode { DescriptorNode::U8 }
            fn encode_payload(&self, _: &mut Vec<u8>) -> Result<(), pardosa::encoding::EncodeError> { Ok(()) }
            fn decode_payload(_: &[u8]) -> Result<Self, pardosa::encoding::DecodeError> { Ok(Self) }
        }

        pub struct SameVersionMigration;
        impl VersionMigration for SameVersionMigration {
            type SourceEvent = EventV1;
            type TargetEvent = EventV1;
            type Error = std::convert::Infallible;
            fn transform(&self, _: &Self::SourceEvent) -> Result<Self::TargetEvent, Self::Error> { Ok(EventV1) }
        }

        pub fn check() {
            let _ = AdjacentMigration::new(SameVersionMigration);
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(!ok, "same-version migration must fail compilation");
    assert!(
        stderr.contains("Migration must be strict n -> n+1"),
        "stderr must cite strict n -> n+1 invariant:\n{stderr}"
    );
}

#[test]
fn test_compile_fail_migration_source_schema_version_max() {
    let code = r#"
        use pardosa::migration::{AdjacentMigration, VersionMigration};
        use pardosa::schema::{DescriptorNode, PardosaSchema};

        pub struct EventMax;
        impl PardosaSchema for EventMax {
            const SCHEMA_VERSION: u32 = u32::MAX;
            fn schema_descriptor() -> DescriptorNode { DescriptorNode::U8 }
            fn encode_payload(&self, _: &mut Vec<u8>) -> Result<(), pardosa::encoding::EncodeError> { Ok(()) }
            fn decode_payload(_: &[u8]) -> Result<Self, pardosa::encoding::DecodeError> { Ok(Self) }
        }

        pub struct EventWrapped;
        impl PardosaSchema for EventWrapped {
            const SCHEMA_VERSION: u32 = 0;
            fn schema_descriptor() -> DescriptorNode { DescriptorNode::U8 }
            fn encode_payload(&self, _: &mut Vec<u8>) -> Result<(), pardosa::encoding::EncodeError> { Ok(()) }
            fn decode_payload(_: &[u8]) -> Result<Self, pardosa::encoding::DecodeError> { Ok(Self) }
        }

        pub struct OverflowMigration;
        impl VersionMigration for OverflowMigration {
            type SourceEvent = EventMax;
            type TargetEvent = EventWrapped;
            type Error = std::convert::Infallible;
            fn transform(&self, _: &Self::SourceEvent) -> Result<Self::TargetEvent, Self::Error> { Ok(EventWrapped) }
        }

        pub fn check() {
            let _ = AdjacentMigration::new(OverflowMigration);
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(!ok, "u32::MAX source schema version must fail compilation");
    assert!(
        stderr.contains("Source schema version must be less than u32::MAX"),
        "stderr must cite u32::MAX constraint:\n{stderr}"
    );
}

#[test]
fn test_compile_fail_missing_mandatory_schema_version_const() {
    let code = r#"
        use pardosa::schema::{DescriptorNode, PardosaSchema};

        pub struct MissingConstEvent;
        impl PardosaSchema for MissingConstEvent {
            fn schema_descriptor() -> DescriptorNode { DescriptorNode::U8 }
            fn encode_payload(&self, _: &mut Vec<u8>) -> Result<(), pardosa::encoding::EncodeError> { Ok(()) }
            fn decode_payload(_: &[u8]) -> Result<Self, pardosa::encoding::DecodeError> { Ok(Self) }
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(!ok, "omitting const SCHEMA_VERSION must fail compilation");
    assert!(
        stderr.contains("SCHEMA_VERSION")
            || stderr.contains("not all trait items implemented, missing: `SCHEMA_VERSION`"),
        "stderr must cite missing SCHEMA_VERSION item:\n{stderr}"
    );
}
