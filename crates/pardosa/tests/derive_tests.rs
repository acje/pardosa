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
