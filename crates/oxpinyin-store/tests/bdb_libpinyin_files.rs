//! The Berkeley DB backend against files the real libpinyin wrote.
//!
//! Every other test in this crate reads files oxpinyin itself produced,
//! which proves the backend is self-consistent and nothing about
//! compatibility. These read the `bigram.db` that the installed
//! `libpinyin-data` package shipped — 25.9 MB written by libpinyin's own
//! build, through the same system libdb this backend links.
//!
//! # Why the round-trip is the write gate
//!
//! A drop-in that trains a user's profile writes back into libpinyin's
//! files. If the bytes it writes differ from the bytes libpinyin would
//! have written, the user's own libpinyin misreads its own profile and
//! there is no error anywhere — the corruption is silent. So the write
//! path's gate is not "does it round-trip through us" but "does
//! `encode(decode(x))` equal `x` for records libpinyin actually wrote",
//! over every record in a real file rather than a handful of samples.
//!
//! # Presence
//!
//! Skipped with a loud diagnostic when the package is not installed, so
//! the suite still runs on a machine without it. `OXPINYIN_BDB_STRICT=1`
//! turns the skip into a failure, which is what a machine that is
//! supposed to have the data should set — the same discipline
//! `oxpinyin-datagen`'s `OXPINYIN_DATAGEN_STRICT` uses.

#![cfg(feature = "bdb")]

use std::path::{Path, PathBuf};

use oxpinyin_store::{BigramDb, SingleGram};

/// Where the distro installs libpinyin's runtime data.
///
/// `$(libdir)/libpinyin/data`, from `data/Makefile.am`'s
/// `libpinyin_dbdir` — a multiarch *library* path, not `$datadir`.
/// Anything looking under `share` finds nothing.
fn system_data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OXPINYIN_LIBPINYIN_DATA_DIR") {
        let dir = PathBuf::from(dir);
        return dir.is_dir().then_some(dir);
    }
    [
        "/usr/lib/x86_64-linux-gnu/libpinyin/data",
        "/usr/lib64/libpinyin/data",
        "/usr/lib/libpinyin/data",
        "/usr/local/lib/libpinyin/data",
    ]
    .into_iter()
    .map(Path::new)
    .find(|dir| dir.is_dir())
    .map(Path::to_path_buf)
}

/// The system `bigram.db`, or `None` with the skip already reported.
fn system_bigram(what: &str) -> Option<PathBuf> {
    let file = system_data_dir().map(|dir| dir.join("bigram.db"));
    match file {
        Some(file) if file.is_file() => Some(file),
        _ => {
            let message = format!(
                "SKIP {what}: no installed libpinyin bigram.db found. This test is the \
                 only one that reads a file libpinyin itself wrote; without it the suite \
                 says nothing about compatibility. Install the distro's libpinyin data \
                 package (Debian/Ubuntu: libpinyin-data) or set \
                 OXPINYIN_LIBPINYIN_DATA_DIR."
            );
            assert!(
                std::env::var_os("OXPINYIN_BDB_STRICT").is_none(),
                "{message} (OXPINYIN_BDB_STRICT is set, so this skip is a failure)"
            );
            eprintln!("{message}");
            None
        }
    }
}

#[test]
fn every_record_of_the_real_system_bigram_round_trips_byte_for_byte() {
    let Some(path) = system_bigram("bigram round-trip") else {
        return;
    };
    let db = BigramDb::open(&path, true).expect("open the system bigram.db read-only");

    let mut records = 0_u64;
    let mut items = 0_u64;
    db.for_each(&mut |prev, gram| {
        records += 1;
        items += gram.len() as u64;
        // The write gate: our bytes for this gram must be the bytes the
        // file already holds. `SingleGram::decode` has already rejected
        // any chunk that is not 4 + 8n, is not ascending, or is a
        // zero-item chunk with a total, so reaching here is also an
        // assertion that libpinyin's own file satisfies every invariant
        // this backend relies on.
        let encoded = gram.encode();
        let stored = db
            .raw(prev)
            .expect("re-read the record we are visiting")
            .expect("the record we are visiting exists");
        assert_eq!(
            encoded, stored,
            "gram for prev 0x{prev:08x} does not re-encode to the bytes libpinyin wrote"
        );
        // The total is libpinyin's own, not a sum we computed.
        let sum: u64 = gram.items().iter().map(|(_, freq)| u64::from(*freq)).sum();
        assert_eq!(
            u64::from(gram.total()),
            sum,
            "prev 0x{prev:08x}: total_freq must equal the sum of its items"
        );
        Ok(())
    })
    .expect("walk the system bigram.db");

    // The counts `oxpinyin-datagen` derives independently from model20
    // (docs/findings/datagen-model20.md's equivalence proof). Two
    // unrelated routes to the same two numbers is what makes this a
    // format check rather than a self-consistency check.
    assert_eq!(
        records, 56_359,
        "the installed bigram.db should hold the same 56,359 records datagen derives \
         from model20; a different count means a different data release, so compare \
         against that release's own figures before trusting this run"
    );
    assert_eq!(items, 1_849_609, "and the same 1,849,609 successor records");
}

#[test]
fn a_known_record_decodes_to_the_bytes_the_c_probe_read() {
    let Some(path) = system_bigram("known-record decode") else {
        return;
    };
    let db = BigramDb::open(&path, true).expect("open the system bigram.db read-only");

    // Transcribed from a standalone C probe over the same file, driving
    // libdb directly with no Rust involved: prev 0x03001801 has
    // total_freq 65 and four successors.
    let gram = db
        .get(0x0300_1801)
        .expect("point get")
        .expect("prev 0x03001801 is present in the shipped data");
    assert_eq!(gram.total(), 65);
    assert_eq!(
        gram.items(),
        &[
            (0x0100_05db, 8),
            (0x0100_298c, 3),
            (0x0100_6538, 2),
            (0x0100_a271, 52),
        ],
        "the decoded items must match what the C probe read from the same record"
    );
    assert_eq!(
        gram.freq(0x0100_a271),
        Some(52),
        "point lookup in the array"
    );
    assert_eq!(gram.freq(0x0100_0000), None, "a token the row misses");
}

#[test]
fn a_profile_we_write_reads_back_through_libdb_unchanged() {
    // The other direction: bytes this backend writes must decode as
    // libpinyin's layout. Uses a fresh file rather than the system one —
    // the system data is read-only and must stay untouched.
    let path =
        std::env::temp_dir().join(format!("oxpinyin-bdb-writeback-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);

    let mut written = SingleGram::new();
    // Deliberately inserted out of order and across 256 in both the low
    // and a higher byte, so a writer that kept insertion order — or that
    // sorted by anything but the token — produces different bytes.
    for (token, freq) in [
        (0x0100_a271_u32, 52_u32),
        (0x0000_00ff, 7),
        (0x0100_05db, 8),
        (0x0000_0100, 9),
        (0x0700_0001, 1),
    ] {
        written.set_freq(token, freq);
    }
    written.set_total(77);

    {
        let db = BigramDb::open(&path, false).expect("create a bigram.db");
        db.put(1, &written).expect("write the gram");
        db.sync().expect("flush");
    }

    let db = BigramDb::open(&path, true).expect("reopen read-only");
    let read = db
        .get(1)
        .expect("point get")
        .expect("the record is present");
    assert_eq!(read, written, "what we wrote is what we read");
    assert_eq!(
        read.items().iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        [
            0x0000_00ff,
            0x0000_0100,
            0x0100_05db,
            0x0100_a271,
            0x0700_0001
        ],
        "items must be stored ascending by token, whatever order they arrived in"
    );
    let raw = db.raw(1).expect("raw read").expect("present");
    assert_eq!(
        raw.len(),
        4 + 5 * 8,
        "the chunk is a 4-byte total plus five 8-byte items and nothing else"
    );
    assert_eq!(
        &raw[0..4],
        &77_u32.to_ne_bytes(),
        "total_freq is the first four native-endian bytes"
    );
    drop(db);
    let _ = std::fs::remove_file(&path);
}
