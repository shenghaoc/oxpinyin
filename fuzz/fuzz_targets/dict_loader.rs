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

use oxpinyin::data::phrase_library::PhraseLibrary;

fn walk(library: &PhraseLibrary) -> Vec<(u32, usize, usize)> {
    let _ = library.total_freq();
    let _ = library.token_range();
    library
        .items()
        .map(|(token, item)| (token, item.phrase_length(), item.n_pronunciations()))
        .collect()
}

/// Inputs per child: AFL++'s suggested start
/// (`instrumentation/README.persistent_mode.md`), which bounds a child's
/// lifetime. `AFL_FUZZER_LOOPCOUNT` overrides it.
const LOOP_COUNT: usize = 1000;

/// Valid chunk files: the first open of one builds `memmap2`'s page size.
const WARM_UP: &[&[u8]] = &[
    include_bytes!("../../fixtures/w3/kct/art.bin"),
    include_bytes!("../../fixtures/w3/kct/opengram.bin"),
];

fn main() {
    oxpinyin_fuzz::fuzz(LOOP_COUNT, WARM_UP, run);
}

/// Runs one input. The input's file exists only while it runs.
fn run(data: &[u8]) {
    let mut file = tempfile::Builder::new()
        .prefix("oxpinyin-fuzz-dict-loader-")
        .tempfile()
        .expect("scratch file");
    file.write_all(data).expect("scratch write");
    file.flush().expect("scratch flush");
    let path = file.path();
    let first = PhraseLibrary::open(path).map(|library| walk(&library));
    let second = PhraseLibrary::open(path).map(|library| walk(&library));
    match (first, second) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "decode must be deterministic"),
        (Err(_), Err(_)) => {}
        _ => panic!("open must be deterministic"),
    }
}
