//! Differential parity: Rust counter vs pin-built `gen_ngram`.
//!
//! Skips when the migrate export is absent. The committed manifest pins the
//! value-level golden (unigram/bigram counts + an FNV-1a 64-bit checksum of
//! the full `Counts::dump()`). When the pin training binaries are present,
//! a live `gen_binary_files → gen_unigram → gen_ngram → export_interpolation`
//! run in a fresh temp dir is compared value-for-value against the Rust
//! counter, so the manifest cannot go stale silently.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use pinyin_counter::{Counts, count_ngseg, parse_interpolation_dump};
use pinyin_segment::{PhraseLexicon, locate_export_dir};

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

/// Runs the Rust counter over the ngseg fixture with the migrate-export
/// phrase index as the freq-1 floor seed.
fn rust_counts() -> Option<Counts> {
    let export = locate_export_dir()?;
    let lexicon = PhraseLexicon::from_phrase_index(&export.join("phrase_index.redb")).ok()?;
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

/// FNV-1a 64-bit, dependency-free and deterministic across platforms.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

struct Manifest {
    unigrams: usize,
    bigrams: usize,
    checksum: u64,
}

fn parse_manifest(text: &str) -> Manifest {
    let mut manifest = Manifest {
        unigrams: 0,
        bigrams: 0,
        checksum: 0,
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(key), Some(value)) = (fields.next(), fields.next()) else {
            continue;
        };
        match key {
            "unigrams" => manifest.unigrams = value.parse().expect("manifest unigrams"),
            "bigrams" => manifest.bigrams = value.parse().expect("manifest bigrams"),
            "fnv1a64" => {
                manifest.checksum = u64::from_str_radix(value, 16).expect("manifest checksum");
            }
            _ => {}
        }
    }
    manifest
}

#[test]
fn rust_matches_committed_manifest() {
    let Some(counts) = rust_counts() else {
        eprintln!("skipping: migrate export not found (PINYIN_EXPORT_DIR / /tmp/oxpinyin-export)");
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
    assert_eq!(counts.unigrams.get(&16817937), Some(&5), "中国 count"); // 中国
    assert_eq!(counts.unigrams.get(&16782711), Some(&3), "人 count"); // 人

    eprintln!(
        "parity: {} unigrams, {} bigrams, dump checksum {checksum:016x} matches manifest",
        counts.unigrams.len(),
        counts.bigrams.len()
    );
}

fn locate_bin(name: &str) -> Option<PathBuf> {
    let raw = std::env::var_os(name)?;
    let path = PathBuf::from(raw);
    path.is_file().then_some(path)
}

fn locate_data() -> Option<PathBuf> {
    let raw = std::env::var_os("PINYIN_GEN_NGRAM_DATA")?;
    let path = PathBuf::from(raw);
    (path.join("table.conf").is_file()
        && path
            .read_dir()
            .map(|mut d| d.next().is_some())
            .unwrap_or(false))
    .then_some(path)
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
    let temp = std::env::temp_dir().join(format!("pinyin-counter-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).map_err(|error| error.to_string())?;

    // Copy only the raw `.table` sources and `table.conf`. The `.bin`/`.db`
    // files in the data dir are *outputs* of earlier runs; copying them would
    // make `gen_ngram` append onto a stale `bigram.db` (and `gen_unigram`
    // onto a stale `phrase_index.bin`). `gen_binary_files` rebuilds every
    // binary index from the tables, so the pipeline must start clean.
    for entry in data
        .read_dir()
        .map_err(|error| error.to_string())?
        .flatten()
    {
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "table.conf" || name.ends_with(".table") {
                let target = temp.join(entry.file_name());
                std::fs::copy(entry.path(), target).map_err(|error| error.to_string())?;
            }
        }
    }

    let run = |bin: &Path, args: &[&str], stdin: Option<&[u8]>| -> Result<Vec<u8>, String> {
        let mut command = Command::new(bin);
        command.current_dir(&temp).args(args);
        if stdin.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        if let Some(input) = stdin {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .ok_or("no stdin")?
                .write_all(input)
                .map_err(|error| error.to_string())?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "{} exited {}: {}",
                bin.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(output.stdout)
    };

    run(gen_binary_files, &["--gen-punct-table"], None)?;
    run(gen_unigram, &[], None)?;
    run(gen_ngram, &[], Some(fixture))?;
    let dump = run(export_interpolation, &[], None)?;

    let _ = std::fs::remove_dir_all(&temp);

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
    let Some(data) = locate_data() else {
        eprintln!("skipping live gen_ngram: PINYIN_GEN_NGRAM_DATA not set or empty");
        return;
    };
    let Some(rust) = rust_counts() else {
        eprintln!("skipping live gen_ngram: migrate export not found");
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
