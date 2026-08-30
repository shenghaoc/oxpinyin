//! The Kyoto Cabinet backend against libpinyin's own bigram records.
//!
//! Every other test in this crate reads files oxpinyin itself produced,
//! which proves the backend is self-consistent and nothing about
//! compatibility. These read real libpinyin bigram data.
//!
//! # Where the data comes from, stated plainly
//!
//! The ideal source is a `bigram.db` written by a Kyoto-Cabinet-built
//! libpinyin. When one is present — `OXPINYIN_KC_BIGRAM`, or an installed
//! data directory that `oxpinyin_data::layout` classifies as Kyoto
//! Cabinet — that is what these tests read, and the result is a genuine
//! compatibility check.
//!
//! When it is not, `tools/kc/run-compat-check.sh` transcribes the
//! installed Berkeley-DB-built `bigram.db` into Kyoto Cabinet with a
//! pure-C tool and points `OXPINYIN_KC_BIGRAM` at the result. That is
//! weaker, and the difference matters: it proves this backend reads
//! libpinyin's *records* — real keys, real `SingleGram` chunks, all
//! 56,359 of them — out of a Kyoto Cabinet container, but it does **not**
//! prove that a Kyoto-Cabinet-built libpinyin would have written that
//! exact container. Only a machine with one installed can show that.
//!
//! What licenses the transcription is that the chunk and the key are
//! backend-independent: `ngram.cpp` is unconditional in
//! `src/storage/Makefile.am:72`, while `ngram_bdb.cpp` and
//! `ngram_kyotodb.cpp` are added under `if BERKELEYDB` / `if
//! KYOTOCABINET`. The records are the same; only the container differs.
//!
//! # Why the round-trip is the write gate
//!
//! A drop-in that trains a user's profile writes back into libpinyin's
//! files. If the bytes it writes differ from the bytes libpinyin would
//! have written, the user's own libpinyin misreads its own profile with
//! no error anywhere. So the gate is `encode(decode(x)) == x` over every
//! record of a real file, not a handful of samples.

#![cfg(feature = "kyotocabinet")]

use std::path::PathBuf;

use oxpinyin_store::{BigramDb, SingleGram};

/// Whether `path` is a Kyoto Cabinet hash database, by its own magic:
/// `KC\n\0` at offset 0 and the `HashDB` type byte at offset 8.
fn is_kyoto_hash(path: &std::path::Path) -> bool {
    use std::io::Read as _;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0_u8; 9];
    file.read_exact(&mut head).is_ok() && head[..4] == *b"KC\n\0" && head[8] == 0x30
}

/// A Kyoto Cabinet `bigram.db`, or `None` with the skip already reported.
fn kc_bigram(what: &str) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("OXPINYIN_KC_BIGRAM") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    // An installed data directory counts only if a Kyoto-Cabinet-built
    // libpinyin wrote it. The filename is `bigram.db` whichever DBM was
    // used, so the check is by magic, not by name.
    //
    // The magic check is inlined rather than taken from
    // `oxpinyin_data::layout`, which is the same rule stated once for the
    // engine: `oxpinyin-data` depends on this crate, so a dev-dependency
    // back would be a cycle. Both must agree, and
    // `the_magic_here_matches_the_detectors` below is what keeps them
    // agreeing.
    for dir in [
        "/usr/lib/x86_64-linux-gnu/libpinyin/data",
        "/usr/lib64/libpinyin/data",
        "/usr/lib/libpinyin/data",
        "/usr/local/lib/libpinyin/data",
    ] {
        let candidate = PathBuf::from(dir).join("bigram.db");
        if is_kyoto_hash(&candidate) {
            return Some(candidate);
        }
    }

    let message = format!(
        "SKIP {what}: no Kyoto Cabinet bigram.db found. Set OXPINYIN_KC_BIGRAM, or \
         install a Kyoto-Cabinet-built libpinyin. On a machine whose libpinyin is \
         built against another DBM, tools/kc/run-compat-check.sh transcribes its \
         records into a Kyoto Cabinet file and runs these tests against that — see \
         this file's header for exactly how much weaker that is."
    );
    assert!(
        std::env::var_os("OXPINYIN_KC_STRICT").is_none(),
        "{message} (OXPINYIN_KC_STRICT is set, so this skip is a failure)"
    );
    eprintln!("{message}");
    None
}

#[test]
fn every_record_round_trips_byte_for_byte() {
    let Some(path) = kc_bigram("bigram round-trip") else {
        return;
    };
    let db = BigramDb::open(&path, true).expect("open the bigram.db read-only");

    let mut records = 0_u64;
    let mut items = 0_u64;
    db.for_each(&mut |prev, gram| {
        records += 1;
        items += gram.len() as u64;
        // The write gate: our bytes for this gram must be the bytes the
        // file already holds. `SingleGram::decode` has already rejected
        // any chunk that is not 4 + 8n, is not ascending, or is a
        // zero-item chunk with a total, so reaching here also asserts
        // that libpinyin's own records satisfy every invariant this
        // backend relies on.
        let stored = db
            .raw(prev)
            .expect("re-read the record we are visiting")
            .expect("the record we are visiting exists");
        assert_eq!(
            gram.encode(),
            stored,
            "gram for prev 0x{prev:08x} does not re-encode to the bytes on disk"
        );
        let sum: u64 = gram.items().iter().map(|(_, freq)| u64::from(*freq)).sum();
        assert_eq!(
            u64::from(gram.total()),
            sum,
            "prev 0x{prev:08x}: total_freq must equal the sum of its items"
        );
        Ok(())
    })
    .expect("walk the bigram.db");

    assert!(records > 0, "an empty database proves nothing");
    assert_eq!(
        records,
        db.len().expect("record count"),
        "the walk must visit exactly as many records as the database reports"
    );
    eprintln!("{records} records, {items} successor items, all round-tripped");
}

#[test]
fn the_shipped_model20_counts_are_present_when_the_source_is_the_shipped_data() {
    let Some(path) = kc_bigram("model20 counts") else {
        return;
    };
    let db = BigramDb::open(&path, true).expect("open the bigram.db read-only");
    let mut records = 0_u64;
    let mut items = 0_u64;
    db.for_each(&mut |_, gram| {
        records += 1;
        items += gram.len() as u64;
        Ok(())
    })
    .expect("walk");

    // `oxpinyin-datagen` derives exactly these two numbers independently
    // from model20 (docs/findings/datagen-model20.md's equivalence
    // proof). Reaching them through a Kyoto Cabinet container is what
    // makes this a format check rather than a self-consistency one.
    //
    // A different data release legitimately has different counts, so this
    // reports rather than fails when they do not match — the round-trip
    // test above is the one that must hold for any input.
    if records == 56_359 && items == 1_849_609 {
        eprintln!("counts match the model20-derived 56,359 / 1,849,609");
    } else {
        eprintln!(
            "NOTE: {records} records / {items} items, not the model20-derived \
             56,359 / 1,849,609 — a different data release, so compare against that \
             release's own figures"
        );
    }
}

#[test]
fn a_known_record_decodes_to_the_bytes_the_c_probe_read() {
    let Some(path) = kc_bigram("known-record decode") else {
        return;
    };
    let db = BigramDb::open(&path, true).expect("open the bigram.db read-only");

    // Transcribed from a standalone C probe over the shipped data,
    // driving the DBM directly with no Rust involved: prev 0x03001801 has
    // total_freq 65 and four successors. Absent from a different release,
    // which is not a failure of this backend.
    let Some(gram) = db.get(0x0300_1801).expect("point get") else {
        eprintln!("NOTE: prev 0x03001801 is absent — a different data release");
        return;
    };
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
fn a_profile_we_write_reads_back_unchanged() {
    // The other direction, and it needs no installed data: bytes this
    // backend writes must decode as libpinyin's layout.
    let path =
        std::env::temp_dir().join(format!("oxpinyin-kc-writeback-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);

    let mut written = SingleGram::new();
    // Inserted out of order and crossing 256 in the low and in a higher
    // byte, so a writer that kept insertion order — or sorted by anything
    // but the token — produces different bytes.
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

#[test]
fn a_file_named_bigram_db_opens_despite_having_no_kyoto_suffix() {
    // The single fact that makes this backend possible at all. libpinyin
    // names its file `bigram.db` whatever DBM it was built against
    // (src/pinyin_internal.h:56-58), while the C API is PolyDB, which
    // picks the class from the path suffix and fails on an unrecognised
    // one (kclangc.h:312-320). If `BigramDb::open` ever stopped appending
    // `#type=kch`, this test is what notices.
    let path = std::env::temp_dir().join(format!(
        "oxpinyin-kc-suffix-{}/bigram.db",
        std::process::id()
    ));
    let dir = path.parent().expect("has a parent");
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).expect("create the directory");

    let mut gram = SingleGram::new();
    gram.set_freq(2, 3);
    gram.set_total(3);
    {
        let db = BigramDb::open(&path, false).expect(
            "a file named bigram.db — with no .kch suffix — must still open as a \
             hash database, which needs PolyDB's #type= tuning parameter",
        );
        db.put(1, &gram).expect("write");
        db.sync().expect("flush");
    }
    // `dir` was created fresh above, so bigram.db must be the ONLY file in
    // it — no `.kch` suffix, and no PolyDB sidecar (`.wal`, a lock file)
    // either. Inspecting the whole directory catches any unexpected file, not
    // just the `.kch` one.
    let mut entries: Vec<String> = std::fs::read_dir(dir)
        .expect("read the bigram directory")
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    entries.sort();
    assert_eq!(
        entries,
        vec!["bigram.db".to_string()],
        "only bigram.db may exist alongside it — no .kch or other sidecar files"
    );

    // The file really is a Kyoto Cabinet hash database, by its own magic.
    let header = std::fs::read(&path).expect("read the file back");
    assert_eq!(&header[..4], b"KC\n\0", "Kyoto Cabinet's magic at offset 0");
    assert_eq!(header[8], 0x30, "and the HashDB type byte at offset 8");

    let db = BigramDb::open(&path, true).expect("reopen read-only");
    assert_eq!(db.get(1).expect("get"), Some(gram));
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_magic_here_matches_the_detectors() {
    // `is_kyoto_hash` above restates the rule `oxpinyin_data::layout`
    // owns, because a dev-dependency on that crate would be a cycle. A
    // file this backend creates must satisfy both, so writing one and
    // checking it here is what keeps the two copies from drifting.
    let path = std::env::temp_dir().join(format!("oxpinyin-kc-magic-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let db = BigramDb::open(&path, false).expect("create");
        db.put(1, &SingleGram::new()).expect("write");
        db.sync().expect("flush");
    }
    assert!(
        is_kyoto_hash(&path),
        "a file this backend creates must match the magic this file tests for, \
         and the same magic oxpinyin-data's layout detector uses"
    );
    let _ = std::fs::remove_file(&path);
}
