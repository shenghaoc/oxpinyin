//! CI-unconditional format contract: emit → `parse_interpolation2` →
//! the same integer counts.
//!
//! `oxpinyin_data::parse_interpolation2` is the decoder's consumer of
//! `interpolation2.text`. This gate needs no libpinyin build and no
//! migrate export: a synthetic [`Counts`] + [`PhraseLexicon`] is enough
//! to prove the emitter writes exactly the grammar the parser reads.

use std::path::PathBuf;

use oxpinyin_counter::Counts;
use oxpinyin_data::parse_interpolation2;
use oxpinyin_emitter::{emit_interpolation2, write_interpolation2};
use oxpinyin_segment::PhraseLexicon;

/// One distinct name per test: tests run in parallel.
fn temp_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "oxpinyin-emitter-roundtrip-{name}-{}.text",
        std::process::id()
    ));
    path
}

fn sample() -> (Counts, PhraseLexicon) {
    let mut counts = Counts::default();
    counts.unigrams.insert(10, 5);
    counts.unigrams.insert(20, 7);
    counts.bigrams.insert((1, 10), 2);
    counts.bigrams.insert((10, 20), 3);
    let lexicon = PhraseLexicon::from_pairs(vec![(10, "甲".to_owned()), (20, "乙".to_owned())]);
    (counts, lexicon)
}

#[test]
fn emit_roundtrips_through_parse_interpolation2() {
    let (counts, lexicon) = sample();
    let path = temp_path("synthetic");
    write_interpolation2(&path, &counts, &lexicon).expect("write");

    // `parse_interpolation2` reads only the `\1-gram` `token u32` +
    // `count u64` records; the `\2-gram` section is ignored.
    let table = parse_interpolation2(&path).expect("parse_interpolation2");
    let _ = std::fs::remove_file(&path);

    assert_eq!(table.len(), counts.unigrams.len(), "unigram record count");
    let expected_total: u64 = counts.unigrams.values().copied().sum();
    assert_eq!(table.total(), expected_total);
    for (&token, &count) in &counts.unigrams {
        assert_eq!(
            table.count(token),
            Some(count),
            "unigram token {token} must survive emit→parse bit-exact"
        );
    }

    // The 2-gram half is outside parse_interpolation2's contract; pin it
    // through the value-level dump reader so those records are not silent.
    let parsed =
        oxpinyin_counter::parse_interpolation_dump(&emit_interpolation2(&counts, &lexicon));
    assert_eq!(parsed.bigrams, counts.bigrams);
}

#[test]
fn emit_writes_the_two_unigram_records_the_parser_keeps() {
    // The committed pin for the synthetic gate: 2 unigram records
    // survive emit → parse_interpolation2 bit-exact.
    let (counts, lexicon) = sample();
    let text = emit_interpolation2(&counts, &lexicon);
    let unigram_items = text
        .lines()
        .skip_while(|line| *line != "\\1-gram")
        .skip(1)
        .take_while(|line| !line.starts_with('\\') || line.starts_with("\\item"))
        .filter(|line| line.starts_with("\\item "))
        .count();
    assert_eq!(unigram_items, 2);
    assert_eq!(counts.unigrams.len(), 2);
}
