use super::*;

// Pin memory_chunk.h:424-438: reject a short header before reading either word.
#[test]
fn header_size_and_read_failure_follow_pin_load() {
    let calls = std::cell::Cell::new(0);
    let read = || {
        calls.set(calls.get() + 1);
        Err(io::ErrorKind::Other.into())
    };
    assert!(matches!(
        load_header(7, read),
        Err(ChunkLoadError::ShortHeader)
    ));
    assert_eq!(calls.get(), 0);
    assert!(matches!(load_header(8, read), Err(ChunkLoadError::Header)));
    assert_eq!(calls.get(), 1);
    let mut reads = 0;
    assert!(matches!(
        load_header(8, || {
            reads += 1;
            if reads == 1 {
                Ok([0; 4])
            } else {
                Err(io::ErrorKind::Other.into())
            }
        }),
        Err(ChunkLoadError::Header)
    ));
    assert_eq!(reads, 2);
    assert!(load_header(8, || Ok([0; 4])).is_ok());
}

#[test]
fn short_payload_is_checksummed_at_the_returned_length() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chunk");
    // The checksum names the single returned byte, not the whole payload.
    std::fs::write(&path, [4, 0, 0, 0, 1, 0, 0, 0, 1, 2, 3, 4]).unwrap();
    let calls = std::cell::Cell::new(0);
    let chunk = load_with(&path, |source, buffer| {
        calls.set(calls.get() + 1);
        source.read_payload(buffer, Some(1))
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(chunk.length, 4_u32.to_le_bytes());
    assert_eq!(chunk.finish(|bytes| u32::from(bytes[0])), Some(vec![1]));

    // The same short result against a different checksum is a refusal.
    std::fs::write(&path, [4, 0, 0, 0, 2, 0, 0, 0, 1, 2, 3, 4]).unwrap();
    let chunk = load_with(&path, |source, buffer| source.read_payload(buffer, Some(1))).unwrap();
    assert_eq!(chunk.finish(|bytes| u32::from(bytes[0])), None);
}

#[test]
fn interrupted_payload_read_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chunk");
    std::fs::write(&path, [4, 0, 0, 0, 1, 0, 0, 0, 1, 2, 3, 4]).unwrap();
    let calls = std::cell::Cell::new(0);
    let Err(ChunkLoadError::Io(error)) = load_with(&path, |_, _| {
        calls.set(calls.get() + 1);
        Err(io::ErrorKind::Interrupted.into())
    }) else {
        panic!("interrupted payload must fail")
    };
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert_eq!(calls.get(), 1);
}

#[test]
fn file_size_is_narrowed_before_length_comparison() {
    let declared = 4_u32;
    assert_eq!(available_payload_length(8 + 4), declared);
    assert_eq!(available_payload_length(8 + 4 + (1_u64 << 32)), declared);
    assert_ne!(available_payload_length(8 + 5 + (1_u64 << 32)), declared);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chunk");
    std::fs::write(&path, [4, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5]).unwrap();
    let called = std::cell::Cell::new(false);
    assert!(matches!(
        load_with(&path, |_, _| {
            called.set(true);
            Ok(0)
        }),
        Err(ChunkLoadError::Io(_))
    ));
    assert!(!called.get(), "framing fails before reading the payload");
}

// Windows only: the wide-character (UTF-16) file API opens the path.
#[cfg(windows)]
#[test]
fn mapping_opens_a_non_ascii_windows_path() {
    let root = tempfile::tempdir().unwrap();
    let dir = crate::input::path::build_chunk_path(root.path(), std::ffi::OsStr::new("目录"));
    std::fs::create_dir(&dir).unwrap();
    let path = crate::input::path::build_chunk_path(&dir, std::ffi::OsStr::new("词库.bin"));
    save(&path, &[1_u8, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    let mut source = MappingSource::open(&path).unwrap();
    assert_eq!(source.file_size(), 8);
    assert_eq!(source.read_word().unwrap(), [1, 2, 3, 4]);
    assert_eq!(source.read_word().unwrap(), [5, 6, 7, 8]);
    assert_eq!(
        source.map().unwrap().finish().as_slice(),
        &[1, 2, 3, 4, 5, 6, 7, 8]
    );
}

#[test]
fn one_mapping_owns_all_file_bytes_and_keeps_writes_private() {
    let dir = tempfile::tempdir().unwrap();
    for len in [1_usize, 7, 8, 4095, 4096, 4097, 3 * 4096 + 1] {
        let path = dir.path().join(format!("len-{len}"));
        let bytes: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        let source = MappingSource::open(&path).unwrap();
        assert_eq!(source.file_size(), len as u64);
        let pending = source.map().unwrap();
        assert_eq!(pending.as_slice(), bytes, "{len}");
        let mut mapping = pending.finish();
        // Linux only: /proc/self/maps names the mapping's permissions.
        #[cfg(target_os = "linux")]
        {
            let ranges = file_mappings(&path);
            assert_eq!(ranges.len(), 1);
            assert_eq!(ranges[0].split_whitespace().nth(1), Some("rw-p"));
        }
        mapping.as_mut_slice()[0] = 255;
        assert_eq!(mapping.as_slice()[0], 255);
        assert_eq!(&mapping.as_slice()[1..], &bytes[1..]);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "private writes stay private"
        );
    }
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

// Linux only: /proc/self/fd and /proc/self/maps.
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

    let rejected = MappingSource::open(&path).unwrap().map().unwrap();
    assert_eq!(open_descriptors(&path), 1);
    drop(rejected);
    assert_eq!(open_descriptors(&path), 0);
    assert!(file_mappings(&path).is_empty());
}

// Linux only: /proc/self/maps, and a 4 GiB sparse mapping on a 64-bit host.
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
#[test]
fn sparse_file_has_one_whole_writable_private_mapping_and_no_residual() {
    // Row 109's source-derived witness: the pin's u32 data length wraps
    // to zero, but mmap still receives the full 2^32 + 8 file size.
    // Only sparse file length and virtual extent grow; no payload is read.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sparse");
    let file_size = (1_u64 << 32) + 8;
    std::fs::File::create(&path)
        .unwrap()
        .set_len(file_size)
        .unwrap();
    let mut source = MappingSource::open(&path).unwrap();
    assert_eq!(source.file_size(), file_size);
    assert_eq!(source.read_word().unwrap(), [0; 4]);
    assert_eq!(source.read_word().unwrap(), [0; 4]);
    let mapping = source.map().unwrap().finish();
    assert_eq!(mapping.as_slice().len() as u64, file_size);
    let ranges = file_mappings(&path);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].split_whitespace().nth(1), Some("rw-p"));
    assert_eq!(open_descriptors(&path), 0);
    drop(mapping);
    assert!(file_mappings(&path).is_empty());
}

#[test]
fn header_reads_are_separate_words_and_short_reads_fail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("header");
    std::fs::write(&path, [1, 2, 3, 4, 5, 6, 7, 8, 9]).unwrap();
    let mut source = MappingSource::open(&path).unwrap();
    assert_eq!(source.read_word().unwrap(), [1, 2, 3, 4]);
    assert_eq!(source.read_word().unwrap(), [5, 6, 7, 8]);
    assert_eq!(
        source.read_word().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

fn chunk(len: usize) -> Vec<u8> {
    let mut bytes = vec![0_u8; 8 + len];
    bytes[..4].copy_from_slice(&(len as u32).to_ne_bytes());
    for (at, byte) in bytes[8..].iter_mut().enumerate() {
        *byte = at as u8;
    }
    bytes
}

#[test]
fn a_chunk_is_the_written_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chunk");
    let bytes = chunk(3);
    save(&path, &bytes).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

/// tests/include/test_memory_chunk.cpp:44-65 at 074a2219: the two
/// surviving words, then `save`, `load` and `mmap` of one file.
#[test]
fn framed_chunk_replaces_an_old_file_and_reads_through_heap_and_map() {
    // The file the pin's test_memory_chunk leaves after the removal:
    // data length 8, checksum 10 ^ 12, then the words 10 and 12.
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

#[test]
fn an_unopenable_path_is_the_soft_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing").join("chunk");
    assert!(matches!(save(&path, &chunk(0)), Err(ChunkSaveError::Io(_))));
}

// Windows only: the read-only file attribute.
#[cfg(windows)]
#[test]
fn saving_over_a_read_only_file_is_permission_denied() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("readonly.bin");
    std::fs::write(&path, chunk(0)).unwrap();
    let original = std::fs::metadata(&path).unwrap().permissions();
    let mut permissions = original.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    let result = save(&path, &chunk(1));
    std::fs::set_permissions(&path, original).unwrap();
    let Err(ChunkSaveError::Io(error)) = result else {
        panic!("expected an I/O error")
    };
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(error.raw_os_error(), Some(5));
}

// Linux only: /dev/full.
#[cfg(target_os = "linux")]
#[test]
fn a_header_write_failure_is_the_abort_site() {
    // /dev/full opens for writing and refuses every write, which is the
    // pin's first header `assert` (memory_chunk.h:543).
    assert!(matches!(
        save(Path::new("/dev/full"), &chunk(0)),
        Err(ChunkSaveError::Header {
            field: ChunkHeaderField::Length,
            ..
        })
    ));
}
