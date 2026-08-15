//! Tier 2 — end-to-end differential (the real oracle).
//!
//! Feeds the committed T4b sample (cleaned raw Han text) through the full
//! Rust trainer chain and the pin-built libpinyin trainer chain on the
//! same bytes, and asserts the two `interpolation2.text` outputs are
//! value-identical and the λ estimates agree at the pin's printed
//! precision.
//!
//! Rust chain:   T4b sample → pinyin-segment (T1) → count_ngseg (T2) →
//!               count_deleted + estimate_lambda (T3) → emit_interpolation2 (T4a)
//! Pin chain:    T4b sample → ngseg → gen_binary_files → gen_unigram →
//!               gen_ngram → gen_deleted_ngram → estimate_interpolation →
//!               export_interpolation
//!
//! Env-gated on the same variables the T1–T4a tasks use; skipped (not
//! failed) when the pin tools or the rust-side tables are absent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use pinyin_counter::{count_ngseg, parse_interpolation_dump};
use pinyin_emitter::emit_interpolation2;
use pinyin_lambda::{count_deleted, estimate_lambda};
use pinyin_segment::{
    PINNED_LAMBDA, Segmenter, SegmenterPaths, locate_export_dir, locate_model_dir,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn sample_path() -> PathBuf {
    repo_root().join("fixtures/w9/corpus-sample.txt")
}

fn locate_bin(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name)?);
    path.is_file().then_some(path)
}

fn locate_data(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name)?);
    (path.join("table.conf").is_file()
        && path
            .read_dir()
            .map(|mut dir| dir.next().is_some())
            .unwrap_or(false))
    .then_some(path)
}

/// The Rust trainer chain over the sample. `None` when the migrate
/// export or the model20 cache is absent.
struct RustChain {
    ngseg_text: String,
    interpolation: String,
    per_context: BTreeMap<u32, String>,
    average: f64,
}

fn run_rust_chain(sample: &[u8]) -> Option<RustChain> {
    let export = locate_export_dir()?;
    let model = locate_model_dir()?;
    let paths = SegmenterPaths::from_dirs(&export, &model);
    let segmenter = Segmenter::open(&paths, PINNED_LAMBDA).ok()?;
    let lexicon = segmenter.lexicon();

    // T1: the T4b sample is consumed as-is — zero glue.
    let ngseg_text = segmenter.segment_bytes(sample, false).ok()?;

    // T2 + T3 + T4a.
    let counts = count_ngseg(lexicon, &ngseg_text, true).ok()?;
    let deleted = count_deleted(&ngseg_text, true).ok()?;
    let lambda = estimate_lambda(&counts, &deleted).ok()?;
    let interpolation = emit_interpolation2(&counts, lexicon);
    let per_context: BTreeMap<u32, String> = lambda
        .per_context
        .iter()
        .map(|(token, value)| (*token, format!("{value:.6}")))
        .collect();
    Some(RustChain {
        ngseg_text,
        interpolation,
        per_context,
        average: lambda.average,
    })
}

/// Copies the raw `.table` + `table.conf` into a fresh dir (never the
/// stale `.bin`/`.db` outputs) and runs one pin command there.
struct PinDir {
    dir: PathBuf,
}

impl PinDir {
    fn fresh(data: &Path, tag: &str) -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("pinyin-corpus-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        for entry in data
            .read_dir()
            .map_err(|error| error.to_string())?
            .flatten()
        {
            if entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
            {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name == "table.conf" || name.ends_with(".table") {
                    std::fs::copy(entry.path(), dir.join(entry.file_name()))
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Ok(Self { dir })
    }

    fn run(&self, bin: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let mut command = Command::new(bin);
        command.current_dir(&self.dir).args(args);
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
    }
}

impl Drop for PinDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Parses `estimate_interpolation` stdout: `token:%d lambda:%f` per
/// context and `average lambda:%f`.
fn parse_estimate_stdout(text: &str) -> (BTreeMap<u32, String>, Option<String>) {
    let mut per_context = BTreeMap::new();
    let mut average = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("token:")
            && let Some((token, lambda)) = rest.split_once(" lambda:")
            && let Ok(token) = token.parse::<u32>()
        {
            per_context.insert(token, lambda.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("average lambda:") {
            average = Some(rest.trim().to_string());
        }
    }
    (per_context, average)
}

/// The Rust chain alone: T4b's output feeds T1 with zero glue. Skips
/// without the migrate export / model20 cache; CI-unconditional
/// otherwise.
#[test]
fn rust_chain_consumes_t4b_sample_with_zero_glue() {
    let sample = std::fs::read(sample_path()).expect("committed sample");
    let Some(chain) = run_rust_chain(&sample) else {
        eprintln!(
            "skipping: migrate export or model20 cache not found \
             (PINYIN_EXPORT_DIR / PINYIN_MODEL_DIR)"
        );
        return;
    };
    let parsed = parse_interpolation_dump(&chain.interpolation);
    assert!(
        !parsed.unigrams.is_empty(),
        "T4b sample must produce unigram counts through T1→T2→T4a"
    );
    assert!(
        !chain.per_context.is_empty(),
        "T3 must estimate at least one per-context λ"
    );
    assert!(
        !chain.interpolation.to_ascii_lowercase().contains("lambda"),
        "interpolation2.text must not contain λ"
    );
    eprintln!(
        "rust chain over T4b sample: {} unigrams, {} bigrams, {} λ contexts, average λ {:.6}",
        parsed.unigrams.len(),
        parsed.bigrams.len(),
        chain.per_context.len(),
        chain.average
    );
}

/// Full end-to-end differential: T4b sample → rust chain vs pin chain.
/// Env-gated on the pin tools; reports env-blocked rather than
/// fabricating a result.
#[test]
fn end_to_end_matches_live_libpinyin_pipeline() {
    let (
        Some(ngseg),
        Some(gen_binary_files),
        Some(gen_unigram),
        Some(gen_ngram),
        Some(gen_deleted_ngram),
        Some(estimate_interpolation),
        Some(export_interpolation),
    ) = (
        locate_bin("PINYIN_NGSEG"),
        locate_bin("PINYIN_GEN_BINARY_FILES"),
        locate_bin("PINYIN_GEN_UNIGRAM"),
        locate_bin("PINYIN_GEN_NGRAM"),
        locate_bin("PINYIN_GEN_DELETED_NGRAM"),
        locate_bin("PINYIN_ESTIMATE_INTERPOLATION"),
        locate_bin("PINYIN_EXPORT_INTERPOLATION"),
    )
    else {
        eprintln!(
            "skipping live end-to-end: set PINYIN_NGSEG, PINYIN_GEN_BINARY_FILES, \
             PINYIN_GEN_UNIGRAM, PINYIN_GEN_NGRAM, PINYIN_GEN_DELETED_NGRAM, \
             PINYIN_ESTIMATE_INTERPOLATION, and PINYIN_EXPORT_INTERPOLATION"
        );
        return;
    };
    let Some(ngseg_data) = locate_data("PINYIN_NGSEG_DATA") else {
        eprintln!(
            "skipping live end-to-end: PINYIN_NGSEG_DATA not set (needs table.conf + bigram.db)"
        );
        return;
    };
    let Some(training_data) = locate_data("PINYIN_GEN_NGRAM_DATA") else {
        eprintln!("skipping live end-to-end: PINYIN_GEN_NGRAM_DATA not set or empty");
        return;
    };
    let sample = std::fs::read(sample_path()).expect("committed sample");
    let Some(chain) = run_rust_chain(&sample) else {
        eprintln!("skipping live end-to-end: migrate export / model20 cache not found");
        return;
    };

    // Pin T1: ngseg over the same sample bytes, in the pin data dir.
    let temp = std::env::temp_dir().join(format!("pinyin-corpus-input-{}", std::process::id()));
    std::fs::write(&temp, &sample).expect("write sample for ngseg");
    let pin_ngseg = {
        let output = Command::new(&ngseg)
            .current_dir(&ngseg_data)
            .arg(&temp)
            .output()
            .expect("ngseg runs");
        assert!(
            output.status.success(),
            "ngseg exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    let _ = std::fs::remove_file(&temp);

    // The join: T1 over T4b's output is byte-identical to ngseg over it.
    assert_eq!(
        chain.ngseg_text.as_bytes(),
        pin_ngseg,
        "T1(segment_bytes) diverges from pin ngseg on the T4b sample"
    );

    // Pin T2–T4a in a fresh data dir; the pin's gen_ngram consumes the
    // same ngseg stream the Rust side counted.
    let pin = PinDir::fresh(&training_data, "e2e").expect("temp data dir");
    pin.run(&gen_binary_files, &["--gen-punct-table"], None)
        .expect("gen_binary_files");
    pin.run(&gen_unigram, &[], None).expect("gen_unigram");
    pin.run(&gen_ngram, &[], Some(&pin_ngseg))
        .expect("gen_ngram");
    pin.run(&gen_deleted_ngram, &[], Some(&pin_ngseg))
        .expect("gen_deleted_ngram");
    let estimate = pin
        .run(&estimate_interpolation, &[], None)
        .expect("estimate_interpolation");
    let (pin_per_context, pin_average) =
        parse_estimate_stdout(&String::from_utf8(estimate).expect("utf8"));
    let pin_interp = pin
        .run(&export_interpolation, &[], None)
        .expect("export_interpolation");

    // interpolation2.text: value-identical.
    let rust_values = parse_interpolation_dump(&chain.interpolation);
    let pin_values = parse_interpolation_dump(&String::from_utf8(pin_interp).expect("utf8"));
    assert_eq!(
        rust_values.unigrams.len(),
        pin_values.unigrams.len(),
        "unigram count: rust {} vs pin {}",
        rust_values.unigrams.len(),
        pin_values.unigrams.len()
    );
    for (token, rust_count) in &rust_values.unigrams {
        let pin_count = pin_values.unigrams.get(token).copied();
        assert_eq!(
            pin_count,
            Some(*rust_count),
            "unigram diverges: token {token} = rust {rust_count}, pin {pin_count:?}"
        );
    }
    assert_eq!(
        rust_values.bigrams.len(),
        pin_values.bigrams.len(),
        "bigram count: rust {} vs pin {}",
        rust_values.bigrams.len(),
        pin_values.bigrams.len()
    );
    for (pair, rust_count) in &rust_values.bigrams {
        let pin_count = pin_values.bigrams.get(pair).copied();
        assert_eq!(
            pin_count,
            Some(*rust_count),
            "bigram diverges: pair {pair:?} = rust {rust_count}, pin {pin_count:?}"
        );
    }

    // λ: per-context byte-identical at six decimals; the average within
    // the pin's printed precision (lambda-port.md).
    assert_eq!(
        chain.per_context.len(),
        pin_per_context.len(),
        "λ context count: rust {} vs pin {}",
        chain.per_context.len(),
        pin_per_context.len()
    );
    for (token, rust) in &chain.per_context {
        let pin = pin_per_context.get(token).cloned();
        assert_eq!(
            pin.as_deref(),
            Some(rust.as_str()),
            "per-context λ diverges at prev {token}: rust {rust} vs pin {pin:?}"
        );
    }
    let pin_average: f64 = pin_average.expect("average line").parse().expect("f64");
    let delta = (chain.average - pin_average).abs();
    assert!(
        delta < 1e-6,
        "average λ: rust {:.17} vs pin {pin_average:.6}, |Δ| = {delta:.3e} ≥ 1e-6",
        chain.average
    );

    eprintln!(
        "live end-to-end: ngseg bit-identical; {} unigrams + {} bigrams value-identical; \
         {} λ contexts byte-identical at 6dp, average λ {:.6} (|Δ| = {delta:.2e})",
        rust_values.unigrams.len(),
        rust_values.bigrams.len(),
        chain.per_context.len(),
        chain.average
    );
}
