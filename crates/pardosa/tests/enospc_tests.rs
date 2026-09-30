use pardosa::file::{
    ContainerFrame, ContainerHeader, FileStorageAdapter, QuotaWriter, TornTailInfo,
};
use pardosa::prelude::*;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

fn sample_claim(epoch: u64) -> OwnershipClaimRecord {
    OwnershipClaimRecord {
        epoch,
        machine_id: [1u8; 16],
        boot_id: [2u8; 16],
        process_id: 12345,
        process_start_time_ns: 1_000_000,
        claim_time_ns: 2_000_000,
        operator_label: "enospc-test-operator".to_string(),
    }
}

#[test]
fn test_quota_writer_enospc_and_partial_writes() {
    let mut sink = Vec::new();
    let mut writer = QuotaWriter::new(&mut sink, 7);

    assert_eq!(writer.remaining_quota(), 7);
    assert_eq!(writer.bytes_written(), 0);

    let n1 = writer.write(b"abc").expect("write 3 bytes");
    assert_eq!(n1, 3);
    assert_eq!(writer.remaining_quota(), 4);
    assert_eq!(writer.bytes_written(), 3);

    let n2 = writer.write(b"defgh").expect("write partial 4 bytes");
    assert_eq!(n2, 4);
    assert_eq!(writer.remaining_quota(), 0);
    assert_eq!(writer.bytes_written(), 7);

    let err = writer.write(b"xyz").expect_err("quota exhausted");
    assert_eq!(err.kind(), std::io::ErrorKind::StorageFull);

    writer.set_quota(5);
    assert_eq!(writer.remaining_quota(), 5);
    let n3 = writer.write(b"hello").expect("write after quota reset");
    assert_eq!(n3, 5);
    assert_eq!(writer.bytes_written(), 12);
    assert_eq!(sink.as_slice(), b"abcdefghello");

    let mut no_partial_sink = Vec::new();
    let mut no_partial_writer = QuotaWriter::new(&mut no_partial_sink, 4).with_allow_partial(false);
    let err_exceed = no_partial_writer
        .write(b"12345")
        .expect_err("write exceeding quota without partial must fail");
    assert_eq!(err_exceed.kind(), std::io::ErrorKind::StorageFull);
    assert_eq!(no_partial_writer.remaining_quota(), 4);
    assert_eq!(no_partial_writer.bytes_written(), 0);

    let n_fit = no_partial_writer
        .write(b"1234")
        .expect("write fitting quota without partial");
    assert_eq!(n_fit, 4);
    assert_eq!(no_partial_writer.remaining_quota(), 0);
    assert_eq!(no_partial_writer.bytes_written(), 4);

    let mut force_sink = Vec::new();
    let mut force_writer = QuotaWriter::new(&mut force_sink, 100).with_force_enospc(true);
    let err_force = force_writer.write(b"data").expect_err("forced enospc");
    assert_eq!(err_force.kind(), std::io::ErrorKind::StorageFull);

    let mut inject_sink = Vec::new();
    let mut inject_writer = QuotaWriter::new(&mut inject_sink, 100)
        .with_injected_error(std::io::ErrorKind::PermissionDenied);
    let err_inject = inject_writer
        .write(b"data")
        .expect_err("injected permission denied");
    assert_eq!(err_inject.kind(), std::io::ErrorKind::PermissionDenied);
    inject_writer.set_injected_error(None);
    let n_after_clear = inject_writer
        .write(b"data")
        .expect("write succeeds after clearing injected error");
    assert_eq!(n_after_clear, 4);
}

#[test]
fn test_quota_writer_read_and_seek() {
    let raw = b"abcdefghijklmnopqrstuvwxyz".to_vec();
    let cursor = std::io::Cursor::new(raw);
    let mut writer = QuotaWriter::new(cursor, 50);

    let pos = writer.seek(SeekFrom::Start(10)).expect("seek to 10");
    assert_eq!(pos, 10);

    let mut buf = [0u8; 5];
    let n = writer.read(&mut buf).expect("read 5 bytes");
    assert_eq!(n, 5);
    assert_eq!(&buf, b"klmno");

    let unwrapped = writer.into_inner();
    assert_eq!(&unwrapped.into_inner()[0..5], b"abcde");
}

#[test]
fn test_enospc_during_container_header_creation_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let stem = dir.path().join("partial_header_store");
    let adapter = FileStorageAdapter::new(&stem);
    let claim = sample_claim(1);

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(adapter.pgno_path())
        .expect("create pgno file");
    let mut quota_writer = QuotaWriter::new(file, 6);
    let header_bytes = ContainerHeader::new().to_bytes();
    let err = quota_writer
        .write_all(&header_bytes)
        .expect_err("write_all must fail with ENOSPC after 6 bytes");
    assert_eq!(err.kind(), std::io::ErrorKind::StorageFull);
    assert_eq!(quota_writer.bytes_written(), 6);
    drop(quota_writer);

    adapter
        .create_incomplete_meta_only(&claim)
        .expect("create meta only");
    let desc = AdmittedDescriptor::default_for_test();
    let mut bytes = Vec::new();
    desc.root().encode(&mut bytes).unwrap();
    adapter
        .record_meta_record_for_test(&OwnershipRecord::SchemaDescriptor {
            schema_version: desc.version(),
            descriptor_bytes: bytes,
        })
        .unwrap();

    let err_open_write = adapter
        .open_write(1)
        .expect_err("open_write must fail closed on partial header");
    assert_eq!(
        *err_open_write.condition(),
        FailureCondition::PrecursorChainBroken(None)
    );
    assert!(err_open_write
        .to_string()
        .contains("container file too short for header"));

    let err_open_read = adapter
        .open_read()
        .expect_err("open_read must fail closed on partial header");
    assert_eq!(
        *err_open_read.condition(),
        FailureCondition::PrecursorChainBroken(None)
    );

    let dir_meta = tempfile::tempdir().unwrap();
    let stem_meta = dir_meta.path().join("partial_meta_store");
    let adapter_meta = FileStorageAdapter::new(&stem_meta);

    let meta_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(adapter_meta.meta_path())
        .expect("create meta file");
    let mut meta_quota_writer = QuotaWriter::new(meta_file, 8);
    let meta_err = meta_quota_writer
        .write_all(&header_bytes)
        .expect_err("write_all must fail with ENOSPC on meta");
    assert_eq!(meta_err.kind(), std::io::ErrorKind::StorageFull);
    assert_eq!(meta_quota_writer.bytes_written(), 8);
    drop(meta_quota_writer);

    let err_read_meta = adapter_meta
        .read_meta_records()
        .expect_err("read_meta_records must fail on partial header");
    assert_eq!(
        *err_read_meta.condition(),
        FailureCondition::OwnershipRecordUnreadable
    );
}

#[test]
fn test_enospc_mid_frame_write_triggers_atomic_cursor_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let stem = dir.path().join("atomic_rollback_store");
    let adapter = FileStorageAdapter::new(&stem);
    let claim = sample_claim(1);
    let desc = AdmittedDescriptor::default_for_test();

    let mut writer = adapter.create(&claim, &desc).expect("create writer");
    let fiber_id = derive_fiber_id("order:1001");
    let event_id_1 = [0x01; 16];
    let v1 = writer
        .append_to_fiber(fiber_id, event_id_1, b"first-confirmed-event")
        .expect("first append succeeds");
    assert!(matches!(v1, WriteLandingVerdict::Landed(_)));

    let len_after_v1 = std::fs::metadata(writer.pgno_path()).unwrap().len();

    let mut enospc_writer = writer.with_simulate_enospc(20);
    let event_id_2 = [0x02; 16];
    let v2 = enospc_writer
        .append_to_fiber(
            fiber_id,
            event_id_2,
            b"second-event-failing-mid-write-due-to-enospc",
        )
        .expect("append returns undetermined verdict on ENOSPC");
    assert_eq!(v2, WriteLandingVerdict::Undetermined { carried_epoch: 1 });

    let diag = enospc_writer
        .uncertain_diagnostic()
        .expect("uncertain diagnostic present");
    assert!(diag.contains("write_all failed; write landing undetermined:"));
    assert!(diag.contains("simulated ENOSPC: byte write quota exhausted"));

    let len_after_enospc = std::fs::metadata(enospc_writer.pgno_path()).unwrap().len();
    assert_eq!(
        len_after_v1, len_after_enospc,
        "atomic rollback must restore file length to initial boundary"
    );

    let err_subsequent = enospc_writer
        .append_to_fiber(fiber_id, [0x03; 16], b"third-event-refused")
        .expect_err("subsequent append must be refused in uncertain state");
    assert_eq!(
        *err_subsequent.condition(),
        FailureCondition::OwnershipRecordUnreadable
    );

    drop(enospc_writer);

    let mut reader = adapter.open_read().expect("open reader after rollback");
    let envelopes = reader
        .read_all_envelopes()
        .expect("read envelopes from rolled-back container");
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].header.event_id, event_id_1);

    let latest = reader
        .get_latest(fiber_id)
        .expect("get latest")
        .expect("some");
    assert_eq!(latest.header.event_id, event_id_1);
}

#[test]
fn test_reopen_after_enospc_incremental_recovery_truncates_torn_trailing_frame() {
    let dir = tempfile::tempdir().unwrap();
    let stem = dir.path().join("torn_trailing_store");
    let adapter = FileStorageAdapter::new(&stem);
    let claim = sample_claim(1);
    let desc = AdmittedDescriptor::default_for_test();

    let mut writer = adapter.create(&claim, &desc).expect("create writer");
    let fiber_id = derive_fiber_id("account:5005");
    let event_id_1 = [0x11; 16];
    let event_id_2 = [0x22; 16];

    let v1 = writer
        .append_to_fiber(fiber_id, event_id_1, b"account-created")
        .expect("append 1");
    assert!(matches!(v1, WriteLandingVerdict::Landed(_)));
    let v2 = writer
        .append_to_fiber(fiber_id, event_id_2, b"funds-deposited")
        .expect("append 2");
    assert!(matches!(v2, WriteLandingVerdict::Landed(_)));
    drop(writer);

    let valid_len = std::fs::metadata(adapter.pgno_path()).unwrap().len();

    let mut pgno_file = OpenOptions::new()
        .write(true)
        .open(adapter.pgno_path())
        .expect("open pgno for fault injection");
    pgno_file
        .seek(SeekFrom::End(0))
        .expect("seek to end of pgno");

    let mut torn_frame = Vec::new();
    ContainerFrame::encode_payload(b"uncommitted-transaction-payload", &mut torn_frame).unwrap();
    let torn_slice = &torn_frame[..18];
    pgno_file
        .write_all(torn_slice)
        .expect("inject 18 torn bytes");
    pgno_file.sync_data().expect("sync injected torn bytes");
    drop(pgno_file);

    let torn_len = std::fs::metadata(adapter.pgno_path()).unwrap().len();
    assert_eq!(torn_len, valid_len + 18);

    let mut writer2 = adapter
        .open_write(1)
        .expect("open_write must recover and truncate torn trailing frame");

    let truncated_len = std::fs::metadata(adapter.pgno_path()).unwrap().len();
    assert_eq!(
        truncated_len, valid_len,
        "container file must be truncated back to last valid frame boundary"
    );

    let truncation_info = writer2
        .recovered_tail_truncation()
        .expect("recovered_tail_truncation must be recorded");
    assert_eq!(truncation_info.valid_len, valid_len);
    assert_eq!(truncation_info.truncated_bytes, 18);

    let latest_after_reopen = writer2
        .get_latest(fiber_id)
        .expect("get latest after open_write")
        .expect("latest event present");
    assert_eq!(latest_after_reopen.header.event_id, event_id_2);

    let event_id_3 = [0x33; 16];
    let v3 = writer2
        .append_to_fiber(fiber_id, event_id_3, b"funds-transferred-cleanly")
        .expect("new append succeeds after torn frame truncation");
    assert!(matches!(v3, WriteLandingVerdict::Landed(_)));
    drop(writer2);

    let mut reader = adapter
        .open_read()
        .expect("open reader after resumed append");
    let all_envelopes = reader
        .read_all_envelopes()
        .expect("read all envelopes after recovery and append");
    assert_eq!(all_envelopes.len(), 3);
    assert_eq!(all_envelopes[0].header.event_id, event_id_1);
    assert_eq!(all_envelopes[1].header.event_id, event_id_2);
    assert_eq!(all_envelopes[2].header.event_id, event_id_3);
}

#[test]
fn test_torn_trailing_frame_at_various_truncation_points() {
    for torn_bytes_count in [1usize, 3, 4, 10, 25] {
        let dir = tempfile::tempdir().unwrap();
        let stem = dir.path().join("various_torn_store");
        let adapter = FileStorageAdapter::new(&stem);
        let claim = sample_claim(1);
        let desc = AdmittedDescriptor::default_for_test();

        let mut writer = adapter.create(&claim, &desc).expect("create writer");
        let fiber_id = derive_fiber_id("sensor:99");
        let event_id_1 = [0xAA; 16];
        writer
            .append_to_fiber(fiber_id, event_id_1, b"reading-1")
            .expect("append 1");
        drop(writer);

        let valid_len = std::fs::metadata(adapter.pgno_path()).unwrap().len();

        let mut full_frame = Vec::new();
        ContainerFrame::encode_payload(
            b"payload-long-enough-to-allow-various-truncation-slices",
            &mut full_frame,
        )
        .unwrap();
        assert!(full_frame.len() > torn_bytes_count);

        let mut file = OpenOptions::new()
            .write(true)
            .open(adapter.pgno_path())
            .expect("open pgno for torn injection");
        file.seek(SeekFrom::End(0)).expect("seek to end");
        file.write_all(&full_frame[..torn_bytes_count])
            .expect("write torn slice");
        file.sync_data().expect("sync torn slice");
        drop(file);

        let injected_len = std::fs::metadata(adapter.pgno_path()).unwrap().len();
        assert_eq!(injected_len, valid_len + torn_bytes_count as u64);

        let mut recovered_writer = adapter
            .open_write(1)
            .expect("open_write must succeed by truncating torn trailing bytes");
        let truncated_len = std::fs::metadata(adapter.pgno_path()).unwrap().len();
        assert_eq!(
            truncated_len, valid_len,
            "truncation must return exactly to valid_len for {torn_bytes_count} torn bytes"
        );

        let event_id_2 = [0xBB; 16];
        let v2 = recovered_writer
            .append_to_fiber(fiber_id, event_id_2, b"reading-2")
            .expect("subsequent append succeeds");
        assert!(matches!(v2, WriteLandingVerdict::Landed(_)));
    }
}

#[test]
fn test_corrupted_frame_not_torn_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let stem = dir.path().join("corrupt_crc_store");
    let adapter = FileStorageAdapter::new(&stem);
    let claim = sample_claim(1);
    let desc = AdmittedDescriptor::default_for_test();

    let mut writer = adapter.create(&claim, &desc).expect("create writer");
    let fiber_id = derive_fiber_id("sensor:88");
    let event_id_1 = [0x55; 16];
    writer
        .append_to_fiber(fiber_id, event_id_1, b"clean-event-1")
        .expect("append 1");
    drop(writer);

    let mut corrupt_frame = Vec::new();
    ContainerFrame::encode_payload(b"corrupted-checksum-payload", &mut corrupt_frame).unwrap();
    let last_idx = corrupt_frame.len() - 1;
    corrupt_frame[last_idx] ^= 0xFF;

    let mut file = OpenOptions::new()
        .write(true)
        .open(adapter.pgno_path())
        .expect("open pgno");
    file.seek(SeekFrom::End(0)).expect("seek to end");
    file.write_all(&corrupt_frame)
        .expect("write corrupted full frame");
    file.sync_data().expect("sync corrupted full frame");
    drop(file);

    let corrupted_file_len = std::fs::metadata(adapter.pgno_path()).unwrap().len();

    let err_open_write = adapter
        .open_write(1)
        .expect_err("open_write must fail closed on checksum corruption");
    assert_eq!(
        *err_open_write.condition(),
        FailureCondition::PrecursorChainBroken(None)
    );
    assert!(err_open_write.to_string().contains("ChecksumMismatch"));

    let len_after_attempt = std::fs::metadata(adapter.pgno_path()).unwrap().len();
    assert_eq!(
        len_after_attempt, corrupted_file_len,
        "corrupted file must not be truncated"
    );
}

#[test]
fn test_torn_tail_with_zero_run_recovers_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let stem = dir.path().join("zero_run_store");
    let adapter = FileStorageAdapter::new(&stem);
    let claim = sample_claim(1);
    let desc = AdmittedDescriptor::default_for_test();

    let mut writer = adapter.create(&claim, &desc).expect("create writer");
    let fiber_id = derive_fiber_id("sensor:55");
    let event_id_1 = [0x55; 16];
    writer
        .append_to_fiber(fiber_id, event_id_1, b"initial-event")
        .expect("append 1");
    drop(writer);

    let valid_len = std::fs::metadata(adapter.pgno_path()).unwrap().len();

    let mut pgno_file = OpenOptions::new()
        .write(true)
        .open(adapter.pgno_path())
        .expect("open pgno");
    pgno_file.seek(SeekFrom::End(0)).expect("seek end");
    let mut full_frame = Vec::new();
    ContainerFrame::encode_payload(&[0u8; 64], &mut full_frame).unwrap();
    assert_eq!(full_frame.len(), 72);
    let torn_slice = &full_frame[..30];
    pgno_file
        .write_all(torn_slice)
        .expect("write torn tail with zero run");
    pgno_file.sync_data().expect("sync torn tail");
    drop(pgno_file);

    let mut writer2 = adapter
        .open_write(1)
        .expect("open_write must recover torn tail even with zero run per ruling");
    let recovered_len = std::fs::metadata(adapter.pgno_path()).unwrap().len();
    assert_eq!(recovered_len, valid_len);

    let info: TornTailInfo = writer2
        .recovered_tail_truncation()
        .expect("truncation recorded");
    assert_eq!(info.valid_len, valid_len);
    assert_eq!(info.truncated_bytes, 30);

    let event_id_2 = [0x66; 16];
    let v2 = writer2
        .append_to_fiber(fiber_id, event_id_2, b"second-event-after-recovery")
        .expect("append succeeds after zero-run recovery");
    assert!(matches!(v2, WriteLandingVerdict::Landed(_)));
}
