//! Deterministic IEEE-754 floating-point wrappers and total classification enums.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use crate::encoding::{DecodeError, EncodeError, ValueConstraint};
use crate::schema::{DescriptorNode, PardosaType, VariantDescriptor};

/// Deterministic 32-bit floating-point wrapper enforcing total ordering and canonical representation.
#[derive(Debug, Clone, Copy)]
pub struct OrderedF32 {
    inner: f32,
}

impl OrderedF32 {
    /// Attempts to construct an `OrderedF32` from an IEEE-754 primitive `f32`.
    ///
    /// # Errors
    /// Returns [`DecodeError::ValueConstraintViolated`] with [`ValueConstraint::NotReal`]
    /// if `v` is NaN, infinite, or subnormal.
    pub fn try_from(v: f32) -> Result<Self, DecodeError> {
        if v.is_nan() || v.is_infinite() || v.is_subnormal() {
            return Err(DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            });
        }
        let normalized = if v == 0.0 { 0.0 } else { v };
        Ok(Self { inner: normalized })
    }

    /// Returns the underlying primitive `f32` value.
    #[must_use]
    pub fn get(self) -> f32 {
        self.inner
    }
}

impl PartialEq for OrderedF32 {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for OrderedF32 {}

impl PartialOrd for OrderedF32 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedF32 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.inner.total_cmp(&other.inner)
    }
}

impl Hash for OrderedF32 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.to_bits().hash(state);
    }
}

impl TryFrom<f32> for OrderedF32 {
    type Error = DecodeError;

    fn try_from(v: f32) -> Result<Self, Self::Error> {
        Self::try_from(v)
    }
}

impl From<OrderedF32> for f32 {
    fn from(v: OrderedF32) -> Self {
        v.get()
    }
}

impl PardosaType for OrderedF32 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::OrderedF32
    }

    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.extend_from_slice(&self.inner.to_bits().to_le_bytes());
        Ok(())
    }

    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::TruncatedPayload {
                expected: 4,
                available: buf.len(),
            });
        }
        let raw_bits = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let val = f32::from_bits(raw_bits);
        let ordered = Self::try_from(val)?;
        Ok((ordered, 4))
    }
}

/// Deterministic 64-bit floating-point wrapper enforcing total ordering and canonical representation.
#[derive(Debug, Clone, Copy)]
pub struct OrderedF64 {
    inner: f64,
}

impl OrderedF64 {
    /// Attempts to construct an `OrderedF64` from an IEEE-754 primitive `f64`.
    ///
    /// # Errors
    /// Returns [`DecodeError::ValueConstraintViolated`] with [`ValueConstraint::NotReal`]
    /// if `v` is NaN, infinite, or subnormal.
    pub fn try_from(v: f64) -> Result<Self, DecodeError> {
        if v.is_nan() || v.is_infinite() || v.is_subnormal() {
            return Err(DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            });
        }
        let normalized = if v == 0.0 { 0.0 } else { v };
        Ok(Self { inner: normalized })
    }

    /// Returns the underlying primitive `f64` value.
    #[must_use]
    pub fn get(self) -> f64 {
        self.inner
    }
}

impl PartialEq for OrderedF64 {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for OrderedF64 {}

impl PartialOrd for OrderedF64 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedF64 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.inner.total_cmp(&other.inner)
    }
}

impl Hash for OrderedF64 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.to_bits().hash(state);
    }
}

impl TryFrom<f64> for OrderedF64 {
    type Error = DecodeError;

    fn try_from(v: f64) -> Result<Self, Self::Error> {
        Self::try_from(v)
    }
}

impl From<OrderedF64> for f64 {
    fn from(v: OrderedF64) -> Self {
        v.get()
    }
}

impl PardosaType for OrderedF64 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::OrderedF64
    }

    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        buf.extend_from_slice(&self.inner.to_bits().to_le_bytes());
        Ok(())
    }

    fn decode_type(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 8 {
            return Err(DecodeError::TruncatedPayload {
                expected: 8,
                available: buf.len(),
            });
        }
        let raw_bits = u64::from_le_bytes([
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
        ]);
        let val = f64::from_bits(raw_bits);
        let ordered = Self::try_from(val)?;
        Ok((ordered, 8))
    }
}

/// Total classification enum for 32-bit floating-point values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum EventF32 {
    /// Canonical NaN value with payload bits stripped.
    NaN = 0,
    /// Negative infinity (`-∞`).
    NegInf = 1,
    /// Finite, normalized, non-subnormal floating-point value.
    Finite(OrderedF32) = 2,
    /// Positive infinity (`+∞`).
    PosInf = 3,
}

impl EventF32 {
    /// Classifies an IEEE-754 primitive `f32` into an `EventF32`.
    ///
    /// # Errors
    /// Returns [`DecodeError::ValueConstraintViolated`] with [`ValueConstraint::NotReal`]
    /// if `v` is a subnormal floating-point value.
    pub fn from_f32(v: f32) -> Result<Self, DecodeError> {
        if v.is_nan() {
            Ok(Self::NaN)
        } else if v == f32::NEG_INFINITY {
            Ok(Self::NegInf)
        } else if v == f32::INFINITY {
            Ok(Self::PosInf)
        } else {
            let ordered = OrderedF32::try_from(v)?;
            Ok(Self::Finite(ordered))
        }
    }

    /// Converts this `EventF32` into an IEEE-754 primitive `f32`.
    #[must_use]
    pub fn to_f32(self) -> f32 {
        match self {
            Self::NaN => f32::NAN,
            Self::NegInf => f32::NEG_INFINITY,
            Self::Finite(v) => v.get(),
            Self::PosInf => f32::INFINITY,
        }
    }
}

impl TryFrom<f32> for EventF32 {
    type Error = DecodeError;

    fn try_from(v: f32) -> Result<Self, Self::Error> {
        Self::from_f32(v)
    }
}

impl From<EventF32> for f32 {
    fn from(v: EventF32) -> Self {
        v.to_f32()
    }
}

impl From<OrderedF32> for EventF32 {
    fn from(v: OrderedF32) -> Self {
        Self::Finite(v)
    }
}

impl TryFrom<EventF32> for OrderedF32 {
    type Error = DecodeError;

    fn try_from(v: EventF32) -> Result<Self, Self::Error> {
        match v {
            EventF32::Finite(inner) => Ok(inner),
            _ => Err(DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }),
        }
    }
}

impl PardosaType for EventF32 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::Enum {
            name: "EventF32".to_string(),
            discriminant_width: 1,
            variants: vec![
                VariantDescriptor {
                    discriminant: 0,
                    name: "NaN".to_string(),
                    payload: None,
                },
                VariantDescriptor {
                    discriminant: 1,
                    name: "NegInf".to_string(),
                    payload: None,
                },
                VariantDescriptor {
                    discriminant: 2,
                    name: "Finite".to_string(),
                    payload: Some(DescriptorNode::OrderedF32),
                },
                VariantDescriptor {
                    discriminant: 3,
                    name: "PosInf".to_string(),
                    payload: None,
                },
            ],
        }
    }

    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::NaN => {
                buf.push(0x00);
                Ok(())
            }
            Self::NegInf => {
                buf.push(0x01);
                Ok(())
            }
            Self::Finite(ordered) => {
                buf.push(0x02);
                ordered.encode_type(buf)
            }
            Self::PosInf => {
                buf.push(0x03);
                Ok(())
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
            0x00 => Ok((Self::NaN, 1)),
            0x01 => Ok((Self::NegInf, 1)),
            0x02 => {
                let (ordered, consumed) = OrderedF32::decode_type(&buf[1..])?;
                Ok((Self::Finite(ordered), 1 + consumed))
            }
            0x03 => Ok((Self::PosInf, 1)),
            other => Err(DecodeError::UnknownVariantDiscriminant {
                discriminant: other as u32,
            }),
        }
    }
}

/// Total classification enum for 64-bit floating-point values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum EventF64 {
    /// Canonical NaN value with payload bits stripped.
    NaN = 0,
    /// Negative infinity (`-∞`).
    NegInf = 1,
    /// Finite, normalized, non-subnormal floating-point value.
    Finite(OrderedF64) = 2,
    /// Positive infinity (`+∞`).
    PosInf = 3,
}

impl EventF64 {
    /// Classifies an IEEE-754 primitive `f64` into an `EventF64`.
    ///
    /// # Errors
    /// Returns [`DecodeError::ValueConstraintViolated`] with [`ValueConstraint::NotReal`]
    /// if `v` is a subnormal floating-point value.
    pub fn from_f64(v: f64) -> Result<Self, DecodeError> {
        if v.is_nan() {
            Ok(Self::NaN)
        } else if v == f64::NEG_INFINITY {
            Ok(Self::NegInf)
        } else if v == f64::INFINITY {
            Ok(Self::PosInf)
        } else {
            let ordered = OrderedF64::try_from(v)?;
            Ok(Self::Finite(ordered))
        }
    }

    /// Converts this `EventF64` into an IEEE-754 primitive `f64`.
    #[must_use]
    pub fn to_f64(self) -> f64 {
        match self {
            Self::NaN => f64::NAN,
            Self::NegInf => f64::NEG_INFINITY,
            Self::Finite(v) => v.get(),
            Self::PosInf => f64::INFINITY,
        }
    }
}

impl TryFrom<f64> for EventF64 {
    type Error = DecodeError;

    fn try_from(v: f64) -> Result<Self, Self::Error> {
        Self::from_f64(v)
    }
}

impl From<EventF64> for f64 {
    fn from(v: EventF64) -> Self {
        v.to_f64()
    }
}

impl From<OrderedF64> for EventF64 {
    fn from(v: OrderedF64) -> Self {
        Self::Finite(v)
    }
}

impl TryFrom<EventF64> for OrderedF64 {
    type Error = DecodeError;

    fn try_from(v: EventF64) -> Result<Self, Self::Error> {
        match v {
            EventF64::Finite(inner) => Ok(inner),
            _ => Err(DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }),
        }
    }
}

impl PardosaType for EventF64 {
    fn descriptor_node() -> DescriptorNode {
        DescriptorNode::Enum {
            name: "EventF64".to_string(),
            discriminant_width: 1,
            variants: vec![
                VariantDescriptor {
                    discriminant: 0,
                    name: "NaN".to_string(),
                    payload: None,
                },
                VariantDescriptor {
                    discriminant: 1,
                    name: "NegInf".to_string(),
                    payload: None,
                },
                VariantDescriptor {
                    discriminant: 2,
                    name: "Finite".to_string(),
                    payload: Some(DescriptorNode::OrderedF64),
                },
                VariantDescriptor {
                    discriminant: 3,
                    name: "PosInf".to_string(),
                    payload: None,
                },
            ],
        }
    }

    fn encode_type(&self, buf: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::NaN => {
                buf.push(0x00);
                Ok(())
            }
            Self::NegInf => {
                buf.push(0x01);
                Ok(())
            }
            Self::Finite(ordered) => {
                buf.push(0x02);
                ordered.encode_type(buf)
            }
            Self::PosInf => {
                buf.push(0x03);
                Ok(())
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
            0x00 => Ok((Self::NaN, 1)),
            0x01 => Ok((Self::NegInf, 1)),
            0x02 => {
                let (ordered, consumed) = OrderedF64::decode_type(&buf[1..])?;
                Ok((Self::Finite(ordered), 1 + consumed))
            }
            0x03 => Ok((Self::PosInf, 1)),
            other => Err(DecodeError::UnknownVariantDiscriminant {
                discriminant: other as u32,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_ordered_f32_valid_and_normalization() {
        let pos = OrderedF32::try_from(42.5f32).unwrap();
        assert_eq!(pos.get(), 42.5);

        let neg = OrderedF32::try_from(-10.25f32).unwrap();
        assert_eq!(neg.get(), -10.25);

        let p_zero = OrderedF32::try_from(0.0f32).unwrap();
        let n_zero = OrderedF32::try_from(-0.0f32).unwrap();
        assert_eq!(p_zero.get().to_bits(), 0x0000_0000);
        assert_eq!(n_zero.get().to_bits(), 0x0000_0000);
        assert_eq!(p_zero, n_zero);

        let mut set = HashSet::new();
        set.insert(p_zero);
        assert!(set.contains(&n_zero));
    }

    #[test]
    fn test_ordered_f32_rejections() {
        let err_nan = OrderedF32::try_from(f32::NAN).unwrap_err();
        assert_eq!(
            err_nan,
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );

        let err_pos_inf = OrderedF32::try_from(f32::INFINITY).unwrap_err();
        assert_eq!(
            err_pos_inf,
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );

        let err_neg_inf = OrderedF32::try_from(f32::NEG_INFINITY).unwrap_err();
        assert_eq!(
            err_neg_inf,
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );

        let subnormal_pos = f32::from_bits(1);
        let err_sub_pos = OrderedF32::try_from(subnormal_pos).unwrap_err();
        assert_eq!(
            err_sub_pos,
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );

        let subnormal_neg = f32::from_bits(0x8000_0001);
        let err_sub_neg = OrderedF32::try_from(subnormal_neg).unwrap_err();
        assert_eq!(
            err_sub_neg,
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );
    }

    #[test]
    fn test_ordered_f32_total_ordering() {
        let a = OrderedF32::try_from(-100.0f32).unwrap();
        let b = OrderedF32::try_from(-0.0f32).unwrap();
        let c = OrderedF32::try_from(0.0f32).unwrap();
        let d = OrderedF32::try_from(50.0f32).unwrap();

        assert_eq!(b, c);
        assert!(a < b);
        assert!(b <= c);
        assert!(c < d);
    }

    #[test]
    fn test_ordered_f32_wire_codecs() {
        let val = OrderedF32::try_from(123.456f32).unwrap();
        let mut buf = Vec::new();
        val.encode_type(&mut buf).unwrap();
        assert_eq!(buf.len(), 4);

        let (decoded, consumed) = OrderedF32::decode_type(&buf).unwrap();
        assert_eq!(consumed, 4);
        assert_eq!(decoded, val);

        let truncated = OrderedF32::decode_type(&buf[..3]).unwrap_err();
        assert_eq!(
            truncated,
            DecodeError::TruncatedPayload {
                expected: 4,
                available: 3,
            }
        );

        let mut nan_buf = Vec::new();
        nan_buf.extend_from_slice(&f32::NAN.to_bits().to_le_bytes());
        let err_nan = OrderedF32::decode_type(&nan_buf).unwrap_err();
        assert_eq!(
            err_nan,
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );

        let mut neg_zero_buf = Vec::new();
        neg_zero_buf.extend_from_slice(&(-0.0f32).to_bits().to_le_bytes());
        let (decoded_zero, consumed_zero) = OrderedF32::decode_type(&neg_zero_buf).unwrap();
        assert_eq!(consumed_zero, 4);
        assert_eq!(decoded_zero.get().to_bits(), 0x0000_0000);
    }

    #[test]
    fn test_event_f32_classification_and_nan_stripping() {
        let nan1 = EventF32::from_f32(f32::NAN).unwrap();
        let nan2 = EventF32::from_f32(f32::from_bits(0x7fc0_1234)).unwrap();
        let nan3 = EventF32::from_f32(f32::from_bits(0xffc0_9999)).unwrap();

        assert_eq!(nan1, EventF32::NaN);
        assert_eq!(nan2, EventF32::NaN);
        assert_eq!(nan3, EventF32::NaN);

        let mut buf1 = Vec::new();
        nan1.encode_type(&mut buf1).unwrap();
        assert_eq!(buf1, vec![0x00]);

        let mut buf2 = Vec::new();
        nan2.encode_type(&mut buf2).unwrap();
        assert_eq!(buf2, vec![0x00]);

        let (decoded_nan, consumed) = EventF32::decode_type(&buf1).unwrap();
        assert_eq!(consumed, 1);
        assert_eq!(decoded_nan, EventF32::NaN);
        assert!(decoded_nan.to_f32().is_nan());

        let pos_inf = EventF32::from_f32(f32::INFINITY).unwrap();
        assert_eq!(pos_inf, EventF32::PosInf);
        assert_eq!(pos_inf.to_f32(), f32::INFINITY);
        let mut buf_pos = Vec::new();
        pos_inf.encode_type(&mut buf_pos).unwrap();
        assert_eq!(buf_pos, vec![0x03]);
        let (dec_pos, c_pos) = EventF32::decode_type(&buf_pos).unwrap();
        assert_eq!(c_pos, 1);
        assert_eq!(dec_pos, EventF32::PosInf);

        let neg_inf = EventF32::from_f32(f32::NEG_INFINITY).unwrap();
        assert_eq!(neg_inf, EventF32::NegInf);
        assert_eq!(neg_inf.to_f32(), f32::NEG_INFINITY);
        let mut buf_neg = Vec::new();
        neg_inf.encode_type(&mut buf_neg).unwrap();
        assert_eq!(buf_neg, vec![0x01]);
        let (dec_neg, c_neg) = EventF32::decode_type(&buf_neg).unwrap();
        assert_eq!(c_neg, 1);
        assert_eq!(dec_neg, EventF32::NegInf);

        let finite = EventF32::from_f32(-0.0f32).unwrap();
        assert_eq!(finite, EventF32::Finite(OrderedF32::try_from(0.0).unwrap()));
        assert_eq!(finite.to_f32().to_bits(), 0x0000_0000);
        let mut buf_fin = Vec::new();
        finite.encode_type(&mut buf_fin).unwrap();
        assert_eq!(buf_fin, vec![0x02, 0x00, 0x00, 0x00, 0x00]);
        let (dec_fin, c_fin) = EventF32::decode_type(&buf_fin).unwrap();
        assert_eq!(c_fin, 5);
        assert_eq!(dec_fin, finite);

        let subnormal = f32::from_bits(1);
        let err_sub = EventF32::from_f32(subnormal).unwrap_err();
        assert_eq!(
            err_sub,
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );

        let err_disc = EventF32::decode_type(&[0x04]).unwrap_err();
        assert_eq!(
            err_disc,
            DecodeError::UnknownVariantDiscriminant { discriminant: 4 }
        );
    }

    #[test]
    fn test_ordered_f64_and_event_f64() {
        let p_zero = OrderedF64::try_from(0.0f64).unwrap();
        let n_zero = OrderedF64::try_from(-0.0f64).unwrap();
        assert_eq!(p_zero.get().to_bits(), 0x0000_0000_0000_0000);
        assert_eq!(n_zero.get().to_bits(), 0x0000_0000_0000_0000);
        assert_eq!(p_zero, n_zero);

        let err_nan = OrderedF64::try_from(f64::NAN).unwrap_err();
        assert_eq!(
            err_nan,
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );

        let err_sub = OrderedF64::try_from(f64::from_bits(1)).unwrap_err();
        assert_eq!(
            err_sub,
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );

        let nan = EventF64::from_f64(f64::from_bits(0x7ff8_0000_1234_5678)).unwrap();
        assert_eq!(nan, EventF64::NaN);
        let mut buf_nan = Vec::new();
        nan.encode_type(&mut buf_nan).unwrap();
        assert_eq!(buf_nan, vec![0x00]);
        let (dec_nan, c_nan) = EventF64::decode_type(&buf_nan).unwrap();
        assert_eq!(c_nan, 1);
        assert_eq!(dec_nan, EventF64::NaN);

        let pos_inf = EventF64::from_f64(f64::INFINITY).unwrap();
        assert_eq!(pos_inf, EventF64::PosInf);
        let mut buf_pos = Vec::new();
        pos_inf.encode_type(&mut buf_pos).unwrap();
        assert_eq!(buf_pos, vec![0x03]);

        let neg_inf = EventF64::from_f64(f64::NEG_INFINITY).unwrap();
        assert_eq!(neg_inf, EventF64::NegInf);
        let mut buf_neg = Vec::new();
        neg_inf.encode_type(&mut buf_neg).unwrap();
        assert_eq!(buf_neg, vec![0x01]);

        let finite = EventF64::from_f64(9876.54321f64).unwrap();
        let mut buf_fin = Vec::new();
        finite.encode_type(&mut buf_fin).unwrap();
        assert_eq!(buf_fin.len(), 9);
        assert_eq!(buf_fin[0], 0x02);
        let (dec_fin, c_fin) = EventF64::decode_type(&buf_fin).unwrap();
        assert_eq!(c_fin, 9);
        assert_eq!(dec_fin, finite);
        assert_eq!(dec_fin.to_f64(), 9876.54321f64);
    }

    #[test]
    fn test_schema_identity_distinction_and_wrong_codec_rejection() {
        let ordered_f32_desc = <OrderedF32 as PardosaType>::descriptor_node();
        let event_f32_desc = <EventF32 as PardosaType>::descriptor_node();
        assert_ne!(ordered_f32_desc, event_f32_desc);

        let ordered_f64_desc = <OrderedF64 as PardosaType>::descriptor_node();
        let event_f64_desc = <EventF64 as PardosaType>::descriptor_node();
        assert_ne!(ordered_f64_desc, event_f64_desc);

        let id_ord32 = crate::schema::SchemaIdentity::from_descriptor(1, &ordered_f32_desc);
        let id_evt32 = crate::schema::SchemaIdentity::from_descriptor(1, &event_f32_desc);
        assert_ne!(id_ord32, id_evt32);

        let id_ord64 = crate::schema::SchemaIdentity::from_descriptor(1, &ordered_f64_desc);
        let id_evt64 = crate::schema::SchemaIdentity::from_descriptor(1, &event_f64_desc);
        assert_ne!(id_ord64, id_evt64);

        let mut nan32_wire = Vec::new();
        EventF32::NaN.encode_type(&mut nan32_wire).unwrap();
        let err_nan32 = OrderedF32::decode_type(&nan32_wire).unwrap_err();
        assert_eq!(
            err_nan32,
            DecodeError::TruncatedPayload {
                expected: 4,
                available: 1,
            }
        );

        let mut neg_inf32_wire = Vec::new();
        EventF32::NegInf.encode_type(&mut neg_inf32_wire).unwrap();
        let err_neg_inf32 = OrderedF32::decode_type(&neg_inf32_wire).unwrap_err();
        assert_eq!(
            err_neg_inf32,
            DecodeError::TruncatedPayload {
                expected: 4,
                available: 1,
            }
        );

        let mut pos_inf32_wire = Vec::new();
        EventF32::PosInf.encode_type(&mut pos_inf32_wire).unwrap();
        let err_pos_inf32 = OrderedF32::decode_type(&pos_inf32_wire).unwrap_err();
        assert_eq!(
            err_pos_inf32,
            DecodeError::TruncatedPayload {
                expected: 4,
                available: 1,
            }
        );

        let mut fin32_wire = Vec::new();
        EventF32::Finite(OrderedF32::try_from(1.0f32).unwrap())
            .encode_type(&mut fin32_wire)
            .unwrap();
        let err_fin32 = OrderedF32::decode_type(&fin32_wire).unwrap_err();
        assert_eq!(
            err_fin32,
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );

        let mut nan64_wire = Vec::new();
        EventF64::NaN.encode_type(&mut nan64_wire).unwrap();
        let err_nan64 = OrderedF64::decode_type(&nan64_wire).unwrap_err();
        assert_eq!(
            err_nan64,
            DecodeError::TruncatedPayload {
                expected: 8,
                available: 1,
            }
        );

        let mut neg_inf64_wire = Vec::new();
        EventF64::NegInf.encode_type(&mut neg_inf64_wire).unwrap();
        let err_neg_inf64 = OrderedF64::decode_type(&neg_inf64_wire).unwrap_err();
        assert_eq!(
            err_neg_inf64,
            DecodeError::TruncatedPayload {
                expected: 8,
                available: 1,
            }
        );

        let mut pos_inf64_wire = Vec::new();
        EventF64::PosInf.encode_type(&mut pos_inf64_wire).unwrap();
        let err_pos_inf64 = OrderedF64::decode_type(&pos_inf64_wire).unwrap_err();
        assert_eq!(
            err_pos_inf64,
            DecodeError::TruncatedPayload {
                expected: 8,
                available: 1,
            }
        );

        let mut fin64_wire = Vec::new();
        EventF64::Finite(OrderedF64::try_from(2.0f64).unwrap())
            .encode_type(&mut fin64_wire)
            .unwrap();
        let err_fin64 = OrderedF64::decode_type(&fin64_wire).unwrap_err();
        assert_eq!(
            err_fin64,
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );

        let bad_disc_wire = [0x04u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let err_evt32 = EventF32::decode_type(&bad_disc_wire).unwrap_err();
        assert_eq!(
            err_evt32,
            DecodeError::UnknownVariantDiscriminant { discriminant: 4 }
        );

        let err_evt64 = EventF64::decode_type(&bad_disc_wire).unwrap_err();
        assert_eq!(
            err_evt64,
            DecodeError::UnknownVariantDiscriminant { discriminant: 4 }
        );
    }

    fn compute_hash<T: Hash>(val: &T) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        val.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn test_exhaustive_f32_contract() {
        let n100 = OrderedF32::try_from(-100.0f32).unwrap();
        let n0 = OrderedF32::try_from(-0.0f32).unwrap();
        let p0 = OrderedF32::try_from(0.0f32).unwrap();
        let p100 = OrderedF32::try_from(100.0f32).unwrap();

        assert!(n100 < n0);
        assert_eq!(n0, p0);
        assert_eq!(n0.cmp(&p0), Ordering::Equal);
        assert!(p0 < p100);

        assert_eq!(n0.get().to_bits(), 0x0000_0000);
        assert_eq!(p0.get().to_bits(), 0x0000_0000);
        assert_eq!(compute_hash(&n0), compute_hash(&p0));

        let ev_nan = EventF32::NaN;
        let ev_neginf = EventF32::NegInf;
        let ev_n100 = EventF32::Finite(n100);
        let ev_n0 = EventF32::from_f32(-0.0f32).unwrap();
        let ev_p0 = EventF32::from_f32(0.0f32).unwrap();
        let ev_p100 = EventF32::Finite(p100);
        let ev_posinf = EventF32::PosInf;

        assert!(ev_nan < ev_neginf);
        assert!(ev_neginf < ev_n100);
        assert!(ev_n100 < ev_n0);
        assert_eq!(ev_n0, ev_p0);
        assert_eq!(ev_n0.cmp(&ev_p0), Ordering::Equal);
        assert_eq!(compute_hash(&ev_n0), compute_hash(&ev_p0));
        assert!(ev_p0 < ev_p100);
        assert!(ev_p100 < ev_posinf);

        let mut n0_buf = Vec::new();
        n0.encode_type(&mut n0_buf).unwrap();
        assert_eq!(n0_buf, vec![0x00, 0x00, 0x00, 0x00]);

        let (dec_n0, c) = OrderedF32::decode_type(&[0x00, 0x00, 0x00, 0x80]).unwrap();
        assert_eq!(c, 4);
        assert_eq!(dec_n0.get().to_bits(), 0x0000_0000);

        let (dec_ev_n0, c_ev) = EventF32::decode_type(&[0x02, 0x00, 0x00, 0x00, 0x80]).unwrap();
        assert_eq!(c_ev, 5);
        assert_eq!(dec_ev_n0, ev_p0);

        let subnormals = [
            f32::from_bits(0x0000_0001),
            f32::from_bits(0x007f_ffff),
            f32::from_bits(0x8000_0001),
            f32::from_bits(0x807f_ffff),
        ];
        for s in &subnormals {
            let err_ord = OrderedF32::try_from(*s).unwrap_err();
            assert_eq!(
                err_ord,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );

            let err_evt = EventF32::from_f32(*s).unwrap_err();
            assert_eq!(
                err_evt,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );

            let err_wire = OrderedF32::decode_type(&s.to_bits().to_le_bytes()).unwrap_err();
            assert_eq!(
                err_wire,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );

            let mut b = vec![0x02];
            b.extend_from_slice(&s.to_bits().to_le_bytes());
            let err_evt_wire = EventF32::decode_type(&b).unwrap_err();
            assert_eq!(
                err_evt_wire,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
        }

        let normals = [f32::MIN_POSITIVE, f32::MAX, -f32::MIN_POSITIVE, -f32::MAX];
        for n in &normals {
            let ord = OrderedF32::try_from(*n).unwrap();
            let mut buf = Vec::new();
            ord.encode_type(&mut buf).unwrap();
            let (dec, c) = OrderedF32::decode_type(&buf).unwrap();
            assert_eq!(c, 4);
            assert_eq!(dec, ord);

            let ev = EventF32::from_f32(*n).unwrap();
            assert_eq!(ev, EventF32::Finite(ord));
            let mut ev_buf = Vec::new();
            ev.encode_type(&mut ev_buf).unwrap();
            let (dec_ev, c_ev) = EventF32::decode_type(&ev_buf).unwrap();
            assert_eq!(c_ev, 5);
            assert_eq!(dec_ev, ev);
        }

        assert_eq!(
            OrderedF32::try_from(f32::INFINITY).unwrap_err(),
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );
        assert_eq!(
            OrderedF32::try_from(f32::NEG_INFINITY).unwrap_err(),
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );
        assert_eq!(EventF32::from_f32(f32::INFINITY).unwrap(), EventF32::PosInf);
        assert_eq!(
            EventF32::from_f32(f32::NEG_INFINITY).unwrap(),
            EventF32::NegInf
        );

        for inf in [f32::INFINITY, f32::NEG_INFINITY] {
            let inf_wire = inf.to_bits().to_le_bytes();
            assert_eq!(
                OrderedF32::decode_type(&inf_wire).unwrap_err(),
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
            let mut finite_inf_wire = vec![0x02];
            finite_inf_wire.extend_from_slice(&inf_wire);
            assert_eq!(
                EventF32::decode_type(&finite_inf_wire).unwrap_err(),
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
        }

        let nans = [
            f32::NAN,
            f32::from_bits(0x7f80_0001),
            f32::from_bits(0xff80_0001),
            f32::from_bits(0x7fc0_0001),
            f32::from_bits(0xffc0_0001),
        ];
        for nan in &nans {
            assert_eq!(
                OrderedF32::try_from(*nan).unwrap_err(),
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
            let raw_wire = nan.to_bits().to_le_bytes();
            assert_eq!(
                OrderedF32::decode_type(&raw_wire).unwrap_err(),
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
            let mut finite_nan_wire = vec![0x02];
            finite_nan_wire.extend_from_slice(&raw_wire);
            assert_eq!(
                EventF32::decode_type(&finite_nan_wire).unwrap_err(),
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
            let ev_nan = EventF32::from_f32(*nan).unwrap();
            assert_eq!(ev_nan, EventF32::NaN);
            let mut buf = Vec::new();
            ev_nan.encode_type(&mut buf).unwrap();
            assert_eq!(buf, vec![0x00]);
            let (dec_nan, c_nan) = EventF32::decode_type(&buf).unwrap();
            assert_eq!(c_nan, 1);
            assert_eq!(dec_nan, EventF32::NaN);
        }

        for bad in [0x04u8, 0x05, 0x42, 0xFF] {
            let err = EventF32::decode_type(&[bad]).unwrap_err();
            assert_eq!(
                err,
                DecodeError::UnknownVariantDiscriminant {
                    discriminant: bad as u32,
                }
            );
        }

        assert_eq!(
            EventF32::decode_type(&[]).unwrap_err(),
            DecodeError::TruncatedPayload {
                expected: 1,
                available: 0,
            }
        );
        for len in 0..4 {
            let mut b = vec![0x02];
            b.extend(vec![0u8; len]);
            assert_eq!(
                EventF32::decode_type(&b).unwrap_err(),
                DecodeError::TruncatedPayload {
                    expected: 4,
                    available: len,
                }
            );
        }
    }

    #[test]
    fn test_exhaustive_f64_contract() {
        let n100 = OrderedF64::try_from(-100.0f64).unwrap();
        let n0 = OrderedF64::try_from(-0.0f64).unwrap();
        let p0 = OrderedF64::try_from(0.0f64).unwrap();
        let p100 = OrderedF64::try_from(100.0f64).unwrap();

        assert!(n100 < n0);
        assert_eq!(n0, p0);
        assert_eq!(n0.cmp(&p0), Ordering::Equal);
        assert!(p0 < p100);

        assert_eq!(n0.get().to_bits(), 0x0000_0000_0000_0000);
        assert_eq!(p0.get().to_bits(), 0x0000_0000_0000_0000);
        assert_eq!(compute_hash(&n0), compute_hash(&p0));

        let ev_nan = EventF64::NaN;
        let ev_neginf = EventF64::NegInf;
        let ev_n100 = EventF64::Finite(n100);
        let ev_n0 = EventF64::from_f64(-0.0f64).unwrap();
        let ev_p0 = EventF64::from_f64(0.0f64).unwrap();
        let ev_p100 = EventF64::Finite(p100);
        let ev_posinf = EventF64::PosInf;

        assert!(ev_nan < ev_neginf);
        assert!(ev_neginf < ev_n100);
        assert!(ev_n100 < ev_n0);
        assert_eq!(ev_n0, ev_p0);
        assert_eq!(ev_n0.cmp(&ev_p0), Ordering::Equal);
        assert_eq!(compute_hash(&ev_n0), compute_hash(&ev_p0));
        assert!(ev_p0 < ev_p100);
        assert!(ev_p100 < ev_posinf);

        let mut n0_buf = Vec::new();
        n0.encode_type(&mut n0_buf).unwrap();
        assert_eq!(n0_buf, vec![0x00; 8]);

        let (dec_n0, c) =
            OrderedF64::decode_type(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80]).unwrap();
        assert_eq!(c, 8);
        assert_eq!(dec_n0.get().to_bits(), 0x0000_0000_0000_0000);

        let (dec_ev_n0, c_ev) =
            EventF64::decode_type(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80]).unwrap();
        assert_eq!(c_ev, 9);
        assert_eq!(dec_ev_n0, ev_p0);

        let subnormals = [
            f64::from_bits(0x0000_0000_0000_0001),
            f64::from_bits(0x000f_ffff_ffff_ffff),
            f64::from_bits(0x8000_0000_0000_0001),
            f64::from_bits(0x800f_ffff_ffff_ffff),
        ];
        for s in &subnormals {
            let err_ord = OrderedF64::try_from(*s).unwrap_err();
            assert_eq!(
                err_ord,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );

            let err_evt = EventF64::from_f64(*s).unwrap_err();
            assert_eq!(
                err_evt,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );

            let err_wire = OrderedF64::decode_type(&s.to_bits().to_le_bytes()).unwrap_err();
            assert_eq!(
                err_wire,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );

            let mut b = vec![0x02];
            b.extend_from_slice(&s.to_bits().to_le_bytes());
            let err_evt_wire = EventF64::decode_type(&b).unwrap_err();
            assert_eq!(
                err_evt_wire,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
        }

        let normals = [f64::MIN_POSITIVE, f64::MAX, -f64::MIN_POSITIVE, -f64::MAX];
        for n in &normals {
            let ord = OrderedF64::try_from(*n).unwrap();
            let mut buf = Vec::new();
            ord.encode_type(&mut buf).unwrap();
            let (dec, c) = OrderedF64::decode_type(&buf).unwrap();
            assert_eq!(c, 8);
            assert_eq!(dec, ord);

            let ev = EventF64::from_f64(*n).unwrap();
            assert_eq!(ev, EventF64::Finite(ord));
            let mut ev_buf = Vec::new();
            ev.encode_type(&mut ev_buf).unwrap();
            let (dec_ev, c_ev) = EventF64::decode_type(&ev_buf).unwrap();
            assert_eq!(c_ev, 9);
            assert_eq!(dec_ev, ev);
        }

        assert_eq!(
            OrderedF64::try_from(f64::INFINITY).unwrap_err(),
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );
        assert_eq!(
            OrderedF64::try_from(f64::NEG_INFINITY).unwrap_err(),
            DecodeError::ValueConstraintViolated {
                constraint: ValueConstraint::NotReal,
            }
        );
        assert_eq!(EventF64::from_f64(f64::INFINITY).unwrap(), EventF64::PosInf);
        assert_eq!(
            EventF64::from_f64(f64::NEG_INFINITY).unwrap(),
            EventF64::NegInf
        );

        for inf in [f64::INFINITY, f64::NEG_INFINITY] {
            let inf_wire = inf.to_bits().to_le_bytes();
            assert_eq!(
                OrderedF64::decode_type(&inf_wire).unwrap_err(),
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
            let mut finite_inf_wire = vec![0x02];
            finite_inf_wire.extend_from_slice(&inf_wire);
            assert_eq!(
                EventF64::decode_type(&finite_inf_wire).unwrap_err(),
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
        }

        let nans = [
            f64::NAN,
            f64::from_bits(0x7ff0_0000_0000_0001),
            f64::from_bits(0xfff0_0000_0000_0001),
            f64::from_bits(0x7ff8_0000_0000_0001),
            f64::from_bits(0xfff8_0000_0000_0001),
        ];
        for nan in &nans {
            assert_eq!(
                OrderedF64::try_from(*nan).unwrap_err(),
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
            let raw_wire = nan.to_bits().to_le_bytes();
            assert_eq!(
                OrderedF64::decode_type(&raw_wire).unwrap_err(),
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
            let mut finite_nan_wire = vec![0x02];
            finite_nan_wire.extend_from_slice(&raw_wire);
            assert_eq!(
                EventF64::decode_type(&finite_nan_wire).unwrap_err(),
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
            let ev_nan = EventF64::from_f64(*nan).unwrap();
            assert_eq!(ev_nan, EventF64::NaN);
            let mut buf = Vec::new();
            ev_nan.encode_type(&mut buf).unwrap();
            assert_eq!(buf, vec![0x00]);
            let (dec_nan, c_nan) = EventF64::decode_type(&buf).unwrap();
            assert_eq!(c_nan, 1);
            assert_eq!(dec_nan, EventF64::NaN);
        }

        for bad in [0x04u8, 0x05, 0x42, 0xFF] {
            let err = EventF64::decode_type(&[bad]).unwrap_err();
            assert_eq!(
                err,
                DecodeError::UnknownVariantDiscriminant {
                    discriminant: bad as u32,
                }
            );
        }

        assert_eq!(
            EventF64::decode_type(&[]).unwrap_err(),
            DecodeError::TruncatedPayload {
                expected: 1,
                available: 0,
            }
        );
        for len in 0..8 {
            let mut b = vec![0x02];
            b.extend(vec![0u8; len]);
            assert_eq!(
                EventF64::decode_type(&b).unwrap_err(),
                DecodeError::TruncatedPayload {
                    expected: 8,
                    available: len,
                }
            );
        }
    }

    #[test]
    fn test_wire_nan_and_infinity_rejection_f32() {
        let non_finite_raw_bits: [u32; 6] = [
            0x7fc0_0000,
            0xffc0_0000,
            0x7f80_0001,
            0xff80_0001,
            f32::INFINITY.to_bits(),
            f32::NEG_INFINITY.to_bits(),
        ];

        for bits in non_finite_raw_bits {
            let raw_bytes = bits.to_le_bytes();
            let err_ord = OrderedF32::decode_type(&raw_bytes).unwrap_err();
            assert_eq!(
                err_ord,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );

            let mut finite_payload = vec![0x02];
            finite_payload.extend_from_slice(&raw_bytes);
            let err_event = EventF32::decode_type(&finite_payload).unwrap_err();
            assert_eq!(
                err_event,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
        }

        let control_scalars = [
            0.0f32,
            -0.0f32,
            1.0f32,
            -1.0f32,
            f32::MIN_POSITIVE,
            f32::MAX,
        ];
        for val in control_scalars {
            let raw_bytes = val.to_bits().to_le_bytes();
            let (ord_val, ord_consumed) = OrderedF32::decode_type(&raw_bytes).unwrap();
            assert_eq!(ord_consumed, 4);
            if val == 0.0 {
                assert_eq!(ord_val.get().to_bits(), 0x0000_0000);
            } else {
                assert_eq!(ord_val.get(), val);
            }

            let mut finite_payload = vec![0x02];
            finite_payload.extend_from_slice(&raw_bytes);
            let (event_val, event_consumed) = EventF32::decode_type(&finite_payload).unwrap();
            assert_eq!(event_consumed, 5);
            assert_eq!(event_val, EventF32::Finite(ord_val));
        }
    }

    #[test]
    fn test_wire_nan_and_infinity_rejection_f64() {
        let non_finite_raw_bits: [u64; 6] = [
            0x7ff8_0000_0000_0000,
            0xfff8_0000_0000_0000,
            0x7ff0_0000_0000_0001,
            0xfff0_0000_0000_0001,
            f64::INFINITY.to_bits(),
            f64::NEG_INFINITY.to_bits(),
        ];

        for bits in non_finite_raw_bits {
            let raw_bytes = bits.to_le_bytes();
            let err_ord = OrderedF64::decode_type(&raw_bytes).unwrap_err();
            assert_eq!(
                err_ord,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );

            let mut finite_payload = vec![0x02];
            finite_payload.extend_from_slice(&raw_bytes);
            let err_event = EventF64::decode_type(&finite_payload).unwrap_err();
            assert_eq!(
                err_event,
                DecodeError::ValueConstraintViolated {
                    constraint: ValueConstraint::NotReal,
                }
            );
        }

        let control_scalars = [
            0.0f64,
            -0.0f64,
            1.0f64,
            -1.0f64,
            f64::MIN_POSITIVE,
            f64::MAX,
        ];
        for val in control_scalars {
            let raw_bytes = val.to_bits().to_le_bytes();
            let (ord_val, ord_consumed) = OrderedF64::decode_type(&raw_bytes).unwrap();
            assert_eq!(ord_consumed, 8);
            if val == 0.0 {
                assert_eq!(ord_val.get().to_bits(), 0x0000_0000_0000_0000);
            } else {
                assert_eq!(ord_val.get(), val);
            }

            let mut finite_payload = vec![0x02];
            finite_payload.extend_from_slice(&raw_bytes);
            let (event_val, event_consumed) = EventF64::decode_type(&finite_payload).unwrap();
            assert_eq!(event_consumed, 9);
            assert_eq!(event_val, EventF64::Finite(ord_val));
        }
    }
}
