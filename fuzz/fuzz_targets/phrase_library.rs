#![no_main]
//! Hostile bytes through the MemoryChunk phrase-library reader — the
//! mmap'd per-library file `pinyin_init` opens sixteen of from a system
//! directory a distro or a user wrote. `PhraseLibrary::open` must return
//! a value on every input, every accessor over an opened library must
//! stay in bounds, and both must be deterministic.
//!
//! The chunk header is a length word plus a checksum over the payload;
//! random bytes fail it at once, so the target treats its input as the
//! payload and writes a valid header in front, which is what lets the
//! fuzzer reach the sub-index and item decoders behind the checksum.

use std::io::Write;

use libfuzzer_sys::fuzz_target;
use oxpinyin_data::chunk_format::{chunk_checksum, CHUNK_HEADER_SIZE};
use oxpinyin_data::phrase_library::PhraseLibrary;

fn scratch_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "oxpinyin-fuzz-phrase-library-{}",
        std::process::id()
    ))
}

fn write_chunk(path: &std::path::Path, payload: &[u8]) {
    let mut bytes = Vec::with_capacity(CHUNK_HEADER_SIZE + payload.len());
    let len = u32::try_from(payload.len()).expect("fuzz inputs fit a u32 length word");
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(&chunk_checksum(payload).to_le_bytes());
    bytes.extend_from_slice(payload);
    let mut file = std::fs::File::create(path).expect("scratch file");
    file.write_all(&bytes).expect("scratch write");
}

fn walk(library: &PhraseLibrary) -> Vec<(u32, Option<String>, usize, usize, u64)> {
    let mut rows = Vec::new();
    let _ = library.total_freq();
    let _ = library.token_range();
    for (token, item) in library.items() {
        let mut n = 0usize;
        let mut freq_sum = 0u64;
        for pronunciation in item.pronunciations() {
            freq_sum += u64::from(pronunciation.freq) + pronunciation.keys.len() as u64;
            n += 1;
        }
        let _ = item.pronunciation(0);
        let _ = item.unigram();
        let _ = item.n_pronunciations();
        let _ = library.item(token);
        rows.push((token, item.phrase_text(), item.phrase_length(), n, freq_sum));
    }
    rows
}

fuzz_target!(|payload: &[u8]| {
    let path = scratch_path();
    write_chunk(&path, payload);
    let first = PhraseLibrary::open(&path).map(|lib| walk(&lib));
    let second = PhraseLibrary::open(&path).map(|lib| walk(&lib));
    match (first, second) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "decode must be deterministic"),
        (Err(_), Err(_)) => {}
        _ => panic!("open must be deterministic"),
    }
});
