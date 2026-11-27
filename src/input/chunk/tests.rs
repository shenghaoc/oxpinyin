use super::*;
use crate::input::chunk::HeaderFault;

// 074a2219 memory_chunk.h:426–438 checks size before the two reads; mask prefetch defers read faults.
#[test]
fn header_size_and_read_failure_follow_pin_load() {
    for (size, failed_read, expected_reads, expected) in [
        (7, None, 0, Err("short")),
        (8, Some(1), 1, Err("header")),
        (8, Some(2), 2, Err("header")),
        (8, None, 2, Ok(())),
    ] {
        let mut reads = 0;
        let result = load_header(size, HeaderFault::Defer, || {
            reads += 1;
            if failed_read == Some(reads) {
                Err(io::ErrorKind::Other.into())
            } else {
                Ok([0; 4])
            }
        });
        assert_eq!(
            result.map(|_| ()).map_err(|error| match error {
                ChunkLoadError::ShortHeader => "short",
                ChunkLoadError::Header => "header",
                ChunkLoadError::Io(_) => "io",
            }),
            expected
        );
        assert_eq!(reads, expected_reads);
    }
}

// 074a2219 memory_chunk.h:432–438 aborts on a failed header word.
#[test]
fn length_read_aborts_at_load_header() {
    crate::store::death_test::assert_aborts(
        "input::chunk::tests::length_read_aborts_at_load_header",
        || {
            let _ = load_header(8, HeaderFault::Abort, || Err(io::ErrorKind::Other.into()));
        },
    );
}

// 074a2219 memory_chunk.h:452–462 checks the returned payload prefix without retry.
#[test]
fn short_payload_is_checksummed_at_the_returned_length() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chunk");
    for (checksum, expected) in [(1, Some(vec![1])), (2, None)] {
        std::fs::write(&path, [4, 0, 0, 0, checksum, 0, 0, 0, 1, 2, 3, 4]).unwrap();
        let calls = std::cell::Cell::new(0);
        let chunk = load_with(&path, HeaderFault::Abort, |source, buffer| {
            calls.set(calls.get() + 1);
            source.read_payload(buffer, Some(1))
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(chunk.finish(|bytes| u32::from(bytes[0])), expected);
    }
}

// Register row 114 refuses a negative payload read without retry.
#[test]
fn interrupted_payload_read_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chunk");
    std::fs::write(&path, [4, 0, 0, 0, 1, 2, 3, 4, 1, 2, 3, 4]).unwrap();
    let calls = std::cell::Cell::new(0);
    let Err(ChunkLoadError::Io(error)) = load_with(&path, HeaderFault::Abort, |_, _| {
        calls.set(calls.get() + 1);
        Err(io::ErrorKind::Interrupted.into())
    }) else {
        panic!("interrupted payload must fail")
    };
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert_eq!(calls.get(), 1);
}

// 074a2219 memory_chunk.h:440–444 narrows file size before comparing the declared length.
#[test]
fn file_size_is_narrowed_before_length_comparison() {
    for (size, matches) in [
        (12, true),
        (12 + (1_u64 << 32), true),
        (13 + (1_u64 << 32), false),
    ] {
        assert_eq!(available_payload_length(size) == 4, matches);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chunk");
    std::fs::write(&path, [4, 0, 0, 0, 4, 2, 3, 4, 1, 2, 3, 4, 5]).unwrap();
    let called = std::cell::Cell::new(false);
    assert!(matches!(
        load_with(&path, HeaderFault::Abort, |_, _| {
            called.set(true);
            Ok(0)
        }),
        Err(ChunkLoadError::Io(_))
    ));
    assert!(!called.get());
}

#[cfg(target_os = "linux")]
fn open_descriptors(path: &Path) -> usize {
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| std::fs::read_link(entry.path()).is_ok_and(|target| target == path))
        .count()
}

#[cfg(target_os = "linux")]
fn file_mappings(path: &Path) -> Vec<String> {
    let suffix = format!(" {}", path.display());
    std::fs::read_to_string("/proc/self/maps")
        .unwrap()
        .lines()
        .filter(|line| line.ends_with(&suffix))
        .map(str::to_owned)
        .collect()
}

// 074a2219 memory_chunk.h:505–523 and row 109: descriptor and mapping ownership, via Linux /proc.
#[cfg(target_os = "linux")]
#[test]
fn validation_owns_the_descriptor_and_finish_closes_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("descriptor");
    std::fs::write(&path, [0_u8; 8]).unwrap();
    let source = MappingSource::open(&path).unwrap();
    assert_eq!(open_descriptors(&path), 1);
    let pending = source.map().unwrap();
    assert_eq!(open_descriptors(&path), 1);
    assert_eq!(file_mappings(&path).len(), 1);
    let mapping = pending.finish();
    assert_eq!(open_descriptors(&path), 0);
    assert_eq!(file_mappings(&path).len(), 1);
    drop(mapping);
    assert!(file_mappings(&path).is_empty());
}

// 074a2219 memory_chunk.h:536–557 truncates and writes; tests/include/test_memory_chunk.cpp:44–65 supplies the frame.
#[test]
fn framed_chunk_replaces_an_old_file_and_reads_through_heap_and_map() {
    let file = [8_u32, 10 ^ 12, 10, 12]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<u8>>();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.bin");
    std::fs::write(&path, b"old bytes that must be truncated").unwrap();
    save(&path, &file).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), file);

    let mapped = MappingSource::open(&path).unwrap().map().unwrap();
    assert_eq!(mapped.as_slice(), file.as_slice());
    drop(mapped);

    assert!(matches!(
        save(dir.path(), &file),
        Err(ChunkSaveError::Io(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), file);
}

// Rust Windows save preserves the native access-denied code for a read-only file.
#[cfg(windows)]
#[test]
fn saving_over_a_read_only_file_is_permission_denied() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("readonly.bin");
    std::fs::write(&path, [0; 8]).unwrap();
    let original = std::fs::metadata(&path).unwrap().permissions();
    let mut permissions = original.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    let result = save(&path, &[1, 0, 0, 0, 0, 0, 0, 0, 0]);
    std::fs::set_permissions(&path, original).unwrap();
    let Err(ChunkSaveError::Io(error)) = result else {
        panic!("expected an I/O error")
    };
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(error.raw_os_error(), Some(5));
}

// 074a2219 memory_chunk.h:542–547 aborts on failed header writes; Linux /dev/full supplies the failure.
#[cfg(target_os = "linux")]
#[test]
fn a_header_write_failure_is_the_abort_site() {
    crate::store::death_test::assert_aborts(
        "input::chunk::tests::a_header_write_failure_is_the_abort_site",
        || {
            let _ = save(Path::new("/dev/full"), &[0; 8]);
        },
    );
}
