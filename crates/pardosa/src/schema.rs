//! Schema descriptors, admitted S3 constructor AST, and derived schema identity.

use crate::encoding::{
    DecodeError, EncodeError, EventBytes, EventString, EventVec, NonEmptyEventString, Timestamp,
    Uuid, ValueConstraint,
};
use crate::store::{FailureCondition, OperationFailure};

mod floats;
pub use floats::{EventF32, EventF64, OrderedF32, OrderedF64};

/// Maximum recursion depth allowed during descriptor AST decoding to prevent cycles.
pub const MAX_DESCRIPTOR_DEPTH: usize = 64;

/// A field within a struct descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDescriptor {
    /// Field name.
    pub name: String,
    /// Type descriptor of the field.
    pub node: DescriptorNode,
}

/// A variant within an enum descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDescriptor {
    /// Explicit discriminant value.
    pub discriminant: u32,
    /// Variant name.
    pub name: String,
    /// Variant payload descriptor (or `None` if unit variant).
    pub payload: Option<DescriptorNode>,
}

/// Complete S3 admitted constructor vocabulary AST per C6.23.
///
/// In Pardosa 0.5.2, deterministic float scalar leaves `OrderedF32` (tag `0x13`) and `OrderedF64` (tag `0x14`)
/// are admitted, representing normalized non-subnormal finite numbers and positive zero. The classification
/// enums `EventF32` and `EventF64` are represented as composite `Enum` descriptors (tag `0x10`) with a 1-byte
/// discriminant and four variants: `NaN` (0), `NegInf` (1), `Finite` (2, payload `OrderedF32`/`OrderedF64`), and `PosInf` (3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescriptorNode {
    /// 0x01: Unsigned 8-bit integer.
    U8,
    /// 0x02: Unsigned 16-bit integer (little-endian).
    U16,
    /// 0x03: Unsigned 32-bit integer (little-endian).
    U32,
    /// 0x04: Unsigned 64-bit integer (little-endian).
    U64,
    /// 0x05: Signed 8-bit integer.
    I8,
    /// 0x06: Signed 16-bit integer (little-endian).
    I16,
    /// 0x07: Signed 32-bit integer (little-endian).
    I32,
    /// 0x08: Signed 64-bit integer (little-endian).
    I64,
    /// 0x09: Two-valued boolean (0x00=false, 0x01=true).
    Bool,
    /// 0x0A: Bounded UTF-8 text.
    EventString {
        /// Maximum allowed byte length.
        max_bytes: u32,
    },
    /// 0x0B: Bounded non-empty UTF-8 text (1 <= len <= max_bytes).
    NonEmptyEventString {
        /// Maximum allowed byte length.
        max_bytes: u32,
    },
    /// 0x0C: Bounded raw byte sequence.
    EventBytes {
        /// Maximum allowed byte length.
        max_bytes: u32,
    },
    /// 0x0D: Bounded collection of admitted type items.
    EventVec {
        /// Inner item type descriptor.
        inner: Box<DescriptorNode>,
        /// Maximum allowed item count.
        max_items: u32,
    },
    /// 0x0E: Optional admitted value.
    Option {
        /// Inner type descriptor.
        inner: Box<DescriptorNode>,
    },
    /// 0x0F: Composite product structure with ordered fields.
    Struct {
        /// Type name.
        name: String,
        /// Ordered field descriptors.
        fields: Vec<FieldDescriptor>,
    },
    /// 0x10: Composite sum enum with explicit discriminants.
    Enum {
        /// Type name.
        name: String,
        /// Discriminant width in bytes (1 or 2).
        discriminant_width: u8,
        /// Variants of this enum.
        variants: Vec<VariantDescriptor>,
    },
    /// 0x11: Non-zero Unix nanosecond timestamp.
    Timestamp,
    /// 0x12: 128-bit UUID.
    Uuid,
    /// 0x13: Deterministic IEEE-754 32-bit floating point value (normalized, non-subnormal finite or +0.0).
    OrderedF32,
    /// 0x14: Deterministic IEEE-754 64-bit floating point value (normalized, non-subnormal finite or +0.0).
    OrderedF64,
}

impl DescriptorNode {
    /// Returns 1-byte wire tag for this constructor per C6.23.
    #[must_use]
    pub fn constructor_tag(&self) -> u8 {
        match self {
            Self::U8 => 0x01,
            Self::U16 => 0x02,
            Self::U32 => 0x03,
            Self::U64 => 0x04,
            Self::I8 => 0x05,
            Self::I16 => 0x06,
            Self::I32 => 0x07,
            Self::I64 => 0x08,
            Self::Bool => 0x09,
            Self::EventString { .. } => 0x0A,
            Self::NonEmptyEventString { .. } => 0x0B,
            Self::EventBytes { .. } => 0x0C,
            Self::EventVec { .. } => 0x0D,
            Self::Option { .. } => 0x0E,
            Self::Struct { .. } => 0x0F,
            Self::Enum { .. } => 0x10,
            Self::Timestamp => 0x11,
            Self::Uuid => 0x12,
            Self::OrderedF32 => 0x13,
            Self::OrderedF64 => 0x14,
        }
    }

    /// Serializes this descriptor node into binary AST format per C6.23.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(self.constructor_tag());
        match self {
            Self::U8
            | Self::U16
            | Self::U32
            | Self::U64
            | Self::I8
            | Self::I16
            | Self::I32
            | Self::I64
            | Self::Bool
            | Self::Timestamp
            | Self::Uuid
            | Self::OrderedF32
            | Self::OrderedF64 => {}
            Self::EventString { max_bytes }
            | Self::NonEmptyEventString { max_bytes }
            | Self::EventBytes { max_bytes } => {
                buf.extend_from_slice(&max_bytes.to_le_bytes());
            }
            Self::EventVec { inner, max_items } => {
                inner.encode(buf);
                buf.extend_from_slice(&max_items.to_le_bytes());
            }
            Self::Option { inner } => {
                inner.encode(buf);
            }
            Self::Struct { name, fields } => {
                let name_bytes = name.as_bytes();
                buf.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(name_bytes);
                buf.extend_from_slice(&(fields.len() as u32).to_le_bytes());
                for f in fields {
                    let fname_bytes = f.name.as_bytes();
                    buf.extend_from_slice(&(fname_bytes.len() as u32).to_le_bytes());
                    buf.extend_from_slice(fname_bytes);
                    f.node.encode(buf);
                }
            }
            Self::Enum {
                name,
                discriminant_width,
                variants,
            } => {
                let name_bytes = name.as_bytes();
                buf.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(name_bytes);
                buf.push(*discriminant_width);
                buf.extend_from_slice(&(variants.len() as u32).to_le_bytes());
                for v in variants {
                    if *discriminant_width == 1 {
                        buf.push(v.discriminant as u8);
                    } else {
                        buf.extend_from_slice(&(v.discriminant as u16).to_le_bytes());
                    }
                    let vname_bytes = v.name.as_bytes();
                    buf.extend_from_slice(&(vname_bytes.len() as u32).to_le_bytes());
                    buf.extend_from_slice(vname_bytes);
                    match &v.payload {
                        Some(payload) => payload.encode(buf),
                        None => buf.push(0x00),
                    }
                }
            }
        }
    }

    /// Decodes a descriptor node from binary AST format.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedPayload` if bytes are truncated.
    /// Returns `DecodeError::UnknownConstructorTag` if tag is outside 0x01..=0x14.
    /// Returns `DecodeError::CycleDetected` if depth exceeds `MAX_DESCRIPTOR_DEPTH`.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        Self::decode_recursive(buf, 0)
    }

    fn decode_recursive(buf: &[u8], depth: usize) -> Result<(Self, usize), DecodeError> {
        if depth > MAX_DESCRIPTOR_DEPTH {
            return Err(DecodeError::CycleDetected {
                type_name: "maximum recursion depth exceeded".to_string(),
            });
        }
        if buf.is_empty() {
            return Err(DecodeError::TruncatedPayload {
                expected: 1,
                available: 0,
            });
        }
        let tag = buf[0];
        match tag {
            0x01 => Ok((Self::U8, 1)),
            0x02 => Ok((Self::U16, 1)),
            0x03 => Ok((Self::U32, 1)),
            0x04 => Ok((Self::U64, 1)),
            0x05 => Ok((Self::I8, 1)),
            0x06 => Ok((Self::I16, 1)),
            0x07 => Ok((Self::I32, 1)),
            0x08 => Ok((Self::I64, 1)),
            0x09 => Ok((Self::Bool, 1)),
            0x0A => {
                if buf.len() < 5 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: 5,
                        available: buf.len(),
                    });
                }
                let max_bytes = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
                Ok((Self::EventString { max_bytes }, 5))
            }
            0x0B => {
                if buf.len() < 5 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: 5,
                        available: buf.len(),
                    });
                }
                let max_bytes = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
                Ok((Self::NonEmptyEventString { max_bytes }, 5))
            }
            0x0C => {
                if buf.len() < 5 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: 5,
                        available: buf.len(),
                    });
                }
                let max_bytes = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
                Ok((Self::EventBytes { max_bytes }, 5))
            }
            0x0D => {
                let (inner, consumed) = Self::decode_recursive(&buf[1..], depth + 1)?;
                let total_before_max = 1 + consumed;
                if buf.len() < total_before_max + 4 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: total_before_max + 4,
                        available: buf.len(),
                    });
                }
                let max_items = u32::from_le_bytes([
                    buf[total_before_max],
                    buf[total_before_max + 1],
                    buf[total_before_max + 2],
                    buf[total_before_max + 3],
                ]);
                Ok((
                    Self::EventVec {
                        inner: Box::new(inner),
                        max_items,
                    },
                    total_before_max + 4,
                ))
            }
            0x0E => {
                let (inner, consumed) = Self::decode_recursive(&buf[1..], depth + 1)?;
                Ok((
                    Self::Option {
                        inner: Box::new(inner),
                    },
                    1 + consumed,
                ))
            }
            0x0F => {
                let mut cursor = 1;
                if buf.len() < cursor + 4 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: cursor + 4,
                        available: buf.len(),
                    });
                }
                let name_len = u32::from_le_bytes([
                    buf[cursor],
                    buf[cursor + 1],
                    buf[cursor + 2],
                    buf[cursor + 3],
                ]) as usize;
                cursor += 4;
                if buf.len() < cursor + name_len {
                    return Err(DecodeError::TruncatedPayload {
                        expected: cursor + name_len,
                        available: buf.len(),
                    });
                }
                let name = std::str::from_utf8(&buf[cursor..cursor + name_len])
                    .map_err(|_| DecodeError::InvalidUtf8)?
                    .to_string();
                cursor += name_len;
                if buf.len() < cursor + 4 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: cursor + 4,
                        available: buf.len(),
                    });
                }
                let field_count = u32::from_le_bytes([
                    buf[cursor],
                    buf[cursor + 1],
                    buf[cursor + 2],
                    buf[cursor + 3],
                ]) as usize;
                cursor += 4;
                let mut fields = Vec::with_capacity(field_count);
                for _ in 0..field_count {
                    if buf.len() < cursor + 4 {
                        return Err(DecodeError::TruncatedPayload {
                            expected: cursor + 4,
                            available: buf.len(),
                        });
                    }
                    let fname_len = u32::from_le_bytes([
                        buf[cursor],
                        buf[cursor + 1],
                        buf[cursor + 2],
                        buf[cursor + 3],
                    ]) as usize;
                    cursor += 4;
                    if buf.len() < cursor + fname_len {
                        return Err(DecodeError::TruncatedPayload {
                            expected: cursor + fname_len,
                            available: buf.len(),
                        });
                    }
                    let fname = std::str::from_utf8(&buf[cursor..cursor + fname_len])
                        .map_err(|_| DecodeError::InvalidUtf8)?
                        .to_string();
                    cursor += fname_len;
                    let (fnode, fconsumed) = Self::decode_recursive(&buf[cursor..], depth + 1)?;
                    cursor += fconsumed;
                    fields.push(FieldDescriptor {
                        name: fname,
                        node: fnode,
                    });
                }
                Ok((Self::Struct { name, fields }, cursor))
            }
            0x10 => {
                let mut cursor = 1;
                if buf.len() < cursor + 4 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: cursor + 4,
                        available: buf.len(),
                    });
                }
                let name_len = u32::from_le_bytes([
                    buf[cursor],
                    buf[cursor + 1],
                    buf[cursor + 2],
                    buf[cursor + 3],
                ]) as usize;
                cursor += 4;
                if buf.len() < cursor + name_len {
                    return Err(DecodeError::TruncatedPayload {
                        expected: cursor + name_len,
                        available: buf.len(),
                    });
                }
                let name = std::str::from_utf8(&buf[cursor..cursor + name_len])
                    .map_err(|_| DecodeError::InvalidUtf8)?
                    .to_string();
                cursor += name_len;
                if buf.len() < cursor + 1 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: cursor + 1,
                        available: buf.len(),
                    });
                }
                let discriminant_width = buf[cursor];
                cursor += 1;
                if discriminant_width != 1 && discriminant_width != 2 {
                    return Err(DecodeError::InvalidDiscriminantWidth {
                        width: discriminant_width,
                    });
                }
                if buf.len() < cursor + 4 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: cursor + 4,
                        available: buf.len(),
                    });
                }
                let variant_count = u32::from_le_bytes([
                    buf[cursor],
                    buf[cursor + 1],
                    buf[cursor + 2],
                    buf[cursor + 3],
                ]) as usize;
                cursor += 4;
                let mut variants = Vec::with_capacity(variant_count);
                for _ in 0..variant_count {
                    let disc_width_bytes = discriminant_width as usize;
                    if buf.len() < cursor + disc_width_bytes {
                        return Err(DecodeError::TruncatedPayload {
                            expected: cursor + disc_width_bytes,
                            available: buf.len(),
                        });
                    }
                    let discriminant = if discriminant_width == 1 {
                        let d = buf[cursor] as u32;
                        cursor += 1;
                        d
                    } else {
                        let d = u16::from_le_bytes([buf[cursor], buf[cursor + 1]]) as u32;
                        cursor += 2;
                        d
                    };
                    if buf.len() < cursor + 4 {
                        return Err(DecodeError::TruncatedPayload {
                            expected: cursor + 4,
                            available: buf.len(),
                        });
                    }
                    let vname_len = u32::from_le_bytes([
                        buf[cursor],
                        buf[cursor + 1],
                        buf[cursor + 2],
                        buf[cursor + 3],
                    ]) as usize;
                    cursor += 4;
                    if buf.len() < cursor + vname_len {
                        return Err(DecodeError::TruncatedPayload {
                            expected: cursor + vname_len,
                            available: buf.len(),
                        });
                    }
                    let vname = std::str::from_utf8(&buf[cursor..cursor + vname_len])
                        .map_err(|_| DecodeError::InvalidUtf8)?
                        .to_string();
                    cursor += vname_len;
                    if buf.len() < cursor + 1 {
                        return Err(DecodeError::TruncatedPayload {
                            expected: cursor + 1,
                            available: buf.len(),
                        });
                    }
                    let payload = if buf[cursor] == 0x00 {
                        cursor += 1;
                        None
                    } else {
                        let (pnode, pconsumed) = Self::decode_recursive(&buf[cursor..], depth + 1)?;
                        cursor += pconsumed;
                        Some(pnode)
                    };
                    variants.push(VariantDescriptor {
                        discriminant,
                        name: vname,
                        payload,
                    });
                }
                Ok((
                    Self::Enum {
                        name,
                        discriminant_width,
                        variants,
                    },
                    cursor,
                ))
            }
            0x11 => Ok((Self::Timestamp, 1)),
            0x12 => Ok((Self::Uuid, 1)),
            0x13 => Ok((Self::OrderedF32, 1)),
            0x14 => Ok((Self::OrderedF64, 1)),
            other => Err(DecodeError::UnknownConstructorTag { tag: other }),
        }
    }
}

/// A complete schema descriptor combining version and AST root node per C6.26/C6.27.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaDescriptor {
    /// Schema version number.
    pub version: u32,
    /// Root AST node.
    pub root: DescriptorNode,
}

impl SchemaDescriptor {
    /// Creates a new schema descriptor.
    #[must_use]
    pub fn new(version: u32, root: DescriptorNode) -> Self {
        Self { version, root }
    }

    /// Encodes this schema descriptor into wire format (version + AST).
    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.version.to_le_bytes());
        self.root.encode(buf);
    }

    /// Decodes a schema descriptor from wire bytes.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedPayload` if bytes are fewer than 4.
    /// Returns `DecodeError` on AST decoding failure.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::TruncatedPayload {
                expected: 4,
                available: buf.len(),
            });
        }
        let version = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let (root, consumed) = DescriptorNode::decode(&buf[4..])?;
        Ok((Self { version, root }, 4 + consumed))
    }

    /// Derives the schema identity for this descriptor.
    #[must_use]
    pub fn identity(&self) -> SchemaIdentity {
        SchemaIdentity::from_descriptor(self.version, &self.root)
    }

    /// Asserts that this schema descriptor is structurally complete per C8.2.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::MissingSchemaDescriptor`]
    /// if version is zero.
    /// Returns [`OperationFailure`] with [`FailureCondition::ValueConstraintViolated`]
    /// if any variant, field, or bound is invalid.
    pub fn validate_structural_completeness(&self) -> Result<(), OperationFailure> {
        if self.version == 0 {
            return Err(OperationFailure::new(
                FailureCondition::MissingSchemaDescriptor,
                "schema descriptor declared version must be non-zero per C8.2",
            ));
        }
        self.root.validate_structural_completeness()
    }
}

impl DescriptorNode {
    /// Asserts that this descriptor AST node is structurally complete per C8.2.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::ValueConstraintViolated`]
    /// if any variant, field, or bound is invalid.
    pub fn validate_structural_completeness(&self) -> Result<(), OperationFailure> {
        match self {
            Self::U8
            | Self::U16
            | Self::U32
            | Self::U64
            | Self::I8
            | Self::I16
            | Self::I32
            | Self::I64
            | Self::Bool
            | Self::Timestamp
            | Self::Uuid
            | Self::OrderedF32
            | Self::OrderedF64 => Ok(()),
            Self::EventString { max_bytes }
            | Self::NonEmptyEventString { max_bytes }
            | Self::EventBytes { max_bytes } => {
                if *max_bytes == 0 {
                    Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: ValueConstraint::Empty,
                        },
                        "bounded type must carry non-zero bound per C8.2",
                    ))
                } else {
                    Ok(())
                }
            }
            Self::EventVec { inner, max_items } => {
                if *max_items == 0 {
                    Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: ValueConstraint::Empty,
                        },
                        "collection must carry non-zero max_items per C8.2",
                    ))
                } else {
                    inner.validate_structural_completeness()
                }
            }
            Self::Option { inner } => inner.validate_structural_completeness(),
            Self::Struct { fields, .. } => {
                for f in fields {
                    if f.name.is_empty() {
                        return Err(OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::Empty,
                            },
                            "struct field name must be non-empty per C8.2",
                        ));
                    }
                    f.node.validate_structural_completeness()?;
                }
                Ok(())
            }
            Self::Enum {
                variants,
                discriminant_width,
                ..
            } => {
                if variants.is_empty() {
                    return Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: ValueConstraint::Empty,
                        },
                        "enumeration must carry at least one variant per C8.2",
                    ));
                }
                if *discriminant_width != 1 && *discriminant_width != 2 {
                    return Err(OperationFailure::new(
                        FailureCondition::ValueConstraintViolated {
                            constraint: ValueConstraint::NotReal,
                        },
                        "enumeration discriminant width must be 1 or 2 bytes per C8.2",
                    ));
                }
                for v in variants {
                    if v.name.is_empty() {
                        return Err(OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::Empty,
                            },
                            "enum variant name must be non-empty per C8.2",
                        ));
                    }
                    if let Some(payload) = &v.payload {
                        payload.validate_structural_completeness()?;
                    }
                }
                Ok(())
            }
        }
    }
}

/// Derived cryptographic schema identity computed via BLAKE3 over canonical descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SchemaIdentity([u8; 32]);

impl SchemaIdentity {
    /// Creates a schema identity from a raw 32-byte digest.
    #[must_use]
    pub const fn from_raw(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Computes schema identity from schema version and descriptor AST root.
    #[must_use]
    pub fn from_descriptor(version: u32, root: &DescriptorNode) -> Self {
        let mut buf = Vec::new();
        buf.extend_from_slice(&version.to_le_bytes());
        root.encode(&mut buf);
        let hash = blake3::hash(&buf);
        Self(*hash.as_bytes())
    }

    /// Returns reference to 32-byte hash digest.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns hex-encoded representation of the identity digest.
    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
        for &b in &self.0 {
            s.push(HEX_CHARS[(b >> 4) as usize] as char);
            s.push(HEX_CHARS[(b & 0x0F) as usize] as char);
        }
        s
    }
}

/// Derives a 16-byte fiber identifier from a domain key string using canonical domain separation.
#[must_use]
pub fn derive_fiber_id(domain_key: &str) -> [u8; 16] {
    let key_material = blake3::derive_key("pardosa.fiber_id.v1", domain_key.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(&key_material[..16]);
    out
}

/// Trait implemented by payload types declaring schema descriptor and codecs.
pub trait PardosaSchema: Sized {
    /// Returns declared schema version.
    fn schema_version() -> u32;

    /// Returns root schema descriptor AST node.
    fn schema_descriptor() -> DescriptorNode;

    /// Derives schema identity.
    fn schema_identity() -> SchemaIdentity {
        SchemaIdentity::from_descriptor(Self::schema_version(), &Self::schema_descriptor())
    }

    /// Encodes this payload into wire bytes.
    ///
    /// # Errors
    /// Returns `EncodeError` if payload serialization fails.
    fn encode_payload(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError>;

    /// Decodes a payload from wire bytes.
    ///
    /// # Errors
    /// Returns `DecodeError` if wire bytes are malformed or invalid.
    fn decode_payload(buf: &[u8]) -> Result<Self, DecodeError>;
}

/// Trait implemented by admitted types composing event payloads.
pub trait PardosaType: Sized {
    /// Returns descriptor node for this admitted type.
    fn descriptor_node() -> DescriptorNode;

    /// Encodes this value into wire format.
    ///
    /// # Errors
    /// Returns `EncodeError` on failure.
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError>;

    /// Decodes a value from wire format.
    ///
    /// # Errors
    /// Returns `DecodeError` on failure.
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError>;
}

impl PardosaType for u8 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::U8
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.push(*self);
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::TruncatedPayload {
                expected: 1,
                available: 0,
            });
        }
        Ok((buf[0], 1))
    }
}

impl PardosaType for u16 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::U16
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.extend_from_slice(&self.to_le_bytes());
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 2 {
            return Err(DecodeError::TruncatedPayload {
                expected: 2,
                available: buf.len(),
            });
        }
        Ok((u16::from_le_bytes([buf[0], buf[1]]), 2))
    }
}

impl PardosaType for u32 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::U32
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.extend_from_slice(&self.to_le_bytes());
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::TruncatedPayload {
                expected: 4,
                available: buf.len(),
            });
        }
        Ok((u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]), 4))
    }
}

impl PardosaType for u64 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::U64
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.extend_from_slice(&self.to_le_bytes());
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 8 {
            return Err(DecodeError::TruncatedPayload {
                expected: 8,
                available: buf.len(),
            });
        }
        Ok((
            u64::from_le_bytes([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            ]),
            8,
        ))
    }
}

impl PardosaType for i8 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::I8
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.push(*self as u8);
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::TruncatedPayload {
                expected: 1,
                available: 0,
            });
        }
        Ok((buf[0] as i8, 1))
    }
}

impl PardosaType for i16 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::I16
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.extend_from_slice(&self.to_le_bytes());
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 2 {
            return Err(DecodeError::TruncatedPayload {
                expected: 2,
                available: buf.len(),
            });
        }
        Ok((i16::from_le_bytes([buf[0], buf[1]]), 2))
    }
}

impl PardosaType for i32 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::I32
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.extend_from_slice(&self.to_le_bytes());
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::TruncatedPayload {
                expected: 4,
                available: buf.len(),
            });
        }
        Ok((i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]), 4))
    }
}

impl PardosaType for i64 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::I64
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.extend_from_slice(&self.to_le_bytes());
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 8 {
            return Err(DecodeError::TruncatedPayload {
                expected: 8,
                available: buf.len(),
            });
        }
        Ok((
            i64::from_le_bytes([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            ]),
            8,
        ))
    }
}

impl PardosaType for bool {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::Bool
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.push(if *self { 0x01 } else { 0x00 });
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::TruncatedPayload {
                expected: 1,
                available: 0,
            });
        }
        match buf[0] {
            0x00 => Ok((false, 1)),
            0x01 => Ok((true, 1)),
            other => Err(DecodeError::InvalidBoolean { value: other }),
        }
    }
}

impl PardosaType for Timestamp {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::Timestamp
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.encode(buf);
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        Self::decode(buf)
    }
}

impl PardosaType for Uuid {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::Uuid
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.encode(buf);
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        Self::decode(buf)
    }
}

impl<T: PardosaType> PardosaType for Option<T> {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::Option {
            inner: Box::new(T::descriptor_node()),
        }
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            None => {
                buf.push(0x00);
                Ok(())
            }
            Some(inner) => {
                buf.push(0x01);
                inner.encode_type(buf)
            }
        }
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::TruncatedPayload {
                expected: 1,
                available: 0,
            });
        }
        match buf[0] {
            0x00 => Ok((None, 1)),
            0x01 => {
                let (val, consumed) = T::decode_type(&buf[1..])?;
                Ok((Some(val), 1 + consumed))
            }
            other => Err(DecodeError::InvalidOptionTag { tag: other }),
        }
    }
}

impl<const MAX: usize> PardosaType for EventString<MAX> {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::EventString {
            max_bytes: MAX as u32,
        }
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.encode(buf);
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        Self::decode(buf)
    }
}

impl<const MAX: usize> PardosaType for NonEmptyEventString<MAX> {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::NonEmptyEventString {
            max_bytes: MAX as u32,
        }
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.encode(buf);
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        Self::decode(buf)
    }
}

impl<const MAX: usize> PardosaType for EventBytes<MAX> {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::EventBytes {
            max_bytes: MAX as u32,
        }
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.encode(buf);
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        Self::decode(buf)
    }
}

impl<T: PardosaType, const MAX: usize> PardosaType for EventVec<T, MAX> {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::EventVec {
            inner: Box::new(T::descriptor_node()),
            max_items: MAX as u32,
        }
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        let count = self.as_slice().len();
        if count > MAX {
            return Err(EncodeError::ItemCountExceeded { count, max: MAX });
        }
        buf.extend_from_slice(&(count as u32).to_le_bytes());
        for item in self.as_slice() {
            item.encode_type(buf)?;
        }
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::TruncatedPayload {
                expected: 4,
                available: buf.len(),
            });
        }
        let count = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if count > MAX {
            return Err(DecodeError::ItemCountExceeded { count, max: MAX });
        }
        let mut cursor = 4;
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            let (item, consumed) = T::decode_type(&buf[cursor..])?;
            cursor += consumed;
            items.push(item);
        }
        Ok((Self::new(items)?, cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_primitive_descriptors_roundtrip() {
        let primitives = vec![
            DescriptorNode::U8,
            DescriptorNode::U16,
            DescriptorNode::U32,
            DescriptorNode::U64,
            DescriptorNode::I8,
            DescriptorNode::I16,
            DescriptorNode::I32,
            DescriptorNode::I64,
            DescriptorNode::Bool,
            DescriptorNode::Timestamp,
            DescriptorNode::Uuid,
            DescriptorNode::OrderedF32,
            DescriptorNode::OrderedF64,
            DescriptorNode::EventString { max_bytes: 128 },
            DescriptorNode::NonEmptyEventString { max_bytes: 64 },
            DescriptorNode::EventBytes { max_bytes: 256 },
            DescriptorNode::Option {
                inner: Box::new(DescriptorNode::U32),
            },
            DescriptorNode::EventVec {
                inner: Box::new(DescriptorNode::I64),
                max_items: 50,
            },
        ];

        for node in primitives {
            let mut buf = Vec::new();
            node.encode(&mut buf);
            let (decoded, consumed) = DescriptorNode::decode(&buf).expect("decode primitive");
            assert_eq!(consumed, buf.len());
            assert_eq!(decoded, node);
        }
    }

    #[test]
    fn test_composite_struct_descriptor_roundtrip() {
        let node = DescriptorNode::Struct {
            name: "OrderPlaced".to_string(),
            fields: vec![
                FieldDescriptor {
                    name: "order_id".to_string(),
                    node: DescriptorNode::U64,
                },
                FieldDescriptor {
                    name: "customer_id".to_string(),
                    node: DescriptorNode::Uuid,
                },
                FieldDescriptor {
                    name: "notes".to_string(),
                    node: DescriptorNode::Option {
                        inner: Box::new(DescriptorNode::EventString { max_bytes: 512 }),
                    },
                },
            ],
        };

        let mut buf = Vec::new();
        node.encode(&mut buf);
        let (decoded, consumed) = DescriptorNode::decode(&buf).expect("decode struct");
        assert_eq!(consumed, buf.len());
        assert_eq!(decoded, node);
    }

    #[test]
    fn test_composite_enum_descriptor_roundtrip() {
        let node = DescriptorNode::Enum {
            name: "Event".to_string(),
            discriminant_width: 1,
            variants: vec![
                VariantDescriptor {
                    discriminant: 0,
                    name: "Tombstone".to_string(),
                    payload: None,
                },
                VariantDescriptor {
                    discriminant: 1,
                    name: "Created".to_string(),
                    payload: Some(DescriptorNode::U32),
                },
            ],
        };

        let mut buf = Vec::new();
        node.encode(&mut buf);
        let (decoded, consumed) = DescriptorNode::decode(&buf).expect("decode enum");
        assert_eq!(consumed, buf.len());
        assert_eq!(decoded, node);
    }

    #[test]
    fn test_invalid_discriminant_width() {
        let mut buf = vec![0x10];
        let name_bytes = b"Test";
        buf.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(name_bytes);
        buf.push(3);
        buf.extend_from_slice(&0u32.to_le_bytes());

        let err = DescriptorNode::decode(&buf).unwrap_err();
        assert_eq!(err, DecodeError::InvalidDiscriminantWidth { width: 3 });
    }

    #[test]
    fn test_unknown_constructor_tag_rejection() {
        let err0 = DescriptorNode::decode(&[0x00]).unwrap_err();
        assert_eq!(err0, DecodeError::UnknownConstructorTag { tag: 0x00 });

        let err15 = DescriptorNode::decode(&[0x15]).unwrap_err();
        assert_eq!(err15, DecodeError::UnknownConstructorTag { tag: 0x15 });

        let err_ff = DescriptorNode::decode(&[0xFF]).unwrap_err();
        assert_eq!(err_ff, DecodeError::UnknownConstructorTag { tag: 0xFF });
    }

    #[test]
    fn test_ordered_float_descriptor_roundtrip() {
        let mut buf = Vec::new();
        DescriptorNode::OrderedF32.encode(&mut buf);
        assert_eq!(buf, vec![0x13]);
        let (node32, consumed32) = DescriptorNode::decode(&buf).unwrap();
        assert_eq!(consumed32, 1);
        assert_eq!(node32, DescriptorNode::OrderedF32);

        let mut buf64 = Vec::new();
        DescriptorNode::OrderedF64.encode(&mut buf64);
        assert_eq!(buf64, vec![0x14]);
        let (node64, consumed64) = DescriptorNode::decode(&buf64).unwrap();
        assert_eq!(consumed64, 1);
        assert_eq!(node64, DescriptorNode::OrderedF64);
    }

    #[test]
    fn test_schema_descriptor_and_identity() {
        let desc1 = SchemaDescriptor::new(1, DescriptorNode::U32);
        let desc2 = SchemaDescriptor::new(2, DescriptorNode::U32);
        let desc3 = SchemaDescriptor::new(1, DescriptorNode::U64);

        assert_ne!(desc1.identity(), desc2.identity());
        assert_ne!(desc1.identity(), desc3.identity());
        assert_eq!(desc1.identity(), desc1.identity());

        let mut buf = Vec::new();
        desc1.encode(&mut buf);
        let (decoded, consumed) = SchemaDescriptor::decode(&buf).unwrap();
        assert_eq!(consumed, buf.len());
        assert_eq!(decoded, desc1);
        assert_eq!(decoded.identity(), desc1.identity());
    }

    #[test]
    fn test_recursion_depth_limit() {
        let mut buf = vec![0x0E; 70];
        buf.push(0x01);

        let err = DescriptorNode::decode(&buf).unwrap_err();
        assert_eq!(err.error_kind(), "CycleDetected");
    }

    #[test]
    fn test_derive_fiber_id() {
        let key1 = "account.us-east.98765";
        let fiber_id_1 = derive_fiber_id(key1);
        let fiber_id_1_again = derive_fiber_id(key1);
        assert_eq!(fiber_id_1, fiber_id_1_again);
        assert_ne!(fiber_id_1, [0u8; 16]);

        let key2 = "account.us-west.98765";
        let fiber_id_2 = derive_fiber_id(key2);
        assert_ne!(fiber_id_1, fiber_id_2);

        let expected_key = blake3::derive_key("pardosa.fiber_id.v1", key1.as_bytes());
        assert_eq!(fiber_id_1, expected_key[..16]);
    }
}
