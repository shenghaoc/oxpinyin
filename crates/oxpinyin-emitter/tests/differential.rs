//! Fixture-chain round-trip and env-gated differential vs pin-built
//! `export_interpolation`.
//!
//! Two gates, mirroring T1 (`PINYIN_NGSEG`) / T2 (`PINYIN_GEN_NGRAM`):
//!
//! - `fixture_emit_roundtrips_through_parse_interpolation2` — T1's
//!   `segmenter-ngseg.txt` → T2's counter → this crate's emitter →
//!   `parse_interpolation2`. Skips without the system-table export. Pins N
//!   unigram records + an FNV-1a checksum of the emitted text against
//!   `fixtures/w9/interpolation2.manifest`.
//! - `rust_matches_live_export_interpolation` — skips unless
//!   `PINYIN_GEN_BINARY_FILES`, `PINYIN_GEN_UNIGRAM`, `PINYIN_GEN_NGRAM`,
//!   `PINYIN_EXPORT_INTERPOLATION`, and `PINYIN_GEN_NGRAM_DATA` are set;
//!   then compares `(token, count)` records value-for-value. There are
//!   no probability fields in the format.

use std::path::{Path, PathBuf};

use oxpinyin_counter::{Counts, count_ngseg, parse_interpolation_dump};
use oxpinyin_data::parse_interpolation2;
use oxpinyin_emitter::emit_interpolation2;
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
    repo_root().join("fixtures/w9/interpolation2.manifest")
}

fn rust_counts_and_text() -> Option<(Counts, PhraseLexicon, String)> {
    let export = locate_export_dir()?;
    let lexicon = PhraseLexicon::from_phrase_index(
        &export.join(oxpinyin_segment::default_store_file("phrase_index")),
    )
    .ok()?;
    let text = std::fs::read_to_string(fixture_ngseg()).ok()?;
    let counts = count_ngseg(&lexicon, &text, true).ok()?;
    let emitted = emit_interpolation2(&counts, &lexicon);
    Some((counts, lexicon, emitted))
}

/// Reports the first value divergence instead of dumping both maps.
fn assert_counts_equal(rust: &Counts, live: &Counts) {
    assert_eq!(
        rust.unigrams.len(),
        live.unigrams.len(),
        "unigram count: rust {} vs export_interpolation {}",
        rust.unigrams.len(),
        live.unigrams.len()
    );
    assert_eq!(
        rust.bigrams.len(),
        live.bigrams.len(),
        "bigram count: rust {} vs export_interpolation {}",
        rust.bigrams.len(),
        live.bigrams.len()
    );

    for (token, rust_count) in &rust.unigrams {
        let live_count = live.unigrams.get(token).copied();
        assert_eq!(
            live_count,
            Some(*rust_count),
            "unigram diverges: token {token} = rust {rust_count}, export {live_count:?}"
        );
    }
    for (pair, rust_count) in &rust.bigrams {
        let live_count = live.bigrams.get(pair).copied();
        assert_eq!(
            live_count,
            Some(*rust_count),
            "bigram diverges: pair {pair:?} = rust {rust_count}, export {live_count:?}"
        );
    }
}

#[test]
fn fixture_emit_roundtrips_through_parse_interpolation2() {
    let Some((counts, _lexicon, emitted)) = rust_counts_and_text() else {
        eprintln!(
            "skipping: system tables not found (PINYIN_EXPORT_DIR | /tmp/oxpinyin-export; produce with oxpinyin-datagen compile)"
        );
        return;
    };

    // Nothing dropped: every T2 count has resolvable phrase text, so the
    // emit filters do not shrink the maps.
    let dumped = parse_interpolation_dump(&emitted);
    assert_eq!(dumped.unigrams.len(), counts.unigrams.len());
    assert_eq!(dumped.bigrams.len(), counts.bigrams.len());
    assert_eq!(dumped, counts);

    let path = std::env::temp_dir().join(format!(
        "oxpinyin-emitter-fixture-{}.text",
        std::process::id()
    ));
    std::fs::write(&path, emitted.as_bytes()).expect("write fixture emit");
    let table = parse_interpolation2(&path).expect("parse_interpolation2");
    let _ = std::fs::remove_file(&path);

    assert_eq!(table.len(), counts.unigrams.len());
    for (&token, &count) in &counts.unigrams {
        assert_eq!(table.count(token), Some(count), "token {token}");
    }

    let manifest_path = manifest_path();
    if !manifest_path.is_file() {
        eprintln!(
            "skipping golden compare: {} is not committed yet",
            manifest_path.display()
        );
        eprintln!(
            "--- rust emit --- unigrams {} bigrams {} fnv1a64 {:016x}",
            counts.unigrams.len(),
            counts.bigrams.len(),
            fnv1a64(emitted.as_bytes())
        );
        return;
    }
    let manifest = parse_manifest(&std::fs::read_to_string(&manifest_path).expect("manifest"));
    assert_eq!(counts.unigrams.len(), manifest.unigrams, "unigram count");
    assert_eq!(counts.bigrams.len(), manifest.bigrams, "bigram count");
    let checksum = fnv1a64(emitted.as_bytes());
    assert_eq!(
        checksum, manifest.checksum,
        "emitted-text checksum diverged"
    );

    assert!(
        !emitted.to_ascii_lowercase().contains("lambda"),
        "emitted interpolation2.text must not contain λ"
    );

    eprintln!(
        "round-trip: {} unigram records survive emit→parse_interpolation2 bit-exact; \
         {} bigrams value-identical via parse_interpolation_dump",
        table.len(),
        dumped.bigrams.len()
    );
}

/// Copies the flat data dir into a fresh temp dir and runs the full pin
/// pipeline there, returning the raw `export_interpolation` stdout.
fn run_live_export(
    gen_binary_files: &Path,
    gen_unigram: &Path,
    gen_ngram: &Path,
    export_interpolation: &Path,
    data: &Path,
    fixture: &[u8],
) -> Result<String, String> {
    let pin = PinDir::fresh(data, "emitter-live")?;

    pin.run(gen_binary_files, &["--gen-punct-table"], None)?;
    pin.run(gen_unigram, &[], None)?;
    pin.run(gen_ngram, &[], Some(fixture))?;
    let dump = pin.run(export_interpolation, &[], None)?;

    let text = String::from_utf8(dump).map_err(|error| error.to_string())?;
    if text.to_ascii_lowercase().contains("lambda") {
        return Err(
            "export_interpolation unexpectedly embedded λ; PR #55 says λ ∈ table.conf".into(),
        );
    }
    Ok(text)
}

#[test]
fn rust_matches_live_export_interpolation() {
    let (Some(gen_binary_files), Some(gen_unigram), Some(gen_ngram), Some(export_interpolation)) = (
        locate_bin("PINYIN_GEN_BINARY_FILES"),
        locate_bin("PINYIN_GEN_UNIGRAM"),
        locate_bin("PINYIN_GEN_NGRAM"),
        locate_bin("PINYIN_EXPORT_INTERPOLATION"),
    ) else {
        eprintln!(
            "skipping live export_interpolation: set PINYIN_GEN_BINARY_FILES, \
             PINYIN_GEN_UNIGRAM, PINYIN_GEN_NGRAM, and PINYIN_EXPORT_INTERPOLATION"
        );
        return;
    };
    let Some(data) = locate_data("PINYIN_GEN_NGRAM_DATA") else {
        eprintln!("skipping live export_interpolation: PINYIN_GEN_NGRAM_DATA not set or empty");
        return;
    };
    let Some((rust, _, emitted)) = rust_counts_and_text() else {
        eprintln!(
            "skipping live export_interpolation: system tables not found (oxpinyin-datagen compile)"
        );
        return;
    };
    let fixture = std::fs::read(fixture_ngseg()).expect("fixture");

    let live_text = run_live_export(
        &gen_binary_files,
        &gen_unigram,
        &gen_ngram,
        &export_interpolation,
        &data,
        &fixture,
    )
    .expect("live export_interpolation pipeline");

    let live = parse_interpolation_dump(&live_text);
    let rust_values = parse_interpolation_dump(&emitted);
    assert_counts_equal(&rust_values, &live);
    assert_eq!(rust_values, rust, "emitter must not drop T2 counts");

    eprintln!(
        "live parity: {} unigrams, {} bigrams, value-identical to export_interpolation",
        live.unigrams.len(),
        live.bigrams.len()
    );
}
