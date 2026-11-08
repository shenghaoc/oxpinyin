#![no_main]
//! Hostile bytes through the system chunk loader: `PhraseLibrary::open` is the
//! entry `pinyin_init` uses for the per-library `.bin` files of a system
//! directory, so the input is written to a file as it stands, header and
//! checksum included. It must return a value on every input (no panic, no
//! allocator abort), every accessor over an opened library must stay in
//! bounds, and both must be deterministic. The `phrase-library` target writes
//! a valid header in front of its input to reach the decoders behind the
//! checksum; this one starts from whole files, so the committed `.bin`
//! fixtures seed the header and checksum checks themselves.

use std::io::Write;

use libfuzzer_sys::fuzz_target;
use oxpinyin_data::phrase_library::PhraseLibrary;

fn scratch_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("oxpinyin-fuzz-dict-loader-{}", std::process::id()))
}

fn walk(library: &PhraseLibrary) -> Vec<(u32, usize, usize)> {
    let _ = library.total_freq();
    let _ = library.token_range();
    library
        .items()
        .map(|(token, item)| (token, item.phrase_length(), item.n_pronunciations()))
        .collect()
}

fuzz_target!(|data: &[u8]| {
    let path = scratch_path();
    let mut file = std::fs::File::create(&path).expect("scratch file");
    file.write_all(data).expect("scratch write");
    drop(file);
    let first = PhraseLibrary::open(&path).map(|library| walk(&library));
    let second = PhraseLibrary::open(&path).map(|library| walk(&library));
    match (first, second) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "decode must be deterministic"),
        (Err(_), Err(_)) => {}
        _ => panic!("open must be deterministic"),
    }
});
