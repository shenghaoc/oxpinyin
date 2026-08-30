//! Differential parity: Rust counter vs pin-built `gen_ngram`.
//!
//! Skips when the system-table export is absent. The committed manifest pins the
//! value-level golden (unigram/bigram counts + an FNV-1a 64-bit checksum of
//! the full `Counts::dump()`). When the pin training binaries are present,
//! a live `gen_binary_files → gen_unigram → gen_ngram → export_interpolation`
//! run in a fresh temp dir is compared value-for-value against the Rust
//! counter, so the manifest cannot go stale silently.

use std::path::{Path, PathBuf};

use oxpinyin_counter::{Counts, count_ngseg, parse_interpolation_dump};
use oxpinyin_segment::{PhraseLexicon, locate_export_dir};
use oxpinyin_testsupport::{PinDir, fnv1a64, locate_bin, locate_data, parse_manifest};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn fixture_ngseg() -> PathBuf {
    repo_root().join("fixtures/w9/segmenter-ngseg.txt")
}

fn manifest_path() -> PathBuf {
    repo_root().join("fixtures/w9/counter-ngram.manifest")
}

/// Runs the Rust counter over the ngseg fixture with the system-table export
/// phrase index as the freq-1 floor seed.
fn rust_counts() -> Option<Counts> {
    let export = locate_export_dir()?;
    let lexicon = PhraseLexicon::from_phrase_index(
        &export.join(oxpinyin_segment::default_store_file("phrase_index")),
    )
    .ok()?;
    let text = std::fs::read_to_string(fixture_ngseg()).ok()?;
    count_ngseg(&lexicon, &text, true).ok()
}

/// Reports the first value divergence (token or pair) instead of dumping both
/// full maps, so a mismatch names exactly which entry differs and by how much.
fn assert_counts_equal(rust: &Counts, live: &Counts) {
    assert_eq!(
        rust.unigrams.len(),
        live.unigrams.len(),
        "unigram count: rust {} vs gen_ngram {}",
        rust.unigrams.len(),
        live.unigrams.len()
    );
    assert_eq!(
        rust.bigrams.len(),
        live.bigrams.len(),
        "bigram count: rust {} vs gen_ngram {}",
        rust.bigrams.len(),
        live.bigrams.len()
    );

    for (token, rust_count) in &rust.unigrams {
        let live_count = live.unigrams.get(token).copied();
        assert_eq!(
            live_count,
            Some(*rust_count),
            "unigram diverges: token {token} = rust {rust_count}, gen_ngram {live_count:?}"
        );
    }
    for (pair, rust_count) in &rust.bigrams {
        let live_count = live.bigrams.get(pair).copied();
        assert_eq!(
            live_count,
            Some(*rust_count),
            "bigram diverges: pair {pair:?} = rust {rust_count}, gen_ngram {live_count:?}"
        );
    }
}

#[test]
fn rust_matches_committed_manifest() {
    let Some(counts) = rust_counts() else {
        eprintln!(
            "skipping: system tables not found (PINYIN_EXPORT_DIR | /tmp/oxpinyin-export; produce with oxpinyin-datagen compile)"
        );
        return;
    };
    let manifest_path = manifest_path();
    if !manifest_path.is_file() {
        eprintln!(
            "skipping golden compare: {} is not committed yet",
            manifest_path.display()
        );
        eprintln!("--- rust counts ---");
        eprintln!(
            "unigrams {} bigrams {}",
            counts.unigrams.len(),
            counts.bigrams.len()
        );
        return;
    }
    let manifest = parse_manifest(&std::fs::read_to_string(&manifest_path).expect("manifest"));

    assert_eq!(counts.unigrams.len(), manifest.unigrams, "unigram count");
    assert_eq!(counts.bigrams.len(), manifest.bigrams, "bigram count");

    let dump = counts.dump();
    let checksum = fnv1a64(dump.as_bytes());
    assert_eq!(checksum, manifest.checksum, "full dump checksum diverged");

    // Spot-check corpus-derived counts (not the freq-1 floor).
    assert_eq!(counts.unigrams.get(&16_817_937), Some(&5), "中国 count"); // 中国
    assert_eq!(counts.unigrams.get(&16_782_711), Some(&3), "人 count"); // 人

    eprintln!(
        "parity: {} unigrams, {} bigrams, dump checksum {checksum:016x} matches manifest",
        counts.unigrams.len(),
        counts.bigrams.len()
    );
}

/// Copies the flat data dir into a fresh temp dir and runs the full pin
/// pipeline there, returning the `export_interpolation` dump.
fn run_live_pipeline(
    gen_binary_files: &Path,
    gen_unigram: &Path,
    gen_ngram: &Path,
    export_interpolation: &Path,
    data: &Path,
    fixture: &[u8],
) -> Result<Counts, String> {
    let pin = PinDir::fresh(data, "counter-live")?;

    pin.run(gen_binary_files, &["--gen-punct-table"], None)?;
    pin.run(gen_unigram, &[], None)?;
    pin.run(gen_ngram, &[], Some(fixture))?;
    let dump = pin.run(export_interpolation, &[], None)?;

    let text = std::str::from_utf8(&dump).map_err(|error| error.to_string())?;
    Ok(parse_interpolation_dump(text))
}

#[test]
fn rust_matches_live_gen_ngram() {
    let (Some(gen_binary_files), Some(gen_unigram), Some(gen_ngram), Some(export_interpolation)) = (
        locate_bin("PINYIN_GEN_BINARY_FILES"),
        locate_bin("PINYIN_GEN_UNIGRAM"),
        locate_bin("PINYIN_GEN_NGRAM"),
        locate_bin("PINYIN_EXPORT_INTERPOLATION"),
    ) else {
        eprintln!(
            "skipping live gen_ngram: set PINYIN_GEN_BINARY_FILES, PINYIN_GEN_UNIGRAM, \
             PINYIN_GEN_NGRAM, and PINYIN_EXPORT_INTERPOLATION"
        );
        return;
    };
    let Some(data) = locate_data("PINYIN_GEN_NGRAM_DATA") else {
        eprintln!("skipping live gen_ngram: PINYIN_GEN_NGRAM_DATA not set or empty");
        return;
    };
    let Some(rust) = rust_counts() else {
        eprintln!("skipping live gen_ngram: system tables not found (oxpinyin-datagen compile)");
        return;
    };
    let fixture = std::fs::read(fixture_ngseg()).expect("fixture");

    let live = run_live_pipeline(
        &gen_binary_files,
        &gen_unigram,
        &gen_ngram,
        &export_interpolation,
        &data,
        &fixture,
    )
    .expect("live gen_ngram pipeline");

    assert_counts_equal(&rust, &live);
    eprintln!(
        "live parity: {} unigrams, {} bigrams, value-identical to gen_ngram",
        live.unigrams.len(),
        live.bigrams.len()
    );
}
