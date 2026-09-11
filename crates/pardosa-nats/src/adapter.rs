//! JetStream storage adapter implementation for Pardosa.

use pardosa::prelude::*;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

pub(crate) fn run_future<F, T>(handle: &tokio::runtime::Handle, fut: F) -> T
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::runtime::Handle::try_current() {
        Ok(curr) => match curr.runtime_flavor() {
            tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| handle.block_on(fut))
            }
            _ => std::thread::scope(|s| s.spawn(|| handle.block_on(fut)).join().unwrap()),
        },
        Err(_) => handle.block_on(fut),
    }
}

fn is_wrong_last_sequence(err: &async_nats::jetstream::context::PublishError) -> bool {
    use async_nats::jetstream::context::PublishErrorKind;
    use async_nats::jetstream::ErrorCode;
    if err.kind() == PublishErrorKind::WrongLastSequence {
        return true;
    }
    if let Some(source) = std::error::Error::source(err) {
        if let Some(api_err) = source.downcast_ref::<async_nats::jetstream::Error>() {
            let code = api_err.error_code();
            if code == ErrorCode::STREAM_WRONG_LAST_SEQUENCE
                || code == ErrorCode::STREAM_WRONG_LAST_SEQUENCE_CONSTANT
                || code == ErrorCode(10071)
                || code == ErrorCode(10164)
            {
                return true;
            }
        }
    }
    let msg = err.to_string();
    msg.contains("wrong last sequence") || msg.contains("10071") || msg.contains("10164")
}

/// All decoded ownership records found in an artefact's meta stream.
pub type NatsMetaRecords = MetaRecords;

async fn stream_exists(js: &async_nats::jetstream::Context, stream_name: &str) -> bool {
    js.get_stream(stream_name).await.is_ok()
}

async fn read_meta_records_async(
    js: &async_nats::jetstream::Context,
    meta_stream_name: &str,
) -> Result<NatsMetaRecords, OperationFailure> {
    let mut stream = match js.get_stream(meta_stream_name).await {
        Ok(s) => s,
        Err(_) => return Ok(NatsMetaRecords::default()),
    };
    let info = match stream.info().await {
        Ok(info) => info,
        Err(err) => {
            return Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!("failed to get info for meta stream {meta_stream_name}: {err}"),
            ));
        }
    };
    if info.state.messages == 0 {
        return Ok(NatsMetaRecords::default());
    }
    let first = info.state.first_sequence;
    let last = info.state.last_sequence;
    let mut records = NatsMetaRecords::default();
    for seq in first..=last {
        let raw = match stream.get_raw_message(seq).await {
            Ok(msg) => msg,
            Err(_) => continue,
        };
        if seq == 1 {
            let _ = ContainerHeader::decode(&raw.payload);
            continue;
        }
        let (payload, _) = match ContainerFrame::decode(&raw.payload) {
            Ok(res) => res,
            Err(err) => {
                return Err(OperationFailure::new(
                    FailureCondition::OwnershipRecordUnreadable,
                    format!("failed to decode frame in meta stream at seq {seq}: {err}"),
                ));
            }
        };
        if let Ok((record, _)) = OwnershipRecord::decode(&payload) {
            match record {
                OwnershipRecord::OwnershipClaim(claim) => {
                    records.latest_claim = Some(claim);
                }
                OwnershipRecord::SchemaDescriptor {
                    schema_version,
                    descriptor_bytes,
                } => {
                    if let Ok((root, _)) = DescriptorNode::decode(&descriptor_bytes) {
                        records.schema_descriptor =
                            Some(SchemaDescriptor::new(schema_version, root));
                    }
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
    }
    Ok(records)
}

async fn read_data_frames_async(
    js: &async_nats::jetstream::Context,
    data_stream_name: &str,
) -> Result<(ContainerHeader, Vec<Vec<u8>>, RollingCommitment, u64), OperationFailure> {
    let mut stream = js.get_stream(data_stream_name).await.map_err(|err| {
        OperationFailure::new(
            FailureCondition::NoArtefactExists,
            format!("failed to open data stream {data_stream_name}: {err}"),
        )
    })?;
    let info = stream.info().await.map_err(|err| {
        OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            format!("failed to get info for data stream {data_stream_name}: {err}"),
        )
    })?;
    if info.state.messages == 0 {
        return Err(OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            "data stream is empty; missing container header per C10.3",
        ));
    }
    let first = info.state.first_sequence;
    let last = info.state.last_sequence;
    let header_raw = stream.get_raw_message(first).await.map_err(|err| {
        OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            format!("failed to read container header from data stream: {err}"),
        )
    })?;
    let (header, _) = ContainerHeader::decode(&header_raw.payload).map_err(|err| {
        OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            format!("invalid container header in data stream: {err}"),
        )
    })?;

    let mut frames = Vec::new();
    let mut rolling = RollingCommitment::new();
    for seq in (first + 1)..=last {
        let raw = stream.get_raw_message(seq).await.map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("failed to read frame from data stream at seq {seq}: {err}"),
            )
        })?;
        let (payload, _) = ContainerFrame::decode(&raw.payload).map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("corrupted container frame in data stream at seq {seq}: {err}"),
            )
        })?;
        rolling.update_frame(&raw.payload);
        frames.push(payload);
    }
    Ok((header, frames, rolling, last))
}

/// JetStream storage adapter managing container artefacts in NATS JetStream per C5.10, C5.11, and C10.3.
#[derive(Clone)]
pub struct NatsStorageAdapter {
    url: String,
    stem: String,
    meta_stream_name: String,
    data_stream_name: String,
    client: async_nats::Client,
    js: async_nats::jetstream::Context,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl fmt::Debug for NatsStorageAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NatsStorageAdapter")
            .field("url", &self.url)
            .field("stem", &self.stem)
            .field("meta_stream_name", &self.meta_stream_name)
            .field("data_stream_name", &self.data_stream_name)
            .finish()
    }
}

impl NatsStorageAdapter {
    /// Creates a new NATS storage adapter by connecting to a NATS server URL.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if connection fails.
    pub fn new(url: impl Into<String>, stem: impl Into<String>) -> Result<Self, OperationFailure> {
        let url_str = url.into();
        let stem_str = stem.into();
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to initialize runtime: {err}"),
                    )
                })?,
        );
        let u_clone = url_str.clone();
        let client = rt
            .block_on(async move { async_nats::connect(&u_clone).await })
            .map_err(|err| {
                OperationFailure::new(
                    FailureCondition::OwnershipRecordUnreadable,
                    format!("failed to connect to NATS at {url_str}: {err}"),
                )
            })?;
        Ok(Self::from_client_internal(url_str, client, stem_str, rt))
    }

    /// Creates a new NATS storage adapter from an existing connected client.
    #[must_use]
    pub fn from_client(client: async_nats::Client, stem: impl Into<String>) -> Self {
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to create runtime for NATS adapter"),
        );
        Self::from_client_internal("nats://connected".to_string(), client, stem.into(), rt)
    }

    /// Creates a new NATS storage adapter from an existing connected client with a provided runtime.
    #[must_use]
    pub fn from_client_with_runtime(
        client: async_nats::Client,
        stem: impl Into<String>,
        runtime: Arc<tokio::runtime::Runtime>,
    ) -> Self {
        Self::from_client_internal("nats://connected".to_string(), client, stem.into(), runtime)
    }

    fn from_client_internal(
        url: String,
        client: async_nats::Client,
        stem: String,
        runtime: Arc<tokio::runtime::Runtime>,
    ) -> Self {
        let meta_stream_name = format!("{stem}_meta");
        let data_stream_name = format!("{stem}_data");
        let js = runtime.block_on(async { async_nats::jetstream::new(client.clone()) });
        Self {
            url,
            stem,
            meta_stream_name,
            data_stream_name,
            client,
            js,
            runtime,
        }
    }

    /// Returns the NATS connection URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the common stream name stem.
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

    /// Returns the current monotonic epoch for this artefact from the meta stream.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if reading fails.
    pub fn current_epoch(&self) -> Result<u64, OperationFailure> {
        let meta = self.read_meta_records()?;
        Ok(meta.latest_claim.map_or(0, |c| c.epoch))
    }

    /// Returns the ownership record stream name (`{stem}_meta`).
    #[must_use]
    pub fn meta_stream_name(&self) -> &str {
        &self.meta_stream_name
    }

    /// Returns the event data stream name (`{stem}_data`).
    #[must_use]
    pub fn data_stream_name(&self) -> &str {
        &self.data_stream_name
    }

    /// Returns the presence of artefact streams in JetStream per C5.10.
    #[must_use]
    pub fn presence(&self) -> ArtefactPresence {
        let js = self.js.clone();
        let meta_name = self.meta_stream_name.clone();
        let data_name = self.data_stream_name.clone();
        let handle = self.runtime.handle().clone();
        run_future(&handle, async move {
            let meta_exists = stream_exists(&js, &meta_name).await;
            let data_exists = stream_exists(&js, &data_name).await;
            match (meta_exists, data_exists) {
                (false, false) => ArtefactPresence::None,
                (true, false) => ArtefactPresence::OwnershipRecordOnly,
                (false, true) => ArtefactPresence::EventDataOnly,
                (true, true) => ArtefactPresence::Both,
            }
        })
    }

    /// Creates the artefact streams exclusively with initial ownership claim per C5.10, C5.64, and C12.3.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::StoreAlreadyExists`] if artefact already exists.
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if creation fails.
    pub fn create(
        &self,
        initial_claim: &OwnershipClaimRecord,
    ) -> Result<NatsWriterSession, OperationFailure> {
        admit_create(self.presence())?;

        let js = self.js.clone();
        let client = self.client.clone();
        let stem = self.stem.clone();
        let meta_name = self.meta_stream_name.clone();
        let data_name = self.data_stream_name.clone();
        let claim = initial_claim.clone();
        let handle = self.runtime.handle().clone();
        let runtime = self.runtime.clone();

        run_future(&handle, async move {
            js.create_stream(async_nats::jetstream::stream::Config {
                name: meta_name.clone(),
                subjects: vec![meta_name.clone()],
                storage: async_nats::jetstream::stream::StorageType::File,
                ..Default::default()
            })
            .await
            .map_err(|err| {
                OperationFailure::new(
                    FailureCondition::OwnershipRecordUnreadable,
                    format!("failed to create meta stream: {err}"),
                )
            })?;

            let header_bytes = ContainerHeader::new().to_bytes();
            let mut init_meta_headers = async_nats::HeaderMap::new();
            init_meta_headers.insert(
                async_nats::header::NATS_EXPECTED_LAST_SUBJECT_SEQUENCE,
                async_nats::HeaderValue::from(0),
            );
            js.publish_with_headers(
                meta_name.clone(),
                init_meta_headers,
                header_bytes.to_vec().into(),
            )
            .await
            .map_err(|err| {
                if is_wrong_last_sequence(&err) {
                    OperationFailure::new(
                        FailureCondition::StoreAlreadyExists,
                        "artefact already created by concurrent writer per C12.3",
                    )
                } else {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to publish container header to meta stream: {err}"),
                    )
                }
            })?
            .await
            .map_err(|err| {
                if is_wrong_last_sequence(&err) {
                    OperationFailure::new(
                        FailureCondition::StoreAlreadyExists,
                        "artefact already created by concurrent writer per C12.3",
                    )
                } else {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to ack container header on meta stream: {err}"),
                    )
                }
            })?;

            let mut claim_bytes = Vec::new();
            OwnershipRecord::OwnershipClaim(claim.clone()).encode(&mut claim_bytes);
            let mut claim_frame = Vec::new();
            ContainerFrame::encode_payload(&claim_bytes, &mut claim_frame);

            let mut claim_meta_headers = async_nats::HeaderMap::new();
            claim_meta_headers.insert(
                async_nats::header::NATS_EXPECTED_LAST_SUBJECT_SEQUENCE,
                async_nats::HeaderValue::from(1),
            );
            js.publish_with_headers(meta_name.clone(), claim_meta_headers, claim_frame.into())
                .await
                .map_err(|err| {
                    if is_wrong_last_sequence(&err) {
                        OperationFailure::new(
                            FailureCondition::StoreAlreadyExists,
                            "artefact already created by concurrent writer per C12.3",
                        )
                    } else {
                        OperationFailure::new(
                            FailureCondition::OwnershipRecordUnreadable,
                            format!("failed to publish claim frame to meta stream: {err}"),
                        )
                    }
                })?
                .await
                .map_err(|err| {
                    if is_wrong_last_sequence(&err) {
                        OperationFailure::new(
                            FailureCondition::StoreAlreadyExists,
                            "artefact already created by concurrent writer per C12.3",
                        )
                    } else {
                        OperationFailure::new(
                            FailureCondition::OwnershipRecordUnreadable,
                            format!("failed to ack claim frame on meta stream: {err}"),
                        )
                    }
                })?;

            js.create_stream(async_nats::jetstream::stream::Config {
                name: data_name.clone(),
                subjects: vec![data_name.clone()],
                storage: async_nats::jetstream::stream::StorageType::File,
                ..Default::default()
            })
            .await
            .map_err(|err| {
                OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    format!("failed to create data stream: {err}"),
                )
            })?;

            let mut init_data_headers = async_nats::HeaderMap::new();
            init_data_headers.insert(
                async_nats::header::NATS_EXPECTED_LAST_SUBJECT_SEQUENCE,
                async_nats::HeaderValue::from(0),
            );
            let data_ack = js
                .publish_with_headers(
                    data_name.clone(),
                    init_data_headers,
                    header_bytes.to_vec().into(),
                )
                .await
                .map_err(|err| {
                    if is_wrong_last_sequence(&err) {
                        OperationFailure::new(
                            FailureCondition::StoreAlreadyExists,
                            "artefact already created by concurrent writer per C12.3",
                        )
                    } else {
                        OperationFailure::new(
                            FailureCondition::PrecursorChainBroken(None),
                            format!("failed to publish container header to data stream: {err}"),
                        )
                    }
                })?
                .await
                .map_err(|err| {
                    if is_wrong_last_sequence(&err) {
                        OperationFailure::new(
                            FailureCondition::StoreAlreadyExists,
                            "artefact already created by concurrent writer per C12.3",
                        )
                    } else {
                        OperationFailure::new(
                            FailureCondition::PrecursorChainBroken(None),
                            format!("failed to ack container header on data stream: {err}"),
                        )
                    }
                })?;

            Ok(NatsWriterSession {
                stem,
                meta_stream_name: meta_name,
                data_stream_name: data_name,
                carried_epoch: claim.epoch,
                claim,
                rolling_commitment: RollingCommitment::new(),
                last_data_seq: data_ack.sequence,
                client,
                js,
                runtime,
                publish_timeout: Duration::from_secs(5),
                simulate_indeterminate: false,
                fiber_index: SessionIndex::new(),
                seen_events: HashSet::new(),
                uncertain: false,
                uncertain_diagnostic: None,
            })
        })
    }

    /// Creates only the `{stem}_meta` component of an artefact for testing incomplete creation per C5.10.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::StoreAlreadyExists`] if meta stream already exists.
    pub fn create_incomplete_meta_only(
        &self,
        claim: &OwnershipClaimRecord,
    ) -> Result<(), OperationFailure> {
        let presence = self.presence();
        if presence != ArtefactPresence::None {
            return Err(OperationFailure::new(
                FailureCondition::StoreAlreadyExists,
                "artefact streams already exist per C12.3",
            ));
        }

        let js = self.js.clone();
        let meta_name = self.meta_stream_name.clone();
        let claim_clone = claim.clone();
        let handle = self.runtime.handle().clone();

        run_future(&handle, async move {
            js.create_stream(async_nats::jetstream::stream::Config {
                name: meta_name.clone(),
                subjects: vec![meta_name.clone()],
                storage: async_nats::jetstream::stream::StorageType::File,
                ..Default::default()
            })
            .await
            .map_err(|err| {
                OperationFailure::new(
                    FailureCondition::OwnershipRecordUnreadable,
                    format!("failed to create meta stream: {err}"),
                )
            })?;

            let header_bytes = ContainerHeader::new().to_bytes();
            let mut init_meta_headers = async_nats::HeaderMap::new();
            init_meta_headers.insert(
                async_nats::header::NATS_EXPECTED_LAST_SUBJECT_SEQUENCE,
                async_nats::HeaderValue::from(0),
            );
            js.publish_with_headers(
                meta_name.clone(),
                init_meta_headers,
                header_bytes.to_vec().into(),
            )
            .await
            .map_err(|err| {
                if is_wrong_last_sequence(&err) {
                    OperationFailure::new(
                        FailureCondition::StoreAlreadyExists,
                        "artefact already created by concurrent writer per C12.3",
                    )
                } else {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to publish container header: {err}"),
                    )
                }
            })?
            .await
            .map_err(|err| {
                if is_wrong_last_sequence(&err) {
                    OperationFailure::new(
                        FailureCondition::StoreAlreadyExists,
                        "artefact already created by concurrent writer per C12.3",
                    )
                } else {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to ack container header: {err}"),
                    )
                }
            })?;

            let mut claim_bytes = Vec::new();
            OwnershipRecord::OwnershipClaim(claim_clone).encode(&mut claim_bytes);
            let mut claim_frame = Vec::new();
            ContainerFrame::encode_payload(&claim_bytes, &mut claim_frame);

            let mut claim_meta_headers = async_nats::HeaderMap::new();
            claim_meta_headers.insert(
                async_nats::header::NATS_EXPECTED_LAST_SUBJECT_SEQUENCE,
                async_nats::HeaderValue::from(1),
            );
            js.publish_with_headers(meta_name.clone(), claim_meta_headers, claim_frame.into())
                .await
                .map_err(|err| {
                    if is_wrong_last_sequence(&err) {
                        OperationFailure::new(
                            FailureCondition::StoreAlreadyExists,
                            "artefact already created by concurrent writer per C12.3",
                        )
                    } else {
                        OperationFailure::new(
                            FailureCondition::OwnershipRecordUnreadable,
                            format!("failed to publish claim frame: {err}"),
                        )
                    }
                })?
                .await
                .map_err(|err| {
                    if is_wrong_last_sequence(&err) {
                        OperationFailure::new(
                            FailureCondition::StoreAlreadyExists,
                            "artefact already created by concurrent writer per C12.3",
                        )
                    } else {
                        OperationFailure::new(
                            FailureCondition::OwnershipRecordUnreadable,
                            format!("failed to ack claim frame: {err}"),
                        )
                    }
                })?;

            Ok(())
        })
    }

    /// Completes creation of an artefact where `{stem}_meta` exists without `{stem}_data` per C5.10.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if state is not incomplete creation or if creation fails.
    pub fn complete_creation(
        &self,
        claim: &OwnershipClaimRecord,
    ) -> Result<NatsWriterSession, OperationFailure> {
        let presence = self.presence();
        if presence != ArtefactPresence::OwnershipRecordOnly {
            return Err(OperationFailure::new(
                FailureCondition::StoreAlreadyExists,
                "artefact creation is not in incomplete state per C5.10",
            ));
        }

        let js = self.js.clone();
        let client = self.client.clone();
        let stem = self.stem.clone();
        let meta_name = self.meta_stream_name.clone();
        let data_name = self.data_stream_name.clone();
        let claim_clone = claim.clone();
        let handle = self.runtime.handle().clone();
        let runtime = self.runtime.clone();

        run_future(&handle, async move {
            js.create_stream(async_nats::jetstream::stream::Config {
                name: data_name.clone(),
                subjects: vec![data_name.clone()],
                storage: async_nats::jetstream::stream::StorageType::File,
                ..Default::default()
            })
            .await
            .map_err(|err| {
                OperationFailure::new(
                    FailureCondition::StoreAlreadyExists,
                    format!("failed to create data stream during completion: {err}"),
                )
            })?;

            let header_bytes = ContainerHeader::new().to_bytes();
            let mut init_data_headers = async_nats::HeaderMap::new();
            init_data_headers.insert(
                async_nats::header::NATS_EXPECTED_LAST_SUBJECT_SEQUENCE,
                async_nats::HeaderValue::from(0),
            );
            let data_ack = js
                .publish_with_headers(
                    data_name.clone(),
                    init_data_headers,
                    header_bytes.to_vec().into(),
                )
                .await
                .map_err(|err| {
                    if is_wrong_last_sequence(&err) {
                        OperationFailure::new(
                            FailureCondition::StoreAlreadyExists,
                            "artefact data stream already created by concurrent writer per C12.3",
                        )
                    } else {
                        OperationFailure::new(
                            FailureCondition::PrecursorChainBroken(None),
                            format!("failed to publish container header to data stream: {err}"),
                        )
                    }
                })?
                .await
                .map_err(|err| {
                    if is_wrong_last_sequence(&err) {
                        OperationFailure::new(
                            FailureCondition::StoreAlreadyExists,
                            "artefact data stream already created by concurrent writer per C12.3",
                        )
                    } else {
                        OperationFailure::new(
                            FailureCondition::PrecursorChainBroken(None),
                            format!("failed to ack container header on data stream: {err}"),
                        )
                    }
                })?;

            Ok(NatsWriterSession {
                stem,
                meta_stream_name: meta_name,
                data_stream_name: data_name,
                carried_epoch: claim_clone.epoch,
                claim: claim_clone,
                rolling_commitment: RollingCommitment::new(),
                last_data_seq: data_ack.sequence,
                client,
                js,
                runtime,
                publish_timeout: Duration::from_secs(5),
                simulate_indeterminate: false,
                fiber_index: SessionIndex::new(),
                seen_events: HashSet::new(),
                uncertain: false,
                uncertain_diagnostic: None,
            })
        })
    }

    /// Opens the artefact strictly for writing with a carried epoch per C5.5, C5.6, C5.62, and C12.4.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::NoArtefactExists`] if streams are missing.
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipUnestablished`] if opening orphan data stream.
    /// Returns [`OperationFailure`] with [`FailureCondition::StaleEpoch`] if carried epoch is superseded.
    pub fn open_write(&self, carried_epoch: u64) -> Result<NatsWriterSession, OperationFailure> {
        let handle = self.runtime.handle().clone();
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
                let js_meta = self.js.clone();
                let meta_name = self.meta_stream_name.clone();
                let meta = run_future(&handle, async move {
                    read_meta_records_async(&js_meta, &meta_name).await
                })?;
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

        let js_meta = self.js.clone();
        let meta_name = self.meta_stream_name.clone();
        let meta = run_future(&handle, async move {
            read_meta_records_async(&js_meta, &meta_name).await
        })?;
        if meta.outbound_pointer.is_some() {
            return Err(OperationFailure::new(
                FailureCondition::RetiredMigrationSource,
                "artefact append authority permanently retired via outbound pointer per C5.63",
            ));
        }
        let claim = meta.latest_claim.ok_or_else(|| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "no ownership claim found in meta stream per C5.12",
            )
        })?;
        if claim.epoch != carried_epoch {
            return Err(OperationFailure::new(
                FailureCondition::StaleEpoch,
                "carried epoch does not match recorded epoch in meta stream per C5.5 and C12.4",
            ));
        }

        let js_data = self.js.clone();
        let data_name = self.data_stream_name.clone();
        let (_, frames, rolling, last_data_seq) = run_future(&handle, async move {
            read_data_frames_async(&js_data, &data_name).await
        })?;
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

        Ok(NatsWriterSession {
            stem: self.stem.clone(),
            meta_stream_name: self.meta_stream_name.clone(),
            data_stream_name: self.data_stream_name.clone(),
            carried_epoch,
            claim,
            rolling_commitment: rolling,
            last_data_seq,
            client: self.client.clone(),
            js: self.js.clone(),
            runtime: self.runtime.clone(),
            publish_timeout: Duration::from_secs(5),
            simulate_indeterminate: false,
            fiber_index,
            seen_events,
            uncertain: false,
            uncertain_diagnostic: None,
        })
    }

    /// Opens the artefact strictly for reading per C5.6, C5.11, C5.62, and C6.14.
    ///
    /// Permits concurrent readers without taking writer locks.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::NoArtefactExists`] if artefact is missing.
    pub fn open_read(&self) -> Result<NatsReaderSession, OperationFailure> {
        let presence = self.presence();
        if presence == ArtefactPresence::None {
            return Err(OperationFailure::new(
                FailureCondition::NoArtefactExists,
                "no artefact exists on open; strict open does not create per C5.62",
            ));
        }

        let js = self.js.clone();
        let meta_name = self.meta_stream_name.clone();
        let handle = self.runtime.handle().clone();
        let meta_records = run_future(&handle, async move {
            read_meta_records_async(&js, &meta_name).await
        })?;

        let admission = admit_open(presence, meta_records.latest_claim.clone(), false)?;

        let mut fiber_index = SessionIndex::new();
        let mut failed = None;
        if presence == ArtefactPresence::Both || presence == ArtefactPresence::EventDataOnly {
            let js_data = self.js.clone();
            let data_name = self.data_stream_name.clone();
            let (_, frames, _, _) = run_future(&handle, async move {
                read_data_frames_async(&js_data, &data_name).await
            })?;
            match SessionIndex::build_from_frames(frames.iter().map(|f| f.as_slice())) {
                Ok(idx) => fiber_index = idx,
                Err(err) => failed = Some(err.condition().clone()),
            }
        }

        Ok(NatsReaderSession {
            stem: self.stem.clone(),
            meta_stream_name: self.meta_stream_name.clone(),
            data_stream_name: self.data_stream_name.clone(),
            admission,
            meta_records,
            rolling_commitment: RollingCommitment::new(),
            js: self.js.clone(),
            runtime: self.runtime.clone(),
            fiber_index,
            failed,
        })
    }

    /// Reads all ownership records from the meta stream.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if reading fails.
    pub fn read_meta_records(&self) -> Result<NatsMetaRecords, OperationFailure> {
        let js = self.js.clone();
        let meta_name = self.meta_stream_name.clone();
        let handle = self.runtime.handle().clone();
        run_future(&handle, async move {
            read_meta_records_async(&js, &meta_name).await
        })
    }

    /// Appends an arbitrary ownership record to the meta stream.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_ownership_record(
        &self,
        record: &OwnershipRecord,
    ) -> Result<(), OperationFailure> {
        let js = self.js.clone();
        let meta_name = self.meta_stream_name.clone();
        let record_clone = record.clone();
        let handle = self.runtime.handle().clone();

        run_future(&handle, async move {
            let mut record_bytes = Vec::new();
            record_clone.encode(&mut record_bytes);
            let mut record_frame = Vec::new();
            ContainerFrame::encode_payload(&record_bytes, &mut record_frame);

            js.publish(meta_name.clone(), record_frame.into())
                .await
                .map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to publish meta frame: {err}"),
                    )
                })?
                .await
                .map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to ack meta frame: {err}"),
                    )
                })?;

            Ok(())
        })
    }

    /// Appends an updated ownership claim record to the meta stream.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_ownership_claim(
        &self,
        claim: &OwnershipClaimRecord,
    ) -> Result<(), OperationFailure> {
        self.record_ownership_record(&OwnershipRecord::OwnershipClaim(claim.clone()))
    }

    /// Appends an outbound generation pointer record to the meta stream per C6.17 and C5.63.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_outbound_pointer(
        &self,
        pointer: &OutboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_ownership_record(&OwnershipRecord::OutboundPointer(pointer.clone()))
    }

    /// Appends an inbound generation pointer record to the meta stream per C6.16.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_inbound_pointer(
        &self,
        pointer: &InboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_ownership_record(&OwnershipRecord::InboundPointer(pointer.clone()))
    }

    /// Appends a migration start record to the meta stream per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_migration_start(
        &self,
        start: &MigrationStartRecord,
    ) -> Result<(), OperationFailure> {
        self.record_ownership_record(&OwnershipRecord::MigrationStart(start.clone()))
    }

    /// Appends a migration end record to the meta stream per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_migration_end(&self, end: &MigrationEndRecord) -> Result<(), OperationFailure> {
        self.record_ownership_record(&OwnershipRecord::MigrationEnd(end.clone()))
    }

    /// Appends a rescue policy choice record to the meta stream per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_rescue_policy_choice(
        &self,
        choice: &RescuePolicyChoiceRecord,
    ) -> Result<(), OperationFailure> {
        self.record_ownership_record(&OwnershipRecord::RescuePolicyChoice(choice.clone()))
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

    /// Deletes both artefact streams from JetStream.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if stream deletion fails unexpectedly.
    pub fn delete_streams(&self) -> Result<(), OperationFailure> {
        let js = self.js.clone();
        let meta_name = self.meta_stream_name.clone();
        let data_name = self.data_stream_name.clone();
        let handle = self.runtime.handle().clone();

        run_future(&handle, async move {
            let _ = js.delete_stream(&meta_name).await;
            let _ = js.delete_stream(&data_name).await;
            Ok(())
        })
    }
}

/// Active JetStream writer session holding append authority with OCC sequence checks per C5.5 and C5.7.
pub struct NatsWriterSession {
    stem: String,
    meta_stream_name: String,
    data_stream_name: String,
    carried_epoch: u64,
    claim: OwnershipClaimRecord,
    rolling_commitment: RollingCommitment,
    last_data_seq: u64,
    client: async_nats::Client,
    js: async_nats::jetstream::Context,
    runtime: Arc<tokio::runtime::Runtime>,
    publish_timeout: Duration,
    simulate_indeterminate: bool,
    fiber_index: SessionIndex,
    seen_events: HashSet<[u8; 16]>,
    uncertain: bool,
    uncertain_diagnostic: Option<String>,
}

impl fmt::Debug for NatsWriterSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NatsWriterSession")
            .field("stem", &self.stem)
            .field("meta_stream_name", &self.meta_stream_name)
            .field("data_stream_name", &self.data_stream_name)
            .field("carried_epoch", &self.carried_epoch)
            .field("claim", &self.claim)
            .field("last_data_seq", &self.last_data_seq)
            .finish()
    }
}

impl NatsWriterSession {
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

    /// Returns the common artefact stem.
    #[must_use]
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// Returns the ownership record stream name (`{stem}_meta`).
    #[must_use]
    pub fn meta_stream_name(&self) -> &str {
        &self.meta_stream_name
    }

    /// Returns the event data stream name (`{stem}_data`).
    #[must_use]
    pub fn data_stream_name(&self) -> &str {
        &self.data_stream_name
    }

    /// Returns the last sequence recorded in the data stream.
    #[must_use]
    pub fn last_sequence(&self) -> u64 {
        self.last_data_seq
    }

    /// Configures the timeout for publish operations.
    #[must_use]
    pub fn with_publish_timeout(mut self, timeout: Duration) -> Self {
        self.publish_timeout = timeout;
        self
    }

    /// Configures whether to simulate an indeterminate write landing per C5.16.
    #[must_use]
    pub fn with_simulate_indeterminate(mut self, simulate: bool) -> Self {
        self.simulate_indeterminate = simulate;
        self
    }

    fn check_authority(&self) -> Result<(), OperationFailure> {
        if self.uncertain {
            return Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "writer session in uncertain state; reconciliation required",
            ));
        }
        let js_meta = self.js.clone();
        let meta_name = self.meta_stream_name.clone();
        let handle = self.runtime.handle().clone();

        let meta = run_future(&handle, async move {
            read_meta_records_async(&js_meta, &meta_name).await
        })?;
        if meta.outbound_pointer.is_some() {
            return Err(OperationFailure::new(
                FailureCondition::RetiredMigrationSource,
                "artefact append authority permanently retired via outbound pointer per C5.63",
            ));
        }
        let latest_claim = meta.latest_claim.ok_or_else(|| {
            OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                "no ownership claim found in meta stream during per-landing check per C5.12",
            )
        })?;
        if latest_claim.epoch != self.carried_epoch {
            return Err(OperationFailure::new(
                FailureCondition::StaleEpoch,
                "writer epoch superseded in meta stream; write rejected per C5.5 and C12.4",
            ));
        }
        Ok(())
    }

    /// Appends a framed payload byte slice to the data stream with per-landing epoch verification and OCC sequence checks.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::StaleEpoch`] if epoch is superseded.
    /// Returns [`OperationFailure`] with [`FailureCondition::ConcurrencyConflict`] if OCC expected sequence mismatches.
    pub fn append_frame(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        match self.append_frame_verdict(payload)? {
            WriteLandingVerdict::Landed(frame_count) => Ok(frame_count),
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                self.uncertain = true;
                let diagnostic = format!(
                    "write landing undetermined for carried epoch {carried_epoch} per C5.16"
                );
                if self.uncertain_diagnostic.is_none() {
                    self.uncertain_diagnostic = Some(diagnostic.clone());
                }
                Err(OperationFailure::new(
                    FailureCondition::OwnershipRecordUnreadable,
                    diagnostic,
                ))
            }
        }
    }

    /// Appends a framed payload byte slice, returning a [`WriteLandingVerdict`] explicitly handling indeterminate landings per C5.16.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::StaleEpoch`] if epoch is superseded.
    /// Returns [`OperationFailure`] with [`FailureCondition::ConcurrencyConflict`] if OCC sequence check fails.
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

        let verdict = self.append_frame_verdict_raw(payload)?;
        match verdict {
            WriteLandingVerdict::Landed(_) => {
                self.fiber_index.commit_append(reservation)?;
                self.seen_events.insert(event_id);
            }
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                self.uncertain = true;
                if self.uncertain_diagnostic.is_none() {
                    self.uncertain_diagnostic = Some(format!(
                        "write landing undetermined for carried epoch {carried_epoch} per C5.16"
                    ));
                }
            }
        }
        Ok(verdict)
    }

    fn append_frame_verdict_raw(
        &mut self,
        payload: &[u8],
    ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        let handle = self.runtime.handle().clone();

        let mut frame_buf = Vec::new();
        ContainerFrame::encode_payload(payload, &mut frame_buf);

        let mut headers = async_nats::HeaderMap::new();
        headers.insert(
            async_nats::header::NATS_EXPECTED_LAST_SUBJECT_SEQUENCE,
            async_nats::HeaderValue::from(self.last_data_seq),
        );

        let js = self.js.clone();
        let data_name = self.data_stream_name.clone();
        let timeout = self.publish_timeout;
        let carried_epoch = self.carried_epoch;
        let payload_bytes = frame_buf.clone();

        let (ack, diagnostic) = run_future(&handle, async move {
            let pub_future = js
                .publish_with_headers(data_name.clone(), headers, payload_bytes.into())
                .await
                .map_err(|err| {
                    if is_wrong_last_sequence(&err) {
                        OperationFailure::new(
                            FailureCondition::ConcurrencyConflict,
                            "two-writer concurrency collision: expected sequence mismatch",
                        )
                    } else {
                        OperationFailure::new(
                            FailureCondition::PrecursorChainBroken(None),
                            format!("failed to initiate publish to data stream: {err}"),
                        )
                    }
                })?;

            match tokio::time::timeout(timeout, pub_future).await {
                Ok(Ok(ack)) => Ok((WriteLandingVerdict::Landed(ack), None)),
                Ok(Err(err)) => {
                    if is_wrong_last_sequence(&err) {
                        Err(OperationFailure::new(
                            FailureCondition::ConcurrencyConflict,
                            "two-writer concurrency collision: expected sequence mismatch",
                        ))
                    } else {
                        Ok((
                            WriteLandingVerdict::Undetermined { carried_epoch },
                            Some(format!(
                                "publish ack failed; write landing undetermined: {err}"
                            )),
                        ))
                    }
                }
                Err(_) => Ok((
                    WriteLandingVerdict::Undetermined { carried_epoch },
                    Some(format!(
                        "publish ack timed out after {timeout:?}; write landing undetermined"
                    )),
                )),
            }
        })?;

        match ack {
            WriteLandingVerdict::Landed(pub_ack) => {
                self.last_data_seq = pub_ack.sequence;
                self.rolling_commitment.update_frame(&frame_buf);
                Ok(WriteLandingVerdict::Landed(
                    self.rolling_commitment.frame_count(),
                ))
            }
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                self.uncertain = true;
                self.uncertain_diagnostic = diagnostic.or_else(|| {
                    Some(format!(
                        "write landing undetermined for carried epoch {carried_epoch} per C5.16"
                    ))
                });
                Ok(WriteLandingVerdict::Undetermined { carried_epoch })
            }
        }
    }

    /// Appends an event envelope to the data stream.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if append or epoch verification fails.
    pub fn append_envelope(&mut self, envelope: &EventEnvelope) -> Result<u64, OperationFailure> {
        let mut env_buf = Vec::new();
        envelope.encode(&mut env_buf);
        self.append_frame(&env_buf)
    }

    /// Appends an event envelope returning a [`WriteLandingVerdict`].
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if epoch or sequence check fails.
    pub fn append_envelope_verdict(
        &mut self,
        envelope: &EventEnvelope,
    ) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        let mut env_buf = Vec::new();
        envelope.encode(&mut env_buf);
        self.append_frame_verdict(&env_buf)
    }

    /// Appends a raw frame payload to the data stream.
    ///
    /// If the payload decodes to an exact [`EventEnvelope`], it undergoes session index pre-admission validation.
    /// Otherwise, the raw frame is appended and unindexes point lookups.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if authority check fails, pre-admission validation fails, or publish fails.
    pub fn append_raw_frame(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        self.check_authority()?;

        if self.simulate_indeterminate {
            self.uncertain = true;
            let diagnostic = format!(
                "write landing undetermined for carried epoch {} per C5.16",
                self.carried_epoch
            );
            self.uncertain_diagnostic = Some(diagnostic.clone());
            return Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                diagnostic,
            ));
        }

        if payload.len() >= 85 {
            if let Ok((env, consumed)) = EventEnvelope::decode(payload) {
                if consumed == payload.len() {
                    let reservation = self.fiber_index.prepare_append(&env)?;
                    let verdict = self.append_frame_verdict_raw(payload)?;
                    return match verdict {
                        WriteLandingVerdict::Landed(seq) => {
                            self.fiber_index.commit_append(reservation)?;
                            self.seen_events.insert(env.header.event_id);
                            Ok(seq)
                        }
                        WriteLandingVerdict::Undetermined { carried_epoch } => {
                            self.uncertain = true;
                            let diagnostic = format!(
                                "write landing undetermined for carried epoch {carried_epoch} per C5.16"
                            );
                            if self.uncertain_diagnostic.is_none() {
                                self.uncertain_diagnostic = Some(diagnostic.clone());
                            }
                            Err(OperationFailure::new(
                                FailureCondition::OwnershipRecordUnreadable,
                                diagnostic,
                            ))
                        }
                    };
                }
            }
        }

        let verdict = self.append_frame_verdict_raw(payload)?;
        match verdict {
            WriteLandingVerdict::Landed(seq) => {
                self.fiber_index.mark_has_raw_frames();
                Ok(seq)
            }
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                self.uncertain = true;
                let diagnostic = format!(
                    "write landing undetermined for carried epoch {carried_epoch} per C5.16"
                );
                if self.uncertain_diagnostic.is_none() {
                    self.uncertain_diagnostic = Some(diagnostic.clone());
                }
                Err(OperationFailure::new(
                    FailureCondition::OwnershipRecordUnreadable,
                    diagnostic,
                ))
            }
        }
    }

    /// Appends an unvalidated raw frame directly to the data stream for testing or migration recovery.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if authority check fails, session is uncertain, or publish fails.
    #[doc(hidden)]
    pub fn append_unvalidated_frame(&mut self, payload: &[u8]) -> Result<u64, OperationFailure> {
        self.check_authority()?;
        let verdict = self.append_frame_verdict_raw(payload)?;
        match verdict {
            WriteLandingVerdict::Landed(seq) => {
                self.fiber_index.mark_has_raw_frames();
                Ok(seq)
            }
            WriteLandingVerdict::Undetermined { carried_epoch } => {
                self.uncertain = true;
                let diagnostic = format!(
                    "write landing undetermined for carried epoch {carried_epoch} per C5.16"
                );
                if self.uncertain_diagnostic.is_none() {
                    self.uncertain_diagnostic = Some(diagnostic.clone());
                }
                Err(OperationFailure::new(
                    FailureCondition::OwnershipRecordUnreadable,
                    diagnostic,
                ))
            }
        }
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
    /// Returns [`OperationFailure`] if append is rejected by fiber lifecycle or underlying JetStream storage fails.
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
    /// Returns [`OperationFailure`] if detach is rejected by fiber lifecycle or underlying JetStream storage fails.
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
    /// Returns [`OperationFailure`] if rescue is rejected by fiber lifecycle or underlying JetStream storage fails.
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

        let mut rec_buf = Vec::new();
        record.encode(&mut rec_buf);
        let mut frame_buf = Vec::new();
        ContainerFrame::encode_payload(&rec_buf, &mut frame_buf);

        let js = self.js.clone();
        let meta_name = self.meta_stream_name.clone();
        let handle = self.runtime.handle().clone();
        let timeout = self.publish_timeout;

        let ack_result = run_future(&handle, async move {
            let pub_future = js
                .publish(meta_name.clone(), frame_buf.into())
                .await
                .map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to publish schema descriptor to meta stream: {err}"),
                    )
                })?;

            match tokio::time::timeout(timeout, pub_future).await {
                Ok(Ok(_ack)) => Ok(None),
                Ok(Err(err)) => Ok(Some(format!(
                    "failed to ack schema descriptor on meta stream: {err}"
                ))),
                Err(_) => Ok(Some(format!(
                    "schema descriptor ack timed out after {timeout:?} on meta stream"
                ))),
            }
        })?;

        if let Some(diagnostic) = ack_result {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(format!(
                "schema descriptor write failed; write landing undetermined: {diagnostic}"
            ));
            return Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                diagnostic,
            ));
        }

        Ok(())
    }

    /// Flushes client connection to ensure broker receipt.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if flush fails.
    pub fn sync(&mut self) -> Result<(), OperationFailure> {
        self.check_authority()?;
        let client = self.client.clone();
        let handle = self.runtime.handle().clone();
        run_future(&handle, async move {
            client.flush().await.map_err(|err| {
                OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    format!("failed to flush NATS client: {err}"),
                )
            })
        })
        .inspect_err(|err| {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(format!(
                "sync flush failed; write landing undetermined: {err}"
            ));
        })
    }

    /// Appends an arbitrary ownership record to the meta stream.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_ownership_record(
        &mut self,
        record: &OwnershipRecord,
    ) -> Result<(), OperationFailure> {
        self.check_authority()?;
        let js = self.js.clone();
        let meta_name = self.meta_stream_name.clone();
        let record_clone = record.clone();
        let handle = self.runtime.handle().clone();
        let timeout = self.publish_timeout;

        let ack_result = run_future(&handle, async move {
            let mut record_bytes = Vec::new();
            record_clone.encode(&mut record_bytes);
            let mut record_frame = Vec::new();
            ContainerFrame::encode_payload(&record_bytes, &mut record_frame);

            let pub_future = js
                .publish(meta_name.clone(), record_frame.into())
                .await
                .map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to publish meta frame: {err}"),
                    )
                })?;

            match tokio::time::timeout(timeout, pub_future).await {
                Ok(Ok(_ack)) => Ok(None),
                Ok(Err(err)) => Ok(Some(format!("failed to ack meta frame: {err}"))),
                Err(_) => Ok(Some(format!(
                    "meta frame ack timed out after {timeout:?} on meta stream"
                ))),
            }
        })?;

        if let Some(diagnostic) = ack_result {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(format!(
                "ownership record write failed; write landing undetermined: {diagnostic}"
            ));
            return Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                diagnostic,
            ));
        }

        Ok(())
    }

    /// Appends an outbound generation pointer record to the meta stream per C6.17 and C5.63.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_outbound_pointer(
        &mut self,
        pointer: &OutboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_ownership_record(&OwnershipRecord::OutboundPointer(pointer.clone()))
    }

    /// Appends an inbound generation pointer record to the meta stream per C6.16.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_inbound_pointer(
        &mut self,
        pointer: &InboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_ownership_record(&OwnershipRecord::InboundPointer(pointer.clone()))
    }

    /// Appends a migration start record to the meta stream per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_migration_start(
        &mut self,
        start: &MigrationStartRecord,
    ) -> Result<(), OperationFailure> {
        self.record_ownership_record(&OwnershipRecord::MigrationStart(start.clone()))
    }

    /// Appends a migration end record to the meta stream per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_migration_end(
        &mut self,
        end: &MigrationEndRecord,
    ) -> Result<(), OperationFailure> {
        self.record_ownership_record(&OwnershipRecord::MigrationEnd(end.clone()))
    }

    /// Appends a rescue policy choice record to the meta stream per C4.13.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::OwnershipRecordUnreadable`] if writing fails.
    pub fn record_rescue_policy_choice(
        &mut self,
        choice: &RescuePolicyChoiceRecord,
    ) -> Result<(), OperationFailure> {
        self.record_ownership_record(&OwnershipRecord::RescuePolicyChoice(choice.clone()))
    }
}

/// Reader session providing non-exclusive read-only access to an artefact container in JetStream per C5.6, C5.11, and C6.14.
pub struct NatsReaderSession {
    stem: String,
    meta_stream_name: String,
    data_stream_name: String,
    admission: OpenAdmission,
    meta_records: NatsMetaRecords,
    rolling_commitment: RollingCommitment,
    js: async_nats::jetstream::Context,
    runtime: Arc<tokio::runtime::Runtime>,
    fiber_index: SessionIndex,
    failed: Option<FailureCondition>,
}

impl fmt::Debug for NatsReaderSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NatsReaderSession")
            .field("stem", &self.stem)
            .field("meta_stream_name", &self.meta_stream_name)
            .field("data_stream_name", &self.data_stream_name)
            .field("admission", &self.admission)
            .field("meta_records", &self.meta_records)
            .finish()
    }
}

impl NatsReaderSession {
    /// Returns the common artefact stem.
    #[must_use]
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// Returns the ownership record stream name (`{stem}_meta`).
    #[must_use]
    pub fn meta_stream_name(&self) -> &str {
        &self.meta_stream_name
    }

    /// Returns the event data stream name (`{stem}_data`).
    #[must_use]
    pub fn data_stream_name(&self) -> &str {
        &self.data_stream_name
    }

    /// Returns the open admission status for this reader session.
    #[must_use]
    pub fn admission(&self) -> &OpenAdmission {
        &self.admission
    }

    /// Returns all meta records decoded from the meta stream.
    #[must_use]
    pub fn meta_records(&self) -> &NatsMetaRecords {
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

    /// Reads all framed payloads from the data stream, validating CRC32C on each frame.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] with [`FailureCondition::PrecursorChainBroken`] if checksum fails or stream corrupted.
    pub fn read_all_frames(&mut self) -> Result<Vec<Vec<u8>>, OperationFailure> {
        let js = self.js.clone();
        let data_name = self.data_stream_name.clone();
        let handle = self.runtime.handle().clone();
        let res = run_future(&handle, async move {
            read_data_frames_async(&js, &data_name).await
        });
        match res {
            Ok((_, frames, rolling, _)) => {
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

    /// Reads and decodes all event envelopes from the data stream.
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

impl MigrationSource for NatsStorageAdapter {
    fn locator_id(&self) -> [u8; 16] {
        self.locator_id()
    }

    fn current_epoch(&self) -> Result<u64, OperationFailure> {
        let meta = self.read_meta_records()?;
        Ok(meta.latest_claim.map_or(0, |c| c.epoch))
    }

    fn read_envelopes(&self) -> Result<Vec<EventEnvelope>, OperationFailure> {
        let mut reader = self.open_read()?;
        reader.read_all_envelopes_for_migration()
    }

    fn record_outbound_pointer(
        &self,
        pointer: &OutboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_outbound_pointer(pointer)
    }

    fn record_meta(&self, record: &OwnershipRecord) -> Result<(), OperationFailure> {
        self.record_ownership_record(record)
    }

    fn read_meta(&self) -> Result<MetaRecords, OperationFailure> {
        self.read_meta_records()
    }
}

impl MigrationTarget for NatsStorageAdapter {
    fn locator_id(&self) -> [u8; 16] {
        self.locator_id()
    }

    fn current_epoch(&self) -> Result<u64, OperationFailure> {
        let meta = self.read_meta_records()?;
        Ok(meta.latest_claim.map_or(0, |c| c.epoch))
    }

    fn append_envelopes(&mut self, envelopes: &[EventEnvelope]) -> Result<(), OperationFailure> {
        let epoch = self.current_epoch()?;
        let mut writer = self.open_write(epoch)?;
        for env in envelopes {
            let mut env_buf = Vec::new();
            env.encode(&mut env_buf);
            writer.append_unvalidated_frame(&env_buf)?;
        }
        writer.sync()
    }

    fn record_inbound_pointer(
        &self,
        pointer: &InboundPointerRecord,
    ) -> Result<(), OperationFailure> {
        self.record_inbound_pointer(pointer)
    }

    fn record_meta(&self, record: &OwnershipRecord) -> Result<(), OperationFailure> {
        self.record_ownership_record(record)
    }

    fn set_schema_descriptor(
        &mut self,
        descriptor: &SchemaDescriptor,
    ) -> Result<(), OperationFailure> {
        let epoch = self.current_epoch()?;
        let mut writer = self.open_write(epoch)?;
        writer.set_schema_descriptor(descriptor)
    }
}
