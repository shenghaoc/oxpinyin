//! Differential parity: Rust counter vs pin-built `gen_ngram`.
//!
//! Requires the system-table export (`PINYIN_EXPORT_DIR`); the live gate
//! also requires the pin-built counter tools and `PINYIN_GEN_NGRAM_DATA`.
//! Ordinary `cargo test` leaves these tests ignored; explicitly selecting
//! them fails on a missing input (`docs/testing/README.md`). After loading
//! those inputs, run from the repository root:
//!
//! ```text
//! just heavy -- cargo test --locked -p oxpinyin-training --no-default-features --features tkrzw --test counter-differential -- --include-ignored
//! ```
//!
//! The committed manifest pins the value-level golden (unigram/bigram
//! counts + an FNV-1a 64-bit checksum of the full `Counts::dump()`). The live
//! gate compares a `gen_binary_files → gen_unigram → gen_ngram → export_interpolation`
//! run in a fresh temp dir value-for-value against the Rust counter, so
//! the manifest cannot go stale silently.
//!
//! See the [oracle runbook](../../../docs/runbooks/oracle.md#pre-release-real-installation-checks)
//! for execution owners and pre-release input identities.

use std::path::{Path, PathBuf};

use oxpinyin_testsupport::{PinDir, fnv1a64, locate_bin, locate_data, parse_manifest};
use oxpinyin_training::counter::{Counts, count_ngseg, parse_interpolation_dump};
use oxpinyin_training::segment::{
    DEFAULT_EXPORT_DIR, EXPORT_DIR_ENV, PhraseLexicon, locate_export_dir,
};

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
    let lexicon = PhraseLexicon::from_system_dir(&export).ok()?;
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
#[ignore = "needs the system-table export (PINYIN_EXPORT_DIR); run with --include-ignored"]
fn rust_matches_committed_manifest() {
    let Some(counts) = rust_counts() else {
        panic!(
            "missing input: system tables not found (${EXPORT_DIR_ENV} | {DEFAULT_EXPORT_DIR}; produce with the oxpinyin-utils data-dir sequence)"
        )
    };
    let manifest_path = manifest_path();
    assert!(
        manifest_path.is_file(),
        "committed golden missing: {} is not committed yet",
        manifest_path.display()
    );
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

/// The fixture with each token field respelled so that `atoi` still reads the
/// same token: a sign, C white space before the digits, leading zeros,
/// trailing bytes, a value above 2^32 and a negative whose low 32 bits are
/// the token. Zero is respelled as a signed or wrapped zero or a field with no
/// digits, which bypass phrase validation as `0` does (`utils_helper.h:63`).
fn spell_token_fields(fixture: &[u8]) -> Vec<u8> {
    const WRAP: u64 = 1 << 32;
    let mut out = Vec::new();
    let body = fixture.strip_suffix(b"\n").unwrap_or(fixture);
    for (index, line) in body.split(|byte| *byte == b'\n').enumerate() {
        if line.is_empty() {
            out.push(b'\n');
            continue;
        }
        let Some(split) = line.iter().position(|byte| matches!(byte, b' ' | b'\t')) else {
            panic!("fixture has no separator: {line:?}");
        };
        let token: u64 = std::str::from_utf8(&line[..split])
            .expect("ASCII token")
            .parse()
            .expect("fixture token");
        let field = if token == 0 {
            [
                "-4294967296",
                "4294967296",
                "-0",
                "abc",
                "+",
                "-9223372036854775808",
                "\r0",
            ][index % 7]
                .to_owned()
        } else {
            match index % 6 {
                0 => format!("+{token}"),
                1 => format!("\r{token}"),
                2 => format!("{}", token + WRAP),
                3 => format!("-{}", WRAP - token),
                4 => format!("\x0b00{token}"),
                _ => format!("\x0c{token}x"),
            }
        };
        out.extend_from_slice(field.as_bytes());
        out.extend_from_slice(&line[split..]);
        out.push(b'\n');
    }
    out
}

#[test]
#[ignore = "needs the pin-built gen_ngram tool chain and PINYIN_GEN_NGRAM_DATA; run with --include-ignored"]
fn rust_matches_live_gen_ngram() {
    let (Some(gen_binary_files), Some(gen_unigram), Some(gen_ngram), Some(export_interpolation)) = (
        locate_bin("PINYIN_GEN_BINARY_FILES"),
        locate_bin("PINYIN_GEN_UNIGRAM"),
        locate_bin("PINYIN_GEN_NGRAM"),
        locate_bin("PINYIN_EXPORT_INTERPOLATION"),
    ) else {
        panic!(
            "missing input for the live gen_ngram: set PINYIN_GEN_BINARY_FILES, PINYIN_GEN_UNIGRAM, \
             PINYIN_GEN_NGRAM, and PINYIN_EXPORT_INTERPOLATION"
        )
    };
    let Some(data) = locate_data("PINYIN_GEN_NGRAM_DATA") else {
        panic!("missing input for the live gen_ngram: PINYIN_GEN_NGRAM_DATA not set or empty")
    };
    let Some(rust) = rust_counts() else {
        panic!(
            "missing input for the live gen_ngram: system tables not found (the oxpinyin-utils data-dir sequence)"
        )
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

    // Source-derived accepted partitions, utils/utils_helper.h:57-65:
    // atoi accepts a decimal prefix, TAB splits just like SP, and zero
    // bypasses phrase validation even with repeated separators, Unicode
    // format/unassigned scalars and a literal dangling backslash.
    let mut classified: String = std::str::from_utf8(&fixture)
        .expect("UTF-8 fixture")
        .lines()
        .map(|line| match line.split_once(' ') {
            Some(("0", _)) => "-0 \t\u{200d}\u{378}\\\0hidden\n".to_owned(),
            Some((token, phrase)) => format!("{token}suffix\t{phrase}\0hidden\n"),
            None if line.is_empty() => "\n".to_owned(),
            None => panic!("fixture has no separator: {line:?}"),
        })
        .collect();
    // Signed zero and a NUL after the delimiter are source-derived controls,
    // not evidence that positive signs or NUL before the delimiter agree.
    classified.push_str("+0 ignored\n0 \0hidden\n");
    // GLib 2.89.4 glib/gstrfuncs.c:2507-2541 and the pin macro
    // utils/utils_helper.h:51-69: NUL hides later bytes; zero ignores its tail.
    let mut classified = classified.into_bytes();
    classified.extend_from_slice(b"0 \xff\n\0\xff\n");
    for byte in &mut classified {
        if *byte == b'h' {
            *byte = 0xff;
        }
    }
    let export = locate_export_dir().expect("system tables");
    let lexicon = PhraseLexicon::from_system_dir(&export).expect("phrase lexicon");
    assert!(count_ngseg(&lexicon, &classified, true).is_err());
    let classified_live = run_live_pipeline(
        &gen_binary_files,
        &gen_unigram,
        &gen_ngram,
        &export_interpolation,
        &data,
        &classified,
    )
    .expect("classified live gen_ngram pipeline");
    assert_counts_equal(&rust, &classified_live);
    eprintln!(
        "POLICY-ACCEPTED #1297: malformed prefixes are refused; the pin retains the canonical counts"
    );
    // `atoi` is `(int) strtol(field, NULL, 10)` (`utils_helper.h:61`; glibc
    // 2.41 `stdlib/strtol_l.c:295-524`): C white space other than the field
    // separators, one sign and trailing bytes are accepted, a `long` overflow
    // saturates and only the low 32 bits become the token. Every spelling
    // below names the token its line already carries, or zero.
    let spelled = spell_token_fields(&fixture);
    assert!(count_ngseg(&lexicon, &spelled, true).is_err());
    let spelled_live = run_live_pipeline(
        &gen_binary_files,
        &gen_unigram,
        &gen_ngram,
        &export_interpolation,
        &data,
        &spelled,
    )
    .expect("spelled live gen_ngram pipeline");
    assert_counts_equal(&rust, &spelled_live);
    eprintln!(
        "POLICY-ACCEPTED #1297: malformed/overflow token fields are refused; canonical counts MATCH"
    );
    // Pin utils/utils_helper.h:58-65 aborts on shape/phrase mismatch;
    // tag_utility.cpp:413 compares exact bytes for this known resident token.
    for input in [
        "16817937 wrong\n",
        "16817937  中国\n",
        "16817937 中国\r\n",
        "16817937 中\0国\n",
        "16817937\0 中国\n",
    ] {
        assert!(count_ngseg(&lexicon, input, true).is_err());
        assert!(
            run_live_pipeline(
                &gen_binary_files,
                &gen_unigram,
                &gen_ngram,
                &export_interpolation,
                &data,
                input.as_bytes()
            )
            .is_err(),
            "pin must refuse {input:?}"
        );
    }
    eprintln!("known-token phrase mismatches and NUL before delimiter: both refuse");
}
