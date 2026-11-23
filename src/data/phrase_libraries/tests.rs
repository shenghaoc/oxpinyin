use super::*;
use crate::data::phrase_library::verify_chunk;

// Rust PhraseLibraries::open leaves absent slots unloaded.
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

// Rust PhraseLibraries::load refuses a slot outside the library array.
#[test]
fn out_of_range_nibbles_are_refused() {
    let mut libs = PhraseLibraries::empty();
    let (_dir, path) = temp_library(&chunk(&valid_empty_payload()));
    assert!(matches!(libs.load(16, &path), Err(LibraryError::Format(_))));
    assert!(!libs.is_loaded(16));
    assert!(libs.library(0xFF00_0001).is_none());
}

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

// 074a2219 phrase_index.cpp:237-248,335-364 retains an empty sub-index after layout failure.
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

// 074a2219 phrase_index.cpp:335-364 accepts an empty, correctly framed sub-index.
#[test]
fn a_valid_empty_chunk_is_mapped_not_layout_failed() {
    let (dir, _) = temp_library(&chunk(&valid_empty_payload()));
    let libs = PhraseLibraries::open(dir.path(), &[(4_u8, "merged.bin")]).unwrap();
    let library = libs.library(0x0400_0000).expect("loaded");
    assert!(!library.is_layout_failed());
    assert_eq!(library.token_range(), 1..1);
}

// Rust layout validation refuses an out-of-bounds separator index (row 76).
#[test]
fn an_out_of_bounds_separator_index_stays_a_format_refusal() {
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

// 074a2219 phrase_index.cpp:237-273 subtracts the old local total before replacing or unloading.
#[test]
fn a_failed_layout_keeps_its_local_total_out_of_the_facade_total() {
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
    let path = dir.path().join("merged.bin");
    libs.insert_mapping(
        4,
        path.clone(),
        PhraseLibrary::open_retaining_layout(&path).unwrap(),
    );
    assert_eq!(libs.unigram_total(), 0_u32.wrapping_sub(123));
    assert!(libs.unload(4));
    assert_eq!(libs.unigram_total(), 0_u32.wrapping_sub(246));
}

#[cfg(unix)]
mod phrase_library {
    use super::{chunk, valid_empty_payload};
    use crate::data::PhraseLibraries;

    // Rust map_library reports an unmappable path and keeps previously mapped libraries.
    #[test]
    fn libraries_report_absent_files() {
        let dir = tempfile::tempdir().expect("scratch directory");
        let first = dir.path().join("a.bin");
        std::fs::write(&first, chunk(&valid_empty_payload())).expect("write a.bin");
        let mut libraries = PhraseLibraries::empty();
        let mut reported = Vec::new();
        for (nibble, file) in [(1, "a.bin"), (2, "b.bin")] {
            libraries
                .map_library(nibble, dir.path(), file.as_ref(), &mut |path| {
                    reported.push(path.to_owned())
                })
                .unwrap();
        }
        assert!(libraries.is_loaded(1) && !libraries.is_loaded(2));
        assert_eq!(reported, [dir.path().join("b.bin")]);
    }
}
