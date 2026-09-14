use pardosa::prelude::*;

#[derive(Debug, PartialEq, Eq, PardosaSchema)]
#[repr(u8)]
#[pardosa(version = 1)]
enum UserEvent {
    #[pardosa(tombstone)]
    Tombstone = 0,
    UserCreated(u32) = 1,
    UserUpdated {
        id: u32,
        name: EventString<32>,
    } = 2,
    UserDetached = 3,
}

#[test]
fn test_readme_derived_order_event_compiles() {
    #[derive(Debug, Clone, PartialEq, Eq, PardosaSchema)]
    #[repr(u8)]
    #[pardosa(version = 1)]
    enum OrderEvent {
        #[pardosa(tombstone)]
        Tombstone = 0,
        Created {
            order_id: EventString<64>,
            amount_cents: u64,
        } = 1,
        Shipped {
            tracking_number: EventString<64>,
        } = 2,
        Cancelled = 3,
    }

    assert_eq!(OrderEvent::SCHEMA_VERSION, 1);
    let identity = OrderEvent::schema_identity();
    assert_ne!(identity.as_bytes(), &[0u8; 32]);
}

#[test]
fn test_user_event_schema_version_and_identity() {
    assert_eq!(UserEvent::SCHEMA_VERSION, 1);
    let desc = UserEvent::schema_descriptor();
    match desc {
        DescriptorNode::Enum {
            name,
            discriminant_width,
            variants,
        } => {
            assert_eq!(name, "UserEvent");
            assert_eq!(discriminant_width, 1);
            assert_eq!(variants.len(), 4);
            assert_eq!(variants[0].discriminant, 0);
            assert_eq!(variants[0].name, "Tombstone");
            assert_eq!(variants[0].payload, None);
            assert_eq!(variants[1].discriminant, 1);
            assert_eq!(variants[1].name, "UserCreated");
            assert_eq!(variants[2].discriminant, 2);
            assert_eq!(variants[2].name, "UserUpdated");
            assert_eq!(variants[3].discriminant, 3);
            assert_eq!(variants[3].name, "UserDetached");
        }
        other => panic!("expected Enum descriptor node, found {:?}", other),
    }

    let identity = UserEvent::schema_identity();
    assert_ne!(identity.as_bytes(), &[0u8; 32]);
}

#[test]
fn test_user_event_codecs_roundtrip() {
    let tombstone = UserEvent::Tombstone;
    let mut buf = Vec::new();
    tombstone.encode_payload(&mut buf).unwrap();
    assert_eq!(buf, vec![0x00]);
    let decoded = UserEvent::decode_payload(&buf).unwrap();
    assert_eq!(decoded, tombstone);

    let created = UserEvent::UserCreated(42);
    buf.clear();
    created.encode_payload(&mut buf).unwrap();
    assert_eq!(buf, vec![0x01, 0x2a, 0x00, 0x00, 0x00]);
    let decoded = UserEvent::decode_payload(&buf).unwrap();
    assert_eq!(decoded, created);

    let updated = UserEvent::UserUpdated {
        id: 99,
        name: EventString::new("Alice").unwrap(),
    };
    buf.clear();
    updated.encode_payload(&mut buf).unwrap();
    let decoded = UserEvent::decode_payload(&buf).unwrap();
    assert_eq!(decoded, updated);

    let detached = UserEvent::UserDetached;
    buf.clear();
    detached.encode_payload(&mut buf).unwrap();
    let decoded = UserEvent::decode_payload(&buf).unwrap();
    assert_eq!(decoded, detached);
}

#[test]
fn test_user_event_unknown_discriminant_rejection() {
    let err = UserEvent::decode_payload(&[0x99]).unwrap_err();
    match err {
        DecodeError::UnknownVariantDiscriminant { discriminant } => {
            assert_eq!(discriminant, 0x99);
        }
        other => panic!("expected UnknownVariantDiscriminant, found {:?}", other),
    }
}

#[test]
fn test_user_event_truncated_rejection() {
    let err = UserEvent::decode_payload(&[]).unwrap_err();
    assert_eq!(err.error_kind(), "TruncatedPayload");
}

#[test]
fn test_derived_payload_trailing_bytes_rejection() {
    let tombstone_trailing = [0x00, 0xFF];
    let err = UserEvent::decode_payload(&tombstone_trailing).unwrap_err();
    assert_eq!(err.error_kind(), "TruncatedPayload");

    let created_trailing = [0x01, 0x2a, 0x00, 0x00, 0x00, 0xAA];
    let err = UserEvent::decode_payload(&created_trailing).unwrap_err();
    assert_eq!(err.error_kind(), "TruncatedPayload");

    let mut updated_trailing = Vec::new();
    let updated = UserEvent::UserUpdated {
        id: 99,
        name: EventString::new("Alice").unwrap(),
    };
    updated.encode_payload(&mut updated_trailing).unwrap();
    updated_trailing.push(0xBB);
    let err = UserEvent::decode_payload(&updated_trailing).unwrap_err();
    assert_eq!(err.error_kind(), "TruncatedPayload");

    let detached_trailing = [0x03, 0xCC];
    let err = UserEvent::decode_payload(&detached_trailing).unwrap_err();
    assert_eq!(err.error_kind(), "TruncatedPayload");
}

mod f64_ordered_module {
    use pardosa::prelude::*;

    #[derive(Debug, PartialEq, Eq, PardosaSchema)]
    #[repr(u8)]
    #[pardosa(version = 1)]
    pub enum FloatPayloadEvent {
        #[pardosa(tombstone)]
        Tombstone = 0,
        Measurement {
            sensor_id: u32,
            value: OrderedF64,
            label: EventString<32>,
        } = 1,
    }
}

mod f64_event_module {
    use pardosa::prelude::*;

    #[derive(Debug, PartialEq, Eq, PardosaSchema)]
    #[repr(u8)]
    #[pardosa(version = 1)]
    pub enum FloatPayloadEvent {
        #[pardosa(tombstone)]
        Tombstone = 0,
        Measurement {
            sensor_id: u32,
            value: EventF64,
            label: EventString<32>,
        } = 1,
    }
}

mod f32_ordered_module {
    use pardosa::prelude::*;

    #[derive(Debug, PartialEq, Eq, PardosaSchema)]
    #[repr(u8)]
    #[pardosa(version = 1)]
    pub enum FloatPayloadEvent {
        #[pardosa(tombstone)]
        Tombstone = 0,
        Measurement {
            sensor_id: u32,
            value: OrderedF32,
            label: EventString<32>,
        } = 1,
    }
}

mod f32_event_module {
    use pardosa::prelude::*;

    #[derive(Debug, PartialEq, Eq, PardosaSchema)]
    #[repr(u8)]
    #[pardosa(version = 1)]
    pub enum FloatPayloadEvent {
        #[pardosa(tombstone)]
        Tombstone = 0,
        Measurement {
            sensor_id: u32,
            value: EventF32,
            label: EventString<32>,
        } = 1,
    }
}

#[test]
fn test_f64_derived_payload_schema_identity_and_admission_mismatch() {
    let desc_a = f64_ordered_module::FloatPayloadEvent::schema_descriptor();
    let desc_b = f64_event_module::FloatPayloadEvent::schema_descriptor();
    assert_ne!(desc_a, desc_b);

    let id_a = f64_ordered_module::FloatPayloadEvent::schema_identity();
    let id_b = f64_event_module::FloatPayloadEvent::schema_identity();
    assert_ne!(id_a, id_b);

    let schema_desc_a = SchemaDescriptor::new(1, desc_a);
    let schema_desc_b = SchemaDescriptor::new(1, desc_b);
    assert!(schema_desc_a.validate_structural_completeness().is_ok());
    assert!(schema_desc_b.validate_structural_completeness().is_ok());
    assert_eq!(schema_desc_a.identity(), id_a);
    assert_eq!(schema_desc_b.identity(), id_b);

    let mut enc_a = Vec::new();
    schema_desc_a.encode(&mut enc_a).unwrap();
    let (dec_desc_a, len_a) = SchemaDescriptor::decode(&enc_a).unwrap();
    assert_eq!(len_a, enc_a.len());
    assert_eq!(dec_desc_a, schema_desc_a);

    let mut enc_b = Vec::new();
    schema_desc_b.encode(&mut enc_b).unwrap();
    let (dec_desc_b, len_b) = SchemaDescriptor::decode(&enc_b).unwrap();
    assert_eq!(len_b, enc_b.len());
    assert_eq!(dec_desc_b, schema_desc_b);

    let event_a = f64_ordered_module::FloatPayloadEvent::Measurement {
        sensor_id: 101,
        value: OrderedF64::try_from(42.5f64).unwrap(),
        label: EventString::new("pressure").unwrap(),
    };
    let mut payload_a = Vec::new();
    event_a.encode_payload(&mut payload_a).unwrap();
    let decoded_a = f64_ordered_module::FloatPayloadEvent::decode_payload(&payload_a).unwrap();
    assert_eq!(decoded_a, event_a);

    let event_b = f64_event_module::FloatPayloadEvent::Measurement {
        sensor_id: 101,
        value: EventF64::Finite(OrderedF64::try_from(42.5f64).unwrap()),
        label: EventString::new("pressure").unwrap(),
    };
    let mut payload_b = Vec::new();
    event_b.encode_payload(&mut payload_b).unwrap();
    let decoded_b = f64_event_module::FloatPayloadEvent::decode_payload(&payload_b).unwrap();
    assert_eq!(decoded_b, event_b);

    let env_id = EnvelopeIdentity::from_raw([0x55; 32]);

    let admission_ok_a = EventAdmission::Event {
        descriptor: Some(&schema_desc_a),
        expected_schema: &id_a,
        actual_schema: &id_a,
        expected_envelope: &env_id,
        actual_envelope: &env_id,
    };
    assert!(admit_event(&admission_ok_a).is_ok());

    let admission_ok_b = EventAdmission::Event {
        descriptor: Some(&schema_desc_b),
        expected_schema: &id_b,
        actual_schema: &id_b,
        expected_envelope: &env_id,
        actual_envelope: &env_id,
    };
    assert!(admit_event(&admission_ok_b).is_ok());

    let admission_mismatch = EventAdmission::Event {
        descriptor: Some(&schema_desc_a),
        expected_schema: &id_b,
        actual_schema: &id_a,
        expected_envelope: &env_id,
        actual_envelope: &env_id,
    };
    let err = admit_event(&admission_mismatch).unwrap_err();
    assert_eq!(err.condition(), &FailureCondition::SchemaMismatch);
}

#[test]
fn test_f32_derived_payload_schema_identity_and_admission_mismatch() {
    let desc_a = f32_ordered_module::FloatPayloadEvent::schema_descriptor();
    let desc_b = f32_event_module::FloatPayloadEvent::schema_descriptor();
    assert_ne!(desc_a, desc_b);

    let id_a = f32_ordered_module::FloatPayloadEvent::schema_identity();
    let id_b = f32_event_module::FloatPayloadEvent::schema_identity();
    assert_ne!(id_a, id_b);

    let schema_desc_a = SchemaDescriptor::new(1, desc_a);
    let schema_desc_b = SchemaDescriptor::new(1, desc_b);
    assert!(schema_desc_a.validate_structural_completeness().is_ok());
    assert!(schema_desc_b.validate_structural_completeness().is_ok());
    assert_eq!(schema_desc_a.identity(), id_a);
    assert_eq!(schema_desc_b.identity(), id_b);

    let mut enc_a = Vec::new();
    schema_desc_a.encode(&mut enc_a).unwrap();
    let (dec_desc_a, len_a) = SchemaDescriptor::decode(&enc_a).unwrap();
    assert_eq!(len_a, enc_a.len());
    assert_eq!(dec_desc_a, schema_desc_a);

    let mut enc_b = Vec::new();
    schema_desc_b.encode(&mut enc_b).unwrap();
    let (dec_desc_b, len_b) = SchemaDescriptor::decode(&enc_b).unwrap();
    assert_eq!(len_b, enc_b.len());
    assert_eq!(dec_desc_b, schema_desc_b);

    let event_a = f32_ordered_module::FloatPayloadEvent::Measurement {
        sensor_id: 202,
        value: OrderedF32::try_from(19.25f32).unwrap(),
        label: EventString::new("temperature").unwrap(),
    };
    let mut payload_a = Vec::new();
    event_a.encode_payload(&mut payload_a).unwrap();
    let decoded_a = f32_ordered_module::FloatPayloadEvent::decode_payload(&payload_a).unwrap();
    assert_eq!(decoded_a, event_a);

    let event_b = f32_event_module::FloatPayloadEvent::Measurement {
        sensor_id: 202,
        value: EventF32::Finite(OrderedF32::try_from(19.25f32).unwrap()),
        label: EventString::new("temperature").unwrap(),
    };
    let mut payload_b = Vec::new();
    event_b.encode_payload(&mut payload_b).unwrap();
    let decoded_b = f32_event_module::FloatPayloadEvent::decode_payload(&payload_b).unwrap();
    assert_eq!(decoded_b, event_b);

    let env_id = EnvelopeIdentity::from_raw([0x66; 32]);

    let admission_ok_a = EventAdmission::Event {
        descriptor: Some(&schema_desc_a),
        expected_schema: &id_a,
        actual_schema: &id_a,
        expected_envelope: &env_id,
        actual_envelope: &env_id,
    };
    assert!(admit_event(&admission_ok_a).is_ok());

    let admission_ok_b = EventAdmission::Event {
        descriptor: Some(&schema_desc_b),
        expected_schema: &id_b,
        actual_schema: &id_b,
        expected_envelope: &env_id,
        actual_envelope: &env_id,
    };
    assert!(admit_event(&admission_ok_b).is_ok());

    let admission_mismatch = EventAdmission::Event {
        descriptor: Some(&schema_desc_a),
        expected_schema: &id_b,
        actual_schema: &id_a,
        expected_envelope: &env_id,
        actual_envelope: &env_id,
    };
    let err = admit_event(&admission_mismatch).unwrap_err();
    assert_eq!(err.condition(), &FailureCondition::SchemaMismatch);
}

#[test]
fn test_v21_schema_descriptor_golden_wire_bytes_and_conflicting_rejection() {
    let v21_descriptor = SchemaDescriptor::new(
        21,
        DescriptorNode::Enum {
            name: "EvidenceEvent".to_string(),
            discriminant_width: 1,
            variants: vec![
                VariantDescriptor {
                    discriminant: 0,
                    name: "Tombstone".to_string(),
                    payload: None,
                },
                VariantDescriptor {
                    discriminant: 1,
                    name: "Observed".to_string(),
                    payload: Some(DescriptorNode::Struct {
                        name: "EvidencePayload".to_string(),
                        fields: vec![
                            FieldDescriptor {
                                name: "entity_id".to_string(),
                                node: DescriptorNode::Uuid,
                            },
                            FieldDescriptor {
                                name: "timestamp".to_string(),
                                node: DescriptorNode::Timestamp,
                            },
                            FieldDescriptor {
                                name: "label".to_string(),
                                node: DescriptorNode::EventString { max_bytes: 64 },
                            },
                            FieldDescriptor {
                                name: "count".to_string(),
                                node: DescriptorNode::U64,
                            },
                        ],
                    }),
                },
            ],
        },
    );

    let admitted = AdmittedDescriptor::try_from_descriptor(v21_descriptor.clone()).unwrap();
    assert_eq!(admitted.version(), 21);

    let mut wire_bytes = Vec::new();
    v21_descriptor.encode(&mut wire_bytes).unwrap();

    let expected_golden_bytes: [u8; 131] = [
        21, 0, 0, 0, 16, 13, 0, 0, 0, 69, 118, 105, 100, 101, 110, 99, 101, 69, 118, 101, 110, 116,
        1, 2, 0, 0, 0, 0, 9, 0, 0, 0, 84, 111, 109, 98, 115, 116, 111, 110, 101, 0, 1, 8, 0, 0, 0,
        79, 98, 115, 101, 114, 118, 101, 100, 15, 15, 0, 0, 0, 69, 118, 105, 100, 101, 110, 99,
        101, 80, 97, 121, 108, 111, 97, 100, 4, 0, 0, 0, 9, 0, 0, 0, 101, 110, 116, 105, 116, 121,
        95, 105, 100, 18, 9, 0, 0, 0, 116, 105, 109, 101, 115, 116, 97, 109, 112, 17, 5, 0, 0, 0,
        108, 97, 98, 101, 108, 10, 64, 0, 0, 0, 5, 0, 0, 0, 99, 111, 117, 110, 116, 4,
    ];

    assert_eq!(wire_bytes.as_slice(), &expected_golden_bytes);

    let (decoded, consumed) = SchemaDescriptor::decode(&expected_golden_bytes).unwrap();
    assert_eq!(consumed, expected_golden_bytes.len());
    assert_eq!(decoded, v21_descriptor);
    assert_eq!(decoded.identity(), v21_descriptor.identity());

    let temp_dir = tempfile::tempdir().unwrap();
    let stem = temp_dir.path().join("v21_golden");
    let adapter = FileStorageAdapter::new(&stem);
    let claim = OwnershipClaimRecord {
        epoch: 1,
        machine_id: [1u8; 16],
        boot_id: [2u8; 16],
        process_id: 12345,
        process_start_time_ns: 1_000_000,
        claim_time_ns: 2_000_000,
        operator_label: "test-operator".to_string(),
    };
    let admitted_v21 = AdmittedDescriptor::try_from_descriptor(v21_descriptor.clone()).unwrap();
    let writer = adapter
        .create(&claim, &admitted_v21)
        .expect("create writer");
    assert_eq!(writer.schema_descriptor(), Some(&v21_descriptor));
    drop(writer);

    let reader = adapter.open_read().expect("open reader");
    assert_eq!(reader.schema_descriptor(), Some(&v21_descriptor));
}

fn run_rustc(code: &str) -> (bool, String) {
    use std::io::Write;
    use std::process::Command;

    let deps_dir = std::env::current_exe()
        .expect("current test executable")
        .parent()
        .expect("parent deps directory")
        .to_path_buf();

    let entries = std::fs::read_dir(&deps_dir)
        .unwrap_or_else(|e| panic!("failed to read deps dir {}: {e}", deps_dir.display()));
    let mut rlibs = Vec::new();
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|e| panic!("failed to read entry in {}: {e}", deps_dir.display()));
        let p = entry.path();
        if let Some(s) = p.file_name().and_then(|n| n.to_str()) {
            if s.starts_with("libpardosa-") && s.ends_with(".rlib") {
                rlibs.push(p.clone());
            }
        }
    }
    let rlib = match rlibs.len() {
        0 => panic!(
            "could not locate libpardosa rlib in deps directory: {}",
            deps_dir.display()
        ),
        1 => rlibs.remove(0),
        _ => {
            rlibs.sort_by_key(|p| {
                std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            });
            rlibs.pop().unwrap()
        }
    };

    let rustc_cmd = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let mut cmd = Command::new(&rustc_cmd);
    cmd.arg("--edition")
        .arg("2021")
        .arg("-L")
        .arg(&deps_dir)
        .arg("--extern")
        .arg(format!("pardosa={}", rlib.display()));
    let mut child = cmd
        .arg("--crate-type")
        .arg("lib")
        .arg("--emit")
        .arg("mir=-")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn rustc");

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(code.as_bytes())
            .expect("write code to rustc stdin");
    }
    let output = child.wait_with_output().expect("wait rustc");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (output.status.success(), stderr)
}

#[test]
fn test_event_vec_negative_compile_on_non_pardosa() {
    let code = r#"
        use pardosa::prelude::*;
        struct NonPardosa;
        pub fn test_invalid() {
            let _ = EventVec::<NonPardosa, 10>::new(vec![]);
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(
        !ok,
        "EventVec<NonPardosa, 10> must fail compilation without PardosaType bound"
    );
    assert!(
        stderr.contains("PardosaType")
            || stderr.contains("the trait `PardosaType` is not implemented"),
        "stderr should cite PardosaType trait requirement: {stderr}"
    );
}

#[test]
fn test_event_vec_positive_controls_roundtrip() {
    let empty = EventVec::<u32, 10>::new(vec![]).unwrap();
    let mut buf = Vec::new();
    empty.encode_type(&mut buf).unwrap();
    assert_eq!(buf, vec![0, 0, 0, 0]);
    let (decoded_empty, consumed) = EventVec::<u32, 10>::decode_type(&buf).unwrap();
    assert_eq!(consumed, 4);
    assert_eq!(decoded_empty, empty);

    let populated = EventVec::<u32, 10>::new(vec![10, 20, 30]).unwrap();
    buf.clear();
    populated.encode_type(&mut buf).unwrap();
    assert_eq!(buf.len(), 4 + 3 * 4);
    let (decoded_pop, consumed) = EventVec::<u32, 10>::decode_type(&buf).unwrap();
    assert_eq!(consumed, buf.len());
    assert_eq!(decoded_pop, populated);
}

#[derive(Debug, PartialEq, Eq, Clone, PardosaType)]
struct Address {
    street: EventString<32>,
    zip_code: u32,
}

#[derive(Debug, PartialEq, Eq, Clone, PardosaType)]
struct Marker;

#[derive(Debug, PartialEq, Eq, Clone, Copy, PardosaType)]
#[repr(u8)]
enum AccountState {
    Active = 0,
    Suspended = 1,
    Closed = 2,
}

#[derive(Debug, PartialEq, Eq, Clone, PardosaType)]
#[repr(u8)]
enum TransactionPayload {
    None = 0,
    Transfer(u64) = 1,
    Adjustment { reason: EventString<16>, delta: i64 } = 2,
}

#[test]
fn test_derived_pardosa_type_named_struct_roundtrip() {
    let addr = Address {
        street: EventString::new("Main St").unwrap(),
        zip_code: 12345,
    };

    match Address::descriptor_node() {
        DescriptorNode::Struct { name, fields } => {
            assert_eq!(name, "Address");
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name, "street");
            assert_eq!(
                fields[0].node,
                DescriptorNode::EventString { max_bytes: 32 }
            );
            assert_eq!(fields[1].name, "zip_code");
            assert_eq!(fields[1].node, DescriptorNode::U32);
        }
        other => panic!("expected Struct descriptor node, found {:?}", other),
    }

    let mut buf = Vec::new();
    addr.encode_type(&mut buf).unwrap();

    let (decoded, consumed) = Address::decode_type(&buf).unwrap();
    assert_eq!(consumed, buf.len());
    assert_eq!(decoded, addr);

    buf.push(0xFF);
    let (decoded_prefix, consumed_prefix) = Address::decode_type(&buf).unwrap();
    assert_eq!(consumed_prefix, buf.len() - 1);
    assert_eq!(decoded_prefix, addr);
}

#[test]
fn test_derived_pardosa_type_unit_struct_roundtrip() {
    let marker = Marker;

    match Marker::descriptor_node() {
        DescriptorNode::Struct { name, fields } => {
            assert_eq!(name, "Marker");
            assert_eq!(fields.len(), 0);
        }
        other => panic!("expected Struct descriptor node, found {:?}", other),
    }

    let mut buf = Vec::new();
    marker.encode_type(&mut buf).unwrap();
    assert!(buf.is_empty());

    let (decoded, consumed) = Marker::decode_type(&buf).unwrap();
    assert_eq!(consumed, 0);
    assert_eq!(decoded, marker);
}

#[test]
fn test_derived_pardosa_type_scalar_enum_roundtrip() {
    match AccountState::descriptor_node() {
        DescriptorNode::Enum {
            name,
            discriminant_width,
            variants,
        } => {
            assert_eq!(name, "AccountState");
            assert_eq!(discriminant_width, 1);
            assert_eq!(variants.len(), 3);
            assert_eq!(variants[0].name, "Active");
            assert_eq!(variants[0].discriminant, 0);
            assert_eq!(variants[0].payload, None);
            assert_eq!(variants[1].name, "Suspended");
            assert_eq!(variants[1].discriminant, 1);
            assert_eq!(variants[2].name, "Closed");
            assert_eq!(variants[2].discriminant, 2);
        }
        other => panic!("expected Enum descriptor node, found {:?}", other),
    }

    for state in [
        AccountState::Active,
        AccountState::Suspended,
        AccountState::Closed,
    ] {
        let mut buf = Vec::new();
        state.encode_type(&mut buf).unwrap();
        assert_eq!(buf.len(), 1);

        let (decoded, consumed) = AccountState::decode_type(&buf).unwrap();
        assert_eq!(consumed, 1);
        assert_eq!(decoded, state);
    }

    let err = AccountState::decode_type(&[99]).unwrap_err();
    assert!(matches!(
        err,
        DecodeError::UnknownVariantDiscriminant { discriminant: 99 }
    ));
}

#[test]
fn test_derived_pardosa_type_composite_enum_roundtrip() {
    match TransactionPayload::descriptor_node() {
        DescriptorNode::Enum {
            name,
            discriminant_width,
            variants,
        } => {
            assert_eq!(name, "TransactionPayload");
            assert_eq!(discriminant_width, 1);
            assert_eq!(variants.len(), 3);
            assert_eq!(variants[0].name, "None");
            assert_eq!(variants[0].payload, None);
            assert_eq!(variants[1].name, "Transfer");
            assert_eq!(variants[1].payload, Some(DescriptorNode::U64));
            assert_eq!(variants[2].name, "Adjustment");
            assert!(matches!(
                variants[2].payload,
                Some(DescriptorNode::Struct { .. })
            ));
        }
        other => panic!("expected Enum descriptor node, found {:?}", other),
    }

    let cases = vec![
        TransactionPayload::None,
        TransactionPayload::Transfer(1_000_000),
        TransactionPayload::Adjustment {
            reason: EventString::new("Refund").unwrap(),
            delta: -500,
        },
    ];

    for case in cases {
        let mut buf = Vec::new();
        case.encode_type(&mut buf).unwrap();

        let (decoded, consumed) = TransactionPayload::decode_type(&buf).unwrap();
        assert_eq!(consumed, buf.len());
        assert_eq!(decoded, case);
    }
}

#[test]
fn test_compile_fail_missing_schema_version_attribute() {
    let code = r#"
        use pardosa::prelude::*;

        #[derive(Debug, PartialEq, Eq, PardosaSchema)]
        #[repr(u8)]
        enum MissingVersionEvent {
            #[pardosa(tombstone)]
            Tombstone = 0,
            Action = 1,
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(
        !ok,
        "omitting #[pardosa(version = N)] must fail compilation"
    );
    assert!(
        stderr.contains(
            "missing mandatory `#[pardosa(version = N)]` attribute on PardosaSchema root"
        ),
        "stderr must cite missing mandatory version attribute:\n{stderr}"
    );
}

#[test]
fn test_compile_fail_override_schema_version() {
    let code = r#"
        use pardosa::prelude::*;

        #[derive(Debug, PartialEq, Eq)]
        enum ManualSchemaEvent {
            Tombstone,
        }

        impl PardosaSchema for ManualSchemaEvent {
            const SCHEMA_VERSION: u32 = 1;
            fn schema_version() -> u32 { 1 }
            fn schema_descriptor() -> DescriptorNode { DescriptorNode::U8 }
            fn encode_payload(&self, _buf: &mut Vec<u8>) -> Result<(), EncodeError> { Ok(()) }
            fn decode_payload(_buf: &[u8]) -> Result<Self, DecodeError> { Ok(Self::Tombstone) }
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(
        !ok,
        "overriding schema_version in PardosaSchema impl must fail compilation"
    );
    assert!(
        stderr.contains("E0407") || stderr.contains("is not a member of trait `PardosaSchema`"),
        "stderr must cite E0407:\n{stderr}"
    );
}

#[test]
fn test_m1_hygiene_reserved_field_names() {
    #[derive(Debug, PartialEq, Eq, Clone, PardosaType)]
    struct ReservedFieldsStruct {
        consumed: u32,
        cursor: u32,
        buf: u32,
        value: u32,
    }

    #[derive(Debug, PartialEq, Eq, Clone, PardosaType)]
    #[repr(u8)]
    enum ReservedFieldsEnum {
        Data {
            consumed: u32,
            cursor: u32,
            buf: u32,
            value: u32,
        } = 0,
    }

    #[derive(Debug, PartialEq, Eq, Clone, PardosaSchema)]
    #[repr(u8)]
    #[pardosa(version = 1)]
    enum ReservedFieldsSchema {
        #[pardosa(tombstone)]
        Tombstone = 0,
        Data {
            consumed: u32,
            cursor: u32,
            buf: u32,
            value: u32,
        } = 1,
    }

    let s = ReservedFieldsStruct {
        consumed: 1,
        cursor: 2,
        buf: 3,
        value: 4,
    };
    let mut s_buf = Vec::new();
    s.encode_type(&mut s_buf).unwrap();
    let (s_decoded, s_consumed) = ReservedFieldsStruct::decode_type(&s_buf).unwrap();
    assert_eq!(s_consumed, s_buf.len());
    assert_eq!(s_decoded, s);

    let e = ReservedFieldsEnum::Data {
        consumed: 10,
        cursor: 20,
        buf: 30,
        value: 40,
    };
    let mut e_buf = Vec::new();
    e.encode_type(&mut e_buf).unwrap();
    let (e_decoded, e_consumed) = ReservedFieldsEnum::decode_type(&e_buf).unwrap();
    assert_eq!(e_consumed, e_buf.len());
    assert_eq!(e_decoded, e);

    let schema_event = ReservedFieldsSchema::Data {
        consumed: 100,
        cursor: 200,
        buf: 300,
        value: 400,
    };
    let mut schema_buf = Vec::new();
    schema_event.encode_payload(&mut schema_buf).unwrap();
    let schema_decoded = ReservedFieldsSchema::decode_payload(&schema_buf).unwrap();
    assert_eq!(schema_decoded, schema_event);
}

#[test]
fn test_m2_macro_qualification_shadowing_vec() {
    #[allow(unused_macros)]
    macro_rules! vec {
        ($($t:tt)*) => {
            ::std::vec::Vec::new()
        };
    }

    #[derive(Debug, PartialEq, Eq, PardosaType)]
    struct ShadowedStruct {
        value: u32,
    }

    #[derive(Debug, PartialEq, Eq, PardosaType)]
    #[repr(u8)]
    enum ShadowedEnum {
        Variant { code: u16 } = 0,
    }

    #[derive(Debug, PartialEq, Eq, PardosaSchema)]
    #[repr(u8)]
    #[pardosa(version = 1)]
    enum ShadowedSchema {
        #[pardosa(tombstone)]
        Tombstone = 0,
        Event {
            tag: u8,
        } = 1,
    }

    let node = ShadowedStruct::descriptor_node();
    match node {
        DescriptorNode::Struct { fields, .. } => {
            assert_eq!(
                fields.len(),
                1,
                "fields must be populated despite local vec! macro"
            );
            assert_eq!(fields[0].name, "value");
        }
        other => panic!("expected Struct descriptor node, got {other:?}"),
    }

    let enum_node = ShadowedEnum::descriptor_node();
    match enum_node {
        DescriptorNode::Enum { variants, .. } => {
            assert_eq!(
                variants.len(),
                1,
                "variants must be populated despite local vec! macro"
            );
        }
        other => panic!("expected Enum descriptor node, got {other:?}"),
    }

    let schema_node = ShadowedSchema::schema_descriptor();
    match schema_node {
        DescriptorNode::Enum { variants, .. } => {
            assert_eq!(
                variants.len(),
                2,
                "schema variants must be populated despite local vec! macro"
            );
        }
        other => panic!("expected Enum descriptor node, got {other:?}"),
    }
}

#[test]
fn test_m3_checked_cursor_advancement_composition() {
    #[derive(Debug, PartialEq, Eq, Clone)]
    struct BadConsumedType;

    impl PardosaType for BadConsumedType {
        const TYPE_DEPTH: usize = 0;
        fn descriptor_node() -> DescriptorNode {
            DescriptorNode::U8
        }
        fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
            buf.push(0);
            Ok(())
        }
        fn decode_type(_buf: &[u8]) -> Result<(Self, usize), DecodeError> {
            Ok((Self, usize::MAX))
        }
    }

    #[derive(Debug, PartialEq, Eq, Clone, PardosaType)]
    struct ComposedStruct {
        bad: BadConsumedType,
        after: u8,
    }

    let err = ComposedStruct::decode_type(&[0u8; 10]).unwrap_err();
    assert_eq!(
        err,
        DecodeError::TruncatedPayload {
            expected: usize::MAX,
            available: 10,
        }
    );
}

#[test]
fn test_h1_type_depth_const_values() {
    assert_eq!(u32::TYPE_DEPTH, 0);
    assert_eq!(Option::<u32>::TYPE_DEPTH, 1);
    assert_eq!(EventVec::<u32, 10>::TYPE_DEPTH, 1);
    assert_eq!(Option::<Option<u32>>::TYPE_DEPTH, 2);
    assert_eq!(EventVec::<Option<u32>, 10>::TYPE_DEPTH, 2);

    #[derive(PardosaType)]
    struct LeafStruct {
        a: u32,
    }
    assert_eq!(LeafStruct::TYPE_DEPTH, 1);

    #[derive(PardosaType)]
    struct NestedStruct {
        inner: LeafStruct,
    }
    assert_eq!(NestedStruct::TYPE_DEPTH, 2);
}

#[test]
fn test_compile_fail_h1_mutual_recursion() {
    let code = r#"
        use pardosa::prelude::*;

        #[derive(PardosaType)]
        struct A {
            b: EventVec<B, 1>,
        }

        #[derive(PardosaType)]
        struct B {
            a: EventVec<A, 1>,
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(!ok, "mutual recursion cycle must fail compilation");
    assert!(
        stderr.contains("cycle detected") || stderr.contains("recursion limit"),
        "stderr must cite cycle or recursion limit:\n{stderr}"
    );
}

#[test]
fn test_compile_fail_h1_type_alias_recursion() {
    let code = r#"
        use pardosa::prelude::*;

        type NodeAlias = Node;

        #[derive(PardosaType)]
        struct Node {
            next: EventVec<NodeAlias, 1>,
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(!ok, "type alias cycle must fail compilation");
    assert!(
        stderr.contains("cycle detected") || stderr.contains("recursion limit"),
        "stderr must cite cycle or recursion limit:\n{stderr}"
    );
}

#[test]
fn test_compile_fail_version_zero() {
    let code = r#"
        use pardosa::prelude::*;

        #[derive(Debug, PartialEq, Eq, PardosaSchema)]
        #[repr(u8)]
        #[pardosa(version = 0)]
        enum ZeroVersionEvent {
            #[pardosa(tombstone)]
            Tombstone = 0,
            Action = 1,
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(!ok, "version = 0 must fail compilation");
    assert!(
        stderr.contains("schema version must be non-zero"),
        "stderr must cite non-zero version:\n{stderr}"
    );
}

#[test]
fn test_compile_fail_duplicate_version() {
    let code = r#"
        use pardosa::prelude::*;

        #[derive(Debug, PartialEq, Eq, PardosaSchema)]
        #[repr(u8)]
        #[pardosa(version = 1, version = 2)]
        enum DuplicateVersionEvent {
            #[pardosa(tombstone)]
            Tombstone = 0,
            Action = 1,
        }
    "#;
    let (ok, stderr) = run_rustc(code);
    assert!(!ok, "duplicate version must fail compilation");
    assert!(
        stderr.contains("duplicate `version` attribute"),
        "stderr must cite duplicate version attribute:\n{stderr}"
    );
}
