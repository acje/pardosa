//! Schema descriptors, admitted S3 constructor AST, and derived schema identity.

use crate::encoding::{
    DecodeError, EncodeError, EventBytes, EventString, EventVec, NonEmptyEventString,
    OwnershipRecord, Timestamp, Uuid, ValueConstraint,
};
use crate::store::{FailureCondition, OperationFailure};

mod floats;
pub use floats::{EventF32, EventF64, OrderedF32, OrderedF64};

/// Maximum recursion depth allowed during descriptor AST decoding to prevent cycles.
pub const MAX_DESCRIPTOR_DEPTH: usize = 16;

/// Alias for maximum recursion depth allowed during descriptor AST operations.
pub const MAX_RECURSION_DEPTH: usize = MAX_DESCRIPTOR_DEPTH;

/// Authorized implementation bound: 64 KiB maximum encoded descriptor record size to prevent unbounded metadata allocations.
pub const MAX_DESCRIPTOR_RECORD_BYTES: usize = 65536;

#[cfg(test)]
thread_local! {
    pub(crate) static THREAD_ENCODE_CALL_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

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
    /// 0x0B: Bounded non-empty UTF-8 text (1 <= len <= `max_bytes`).
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
    ///
    /// # Errors
    /// Returns `EncodeError::DepthExceeded` if recursion depth exceeds `MAX_DESCRIPTOR_DEPTH` (16).
    /// Returns `EncodeError::Custom` if a discriminant exceeds its width or if wire length exceeds `u32::MAX`.
    pub fn encode(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        #[cfg(test)]
        THREAD_ENCODE_CALL_COUNT.with(|cell| cell.set(cell.get() + 1));
        self.encode_recursive(buf, 0)
    }

    /// Computes the exact encoded wire length using checked usize arithmetic.
    ///
    /// # Errors
    /// Returns `EncodeError::DepthExceeded` if recursion depth exceeds `MAX_RECURSION_DEPTH` (16).
    /// Returns `EncodeError::Custom` on component u32 overflow, aggregate usize overflow, or if a discriminant exceeds its width. Aggregate wire bounds (64 KiB) are enforced separately during descriptor admission.
    pub fn encoded_len(&self) -> Result<usize, EncodeError> {
        self.encoded_len_recursive(0)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive calculation across all descriptor node variants"
    )]
    fn encoded_len_recursive(&self, depth: usize) -> Result<usize, EncodeError> {
        if depth > MAX_RECURSION_DEPTH {
            return Err(EncodeError::DepthExceeded {
                depth,
                max: MAX_RECURSION_DEPTH,
            });
        }
        let tag_len = 1usize;
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
            | Self::OrderedF64 => Ok(tag_len),
            Self::EventString { .. }
            | Self::NonEmptyEventString { .. }
            | Self::EventBytes { .. } => tag_len
                .checked_add(4)
                .ok_or_else(|| EncodeError::Custom("length overflow".into())),
            Self::EventVec { inner, .. } => {
                let inner_len = inner.encoded_len_recursive(depth + 1)?;
                tag_len
                    .checked_add(inner_len)
                    .and_then(|l| l.checked_add(4))
                    .ok_or_else(|| EncodeError::Custom("length overflow".into()))
            }
            Self::Option { inner } => {
                let inner_len = inner.encoded_len_recursive(depth + 1)?;
                tag_len
                    .checked_add(inner_len)
                    .ok_or_else(|| EncodeError::Custom("length overflow".into()))
            }
            Self::Struct { name, fields } => {
                let name_bytes = name.as_bytes();
                let name_len = u32::try_from(name_bytes.len())
                    .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?
                    as usize;
                let mut total = tag_len
                    .checked_add(4)
                    .and_then(|l| l.checked_add(name_len))
                    .and_then(|l| l.checked_add(4))
                    .ok_or_else(|| EncodeError::Custom("length overflow".into()))?;
                let _ = u32::try_from(fields.len())
                    .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?;
                for f in fields {
                    let fname_bytes = f.name.as_bytes();
                    let fname_len = u32::try_from(fname_bytes.len())
                        .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?
                        as usize;
                    let fnode_len = f.node.encoded_len_recursive(depth + 1)?;
                    total = total
                        .checked_add(4)
                        .and_then(|l| l.checked_add(fname_len))
                        .and_then(|l| l.checked_add(fnode_len))
                        .ok_or_else(|| EncodeError::Custom("length overflow".into()))?;
                }
                Ok(total)
            }
            Self::Enum {
                name,
                discriminant_width,
                variants,
            } => {
                let name_bytes = name.as_bytes();
                let name_len = u32::try_from(name_bytes.len())
                    .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?
                    as usize;
                let mut total = tag_len
                    .checked_add(4)
                    .and_then(|l| l.checked_add(name_len))
                    .and_then(|l| l.checked_add(1))
                    .and_then(|l| l.checked_add(4))
                    .ok_or_else(|| EncodeError::Custom("length overflow".into()))?;
                let _ = u32::try_from(variants.len())
                    .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?;
                for v in variants {
                    let disc_len = if *discriminant_width == 1 {
                        if v.discriminant > 255 {
                            return Err(EncodeError::Custom(
                                "discriminant exceeds 1-byte width".to_string(),
                            ));
                        }
                        1usize
                    } else {
                        if v.discriminant > 65535 {
                            return Err(EncodeError::Custom(
                                "discriminant exceeds 2-byte width".to_string(),
                            ));
                        }
                        2usize
                    };
                    let vname_bytes = v.name.as_bytes();
                    let vname_len = u32::try_from(vname_bytes.len())
                        .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?
                        as usize;
                    let payload_len = match &v.payload {
                        Some(payload) => payload.encoded_len_recursive(depth + 1)?,
                        None => 1usize,
                    };
                    total = total
                        .checked_add(disc_len)
                        .and_then(|l| l.checked_add(4))
                        .and_then(|l| l.checked_add(vname_len))
                        .and_then(|l| l.checked_add(payload_len))
                        .ok_or_else(|| EncodeError::Custom("length overflow".into()))?;
                }
                Ok(total)
            }
        }
    }

    fn encode_recursive(&self, buf: &mut Vec<u8>, depth: usize) -> Result<(), EncodeError> {
        if depth > MAX_DESCRIPTOR_DEPTH {
            return Err(EncodeError::DepthExceeded {
                depth,
                max: MAX_DESCRIPTOR_DEPTH,
            });
        }
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
            | Self::OrderedF64 => Ok(()),
            Self::EventString { max_bytes }
            | Self::NonEmptyEventString { max_bytes }
            | Self::EventBytes { max_bytes } => {
                buf.extend_from_slice(&max_bytes.to_le_bytes());
                Ok(())
            }
            Self::EventVec { inner, max_items } => {
                inner.encode_recursive(buf, depth + 1)?;
                buf.extend_from_slice(&max_items.to_le_bytes());
                Ok(())
            }
            Self::Option { inner } => inner.encode_recursive(buf, depth + 1),
            Self::Struct { name, fields } => {
                let name_bytes = name.as_bytes();
                let name_len = u32::try_from(name_bytes.len())
                    .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?;
                buf.extend_from_slice(&name_len.to_le_bytes());
                buf.extend_from_slice(name_bytes);
                let fields_len = u32::try_from(fields.len())
                    .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?;
                buf.extend_from_slice(&fields_len.to_le_bytes());
                for f in fields {
                    let fname_bytes = f.name.as_bytes();
                    let fname_len = u32::try_from(fname_bytes.len())
                        .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?;
                    buf.extend_from_slice(&fname_len.to_le_bytes());
                    buf.extend_from_slice(fname_bytes);
                    f.node.encode_recursive(buf, depth + 1)?;
                }
                Ok(())
            }
            Self::Enum {
                name,
                discriminant_width,
                variants,
            } => {
                let name_bytes = name.as_bytes();
                let name_len = u32::try_from(name_bytes.len())
                    .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?;
                buf.extend_from_slice(&name_len.to_le_bytes());
                buf.extend_from_slice(name_bytes);
                buf.push(*discriminant_width);
                let variants_len = u32::try_from(variants.len())
                    .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?;
                buf.extend_from_slice(&variants_len.to_le_bytes());
                for v in variants {
                    if *discriminant_width == 1 {
                        let Ok(d8) = u8::try_from(v.discriminant) else {
                            return Err(EncodeError::Custom(
                                "discriminant exceeds 1-byte width".to_string(),
                            ));
                        };
                        buf.push(d8);
                    } else {
                        let Ok(d16) = u16::try_from(v.discriminant) else {
                            return Err(EncodeError::Custom(
                                "discriminant exceeds 2-byte width".to_string(),
                            ));
                        };
                        buf.extend_from_slice(&d16.to_le_bytes());
                    }
                    let vname_bytes = v.name.as_bytes();
                    let vname_len = u32::try_from(vname_bytes.len())
                        .map_err(|_| EncodeError::Custom("length exceeds u32::MAX".into()))?;
                    buf.extend_from_slice(&vname_len.to_le_bytes());
                    buf.extend_from_slice(vname_bytes);
                    match &v.payload {
                        Some(payload) => payload.encode_recursive(buf, depth + 1)?,
                        None => buf.push(0x00),
                    }
                }
                Ok(())
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

    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive deserializer across all descriptor node variants"
    )]
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
                let remaining = buf.len().saturating_sub(cursor);
                let min_bytes_per_field = 5;
                if remaining / min_bytes_per_field < field_count {
                    return Err(DecodeError::TruncatedPayload {
                        expected: cursor
                            .saturating_add(field_count.saturating_mul(min_bytes_per_field)),
                        available: buf.len(),
                    });
                }
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
                let remaining = buf.len().saturating_sub(cursor);
                let min_bytes_per_variant = (discriminant_width as usize).saturating_add(5);
                if remaining / min_bytes_per_variant < variant_count {
                    return Err(DecodeError::TruncatedPayload {
                        expected: cursor
                            .saturating_add(variant_count.saturating_mul(min_bytes_per_variant)),
                        available: buf.len(),
                    });
                }
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
                        let d = u32::from(buf[cursor]);
                        cursor += 1;
                        d
                    } else {
                        let d = u32::from(u16::from_le_bytes([buf[cursor], buf[cursor + 1]]));
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
    ///
    /// # Errors
    /// Returns `EncodeError::DepthExceeded` if recursion depth exceeds `MAX_DESCRIPTOR_DEPTH` (16).
    /// Returns `EncodeError::Custom` if a discriminant exceeds its width or if wire length exceeds `u32::MAX`.
    pub fn encode(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.extend_from_slice(&self.version.to_le_bytes());
        self.root.encode(buf)
    }

    /// Computes the exact encoded wire length using checked usize arithmetic.
    ///
    /// # Errors
    /// Returns `EncodeError::DepthExceeded` if recursion depth exceeds `MAX_RECURSION_DEPTH` (16).
    /// Returns `EncodeError::Custom` on component u32 overflow, aggregate usize overflow, or if a discriminant exceeds its width. Aggregate wire bounds (64 KiB) are enforced separately during descriptor admission.
    pub fn encoded_record_len(&self) -> Result<usize, EncodeError> {
        let root_len = self.root.encoded_len()?;
        1usize
            .checked_add(4)
            .and_then(|prefix| prefix.checked_add(root_len))
            .ok_or_else(|| EncodeError::Custom("length overflow".into()))
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

    /// Derives the schema identity for this descriptor fallibly.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if the descriptor violates structural completeness,
    /// has version 0, depth > 16, invalid or duplicate discriminants, invalid widths,
    /// or if encoded descriptor record exceeds 64 KiB.
    pub fn try_identity(&self) -> Result<SchemaIdentity, OperationFailure> {
        let admitted = AdmittedDescriptor::try_from_descriptor(self.clone())?;
        Ok(SchemaIdentity::from_descriptor(&admitted))
    }

    /// Derives the schema identity for this descriptor.
    ///
    /// # Panics
    /// Panics if the schema descriptor violates admission validation. Use [`try_identity`](Self::try_identity)
    /// for fallible derivation.
    #[must_use]
    pub fn identity(&self) -> SchemaIdentity {
        self.try_identity()
            .expect("SchemaDescriptor must be valid to compute identity; use try_identity for fallible derivation")
    }

    /// Asserts that this schema descriptor is structurally complete per C8.2.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::MissingSchemaDescriptor`]
    /// if version is zero.
    /// Returns [`OperationFailure`] with [`FailureCondition::ValueConstraintViolated`]
    /// if any variant, field, or bound is invalid, or if recursion depth exceeds 16.
    pub fn validate_structural_completeness(&self) -> Result<(), OperationFailure> {
        if self.version == 0 {
            return Err(OperationFailure::new(
                FailureCondition::MissingSchemaDescriptor,
                "schema descriptor declared version must be non-zero per C8.2",
            ));
        }
        self.root.validate_structural_completeness()
    }

    /// Asserts that this schema descriptor is structurally valid and within depth limits.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if validation fails.
    pub fn validate_structure(&self) -> Result<(), OperationFailure> {
        self.validate_structural_completeness()
    }

    /// Asserts structural completeness and converts this descriptor into an [`AdmittedDescriptor`].
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if validation fails.
    pub fn into_admitted(self) -> Result<AdmittedDescriptor, OperationFailure> {
        AdmittedDescriptor::try_from_descriptor(self)
    }
}

/// A validated schema descriptor guaranteed to be structurally complete and admitted per C8.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedDescriptor {
    descriptor: SchemaDescriptor,
    encoded_record: Vec<u8>,
    identity: SchemaIdentity,
}

impl AdmittedDescriptor {
    /// Validates and admits a schema descriptor.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if the schema descriptor violates structural completeness,
    /// has version 0, depth > 16, invalid or duplicate discriminants, invalid widths,
    /// encoding wire length overflow, or if the encoded descriptor record exceeds
    /// [`MAX_DESCRIPTOR_RECORD_BYTES`] (64 KiB per authorized 64 KiB implementation resource bound).
    pub fn try_from_descriptor(descriptor: SchemaDescriptor) -> Result<Self, OperationFailure> {
        descriptor.validate_structural_completeness()?;
        let record_len = descriptor.encoded_record_len().map_err(|err| {
            OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                format!("schema descriptor encoded length calculation failed: {err}"),
            )
        })?;
        if record_len > MAX_DESCRIPTOR_RECORD_BYTES {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                format!("encoded descriptor record exceeds 64 KiB: {record_len}"),
            ));
        }
        let mut desc_bytes = Vec::with_capacity(record_len);
        descriptor.root.encode(&mut desc_bytes).map_err(|err| {
            OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                format!("schema descriptor encoding failed: {err}"),
            )
        })?;
        let desc_record = OwnershipRecord::SchemaDescriptor {
            schema_version: descriptor.version,
            descriptor_bytes: desc_bytes,
        };
        let mut encoded_record = Vec::with_capacity(record_len);
        desc_record.encode(&mut encoded_record);
        let identity = SchemaIdentity::from_raw(*blake3::hash(&encoded_record[1..]).as_bytes());
        Ok(Self {
            descriptor,
            encoded_record,
            identity,
        })
    }

    /// Returns the declared schema version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.descriptor.version
    }

    /// Returns the root descriptor AST node.
    #[must_use]
    pub fn root(&self) -> &DescriptorNode {
        &self.descriptor.root
    }

    /// Returns the inner [`SchemaDescriptor`].
    #[must_use]
    pub fn into_inner(self) -> SchemaDescriptor {
        self.descriptor
    }

    /// Returns a reference to the inner [`SchemaDescriptor`].
    #[must_use]
    pub fn descriptor(&self) -> &SchemaDescriptor {
        &self.descriptor
    }

    /// Derives the schema identity.
    #[must_use]
    pub fn identity(&self) -> SchemaIdentity {
        self.identity
    }

    /// Returns the canonical encoded [`OwnershipRecord::SchemaDescriptor`] wire bytes.
    #[must_use]
    pub fn encoded_record(&self) -> &[u8] {
        &self.encoded_record
    }

    /// Creates a default valid descriptor for testing.
    ///
    /// # Panics
    /// Panics if the default test descriptor is invalid.
    #[cfg(any(test, feature = "unstable-test-support"))]
    #[must_use]
    pub fn default_for_test() -> Self {
        Self::try_from_descriptor(SchemaDescriptor::new(1, DescriptorNode::U64))
            .expect("default_for_test descriptor must be valid")
    }
}

impl TryFrom<SchemaDescriptor> for AdmittedDescriptor {
    type Error = OperationFailure;

    fn try_from(descriptor: SchemaDescriptor) -> Result<Self, Self::Error> {
        Self::try_from_descriptor(descriptor)
    }
}

impl std::ops::Deref for AdmittedDescriptor {
    type Target = SchemaDescriptor;

    fn deref(&self) -> &Self::Target {
        &self.descriptor
    }
}

impl AsRef<SchemaDescriptor> for AdmittedDescriptor {
    fn as_ref(&self) -> &SchemaDescriptor {
        &self.descriptor
    }
}

impl DescriptorNode {
    /// Asserts that this descriptor AST node is structurally valid and within depth limits.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if validation fails.
    pub fn validate_structure(&self) -> Result<(), OperationFailure> {
        self.validate_structural_completeness()
    }

    /// Asserts that this descriptor AST node is structurally complete per C8.2.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::ValueConstraintViolated`]
    /// if any variant, field, or bound is invalid, or if recursion depth exceeds 16.
    pub fn validate_structural_completeness(&self) -> Result<(), OperationFailure> {
        self.validate_structural_completeness_recursive(0)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive structural validation across all descriptor node variants"
    )]
    fn validate_structural_completeness_recursive(
        &self,
        depth: usize,
    ) -> Result<(), OperationFailure> {
        if depth > MAX_DESCRIPTOR_DEPTH {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "descriptor nesting depth exceeds implementation depth limit of 16 (structural completeness per C8.2)",
            ));
        }
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
                    inner.validate_structural_completeness_recursive(depth + 1)
                }
            }
            Self::Option { inner } => inner.validate_structural_completeness_recursive(depth + 1),
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
                    f.node
                        .validate_structural_completeness_recursive(depth + 1)?;
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
                let mut seen_discriminants = std::collections::HashSet::new();
                for v in variants {
                    if v.name.is_empty() {
                        return Err(OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::Empty,
                            },
                            "enum variant name must be non-empty per C8.2",
                        ));
                    }
                    if *discriminant_width == 1 && v.discriminant > 255 {
                        return Err(OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::TooLong,
                            },
                            "variant discriminant exceeds 1-byte discriminant width per M4",
                        ));
                    }
                    if *discriminant_width == 2 && v.discriminant > 65535 {
                        return Err(OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::TooLong,
                            },
                            "variant discriminant exceeds 2-byte discriminant width per M4",
                        ));
                    }
                    if !seen_discriminants.insert(v.discriminant) {
                        return Err(OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::NotReal,
                            },
                            "enumeration contains duplicate variant discriminant per M4",
                        ));
                    }
                    if let Some(payload) = &v.payload {
                        payload.validate_structural_completeness_recursive(depth + 1)?;
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

    /// Computes schema identity from schema version and descriptor AST root, propagating encoding errors.
    ///
    /// # Errors
    /// Returns [`EncodeError`] if descriptor encoding fails (e.g. recursion depth exceeds 16, invalid discriminant width, or wire length overflow).
    pub fn try_from_descriptor(version: u32, root: &DescriptorNode) -> Result<Self, EncodeError> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&version.to_le_bytes());
        root.encode(&mut buf)?;
        let hash = blake3::hash(&buf);
        Ok(Self(*hash.as_bytes()))
    }

    /// Computes schema identity from an admitted schema descriptor.
    #[must_use]
    pub fn from_descriptor(descriptor: &AdmittedDescriptor) -> Self {
        descriptor.identity()
    }

    /// Returns reference to 32-byte hash digest.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns hex-encoded representation of the identity digest.
    #[must_use]
    pub fn to_hex(&self) -> String {
        const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(64);
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
    /// Declared schema version constant.
    const SCHEMA_VERSION: u32;

    /// Returns root schema descriptor AST node.
    fn schema_descriptor() -> DescriptorNode;

    /// Derives schema identity.
    #[must_use]
    fn schema_identity() -> SchemaIdentity {
        let desc = SchemaDescriptor::new(Self::SCHEMA_VERSION, Self::schema_descriptor());
        let admitted = AdmittedDescriptor::try_from_descriptor(desc)
            .expect("PardosaSchema must produce a valid schema descriptor");
        SchemaIdentity::from_descriptor(&admitted)
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
    /// Maximum nesting depth of this type in schema descriptor AST.
    const TYPE_DEPTH: usize;

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
    const TYPE_DEPTH: usize = 0;
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
    const TYPE_DEPTH: usize = 0;
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
    const TYPE_DEPTH: usize = 0;
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
    const TYPE_DEPTH: usize = 0;
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
    const TYPE_DEPTH: usize = 0;
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::I8
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.push((*self).cast_unsigned());
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::TruncatedPayload {
                expected: 1,
                available: 0,
            });
        }
        Ok((buf[0].cast_signed(), 1))
    }
}

impl PardosaType for i16 {
    const TYPE_DEPTH: usize = 0;
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
    const TYPE_DEPTH: usize = 0;
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
    const TYPE_DEPTH: usize = 0;
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
    const TYPE_DEPTH: usize = 0;
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::Bool
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.push(u8::from(*self));
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
    const TYPE_DEPTH: usize = 0;
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
    const TYPE_DEPTH: usize = 0;
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
    const TYPE_DEPTH: usize = T::TYPE_DEPTH + 1;
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
                let total = crate::encoding::checked_advance(1, consumed, buf.len())?;
                Ok((Some(val), total))
            }
            other => Err(DecodeError::InvalidOptionTag { tag: other }),
        }
    }
}

impl<const MAX: usize> PardosaType for EventString<MAX> {
    const TYPE_DEPTH: usize = 0;
    fn descriptor_node() -> DescriptorNode {
        let max_bytes = u32::try_from(MAX).expect("const MAX exceeds u32::MAX wire limit");
        DescriptorNode::EventString { max_bytes }
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
    const TYPE_DEPTH: usize = 0;
    fn descriptor_node() -> DescriptorNode {
        let max_bytes = u32::try_from(MAX).expect("const MAX exceeds u32::MAX wire limit");
        DescriptorNode::NonEmptyEventString { max_bytes }
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
    const TYPE_DEPTH: usize = 0;
    fn descriptor_node() -> DescriptorNode {
        let max_bytes = u32::try_from(MAX).expect("const MAX exceeds u32::MAX wire limit");
        DescriptorNode::EventBytes { max_bytes }
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
    const TYPE_DEPTH: usize = T::TYPE_DEPTH + 1;
    fn descriptor_node() -> DescriptorNode {
        let max_items = u32::try_from(MAX).expect("const MAX exceeds u32::MAX wire limit");
        DescriptorNode::EventVec {
            inner: Box::new(T::descriptor_node()),
            max_items,
        }
    }
    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        let count = self.as_slice().len();
        if count > MAX {
            return Err(EncodeError::ItemCountExceeded { count, max: MAX });
        }
        let count_u32 =
            u32::try_from(count).map_err(|_| EncodeError::ItemCountExceeded { count, max: MAX })?;
        buf.extend_from_slice(&count_u32.to_le_bytes());
        for item in self.as_slice() {
            item.encode_type(buf)?;
        }
        Ok(())
    }
    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if MAX > u32::MAX as usize {
            return Err(DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            });
        }
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
        if count == 0 {
            return Ok((Self::new(Vec::new())?, 4));
        }
        let remaining_wire = buf.len() - 4;
        let initial_capacity = remaining_wire.min(count);
        let mut cursor = 4;
        let mut items = Vec::with_capacity(initial_capacity);
        for _ in 0..count {
            let (item, consumed) = T::decode_type(&buf[cursor..])?;
            let next_cursor =
                cursor
                    .checked_add(consumed)
                    .ok_or(DecodeError::TruncatedPayload {
                        expected: usize::MAX,
                        available: buf.len(),
                    })?;
            if next_cursor > buf.len() {
                return Err(DecodeError::TruncatedPayload {
                    expected: next_cursor,
                    available: buf.len(),
                });
            }
            cursor = next_cursor;
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
            node.encode(&mut buf).unwrap();
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
        node.encode(&mut buf).unwrap();
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
        node.encode(&mut buf).unwrap();
        let (decoded, consumed) = DescriptorNode::decode(&buf).expect("decode enum");
        assert_eq!(consumed, buf.len());
        assert_eq!(decoded, node);
    }

    #[test]
    fn test_invalid_discriminant_width() {
        let mut buf = vec![0x10];
        let name_bytes = b"Test";
        buf.extend_from_slice(&4u32.to_le_bytes());
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
        DescriptorNode::OrderedF32.encode(&mut buf).unwrap();
        assert_eq!(buf, vec![0x13]);
        let (node32, consumed32) = DescriptorNode::decode(&buf).unwrap();
        assert_eq!(consumed32, 1);
        assert_eq!(node32, DescriptorNode::OrderedF32);

        let mut buf64 = Vec::new();
        DescriptorNode::OrderedF64.encode(&mut buf64).unwrap();
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
        desc1.encode(&mut buf).unwrap();
        let (decoded, consumed) = SchemaDescriptor::decode(&buf).unwrap();
        assert_eq!(consumed, buf.len());
        assert_eq!(decoded, desc1);
        assert_eq!(decoded.identity(), desc1.identity());
    }

    #[test]
    fn test_depth_limit_boundary_16_passes_17_fails() {
        let mut node_16 = DescriptorNode::U32;
        for _ in 0..16 {
            node_16 = DescriptorNode::Option {
                inner: Box::new(node_16),
            };
        }
        assert!(node_16.validate_structure().is_ok());
        let mut buf_16 = Vec::new();
        assert!(node_16.encode(&mut buf_16).is_ok());

        let mut node_17 = DescriptorNode::U32;
        for _ in 0..17 {
            node_17 = DescriptorNode::Option {
                inner: Box::new(node_17),
            };
        }
        let err_validate = node_17.validate_structure().unwrap_err();
        assert_eq!(
            *err_validate.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
        let mut buf_17 = Vec::new();
        let err_encode = node_17.encode(&mut buf_17).unwrap_err();
        assert_eq!(
            err_encode,
            EncodeError::DepthExceeded { depth: 17, max: 16 }
        );
    }

    #[test]
    fn test_recursion_depth_limit() {
        let mut buf = vec![0x0E; 20];
        buf.push(0x01);

        let err = DescriptorNode::decode(&buf).unwrap_err();
        assert_eq!(err.error_kind(), "CycleDetected");
    }

    #[test]
    fn test_encode_and_validate_structure_reject_depth_exceeding_16() {
        let mut deep_node = DescriptorNode::U32;
        for _ in 0..17 {
            deep_node = DescriptorNode::Option {
                inner: Box::new(deep_node),
            };
        }
        let mut buf = Vec::new();
        let err_encode = deep_node.encode(&mut buf).unwrap_err();
        assert_eq!(
            err_encode,
            EncodeError::DepthExceeded { depth: 17, max: 16 }
        );

        let err_validate = deep_node.validate_structure().unwrap_err();
        assert_eq!(
            *err_validate.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );

        let desc = SchemaDescriptor::new(1, deep_node);
        let err_desc_validate = desc.validate_structure().unwrap_err();
        assert_eq!(
            *err_desc_validate.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );

        let mut desc_buf = Vec::new();
        assert!(desc.encode(&mut desc_buf).is_err());
    }

    #[test]
    fn test_validate_structure_discriminant_bounds_and_duplicates() {
        let width1_overflow = DescriptorNode::Enum {
            name: "Overflow".to_string(),
            discriminant_width: 1,
            variants: vec![VariantDescriptor {
                discriminant: 256,
                name: "Bad".to_string(),
                payload: None,
            }],
        };
        assert!(width1_overflow.validate_structure().is_err());

        let duplicate = DescriptorNode::Enum {
            name: "Duplicate".to_string(),
            discriminant_width: 1,
            variants: vec![
                VariantDescriptor {
                    discriminant: 1,
                    name: "A".to_string(),
                    payload: None,
                },
                VariantDescriptor {
                    discriminant: 1,
                    name: "B".to_string(),
                    payload: None,
                },
            ],
        };
        assert!(duplicate.validate_structure().is_err());
    }

    #[test]
    fn test_allocation_bomb_rejection() {
        let mut struct_bomb = vec![0x0F];
        struct_bomb.extend_from_slice(&0u32.to_le_bytes());
        struct_bomb.extend_from_slice(&u32::MAX.to_le_bytes());
        let err_struct = DescriptorNode::decode(&struct_bomb).unwrap_err();
        assert_eq!(err_struct.error_kind(), "TruncatedPayload");

        let mut enum_bomb = vec![0x10];
        enum_bomb.extend_from_slice(&0u32.to_le_bytes());
        enum_bomb.push(1);
        enum_bomb.extend_from_slice(&u32::MAX.to_le_bytes());
        let err_enum = DescriptorNode::decode(&enum_bomb).unwrap_err();
        assert_eq!(err_enum.error_kind(), "TruncatedPayload");
    }

    #[test]
    fn test_event_vec_decode_allocation_bomb_rejection() {
        let buf = [255, 255, 255, 255];
        let err = EventVec::<u64, 4_294_967_295>::decode_type(&buf).unwrap_err();
        assert!(matches!(
            err,
            DecodeError::TruncatedPayload {
                expected: 8,
                available: 0
            }
        ));
    }

    #[test]
    fn test_event_vec_decode_zero_wire_loop_bounds() {
        #[derive(Debug, Clone, PartialEq, Eq)]
        struct ZeroWire;

        impl PardosaType for ZeroWire {
            const TYPE_DEPTH: usize = 0;
            fn descriptor_node() -> DescriptorNode {
                DescriptorNode::Struct {
                    name: "ZeroWire".to_string(),
                    fields: vec![],
                }
            }
            fn encode_type(&self, _buf: &mut Vec<u8>) -> Result<(), EncodeError> {
                Ok(())
            }
            fn decode_type(_buf: &[u8]) -> Result<(Self, usize), DecodeError> {
                Ok((ZeroWire, 0))
            }
        }

        let empty_vec = EventVec::<ZeroWire, 10>::new(vec![]).unwrap();
        let mut buf = Vec::new();
        empty_vec.encode_type(&mut buf).unwrap();
        assert_eq!(buf, vec![0x00, 0x00, 0x00, 0x00]);
        let (decoded_empty, consumed_empty) = EventVec::<ZeroWire, 10>::decode_type(&buf).unwrap();
        assert_eq!(consumed_empty, 4);
        assert_eq!(decoded_empty, empty_vec);

        let non_empty = EventVec::<ZeroWire, 10>::new(vec![ZeroWire, ZeroWire, ZeroWire]).unwrap();
        buf.clear();
        non_empty.encode_type(&mut buf).unwrap();
        assert_eq!(buf, vec![0x03, 0x00, 0x00, 0x00]);
        let (decoded_non_empty, consumed_non_empty) =
            EventVec::<ZeroWire, 10>::decode_type(&buf).unwrap();
        assert_eq!(consumed_non_empty, 4);
        assert_eq!(decoded_non_empty, non_empty);
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

    #[test]
    fn test_schema_identity_try_from_descriptor_depth_limit() {
        let mut deep_node = DescriptorNode::U32;
        for _ in 0..17 {
            deep_node = DescriptorNode::Option {
                inner: Box::new(deep_node),
            };
        }
        let err = SchemaIdentity::try_from_descriptor(1, &deep_node).unwrap_err();
        assert_eq!(err, EncodeError::DepthExceeded { depth: 17, max: 16 });

        let valid_node = DescriptorNode::U64;
        let identity = SchemaIdentity::try_from_descriptor(1, &valid_node).unwrap();
        let admitted =
            AdmittedDescriptor::try_from_descriptor(SchemaDescriptor::new(1, valid_node)).unwrap();
        assert_eq!(identity, SchemaIdentity::from_descriptor(&admitted));
    }

    #[test]
    fn test_schema_identity_eliminates_panics_on_depth_exceeded() {
        let mut deep_node = DescriptorNode::U32;
        for _ in 0..17 {
            deep_node = DescriptorNode::Option {
                inner: Box::new(deep_node),
            };
        }
        let deep_desc = SchemaDescriptor::new(1, deep_node);
        let err = AdmittedDescriptor::try_from_descriptor(deep_desc).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong
            }
        );
    }

    #[test]
    fn test_admitted_descriptor_validation() {
        let valid_desc = SchemaDescriptor::new(1, DescriptorNode::U64);
        let admitted = AdmittedDescriptor::try_from_descriptor(valid_desc.clone()).unwrap();
        assert_eq!(admitted.version(), 1);
        assert_eq!(admitted.root(), &DescriptorNode::U64);
        assert_eq!(admitted.descriptor(), &valid_desc);
        assert_eq!(admitted.identity(), valid_desc.identity());

        let zero_version = SchemaDescriptor::new(0, DescriptorNode::U64);
        let err_zero = AdmittedDescriptor::try_from_descriptor(zero_version).unwrap_err();
        assert_eq!(
            *err_zero.condition(),
            FailureCondition::MissingSchemaDescriptor
        );

        let mut deep_node = DescriptorNode::U32;
        for _ in 0..65 {
            deep_node = DescriptorNode::Option {
                inner: Box::new(deep_node),
            };
        }
        let deep_desc = SchemaDescriptor::new(1, deep_node);
        let err_deep = AdmittedDescriptor::try_from_descriptor(deep_desc).unwrap_err();
        assert!(matches!(
            err_deep.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong
            }
        ));

        let dup_enum = SchemaDescriptor::new(
            1,
            DescriptorNode::Enum {
                name: "Dup".to_string(),
                discriminant_width: 1,
                variants: vec![
                    VariantDescriptor {
                        discriminant: 1,
                        name: "A".to_string(),
                        payload: None,
                    },
                    VariantDescriptor {
                        discriminant: 1,
                        name: "B".to_string(),
                        payload: None,
                    },
                ],
            },
        );
        let err_dup = AdmittedDescriptor::try_from_descriptor(dup_enum).unwrap_err();
        assert!(matches!(
            err_dup.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal
            }
        ));

        let bad_width_enum = SchemaDescriptor::new(
            1,
            DescriptorNode::Enum {
                name: "BadWidth".to_string(),
                discriminant_width: 3,
                variants: vec![VariantDescriptor {
                    discriminant: 1,
                    name: "A".to_string(),
                    payload: None,
                }],
            },
        );
        let err_width = AdmittedDescriptor::try_from_descriptor(bad_width_enum).unwrap_err();
        assert!(matches!(
            err_width.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal
            }
        ));
    }

    #[test]
    fn test_schema_descriptor_try_identity() {
        let valid = SchemaDescriptor::new(1, DescriptorNode::U64);
        let identity = valid.try_identity().unwrap();
        assert_eq!(identity, valid.identity());

        let invalid_zero = SchemaDescriptor::new(0, DescriptorNode::U64);
        let err_zero = invalid_zero.try_identity().unwrap_err();
        assert_eq!(
            *err_zero.condition(),
            FailureCondition::MissingSchemaDescriptor
        );

        let mut deep = DescriptorNode::U32;
        for _ in 0..17 {
            deep = DescriptorNode::Option {
                inner: Box::new(deep),
            };
        }
        let invalid_deep = SchemaDescriptor::new(1, deep);
        let err_deep = invalid_deep.try_identity().unwrap_err();
        assert!(matches!(
            err_deep.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong
            }
        ));
    }

    #[test]
    fn test_descriptor_exact_wire_boundary_struct_admission() {
        let exact_field_len = 65516;
        let desc_exact = SchemaDescriptor::new(
            1,
            DescriptorNode::Struct {
                name: "S".to_string(),
                fields: vec![FieldDescriptor {
                    name: "f".repeat(exact_field_len),
                    node: DescriptorNode::U8,
                }],
            },
        );
        let admitted = AdmittedDescriptor::try_from_descriptor(desc_exact)
            .expect("exact 65,536 bytes must be admitted");
        assert_eq!(admitted.encoded_record().len(), 65536);

        let over_field_len = 65517;
        let desc_over = SchemaDescriptor::new(
            1,
            DescriptorNode::Struct {
                name: "S".to_string(),
                fields: vec![FieldDescriptor {
                    name: "f".repeat(over_field_len),
                    node: DescriptorNode::U8,
                }],
            },
        );
        let err = AdmittedDescriptor::try_from_descriptor(desc_over)
            .expect_err("65,537 bytes must be rejected");
        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );

        let huge_name_desc = SchemaDescriptor::new(
            1,
            DescriptorNode::Struct {
                name: "S".repeat(MAX_DESCRIPTOR_RECORD_BYTES + 1),
                fields: vec![FieldDescriptor {
                    name: "f".to_string(),
                    node: DescriptorNode::U8,
                }],
            },
        );
        let err_huge = AdmittedDescriptor::try_from_descriptor(huge_name_desc)
            .expect_err("AST string exceeding bound must be rejected before allocation");
        assert_eq!(
            *err_huge.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
    }

    #[test]
    fn test_descriptor_aggregate_oversized_fields_rejected_before_allocation() {
        let field_name_1 = "a".repeat(33_000);
        let field_name_2 = "b".repeat(33_000);
        let desc = SchemaDescriptor::new(
            1,
            DescriptorNode::Struct {
                name: "S".to_string(),
                fields: vec![
                    FieldDescriptor {
                        name: field_name_1,
                        node: DescriptorNode::U8,
                    },
                    FieldDescriptor {
                        name: field_name_2,
                        node: DescriptorNode::U8,
                    },
                ],
            },
        );
        let expected_len = desc
            .encoded_record_len()
            .expect("encoded_record_len must calculate length without allocating");
        assert_eq!(expected_len, 66025);
        assert!(expected_len > MAX_DESCRIPTOR_RECORD_BYTES);

        let count_before = THREAD_ENCODE_CALL_COUNT.with(std::cell::Cell::get);
        let err = AdmittedDescriptor::try_from_descriptor(desc).expect_err(
            "aggregate fields exceeding 64 KiB must be rejected with TooLong before allocation",
        );
        let count_after = THREAD_ENCODE_CALL_COUNT.with(std::cell::Cell::get);
        assert_eq!(
            count_before, count_after,
            "root.encode must not be executed when aggregate descriptor exceeds 64 KiB"
        );

        assert_eq!(
            *err.condition(),
            FailureCondition::ValueConstraintViolated {
                constraint: ValueConstraint::TooLong,
            }
        );
        assert_eq!(
            err.diagnostic_detail().message(),
            format!("encoded descriptor record exceeds 64 KiB: {expected_len}")
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "table-driven exhaustive parity check over all descriptor node shapes"
    )]
    fn test_descriptor_node_encoded_len_parity() {
        let shapes = vec![
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
            DescriptorNode::EventString { max_bytes: 100 },
            DescriptorNode::NonEmptyEventString { max_bytes: 50 },
            DescriptorNode::EventBytes { max_bytes: 256 },
            DescriptorNode::Option {
                inner: Box::new(DescriptorNode::Uuid),
            },
            DescriptorNode::Option {
                inner: Box::new(DescriptorNode::EventString { max_bytes: 64 }),
            },
            DescriptorNode::EventVec {
                inner: Box::new(DescriptorNode::U32),
                max_items: 10,
            },
            DescriptorNode::EventVec {
                inner: Box::new(DescriptorNode::Option {
                    inner: Box::new(DescriptorNode::I16),
                }),
                max_items: 5,
            },
            DescriptorNode::Struct {
                name: "Empty".to_string(),
                fields: vec![],
            },
            DescriptorNode::Struct {
                name: "Person".to_string(),
                fields: vec![
                    FieldDescriptor {
                        name: "age".to_string(),
                        node: DescriptorNode::U8,
                    },
                    FieldDescriptor {
                        name: "name".to_string(),
                        node: DescriptorNode::EventString { max_bytes: 32 },
                    },
                ],
            },
            DescriptorNode::Enum {
                name: "State".to_string(),
                discriminant_width: 1,
                variants: vec![
                    VariantDescriptor {
                        discriminant: 0,
                        name: "Off".to_string(),
                        payload: None,
                    },
                    VariantDescriptor {
                        discriminant: 1,
                        name: "On".to_string(),
                        payload: None,
                    },
                ],
            },
            DescriptorNode::Enum {
                name: "Action".to_string(),
                discriminant_width: 1,
                variants: vec![
                    VariantDescriptor {
                        discriminant: 0,
                        name: "Stop".to_string(),
                        payload: None,
                    },
                    VariantDescriptor {
                        discriminant: 1,
                        name: "Move".to_string(),
                        payload: Some(DescriptorNode::U32),
                    },
                ],
            },
            DescriptorNode::Enum {
                name: "Code".to_string(),
                discriminant_width: 2,
                variants: vec![
                    VariantDescriptor {
                        discriminant: 256,
                        name: "High".to_string(),
                        payload: Some(DescriptorNode::Uuid),
                    },
                    VariantDescriptor {
                        discriminant: 65535,
                        name: "Max".to_string(),
                        payload: None,
                    },
                ],
            },
            DescriptorNode::Struct {
                name: "Complex".to_string(),
                fields: vec![
                    FieldDescriptor {
                        name: "id".to_string(),
                        node: DescriptorNode::Uuid,
                    },
                    FieldDescriptor {
                        name: "items".to_string(),
                        node: DescriptorNode::EventVec {
                            inner: Box::new(DescriptorNode::Option {
                                inner: Box::new(DescriptorNode::Enum {
                                    name: "Sub".to_string(),
                                    discriminant_width: 1,
                                    variants: vec![VariantDescriptor {
                                        discriminant: 42,
                                        name: "V".to_string(),
                                        payload: None,
                                    }],
                                }),
                            }),
                            max_items: 100,
                        },
                    },
                ],
            },
        ];

        for (idx, node) in shapes.iter().enumerate() {
            let mut actual_encoded_bytes = Vec::new();
            node.encode(&mut actual_encoded_bytes)
                .unwrap_or_else(|err| panic!("encoding shape {idx} must succeed: {err}"));
            let predicted_len = node
                .encoded_len()
                .unwrap_or_else(|err| panic!("sizing shape {idx} must succeed: {err}"));
            assert_eq!(
                predicted_len,
                actual_encoded_bytes.len(),
                "node.encoded_len() must exactly match actual encoded byte length for shape {idx}"
            );

            let desc = SchemaDescriptor::new(1, node.clone());
            let mut actual_record_bytes = Vec::new();
            OwnershipRecord::SchemaDescriptor {
                schema_version: 1,
                descriptor_bytes: actual_encoded_bytes,
            }
            .encode(&mut actual_record_bytes);
            let predicted_record_len = desc
                .encoded_record_len()
                .unwrap_or_else(|err| panic!("sizing record for shape {idx} must succeed: {err}"));
            assert_eq!(
                predicted_record_len,
                actual_record_bytes.len(),
                "desc.encoded_record_len() must exactly match actual encoded record byte length for shape {idx}"
            );
        }
    }
}
