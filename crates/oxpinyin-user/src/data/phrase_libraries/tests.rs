use super::*;

#[test]
fn open_of_an_empty_directory_loads_nothing() {
    let dir = tempfile::Builder::new()
        .prefix("oxpinyin-empty-libs-")
        .tempdir()
        .unwrap();
    let mut libs = PhraseLibraries::open(dir.path(), SYSTEM_LIBRARY_STEMS).unwrap();
    assert!(libs.is_empty());
    assert!(libs.phrase_text(0x0100_0001).is_none());
    assert_eq!(libs.unigram_total(), 0);
    assert!(!libs.is_loaded(1));
    assert!(!libs.unload(1));
    dir.close().unwrap();
}

#[test]
fn out_of_range_nibbles_are_refused() {
    let mut libs = PhraseLibraries::empty();
    assert!(libs.load(16, Path::new("/no/such/file")).is_err());
    assert!(!libs.is_loaded(16));
    assert!(libs.library(0xFF00_0001).is_none());
}

/// A `MemoryChunk` image: `{length: u32, checksum: u32}` then `payload`.
fn chunk(payload: &[u8]) -> Vec<u8> {
    let mut checksum = 0_u32;
    let aligned = payload.len() & !0x3;
    for word in payload[..aligned].as_chunks::<4>().0.iter() {
        checksum ^= u32::from_le_bytes(*word);
    }
    for (shift, byte) in payload[aligned..].iter().enumerate() {
        checksum ^= u32::from(*byte) << (8 * shift);
    }
    let mut bytes = Vec::with_capacity(8 + payload.len());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&checksum.to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// A valid empty `SubPhraseIndex`: total 0, the four words, then the
/// three separators with an empty offset array and an empty entry area.
fn valid_empty_payload() -> Vec<u8> {
    let mut payload = vec![0_u8; 19];
    payload[4..8].copy_from_slice(&17_u32.to_le_bytes());
    payload[8..12].copy_from_slice(&18_u32.to_le_bytes());
    payload[12..16].copy_from_slice(&19_u32.to_le_bytes());
    payload[16..19].fill(b'#');
    payload
}

fn temp_library(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("oxpinyin-layout-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("merged.bin");
    std::fs::write(&path, bytes).unwrap();
    (dir, path)
}

#[test]
fn a_header_only_chunk_loads_the_pins_empty_sub_index() {
    let (dir, path) = temp_library(&[0_u8; 8]);
    assert!(verify_chunk(&path).is_ok(), "the pin's mmap accepts it");
    let libs = PhraseLibraries::open(dir.path(), &[(4_u8, "merged.bin")]).unwrap();
    assert!(libs.is_loaded(4));
    assert!(libs.phrase_text(0x0400_0001).is_none());
    assert_eq!(libs.unigram_total(), 0);
    let library = libs.library(0x0400_0000).expect("loaded");
    assert!(library.is_layout_failed());
    assert_eq!(library.token_range(), 1..1);
    assert!(!library.has_items());
    assert_eq!(library.total_freq(), 0);
}

#[test]
fn a_valid_empty_chunk_is_mapped_not_layout_failed() {
    let (dir, _) = temp_library(&chunk(&valid_empty_payload()));
    let libs = PhraseLibraries::open(dir.path(), &[(4_u8, "merged.bin")]).unwrap();
    let library = libs.library(0x0400_0000).expect("loaded");
    assert!(!library.is_layout_failed());
    assert_eq!(library.token_range(), 1..1);
}

#[test]
fn an_out_of_bounds_separator_index_stays_a_format_refusal() {
    // The pin would read payload[index_two - 1] past the chunk; this
    // reader refuses the layout instead of reproducing the read.
    let mut payload = vec![0_u8; 32];
    payload[4..8].copy_from_slice(&17_u32.to_le_bytes());
    payload[8..12].copy_from_slice(&20_u32.to_le_bytes());
    payload[12..16].copy_from_slice(&21_u32.to_le_bytes());
    payload[16] = b'#';
    payload[8..12].copy_from_slice(&100_u32.to_le_bytes());
    let (dir, _) = temp_library(&chunk(&payload));
    let error = match PhraseLibraries::open(dir.path(), &[(4_u8, "merged.bin")]) {
        Err(error) => error,
        Ok(_) => panic!("an out-of-bounds layout must be refused"),
    };
    assert!(matches!(error, LibraryError::Format(_)), "{error}");
}

#[test]
fn a_failed_layout_keeps_its_local_total_out_of_the_facade_total() {
    // `FacadePhraseIndex::load` subtracts the old total, then returns
    // before adding the newly read one (`phrase_index.cpp:237-248`).
    // The 123 stays on the empty sub-index, not on the facade aggregate.
    let mut payload = vec![0_u8; 32];
    payload[0..4].copy_from_slice(&123_u32.to_le_bytes());
    payload[4..8].copy_from_slice(&17_u32.to_le_bytes());
    payload[8..12].copy_from_slice(&20_u32.to_le_bytes());
    payload[12..16].copy_from_slice(&24_u32.to_le_bytes());
    payload[16] = b'#';
    payload[19] = b'!';
    let (dir, _) = temp_library(&chunk(&payload));
    let mut libs = PhraseLibraries::open(dir.path(), &[(4_u8, "merged.bin")]).unwrap();
    let library = libs.library(0x0400_0000).expect("loaded");
    assert_eq!(library.total_freq(), 123, "the local total is preserved");
    assert!(library.is_layout_failed());
    assert_eq!(libs.unigram_total(), 0, "the facade does not add it");
    // tests/storage/test_phrase_index_logger.cpp:29-30 and :53-54 at
    // 074a2219 load sub-index 1 twice. Reattaching the same failed chunk
    // also subtracts the previous sub-index total and declines to add
    // the refused replacement.
    libs.load_heap(4, &dir.path().join("merged.bin")).unwrap();
    assert_eq!(libs.unigram_total(), 0_u32.wrapping_sub(123));
    // The pin's `FacadePhraseIndex::unload` subtracts the local total
    // whether or not the failed load added it.
    assert!(libs.unload(4));
    assert_eq!(libs.unigram_total(), 0_u32.wrapping_sub(246));
}

#[test]
fn an_in_bounds_second_separator_fault_is_a_layout_fault() {
    let mut payload = vec![0_u8; 32];
    payload[4..8].copy_from_slice(&17_u32.to_le_bytes());
    payload[8..12].copy_from_slice(&20_u32.to_le_bytes());
    payload[12..16].copy_from_slice(&21_u32.to_le_bytes());
    payload[16] = b'#';
    payload[19] = b'!';
    let (dir, _) = temp_library(&chunk(&payload));
    let libs = PhraseLibraries::open(dir.path(), &[(4_u8, "merged.bin")]).unwrap();
    let library = libs.library(0x0400_0000).expect("loaded");
    assert!(library.is_layout_failed());
    assert_eq!(library.token_range(), 1..1);
}

/// The phrase-library suite's absent-file test; it builds its chunk with the shared
/// `ChunkBuilder` of `tests/support/mod.rs`.
#[cfg(unix)]
mod phrase_library {
    use crate::data::PhraseLibraries;
    use crate::data::support::ChunkBuilder;

    /// 你 (slot 4, one pronunciation of 2 packed keys) and 你好 (slot 9,
    /// two pronunciations of 4 packed keys) — the shapes the decode tests
    /// read back.
    fn sample() -> Vec<u8> {
        let mut builder = ChunkBuilder::new(9);
        builder.add(4, 5, "你", vec![(vec![0x12, 0x34], 5)]);
        builder.add(
            9,
            4,
            "你好",
            vec![
                (vec![0x12, 0x34, 0x56, 0x78], 3),
                (vec![0xAA, 0xBB, 0xCC, 0xDD], 1),
            ],
        );
        builder.build()
    }

    // ── the files the pin's `MemoryChunk::mmap` refuses (#545) ──────────

    #[cfg(unix)]
    #[test]
    fn libraries_record_absent_files() {
        let dir = tempfile::tempdir().expect("scratch directory");
        let first = dir.path().join("a.bin");
        std::fs::write(&first, sample()).expect("write a.bin");
        let libraries = PhraseLibraries::open(dir.path(), &[(1, "a.bin"), (2, "b.bin")])
            .expect("b.bin is absent");
        assert!(libraries.is_loaded(1) && !libraries.is_loaded(2));
        assert_eq!(
            libraries.unmapped().collect::<Vec<_>>(),
            [dir.path().join("b.bin")]
        );
    }
}
