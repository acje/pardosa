//! Container format and framing for Pardosa artefacts.

use crate::encoding::{
    DecodeError, EventEnvelope, InboundPointerRecord, MigrationEndRecord, MigrationStartRecord,
    OutboundPointerRecord, OwnershipClaimRecord, OwnershipRecord, RescuePolicyChoiceRecord,
};
use crate::schema::{derive_fiber_id, DescriptorNode, SchemaDescriptor};
use crate::store::{
    admit_create, admit_open, ArtefactPresence, FailureCondition, FiberHandle, OpenAdmission,
    OperationFailure, SessionIndex, WriteLandingVerdict,
};
use std::collections::HashSet;
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
            fiber_index: SessionIndex::new(),
            seen_events: HashSet::new(),
            uncertain: false,
            uncertain_diagnostic: None,
            simulate_indeterminate: false,
            simulate_sync_error: false,
            simulate_write_error: false,
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
            fiber_index: SessionIndex::new(),
            seen_events: HashSet::new(),
            uncertain: false,
            uncertain_diagnostic: None,
            simulate_indeterminate: false,
            simulate_sync_error: false,
            simulate_write_error: false,
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

        let (_, frames, rolling) = read_container_frames(&mut pgno_file)?;
        let fiber_index = SessionIndex::build_from_frames(frames.iter().map(|f| f.as_slice()))?;
        let mut seen_events = HashSet::new();
        for frame in &frames {
            if frame.len() >= 85 {
                if let Ok((env, consumed)) = EventEnvelope::decode(frame) {
                    if consumed == frame.len() {
                        seen_events.insert(env.header.event_id);
                    }
                }
            }
        }
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
            fiber_index,
            seen_events,
            uncertain: false,
            uncertain_diagnostic: None,
            simulate_indeterminate: false,
            simulate_sync_error: false,
            simulate_write_error: false,
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

        let (file, fiber_index, failed) = if self.pgno_path.exists() {
            let mut f = OpenOptions::new()
                .read(true)
                .open(&self.pgno_path)
                .map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::NoArtefactExists,
                        format!("failed to open .pgno for reading: {err}"),
                    )
                })?;
            let (_, frames, _) = read_container_frames(&mut f)?;
            let (fiber_index, failed) =
                match SessionIndex::build_from_frames(frames.iter().map(|f| f.as_slice())) {
                    Ok(idx) => (idx, None),
                    Err(err) => (SessionIndex::new(), Some(err.condition().clone())),
                };
            f.seek(SeekFrom::Start(0)).map_err(|err| {
                OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    format!("failed to seek .pgno to start: {err}"),
                )
            })?;
            (Some(f), fiber_index, failed)
        } else {
            (None, SessionIndex::new(), None)
        };

        Ok(FileReaderSession {
            file,
            meta_path: self.meta_path.clone(),
            pgno_path: self.pgno_path.clone(),
            admission,
            meta_records,
            rolling_commitment: RollingCommitment::new(),
            fiber_index,
            failed,
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
    fiber_index: SessionIndex,
    seen_events: HashSet<[u8; 16]>,
    uncertain: bool,
    uncertain_diagnostic: Option<String>,
    simulate_indeterminate: bool,
    simulate_sync_error: bool,
    simulate_write_error: bool,
}

impl FileWriterSession {
    /// Returns the monotonic epoch carried by this writer session.
    #[must_use]
    pub fn carried_epoch(&self) -> u64 {
        self.carried_epoch
    }

    /// Returns the diagnostic detail if the writer entered an uncertain state.
    #[must_use]
    pub fn uncertain_diagnostic(&self) -> Option<&str> {
        self.uncertain_diagnostic.as_deref()
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

    /// Configures whether to simulate an indeterminate write landing per C5.16.
    #[cfg(any(test, feature = "unstable-test-support"))]
    #[must_use]
    pub fn with_simulate_indeterminate(mut self, simulate: bool) -> Self {
        self.simulate_indeterminate = simulate;
        self
    }

    /// Configures whether to simulate a sync_data failure on write.
    #[cfg(any(test, feature = "unstable-test-support"))]
    #[must_use]
    pub fn with_simulate_sync_error(mut self, simulate: bool) -> Self {
        self.simulate_sync_error = simulate;
        self
    }

    /// Configures whether to simulate a write_all failure on write.
    #[cfg(any(test, feature = "unstable-test-support"))]
    #[must_use]
    pub fn with_simulate_write_error(mut self, simulate: bool) -> Self {
        self.simulate_write_error = simulate;
        self
    }

    fn check_authority(&self) -> Result<(), OperationFailure> {
        if self.uncertain {
            return Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "writer session in uncertain state; reconciliation required",
            ));
        }
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
        Ok(())
    }

    fn append_frame_raw(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        self.check_authority()?;

        let mut frame_buf = Vec::new();
        ContainerFrame::encode_payload(payload, &mut frame_buf);
        let write_res = if self.simulate_write_error {
            Err(std::io::Error::other("simulated write_all failure"))
        } else {
            self.file.write_all(&frame_buf)
        };
        write_res.map_err(|err| {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(format!(
                "write_all failed; write landing undetermined: {err}"
            ));
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!("write landing undetermined on write_all failure: {err}"),
            )
        })?;
        let sync_res = if self.simulate_sync_error {
            Err(std::io::Error::other("simulated sync_data failure"))
        } else {
            self.file.sync_data()
        };
        sync_res.map_err(|err| {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(format!(
                "sync_data failed; write landing undetermined: {err}"
            ));
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!("write landing undetermined on sync_data failure: {err}"),
            )
        })?;
        self.rolling_commitment.update_frame(&frame_buf);
        Ok(self.rolling_commitment.frame_count())
    }

    /// Appends a framed payload byte slice to .pgno with per-landing epoch verification per C5.5 and C12.4.
    ///
    /// [`WriteLandingVerdict`] is the authoritative C5.16 outcome returned by [`Self::append_to_fiber`],
    /// [`Self::detach_fiber`], [`Self::rescue_fiber`], [`Self::append_frame_verdict`], and
    /// [`Self::append_envelope_verdict`].
    ///
    /// This method is a convenience wrapper returning `Result<u64, OperationFailure>` (unwrapping
    /// [`WriteLandingVerdict::Landed`], mapping [`WriteLandingVerdict::Undetermined`] to
    /// [`FailureCondition::OwnershipRecordUnreadable`]), matching the established `pardosa-nats`
    /// API contract since 0.5.1.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::StaleEpoch`] if epoch is superseded.
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if write landing is undetermined.
    pub fn append_frame(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        match self.append_frame_verdict(payload)? {
            WriteLandingVerdict::Landed(count) => Ok(count),
            WriteLandingVerdict::Undetermined { .. } => Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "write landing undetermined; session in uncertain state",
            )),
        }
    }

    /// Appends a framed payload byte slice, returning a [`WriteLandingVerdict`] explicitly handling indeterminate landings per C5.16.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::StaleEpoch`] if epoch is superseded.
    /// Returns [`OperationFailure`] with [`FailureCondition::EnvelopeMismatch`] if frame decode or validation fails.
    pub fn append_frame_verdict(
        &mut self,
        payload: &[u8],
    ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        self.check_authority()?;

        if payload.len() < 85 {
            return Err(OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                format!("payload too short for event envelope: {}", payload.len()),
            ));
        }
        let (env, consumed) = EventEnvelope::decode(payload).map_err(|err| {
            OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                format!("failed to decode envelope: {err}"),
            )
        })?;
        if consumed != payload.len() {
            return Err(OperationFailure::new(
                FailureCondition::EnvelopeMismatch,
                format!(
                    "frame decode error: {}",
                    DecodeError::TruncatedPayload {
                        expected: payload.len(),
                        available: consumed,
                    }
                ),
            ));
        }
        let reservation = self.fiber_index.prepare_append(&env)?;
        let event_id = env.header.event_id;

        if self.simulate_indeterminate {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(
                "write landing undetermined: simulated indeterminate write landing".to_string(),
            );
            return Ok(WriteLandingVerdict::Undetermined {
                carried_epoch: self.carried_epoch,
            });
        }

        let mut frame_buf = Vec::new();
        ContainerFrame::encode_payload(payload, &mut frame_buf);

        let write_res = if self.simulate_write_error {
            Err(std::io::Error::other("simulated write_all failure"))
        } else {
            self.file.write_all(&frame_buf)
        };

        if let Err(err) = write_res {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(format!(
                "write_all failed; write landing undetermined: {err}"
            ));
            return Ok(WriteLandingVerdict::Undetermined {
                carried_epoch: self.carried_epoch,
            });
        }

        let sync_res = if self.simulate_sync_error {
            Err(std::io::Error::other("simulated sync_data failure"))
        } else {
            self.file.sync_data()
        };

        if let Err(err) = sync_res {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(format!(
                "sync_data failed; write landing undetermined: {err}"
            ));
            return Ok(WriteLandingVerdict::Undetermined {
                carried_epoch: self.carried_epoch,
            });
        }

        self.rolling_commitment.update_frame(&frame_buf);
        self.fiber_index.commit_append(reservation)?;
        self.seen_events.insert(event_id);
        Ok(WriteLandingVerdict::Landed(
            self.rolling_commitment.frame_count(),
        ))
    }

    /// Appends an event envelope returning a [`WriteLandingVerdict`].
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if candidate validation, append, or epoch verification fails.
    pub fn append_envelope_verdict(
        &mut self,
        envelope: &EventEnvelope,
    ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        let mut env_buf = Vec::new();
        envelope.encode(&mut env_buf);
        self.append_frame_verdict(&env_buf)
    }

    /// Appends an event envelope to .pgno after pre-landing validation against the session index.
    ///
    /// [`WriteLandingVerdict`] is the authoritative C5.16 outcome returned by [`Self::append_to_fiber`],
    /// [`Self::detach_fiber`], [`Self::rescue_fiber`], [`Self::append_frame_verdict`], and
    /// [`Self::append_envelope_verdict`].
    ///
    /// This method is a convenience wrapper returning `Result<u64, OperationFailure>` (unwrapping
    /// [`WriteLandingVerdict::Landed`], mapping [`WriteLandingVerdict::Undetermined`] to
    /// [`FailureCondition::OwnershipRecordUnreadable`]), matching the established `pardosa-nats`
    /// API contract since 0.5.1.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if candidate validation, append, or epoch verification fails.
    pub fn append_envelope(&mut self, envelope: &EventEnvelope) -> Result<u64, OperationFailure> {
        match self.append_envelope_verdict(envelope)? {
            WriteLandingVerdict::Landed(count) => Ok(count),
            WriteLandingVerdict::Undetermined { .. } => Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "write landing undetermined; session in uncertain state",
            )),
        }
    }

    /// Appends a raw frame payload to the container.
    ///
    /// If the payload decodes to an exact [`EventEnvelope`], it undergoes session index pre-admission validation.
    /// Otherwise, the raw frame is appended and unindexes point lookups.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if authority check fails, pre-admission validation fails, or write/sync fails.
    pub fn append_raw_frame(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        self.check_authority()?;

        if payload.len() >= 85 {
            if let Ok((env, consumed)) = EventEnvelope::decode(payload) {
                if consumed == payload.len() {
                    let reservation = self.fiber_index.prepare_append(&env)?;
                    let count = self.append_frame_raw(payload)?;
                    self.fiber_index.commit_append(reservation)?;
                    self.seen_events.insert(env.header.event_id);
                    return Ok(count);
                }
            }
        }

        let count = self.append_frame_raw(payload)?;
        self.fiber_index.mark_has_raw_frames();
        Ok(count)
    }

    /// Appends an unvalidated raw frame directly to the container for testing or migration recovery.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if authority check fails, session is uncertain, or write/sync fails.
    #[doc(hidden)]
    pub fn append_unvalidated_frame(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        let count = self.append_frame_raw(payload)?;
        self.fiber_index.mark_has_raw_frames();
        Ok(count)
    }

    /// Returns a [`FiberHandle`] for the specified fiber identifier.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if the fiber is broken.
    pub fn fiber(&self, fiber_id: [u8; 16]) -> Result<FiberHandle, OperationFailure> {
        if self.uncertain {
            return Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "writer session in uncertain state; reconciliation required",
            ));
        }
        self.fiber_index.fiber(fiber_id)
    }

    /// Returns a [`FiberHandle`] derived from a domain key string.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if the fiber is broken.
    pub fn fiber_with_key(&self, domain_key: &str) -> Result<FiberHandle, OperationFailure> {
        self.fiber(derive_fiber_id(domain_key))
    }

    /// Returns the latest event envelope recorded for the specified fiber identifier.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if the fiber is broken.
    pub fn get_latest(
        &self,
        fiber_id: [u8; 16],
    ) -> Result<Option<&EventEnvelope>, OperationFailure> {
        if self.uncertain {
            return Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "writer session in uncertain state; reconciliation required",
            ));
        }
        self.fiber_index.get_latest(&fiber_id)
    }

    /// Returns the latest event envelope recorded for the specified domain key.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if the fiber is broken.
    pub fn get_latest_with_key(
        &self,
        domain_key: &str,
    ) -> Result<Option<&EventEnvelope>, OperationFailure> {
        self.get_latest(derive_fiber_id(domain_key))
    }

    /// Appends an event to the specified fiber, minting an envelope and advancing fiber state.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if append is rejected by fiber lifecycle or underlying storage fails.
    pub fn append_to_fiber(
        &mut self,
        fiber_id: [u8; 16],
        event_id: [u8; 16],
        payload: impl Into<Vec<u8>>,
    ) -> Result<WriteLandingVerdict<EventEnvelope>, OperationFailure> {
        self.check_authority()?;
        if self.seen_events.contains(&event_id) {
            return Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "duplicate event ID observed across writer session per C5.61",
            ));
        }
        let mut handle = self.fiber(fiber_id)?;
        let envelope = handle.append(event_id, payload)?;
        let mut env_buf = Vec::new();
        envelope.encode(&mut env_buf);
        let verdict = self.append_frame_verdict(&env_buf)?;
        match verdict {
            WriteLandingVerdict::Landed(_) => Ok(WriteLandingVerdict::Landed(envelope)),
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                Ok(WriteLandingVerdict::Undetermined { carried_epoch })
            }
        }
    }

    /// Detaches the specified fiber, minting a detached envelope and recording soft deletion.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if detach is rejected by fiber lifecycle or underlying storage fails.
    pub fn detach_fiber(
        &mut self,
        fiber_id: [u8; 16],
        event_id: [u8; 16],
        payload: impl Into<Vec<u8>>,
    ) -> Result<WriteLandingVerdict<EventEnvelope>, OperationFailure> {
        self.check_authority()?;
        if self.seen_events.contains(&event_id) {
            return Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "duplicate event ID observed across writer session per C5.61",
            ));
        }
        let mut handle = self.fiber(fiber_id)?;
        let envelope = handle.detach(event_id, payload)?;
        let mut env_buf = Vec::new();
        envelope.encode(&mut env_buf);
        let verdict = self.append_frame_verdict(&env_buf)?;
        match verdict {
            WriteLandingVerdict::Landed(_) => Ok(WriteLandingVerdict::Landed(envelope)),
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                Ok(WriteLandingVerdict::Undetermined { carried_epoch })
            }
        }
    }

    /// Rescues the specified detached or locked fiber, returning it to active state.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if rescue is rejected by fiber lifecycle or underlying storage fails.
    pub fn rescue_fiber(
        &mut self,
        fiber_id: [u8; 16],
        event_id: [u8; 16],
        payload: impl Into<Vec<u8>>,
    ) -> Result<WriteLandingVerdict<EventEnvelope>, OperationFailure> {
        self.check_authority()?;
        if self.seen_events.contains(&event_id) {
            return Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                "duplicate event ID observed across writer session per C5.61",
            ));
        }
        let mut handle = self.fiber(fiber_id)?;
        let envelope = handle.rescue(event_id, payload)?;
        let mut env_buf = Vec::new();
        envelope.encode(&mut env_buf);
        let verdict = self.append_frame_verdict(&env_buf)?;
        match verdict {
            WriteLandingVerdict::Landed(_) => Ok(WriteLandingVerdict::Landed(envelope)),
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                Ok(WriteLandingVerdict::Undetermined { carried_epoch })
            }
        }
    }

    /// Attaches and validates a schema descriptor to the artefact per C8.2.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if schema descriptor is structurally incomplete or write fails.
    pub fn set_schema_descriptor(
        &mut self,
        descriptor: &SchemaDescriptor,
    ) -> Result<(), OperationFailure> {
        self.check_authority()?;
        descriptor.validate_structural_completeness()?;
        let mut desc_bytes = Vec::new();
        descriptor.root.encode(&mut desc_bytes);
        let record = OwnershipRecord::SchemaDescriptor {
            schema_version: descriptor.version,
            descriptor_bytes: desc_bytes,
        };
        append_meta_record(&self.meta_path, &record).inspect_err(|err| {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(format!(
                "metadata write failed; write landing undetermined: {err}"
            ));
        })
    }

    /// Flushes and syncs all pending writes to disk.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if sync fails.
    pub fn sync(&mut self) -> Result<(), OperationFailure> {
        self.check_authority()?;
        self.file.sync_data().map_err(|err| {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(format!(
                "sync_data failed; write landing undetermined: {err}"
            ));
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
        self.check_authority()?;
        append_meta_record(&self.meta_path, record).inspect_err(|err| {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(format!(
                "metadata write failed; write landing undetermined: {err}"
            ));
        })
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
    fiber_index: SessionIndex,
    failed: Option<FailureCondition>,
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
        let res = read_container_frames(file);
        match res {
            Ok((_, frames, rolling)) => {
                self.rolling_commitment = rolling;
                Ok(frames)
            }
            Err(err) => {
                self.failed = Some(err.condition().clone());
                Err(err)
            }
        }
    }

    fn read_all_envelopes_inner(
        &mut self,
        for_migration: bool,
    ) -> Result<Vec<EventEnvelope>, OperationFailure> {
        let res = (|| {
            let frames = self.read_all_frames()?;
            for frame in &frames {
                if frame.len() < 85 {
                    return Err(OperationFailure::new(
                        FailureCondition::EnvelopeMismatch,
                        format!(
                            "frame decode error: {}",
                            DecodeError::TruncatedPayload {
                                expected: 85,
                                available: frame.len(),
                            }
                        ),
                    ));
                }
            }
            let session_index =
                SessionIndex::build_from_frames(frames.iter().map(|f| f.as_slice()))?;
            if !for_migration && session_index.has_broken_fibers() {
                self.fiber_index = session_index;
                return Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    "discovered break in precursor chain",
                ));
            }
            let envelopes = frames
                .iter()
                .map(|frame| SessionIndex::decode_and_validate_frame(frame))
                .collect::<Result<Vec<_>, _>>()?;
            self.fiber_index = session_index;
            Ok(envelopes)
        })();

        if let Err(ref err) = res {
            self.failed = Some(err.condition().clone());
        }
        res
    }

    /// Reads and decodes all event envelopes from the .pgno container file.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if frame checksum fails, frame is shorter than 85 bytes,
    /// envelope decode fails, or precursor chain is broken.
    pub fn read_all_envelopes(&mut self) -> Result<Vec<EventEnvelope>, OperationFailure> {
        self.read_all_envelopes_inner(false)
    }

    /// Reads and decodes all event envelopes for migration, permitting broken history per C5.28.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if frame checksum fails or envelope decode fails.
    pub fn read_all_envelopes_for_migration(
        &mut self,
    ) -> Result<Vec<EventEnvelope>, OperationFailure> {
        self.read_all_envelopes_inner(true)
    }

    /// Returns the latest event envelope recorded for the specified fiber identifier.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if the fiber is broken or reader session has failed.
    pub fn get_latest(
        &self,
        fiber_id: [u8; 16],
    ) -> Result<Option<&EventEnvelope>, OperationFailure> {
        if let Some(cond) = &self.failed {
            return Err(OperationFailure::new(
                cond.clone(),
                "reader in failed state",
            ));
        }
        self.fiber_index.get_latest(&fiber_id)
    }

    /// Returns the latest event envelope recorded for the specified domain key.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if the fiber is broken or reader session has failed.
    pub fn get_latest_with_key(
        &self,
        domain_key: &str,
    ) -> Result<Option<&EventEnvelope>, OperationFailure> {
        self.get_latest(derive_fiber_id(domain_key))
    }

    /// Returns a [`FiberHandle`] for the specified fiber identifier.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if the fiber is broken or reader session has failed.
    pub fn fiber(&self, fiber_id: [u8; 16]) -> Result<FiberHandle, OperationFailure> {
        if let Some(cond) = &self.failed {
            return Err(OperationFailure::new(
                cond.clone(),
                "reader in failed state",
            ));
        }
        self.fiber_index.fiber(fiber_id)
    }

    /// Returns a [`FiberHandle`] derived from a domain key string.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if the fiber is broken.
    pub fn fiber_with_key(&self, domain_key: &str) -> Result<FiberHandle, OperationFailure> {
        self.fiber(derive_fiber_id(domain_key))
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
    use crate::store::FiberState;

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

    #[test]
    fn test_file_session_fiber_handle_and_point_lookup() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("test_fiber_point_lookup");
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
        let fiber_id = [0x42; 16];
        let domain_key = "user:42";
        let derived_id = derive_fiber_id(domain_key);

        assert!(writer.get_latest(fiber_id).expect("get latest").is_none());
        assert!(writer
            .get_latest_with_key(domain_key)
            .expect("get latest with key")
            .is_none());

        let h_initial = writer.fiber(fiber_id).expect("fiber");
        assert_eq!(h_initial.state(), FiberState::Undefined);

        let env1 = match writer
            .append_to_fiber(fiber_id, [0x01; 16], b"payload-1")
            .expect("append to fiber")
        {
            WriteLandingVerdict::Landed(e) => e,
            WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
        };
        assert_eq!(env1.header.fiber_id, fiber_id);
        assert_eq!(env1.header.event_id, [0x01; 16]);

        let latest = writer
            .get_latest(fiber_id)
            .expect("get latest")
            .expect("some");
        assert_eq!(latest.header.event_id, [0x01; 16]);
        assert_eq!(latest.payload, b"payload-1");

        let h_defined = writer.fiber(fiber_id).expect("fiber");
        assert_eq!(h_defined.state(), FiberState::Defined);
        assert_eq!(h_defined.precursor(), [0x01; 16]);
        assert_eq!(h_defined.precursor_hash(), env1.commitment());

        let env_key = match writer
            .append_to_fiber(derived_id, [0x02; 16], b"payload-key")
            .expect("append with key")
        {
            WriteLandingVerdict::Landed(e) => e,
            WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
        };
        let latest_key = writer
            .get_latest_with_key(domain_key)
            .expect("get latest with key")
            .expect("some");
        assert_eq!(latest_key.header.event_id, [0x02; 16]);
        assert_eq!(latest_key.commitment(), env_key.commitment());

        let h_key = writer.fiber_with_key(domain_key).expect("fiber with key");
        assert_eq!(h_key.state(), FiberState::Defined);
        assert_eq!(h_key.fiber_id(), derived_id);

        let env_detach = match writer
            .detach_fiber(fiber_id, [0x03; 16], b"payload-detach")
            .expect("detach fiber")
        {
            WriteLandingVerdict::Landed(e) => e,
            WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
        };
        assert!(env_detach.header.detached);
        let h_detached = writer.fiber(fiber_id).expect("fiber");
        assert_eq!(h_detached.state(), FiberState::Detached);

        let env_rescue = match writer
            .rescue_fiber(fiber_id, [0x04; 16], b"payload-rescue")
            .expect("rescue fiber")
        {
            WriteLandingVerdict::Landed(e) => e,
            WriteLandingVerdict::Undetermined { .. } => panic!("expected landed"),
        };
        assert!(!env_rescue.header.detached);
        let h_rescued = writer.fiber(fiber_id).expect("fiber");
        assert_eq!(h_rescued.state(), FiberState::Defined);
        assert_eq!(h_rescued.precursor(), [0x04; 16]);

        drop(writer);

        let reader = adapter.open_read().expect("open reader");
        let r_latest = reader
            .get_latest(fiber_id)
            .expect("reader get latest")
            .expect("some");
        assert_eq!(r_latest.header.event_id, [0x04; 16]);

        let r_latest_key = reader
            .get_latest_with_key(domain_key)
            .expect("reader get latest key")
            .expect("some");
        assert_eq!(r_latest_key.header.event_id, [0x02; 16]);

        let r_fiber = reader.fiber(fiber_id).expect("fiber");
        assert_eq!(r_fiber.state(), FiberState::Defined);
        assert_eq!(r_fiber.precursor(), [0x04; 16]);

        let r_fiber_key = reader.fiber_with_key(domain_key).expect("fiber with key");
        assert_eq!(r_fiber_key.state(), FiberState::Defined);
        assert_eq!(r_fiber_key.fiber_id(), derived_id);
    }

    #[test]
    fn test_pre_landing_admission_rejection_leaves_file_unchanged() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("pre_landing_unchanged");
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
        let fiber_id = [0x99; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"initial").unwrap();
        writer.append_envelope(&genesis).expect("append genesis");
        let file_len_before = std::fs::metadata(writer.pgno_path()).unwrap().len();

        let dup_genesis = EventEnvelope::genesis([0x02; 16], fiber_id, b"dup").unwrap();
        let err = writer.append_envelope(&dup_genesis).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );

        let file_len_after = std::fs::metadata(writer.pgno_path()).unwrap().len();
        assert_eq!(
            file_len_before, file_len_after,
            "file length must be unchanged on pre-landing validation failure"
        );
    }

    #[test]
    fn test_append_raw_frame_envelope_pre_landing_admission_and_refusal() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("h3_raw_admission");
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
        let fiber_id = [0x77; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"initial").unwrap();
        let mut gen_bytes = Vec::new();
        genesis.encode(&mut gen_bytes);

        writer
            .append_raw_frame(&gen_bytes)
            .expect("append valid raw envelope");
        assert_eq!(
            writer
                .get_latest(fiber_id)
                .unwrap()
                .unwrap()
                .header
                .event_id,
            [0x01; 16]
        );
        let file_len_before = std::fs::metadata(writer.pgno_path()).unwrap().len();

        let dup_genesis = EventEnvelope::genesis([0x02; 16], fiber_id, b"dup").unwrap();
        let mut dup_bytes = Vec::new();
        dup_genesis.encode(&mut dup_bytes);

        let err = writer.append_raw_frame(&dup_bytes).unwrap_err();
        assert_eq!(
            *err.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );

        let file_len_after = std::fs::metadata(writer.pgno_path()).unwrap().len();
        assert_eq!(file_len_before, file_len_after);

        let child = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x02; 16],
                fiber_id,
                detached: false,
                precursor: genesis.header.event_id,
                precursor_hash: genesis.commitment(),
            },
            payload: b"child".to_vec(),
        };
        let mut child_bytes = Vec::new();
        child.encode(&mut child_bytes);

        writer
            .append_raw_frame(&child_bytes)
            .expect("append valid raw child");
        assert_eq!(
            writer
                .get_latest(fiber_id)
                .unwrap()
                .unwrap()
                .header
                .event_id,
            [0x02; 16]
        );
        let handle = writer.fiber(fiber_id).unwrap();
        assert_eq!(handle.event_count(), 2);
    }

    #[test]
    fn test_append_frame_rejects_malformed_boolean_discriminant() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("malformed_bool");
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
        let fiber_id = [0x55; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"initial").unwrap();
        let mut bytes = Vec::new();
        genesis.encode(&mut bytes);
        assert!(bytes.len() >= 85);
        bytes[32] = 2;
        let err = writer
            .append_frame(&bytes)
            .expect_err("malformed boolean discriminant in envelope payload must be rejected");
        assert_eq!(*err.condition(), FailureCondition::EnvelopeMismatch);
    }

    #[test]
    fn test_reader_retains_broken_slot_after_read_all_envelopes_error() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("reader_retains_broken");
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
        let fiber_id = [0x77; 16];
        let genesis = EventEnvelope::genesis([0x01; 16], fiber_id, b"initial").unwrap();
        writer.append_envelope(&genesis).expect("append genesis");

        let mut reader = adapter.open_read().expect("open reader");
        let handle = reader.fiber(fiber_id).expect("reader initial point lookup");
        assert_eq!(handle.state(), FiberState::Defined);

        let broken_env = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x02; 16],
                fiber_id,
                detached: false,
                precursor: [0x99; 16],
                precursor_hash: [0xaa; 32],
            },
            payload: b"broken-precursor".to_vec(),
        };
        let mut broken_bytes = Vec::new();
        broken_env.encode(&mut broken_bytes);
        writer
            .append_frame_raw(&broken_bytes)
            .expect("append raw frame");

        let read_err = reader.read_all_envelopes().unwrap_err();
        assert_eq!(
            *read_err.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );

        let lookup_err = reader
            .fiber(fiber_id)
            .expect_err("reader must retain broken fiber state");
        assert_eq!(
            *lookup_err.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
    }

    #[test]
    fn test_append_frame_rejects_short_payload_under_85_bytes() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("short_payload");
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
        let short_payload = [0u8; 84];
        let err = writer
            .append_frame(&short_payload)
            .expect_err("84-byte payload must be rejected by append_frame per H2");
        assert_eq!(*err.condition(), FailureCondition::EnvelopeMismatch);
        assert!(err
            .to_string()
            .contains("payload too short for event envelope: 84"));
    }

    #[test]
    fn test_writer_rejects_set_schema_descriptor_while_uncertain() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("uncertain_schema");
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
        writer.uncertain = true;
        let descriptor = crate::schema::SchemaDescriptor::new(
            1,
            crate::schema::DescriptorNode::Struct {
                name: "OrderPayload".to_string(),
                fields: vec![crate::schema::FieldDescriptor {
                    name: "id".to_string(),
                    node: crate::schema::DescriptorNode::Uuid,
                }],
            },
        );
        let err = writer
            .set_schema_descriptor(&descriptor)
            .expect_err("set_schema_descriptor must be rejected while uncertain per M3");
        assert_eq!(
            *err.condition(),
            FailureCondition::OwnershipRecordUnreadable
        );
        assert!(err
            .to_string()
            .contains("writer session in uncertain state; reconciliation required"));
    }

    #[test]
    fn test_file_reader_retains_failed_state_refusing_point_lookups() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("reader_failed_retention");
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
            .append_raw_frame(b"raw-non-envelope")
            .expect("append raw frame");

        let reader = adapter.open_read().expect("open reader with raw frames");
        assert!(reader.failed.is_some());
        let err = reader.get_latest([0x11; 16]).unwrap_err();
        assert_eq!(*err.condition(), FailureCondition::EnvelopeMismatch);
        assert!(err.to_string().contains("reader in failed state"));

        let err_fiber = reader.fiber([0x11; 16]).unwrap_err();
        assert_eq!(*err_fiber.condition(), FailureCondition::EnvelopeMismatch);
        assert!(err_fiber.to_string().contains("reader in failed state"));
    }

    #[test]
    fn test_raw_frame_invalidates_ordinary_point_lookup() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("raw_invalidates_lookup");
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
        let fiber_id = [0x22; 16];

        writer
            .append_raw_frame(b"unindexed-raw-frame")
            .expect("append raw frame");

        let err_latest = writer.get_latest(fiber_id).unwrap_err();
        assert_eq!(*err_latest.condition(), FailureCondition::EnvelopeMismatch);
        assert!(err_latest
            .to_string()
            .contains("session contains unindexed raw frames; point lookup unavailable"));

        let err_fiber = writer.fiber(fiber_id).unwrap_err();
        assert_eq!(*err_fiber.condition(), FailureCondition::EnvelopeMismatch);
        assert!(err_fiber
            .to_string()
            .contains("session contains unindexed raw frames; point lookup unavailable"));
    }

    #[test]
    fn test_unvalidated_frame_revokes_point_lookup_and_append() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("unvalidated_revokes_all");
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
        let fiber_id = [0x22; 16];

        writer
            .append_unvalidated_frame(b"unvalidated-frame-bytes")
            .expect("append unvalidated frame");

        let err_latest = writer.get_latest(fiber_id).unwrap_err();
        assert_eq!(*err_latest.condition(), FailureCondition::EnvelopeMismatch);
        assert!(err_latest
            .to_string()
            .contains("session contains unindexed raw frames; point lookup unavailable"));

        let err_fiber = writer.fiber(fiber_id).unwrap_err();
        assert_eq!(*err_fiber.condition(), FailureCondition::EnvelopeMismatch);
        assert!(err_fiber
            .to_string()
            .contains("session contains unindexed raw frames; point lookup unavailable"));

        let err_append_fiber = writer
            .append_to_fiber(fiber_id, [0x01; 16], b"payload")
            .unwrap_err();
        assert_eq!(
            *err_append_fiber.condition(),
            FailureCondition::EnvelopeMismatch
        );

        let env = EventEnvelope::genesis([0x02; 16], fiber_id, b"envelope").unwrap();
        let err_append_env = writer.append_envelope(&env).unwrap_err();
        assert_eq!(
            *err_append_env.condition(),
            FailureCondition::EnvelopeMismatch
        );
        assert!(err_append_env
            .to_string()
            .contains("session contains unindexed raw frames; append unavailable"));
    }

    #[test]
    fn test_file_append_to_fiber_indeterminate_on_sync_error() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("test_file_indeterminate");
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
        let writer = adapter.create(&claim).expect("create writer");
        let fiber_id = [0x55; 16];
        let event_id = [0x77; 16];

        let mut writer = writer.with_simulate_sync_error(true);
        let verdict = writer
            .append_to_fiber(fiber_id, event_id, b"payload-sync-err")
            .expect("append to fiber returns verdict");
        assert_eq!(
            verdict,
            WriteLandingVerdict::Undetermined { carried_epoch: 1 }
        );
        assert_eq!(
            writer.uncertain_diagnostic(),
            Some("sync_data failed; write landing undetermined: simulated sync_data failure")
        );

        let err_fiber = writer.fiber(fiber_id).unwrap_err();
        assert_eq!(
            *err_fiber.condition(),
            FailureCondition::OwnershipRecordUnreadable
        );
        assert!(err_fiber
            .to_string()
            .contains("writer session in uncertain state; reconciliation required"));

        let err_latest = writer.get_latest(fiber_id).unwrap_err();
        assert_eq!(
            *err_latest.condition(),
            FailureCondition::OwnershipRecordUnreadable
        );

        let err_append = writer
            .append_to_fiber(fiber_id, [0x78; 16], b"payload2")
            .unwrap_err();
        assert_eq!(
            *err_append.condition(),
            FailureCondition::OwnershipRecordUnreadable
        );
    }

    #[test]
    fn test_file_append_envelope_verdict_landed_and_indeterminate() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("test_file_env_verdict");
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
        let env1 = EventEnvelope::genesis([0x01; 16], [0x55; 16], b"payload-1").unwrap();

        let verdict = writer
            .append_envelope_verdict(&env1)
            .expect("append envelope verdict");
        assert_eq!(verdict, WriteLandingVerdict::Landed(1));

        let env2 = EventEnvelope {
            header: crate::encoding::EnvelopeHeader {
                event_id: [0x02; 16],
                fiber_id: [0x55; 16],
                detached: false,
                precursor: env1.header.event_id,
                precursor_hash: env1.commitment(),
            },
            payload: b"payload-2".to_vec(),
        };

        let mut indet_writer = writer.with_simulate_indeterminate(true);
        let indet_verdict = indet_writer
            .append_envelope_verdict(&env2)
            .expect("indet verdict");
        assert_eq!(
            indet_verdict,
            WriteLandingVerdict::Undetermined { carried_epoch: 1 }
        );
        assert_eq!(
            indet_writer.uncertain_diagnostic(),
            Some("write landing undetermined: simulated indeterminate write landing")
        );

        let err_subsequent = indet_writer
            .append_envelope_verdict(&env2)
            .expect_err("subsequent append rejected while uncertain");
        assert_eq!(
            *err_subsequent.condition(),
            FailureCondition::OwnershipRecordUnreadable
        );
    }

    #[test]
    fn test_file_diagnostic_phase_isolation_write_vs_sync() {
        let temp_dir = tempfile::tempdir().unwrap();
        let stem = temp_dir.path().join("test_file_phase_isolation");
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

        let writer = adapter.create(&claim).expect("create writer");
        let mut write_err_writer = writer.with_simulate_write_error(true);
        let fiber_id = [0x55; 16];
        let event_id = [0x77; 16];
        let verdict = write_err_writer
            .append_to_fiber(fiber_id, event_id, b"payload-write-err")
            .expect("append returns verdict");
        assert_eq!(
            verdict,
            WriteLandingVerdict::Undetermined { carried_epoch: 1 }
        );
        let write_diag = write_err_writer
            .uncertain_diagnostic()
            .expect("retained write diagnostic");
        assert!(
            write_diag.starts_with("write_all failed; write landing undetermined:"),
            "expected write_all prefix, found: {write_diag}"
        );

        let stem_sync = temp_dir.path().join("test_file_sync_phase");
        let adapter_sync = FileStorageAdapter::new(&stem_sync);
        let sync_writer = adapter_sync.create(&claim).expect("create writer");
        let mut sync_err_writer = sync_writer.with_simulate_sync_error(true);
        let verdict_sync = sync_err_writer
            .append_to_fiber(fiber_id, event_id, b"payload-sync-err")
            .expect("append returns verdict");
        assert_eq!(
            verdict_sync,
            WriteLandingVerdict::Undetermined { carried_epoch: 1 }
        );
        let sync_diag = sync_err_writer
            .uncertain_diagnostic()
            .expect("retained sync diagnostic");
        assert!(
            sync_diag.starts_with("sync_data failed; write landing undetermined:"),
            "expected sync_data prefix, found: {sync_diag}"
        );

        assert_ne!(write_diag, sync_diag);

        let stem_meta = temp_dir.path().join("test_file_meta_phase");
        let adapter_meta = FileStorageAdapter::new(&stem_meta);
        let mut meta_writer = adapter_meta.create(&claim).expect("create writer");
        let mut perms = std::fs::metadata(meta_writer.meta_path())
            .unwrap()
            .permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(meta_writer.meta_path(), perms.clone()).unwrap();

        let dummy_record = OwnershipRecord::OwnershipClaim(claim.clone());
        let meta_err = meta_writer
            .record_meta_record(&dummy_record)
            .expect_err("read-only meta should fail append");
        assert_eq!(
            *meta_err.condition(),
            FailureCondition::OwnershipRecordUnreadable
        );

        let meta_diag = meta_writer
            .uncertain_diagnostic()
            .expect("retained metadata diagnostic");
        assert!(
            meta_diag.starts_with("metadata write failed; write landing undetermined:"),
            "expected metadata write prefix, found: {meta_diag}"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                meta_writer.meta_path(),
                std::fs::Permissions::from_mode(0o644),
            )
            .expect("restore permissions");
        }
        #[cfg(not(unix))]
        #[allow(clippy::permissions_set_readonly_false)]
        {
            perms.set_readonly(false);
            std::fs::set_permissions(meta_writer.meta_path(), perms).expect("restore permissions");
        }
    }
}
