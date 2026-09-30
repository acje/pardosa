use pardosa::encoding::{
    CleanReleaseRecord, EnvelopeHeader, EventEnvelope, IdentityStructureRecord,
    InboundPointerRecord, MigrationEndRecord, MigrationStartRecord, MigrationStatus,
    OutboundPointerRecord, OwnershipClaimRecord, OwnershipRecord, PartitioningRule,
    RescuePolicyChoiceRecord,
};
use pardosa::file::{ContainerFrame, ContainerHeader};
use proptest::prelude::*;

fn arb_container_header() -> impl Strategy<Value = ContainerHeader> {
    Just(ContainerHeader::new())
}

fn arb_container_frame() -> impl Strategy<Value = ContainerFrame> {
    prop::collection::vec(any::<u8>(), 0..=2048).prop_map(ContainerFrame::new)
}

fn arb_event_envelope() -> impl Strategy<Value = EventEnvelope> {
    (
        any::<[u8; 16]>(),
        any::<[u8; 16]>(),
        any::<bool>(),
        any::<[u8; 16]>(),
        any::<[u8; 32]>(),
        prop::collection::vec(any::<u8>(), 0..=2048),
    )
        .prop_map(
            |(event_id, fiber_id, detached, precursor, precursor_hash, payload)| EventEnvelope {
                header: EnvelopeHeader {
                    event_id,
                    fiber_id,
                    detached,
                    precursor,
                    precursor_hash,
                },
                payload,
            },
        )
}

fn arb_claim() -> impl Strategy<Value = OwnershipRecord> {
    (
        any::<u64>(),
        any::<[u8; 16]>(),
        any::<[u8; 16]>(),
        any::<u64>(),
        any::<u64>(),
        any::<u64>(),
        prop::string::string_regex("[a-zA-Z0-9_ -]{0,64}").expect("valid regex"),
    )
        .prop_map(
            |(
                epoch,
                machine_id,
                boot_id,
                process_id,
                process_start_time_ns,
                claim_time_ns,
                operator_label,
            )| {
                OwnershipRecord::OwnershipClaim(OwnershipClaimRecord {
                    epoch,
                    machine_id,
                    boot_id,
                    process_id,
                    process_start_time_ns,
                    claim_time_ns,
                    operator_label,
                })
            },
        )
}

fn arb_clean_release() -> impl Strategy<Value = OwnershipRecord> {
    (any::<u64>(), any::<u64>()).prop_map(|(epoch, release_time_ns)| {
        OwnershipRecord::CleanRelease(CleanReleaseRecord {
            epoch,
            release_time_ns,
        })
    })
}

fn arb_migration_start() -> impl Strategy<Value = OwnershipRecord> {
    (any::<u32>(), any::<u32>(), any::<u64>(), any::<u8>()).prop_map(
        |(source_generation, target_generation, start_time_ns, rescue_policy_tag)| {
            OwnershipRecord::MigrationStart(MigrationStartRecord {
                source_generation,
                target_generation,
                start_time_ns,
                rescue_policy_tag,
            })
        },
    )
}

fn arb_migration_end() -> impl Strategy<Value = OwnershipRecord> {
    (
        any::<u32>(),
        any::<u32>(),
        any::<u64>(),
        prop_oneof![
            Just(MigrationStatus::Complete),
            Just(MigrationStatus::Interrupted),
        ],
    )
        .prop_map(
            |(source_generation, target_generation, end_time_ns, status)| {
                OwnershipRecord::MigrationEnd(MigrationEndRecord {
                    source_generation,
                    target_generation,
                    end_time_ns,
                    status,
                })
            },
        )
}

fn arb_inbound_pointer() -> impl Strategy<Value = OwnershipRecord> {
    (any::<[u8; 16]>(), any::<u64>()).prop_map(
        |(prior_generation_locator_id, prior_generation_epoch)| {
            OwnershipRecord::InboundPointer(InboundPointerRecord {
                prior_generation_locator_id,
                prior_generation_epoch,
            })
        },
    )
}

fn arb_outbound_pointer() -> impl Strategy<Value = OwnershipRecord> {
    (any::<[u8; 16]>(), any::<u64>()).prop_map(|(next_generation_locator_id, cutover_epoch)| {
        OwnershipRecord::OutboundPointer(OutboundPointerRecord {
            next_generation_locator_id,
            cutover_epoch,
        })
    })
}

fn arb_rescue_policy_choice() -> impl Strategy<Value = OwnershipRecord> {
    (any::<u8>(), prop::collection::vec(any::<u8>(), 0..=512)).prop_map(
        |(policy_tag, parameter_payload)| {
            OwnershipRecord::RescuePolicyChoice(RescuePolicyChoiceRecord {
                policy_tag,
                parameter_payload,
            })
        },
    )
}

fn arb_partitioning_rule() -> impl Strategy<Value = PartitioningRule> {
    prop_oneof![
        any::<u32>().prop_map(|total_draglines| PartitioningRule::StaticModulo { total_draglines }),
        (any::<u64>(), any::<u32>()).prop_map(|(hash_seed, virtual_node_count)| {
            PartitioningRule::ConsistentHash {
                hash_seed,
                virtual_node_count,
            }
        }),
    ]
}

fn arb_identity_structure() -> impl Strategy<Value = OwnershipRecord> {
    (
        any::<[u8; 16]>(),
        any::<u32>(),
        any::<u32>(),
        arb_partitioning_rule(),
    )
        .prop_map(
            |(dataset_id, structure_version, dragline_id, partitioning_rule)| {
                OwnershipRecord::IdentityStructure(IdentityStructureRecord {
                    dataset_id,
                    structure_version,
                    dragline_id,
                    partitioning_rule,
                })
            },
        )
}

fn arb_schema_descriptor() -> impl Strategy<Value = OwnershipRecord> {
    (any::<u32>(), prop::collection::vec(any::<u8>(), 0..=512)).prop_map(
        |(schema_version, descriptor_bytes)| OwnershipRecord::SchemaDescriptor {
            schema_version,
            descriptor_bytes,
        },
    )
}

fn arb_ownership_record() -> impl Strategy<Value = OwnershipRecord> {
    prop_oneof![
        arb_claim(),
        arb_clean_release(),
        arb_migration_start(),
        arb_migration_end(),
        arb_inbound_pointer(),
        arb_outbound_pointer(),
        arb_rescue_policy_choice(),
        arb_identity_structure(),
        arb_schema_descriptor(),
    ]
}

#[test]
fn test_container_frame_checksum_mismatch_rejected() {
    let frame = ContainerFrame {
        payload: vec![1, 2, 3],
        checksum: 0xDEAD_BEEF,
    };
    let mut buf = Vec::new();
    let err = frame.encode(&mut buf).expect_err("mismatched checksum");
    assert!(matches!(
        err,
        pardosa::encoding::EncodeError::ChecksumMismatch { .. }
    ));
}

proptest! {
    #[test]
    fn test_container_frame_invertibility(frame in arb_container_frame()) {
        let mut buf = Vec::new();
        frame.encode(&mut buf).expect("encode frame");
        let (decoded_payload, len) = ContainerFrame::decode(&buf).expect("decode payload");
        prop_assert_eq!(&decoded_payload, &frame.payload);
        prop_assert_eq!(len, buf.len());

        let (decoded_frame, frame_len) = ContainerFrame::decode_frame(&buf).expect("decode frame");
        prop_assert_eq!(&decoded_frame, &frame);
        prop_assert_eq!(frame_len, buf.len());
    }

    #[test]
    fn test_event_envelope_invertibility(env in arb_event_envelope()) {
        let mut buf = Vec::new();
        env.encode(&mut buf).expect("encode envelope");
        let (decoded_env, len) = EventEnvelope::decode(&buf).expect("decode envelope");
        prop_assert_eq!(&decoded_env, &env);
        prop_assert_eq!(len, buf.len());
    }

    #[test]
    fn test_ownership_record_invertibility(rec in arb_ownership_record()) {
        let mut buf = Vec::new();
        rec.encode(&mut buf);
        let (decoded_rec, len) = OwnershipRecord::decode(&buf).expect("decode ownership record");
        prop_assert_eq!(&decoded_rec, &rec);
        prop_assert_eq!(len, buf.len());
    }

    #[test]
    fn test_container_header_invertibility(header in arb_container_header()) {
        let mut buf = Vec::new();
        header.encode(&mut buf);
        let (decoded_header, len) = ContainerHeader::decode(&buf).expect("decode container header");
        prop_assert_eq!(&decoded_header, &header);
        prop_assert_eq!(len, buf.len());
        prop_assert_eq!(len, 12);
    }

    #[test]
    fn test_container_frame_resilience_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..=4096)) {
        let res_payload = ContainerFrame::decode(&bytes);
        let res_frame = ContainerFrame::decode_frame(&bytes);
        if bytes.len() < 8 {
            prop_assert!(res_payload.is_err());
            prop_assert!(res_frame.is_err());
        } else {
            prop_assert!(matches!(res_payload, Ok(_) | Err(_)));
            prop_assert!(matches!(res_frame, Ok(_) | Err(_)));
        }
    }

    #[test]
    fn test_event_envelope_resilience_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..=4096)) {
        let res = EventEnvelope::decode(&bytes);
        if bytes.len() < 85 {
            prop_assert!(res.is_err());
        } else {
            prop_assert!(matches!(res, Ok(_) | Err(_)));
        }
    }

    #[test]
    fn test_ownership_record_resilience_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..=4096)) {
        let res = OwnershipRecord::decode(&bytes);
        if bytes.is_empty() {
            prop_assert!(res.is_err());
        } else {
            prop_assert!(matches!(res, Ok(_) | Err(_)));
        }
    }

    #[test]
    fn test_container_header_resilience_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..=4096)) {
        let res = ContainerHeader::decode(&bytes);
        if bytes.len() < 12 {
            prop_assert!(res.is_err());
        } else {
            prop_assert!(matches!(res, Ok(_) | Err(_)));
        }
    }

    #[test]
    fn test_container_frame_resilience_mutated_valid(
        frame in arb_container_frame(),
        cut in any::<prop::sample::Index>(),
        flip_idx in any::<prop::sample::Index>(),
        flip_val in any::<u8>(),
    ) {
        let mut buf = Vec::new();
        frame.encode(&mut buf).expect("encode frame");

        let truncated = &buf[..cut.index(buf.len() + 1)];
        if truncated.len() < buf.len() {
            prop_assert!(ContainerFrame::decode(truncated).is_err());
            prop_assert!(ContainerFrame::decode_frame(truncated).is_err());
        }

        if !buf.is_empty() {
            let mut corrupted = buf.clone();
            let idx = flip_idx.index(corrupted.len());
            corrupted[idx] ^= flip_val.max(1);
            prop_assert!(ContainerFrame::decode(&corrupted).is_err());
            prop_assert!(ContainerFrame::decode_frame(&corrupted).is_err());
        }
    }

    #[test]
    fn test_event_envelope_resilience_mutated_valid(
        env in arb_event_envelope(),
        cut in any::<prop::sample::Index>(),
        flip_idx in any::<prop::sample::Index>(),
        flip_val in any::<u8>(),
    ) {
        let mut buf = Vec::new();
        env.encode(&mut buf).expect("encode envelope");

        let truncated = &buf[..cut.index(buf.len() + 1)];
        if truncated.len() < buf.len() {
            prop_assert!(EventEnvelope::decode(truncated).is_err());
        }

        if !buf.is_empty() {
            let mut corrupted = buf.clone();
            let idx = flip_idx.index(corrupted.len());
            corrupted[idx] ^= flip_val.max(1);
            if let Ok((corrupted_env, _)) = EventEnvelope::decode(&corrupted) {
                prop_assert_ne!(&corrupted_env, &env);
            }
        }
    }

    #[test]
    fn test_ownership_record_resilience_mutated_valid(
        rec in arb_ownership_record(),
        cut in any::<prop::sample::Index>(),
        flip_idx in any::<prop::sample::Index>(),
        flip_val in any::<u8>(),
    ) {
        let mut buf = Vec::new();
        rec.encode(&mut buf);

        let truncated = &buf[..cut.index(buf.len() + 1)];
        if truncated.len() < buf.len() {
            if let Ok((truncated_rec, _)) = OwnershipRecord::decode(truncated) {
                prop_assert_ne!(&truncated_rec, &rec);
            }
        }

        if !buf.is_empty() {
            let mut corrupted = buf.clone();
            let idx = flip_idx.index(corrupted.len());
            corrupted[idx] ^= flip_val.max(1);
            if let Ok((corrupted_rec, _)) = OwnershipRecord::decode(&corrupted) {
                prop_assert_ne!(&corrupted_rec, &rec);
            }
        }
    }

    #[test]
    fn test_container_header_resilience_mutated_valid(
        header in arb_container_header(),
        cut in any::<prop::sample::Index>(),
        flip_idx in any::<prop::sample::Index>(),
        flip_val in any::<u8>(),
    ) {
        let mut buf = Vec::new();
        header.encode(&mut buf);

        let truncated = &buf[..cut.index(buf.len() + 1)];
        if truncated.len() < 12 {
            prop_assert!(ContainerHeader::decode(truncated).is_err());
        }

        if !buf.is_empty() {
            let mut corrupted = buf.clone();
            let idx = flip_idx.index(corrupted.len());
            corrupted[idx] ^= flip_val.max(1);
            if let Ok((corrupted_header, _)) = ContainerHeader::decode(&corrupted) {
                prop_assert_ne!(&corrupted_header, &header);
            }
        }
    }
}
