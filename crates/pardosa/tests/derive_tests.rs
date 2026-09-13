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

    assert_eq!(OrderEvent::schema_version(), 1);
    let identity = OrderEvent::schema_identity();
    assert_ne!(identity.as_bytes(), &[0u8; 32]);
}

#[test]
fn test_user_event_schema_version_and_identity() {
    assert_eq!(UserEvent::schema_version(), 1);
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
        21, 0, 0, 0,
        16,
        13, 0, 0, 0, 69, 118, 105, 100, 101, 110, 99, 101, 69, 118, 101, 110, 116,
        1,
        2, 0, 0, 0,
        0,
        9, 0, 0, 0, 84, 111, 109, 98, 115, 116, 111, 110, 101,
        0,
        1,
        8, 0, 0, 0, 79, 98, 115, 101, 114, 118, 101, 100,
        15,
        15, 0, 0, 0, 69, 118, 105, 100, 101, 110, 99, 101, 80, 97, 121, 108, 111, 97, 100,
        4, 0, 0, 0,
        9, 0, 0, 0, 101, 110, 116, 105, 116, 121, 95, 105, 100, 18,
        9, 0, 0, 0, 116, 105, 109, 101, 115, 116, 97, 109, 112, 17,
        5, 0, 0, 0, 108, 97, 98, 101, 108, 10, 64, 0, 0, 0,
        5, 0, 0, 0, 99, 111, 117, 110, 116, 4,
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
    let mut writer = adapter.create(&claim).expect("create writer");
    writer
        .set_schema_descriptor(&v21_descriptor)
        .expect("initial v21 schema descriptor set");

    writer
        .set_schema_descriptor(&v21_descriptor)
        .expect("idempotent re-submission of identical v21 descriptor");

    let conflicting_v21_descriptor = SchemaDescriptor::new(
        21,
        DescriptorNode::Enum {
            name: "EvidenceEvent".to_string(),
            discriminant_width: 1,
            variants: vec![VariantDescriptor {
                discriminant: 0,
                name: "Tombstone".to_string(),
                payload: None,
            }],
        },
    );
    let err_conflict = writer
        .set_schema_descriptor(&conflicting_v21_descriptor)
        .unwrap_err();
    assert_eq!(*err_conflict.condition(), FailureCondition::SchemaMismatch);

    let conflicting_version_descriptor = SchemaDescriptor::new(
        22,
        v21_descriptor.root.clone(),
    );
    let err_version = writer
        .set_schema_descriptor(&conflicting_version_descriptor)
        .unwrap_err();
    assert_eq!(*err_version.condition(), FailureCondition::SchemaMismatch);
}
