//! The pin's three DBM test programs (`tests/storage/test_ngram.cpp`,
//! `test_flexible_ngram.cpp`, `test_punct_table.cpp` at 074a2219) on the
//! Berkeley DB and Kyoto Cabinet backends, as native tests: the stdout the
//! pin's source prints, and the rows the programs leave in their fixed
//! `/tmp` files. The expected text is derived by hand from the C++ (the
//! single-gram arithmetic of `ngram.cpp:153-176`, the punctuation entry
//! logic of `punct_table.cpp:41-178`), not captured from a run.
//!
//! The programs open absolute `/tmp` names (the pin's test programs do, and the
//! ports keep them), so the tests serialize on one lock and delete those names
//! first. A Berkeley DB or Kyoto Cabinet hash
//! walk is in the library's own bucket order, which the pin's source does
//! not define, so the two-key walks are compared as sets.
#![cfg(any(feature = "bdb", feature = "kyotocabinet"))]

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use oxpinyin_store::{DefaultStore, RawReadStore};

// Covers `cargo test` (one process, the KC and BDB children); under nextest, where each test is its
// own process, the `store-backend` group of `.config/nextest.toml` serialises this binary instead.
static FIXED_TMP_NAMES: Mutex<()> = Mutex::new(());

const NGRAM_RULE: &str = "--------------------------------------------------------";

fn run(program: &str, files: &[&str]) -> String {
    for file in files {
        let _ = std::fs::remove_file(file);
    }
    let output = Command::new(program).output().expect("spawn the program");
    assert!(output.status.success(), "{program}: {output:?}");
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}

/// Splits the text at `marker` into the part before it, the marker line, and
/// the `count` lines after it; the rest follows.
fn lines_after<'a>(
    lines: &'a [&'a str],
    marker: &str,
    count: usize,
) -> (&'a [&'a str], &'a [&'a str]) {
    let at = lines
        .iter()
        .position(|line| *line == marker)
        .unwrap_or_else(|| panic!("missing {marker:?} in {lines:?}"));
    (&lines[..at], &lines[at + 1..at + 1 + count])
}

fn word(value: u32) -> [u8; 4] {
    value.to_ne_bytes()
}

/// `SingleGram`'s chunk: the total frequency, then `(token, freq)` pairs in
/// token order (`ngram.cpp:31-74`).
fn single_gram(total: u32, items: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = word(total).to_vec();
    for (token, freq) in items {
        bytes.extend_from_slice(&word(*token));
        bytes.extend_from_slice(&word(*freq));
    }
    bytes
}

fn raw(store: &DefaultStore, key: &[u8]) -> Option<Vec<u8>> {
    let mut value = Vec::new();
    store
        .get_raw(key, &mut value)
        .expect("raw read")
        .then_some(value)
}

#[test]
fn test_ngram_prints_the_single_grams_and_leaves_both_bigram_rows() {
    let _lock = FIXED_TMP_NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let stdout = run(
        env!("CARGO_BIN_EXE_test_ngram"),
        &["/tmp/test.db", "/tmp/snapshot.db"],
    );

    // test_ngram.cpp:8-24: total 16, then tokens 2,6,4,3,1 inserted with
    // freqs 1,2,4,8,16 and token 3 set to 32. `SingleGram::search`
    // (ngram.cpp:153-176) lists the tokens below 8 in ascending order with
    // freq / total. The second gram has token 1 removed, token 5 (freq 8)
    // added and total 32 (:42-46), so its probabilities halve.
    let first_gram = [
        "item:1:1.000000",
        "item:2:0.062500",
        "item:3:2.000000",
        "item:4:0.250000",
        "item:6:0.125000",
    ];
    let second_gram = [
        "item:2:0.031250",
        "item:3:1.000000",
        "item:4:0.125000",
        "item:5:0.250000",
        "item:6:0.062500",
    ];
    let mut expected: Vec<&str> = vec![NGRAM_RULE];
    expected.extend(first_gram); // :26-34, the in-memory gram
    expected.push(NGRAM_RULE);
    expected.extend(first_gram); // :50-61, `bigram.load(1)`
    expected.push(NGRAM_RULE);
    expected.extend(second_gram); // :50-61, `bigram.load(2)`
    expected.extend([
        NGRAM_RULE,
        "total_freq:32", // :63-65
        "----------------------system----------------------------",
    ]);
    let lines: Vec<&str> = stdout.lines().collect();
    let (head, walked) = lines_after(&lines, expected[expected.len() - 1], 2);
    assert_eq!(head, &expected[..expected.len() - 1]);
    let mut walked = walked.to_vec();
    walked.sort_unstable();
    assert_eq!(
        walked,
        ["item:1", "item:2"],
        "get_all_items (:69-76) names both tokens"
    );
    assert_eq!(lines.len(), expected.len() + 2, "{stdout}");

    // The attached file keeps both grams (:40-46); the saved snapshot
    // (`save_db`, :78) holds the same two rows.
    let gram1 = single_gram(16, &[(1, 16), (2, 1), (3, 32), (4, 4), (6, 2)]);
    let gram2 = single_gram(32, &[(2, 1), (3, 32), (4, 4), (5, 8), (6, 2)]);
    let attached = DefaultStore::open_hash_read_only(Path::new("/tmp/test.db")).expect("test.db");
    assert_eq!(raw(&attached, &word(1)), Some(gram1.clone()));
    assert_eq!(raw(&attached, &word(2)), Some(gram2.clone()));
    assert_eq!(attached.count_raw().expect("count"), 2);
    let saved = DefaultStore::open_user_bigram(Path::new("/tmp/snapshot.db")).expect("snapshot");
    assert_eq!(raw(&saved, &word(1)), Some(gram1));
    assert_eq!(raw(&saved, &word(2)), Some(gram2));
    assert_eq!(saved.count_raw().expect("count"), 2);
}

#[test]
fn test_flexible_ngram_prints_the_training_grams_headers_and_keys() {
    let _lock = FIXED_TMP_NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let stdout = run(
        env!("CARGO_BIN_EXE_test_flexible_ngram"),
        &["/tmp/training.db"],
    );

    // test_flexible_ngram.cpp:28-44: header 16, the same six inserts as
    // test_ngram, so the gram holds tokens 1,2,3,4,6 with items
    // 16,1,32,4,2. Storing it as token 1 (:61), then adding token 5 (item
    // 8), removing token 1 and setting header 32 (:62-66), gives token 2's
    // gram.
    let first_gram = ["item:1:16", "item:2:1", "item:3:32", "item:4:4", "item:6:2"];
    let second_gram = ["item:2:1", "item:3:32", "item:4:4", "item:5:8", "item:6:2"];
    let rule = NGRAM_RULE;
    let items_rule = "-----------------------items----------------------------";
    let magic_rule = "-----------------------magic header---------------------";
    let array_rule = "-----------------------array header---------------------";
    let mut expected: Vec<&str> = vec![rule];
    expected.extend(first_gram); // :46-54
    expected.push("new array header:32"); // :66
    for gram in [first_gram.as_slice(), second_gram.as_slice()] {
        // :69-81, `bigram.load(m)`
        expected.push(rule);
        expected.extend(gram);
    }
    expected.push(items_rule); // :85, then the two keys
    let head_len = expected.len();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(&lines[..head_len], expected.as_slice());
    let mut keys = lines[head_len..head_len + 2].to_vec();
    keys.sort_unstable();
    assert_eq!(keys, ["item:1", "item:2"], "the magic record is skipped");

    // :91-109: magic header 16; the array headers are the totals stored with
    // each gram (16 and 32), and `set_array_header(1, 1)` rewrites gram 1's
    // header in place without touching its items (:103), so the reloads
    // (:111-123) list the same items. `remove(1)` (:125) leaves one key.
    let mut tail: Vec<&str> = vec![
        magic_rule,
        "magic header:16",
        array_rule,
        "single gram: 1, freq:16",
        "single gram: 2, freq:32",
        array_rule,
        "single gram: 1, freq:1",
        "single gram: 2, freq:32",
    ];
    for gram in [first_gram.as_slice(), second_gram.as_slice()] {
        tail.push(rule);
        tail.extend(gram);
    }
    tail.extend([items_rule, "item:2"]);
    assert_eq!(&lines[head_len + 2..], tail.as_slice(), "{stdout}");
}

#[test]
fn test_punct_table_prints_the_punctuation_entries_and_leaves_the_saved_rows() {
    let _lock = FIXED_TMP_NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let stdout = run(
        env!("CARGO_BIN_EXE_test_punct_table"),
        &["/tmp/punct.bin", "/tmp/snapshot.db"],
    );

    // test_punct_table.cpp:47-74 with `PunctTableEntry` (punct_table.cpp:
    // 97-146): each punctuation is appended as its UCS-4 text and a NUL
    // (:108-112) and printed space-joined from the entry (:74-95). Removing
    // "…" drops only the second text (:117-146). Removing the last
    // punctuation stores an empty entry: the token stays (`store_entry` puts
    // a zero-length value, punct_table_bdb.cpp:162-180,
    // punct_table_kyotodb.cpp:144) but `get_all_punctuations` answers false
    // (punct_table.cpp:77-79), so no "Punctuations" line follows it.
    let expected = [
        "created table.",
        "insert some punctuations.",
        "token: 1",
        "Punctuations: …… … ？",
        "remove some punctuations.",
        "token: 1",
        "Punctuations: …… ？",
        "after save and load table.",
        "token: 1",
        "Punctuations: …… ？",
        "remove some punctuations.",
        "token: 1",
        "Punctuations: ？",
        "remove some punctuations.",
        "token: 1",
    ];
    assert_eq!(stdout.lines().collect::<Vec<_>>(), expected, "{stdout}");

    // The attached `punct.bin` holds token 1 with "……" and "？" as NUL-ended
    // UCS-4 (the later removals ran on the table `load_db` rebuilt, which is
    // memory only); `save_db` wrote the same row to the snapshot.
    let mut value = Vec::new();
    for unit in [0x2026_u32, 0x2026, 0, 0xFF1F, 0] {
        value.extend_from_slice(&word(unit));
    }
    let attached =
        <DefaultStore as oxpinyin_store::ReadStore>::open_read_only(Path::new("/tmp/punct.bin"))
            .expect("punct.bin");
    assert_eq!(raw(&attached, &word(1)), Some(value.clone()));
    assert_eq!(attached.count_raw().expect("count"), 1);
    let saved = DefaultStore::open_user_index(Path::new("/tmp/snapshot.db")).expect("snapshot");
    assert_eq!(raw(&saved, &word(1)), Some(value));
    assert_eq!(saved.count_raw().expect("count"), 1);
}
