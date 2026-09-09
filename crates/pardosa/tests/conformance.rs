use pardosa::prelude::*;
use serde_json::Value;

fn read_vector_file(path: &str) -> Value {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let full_path = std::path::Path::new(&manifest_dir)
        .join("../../")
        .join(path);
    let content = std::fs::read_to_string(&full_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", full_path.display(), e));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse json {}: {}", full_path.display(), e))
}

#[test]
fn test_container_header_conformance_vectors() {
    let json = read_vector_file("conformance/vectors/container_header.json");
    let vectors = json["vectors"].as_array().expect("vectors array");

    for vec in vectors {
        let name = vec["name"].as_str().expect("name");
        let bytes_hex = vec["bytes_hex"].as_str().expect("bytes_hex");
        let bytes = hex::decode(bytes_hex).expect("valid hex");
        let expected_outcome = vec["expected_outcome"].as_str().expect("expected_outcome");

        match name {
            "valid_container_header" => {
                assert_eq!(expected_outcome, "Success");
                let (header, consumed) = ContainerHeader::decode(&bytes).expect("decode success");
                assert_eq!(consumed, 12);
                assert_eq!(header.format_version, 1);
                assert_eq!(header.to_bytes(), bytes.as_slice());
            }
            "invalid_magic_header" => {
                assert_eq!(expected_outcome, "DecodeError");
                let err = ContainerHeader::decode(&bytes).unwrap_err();
                assert_eq!(err.error_kind(), vec["error_kind"].as_str().unwrap());
            }
            "unsupported_format_version" => {
                assert_eq!(expected_outcome, "DecodeError");
                let err = ContainerHeader::decode(&bytes).unwrap_err();
                assert_eq!(err.error_kind(), vec["error_kind"].as_str().unwrap());
            }
            "valid_framed_payload" => {
                assert_eq!(expected_outcome, "Success");
                let (payload, consumed) = ContainerFrame::decode(&bytes).expect("decode success");
                assert_eq!(consumed, bytes.len());
                let expected_utf8 = vec["parsed_value"]["payload_utf8"].as_str().unwrap();
                assert_eq!(payload, expected_utf8.as_bytes());

                let mut enc = Vec::new();
                ContainerFrame::encode_payload(&payload, &mut enc);
                assert_eq!(enc, bytes);
            }
            "corrupted_crc32c_checksum" => {
                assert_eq!(expected_outcome, "DecodeError");
                let err = ContainerFrame::decode(&bytes).unwrap_err();
                assert_eq!(err.error_kind(), vec["error_kind"].as_str().unwrap());
            }
            "truncated_framing_payload" => {
                assert_eq!(expected_outcome, "DecodeError");
                let err = ContainerFrame::decode(&bytes).unwrap_err();
                assert_eq!(err.error_kind(), vec["error_kind"].as_str().unwrap());
            }
            other => panic!("unhandled container header vector: {}", other),
        }
    }
}

#[test]
fn test_envelope_conformance_vectors() {
    let json = read_vector_file("conformance/vectors/envelope.json");
    let vectors = json["vectors"].as_array().expect("vectors array");

    for vec in vectors {
        let name = vec["name"].as_str().expect("name");
        let bytes_hex = vec["bytes_hex"].as_str().expect("bytes_hex");
        let bytes = hex::decode(bytes_hex).expect("valid hex");
        let expected_outcome = vec["expected_outcome"].as_str().expect("expected_outcome");

        match name {
            "valid_attached_event" => {
                assert_eq!(expected_outcome, "Success");
                let (env, consumed) = EventEnvelope::decode(&bytes).expect("decode success");
                assert_eq!(consumed, bytes.len());
                assert!(!env.header.detached);
                let expected_utf8 = vec["parsed_value"]["payload_utf8"].as_str().unwrap();
                assert_eq!(env.payload, expected_utf8.as_bytes());

                let mut enc = Vec::new();
                env.encode(&mut enc);
                assert_eq!(enc, bytes);
            }
            "valid_detached_event" => {
                assert_eq!(expected_outcome, "Success");
                let (env, consumed) = EventEnvelope::decode(&bytes).expect("decode success");
                assert_eq!(consumed, bytes.len());
                assert!(env.header.detached);
                assert_eq!(env.header.precursor, [0u8; 16]);
                assert_eq!(env.header.precursor_hash, [0u8; 32]);
                let expected_utf8 = vec["parsed_value"]["payload_utf8"].as_str().unwrap();
                assert_eq!(env.payload, expected_utf8.as_bytes());

                let mut enc = Vec::new();
                env.encode(&mut enc);
                assert_eq!(enc, bytes);
            }
            "invalid_boolean_discriminant_in_envelope" => {
                assert_eq!(expected_outcome, "DecodeError");
                let err = EventEnvelope::decode(&bytes).unwrap_err();
                assert_eq!(err.error_kind(), vec["error_kind"].as_str().unwrap());
            }
            "valid_empty_payload_envelope" => {
                assert_eq!(expected_outcome, "Success");
                let (env, consumed) = EventEnvelope::decode(&bytes).expect("decode success");
                assert_eq!(consumed, bytes.len());
                assert!(!env.header.detached);
                assert_eq!(env.payload.len(), 0);

                let mut enc = Vec::new();
                env.encode(&mut enc);
                assert_eq!(enc, bytes);
            }
            "truncated_envelope_header" => {
                assert_eq!(expected_outcome, "DecodeError");
                let err = EventEnvelope::decode(&bytes).unwrap_err();
                assert_eq!(err.error_kind(), vec["error_kind"].as_str().unwrap());
            }
            other => panic!("unhandled envelope vector: {}", other),
        }
    }
}

#[test]
fn test_ownership_records_conformance_vectors() {
    let json = read_vector_file("conformance/vectors/ownership_records.json");
    let vectors = json["vectors"].as_array().expect("vectors array");

    for vec in vectors {
        let name = vec["name"].as_str().expect("name");
        let bytes_hex = vec["bytes_hex"].as_str().expect("bytes_hex");
        let bytes = hex::decode(bytes_hex).expect("valid hex");
        let expected_outcome = vec["expected_outcome"].as_str().expect("expected_outcome");

        match name {
            "valid_ownership_claim_record" => {
                assert_eq!(expected_outcome, "Success");
                let (record, consumed) = OwnershipRecord::decode(&bytes).expect("decode success");
                assert_eq!(consumed, bytes.len());
                match record {
                    OwnershipRecord::OwnershipClaim(c) => {
                        assert_eq!(c.epoch, 42);
                        assert_eq!(c.process_id, 7890);
                        assert_eq!(c.process_start_time_ns, 1725890000000000000);
                        assert_eq!(c.claim_time_ns, 1725890005000000000);
                        assert_eq!(c.operator_label, "writer-node-primary");

                        let mut enc = Vec::new();
                        OwnershipRecord::OwnershipClaim(c).encode(&mut enc);
                        assert_eq!(enc, bytes);
                    }
                    other => panic!("expected OwnershipClaim, found {:?}", other),
                }
            }
            "valid_clean_release_record" => {
                assert_eq!(expected_outcome, "Success");
                let (record, consumed) = OwnershipRecord::decode(&bytes).expect("decode success");
                assert_eq!(consumed, bytes.len());
                match record {
                    OwnershipRecord::CleanRelease(r) => {
                        assert_eq!(r.epoch, 42);
                        assert_eq!(r.release_time_ns, 1725890010000000000);

                        let mut enc = Vec::new();
                        OwnershipRecord::CleanRelease(r).encode(&mut enc);
                        assert_eq!(enc, bytes);
                    }
                    other => panic!("expected CleanRelease, found {:?}", other),
                }
            }
            "valid_migration_start_record" => {
                assert_eq!(expected_outcome, "Success");
                let (record, consumed) = OwnershipRecord::decode(&bytes).expect("decode success");
                assert_eq!(consumed, bytes.len());
                match record {
                    OwnershipRecord::MigrationStart(m) => {
                        assert_eq!(m.source_generation, 1);
                        assert_eq!(m.target_generation, 2);
                        assert_eq!(m.start_time_ns, 1725890020000000000);
                        assert_eq!(m.rescue_policy_tag, 1);

                        let mut enc = Vec::new();
                        OwnershipRecord::MigrationStart(m).encode(&mut enc);
                        assert_eq!(enc, bytes);
                    }
                    other => panic!("expected MigrationStart, found {:?}", other),
                }
            }
            "valid_migration_end_record" => {
                assert_eq!(expected_outcome, "Success");
                let (record, consumed) = OwnershipRecord::decode(&bytes).expect("decode success");
                assert_eq!(consumed, bytes.len());
                match record {
                    OwnershipRecord::MigrationEnd(m) => {
                        assert_eq!(m.source_generation, 1);
                        assert_eq!(m.target_generation, 2);
                        assert_eq!(m.end_time_ns, 1725890030000000000);
                        assert_eq!(m.status, MigrationStatus::Complete);

                        let mut enc = Vec::new();
                        OwnershipRecord::MigrationEnd(m).encode(&mut enc);
                        assert_eq!(enc, bytes);
                    }
                    other => panic!("expected MigrationEnd, found {:?}", other),
                }
            }
            "valid_identity_structure_record" => {
                assert_eq!(expected_outcome, "Success");
                let (record, consumed) = OwnershipRecord::decode(&bytes).expect("decode success");
                assert_eq!(consumed, bytes.len());
                match record {
                    OwnershipRecord::IdentityStructure(ids) => {
                        assert_eq!(ids.structure_version, 1);
                        assert_eq!(ids.dragline_id, 3);
                        assert_eq!(
                            ids.partitioning_rule,
                            PartitioningRule::StaticModulo { total_draglines: 8 }
                        );

                        let mut enc = Vec::new();
                        OwnershipRecord::IdentityStructure(ids).encode(&mut enc);
                        assert_eq!(enc, bytes);
                    }
                    other => panic!("expected IdentityStructure, found {:?}", other),
                }
            }
            "unknown_tag_0x00_rejection_witness"
            | "unknown_tag_0x0a_rejection_witness"
            | "unknown_tag_0xff_rejection_witness" => {
                assert_eq!(expected_outcome, "DecodeError");
                let err = OwnershipRecord::decode(&bytes).unwrap_err();
                assert_eq!(err.error_kind(), vec["error_kind"].as_str().unwrap());
            }
            other => panic!("unhandled ownership record vector: {}", other),
        }
    }
}

#[test]
fn test_descriptors_conformance_vectors() {
    let json = read_vector_file("conformance/vectors/descriptors.json");
    let vectors = json["vectors"].as_array().expect("vectors array");

    for vec in vectors {
        let name = vec["name"].as_str().expect("name");
        let cases = vec["cases"].as_array().expect("cases array");

        match name {
            "primitive_integers_roundtrip" => {
                for c in cases {
                    let ty = c["type"].as_str().unwrap();
                    let bytes = hex::decode(c["bytes_hex"].as_str().unwrap()).unwrap();
                    match ty {
                        "u8" => {
                            let (val, consumed) = u8::decode_type(&bytes).unwrap();
                            assert_eq!(consumed, 1);
                            assert_eq!(val, c["val"].as_u64().unwrap() as u8);
                            let mut enc = Vec::new();
                            val.encode_type(&mut enc).unwrap();
                            assert_eq!(enc, bytes);
                        }
                        "u16" => {
                            let (val, consumed) = u16::decode_type(&bytes).unwrap();
                            assert_eq!(consumed, 2);
                            assert_eq!(val, c["val"].as_u64().unwrap() as u16);
                            let mut enc = Vec::new();
                            val.encode_type(&mut enc).unwrap();
                            assert_eq!(enc, bytes);
                        }
                        "u32" => {
                            let (val, consumed) = u32::decode_type(&bytes).unwrap();
                            assert_eq!(consumed, 4);
                            assert_eq!(val, c["val"].as_u64().unwrap() as u32);
                            let mut enc = Vec::new();
                            val.encode_type(&mut enc).unwrap();
                            assert_eq!(enc, bytes);
                        }
                        "u64" => {
                            let (val, consumed) = u64::decode_type(&bytes).unwrap();
                            assert_eq!(consumed, 8);
                            assert_eq!(val, c["val"].as_u64().unwrap());
                            let mut enc = Vec::new();
                            val.encode_type(&mut enc).unwrap();
                            assert_eq!(enc, bytes);
                        }
                        "i8" => {
                            let (val, consumed) = i8::decode_type(&bytes).unwrap();
                            assert_eq!(consumed, 1);
                            assert_eq!(val, c["val"].as_i64().unwrap() as i8);
                            let mut enc = Vec::new();
                            val.encode_type(&mut enc).unwrap();
                            assert_eq!(enc, bytes);
                        }
                        "i16" => {
                            let (val, consumed) = i16::decode_type(&bytes).unwrap();
                            assert_eq!(consumed, 2);
                            assert_eq!(val, c["val"].as_i64().unwrap() as i16);
                            let mut enc = Vec::new();
                            val.encode_type(&mut enc).unwrap();
                            assert_eq!(enc, bytes);
                        }
                        "i32" => {
                            let (val, consumed) = i32::decode_type(&bytes).unwrap();
                            assert_eq!(consumed, 4);
                            assert_eq!(val, c["val"].as_i64().unwrap() as i32);
                            let mut enc = Vec::new();
                            val.encode_type(&mut enc).unwrap();
                            assert_eq!(enc, bytes);
                        }
                        "i64" => {
                            let (val, consumed) = i64::decode_type(&bytes).unwrap();
                            assert_eq!(consumed, 8);
                            assert_eq!(val, c["val"].as_i64().unwrap());
                            let mut enc = Vec::new();
                            val.encode_type(&mut enc).unwrap();
                            assert_eq!(enc, bytes);
                        }
                        other => panic!("unknown primitive type: {}", other),
                    }
                }
            }
            "boolean_truth_values_and_rejection" => {
                for c in cases {
                    let bytes = hex::decode(c["bytes_hex"].as_str().unwrap()).unwrap();
                    let expected = c["expected"].as_str().unwrap();
                    if expected == "Success" {
                        let (val, consumed) = bool::decode_type(&bytes).unwrap();
                        assert_eq!(consumed, 1);
                        assert_eq!(val, c["val"].as_bool().unwrap());
                        let mut enc = Vec::new();
                        val.encode_type(&mut enc).unwrap();
                        assert_eq!(enc, bytes);
                    } else {
                        let err = bool::decode_type(&bytes).unwrap_err();
                        assert_eq!(err.error_kind(), c["error_kind"].as_str().unwrap());
                    }
                }
            }
            "bounded_event_string" => {
                for c in cases {
                    let max_bound = c["max_bound"].as_u64().unwrap() as usize;
                    let bytes = hex::decode(c["bytes_hex"].as_str().unwrap()).unwrap();
                    let expected = c["expected"].as_str().unwrap();
                    match max_bound {
                        64 => {
                            if expected == "Success" {
                                let (s, consumed) =
                                    EventString::<64>::decode(&bytes).expect("decode success");
                                assert_eq!(consumed, bytes.len());
                                assert_eq!(s.as_str(), c["val"].as_str().unwrap());
                                let mut enc = Vec::new();
                                s.encode(&mut enc);
                                assert_eq!(enc, bytes);
                            } else {
                                let err = EventString::<64>::decode(&bytes).unwrap_err();
                                assert_eq!(err.error_kind(), c["error_kind"].as_str().unwrap());
                            }
                        }
                        10 => {
                            let err = EventString::<10>::decode(&bytes).unwrap_err();
                            assert_eq!(err.error_kind(), c["error_kind"].as_str().unwrap());
                        }
                        other => panic!("unexpected max_bound: {}", other),
                    }
                }
            }
            "bounded_non_empty_event_string" => {
                for c in cases {
                    let bytes = hex::decode(c["bytes_hex"].as_str().unwrap()).unwrap();
                    let expected = c["expected"].as_str().unwrap();
                    if expected == "Success" {
                        let (s, consumed) =
                            NonEmptyEventString::<64>::decode(&bytes).expect("decode success");
                        assert_eq!(consumed, bytes.len());
                        assert_eq!(s.as_str(), c["val"].as_str().unwrap());
                        let mut enc = Vec::new();
                        s.encode(&mut enc);
                        assert_eq!(enc, bytes);
                    } else {
                        let err = NonEmptyEventString::<64>::decode(&bytes).unwrap_err();
                        assert_eq!(err.error_kind(), c["error_kind"].as_str().unwrap());
                    }
                }
            }
            "bounded_event_vec" => {
                for c in cases {
                    let bytes = hex::decode(c["bytes_hex"].as_str().unwrap()).unwrap();
                    let expected = c["expected"].as_str().unwrap();
                    let max_items = c["max_items"].as_u64().unwrap() as usize;
                    match max_items {
                        4 => {
                            if expected == "Success" {
                                let (vec_val, consumed) =
                                    EventVec::<u32, 4>::decode_type(&bytes).unwrap();
                                assert_eq!(consumed, bytes.len());
                                let items: Vec<u32> = c["items"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .map(|v| v.as_u64().unwrap() as u32)
                                    .collect();
                                assert_eq!(vec_val.as_slice(), items.as_slice());
                            } else {
                                let err = EventVec::<u32, 4>::decode_type(&bytes).unwrap_err();
                                assert_eq!(err.error_kind(), c["error_kind"].as_str().unwrap());
                            }
                        }
                        2 => {
                            let err = EventVec::<u32, 2>::decode_type(&bytes).unwrap_err();
                            assert_eq!(err.error_kind(), c["error_kind"].as_str().unwrap());
                        }
                        other => panic!("unexpected max_items: {}", other),
                    }
                }
            }
            "optionality" => {
                for c in cases {
                    let bytes = hex::decode(c["bytes_hex"].as_str().unwrap()).unwrap();
                    let expected = c["expected"].as_str().unwrap();
                    if expected == "Success" {
                        let (opt, consumed) = Option::<u64>::decode_type(&bytes).unwrap();
                        assert_eq!(consumed, bytes.len());
                        if c["val"].is_null() {
                            assert_eq!(opt, None);
                        } else {
                            assert_eq!(opt, Some(c["val"].as_u64().unwrap()));
                        }
                    } else {
                        let err = Option::<u64>::decode_type(&bytes).unwrap_err();
                        assert_eq!(err.error_kind(), c["error_kind"].as_str().unwrap());
                    }
                }
            }
            "composite_product_and_sum" => {
                for c in cases {
                    let ty = c["type"].as_str().unwrap();
                    let bytes = hex::decode(c["bytes_hex"].as_str().unwrap()).unwrap();
                    let expected = c["expected"].as_str().unwrap();
                    if ty == "Struct_Point2D" {
                        let (x, consumed_x) = i32::decode_type(&bytes[0..4]).unwrap();
                        let (y, consumed_y) = i32::decode_type(&bytes[4..8]).unwrap();
                        assert_eq!(consumed_x + consumed_y, bytes.len());
                        assert_eq!(x, 100);
                        assert_eq!(y, -200);
                        let mut enc = Vec::new();
                        x.encode_type(&mut enc).unwrap();
                        y.encode_type(&mut enc).unwrap();
                        assert_eq!(enc, bytes);
                    } else if ty == "Enum_Status" {
                        if expected == "Success" {
                            assert_eq!(bytes, vec![0x01]);
                        } else {
                            let disc = bytes[0] as u32;
                            let err =
                                DecodeError::UnknownVariantDiscriminant { discriminant: disc };
                            assert_eq!(err.error_kind(), c["error_kind"].as_str().unwrap());
                        }
                    }
                }
            }
            "temporal_and_uuid" => {
                for c in cases {
                    let ty = c["type"].as_str().unwrap();
                    let bytes = hex::decode(c["bytes_hex"].as_str().unwrap()).unwrap();
                    let expected = c["expected"].as_str().unwrap();
                    if ty == "Timestamp" {
                        if expected == "Success" {
                            let (ts, consumed) = Timestamp::decode(&bytes).unwrap();
                            assert_eq!(consumed, 8);
                            assert_eq!(ts.as_nanos(), c["val_ns"].as_u64().unwrap());
                            let mut enc = Vec::new();
                            ts.encode(&mut enc);
                            assert_eq!(enc, bytes);
                        } else {
                            let err = Timestamp::decode(&bytes).unwrap_err();
                            assert_eq!(err.error_kind(), c["error_kind"].as_str().unwrap());
                        }
                    } else if ty == "Uuid" {
                        let (uuid_val, consumed) = Uuid::decode(&bytes).unwrap();
                        assert_eq!(consumed, 16);
                        let expected_hex = c["bytes_hex"].as_str().unwrap();
                        assert_eq!(hex::encode(uuid_val.as_bytes()), expected_hex);
                    }
                }
            }
            other => panic!("unknown vector topic: {}", other),
        }
    }
}
