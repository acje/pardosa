//! Container format and framing for Pardosa artefacts.

use crate::encoding::{
    DecodeError, EventEnvelope, InboundPointerRecord, MigrationEndRecord, MigrationStartRecord,
    OutboundPointerRecord, OwnershipClaimRecord, OwnershipRecord, RescuePolicyChoiceRecord,
};
use crate::schema::{DescriptorNode, SchemaDescriptor};
use crate::store::{
    admit_create, admit_open, ArtefactPresence, FailureCondition, OpenAdmission, OperationFailure,
};
use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

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

/// Policy governing writer session platform exclusion per C5.6, C5.13, and C5.65.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileExclusionPolicy {
    /// Standard OS file locking (via `File::try_lock`).
    #[default]
    Standard,
    /// Deterministically simulate exclusion mechanism unavailable on this platform.
    SimulateUnavailable,
    /// Deterministically simulate another owner holding the exclusion lock.
    SimulateHeldByOther,
}

fn acquire_file_exclusion(
    file: &File,
    policy: FileExclusionPolicy,
) -> Result<(), OperationFailure> {
    match policy {
        FileExclusionPolicy::Standard => match file.try_lock() {
            Ok(()) => Ok(()),
            Err(TryLockError::WouldBlock) => Err(OperationFailure::new(
                FailureCondition::AnotherOwnerHoldsExclusion,
                "another owner holds the required file exclusion per C5.6 and C5.65",
            )),
            Err(TryLockError::Error(err)) => Err(OperationFailure::new(
                FailureCondition::ExclusionUnavailable,
                format!("platform exclusion mechanism is unavailable per C5.6 and C5.13: {err}"),
            )),
        },
        FileExclusionPolicy::SimulateUnavailable => Err(OperationFailure::new(
            FailureCondition::ExclusionUnavailable,
            "simulated platform exclusion mechanism unavailable per C5.6 and C5.13",
        )),
        FileExclusionPolicy::SimulateHeldByOther => Err(OperationFailure::new(
            FailureCondition::AnotherOwnerHoldsExclusion,
            "simulated another owner holds the required file exclusion per C5.6 and C5.65",
        )),
    }
}

fn read_container_frames(
    file: &mut File,
) -> Result<(ContainerHeader, Vec<Vec<u8>>, RollingCommitment), OperationFailure> {
    file.seek(SeekFrom::Start(0)).map_err(|err| {
        OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            format!("failed to seek container file: {err}"),
        )
    })?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|err| {
        OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            format!("failed to read container file: {err}"),
        )
    })?;
    let (header, mut cursor) = ContainerHeader::decode(&buf).map_err(|err| {
        OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            format!("invalid container header: {err}"),
        )
    })?;
    let mut frames = Vec::new();
    let mut rolling = RollingCommitment::new();
    while cursor < buf.len() {
        let (payload, consumed) = ContainerFrame::decode(&buf[cursor..]).map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("invalid container frame at offset {cursor}: {err}"),
            )
        })?;
        rolling.update_frame(&buf[cursor..cursor + consumed]);
        frames.push(payload);
        cursor += consumed;
    }
    Ok((header, frames, rolling))
}

/// All decoded ownership records found in an artefact's .meta file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetaRecords {
    /// Latest ownership claim record.
    pub latest_claim: Option<OwnershipClaimRecord>,
    /// Attached schema descriptor.
    pub schema_descriptor: Option<SchemaDescriptor>,
    /// Outbound generation pointer indicating cutover and permanent retirement per C6.17 and C5.63.
    pub outbound_pointer: Option<OutboundPointerRecord>,
    /// Inbound generation pointer indicating predecessor generation per C6.16.
    pub inbound_pointer: Option<InboundPointerRecord>,
    /// Migration start record per C4.13.
    pub migration_start: Option<MigrationStartRecord>,
    /// Migration end record per C4.13.
    pub migration_end: Option<MigrationEndRecord>,
    /// Rescue policy choice record per C4.13.
    pub rescue_policy_choice: Option<RescuePolicyChoiceRecord>,
}

/// Reads all ownership records from a .meta file per C4.13 and C5.63.
///
/// # Errors
/// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if file exists but cannot be read or decoded.
pub fn read_meta_records(meta_path: &Path) -> Result<MetaRecords, OperationFailure> {
    if !meta_path.exists() {
        return Ok(MetaRecords::default());
    }
    let mut file = File::open(meta_path).map_err(|err| {
        OperationFailure::new(
            FailureCondition::OwnershipRecordUnreadable,
            format!("failed to open .meta file: {err}"),
        )
    })?;
    let (_, frames, _) = read_container_frames(&mut file).map_err(|err| {
        OperationFailure::new(
            FailureCondition::OwnershipRecordUnreadable,
            format!("failed to read frames from .meta file: {err}"),
        )
    })?;
    let mut records = MetaRecords::default();
    for frame in frames {
        let (record, _) = OwnershipRecord::decode(&frame).map_err(|err| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!("failed to decode ownership record from .meta: {err}"),
            )
        })?;
        match record {
            OwnershipRecord::OwnershipClaim(claim) => {
                records.latest_claim = Some(claim);
            }
            OwnershipRecord::SchemaDescriptor {
                schema_version,
                descriptor_bytes,
            } => {
                let (root, _) = DescriptorNode::decode(&descriptor_bytes).map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to decode schema descriptor in .meta: {err}"),
                    )
                })?;
                records.schema_descriptor = Some(SchemaDescriptor::new(schema_version, root));
            }
            OwnershipRecord::OutboundPointer(p) => {
                records.outbound_pointer = Some(p);
            }
            OwnershipRecord::InboundPointer(p) => {
                records.inbound_pointer = Some(p);
            }
            OwnershipRecord::MigrationStart(m) => {
                records.migration_start = Some(m);
            }
            OwnershipRecord::MigrationEnd(m) => {
                records.migration_end = Some(m);
            }
            OwnershipRecord::RescuePolicyChoice(r) => {
                records.rescue_policy_choice = Some(r);
            }
            _ => {}
        }
    }
    Ok(records)
}

fn append_meta_record(meta_path: &Path, record: &OwnershipRecord) -> Result<(), OperationFailure> {
    let mut meta_file = OpenOptions::new()
        .append(true)
        .open(meta_path)
        .map_err(|err| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!("failed to open .meta for append: {err}"),
            )
        })?;
    let mut rec_buf = Vec::new();
    record.encode(&mut rec_buf);
    let mut frame_buf = Vec::new();
    ContainerFrame::encode_payload(&rec_buf, &mut frame_buf);
    meta_file.write_all(&frame_buf).map_err(|err| {
        OperationFailure::new(
            FailureCondition::OwnershipRecordUnreadable,
            format!("failed to write record to .meta: {err}"),
        )
    })?;
    meta_file.sync_data().map_err(|err| {
        OperationFailure::new(
            FailureCondition::OwnershipRecordUnreadable,
            format!("failed to sync .meta: {err}"),
        )
    })?;
    Ok(())
}

/// Filesystem storage adapter managing container artefacts (.meta and .pgno pairs) per C5.10, C5.11, and C10.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStorageAdapter {
    meta_path: PathBuf,
    pgno_path: PathBuf,
    stem: String,
    exclusion_policy: FileExclusionPolicy,
}

impl FileStorageAdapter {
    /// Creates a new file storage adapter from a base path.
    ///
    /// Strips `.meta` or `.pgno` extension if present to compute common stem.
    #[must_use]
    pub fn new(base_path: impl AsRef<Path>) -> Self {
        let base = base_path.as_ref();
        let (parent, stem) = match (base.parent(), base.file_stem()) {
            (Some(p), Some(s)) => (p, s.to_string_lossy().to_string()),
            (None, Some(s)) => (Path::new(""), s.to_string_lossy().to_string()),
            _ => (Path::new(""), "artefact".to_string()),
        };
        let meta_path = if parent.as_os_str().is_empty() {
            PathBuf::from(format!("{stem}.meta"))
        } else {
            parent.join(format!("{stem}.meta"))
        };
        let pgno_path = if parent.as_os_str().is_empty() {
            PathBuf::from(format!("{stem}.pgno"))
        } else {
            parent.join(format!("{stem}.pgno"))
        };
        Self {
            meta_path,
            pgno_path,
            stem,
            exclusion_policy: FileExclusionPolicy::Standard,
        }
    }

    /// Configures the writer session exclusion policy.
    #[must_use]
    pub fn with_exclusion_policy(mut self, policy: FileExclusionPolicy) -> Self {
        self.exclusion_policy = policy;
        self
    }

    /// Returns the path to the ownership record file (.meta).
    #[must_use]
    pub fn meta_path(&self) -> &Path {
        &self.meta_path
    }

    /// Returns the path to the event data container file (.pgno).
    #[must_use]
    pub fn pgno_path(&self) -> &Path {
        &self.pgno_path
    }

    /// Returns the common artefact stem.
    #[must_use]
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// Returns the 16-byte locator identifier derived from the stem.
    #[must_use]
    pub fn locator_id(&self) -> [u8; 16] {
        let hash = blake3::hash(self.stem.as_bytes());
        let mut id = [0u8; 16];
        id.copy_from_slice(&hash.as_bytes()[0..16]);
        id
    }

    /// Returns the current monotonic epoch for this artefact from .meta.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if reading fails.
    pub fn current_epoch(&self) -> Result<u64, OperationFailure> {
        let meta = self.read_meta_records()?;
        Ok(meta.latest_claim.map_or(0, |c| c.epoch))
    }

    /// Returns the configured exclusion policy.
    #[must_use]
    pub fn exclusion_policy(&self) -> FileExclusionPolicy {
        self.exclusion_policy
    }

    /// Returns the presence of artefact components in storage per C5.10.
    #[must_use]
    pub fn presence(&self) -> ArtefactPresence {
        match (self.meta_path.exists(), self.pgno_path.exists()) {
            (false, false) => ArtefactPresence::None,
            (true, false) => ArtefactPresence::OwnershipRecordOnly,
            (false, true) => ArtefactPresence::EventDataOnly,
            (true, true) => ArtefactPresence::Both,
        }
    }

    /// Creates the artefact files exclusively with initial ownership claim per C5.10, C5.64, and C12.3.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::StoreAlreadyExists`] if artefact already exists.
    /// Returns [`OperationFailure`] with [`FailureCondition::ExclusionUnavailable`] or
    /// [`FailureCondition::AnotherOwnerHoldsExclusion`] if writer exclusion fails.
    pub fn create(
        &self,
        initial_claim: &OwnershipClaimRecord,
    ) -> Result<FileWriterSession, OperationFailure> {
        admit_create(self.presence())?;
        let mut meta_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&self.meta_path)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    OperationFailure::new(
                        FailureCondition::StoreAlreadyExists,
                        "artefact .meta already exists per C12.3",
                    )
                } else {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to create .meta: {err}"),
                    )
                }
            })?;
        let header_bytes = ContainerHeader::new().to_bytes();
        meta_file.write_all(&header_bytes).map_err(|err| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!("failed to write container header to .meta: {err}"),
            )
        })?;
        let mut claim_bytes = Vec::new();
        OwnershipRecord::OwnershipClaim(initial_claim.clone()).encode(&mut claim_bytes);
        let mut frame_buf = Vec::new();
        ContainerFrame::encode_payload(&claim_bytes, &mut frame_buf);
        meta_file.write_all(&frame_buf).map_err(|err| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!("failed to write claim frame to .meta: {err}"),
            )
        })?;
        meta_file.sync_data().map_err(|err| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!("failed to sync .meta: {err}"),
            )
        })?;

        let mut pgno_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&self.pgno_path)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    OperationFailure::new(
                        FailureCondition::StoreAlreadyExists,
                        "artefact .pgno already exists per C12.3",
                    )
                } else {
                    OperationFailure::new(
                        FailureCondition::NoArtefactExists,
                        format!("failed to create .pgno: {err}"),
                    )
                }
            })?;
        pgno_file.write_all(&header_bytes).map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("failed to write container header to .pgno: {err}"),
            )
        })?;
        pgno_file.sync_data().map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("failed to sync .pgno: {err}"),
            )
        })?;

        acquire_file_exclusion(&pgno_file, self.exclusion_policy)?;

        Ok(FileWriterSession {
            file: pgno_file,
            meta_path: self.meta_path.clone(),
            pgno_path: self.pgno_path.clone(),
            carried_epoch: initial_claim.epoch,
            claim: initial_claim.clone(),
            rolling_commitment: RollingCommitment::new(),
            exclusion_policy: self.exclusion_policy,
            locked: true,
        })
    }

    /// Creates only the .meta component of an artefact for testing incomplete creation per C5.10.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::StoreAlreadyExists`] if .meta already exists.
    pub fn create_incomplete_meta_only(
        &self,
        claim: &OwnershipClaimRecord,
    ) -> Result<(), OperationFailure> {
        let mut meta_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&self.meta_path)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    OperationFailure::new(
                        FailureCondition::StoreAlreadyExists,
                        "artefact .meta already exists per C12.3",
                    )
                } else {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to create .meta: {err}"),
                    )
                }
            })?;
        let header_bytes = ContainerHeader::new().to_bytes();
        meta_file.write_all(&header_bytes).map_err(|err| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!("failed to write container header to .meta: {err}"),
            )
        })?;
        let mut claim_bytes = Vec::new();
        OwnershipRecord::OwnershipClaim(claim.clone()).encode(&mut claim_bytes);
        let mut frame_buf = Vec::new();
        ContainerFrame::encode_payload(&claim_bytes, &mut frame_buf);
        meta_file.write_all(&frame_buf).map_err(|err| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!("failed to write claim frame to .meta: {err}"),
            )
        })?;
        meta_file.sync_data().map_err(|err| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!("failed to sync .meta: {err}"),
            )
        })?;
        Ok(())
    }

    /// Completes creation of an artefact where .meta exists without .pgno per C5.10.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if state is not incomplete creation or if creation fails.
    pub fn complete_creation(
        &self,
        claim: &OwnershipClaimRecord,
    ) -> Result<FileWriterSession, OperationFailure> {
        let presence = self.presence();
        if presence != ArtefactPresence::OwnershipRecordOnly {
            return Err(OperationFailure::new(
                FailureCondition::StoreAlreadyExists,
                "artefact creation is not in incomplete state per C5.10",
            ));
        }
        let mut pgno_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&self.pgno_path)
            .map_err(|err| {
                OperationFailure::new(
                    FailureCondition::StoreAlreadyExists,
                    format!("failed to create .pgno during completion: {err}"),
                )
            })?;
        let header_bytes = ContainerHeader::new().to_bytes();
        pgno_file.write_all(&header_bytes).map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("failed to write header to .pgno: {err}"),
            )
        })?;
        pgno_file.sync_data().map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("failed to sync .pgno: {err}"),
            )
        })?;
        acquire_file_exclusion(&pgno_file, self.exclusion_policy)?;

        Ok(FileWriterSession {
            file: pgno_file,
            meta_path: self.meta_path.clone(),
            pgno_path: self.pgno_path.clone(),
            carried_epoch: claim.epoch,
            claim: claim.clone(),
            rolling_commitment: RollingCommitment::new(),
            exclusion_policy: self.exclusion_policy,
            locked: true,
        })
    }

    /// Opens the artefact strictly for writing with a carried epoch per C5.5, C5.6, C5.62, and C12.4.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::NoArtefactExists`] if artefact is missing.
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipUnestablished`] if opening orphan .pgno.
    /// Returns [`OperationFailure`] with [`FailureCondition::StaleEpoch`] if carried epoch is superseded.
    /// Returns [`OperationFailure`] with [`FailureCondition::AnotherOwnerHoldsExclusion`] if lock is held.
    /// Returns [`OperationFailure`] with [`FailureCondition::ExclusionUnavailable`] if lock unsupported.
    pub fn open_write(&self, carried_epoch: u64) -> Result<FileWriterSession, OperationFailure> {
        match self.presence() {
            ArtefactPresence::None => {
                return Err(OperationFailure::new(
                    FailureCondition::NoArtefactExists,
                    "no artefact exists on open; strict open does not create per C5.62",
                ));
            }
            ArtefactPresence::EventDataOnly => {
                return Err(OperationFailure::new(
                    FailureCondition::OwnershipUnestablished,
                    "event data present without ownership record refused on write path per C5.10",
                ));
            }
            ArtefactPresence::OwnershipRecordOnly => {
                let meta = read_meta_records(&self.meta_path)?;
                if meta.outbound_pointer.is_some() {
                    return Err(OperationFailure::new(
                        FailureCondition::RetiredMigrationSource,
                        "artefact append authority permanently retired via outbound pointer per C5.63",
                    ));
                }
                let claim = meta.latest_claim.ok_or_else(|| {
                    OperationFailure::new(
                        FailureCondition::OwnershipUnestablished,
                        "ownership record unseeded; cannot open writer session without established claim per C5.10",
                    )
                })?;
                if claim.epoch != carried_epoch {
                    return Err(OperationFailure::new(
                        FailureCondition::StaleEpoch,
                        "stale epoch on open writer per C5.5 and C12.4",
                    ));
                }
                return self.complete_creation(&claim);
            }
            ArtefactPresence::Both => {}
        }

        let meta = read_meta_records(&self.meta_path)?;
        if meta.outbound_pointer.is_some() {
            return Err(OperationFailure::new(
                FailureCondition::RetiredMigrationSource,
                "artefact append authority permanently retired via outbound pointer per C5.63",
            ));
        }
        let claim = meta.latest_claim.ok_or_else(|| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "no ownership claim found in .meta per C5.12",
            )
        })?;
        if claim.epoch != carried_epoch {
            return Err(OperationFailure::new(
                FailureCondition::StaleEpoch,
                "carried epoch does not match recorded epoch in .meta per C5.5 and C12.4",
            ));
        }

        let mut pgno_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.pgno_path)
            .map_err(|err| {
                OperationFailure::new(
                    FailureCondition::NoArtefactExists,
                    format!("failed to open .pgno: {err}"),
                )
            })?;

        acquire_file_exclusion(&pgno_file, self.exclusion_policy)?;

        let (_, _, rolling) = read_container_frames(&mut pgno_file)?;
        pgno_file.seek(SeekFrom::End(0)).map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("failed to seek to end of .pgno: {err}"),
            )
        })?;

        Ok(FileWriterSession {
            file: pgno_file,
            meta_path: self.meta_path.clone(),
            pgno_path: self.pgno_path.clone(),
            carried_epoch,
            claim,
            rolling_commitment: rolling,
            exclusion_policy: self.exclusion_policy,
            locked: true,
        })
    }

    /// Opens the artefact strictly for reading per C5.6, C5.11, C5.62, and C6.14.
    ///
    /// Does not take writer locks, allowing concurrent readers.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::NoArtefactExists`] if artefact is missing.
    pub fn open_read(&self) -> Result<FileReaderSession, OperationFailure> {
        let presence = self.presence();
        if presence == ArtefactPresence::None {
            return Err(OperationFailure::new(
                FailureCondition::NoArtefactExists,
                "no artefact exists on open; strict open does not create per C5.62",
            ));
        }

        let meta_records = if self.meta_path.exists() {
            read_meta_records(&self.meta_path)?
        } else {
            MetaRecords::default()
        };

        let admission = admit_open(presence, meta_records.latest_claim.clone(), false)?;

        let file = if self.pgno_path.exists() {
            let mut f = OpenOptions::new()
                .read(true)
                .open(&self.pgno_path)
                .map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::NoArtefactExists,
                        format!("failed to open .pgno for reading: {err}"),
                    )
                })?;
            let _ = f.seek(SeekFrom::Start(0));
            Some(f)
        } else {
            None
        };

        Ok(FileReaderSession {
            file,
            meta_path: self.meta_path.clone(),
            pgno_path: self.pgno_path.clone(),
            admission,
            meta_records,
            rolling_commitment: RollingCommitment::new(),
        })
    }

    /// Reads all ownership records from the .meta file.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if reading fails.
    pub fn read_meta_records(&self) -> Result<MetaRecords, OperationFailure> {
        read_meta_records(&self.meta_path)
    }

    /// Appends an arbitrary ownership record to the .meta file.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_meta_record(&self, record: &OwnershipRecord) -> Result<(), OperationFailure> {
        append_meta_record(&self.meta_path, record)
    }

    /// Appends an updated ownership claim record to the .meta file.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_ownership_claim(
        &self,
        claim: &OwnershipClaimRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::OwnershipClaim(claim.clone()))
    }

    /// Appends an outbound generation pointer record to the .meta file per C6.17 and C5.63.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_outbound_pointer(
        &self,
        pointer: &OutboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::OutboundPointer(pointer.clone()))
    }

    /// Appends an inbound generation pointer record to the .meta file per C6.16.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_inbound_pointer(
        &self,
        pointer: &InboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::InboundPointer(pointer.clone()))
    }

    /// Appends a migration start record to the .meta file per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_migration_start(
        &self,
        start: &MigrationStartRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::MigrationStart(start.clone()))
    }

    /// Appends a migration end record to the .meta file per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_migration_end(&self, end: &MigrationEndRecord) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::MigrationEnd(end.clone()))
    }

    /// Appends a rescue policy choice record to the .meta file per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_rescue_policy_choice(
        &self,
        choice: &RescuePolicyChoiceRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::RescuePolicyChoice(choice.clone()))
    }

    /// Queries the outbound generation pointer if present.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if reading fails.
    pub fn outbound_pointer(&self) -> Result<Option<OutboundPointerRecord>, OperationFailure> {
        let meta = self.read_meta_records()?;
        Ok(meta.outbound_pointer)
    }

    /// Queries the inbound generation pointer if present.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if reading fails.
    pub fn inbound_pointer(&self) -> Result<Option<InboundPointerRecord>, OperationFailure> {
        let meta = self.read_meta_records()?;
        Ok(meta.inbound_pointer)
    }

    /// Returns true if this artefact is a retired migration source per C5.63.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if reading fails.
    pub fn is_retired_source(&self) -> Result<bool, OperationFailure> {
        let meta = self.read_meta_records()?;
        Ok(meta.outbound_pointer.is_some())
    }
}

/// Active writer session holding exclusive OS lock on .pgno file per C5.6, C5.65.
pub struct FileWriterSession {
    file: File,
    meta_path: PathBuf,
    pgno_path: PathBuf,
    carried_epoch: u64,
    claim: OwnershipClaimRecord,
    rolling_commitment: RollingCommitment,
    exclusion_policy: FileExclusionPolicy,
    locked: bool,
}

impl FileWriterSession {
    /// Returns the monotonic epoch carried by this writer session.
    #[must_use]
    pub fn carried_epoch(&self) -> u64 {
        self.carried_epoch
    }

    /// Returns the ownership claim record held by this writer session.
    #[must_use]
    pub fn claim(&self) -> &OwnershipClaimRecord {
        &self.claim
    }

    /// Returns the current running physical rolling commitment per C5.26.
    #[must_use]
    pub fn rolling_commitment(&self) -> &RollingCommitment {
        &self.rolling_commitment
    }

    /// Returns the path to the ownership record file (.meta).
    #[must_use]
    pub fn meta_path(&self) -> &Path {
        &self.meta_path
    }

    /// Returns the path to the event data container file (.pgno).
    #[must_use]
    pub fn pgno_path(&self) -> &Path {
        &self.pgno_path
    }

    /// Appends a framed payload byte slice to .pgno with per-landing epoch verification per C5.5 and C12.4.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::StaleEpoch`] if epoch is superseded.
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if .meta cannot be read.
    pub fn append_frame(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        let meta = read_meta_records(&self.meta_path)?;
        if meta.outbound_pointer.is_some() {
            return Err(OperationFailure::new(
                FailureCondition::RetiredMigrationSource,
                "artefact append authority permanently retired via outbound pointer per C5.63",
            ));
        }
        let latest_claim = meta.latest_claim.ok_or_else(|| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "no ownership claim found in .meta during per-landing check per C5.12",
            )
        })?;
        if latest_claim.epoch != self.carried_epoch {
            return Err(OperationFailure::new(
                FailureCondition::StaleEpoch,
                "writer epoch superseded in .meta; write rejected per C5.5 and C12.4",
            ));
        }

        let mut frame_buf = Vec::new();
        ContainerFrame::encode_payload(payload, &mut frame_buf);
        self.file.write_all(&frame_buf).map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("failed to append frame to .pgno: {err}"),
            )
        })?;
        self.file.sync_data().map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("failed to sync .pgno: {err}"),
            )
        })?;
        self.rolling_commitment.update_frame(&frame_buf);
        Ok(self.rolling_commitment.frame_count())
    }

    /// Appends an event envelope to .pgno.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if append or epoch verification fails.
    pub fn append_envelope(&mut self, envelope: &EventEnvelope) -> Result<u64, OperationFailure> {
        let mut env_buf = Vec::new();
        envelope.encode(&mut env_buf);
        self.append_frame(&env_buf)
    }

    /// Attaches and validates a schema descriptor to the artefact per C8.2.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if schema descriptor is structurally incomplete or write fails.
    pub fn set_schema_descriptor(
        &mut self,
        descriptor: &SchemaDescriptor,
    ) -> Result<(), OperationFailure> {
        descriptor.validate_structural_completeness()?;
        let mut desc_bytes = Vec::new();
        descriptor.root.encode(&mut desc_bytes);
        let record = OwnershipRecord::SchemaDescriptor {
            schema_version: descriptor.version,
            descriptor_bytes: desc_bytes,
        };
        append_meta_record(&self.meta_path, &record)
    }

    /// Flushes and syncs all pending writes to disk.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if sync fails.
    pub fn sync(&mut self) -> Result<(), OperationFailure> {
        self.file.sync_data().map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("failed to sync .pgno: {err}"),
            )
        })
    }

    /// Appends an arbitrary ownership record to the .meta file.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_meta_record(&mut self, record: &OwnershipRecord) -> Result<(), OperationFailure> {
        append_meta_record(&self.meta_path, record)
    }

    /// Appends an outbound generation pointer record to the .meta file per C6.17 and C5.63.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_outbound_pointer(
        &mut self,
        pointer: &OutboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::OutboundPointer(pointer.clone()))
    }

    /// Appends an inbound generation pointer record to the .meta file per C6.16.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_inbound_pointer(
        &mut self,
        pointer: &InboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::InboundPointer(pointer.clone()))
    }

    /// Appends a migration start record to the .meta file per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_migration_start(
        &mut self,
        start: &MigrationStartRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::MigrationStart(start.clone()))
    }

    /// Appends a migration end record to the .meta file per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_migration_end(
        &mut self,
        end: &MigrationEndRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::MigrationEnd(end.clone()))
    }

    /// Appends a rescue policy choice record to the .meta file per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_rescue_policy_choice(
        &mut self,
        choice: &RescuePolicyChoiceRecord,
    ) -> Result<(), OperationFailure> {
        self.record_meta_record(&OwnershipRecord::RescuePolicyChoice(choice.clone()))
    }
}

impl fmt::Debug for FileWriterSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileWriterSession")
            .field("meta_path", &self.meta_path)
            .field("pgno_path", &self.pgno_path)
            .field("carried_epoch", &self.carried_epoch)
            .field("claim", &self.claim)
            .field("exclusion_policy", &self.exclusion_policy)
            .field("locked", &self.locked)
            .finish()
    }
}

impl Drop for FileWriterSession {
    fn drop(&mut self) {
        if self.locked && self.exclusion_policy == FileExclusionPolicy::Standard {
            let _ = self.file.unlock();
        }
    }
}

/// Reader session providing non-exclusive read-only access to an artefact container per C5.6, C5.11, and C6.14.
pub struct FileReaderSession {
    file: Option<File>,
    meta_path: PathBuf,
    pgno_path: PathBuf,
    admission: OpenAdmission,
    meta_records: MetaRecords,
    rolling_commitment: RollingCommitment,
}

impl fmt::Debug for FileReaderSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileReaderSession")
            .field("meta_path", &self.meta_path)
            .field("pgno_path", &self.pgno_path)
            .field("admission", &self.admission)
            .field("meta_records", &self.meta_records)
            .finish()
    }
}

impl FileReaderSession {
    /// Returns the path to the ownership record file (.meta).
    #[must_use]
    pub fn meta_path(&self) -> &Path {
        &self.meta_path
    }

    /// Returns the path to the event data container file (.pgno).
    #[must_use]
    pub fn pgno_path(&self) -> &Path {
        &self.pgno_path
    }

    /// Returns the open admission status for this reader session.
    #[must_use]
    pub fn admission(&self) -> &OpenAdmission {
        &self.admission
    }

    /// Returns all meta records decoded from .meta.
    #[must_use]
    pub fn meta_records(&self) -> &MetaRecords {
        &self.meta_records
    }

    /// Returns the recorded ownership claim if present.
    #[must_use]
    pub fn claim(&self) -> Option<&OwnershipClaimRecord> {
        self.meta_records.latest_claim.as_ref()
    }

    /// Returns the recorded schema descriptor if present.
    #[must_use]
    pub fn schema_descriptor(&self) -> Option<&SchemaDescriptor> {
        self.meta_records.schema_descriptor.as_ref()
    }

    /// Returns the outbound generation pointer if present.
    #[must_use]
    pub fn outbound_pointer(&self) -> Option<&OutboundPointerRecord> {
        self.meta_records.outbound_pointer.as_ref()
    }

    /// Returns the inbound generation pointer if present.
    #[must_use]
    pub fn inbound_pointer(&self) -> Option<&InboundPointerRecord> {
        self.meta_records.inbound_pointer.as_ref()
    }

    /// Returns the migration start record if present.
    #[must_use]
    pub fn migration_start(&self) -> Option<&MigrationStartRecord> {
        self.meta_records.migration_start.as_ref()
    }

    /// Returns the migration end record if present.
    #[must_use]
    pub fn migration_end(&self) -> Option<&MigrationEndRecord> {
        self.meta_records.migration_end.as_ref()
    }

    /// Returns the rescue policy choice record if present.
    #[must_use]
    pub fn rescue_policy_choice(&self) -> Option<&RescuePolicyChoiceRecord> {
        self.meta_records.rescue_policy_choice.as_ref()
    }

    /// Returns true if this artefact is a retired migration source per C5.63.
    #[must_use]
    pub fn is_retired_source(&self) -> bool {
        self.meta_records.outbound_pointer.is_some()
    }

    /// Returns the running physical rolling commitment computed across read frames.
    #[must_use]
    pub fn rolling_commitment(&self) -> &RollingCommitment {
        &self.rolling_commitment
    }

    /// Reads all framed payloads from the .pgno container file, validating CRC32C on each frame.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::PrecursorChainBroken`] if checksum fails or file corrupted.
    pub fn read_all_frames(&mut self) -> Result<Vec<Vec<u8>>, OperationFailure> {
        let Some(file) = &mut self.file else {
            return Ok(Vec::new());
        };
        let (_, frames, rolling) = read_container_frames(file)?;
        self.rolling_commitment = rolling;
        Ok(frames)
    }

    /// Reads and decodes all event envelopes from the .pgno container file.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if frame checksum fails or envelope decode fails.
    pub fn read_all_envelopes(&mut self) -> Result<Vec<EventEnvelope>, OperationFailure> {
        let frames = self.read_all_frames()?;
        let mut envelopes = Vec::with_capacity(frames.len());
        for frame in &frames {
            let (env, _) = EventEnvelope::decode(frame).map_err(|err| {
                OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    format!("failed to decode event envelope: {err}"),
                )
            })?;
            envelopes.push(env);
        }
        Ok(envelopes)
    }

    /// Validates structural completeness of the artefact's schema descriptor per C8.2.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::MissingSchemaDescriptor`] if descriptor is absent.
    /// Returns [`OperationFailure`] with [`FailureCondition::ValueConstraintViolated`] if descriptor is invalid.
    pub fn validate_schema_completeness(&self) -> Result<(), OperationFailure> {
        match self.schema_descriptor() {
            Some(descriptor) => descriptor.validate_structural_completeness(),
            None => Err(OperationFailure::new(
                FailureCondition::MissingSchemaDescriptor,
                "artefact schema descriptor is absent per C8.2",
            )),
        }
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
