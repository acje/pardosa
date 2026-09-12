//! JetStream storage adapter implementation for Pardosa.

use pardosa::prelude::*;
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
            {
                return true;
            }
        }
    }
    false
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

const REPLAY_CONCURRENCY: usize = 16;

fn map_nats_stream_open_error(
    stream_name: &str,
    err: &async_nats::jetstream::context::GetStreamError,
) -> OperationFailure {
    use async_nats::jetstream::context::GetStreamErrorKind;
    use async_nats::jetstream::ErrorCode;
    let is_not_found = match err.kind() {
        GetStreamErrorKind::JetStream(js_err) => js_err.error_code() == ErrorCode::STREAM_NOT_FOUND,
        _ => false,
    };

    if is_not_found {
        OperationFailure::new(
            FailureCondition::NoArtefactExists,
            format!("stream {stream_name} not found: {err}"),
        )
    } else {
        OperationFailure::new(
            FailureCondition::TransportUnavailable,
            format!("transport unavailable opening stream {stream_name}: {err}"),
        )
    }
}

fn map_nats_info_error(stream_name: &str, err: impl std::fmt::Display) -> OperationFailure {
    OperationFailure::new(
        FailureCondition::TransportUnavailable,
        format!("transport unavailable querying info for stream {stream_name}: {err}"),
    )
}

fn map_nats_raw_message_error(
    seq: u64,
    err: &async_nats::jetstream::stream::RawMessageError,
) -> OperationFailure {
    use async_nats::jetstream::stream::RawMessageErrorKind;
    use async_nats::jetstream::ErrorCode;
    let is_missing = match err.kind() {
        RawMessageErrorKind::NoMessageFound => true,
        RawMessageErrorKind::JetStream(js_err) => {
            let code = js_err.error_code();
            code == ErrorCode::NO_MESSAGE_FOUND || code == ErrorCode::SEQUENCE_NOT_FOUND
        }
        _ => false,
    };

    if is_missing {
        OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            format!("missing message in data stream at seq {seq}: {err}"),
        )
    } else {
        OperationFailure::new(
            FailureCondition::TransportUnavailable,
            format!("transport unavailable reading raw message at seq {seq}: {err}"),
        )
    }
}

async fn read_data_frames_async(
    js: &async_nats::jetstream::Context,
    data_stream_name: &str,
) -> Result<(ContainerHeader, Vec<Vec<u8>>, RollingCommitment, u64), OperationFailure> {
    let mut stream = js
        .get_stream(data_stream_name)
        .await
        .map_err(|err| map_nats_stream_open_error(data_stream_name, &err))?;
    let (messages, first, last) = {
        let info = stream
            .info()
            .await
            .map_err(|err| map_nats_info_error(data_stream_name, err))?;
        (
            info.state.messages,
            info.state.first_sequence,
            info.state.last_sequence,
        )
    };
    if messages == 0 {
        return Err(OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            "data stream is empty; missing container header per C10.3",
        ));
    }
    let header_raw = stream
        .get_raw_message(first)
        .await
        .map_err(|err| map_nats_raw_message_error(first, &err))?;
    let (header, consumed) = ContainerHeader::decode(&header_raw.payload).map_err(|err| {
        OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            format!("invalid container header in data stream: {err}"),
        )
    })?;
    if consumed != header_raw.payload.len() {
        return Err(OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            "trailing unconsumed bytes in container header",
        ));
    }

    let mut frames = Vec::new();
    let mut rolling = RollingCommitment::new();

    if first < last {
        use futures_util::StreamExt;
        let stream_ref = &stream;
        let mut msg_stream = futures_util::stream::iter((first + 1)..=last)
            .map(|seq| async move {
                let raw = stream_ref
                    .get_raw_message(seq)
                    .await
                    .map_err(|err| map_nats_raw_message_error(seq, &err))?;
                Ok::<_, OperationFailure>((seq, raw))
            })
            .buffered(REPLAY_CONCURRENCY);

        while let Some(item) = msg_stream.next().await {
            let (seq, raw) = item?;
            let (payload, consumed) = ContainerFrame::decode(&raw.payload).map_err(|err| {
                OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    format!("corrupted container frame in data stream at seq {seq}: {err}"),
                )
            })?;
            if consumed != raw.payload.len() {
                return Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    format!("corrupted container frame in data stream at seq {seq}: trailing unconsumed bytes"),
                ));
            }
            rolling.update_frame(&raw.payload);
            frames.push(payload);
        }
    }

    Ok((header, frames, rolling, last))
}

async fn read_chunk_async(
    js: &async_nats::jetstream::Context,
    data_stream_name: &str,
    start_index: u64,
    max_items: usize,
    terminal_seq: Option<u64>,
) -> Result<Vec<Vec<u8>>, OperationFailure> {
    if max_items == 0 {
        return Ok(Vec::new());
    }
    let mut stream = js
        .get_stream(data_stream_name)
        .await
        .map_err(|err| map_nats_stream_open_error(data_stream_name, &err))?;
    let (messages, first, mut last) = {
        let info = stream
            .info()
            .await
            .map_err(|err| map_nats_info_error(data_stream_name, err))?;
        (
            info.state.messages,
            info.state.first_sequence,
            info.state.last_sequence,
        )
    };
    if messages == 0 {
        return Err(OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            "data stream is empty; missing container header per C10.3",
        ));
    }
    if let Some(t_seq) = terminal_seq {
        last = std::cmp::min(last, t_seq);
    }
    let header_raw = stream
        .get_raw_message(first)
        .await
        .map_err(|err| map_nats_raw_message_error(first, &err))?;
    let (_header, consumed) = ContainerHeader::decode(&header_raw.payload).map_err(|err| {
        OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            format!("invalid container header in data stream: {err}"),
        )
    })?;
    if consumed != header_raw.payload.len() {
        return Err(OperationFailure::new(
            FailureCondition::PrecursorChainBroken(None),
            "trailing unconsumed bytes in container header",
        ));
    }

    let start_seq = match first
        .checked_add(1)
        .and_then(|s| s.checked_add(start_index))
    {
        Some(s) => s,
        None => return Ok(Vec::new()),
    };
    if start_seq > last {
        return Ok(Vec::new());
    }

    let max_items_u64 = u64::try_from(max_items).unwrap_or(u64::MAX);
    let end_seq = match start_seq.checked_add(max_items_u64.saturating_sub(1)) {
        Some(end) => end.min(last),
        None => last,
    };
    if start_seq > end_seq {
        return Ok(Vec::new());
    }

    use futures_util::StreamExt;
    let stream_ref = &stream;
    let mut msg_stream = futures_util::stream::iter(start_seq..=end_seq)
        .map(|seq| async move {
            let raw = stream_ref
                .get_raw_message(seq)
                .await
                .map_err(|err| map_nats_raw_message_error(seq, &err))?;
            Ok::<_, OperationFailure>((seq, raw))
        })
        .buffered(REPLAY_CONCURRENCY);

    let mut frames = Vec::with_capacity((end_seq - start_seq + 1) as usize);
    while let Some(item) = msg_stream.next().await {
        let (seq, raw) = item?;
        let (payload, consumed) = ContainerFrame::decode(&raw.payload).map_err(|err| {
            OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("corrupted container frame in data stream at seq {seq}: {err}"),
            )
        })?;
        if consumed != raw.payload.len() {
            return Err(OperationFailure::new(
                FailureCondition::PrecursorChainBroken(None),
                format!("corrupted container frame in data stream at seq {seq}: trailing unconsumed bytes"),
            ));
        }
        frames.push(payload);
    }

    Ok(frames)
}

async fn read_range_async(
    js: &async_nats::jetstream::Context,
    data_stream_name: &str,
    from_seq: u64,
    to_seq: u64,
) -> Result<Vec<Vec<u8>>, OperationFailure> {
    if from_seq > to_seq {
        return Ok(Vec::new());
    }
    let stream = js
        .get_stream(data_stream_name)
        .await
        .map_err(|err| map_nats_stream_open_error(data_stream_name, &err))?;

    use futures_util::StreamExt;
    let stream_ref = &stream;
    let mut msg_stream = futures_util::stream::iter(from_seq..=to_seq)
        .map(|seq| async move {
            let raw = stream_ref
                .get_raw_message(seq)
                .await
                .map_err(|err| map_nats_raw_message_error(seq, &err))?;
            let (payload, consumed) = ContainerFrame::decode(&raw.payload).map_err(|err| {
                OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    format!("corrupted container frame in data stream at seq {seq}: {err}"),
                )
            })?;
            if consumed != raw.payload.len() {
                return Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    format!("corrupted container frame in data stream at seq {seq}: trailing unconsumed bytes"),
                ));
            }
            Ok(payload)
        })
        .buffered(REPLAY_CONCURRENCY);

    let count = match usize::try_from(to_seq.saturating_sub(from_seq).saturating_add(1)) {
        Ok(c) => c,
        Err(_) => usize::MAX,
    };
    let mut frames = Vec::with_capacity(count.min(1024));
    while let Some(res) = msg_stream.next().await {
        frames.push(res?);
    }
    Ok(frames)
}

/// JetStream storage adapter managing container artefacts in NATS JetStream per C5.10, C5.11, and C10.3.
#[derive(Clone)]
pub struct NatsStorageAdapter {
    url: String,
    stem: String,
    meta_stream_name: String,
    data_stream_name: String,
    meta_subject: String,
    data_subject: String,
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
            .field("meta_subject", &self.meta_subject)
            .field("data_subject", &self.data_subject)
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
        let meta_subject = format!("{stem}_meta");
        let data_subject = format!("{stem}_data");
        let js = runtime.block_on(async { async_nats::jetstream::new(client.clone()) });
        Self {
            url,
            stem,
            meta_stream_name,
            data_stream_name,
            meta_subject,
            data_subject,
            client,
            js,
            runtime,
        }
    }

    /// Overrides the subjects used for publishing and stream routing.
    #[must_use]
    pub fn with_subjects(
        mut self,
        meta_subject: impl Into<String>,
        data_subject: impl Into<String>,
    ) -> Self {
        self.meta_subject = meta_subject.into();
        self.data_subject = data_subject.into();
        self
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
        let meta_subject = self.meta_subject.clone();
        let data_subject = self.data_subject.clone();
        let claim = initial_claim.clone();
        let handle = self.runtime.handle().clone();
        let runtime = self.runtime.clone();

        run_future(&handle, async move {
            js.create_stream(async_nats::jetstream::stream::Config {
                name: meta_name.clone(),
                subjects: vec![meta_subject.clone()],
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
                meta_subject.clone(),
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
            js.publish_with_headers(meta_subject.clone(), claim_meta_headers, claim_frame.into())
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
                subjects: vec![data_subject.clone()],
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
                    data_subject.clone(),
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

            let meta_records = NatsMetaRecords {
                latest_claim: Some(claim.clone()),
                ..Default::default()
            };
            let engine = NatsEngine {
                client,
                js,
                runtime,
                stem,
                meta_stream_name: meta_name,
                data_stream_name: data_name,
                meta_subject,
                data_subject,
                carried_epoch: claim.epoch,
                claim: Some(claim),
                meta_records,
                admission: OpenAdmission::Ready,
                last_data_seq: data_ack.sequence,
                read_all_count: 0,
                publish_timeout: Duration::from_secs(5),
                simulate_indeterminate: false,
                uncertain: false,
                uncertain_diagnostic: None,
            };
            let store = Store::open_writer(engine)?;
            Ok(NatsWriterSession { store })
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
        let meta_subject = self.meta_subject.clone();
        let claim_clone = claim.clone();
        let handle = self.runtime.handle().clone();

        run_future(&handle, async move {
            js.create_stream(async_nats::jetstream::stream::Config {
                name: meta_name.clone(),
                subjects: vec![meta_subject.clone()],
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
                meta_subject.clone(),
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
            js.publish_with_headers(meta_subject, claim_meta_headers, claim_frame.into())
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
        let meta_subject = self.meta_subject.clone();
        let data_subject = self.data_subject.clone();
        let claim_clone = claim.clone();
        let handle = self.runtime.handle().clone();
        let runtime = self.runtime.clone();

        run_future(&handle, async move {
            let meta_records = read_meta_records_async(&js, &meta_name).await?;
            let durable_claim = meta_records.latest_claim.as_ref().ok_or_else(|| {
                OperationFailure::new(
                    FailureCondition::OwnershipRecordUnreadable,
                    "no ownership claim found in meta stream per C5.12",
                )
            })?;
            if durable_claim != &claim_clone {
                return Err(OperationFailure::new(
                    FailureCondition::OwnershipUnestablished,
                    "supplied claim does not match durable claim in meta stream per C5.10",
                ));
            }

            js.create_stream(async_nats::jetstream::stream::Config {
                name: data_name.clone(),
                subjects: vec![data_subject.clone()],
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
                    data_subject.clone(),
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

            let engine = NatsEngine {
                client,
                js,
                runtime,
                stem,
                meta_stream_name: meta_name,
                data_stream_name: data_name,
                meta_subject,
                data_subject,
                carried_epoch: claim_clone.epoch,
                claim: Some(claim_clone),
                meta_records,
                admission: OpenAdmission::Ready,
                last_data_seq: data_ack.sequence,
                read_all_count: 0,
                publish_timeout: Duration::from_secs(5),
                simulate_indeterminate: false,
                uncertain: false,
                uncertain_diagnostic: None,
            };
            let store = Store::open_writer(engine)?;
            Ok(NatsWriterSession { store })
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
                let claim = meta.latest_claim.clone().ok_or_else(|| {
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
        let claim = meta.latest_claim.clone().ok_or_else(|| {
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
        let last_data_seq = run_future(&handle, async move {
            let mut stream = js_data
                .get_stream(&data_name)
                .await
                .map_err(|err| map_nats_stream_open_error(&data_name, &err))?;
            let (messages, first_seq, last_seq) = {
                let info = stream
                    .info()
                    .await
                    .map_err(|err| map_nats_info_error(&data_name, err))?;
                (
                    info.state.messages,
                    info.state.first_sequence,
                    info.state.last_sequence,
                )
            };
            if messages == 0 {
                return Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    "data stream is empty; missing container header per C10.3",
                ));
            }
            let header_raw = stream
                .get_raw_message(first_seq)
                .await
                .map_err(|err| map_nats_raw_message_error(first_seq, &err))?;
            let (_header, consumed) =
                ContainerHeader::decode(&header_raw.payload).map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::PrecursorChainBroken(None),
                        format!("invalid container header in data stream: {err}"),
                    )
                })?;
            if consumed != header_raw.payload.len() {
                return Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    "trailing unconsumed bytes in container header",
                ));
            }
            Ok(last_seq)
        })?;

        let engine = NatsEngine {
            client: self.client.clone(),
            js: self.js.clone(),
            runtime: self.runtime.clone(),
            stem: self.stem.clone(),
            meta_stream_name: self.meta_stream_name.clone(),
            data_stream_name: self.data_stream_name.clone(),
            meta_subject: self.meta_subject.clone(),
            data_subject: self.data_subject.clone(),
            carried_epoch,
            claim: Some(claim),
            meta_records: meta,
            admission: OpenAdmission::Ready,
            last_data_seq,
            read_all_count: 0,
            publish_timeout: Duration::from_secs(5),
            simulate_indeterminate: false,
            uncertain: false,
            uncertain_diagnostic: None,
        };

        let store = Store::open_writer(engine)?;
        Ok(NatsWriterSession { store })
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

        let engine = NatsEngine {
            client: self.client.clone(),
            js: self.js.clone(),
            runtime: self.runtime.clone(),
            stem: self.stem.clone(),
            meta_stream_name: self.meta_stream_name.clone(),
            data_stream_name: self.data_stream_name.clone(),
            meta_subject: self.meta_subject.clone(),
            data_subject: self.data_subject.clone(),
            carried_epoch: meta_records.latest_claim.as_ref().map_or(0, |c| c.epoch),
            claim: meta_records.latest_claim.clone(),
            meta_records,
            admission,
            last_data_seq: 0,
            read_all_count: 0,
            publish_timeout: Duration::from_secs(5),
            simulate_indeterminate: false,
            uncertain: false,
            uncertain_diagnostic: None,
        };

        let store = Store::open_reader(engine);
        Ok(NatsReaderSession { store })
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
        let meta_subject = self.meta_subject.clone();
        let record_clone = record.clone();
        let handle = self.runtime.handle().clone();

        run_future(&handle, async move {
            let mut record_bytes = Vec::new();
            record_clone.encode(&mut record_bytes);
            let mut record_frame = Vec::new();
            ContainerFrame::encode_payload(&record_bytes, &mut record_frame);

            js.publish(meta_subject, record_frame.into())
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

/// Pure JetStream storage driver implementing [`StorageEngine`].
#[derive(Debug)]
pub struct NatsEngine {
    pub(crate) client: async_nats::Client,
    pub(crate) js: async_nats::jetstream::Context,
    pub(crate) runtime: Arc<tokio::runtime::Runtime>,
    pub(crate) stem: String,
    pub(crate) meta_stream_name: String,
    pub(crate) data_stream_name: String,
    pub(crate) meta_subject: String,
    pub(crate) data_subject: String,
    pub(crate) carried_epoch: u64,
    pub(crate) claim: Option<OwnershipClaimRecord>,
    pub(crate) meta_records: NatsMetaRecords,
    pub(crate) admission: OpenAdmission,
    pub(crate) last_data_seq: u64,
    pub(crate) read_all_count: usize,
    pub(crate) publish_timeout: Duration,
    pub(crate) simulate_indeterminate: bool,
    pub(crate) uncertain: bool,
    pub(crate) uncertain_diagnostic: Option<String>,
}

impl NatsEngine {
    /// Returns the number of times `read_all` was invoked on this engine.
    #[cfg(any(test, feature = "unstable-test-support"))]
    #[must_use]
    pub fn read_all_count(&self) -> usize {
        self.read_all_count
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

    /// Returns the ownership record subject.
    #[must_use]
    pub fn meta_subject(&self) -> &str {
        &self.meta_subject
    }

    /// Returns the event data subject.
    #[must_use]
    pub fn data_subject(&self) -> &str {
        &self.data_subject
    }

    /// Returns the last consumed message sequence in the event data stream.
    #[must_use]
    pub fn last_sequence(&self) -> u64 {
        self.last_data_seq
    }

    /// Returns all meta records decoded from the meta stream.
    #[must_use]
    pub fn meta_records(&self) -> &NatsMetaRecords {
        &self.meta_records
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
}

impl StorageEngine for NatsEngine {
    fn carried_epoch(&self) -> u64 {
        self.carried_epoch
    }

    fn check_authority(&self) -> Result<(), OperationFailure> {
        self.check_authority()
    }

    fn append_block(&mut self, block: &[u8]) -> Result<WriteLandingVerdict<u64>, OperationFailure> {
        self.check_authority()?;

        if self.simulate_indeterminate {
            self.uncertain = true;
            self.uncertain_diagnostic = Some(
                "write landing undetermined: simulated indeterminate write landing".to_string(),
            );
            return Ok(WriteLandingVerdict::Undetermined {
                carried_epoch: self.carried_epoch,
            });
        }

        if self.last_data_seq == u64::MAX {
            return Err(OperationFailure::new(
                FailureCondition::ValueConstraintViolated {
                    constraint: ValueConstraint::TooLong,
                },
                "sequence number overflow",
            ));
        }

        let handle = self.runtime.handle().clone();
        let mut headers = async_nats::HeaderMap::new();
        headers.insert(
            async_nats::header::NATS_EXPECTED_LAST_SUBJECT_SEQUENCE,
            async_nats::HeaderValue::from(self.last_data_seq),
        );

        let js = self.js.clone();
        let data_subject = self.data_subject.clone();
        let timeout = self.publish_timeout;
        let carried_epoch = self.carried_epoch;
        let block_bytes = block.to_vec();

        let (ack, diagnostic) = run_future(&handle, async move {
            let pub_future = js
                .publish_with_headers(data_subject, headers, block_bytes.into())
                .await
                .map_err(|err| {
                    if is_wrong_last_sequence(&err) {
                        OperationFailure::new(
                            FailureCondition::ConcurrencyConflict,
                            "two-writer concurrency collision: expected sequence mismatch",
                        )
                    } else {
                        OperationFailure::new(
                            FailureCondition::TransportUnavailable,
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
                Ok(WriteLandingVerdict::Landed(pub_ack.sequence))
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

    fn append_batch_detailed(&mut self, blocks: &[&[u8]]) -> BatchLandingVerdict<u64> {
        if let Err(error) = self.check_authority() {
            return BatchLandingVerdict::PreAttemptRefusal {
                error,
                unattempted_count: blocks.len(),
            };
        }
        if blocks.is_empty() {
            return BatchLandingVerdict::LandedAll {
                final_position: self.last_data_seq,
                landed_count: 0,
            };
        }
        if self.simulate_indeterminate {
            self.uncertain = true;
            self.uncertain_diagnostic = Some("simulated indeterminate write landing".to_string());
            return BatchLandingVerdict::PartialProgress {
                landed_count: 0,
                next_attempt: NextAttemptStatus::Undetermined {
                    carried_epoch: self.carried_epoch,
                },
                unattempted_count: blocks.len().saturating_sub(1),
            };
        }

        let handle = self.runtime.handle().clone();
        let js = self.js.clone();
        let data_subject = self.data_subject.clone();
        let timeout = self.publish_timeout;
        let carried_epoch = self.carried_epoch;
        let last_seq = self.last_data_seq;
        let blocks_owned: Vec<Vec<u8>> = blocks.iter().map(|b| b.to_vec()).collect();
        let total_blocks = blocks.len();

        let (verdict, final_seq, has_uncertainty) = run_future(&handle, async move {
            let mut current_seq = last_seq;

            for (i, block) in blocks_owned.iter().enumerate() {
                let expected_seq = match current_seq.checked_add(1) {
                    Some(_) => current_seq,
                    None => {
                        return (
                            BatchLandingVerdict::PartialProgress {
                                landed_count: i,
                                next_attempt: NextAttemptStatus::Rejected(OperationFailure::new(
                                    FailureCondition::ValueConstraintViolated {
                                        constraint: ValueConstraint::TooLong,
                                    },
                                    "sequence number overflow",
                                )),
                                unattempted_count: total_blocks.saturating_sub(i + 1),
                            },
                            current_seq,
                            false,
                        );
                    }
                };

                let mut headers = async_nats::HeaderMap::new();
                headers.insert(
                    async_nats::header::NATS_EXPECTED_LAST_SUBJECT_SEQUENCE,
                    async_nats::HeaderValue::from(expected_seq),
                );

                let pub_future = match js
                    .publish_with_headers(data_subject.clone(), headers, block.clone().into())
                    .await
                {
                    Ok(f) => f,
                    Err(err) => {
                        let failure = if is_wrong_last_sequence(&err) {
                            OperationFailure::new(
                                FailureCondition::ConcurrencyConflict,
                                "two-writer concurrency collision: expected sequence mismatch",
                            )
                        } else {
                            OperationFailure::new(
                                FailureCondition::TransportUnavailable,
                                format!("failed to initiate publish to data stream: {err}"),
                            )
                        };
                        return (
                            BatchLandingVerdict::PartialProgress {
                                landed_count: i,
                                next_attempt: NextAttemptStatus::Rejected(failure),
                                unattempted_count: total_blocks.saturating_sub(i + 1),
                            },
                            current_seq,
                            false,
                        );
                    }
                };

                match tokio::time::timeout(timeout, pub_future).await {
                    Ok(Ok(ack)) => {
                        current_seq = ack.sequence;
                    }
                    Ok(Err(err)) => {
                        if is_wrong_last_sequence(&err) {
                            return (
                                BatchLandingVerdict::PartialProgress {
                                    landed_count: i,
                                    next_attempt: NextAttemptStatus::Rejected(OperationFailure::new(
                                        FailureCondition::ConcurrencyConflict,
                                        "two-writer concurrency collision: expected sequence mismatch",
                                    )),
                                    unattempted_count: total_blocks.saturating_sub(i + 1),
                                },
                                current_seq,
                                false,
                            );
                        } else {
                            return (
                                BatchLandingVerdict::PartialProgress {
                                    landed_count: i,
                                    next_attempt: NextAttemptStatus::Undetermined { carried_epoch },
                                    unattempted_count: total_blocks.saturating_sub(i + 1),
                                },
                                current_seq,
                                true,
                            );
                        }
                    }
                    Err(_) => {
                        return (
                            BatchLandingVerdict::PartialProgress {
                                landed_count: i,
                                next_attempt: NextAttemptStatus::Undetermined { carried_epoch },
                                unattempted_count: total_blocks.saturating_sub(i + 1),
                            },
                            current_seq,
                            true,
                        );
                    }
                }
            }

            (
                BatchLandingVerdict::LandedAll {
                    final_position: current_seq,
                    landed_count: total_blocks,
                },
                current_seq,
                false,
            )
        });

        if has_uncertainty {
            self.uncertain = true;
            self.uncertain_diagnostic = Some("batch publish encountered uncertainty".to_string());
        }
        self.last_data_seq = final_seq;
        verdict
    }

    fn read_all(&mut self) -> Result<Vec<Vec<u8>>, OperationFailure> {
        self.read_all_count += 1;
        let handle = self.runtime.handle().clone();
        let js = self.js.clone();
        let data_name = self.data_stream_name.clone();
        let (_, frames, _, _seq) = run_future(&handle, async move {
            read_data_frames_async(&js, &data_name).await
        })?;
        Ok(frames)
    }

    fn read_chunk(
        &mut self,
        start_index: u64,
        max_items: usize,
    ) -> Result<Vec<Vec<u8>>, OperationFailure> {
        if max_items == 0 {
            return Ok(Vec::new());
        }
        let handle = self.runtime.handle().clone();
        let js = self.js.clone();
        let data_name = self.data_stream_name.clone();
        run_future(&handle, async move {
            read_chunk_async(&js, &data_name, start_index, max_items, None).await
        })
    }

    fn recover_frames(
        &mut self,
        chunk_size: usize,
        on_frame: &mut FrameRecoveryCallback<'_>,
    ) -> Result<u64, OperationFailure> {
        let handle = self.runtime.handle().clone();
        let js = self.js.clone();
        let data_name = self.data_stream_name.clone();

        let (first, last) = run_future(&handle, async move {
            let mut stream = js
                .get_stream(&data_name)
                .await
                .map_err(|err| map_nats_stream_open_error(&data_name, &err))?;
            let (messages, first_seq, last_seq) = {
                let info = stream
                    .info()
                    .await
                    .map_err(|err| map_nats_info_error(&data_name, err))?;
                (
                    info.state.messages,
                    info.state.first_sequence,
                    info.state.last_sequence,
                )
            };
            if messages == 0 {
                return Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    "data stream is empty; missing container header per C10.3",
                ));
            }
            let first_msg = stream
                .get_raw_message(first_seq)
                .await
                .map_err(|err| map_nats_raw_message_error(first_seq, &err))?;
            let (_header, consumed) =
                ContainerHeader::decode(&first_msg.payload).map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::PrecursorChainBroken(None),
                        format!("invalid container header in data stream: {err}"),
                    )
                })?;
            if consumed != first_msg.payload.len() {
                return Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    "trailing unconsumed bytes in container header",
                ));
            }
            Ok((first_seq, last_seq))
        })?;

        let data_start_seq = match first.checked_add(1) {
            Some(s) => s,
            None => {
                self.last_data_seq = last;
                return Ok(0);
            }
        };
        if data_start_seq > last {
            self.last_data_seq = last;
            return Ok(0);
        }

        let chunk_step = chunk_size.max(1) as u64;
        let mut current_seq = data_start_seq;
        let mut total_recovered = 0u64;

        while current_seq <= last {
            let chunk_end = match current_seq.checked_add(chunk_step.saturating_sub(1)) {
                Some(end) => end.min(last),
                None => last,
            };
            let js = self.js.clone();
            let data_name = self.data_stream_name.clone();
            let frames = run_future(&handle, async move {
                read_range_async(&js, &data_name, current_seq, chunk_end).await
            })?;
            let expected_count =
                match usize::try_from(chunk_end.saturating_sub(current_seq).saturating_add(1)) {
                    Ok(c) => c,
                    Err(_) => {
                        return Err(OperationFailure::new(
                            FailureCondition::ValueConstraintViolated {
                                constraint: ValueConstraint::TooLong,
                            },
                            "chunk size exceeds pointer width",
                        ));
                    }
                };
            if frames.len() != expected_count {
                return Err(OperationFailure::new(
                    FailureCondition::PrecursorChainBroken(None),
                    format!("unexpected missing messages between {current_seq} and {chunk_end} in data stream"),
                ));
            }
            for frame in frames {
                on_frame(total_recovered, &frame)?;
                total_recovered += 1;
            }
            if chunk_end == u64::MAX || chunk_end >= last {
                break;
            }
            current_seq = chunk_end + 1;
        }

        self.last_data_seq = last;
        Ok(total_recovered)
    }

    fn set_publish_timeout(&mut self, timeout: std::time::Duration) {
        self.publish_timeout = timeout;
    }

    fn set_simulate_indeterminate(&mut self, simulate: bool) {
        self.simulate_indeterminate = simulate;
    }

    fn is_retired(&self) -> Result<bool, OperationFailure> {
        let js_meta = self.js.clone();
        let meta_name = self.meta_stream_name.clone();
        let handle = self.runtime.handle().clone();
        let meta = run_future(&handle, async move {
            read_meta_records_async(&js_meta, &meta_name).await
        })?;
        Ok(meta.outbound_pointer.is_some())
    }

    fn sync(&mut self) -> Result<(), OperationFailure> {
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

    fn uncertain_diagnostic(&self) -> Option<&str> {
        self.uncertain_diagnostic.as_deref()
    }

    fn claim(&self) -> Option<&OwnershipClaimRecord> {
        self.claim.as_ref()
    }

    fn admission(&self) -> &OpenAdmission {
        &self.admission
    }

    fn schema_descriptor(&self) -> Option<&SchemaDescriptor> {
        self.meta_records.schema_descriptor.as_ref()
    }

    fn set_schema_descriptor(
        &mut self,
        descriptor: &SchemaDescriptor,
    ) -> Result<(), OperationFailure> {
        let mut descriptor_bytes = Vec::new();
        descriptor.root.encode(&mut descriptor_bytes);
        let record = OwnershipRecord::SchemaDescriptor {
            schema_version: descriptor.version,
            descriptor_bytes,
        };
        self.record_meta_record(&record)
    }

    fn record_meta_record(&mut self, record: &OwnershipRecord) -> Result<(), OperationFailure> {
        self.check_authority()?;
        if self.uncertain {
            return Err(OperationFailure::new(
                FailureCondition::OwnershipRecordUnreadable,
                format!(
                    "writer session is uncertain: {}",
                    self.uncertain_diagnostic.as_deref().unwrap_or("unknown")
                ),
            ));
        }
        let js = self.js.clone();
        let meta_subject = self.meta_subject.clone();
        let handle = self.runtime.handle().clone();
        let timeout = self.publish_timeout;
        let record_clone = record.clone();
        let (ack, diagnostic) = run_future(&handle, async move {
            let mut record_bytes = Vec::new();
            record_clone.encode(&mut record_bytes);
            let mut record_frame = Vec::new();
            ContainerFrame::encode_payload(&record_bytes, &mut record_frame);

            let pub_future = js
                .publish(meta_subject, record_frame.into())
                .await
                .map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to initiate publish to meta stream: {err}"),
                    )
                })?;

            match tokio::time::timeout(timeout, pub_future).await {
                Ok(Ok(ack)) => Ok((WriteLandingVerdict::Landed(ack), None)),
                Ok(Err(err)) => Ok((
                    WriteLandingVerdict::Undetermined { carried_epoch: 0 },
                    Some(format!(
                        "metadata publish ack failed; write landing undetermined: {err}"
                    )),
                )),
                Err(_) => Ok((
                    WriteLandingVerdict::Undetermined { carried_epoch: 0 },
                    Some(format!(
                        "metadata publish ack timed out after {timeout:?}; write landing undetermined"
                    )),
                )),
            }
        })?;

        match ack {
            WriteLandingVerdict::Landed(_) => {}
            WriteLandingVerdict::Undetermined { .. } => {
                self.uncertain = true;
                let detail = diagnostic
                    .unwrap_or_else(|| "metadata write landing undetermined per C5.16".to_string());
                self.uncertain_diagnostic = Some(detail.clone());
                return Err(OperationFailure::new(
                    FailureCondition::OwnershipRecordUnreadable,
                    detail,
                ));
            }
        }

        match record {
            OwnershipRecord::OwnershipClaim(claim) => {
                self.meta_records.latest_claim = Some(claim.clone());
            }
            OwnershipRecord::SchemaDescriptor {
                schema_version,
                descriptor_bytes,
            } => {
                let (root, _) = DescriptorNode::decode(descriptor_bytes).map_err(|err| {
                    OperationFailure::new(
                        FailureCondition::OwnershipRecordUnreadable,
                        format!("failed to decode schema descriptor in meta stream: {err}"),
                    )
                })?;
                self.meta_records.schema_descriptor =
                    Some(SchemaDescriptor::new(*schema_version, root));
            }
            OwnershipRecord::OutboundPointer(pointer) => {
                self.meta_records.outbound_pointer = Some(pointer.clone());
            }
            OwnershipRecord::InboundPointer(pointer) => {
                self.meta_records.inbound_pointer = Some(pointer.clone());
            }
            OwnershipRecord::MigrationStart(start) => {
                self.meta_records.migration_start = Some(start.clone());
            }
            OwnershipRecord::MigrationEnd(end) => {
                self.meta_records.migration_end = Some(end.clone());
            }
            OwnershipRecord::RescuePolicyChoice(choice) => {
                self.meta_records.rescue_policy_choice = Some(choice.clone());
            }
            OwnershipRecord::IdentityStructure(_) | OwnershipRecord::CleanRelease(_) => {}
        }
        Ok(())
    }

    fn outbound_pointer(&self) -> Option<&OutboundPointerRecord> {
        self.meta_records.outbound_pointer.as_ref()
    }

    fn inbound_pointer(&self) -> Option<&InboundPointerRecord> {
        self.meta_records.inbound_pointer.as_ref()
    }

    fn migration_start(&self) -> Option<&MigrationStartRecord> {
        self.meta_records.migration_start.as_ref()
    }

    fn migration_end(&self) -> Option<&MigrationEndRecord> {
        self.meta_records.migration_end.as_ref()
    }

    fn rescue_policy_choice(&self) -> Option<&RescuePolicyChoiceRecord> {
        self.meta_records.rescue_policy_choice.as_ref()
    }
}

/// Active JetStream writer session holding append authority with OCC sequence checks.
#[derive(Debug)]
pub struct NatsWriterSession {
    store: Store<NatsEngine>,
}

impl std::ops::Deref for NatsWriterSession {
    type Target = Store<NatsEngine>;
    fn deref(&self) -> &Self::Target {
        &self.store
    }
}

impl std::ops::DerefMut for NatsWriterSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.store
    }
}

impl NatsWriterSession {
    /// Returns the active ownership claim record.
    #[must_use]
    pub fn claim(&self) -> &OwnershipClaimRecord {
        self.store
            .engine()
            .claim()
            .expect("writer session must have admitted claim")
    }

    /// Returns the common artefact stem.
    #[must_use]
    pub fn stem(&self) -> &str {
        self.store.engine().stem()
    }

    /// Returns the ownership record stream name (`{stem}_meta`).
    #[must_use]
    pub fn meta_stream_name(&self) -> &str {
        self.store.engine().meta_stream_name()
    }

    /// Returns the event data stream name (`{stem}_data`).
    #[must_use]
    pub fn data_stream_name(&self) -> &str {
        self.store.engine().data_stream_name()
    }

    /// Returns the last consumed message sequence in the event data stream.
    #[must_use]
    pub fn last_sequence(&self) -> u64 {
        self.store.engine().last_sequence()
    }

    /// Returns all meta records decoded from the meta stream.
    #[must_use]
    pub fn meta_records(&self) -> &NatsMetaRecords {
        self.store.engine().meta_records()
    }

    /// Configures the publish timeout duration.
    #[must_use]
    pub fn with_publish_timeout(mut self, timeout: Duration) -> Self {
        self.store.set_publish_timeout(timeout);
        self
    }

    /// Configures whether to simulate an indeterminate write landing per C5.16.
    #[must_use]
    pub fn with_simulate_indeterminate(mut self, simulate: bool) -> Self {
        self.store.set_simulate_indeterminate(simulate);
        self
    }

    /// Records an arbitrary ownership record into metadata.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if recording fails.
    pub fn record_ownership_record(
        &mut self,
        record: &OwnershipRecord,
    ) -> Result<(), OperationFailure> {
        self.store.record_meta_record(record)
    }

    /// Reads a single raw block at the specified sequence or index from the data stream.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails or index is out of bounds.
    pub fn read_block(&mut self, index: u64) -> Result<Vec<u8>, OperationFailure> {
        self.store.read_block(index)
    }
}

/// Reader session providing non-exclusive read-only access to an artefact container in JetStream.
#[derive(Debug)]
pub struct NatsReaderSession {
    store: Store<NatsEngine>,
}

impl std::ops::Deref for NatsReaderSession {
    type Target = Store<NatsEngine>;
    fn deref(&self) -> &Self::Target {
        &self.store
    }
}

impl NatsReaderSession {
    /// Returns the common artefact stem.
    #[must_use]
    pub fn stem(&self) -> &str {
        self.store.engine().stem()
    }

    /// Returns the ownership record stream name (`{stem}_meta`).
    #[must_use]
    pub fn meta_stream_name(&self) -> &str {
        self.store.engine().meta_stream_name()
    }

    /// Returns the event data stream name (`{stem}_data`).
    #[must_use]
    pub fn data_stream_name(&self) -> &str {
        self.store.engine().data_stream_name()
    }

    /// Returns all meta records decoded from the meta stream.
    #[must_use]
    pub fn meta_records(&self) -> &NatsMetaRecords {
        self.store.engine().meta_records()
    }

    /// Reads a single raw block at the specified sequence or index from the data stream.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails or index is out of bounds.
    pub fn read_block(&mut self, index: u64) -> Result<Vec<u8>, OperationFailure> {
        self.store.read_block(index)
    }

    /// Reads all framed payloads from the data stream, validating CRC32C on each frame.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails or frame checksum fails.
    pub fn read_all_frames(&mut self) -> Result<Vec<Vec<u8>>, OperationFailure> {
        self.store.read_all_frames()
    }

    /// Reads and decodes all event envelopes from the container.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if any frame is invalid or history contains broken fibers.
    pub fn read_all_envelopes(&mut self) -> Result<Vec<EventEnvelope>, OperationFailure> {
        self.store.read_all_envelopes()
    }

    /// Reads all event envelopes for migration, tolerating broken fiber history.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading frames fails or frame decoding fails.
    pub fn read_all_envelopes_for_migration(
        &mut self,
    ) -> Result<Vec<EventEnvelope>, OperationFailure> {
        self.store.read_all_envelopes_for_migration()
    }

    /// Iterates over all event envelopes sequentially using borrowed views without bulk allocation.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails, decoding fails, or the reader retained broken state.
    pub fn for_each_envelope<F>(&mut self, f: F) -> Result<(), OperationFailure>
    where
        F: FnMut(EventEnvelopeRef<'_>) -> Result<(), OperationFailure>,
    {
        self.store.for_each_envelope(f)
    }

    /// Incrementally folds all event envelopes through a visitor closure using borrowed views.
    ///
    /// # Errors
    /// Returns [`OperationFailure`] if reading fails, decoding fails, or the reader retained broken state.
    pub fn fold_envelopes<B, F>(&mut self, init: B, f: F) -> Result<B, OperationFailure>
    where
        F: FnMut(B, EventEnvelopeRef<'_>) -> Result<B, OperationFailure>,
    {
        self.store.fold_envelopes(init, f)
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_nats::jetstream::context::{GetStreamError, GetStreamErrorKind};
    use async_nats::jetstream::stream::{RawMessageError, RawMessageErrorKind};
    use pardosa::store::FailureCondition;

    #[test]
    fn test_map_nats_raw_message_error_no_message_found_is_broken_chain() {
        let err = RawMessageError::new(RawMessageErrorKind::NoMessageFound);
        let failure = map_nats_raw_message_error(42, &err);
        assert_eq!(
            *failure.condition(),
            FailureCondition::PrecursorChainBroken(None)
        );
        assert!(failure.diagnostic_detail().message().contains("seq 42"));
    }

    #[test]
    fn test_map_nats_raw_message_error_other_is_transport_unavailable() {
        let err = RawMessageError::new(RawMessageErrorKind::Other);
        let failure = map_nats_raw_message_error(42, &err);
        assert_eq!(*failure.condition(), FailureCondition::TransportUnavailable);
    }

    #[test]
    fn test_map_nats_stream_open_error_empty_name_is_transport_unavailable() {
        let err = GetStreamError::new(GetStreamErrorKind::EmptyName);
        let failure = map_nats_stream_open_error("test_stream", &err);
        assert_eq!(*failure.condition(), FailureCondition::TransportUnavailable);
    }

    #[test]
    fn test_map_nats_stream_open_error_request_error_does_not_use_display_substring() {
        let err = GetStreamError::with_source(
            GetStreamErrorKind::Request,
            std::io::Error::new(std::io::ErrorKind::NotFound, "stream not found 404"),
        );
        let failure = map_nats_stream_open_error("test_stream", &err);
        assert_eq!(*failure.condition(), FailureCondition::TransportUnavailable);
    }
}
