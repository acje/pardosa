//! Wire encoding, envelope layout, ownership records, and value constraints.

use std::fmt;
use std::ops::Deref;

/// Closed vocabulary of value constraints for value-decoding failures per C6.7 / C5.53.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueConstraint {
    /// Value length or item count exceeds the allowed bound.
    TooLong,
    /// Value is empty where non-empty value was required.
    Empty,
    /// Numeric value is not a real or valid representation.
    NotReal,
    /// Value contains an invalid character.
    InvalidChar,
    /// Value contains invalid UTF-8 bytes.
    InvalidUtf8,
}

/// Errors occurring during decoding of container, envelope, or payload bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Container magic bytes do not match `PARDOSA\x01`.
    InvalidMagic,
    /// Container format version is unsupported.
    UnsupportedFormatVersion {
        /// Recorded format version.
        version: u32,
    },
    /// Recorded CRC32C checksum does not match computed checksum.
    ChecksumMismatch {
        /// Computed CRC32C checksum.
        computed: u32,
        /// Recorded CRC32C checksum.
        recorded: u32,
    },
    /// Declared framing payload length exceeds available bytes.
    TruncatedPayload {
        /// Declared expected length.
        expected: usize,
        /// Available byte count.
        available: usize,
    },
    /// Header bytes are fewer than required fixed layout.
    TruncatedHeader {
        /// Expected header byte count.
        expected: usize,
        /// Available byte count.
        available: usize,
    },
    /// Boolean discriminant in envelope detached flag is neither 0x00 nor 0x01.
    InvalidBooleanDiscriminant {
        /// Invalid byte value encountered.
        value: u8,
    },
    /// Boolean value on wire is neither 0x00 nor 0x01.
    InvalidBoolean {
        /// Invalid byte value encountered.
        value: u8,
    },
    /// Byte length of string or byte buffer exceeds maximum bound.
    LengthExceeded {
        /// Actual byte length.
        length: usize,
        /// Maximum allowed bound.
        max: usize,
    },
    /// Non-empty string has length 0.
    EmptyNonEmptyString,
    /// Byte slice does not contain valid UTF-8 text.
    InvalidUtf8,
    /// Item count in collection exceeds maximum bound.
    ItemCountExceeded {
        /// Actual item count.
        count: usize,
        /// Maximum allowed bound.
        max: usize,
    },
    /// Option tag byte is neither 0x00 nor 0x01.
    InvalidOptionTag {
        /// Invalid tag encountered.
        tag: u8,
    },
    /// Enum discriminant value is unknown.
    UnknownVariantDiscriminant {
        /// Discriminant value encountered.
        discriminant: u32,
    },
    /// Timestamp value is the invalid reserved sentinel 0.
    InvalidTimestampSentinel,
    /// Ownership record tag is outside recognized range 0x01..=0x09.
    UnknownRecordTag {
        /// Tag value encountered.
        tag: u8,
    },
    /// Schema descriptor AST constructor tag is outside recognized range 0x01..=0x12.
    UnknownConstructorTag {
        /// Tag value encountered.
        tag: u8,
    },
    /// Partitioning rule tag is unrecognised.
    UnknownPartitioningRuleTag {
        /// Tag value encountered.
        tag: u8,
    },
    /// Migration status byte is invalid.
    InvalidMigrationStatus {
        /// Status byte encountered.
        status: u8,
    },
    /// Rescue policy tag is invalid.
    InvalidRescuePolicy {
        /// Policy tag encountered.
        tag: u8,
    },
    /// Enum discriminant width is neither 1 nor 2.
    InvalidDiscriminantWidth {
        /// Width byte encountered.
        width: u8,
    },
    /// Cycle or circular reference detected in schema descriptor.
    CycleDetected {
        /// Type name where cycle was detected.
        type_name: String,
    },
    /// Unterminated descriptor payload.
    UnterminatedDescriptor,
    /// Specific value constraint violated.
    ValueConstraintViolated {
        /// Violated constraint.
        constraint: ValueConstraint,
    },
    /// Custom error message.
    Custom(String),
}

impl DecodeError {
    /// Returns the canonical error kind string matching conformance vector definitions.
    #[must_use]
    pub fn error_kind(&self) -> &'static str {
        match self {
            Self::InvalidMagic => "InvalidMagic",
            Self::UnsupportedFormatVersion { .. } => "UnsupportedFormatVersion",
            Self::ChecksumMismatch { .. } => "ChecksumMismatch",
            Self::TruncatedPayload { .. } => "TruncatedPayload",
            Self::TruncatedHeader { .. } => "TruncatedHeader",
            Self::InvalidBooleanDiscriminant { .. } => "InvalidBooleanDiscriminant",
            Self::InvalidBoolean { .. } => "InvalidBoolean",
            Self::LengthExceeded { .. } => "LengthExceeded",
            Self::EmptyNonEmptyString => "EmptyNonEmptyString",
            Self::InvalidUtf8 => "InvalidUtf8",
            Self::ItemCountExceeded { .. } => "ItemCountExceeded",
            Self::InvalidOptionTag { .. } => "InvalidOptionTag",
            Self::UnknownVariantDiscriminant { .. } => "UnknownVariantDiscriminant",
            Self::InvalidTimestampSentinel => "InvalidTimestampSentinel",
            Self::UnknownRecordTag { .. } => "UnknownRecordTag",
            Self::UnknownConstructorTag { .. } => "UnknownConstructorTag",
            Self::UnknownPartitioningRuleTag { .. } => "UnknownPartitioningRuleTag",
            Self::InvalidMigrationStatus { .. } => "InvalidMigrationStatus",
            Self::InvalidRescuePolicy { .. } => "InvalidRescuePolicy",
            Self::InvalidDiscriminantWidth { .. } => "InvalidDiscriminantWidth",
            Self::CycleDetected { .. } => "CycleDetected",
            Self::UnterminatedDescriptor => "UnterminatedDescriptor",
            Self::ValueConstraintViolated { constraint } => match constraint {
                ValueConstraint::TooLong => "LengthExceeded",
                ValueConstraint::Empty => "EmptyNonEmptyString",
                ValueConstraint::NotReal => "NotReal",
                ValueConstraint::InvalidChar => "InvalidChar",
                ValueConstraint::InvalidUtf8 => "InvalidUtf8",
            },
            Self::Custom(_) => "Custom",
        }
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for DecodeError {}

/// Errors occurring during encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// String or byte buffer length exceeds maximum bound.
    LengthExceeded {
        /// Actual length.
        length: usize,
        /// Maximum allowed bound.
        max: usize,
    },
    /// Non-empty string was empty.
    EmptyNonEmptyString,
    /// Timestamp value was the invalid sentinel 0.
    InvalidTimestampSentinel,
    /// Collection item count exceeds maximum bound.
    ItemCountExceeded {
        /// Actual count.
        count: usize,
        /// Maximum allowed bound.
        max: usize,
    },
    /// Custom error message.
    Custom(String),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for EncodeError {}

/// UTF-8 encoded text bounded by `MAX` bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventString<const MAX: usize>(String);

impl<const MAX: usize> EventString<MAX> {
    /// Creates a new `EventString`, verifying `len <= MAX`.
    ///
    /// # Errors
    /// Returns `DecodeError::LengthExceeded` if length exceeds `MAX`.
    pub fn new(s: impl Into<String>) -> Result<Self, DecodeError> {
        let s = s.into();
        if s.len() > MAX {
            return Err(DecodeError::LengthExceeded {
                length: s.len(),
                max: MAX,
            });
        }
        Ok(Self(s))
    }

    /// Returns string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Encodes this event string into wire format.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        let len = self.0.len() as u32;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(self.0.as_bytes());
    }

    /// Decodes an event string from wire bytes.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedPayload` if bytes are truncated.
    /// Returns `DecodeError::LengthExceeded` if declared length exceeds `MAX`.
    /// Returns `DecodeError::InvalidUtf8` if payload is not valid UTF-8.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::TruncatedPayload {
                expected: 4,
                available: buf.len(),
            });
        }
        let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if len > MAX {
            return Err(DecodeError::LengthExceeded {
                length: len,
                max: MAX,
            });
        }
        if buf.len() < 4 + len {
            return Err(DecodeError::TruncatedPayload {
                expected: 4 + len,
                available: buf.len(),
            });
        }
        let s = std::str::from_utf8(&buf[4..4 + len]).map_err(|_| DecodeError::InvalidUtf8)?;
        Ok((Self(s.to_string()), 4 + len))
    }
}

impl<const MAX: usize> Deref for EventString<MAX> {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const MAX: usize> fmt::Display for EventString<MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Non-empty UTF-8 encoded text with length between 1 and `MAX` bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NonEmptyEventString<const MAX: usize>(String);

impl<const MAX: usize> NonEmptyEventString<MAX> {
    /// Creates a new `NonEmptyEventString`, verifying `1 <= len <= MAX`.
    ///
    /// # Errors
    /// Returns `DecodeError::EmptyNonEmptyString` if length is 0.
    /// Returns `DecodeError::LengthExceeded` if length exceeds `MAX`.
    pub fn new(s: impl Into<String>) -> Result<Self, DecodeError> {
        let s = s.into();
        if s.is_empty() {
            return Err(DecodeError::EmptyNonEmptyString);
        }
        if s.len() > MAX {
            return Err(DecodeError::LengthExceeded {
                length: s.len(),
                max: MAX,
            });
        }
        Ok(Self(s))
    }

    /// Returns string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Encodes into wire format.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        let len = self.0.len() as u32;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(self.0.as_bytes());
    }

    /// Decodes from wire bytes.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedPayload` if bytes are truncated.
    /// Returns `DecodeError::EmptyNonEmptyString` if length is 0.
    /// Returns `DecodeError::LengthExceeded` if length exceeds `MAX`.
    /// Returns `DecodeError::InvalidUtf8` if payload is not valid UTF-8.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::TruncatedPayload {
                expected: 4,
                available: buf.len(),
            });
        }
        let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if len == 0 {
            return Err(DecodeError::EmptyNonEmptyString);
        }
        if len > MAX {
            return Err(DecodeError::LengthExceeded {
                length: len,
                max: MAX,
            });
        }
        if buf.len() < 4 + len {
            return Err(DecodeError::TruncatedPayload {
                expected: 4 + len,
                available: buf.len(),
            });
        }
        let s = std::str::from_utf8(&buf[4..4 + len]).map_err(|_| DecodeError::InvalidUtf8)?;
        Ok((Self(s.to_string()), 4 + len))
    }
}

impl<const MAX: usize> Deref for NonEmptyEventString<MAX> {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const MAX: usize> fmt::Display for NonEmptyEventString<MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Opaque byte sequence bounded by `MAX` bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventBytes<const MAX: usize>(Vec<u8>);

impl<const MAX: usize> EventBytes<MAX> {
    /// Creates a new `EventBytes`, verifying `len <= MAX`.
    ///
    /// # Errors
    /// Returns `DecodeError::LengthExceeded` if length exceeds `MAX`.
    pub fn new(b: impl Into<Vec<u8>>) -> Result<Self, DecodeError> {
        let b = b.into();
        if b.len() > MAX {
            return Err(DecodeError::LengthExceeded {
                length: b.len(),
                max: MAX,
            });
        }
        Ok(Self(b))
    }

    /// Returns byte slice.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Encodes into wire format.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        let len = self.0.len() as u32;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(&self.0);
    }

    /// Decodes from wire bytes.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedPayload` if bytes are truncated.
    /// Returns `DecodeError::LengthExceeded` if length exceeds `MAX`.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::TruncatedPayload {
                expected: 4,
                available: buf.len(),
            });
        }
        let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if len > MAX {
            return Err(DecodeError::LengthExceeded {
                length: len,
                max: MAX,
            });
        }
        if buf.len() < 4 + len {
            return Err(DecodeError::TruncatedPayload {
                expected: 4 + len,
                available: buf.len(),
            });
        }
        Ok((Self(buf[4..4 + len].to_vec()), 4 + len))
    }
}

impl<const MAX: usize> Deref for EventBytes<MAX> {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Bounded collection of items of admitted type `T`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventVec<T, const MAX: usize>(Vec<T>);

impl<T, const MAX: usize> EventVec<T, MAX> {
    /// Creates a new `EventVec`, verifying `len <= MAX`.
    ///
    /// # Errors
    /// Returns `DecodeError::ItemCountExceeded` if count exceeds `MAX`.
    pub fn new(items: Vec<T>) -> Result<Self, DecodeError> {
        if items.len() > MAX {
            return Err(DecodeError::ItemCountExceeded {
                count: items.len(),
                max: MAX,
            });
        }
        Ok(Self(items))
    }

    /// Returns slice of items.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// Unwraps inner vector.
    #[must_use]
    pub fn into_inner(self) -> Vec<T> {
        self.0
    }
}

impl<T, const MAX: usize> Deref for EventVec<T, MAX> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Temporal point represented as nanoseconds since Unix epoch. Value 0 is reserved invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(u64);

impl Timestamp {
    /// Creates a new `Timestamp`, ensuring value is non-zero.
    ///
    /// # Errors
    /// Returns `DecodeError::InvalidTimestampSentinel` if `nanos == 0`.
    pub fn new(nanos: u64) -> Result<Self, DecodeError> {
        if nanos == 0 {
            return Err(DecodeError::InvalidTimestampSentinel);
        }
        Ok(Self(nanos))
    }

    /// Returns nanoseconds since Unix epoch.
    #[must_use]
    pub fn as_nanos(&self) -> u64 {
        self.0
    }

    /// Encodes into 8 bytes little-endian.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.0.to_le_bytes());
    }

    /// Decodes from 8 bytes little-endian.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedPayload` if fewer than 8 bytes available.
    /// Returns `DecodeError::InvalidTimestampSentinel` if decoded value is 0.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 8 {
            return Err(DecodeError::TruncatedPayload {
                expected: 8,
                available: buf.len(),
            });
        }
        let nanos = u64::from_le_bytes([
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
        ]);
        if nanos == 0 {
            return Err(DecodeError::InvalidTimestampSentinel);
        }
        Ok((Self(nanos), 8))
    }
}

/// 128-bit universally unique identifier encoded as 16 raw bytes in RFC 4122 network order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// Creates a UUID from 16 raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Returns a reference to the raw 16 bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Returns the raw 16 bytes.
    #[must_use]
    pub const fn to_bytes(&self) -> [u8; 16] {
        self.0
    }

    /// Encodes into 16 raw bytes.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.0);
    }

    /// Decodes from 16 raw bytes.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedPayload` if fewer than 16 bytes available.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 16 {
            return Err(DecodeError::TruncatedPayload {
                expected: 16,
                available: buf.len(),
            });
        }
        let mut b = [0u8; 16];
        b.copy_from_slice(&buf[..16]);
        Ok((Self(b), 16))
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        )
    }
}

#[cfg(feature = "uuid")]
impl From<uuid::Uuid> for Uuid {
    fn from(u: uuid::Uuid) -> Self {
        Self(*u.as_bytes())
    }
}

#[cfg(feature = "uuid")]
impl From<Uuid> for uuid::Uuid {
    fn from(u: Uuid) -> Self {
        uuid::Uuid::from_bytes(u.0)
    }
}

/// Derived identity of the event envelope layout fixed by the specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvelopeIdentity([u8; 32]);

impl EnvelopeIdentity {
    /// Creates an envelope identity from raw 32-byte digest.
    #[must_use]
    pub const fn from_raw(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Computes the envelope identity for the 1.0 envelope specification.
    #[must_use]
    pub fn current() -> Self {
        let hash = blake3::hash(
            b"PARDOSA_ENVELOPE_V1:event_id:16,fiber_id:16,detached:1,precursor:16,precursor_hash:32,payload_length:4",
        );
        Self(*hash.as_bytes())
    }

    /// Returns reference to the 32-byte hash digest.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Fixed 81-byte header of an event envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeHeader {
    /// Event identifier (16 bytes).
    pub event_id: [u8; 16],
    /// Fiber identifier (16 bytes).
    pub fiber_id: [u8; 16],
    /// Detached flag (0x00 = attached, 0x01 = detached).
    pub detached: bool,
    /// Precursor event identifier (16 bytes).
    pub precursor: [u8; 16],
    /// BLAKE3 hash digest of precursor commitment (32 bytes).
    pub precursor_hash: [u8; 32],
}

impl EnvelopeHeader {
    /// Decodes an 81-byte envelope header.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedHeader` if fewer than 81 bytes are available.
    /// Returns `DecodeError::InvalidBooleanDiscriminant` if detached flag is neither 0x00 nor 0x01.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 81 {
            return Err(DecodeError::TruncatedHeader {
                expected: 81,
                available: buf.len(),
            });
        }
        let mut event_id = [0u8; 16];
        event_id.copy_from_slice(&buf[0..16]);
        let mut fiber_id = [0u8; 16];
        fiber_id.copy_from_slice(&buf[16..32]);
        let detached = match buf[32] {
            0x00 => false,
            0x01 => true,
            val => return Err(DecodeError::InvalidBooleanDiscriminant { value: val }),
        };
        let mut precursor = [0u8; 16];
        precursor.copy_from_slice(&buf[33..49]);
        let mut precursor_hash = [0u8; 32];
        precursor_hash.copy_from_slice(&buf[49..81]);
        Ok((
            Self {
                event_id,
                fiber_id,
                detached,
                precursor,
                precursor_hash,
            },
            81,
        ))
    }

    /// Encodes this 81-byte header into a buffer.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.event_id);
        buf.extend_from_slice(&self.fiber_id);
        buf.push(if self.detached { 0x01 } else { 0x00 });
        buf.extend_from_slice(&self.precursor);
        buf.extend_from_slice(&self.precursor_hash);
    }
}

/// Standard 5-field event envelope carrying payload bytes per C4.19.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventEnvelope {
    /// Fixed 81-byte header fields.
    pub header: EnvelopeHeader,
    /// Serialized event payload bytes.
    pub payload: Vec<u8>,
}

impl EventEnvelope {
    /// Encodes this event envelope into 85 + payload_length bytes.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        self.header.encode(buf);
        let payload_len = self.payload.len() as u32;
        buf.extend_from_slice(&payload_len.to_le_bytes());
        buf.extend_from_slice(&self.payload);
    }

    /// Decodes an event envelope from wire bytes.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedHeader` if header is fewer than 81 bytes.
    /// Returns `DecodeError::InvalidBooleanDiscriminant` if detached byte is invalid.
    /// Returns `DecodeError::TruncatedPayload` if declared payload length exceeds available bytes.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        let (header, header_consumed) = EnvelopeHeader::decode(buf)?;
        if buf.len() < header_consumed + 4 {
            return Err(DecodeError::TruncatedHeader {
                expected: header_consumed + 4,
                available: buf.len(),
            });
        }
        let payload_len = u32::from_le_bytes([
            buf[header_consumed],
            buf[header_consumed + 1],
            buf[header_consumed + 2],
            buf[header_consumed + 3],
        ]) as usize;
        let total_consumed = header_consumed + 4 + payload_len;
        if buf.len() < total_consumed {
            return Err(DecodeError::TruncatedPayload {
                expected: total_consumed,
                available: buf.len(),
            });
        }
        let payload = buf[header_consumed + 4..total_consumed].to_vec();
        Ok((Self { header, payload }, total_consumed))
    }

    /// Computes the 32-byte BLAKE3 commitment of this envelope per C4.19.
    #[must_use]
    pub fn commitment(&self) -> [u8; 32] {
        compute_envelope_commitment(&self.header, &self.payload)
    }
}

/// Computes the 32-byte BLAKE3 commitment of an event envelope from its canonical
/// 81-byte header followed by its payload bytes per C4.19.
#[must_use]
pub fn compute_envelope_commitment(header: &EnvelopeHeader, payload: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&header.event_id);
    hasher.update(&header.fiber_id);
    hasher.update(&[if header.detached { 0x01 } else { 0x00 }]);
    hasher.update(&header.precursor);
    hasher.update(&header.precursor_hash);
    hasher.update(payload);
    *hasher.finalize().as_bytes()
}

/// Partitioning rule algorithms for identity structure per C4.14.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitioningRule {
    /// Static modulo partitioning rule across total draglines.
    StaticModulo {
        /// Total number of draglines.
        total_draglines: u32,
    },
    /// Consistent hash partitioning rule.
    ConsistentHash {
        /// Hash seed.
        hash_seed: u64,
        /// Virtual node count per dragline.
        virtual_node_count: u32,
    },
}

impl PartitioningRule {
    /// Encodes this partitioning rule (1-byte tag, 4-byte param len, parameter bytes).
    pub fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::StaticModulo { total_draglines } => {
                buf.push(0x01);
                buf.extend_from_slice(&4u32.to_le_bytes());
                buf.extend_from_slice(&total_draglines.to_le_bytes());
            }
            Self::ConsistentHash {
                hash_seed,
                virtual_node_count,
            } => {
                buf.push(0x02);
                buf.extend_from_slice(&12u32.to_le_bytes());
                buf.extend_from_slice(&hash_seed.to_le_bytes());
                buf.extend_from_slice(&virtual_node_count.to_le_bytes());
            }
        }
    }

    /// Decodes a partitioning rule from bytes.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedPayload` if bytes are truncated.
    /// Returns `DecodeError::UnknownPartitioningRuleTag` if tag is unrecognised.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 5 {
            return Err(DecodeError::TruncatedPayload {
                expected: 5,
                available: buf.len(),
            });
        }
        let tag = buf[0];
        let param_len = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
        let total_consumed = 5 + param_len;
        if buf.len() < total_consumed {
            return Err(DecodeError::TruncatedPayload {
                expected: total_consumed,
                available: buf.len(),
            });
        }
        let param_bytes = &buf[5..total_consumed];
        match tag {
            0x01 => {
                if param_bytes.len() < 4 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: 4,
                        available: param_bytes.len(),
                    });
                }
                let total_draglines = u32::from_le_bytes([
                    param_bytes[0],
                    param_bytes[1],
                    param_bytes[2],
                    param_bytes[3],
                ]);
                Ok((Self::StaticModulo { total_draglines }, total_consumed))
            }
            0x02 => {
                if param_bytes.len() < 12 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: 12,
                        available: param_bytes.len(),
                    });
                }
                let hash_seed = u64::from_le_bytes([
                    param_bytes[0],
                    param_bytes[1],
                    param_bytes[2],
                    param_bytes[3],
                    param_bytes[4],
                    param_bytes[5],
                    param_bytes[6],
                    param_bytes[7],
                ]);
                let virtual_node_count = u32::from_le_bytes([
                    param_bytes[8],
                    param_bytes[9],
                    param_bytes[10],
                    param_bytes[11],
                ]);
                Ok((
                    Self::ConsistentHash {
                        hash_seed,
                        virtual_node_count,
                    },
                    total_consumed,
                ))
            }
            other => Err(DecodeError::UnknownPartitioningRuleTag { tag: other }),
        }
    }
}

/// Completion status for migration end record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStatus {
    /// Migration completed successfully.
    Complete,
    /// Migration was interrupted.
    Interrupted,
}

impl MigrationStatus {
    /// Encodes status as u8.
    #[must_use]
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Complete => 0x01,
            Self::Interrupted => 0x02,
        }
    }

    /// Decodes status from u8.
    ///
    /// # Errors
    /// Returns `DecodeError::InvalidMigrationStatus` if value is neither 0x01 nor 0x02.
    pub fn from_u8(val: u8) -> Result<Self, DecodeError> {
        match val {
            0x01 => Ok(Self::Complete),
            0x02 => Ok(Self::Interrupted),
            other => Err(DecodeError::InvalidMigrationStatus { status: other }),
        }
    }
}

/// Rescue policy choices recorded with migration start per C4.13.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RescuePolicy {
    /// Strict policy.
    Strict,
    /// Drop corrupted policy.
    DropCorrupted,
    /// Halt on gap policy.
    HaltOnGap,
}

impl RescuePolicy {
    /// Encodes policy as u8 tag.
    #[must_use]
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Strict => 0x01,
            Self::DropCorrupted => 0x02,
            Self::HaltOnGap => 0x03,
        }
    }

    /// Decodes policy from u8 tag.
    ///
    /// # Errors
    /// Returns `DecodeError::InvalidRescuePolicy` if tag is not in 0x01..=0x03.
    pub fn from_u8(val: u8) -> Result<Self, DecodeError> {
        match val {
            0x01 => Ok(Self::Strict),
            0x02 => Ok(Self::DropCorrupted),
            0x03 => Ok(Self::HaltOnGap),
            other => Err(DecodeError::InvalidRescuePolicy { tag: other }),
        }
    }
}

/// Kind 0x01: Ownership claim record carrying 7 semantic fields per C6.43.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipClaimRecord {
    /// Monotonic ownership epoch.
    pub epoch: u64,
    /// Machine UUID (16 bytes).
    pub machine_id: [u8; 16],
    /// Boot session UUID (16 bytes).
    pub boot_id: [u8; 16],
    /// Operating system process identifier.
    pub process_id: u64,
    /// Process start time in nanoseconds since Unix epoch.
    pub process_start_time_ns: u64,
    /// Claim time in nanoseconds since Unix epoch.
    pub claim_time_ns: u64,
    /// Human-readable operator label.
    pub operator_label: String,
}

/// Kind 0x02: Clean release record carrying epoch and release timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanReleaseRecord {
    /// Released monotonic epoch.
    pub epoch: u64,
    /// Release timestamp in nanoseconds since Unix epoch.
    pub release_time_ns: u64,
}

/// Kind 0x03: Migration start record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationStartRecord {
    /// Source generation index.
    pub source_generation: u32,
    /// Target generation index.
    pub target_generation: u32,
    /// Migration start timestamp in nanoseconds since Unix epoch.
    pub start_time_ns: u64,
    /// Rescue policy tag (1=Strict, 2=DropCorrupted, 3=HaltOnGap).
    pub rescue_policy_tag: u8,
}

/// Kind 0x04: Migration end record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationEndRecord {
    /// Source generation index.
    pub source_generation: u32,
    /// Target generation index.
    pub target_generation: u32,
    /// Migration end timestamp in nanoseconds since Unix epoch.
    pub end_time_ns: u64,
    /// Completion status.
    pub status: MigrationStatus,
}

/// Kind 0x05: Inbound pointer record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundPointerRecord {
    /// Prior generation locator identifier (16 bytes).
    pub prior_generation_locator_id: [u8; 16],
    /// Prior generation epoch.
    pub prior_generation_epoch: u64,
}

/// Kind 0x06: Outbound pointer record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundPointerRecord {
    /// Next generation locator identifier (16 bytes).
    pub next_generation_locator_id: [u8; 16],
    /// Cutover epoch.
    pub cutover_epoch: u64,
}

/// Kind 0x07: Rescue policy choice record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescuePolicyChoiceRecord {
    /// Policy tag (1=Strict, 2=DropCorrupted, 3=HaltOnGap).
    pub policy_tag: u8,
    /// Parameter bytes.
    pub parameter_payload: Vec<u8>,
}

/// Kind 0x08: Identity structure record per C4.14.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityStructureRecord {
    /// Logical dataset UUID (16 bytes).
    pub dataset_id: [u8; 16],
    /// Structure version (value 1).
    pub structure_version: u32,
    /// Dragline identifier index.
    pub dragline_id: u32,
    /// Partitioning rule algorithm and parameters.
    pub partitioning_rule: PartitioningRule,
}

/// The nine kinds of ownership records fixed at 1.0 per C4.13.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipRecord {
    /// Kind 0x01: Ownership claim.
    OwnershipClaim(OwnershipClaimRecord),
    /// Kind 0x02: Clean release.
    CleanRelease(CleanReleaseRecord),
    /// Kind 0x03: Migration start.
    MigrationStart(MigrationStartRecord),
    /// Kind 0x04: Migration end.
    MigrationEnd(MigrationEndRecord),
    /// Kind 0x05: Inbound pointer.
    InboundPointer(InboundPointerRecord),
    /// Kind 0x06: Outbound pointer.
    OutboundPointer(OutboundPointerRecord),
    /// Kind 0x07: Rescue policy choice.
    RescuePolicyChoice(RescuePolicyChoiceRecord),
    /// Kind 0x08: Identity structure.
    IdentityStructure(IdentityStructureRecord),
    /// Kind 0x09: Schema descriptor raw payload bytes.
    SchemaDescriptor {
        /// Schema version number.
        schema_version: u32,
        /// Raw AST descriptor bytes following schema version.
        descriptor_bytes: Vec<u8>,
    },
}

impl OwnershipRecord {
    /// Returns the 1-byte wire tag for this ownership record kind.
    #[must_use]
    pub fn wire_tag(&self) -> u8 {
        match self {
            Self::OwnershipClaim(_) => 0x01,
            Self::CleanRelease(_) => 0x02,
            Self::MigrationStart(_) => 0x03,
            Self::MigrationEnd(_) => 0x04,
            Self::InboundPointer(_) => 0x05,
            Self::OutboundPointer(_) => 0x06,
            Self::RescuePolicyChoice(_) => 0x07,
            Self::IdentityStructure(_) => 0x08,
            Self::SchemaDescriptor { .. } => 0x09,
        }
    }

    /// Encodes this ownership record into wire format.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(self.wire_tag());
        match self {
            Self::OwnershipClaim(c) => {
                buf.extend_from_slice(&c.epoch.to_le_bytes());
                buf.extend_from_slice(&c.machine_id);
                buf.extend_from_slice(&c.boot_id);
                buf.extend_from_slice(&c.process_id.to_le_bytes());
                buf.extend_from_slice(&c.process_start_time_ns.to_le_bytes());
                buf.extend_from_slice(&c.claim_time_ns.to_le_bytes());
                let label_bytes = c.operator_label.as_bytes();
                buf.extend_from_slice(&(label_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(label_bytes);
            }
            Self::CleanRelease(r) => {
                buf.extend_from_slice(&r.epoch.to_le_bytes());
                buf.extend_from_slice(&r.release_time_ns.to_le_bytes());
            }
            Self::MigrationStart(m) => {
                buf.extend_from_slice(&m.source_generation.to_le_bytes());
                buf.extend_from_slice(&m.target_generation.to_le_bytes());
                buf.extend_from_slice(&m.start_time_ns.to_le_bytes());
                buf.push(m.rescue_policy_tag);
            }
            Self::MigrationEnd(m) => {
                buf.extend_from_slice(&m.source_generation.to_le_bytes());
                buf.extend_from_slice(&m.target_generation.to_le_bytes());
                buf.extend_from_slice(&m.end_time_ns.to_le_bytes());
                buf.push(m.status.to_u8());
            }
            Self::InboundPointer(p) => {
                buf.extend_from_slice(&p.prior_generation_locator_id);
                buf.extend_from_slice(&p.prior_generation_epoch.to_le_bytes());
            }
            Self::OutboundPointer(p) => {
                buf.extend_from_slice(&p.next_generation_locator_id);
                buf.extend_from_slice(&p.cutover_epoch.to_le_bytes());
            }
            Self::RescuePolicyChoice(p) => {
                buf.push(p.policy_tag);
                buf.extend_from_slice(&(p.parameter_payload.len() as u32).to_le_bytes());
                buf.extend_from_slice(&p.parameter_payload);
            }
            Self::IdentityStructure(ids) => {
                buf.extend_from_slice(&ids.dataset_id);
                buf.extend_from_slice(&ids.structure_version.to_le_bytes());
                buf.extend_from_slice(&ids.dragline_id.to_le_bytes());
                ids.partitioning_rule.encode(buf);
            }
            Self::SchemaDescriptor {
                schema_version,
                descriptor_bytes,
            } => {
                buf.extend_from_slice(&schema_version.to_le_bytes());
                buf.extend_from_slice(descriptor_bytes);
            }
        }
    }

    /// Decodes an ownership record from wire bytes.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedPayload` if bytes are truncated.
    /// Returns `DecodeError::UnknownRecordTag` if tag is outside 0x01..=0x09.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::TruncatedPayload {
                expected: 1,
                available: 0,
            });
        }
        let tag = buf[0];
        match tag {
            0x01 => {
                let fixed_prefix = 1 + 64;
                if buf.len() < fixed_prefix + 4 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: fixed_prefix + 4,
                        available: buf.len(),
                    });
                }
                let epoch = u64::from_le_bytes([
                    buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8],
                ]);
                let mut machine_id = [0u8; 16];
                machine_id.copy_from_slice(&buf[9..25]);
                let mut boot_id = [0u8; 16];
                boot_id.copy_from_slice(&buf[25..41]);
                let process_id = u64::from_le_bytes([
                    buf[41], buf[42], buf[43], buf[44], buf[45], buf[46], buf[47], buf[48],
                ]);
                let process_start_time_ns = u64::from_le_bytes([
                    buf[49], buf[50], buf[51], buf[52], buf[53], buf[54], buf[55], buf[56],
                ]);
                let claim_time_ns = u64::from_le_bytes([
                    buf[57], buf[58], buf[59], buf[60], buf[61], buf[62], buf[63], buf[64],
                ]);
                let label_len = u32::from_le_bytes([buf[65], buf[66], buf[67], buf[68]]) as usize;
                let total_len = fixed_prefix + 4 + label_len;
                if buf.len() < total_len {
                    return Err(DecodeError::TruncatedPayload {
                        expected: total_len,
                        available: buf.len(),
                    });
                }
                let operator_label = std::str::from_utf8(&buf[69..total_len])
                    .map_err(|_| DecodeError::InvalidUtf8)?
                    .to_string();
                Ok((
                    Self::OwnershipClaim(OwnershipClaimRecord {
                        epoch,
                        machine_id,
                        boot_id,
                        process_id,
                        process_start_time_ns,
                        claim_time_ns,
                        operator_label,
                    }),
                    total_len,
                ))
            }
            0x02 => {
                let total_len = 1 + 16;
                if buf.len() < total_len {
                    return Err(DecodeError::TruncatedPayload {
                        expected: total_len,
                        available: buf.len(),
                    });
                }
                let epoch = u64::from_le_bytes([
                    buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8],
                ]);
                let release_time_ns = u64::from_le_bytes([
                    buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15], buf[16],
                ]);
                Ok((
                    Self::CleanRelease(CleanReleaseRecord {
                        epoch,
                        release_time_ns,
                    }),
                    total_len,
                ))
            }
            0x03 => {
                let total_len = 1 + 4 + 4 + 8 + 1;
                if buf.len() < total_len {
                    return Err(DecodeError::TruncatedPayload {
                        expected: total_len,
                        available: buf.len(),
                    });
                }
                let source_generation = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
                let target_generation = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
                let start_time_ns = u64::from_le_bytes([
                    buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15], buf[16],
                ]);
                let rescue_policy_tag = buf[17];
                Ok((
                    Self::MigrationStart(MigrationStartRecord {
                        source_generation,
                        target_generation,
                        start_time_ns,
                        rescue_policy_tag,
                    }),
                    total_len,
                ))
            }
            0x04 => {
                let total_len = 1 + 4 + 4 + 8 + 1;
                if buf.len() < total_len {
                    return Err(DecodeError::TruncatedPayload {
                        expected: total_len,
                        available: buf.len(),
                    });
                }
                let source_generation = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
                let target_generation = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
                let end_time_ns = u64::from_le_bytes([
                    buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15], buf[16],
                ]);
                let status = MigrationStatus::from_u8(buf[17])?;
                Ok((
                    Self::MigrationEnd(MigrationEndRecord {
                        source_generation,
                        target_generation,
                        end_time_ns,
                        status,
                    }),
                    total_len,
                ))
            }
            0x05 => {
                let total_len = 1 + 16 + 8;
                if buf.len() < total_len {
                    return Err(DecodeError::TruncatedPayload {
                        expected: total_len,
                        available: buf.len(),
                    });
                }
                let mut prior_generation_locator_id = [0u8; 16];
                prior_generation_locator_id.copy_from_slice(&buf[1..17]);
                let prior_generation_epoch = u64::from_le_bytes([
                    buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23], buf[24],
                ]);
                Ok((
                    Self::InboundPointer(InboundPointerRecord {
                        prior_generation_locator_id,
                        prior_generation_epoch,
                    }),
                    total_len,
                ))
            }
            0x06 => {
                let total_len = 1 + 16 + 8;
                if buf.len() < total_len {
                    return Err(DecodeError::TruncatedPayload {
                        expected: total_len,
                        available: buf.len(),
                    });
                }
                let mut next_generation_locator_id = [0u8; 16];
                next_generation_locator_id.copy_from_slice(&buf[1..17]);
                let cutover_epoch = u64::from_le_bytes([
                    buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23], buf[24],
                ]);
                Ok((
                    Self::OutboundPointer(OutboundPointerRecord {
                        next_generation_locator_id,
                        cutover_epoch,
                    }),
                    total_len,
                ))
            }
            0x07 => {
                if buf.len() < 1 + 1 + 4 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: 6,
                        available: buf.len(),
                    });
                }
                let policy_tag = buf[1];
                let param_len = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]) as usize;
                let total_len = 6 + param_len;
                if buf.len() < total_len {
                    return Err(DecodeError::TruncatedPayload {
                        expected: total_len,
                        available: buf.len(),
                    });
                }
                let parameter_payload = buf[6..total_len].to_vec();
                Ok((
                    Self::RescuePolicyChoice(RescuePolicyChoiceRecord {
                        policy_tag,
                        parameter_payload,
                    }),
                    total_len,
                ))
            }
            0x08 => {
                let prefix_len = 1 + 16 + 4 + 4;
                if buf.len() < prefix_len {
                    return Err(DecodeError::TruncatedPayload {
                        expected: prefix_len,
                        available: buf.len(),
                    });
                }
                let mut dataset_id = [0u8; 16];
                dataset_id.copy_from_slice(&buf[1..17]);
                let structure_version = u32::from_le_bytes([buf[17], buf[18], buf[19], buf[20]]);
                let dragline_id = u32::from_le_bytes([buf[21], buf[22], buf[23], buf[24]]);
                let (partitioning_rule, rule_consumed) =
                    PartitioningRule::decode(&buf[prefix_len..])?;
                let total_len = prefix_len + rule_consumed;
                Ok((
                    Self::IdentityStructure(IdentityStructureRecord {
                        dataset_id,
                        structure_version,
                        dragline_id,
                        partitioning_rule,
                    }),
                    total_len,
                ))
            }
            0x09 => {
                if buf.len() < 1 + 4 {
                    return Err(DecodeError::TruncatedPayload {
                        expected: 5,
                        available: buf.len(),
                    });
                }
                let schema_version = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
                let descriptor_bytes = buf[5..].to_vec();
                let total_len = buf.len();
                Ok((
                    Self::SchemaDescriptor {
                        schema_version,
                        descriptor_bytes,
                    },
                    total_len,
                ))
            }
            other => Err(DecodeError::UnknownRecordTag { tag: other }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_string_valid_and_over_bound() {
        let valid = EventString::<10>::new("12345").unwrap();
        assert_eq!(valid.as_str(), "12345");
        let mut buf = Vec::new();
        valid.encode(&mut buf);
        let (decoded, consumed) = EventString::<10>::decode(&buf).unwrap();
        assert_eq!(consumed, buf.len());
        assert_eq!(decoded, valid);

        let err = EventString::<4>::new("12345").unwrap_err();
        assert_eq!(err.error_kind(), "LengthExceeded");
    }

    #[test]
    fn test_non_empty_event_string() {
        let err_empty = NonEmptyEventString::<10>::new("").unwrap_err();
        assert_eq!(err_empty.error_kind(), "EmptyNonEmptyString");

        let valid = NonEmptyEventString::<10>::new("non-empty").unwrap();
        assert_eq!(valid.as_str(), "non-empty");

        let err_over = NonEmptyEventString::<5>::new("toolong").unwrap_err();
        assert_eq!(err_over.error_kind(), "LengthExceeded");
    }

    #[test]
    fn test_event_bytes_and_vec() {
        let bytes = EventBytes::<8>::new(vec![1, 2, 3]).unwrap();
        assert_eq!(bytes.as_slice(), &[1, 2, 3]);

        let err_bytes = EventBytes::<2>::new(vec![1, 2, 3]).unwrap_err();
        assert_eq!(err_bytes.error_kind(), "LengthExceeded");

        let vec_val = EventVec::<u16, 4>::new(vec![10, 20]).unwrap();
        assert_eq!(vec_val.as_slice(), &[10, 20]);

        let err_vec = EventVec::<u16, 1>::new(vec![10, 20]).unwrap_err();
        assert_eq!(err_vec.error_kind(), "ItemCountExceeded");
    }

    #[test]
    fn test_timestamp_sentinel() {
        let err = Timestamp::new(0).unwrap_err();
        assert_eq!(err.error_kind(), "InvalidTimestampSentinel");

        let ts = Timestamp::new(123456789).unwrap();
        assert_eq!(ts.as_nanos(), 123456789);

        let mut buf = Vec::new();
        ts.encode(&mut buf);
        let (decoded, consumed) = Timestamp::decode(&buf).unwrap();
        assert_eq!(consumed, 8);
        assert_eq!(decoded, ts);
    }

    #[test]
    fn test_uuid_codecs() {
        let raw = [1u8; 16];
        let u = Uuid::from_bytes(raw);
        assert_eq!(u.as_bytes(), &raw);
        assert_eq!(u.to_bytes(), raw);

        let mut buf = Vec::new();
        u.encode(&mut buf);
        let (decoded, consumed) = Uuid::decode(&buf).unwrap();
        assert_eq!(consumed, 16);
        assert_eq!(decoded, u);
    }

    #[test]
    fn test_envelope_identity_stability() {
        let id1 = EnvelopeIdentity::current();
        let id2 = EnvelopeIdentity::current();
        assert_eq!(id1, id2);
        assert_ne!(id1.as_bytes(), &[0u8; 32]);
    }

    #[test]
    fn test_ownership_records_roundtrip_coverage() {
        let records = vec![
            OwnershipRecord::InboundPointer(InboundPointerRecord {
                prior_generation_locator_id: [3u8; 16],
                prior_generation_epoch: 100,
            }),
            OwnershipRecord::OutboundPointer(OutboundPointerRecord {
                next_generation_locator_id: [4u8; 16],
                cutover_epoch: 101,
            }),
            OwnershipRecord::RescuePolicyChoice(RescuePolicyChoiceRecord {
                policy_tag: 0x02,
                parameter_payload: vec![10, 20, 30],
            }),
            OwnershipRecord::IdentityStructure(IdentityStructureRecord {
                dataset_id: [5u8; 16],
                structure_version: 1,
                dragline_id: 7,
                partitioning_rule: PartitioningRule::ConsistentHash {
                    hash_seed: 0xDEADBEEF,
                    virtual_node_count: 64,
                },
            }),
        ];

        for rec in records {
            let mut buf = Vec::new();
            rec.encode(&mut buf);
            let (decoded, consumed) = OwnershipRecord::decode(&buf).unwrap();
            assert_eq!(consumed, buf.len());
            assert_eq!(decoded, rec);
        }
    }
}
