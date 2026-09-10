//! Container format and framing for Pardosa artefacts.

use crate::encoding::DecodeError;

/// Magic bytes identifying a Pardosa container (`PARDOSA\x01`).
pub const CONTAINER_MAGIC: [u8; 8] = [0x50, 0x41, 0x52, 0x44, 0x4f, 0x53, 0x41, 0x01];

/// Current container format version.
pub const CONTAINER_FORMAT_VERSION: u32 = 1;

/// Standard container header for Pardosa artefacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerHeader {
    /// Format version number (must be 1 for 1.0 specification).
    pub format_version: u32,
}

impl Default for ContainerHeader {
    fn default() -> Self {
        Self {
            format_version: CONTAINER_FORMAT_VERSION,
        }
    }
}

impl ContainerHeader {
    /// Creates a new container header with the default format version.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Serializes this container header into 12 bytes.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 12] {
        let mut buf = [0u8; 12];
        buf[..8].copy_from_slice(&CONTAINER_MAGIC);
        buf[8..12].copy_from_slice(&self.format_version.to_le_bytes());
        buf
    }

    /// Encodes this container header into a buffer.
    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_bytes());
    }

    /// Decodes a container header from the start of a byte slice.
    ///
    /// Returns the decoded header and the number of bytes consumed (always 12).
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedHeader` if fewer than 12 bytes are available.
    /// Returns `DecodeError::InvalidMagic` if magic bytes do not match `PARDOSA\x01`.
    /// Returns `DecodeError::UnsupportedFormatVersion` if version is not 1.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), DecodeError> {
        if buf.len() < 8 {
            return Err(DecodeError::TruncatedHeader {
                expected: 12,
                available: buf.len(),
            });
        }
        if buf[..8] != CONTAINER_MAGIC {
            return Err(DecodeError::InvalidMagic);
        }
        if buf.len() < 12 {
            return Err(DecodeError::TruncatedHeader {
                expected: 12,
                available: buf.len(),
            });
        }
        let format_version = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        if format_version != CONTAINER_FORMAT_VERSION {
            return Err(DecodeError::UnsupportedFormatVersion {
                version: format_version,
            });
        }
        Ok((Self { format_version }, 12))
    }
}

/// A framed entry in a Pardosa container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerFrame {
    /// Enclosed payload bytes.
    pub payload: Vec<u8>,
    /// Recorded CRC32C checksum.
    pub checksum: u32,
}

impl ContainerFrame {
    /// Wraps payload bytes into a frame, computing its CRC32C checksum.
    #[must_use]
    pub fn new(payload: Vec<u8>) -> Self {
        let checksum = crc32c::crc32c(&payload);
        Self { payload, checksum }
    }

    /// Encodes a payload into container framing format (length + payload + CRC32C).
    pub fn encode_payload(payload: &[u8], buf: &mut Vec<u8>) {
        let len = payload.len() as u32;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(payload);
        let crc = crc32c::crc32c(payload);
        buf.extend_from_slice(&crc.to_le_bytes());
    }

    /// Decodes a framed chunk from a byte slice.
    ///
    /// Returns the decoded payload bytes and the total number of frame bytes consumed.
    ///
    /// # Errors
    /// Returns `DecodeError::TruncatedPayload` if framing length prefix or payload is truncated.
    /// Returns `DecodeError::ChecksumMismatch` if computed CRC32C does not match recorded checksum.
    pub fn decode(buf: &[u8]) -> Result<(Vec<u8>, usize), DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::TruncatedPayload {
                expected: 4,
                available: buf.len(),
            });
        }
        let payload_len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        let total_frame_len = 4 + payload_len + 4;
        if buf.len() < 4 + payload_len {
            return Err(DecodeError::TruncatedPayload {
                expected: total_frame_len,
                available: buf.len(),
            });
        }
        if buf.len() < total_frame_len {
            return Err(DecodeError::TruncatedPayload {
                expected: total_frame_len,
                available: buf.len(),
            });
        }
        let payload = &buf[4..4 + payload_len];
        let recorded_checksum = u32::from_le_bytes([
            buf[4 + payload_len],
            buf[4 + payload_len + 1],
            buf[4 + payload_len + 2],
            buf[4 + payload_len + 3],
        ]);
        let computed_checksum = crc32c::crc32c(payload);
        if recorded_checksum != computed_checksum {
            return Err(DecodeError::ChecksumMismatch {
                computed: computed_checksum,
                recorded: recorded_checksum,
            });
        }
        Ok((payload.to_vec(), total_frame_len))
    }
}

/// Running physical rolling commitment over sequential frames written to an artefact container per C5.26.
///
/// Computes a continuous BLAKE3 digest over all framed entries written to the container,
/// establishing physical dragline integrity and enabling external observer anchoring.
#[derive(Debug, Clone)]
pub struct RollingCommitment {
    hasher: blake3::Hasher,
    frame_count: u64,
}

impl Default for RollingCommitment {
    fn default() -> Self {
        Self::new()
    }
}

impl RollingCommitment {
    /// Creates a new empty rolling commitment tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            hasher: blake3::Hasher::new(),
            frame_count: 0,
        }
    }

    /// Incorporates a container frame into the running dragline commitment.
    ///
    /// Updates the rolling BLAKE3 digest with the framed entry bytes.
    pub fn update_frame(&mut self, frame_bytes: &[u8]) {
        self.hasher.update(frame_bytes);
        self.frame_count = self.frame_count.saturating_add(1);
    }

    /// Returns the number of frames incorporated into this rolling commitment.
    #[must_use]
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Returns the current 32-byte BLAKE3 commitment digest without finalizing the hasher.
    #[must_use]
    pub fn current_commitment(&self) -> [u8; 32] {
        *self.hasher.finalize().as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_header_roundtrip() {
        let header = ContainerHeader::new();
        let bytes = header.to_bytes();
        let (decoded, consumed) = ContainerHeader::decode(&bytes).unwrap();
        assert_eq!(consumed, 12);
        assert_eq!(decoded, header);
    }

    #[test]
    fn test_container_frame_roundtrip() {
        let payload = b"Hello, Pardosa Container!".to_vec();
        let mut buf = Vec::new();
        ContainerFrame::encode_payload(&payload, &mut buf);
        let (decoded, consumed) = ContainerFrame::decode(&buf).unwrap();
        assert_eq!(consumed, buf.len());
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_corrupt_frame_checksum() {
        let payload = b"Data to corrupt".to_vec();
        let mut buf = Vec::new();
        ContainerFrame::encode_payload(&payload, &mut buf);
        let last_idx = buf.len() - 1;
        buf[last_idx] ^= 0xFF;
        let err = ContainerFrame::decode(&buf).unwrap_err();
        assert_eq!(err.error_kind(), "ChecksumMismatch");
    }

    #[test]
    fn test_rolling_commitment_frame_progression_and_tamper_detection() {
        let mut commitment = RollingCommitment::new();
        assert_eq!(commitment.frame_count(), 0);
        let initial_digest = commitment.current_commitment();

        let mut frame1 = Vec::new();
        ContainerFrame::encode_payload(b"event-1", &mut frame1);
        commitment.update_frame(&frame1);
        assert_eq!(commitment.frame_count(), 1);
        let digest1 = commitment.current_commitment();
        assert_ne!(initial_digest, digest1);

        let mut frame2 = Vec::new();
        ContainerFrame::encode_payload(b"event-2", &mut frame2);
        commitment.update_frame(&frame2);
        assert_eq!(commitment.frame_count(), 2);
        let digest2 = commitment.current_commitment();
        assert_ne!(digest1, digest2);

        let mut tampered_commitment = RollingCommitment::new();
        let mut tampered_frame1 = frame1.clone();
        tampered_frame1[4] ^= 0x01;
        tampered_commitment.update_frame(&tampered_frame1);
        tampered_commitment.update_frame(&frame2);
        assert_ne!(
            commitment.current_commitment(),
            tampered_commitment.current_commitment()
        );
    }
}
