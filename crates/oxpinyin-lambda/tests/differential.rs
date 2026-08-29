//! Differential parity: Rust λ estimator vs pin-built `gen_deleted_ngram`
//! + `estimate_interpolation`.
//!
//! Configuration (documented in `docs/findings/lambda-port.md`): the system
//! bigram + floored unigram are trained on **fold A** — the first ⌊N/2⌋
//! lines of `segmenter-ngseg.txt` — and the held-out (`DELETED_BIGRAM`)
//! counts cover the **full** corpus. Held-out pairs seen in A exercise the
//! bigram-hit path (per-context λ → 1); pairs only in the tail exercise the
//! unigram-only path (λ → 0); contexts spanning both yield intermediate λ.
//! That mix makes the per-context λ vary across (0, 1), so the parity check
//! bites on the actual interpolation arithmetic, not a saturated constant.
//!
//! Two gates, mirroring T1/T2:
//!
//! - `rust_lambda_matches_committed_manifest` — recomputes the estimate from
//!   the system-table export + fixture and checks it against the committed
//!   golden (`fixtures/w9/lambda-estimate.manifest`). Skips without the
//!   export. This is a Rust-internal regression pin: λ is a deterministic
//!   function of integer counts, so equality is bit-exact.
//! - `rust_lambda_matches_live_estimate_interpolation` — env-gated; runs the
//!   pin pipeline and asserts (a) the `DELETED_BIGRAM` counts are integer
//!   bit-exact and (b) every per-context λ and the average match the pin's
//!   `%f` output (six decimals — the tool's entire observable precision).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use oxpinyin_counter::{Counts, count_ngseg, parse_interpolation_dump};
use oxpinyin_lambda::{DeletedCounts, Lambda, count_deleted, estimate_lambda};
use oxpinyin_segment::{PhraseLexicon, locate_export_dir};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn fixture_ngseg() -> PathBuf {
    repo_root().join("fixtures/w9/segmenter-ngseg.txt")
}

fn manifest_path() -> PathBuf {
    repo_root().join("fixtures/w9/lambda-estimate.manifest")
}

/// Fold A: the first ⌊N/2⌋ lines of the ngseg stream. `split_inclusive`
/// keeps each line's `\n`, so the concatenation is byte-identical to
/// `head -n ⌊N/2⌋` — the exact slice fed to the pin's `gen_ngram`.
fn system_fold(full: &str) -> String {
    let lines: Vec<&str> = full.split_inclusive('\n').collect();
    let half = lines.len() / 2;
    lines[..half].concat()
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

/// The committed golden's shape and value. `average_bits` pins the full-f64
/// average by its bit pattern (λ is a pure function of integer counts, so
/// this is exact); the 6dp dump checksum pins the per-context values.
struct Manifest {
    contexts: usize,
    system_bigrams: usize,
    deleted_bigrams: usize,
    uni_total: u64,
    average_bits: u64,
    dump_fnv1a64: u64,
}

/// Rust-side Config X: system counts on fold A, held-out on the full corpus.
struct Estimate {
    system: Counts,
    deleted: DeletedCounts,
    lambda: Lambda,
}

fn rust_estimate() -> Option<Estimate> {
    let export = locate_export_dir()?;
    let lexicon = PhraseLexicon::from_phrase_index(
        &export.join(oxpinyin_segment::default_store_file("phrase_index")),
    )
    .ok()?;
    let full = std::fs::read_to_string(fixture_ngseg()).ok()?;
    let system = count_ngseg(&lexicon, &system_fold(&full), true).ok()?;
    let deleted = count_deleted(&full, true).ok()?;
    let lambda = estimate_lambda(&system, &deleted).ok()?;
    Some(Estimate {
        system,
        deleted,
        lambda,
    })
}

/// The canonical per-context λ dump the checksum covers: `token:%d
/// lambda:%f` lines (ascending `prev`) then `average lambda:%f`, exactly
/// `estimate_interpolation`'s stdout shape at six decimals.
fn lambda_dump(lambda: &Lambda) -> String {
    let mut dump = String::new();
    for (prev, value) in &lambda.per_context {
        dump.push_str(&format!("token:{prev} lambda:{value:.6}\n"));
    }
    dump.push_str(&format!("average lambda:{:.6}\n", lambda.average));
    dump
}

fn parse_manifest(text: &str) -> Manifest {
    let mut fields: BTreeMap<&str, &str> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
            fields.insert(key, value);
        }
    }
    let get = |k: &str| {
        *fields
            .get(k)
            .unwrap_or_else(|| panic!("manifest missing {k}"))
    };
    Manifest {
        contexts: get("contexts").parse().expect("contexts"),
        system_bigrams: get("system_bigrams").parse().expect("system_bigrams"),
        deleted_bigrams: get("deleted_bigrams").parse().expect("deleted_bigrams"),
        uni_total: get("uni_total").parse().expect("uni_total"),
        average_bits: u64::from_str_radix(get("average_bits"), 16).expect("average_bits"),
        dump_fnv1a64: u64::from_str_radix(get("dump_fnv1a64"), 16).expect("dump_fnv1a64"),
    }
}

#[test]
fn rust_lambda_matches_committed_manifest() {
    let Some(estimate) = rust_estimate() else {
        eprintln!(
            "skipping: system tables not found (PINYIN_EXPORT_DIR | /tmp/oxpinyin-export; produce with oxpinyin-datagen compile)"
        );
        return;
    };
    let path = manifest_path();
    if !path.is_file() {
        eprintln!("skipping golden compare: {} not committed", path.display());
        return;
    }
    let manifest = parse_manifest(&std::fs::read_to_string(&path).expect("manifest"));

    assert_eq!(
        estimate.lambda.per_context.len(),
        manifest.contexts,
        "per-context λ count"
    );
    assert_eq!(
        estimate.system.bigrams.len(),
        manifest.system_bigrams,
        "system (fold A) bigram count"
    );
    assert_eq!(
        estimate.deleted.bigrams.len(),
        manifest.deleted_bigrams,
        "held-out (DELETED) bigram count"
    );
    assert_eq!(
        estimate.system.unigrams.values().sum::<u64>(),
        manifest.uni_total,
        "phrase-index unigram total (floor + fold-A increments)"
    );

    // λ is a deterministic function of integer counts — assert the exact
    // bit pattern of the average, not a tolerance.
    assert_eq!(
        estimate.lambda.average.to_bits(),
        manifest.average_bits,
        "average λ bits: got {:.17} ({:#018x})",
        estimate.lambda.average,
        estimate.lambda.average.to_bits()
    );

    let checksum = fnv1a64(lambda_dump(&estimate.lambda).as_bytes());
    assert_eq!(
        checksum, manifest.dump_fnv1a64,
        "per-context λ dump checksum"
    );

    eprintln!(
        "parity: {} contexts, average λ = {:.6} ({:.17}), dump checksum {checksum:016x} matches manifest",
        estimate.lambda.per_context.len(),
        estimate.lambda.average,
        estimate.lambda.average,
    );
}

fn locate_bin(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name)?);
    path.is_file().then_some(path)
}

fn locate_data() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("PINYIN_GEN_NGRAM_DATA")?);
    (path.join("table.conf").is_file()
        && path
            .read_dir()
            .map(|mut d| d.next().is_some())
            .unwrap_or(false))
    .then_some(path)
}

/// Copies the raw `.table` + `table.conf` into a fresh dir (never the stale
/// `.bin`/`.db` outputs) and runs one pin command there.
struct PinDir {
    dir: PathBuf,
}

impl PinDir {
    fn fresh(data: &Path, tag: &str) -> Result<Self, String> {
        let dir =
            std::env::temp_dir().join(format!("oxpinyin-lambda-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for entry in data.read_dir().map_err(|e| e.to_string())?.flatten() {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name == "table.conf" || name.ends_with(".table") {
                    std::fs::copy(entry.path(), dir.join(entry.file_name()))
                        .map_err(|e| e.to_string())?;
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
            .map_err(|e| e.to_string())?;
        if let Some(input) = stdin {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .ok_or("no stdin")?
                .write_all(input)
                .map_err(|e| e.to_string())?;
        }
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
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

/// Parses `estimate_interpolation` stdout: `token:%d lambda:%f` per context
/// and `average lambda:%f`.
fn parse_estimate_stdout(text: &str) -> (BTreeMap<u32, String>, Option<String>) {
    let mut per_context = BTreeMap::new();
    let mut average = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("token:") {
            if let Some((token, lambda)) = rest.split_once(" lambda:")
                && let Ok(token) = token.parse::<u32>()
            {
                per_context.insert(token, lambda.trim().to_string());
            }
        } else if let Some(rest) = line.strip_prefix("average lambda:") {
            average = Some(rest.trim().to_string());
        }
    }
    (per_context, average)
}

#[test]
fn rust_lambda_matches_live_estimate_interpolation() {
    let (
        Some(gen_binary_files),
        Some(gen_unigram),
        Some(gen_ngram),
        Some(gen_deleted_ngram),
        Some(estimate_interpolation),
        Some(export_interpolation),
    ) = (
        locate_bin("PINYIN_GEN_BINARY_FILES"),
        locate_bin("PINYIN_GEN_UNIGRAM"),
        locate_bin("PINYIN_GEN_NGRAM"),
        locate_bin("PINYIN_GEN_DELETED_NGRAM"),
        locate_bin("PINYIN_ESTIMATE_INTERPOLATION"),
        locate_bin("PINYIN_EXPORT_INTERPOLATION"),
    )
    else {
        eprintln!(
            "skipping live estimate_interpolation: set PINYIN_GEN_BINARY_FILES, \
             PINYIN_GEN_UNIGRAM, PINYIN_GEN_NGRAM, PINYIN_GEN_DELETED_NGRAM, \
             PINYIN_ESTIMATE_INTERPOLATION, PINYIN_EXPORT_INTERPOLATION"
        );
        return;
    };
    let Some(data) = locate_data() else {
        eprintln!("skipping live estimate_interpolation: PINYIN_GEN_NGRAM_DATA not set or empty");
        return;
    };
    let Some(estimate) = rust_estimate() else {
        eprintln!(
            "skipping live estimate_interpolation: system tables not found (oxpinyin-datagen compile)"
        );
        return;
    };

    let full = std::fs::read(fixture_ngseg()).expect("fixture");
    let fold_a = system_fold(&String::from_utf8(full.clone()).expect("utf8")).into_bytes();

    // Config X pipeline: SYSTEM_BIGRAM ← gen_ngram over fold A, DELETED_BIGRAM
    // ← gen_deleted_ngram over the full corpus, then estimate_interpolation.
    let pin = PinDir::fresh(&data, "estimate").expect("temp data dir");
    pin.run(&gen_binary_files, &["--gen-punct-table"], None)
        .expect("gen_binary_files");
    pin.run(&gen_unigram, &[], None).expect("gen_unigram");
    pin.run(&gen_ngram, &[], Some(&fold_a)).expect("gen_ngram");
    pin.run(&gen_deleted_ngram, &[], Some(&full))
        .expect("gen_deleted_ngram");
    let est_out = pin
        .run(&estimate_interpolation, &[], None)
        .expect("estimate_interpolation");
    let (pin_per_context, pin_average) =
        parse_estimate_stdout(&String::from_utf8(est_out).expect("utf8"));

    // (a) DELETED_BIGRAM integer bit-exactness. `export_interpolation` can
    // only dump SYSTEM_BIGRAM, but `gen_deleted_ngram`'s bigram loop is
    // byte-identical to `gen_ngram`'s (see lambda-port.md), so a fresh
    // gen_ngram over the *same* full corpus yields the DELETED oracle.
    let oracle = PinDir::fresh(&data, "deloracle").expect("temp data dir");
    oracle
        .run(&gen_binary_files, &["--gen-punct-table"], None)
        .expect("gen_binary_files");
    oracle.run(&gen_unigram, &[], None).expect("gen_unigram");
    oracle.run(&gen_ngram, &[], Some(&full)).expect("gen_ngram");
    let dump = oracle
        .run(&export_interpolation, &[], None)
        .expect("export_interpolation");
    let oracle_counts = parse_interpolation_dump(&String::from_utf8(dump).expect("utf8"));
    assert_eq!(
        estimate.deleted.bigrams, oracle_counts.bigrams,
        "DELETED_BIGRAM counts diverge from gen_deleted_ngram (== gen_ngram loop)"
    );

    // (b) per-context λ: byte-identical at the pin's `%f` precision.
    assert_eq!(
        estimate.lambda.per_context.len(),
        pin_per_context.len(),
        "context count: rust {} vs pin {}",
        estimate.lambda.per_context.len(),
        pin_per_context.len()
    );
    for (prev, value) in &estimate.lambda.per_context {
        let rust = format!("{value:.6}");
        let pin = pin_per_context.get(prev).cloned();
        assert_eq!(
            pin.as_deref(),
            Some(rust.as_str()),
            "per-context λ diverges at prev {prev}: rust {rust} vs pin {pin:?}"
        );
    }

    // (c) average λ within the pin's observable floor. `%f` rounds to six
    // decimals (5e-7 floor); 1e-6 sits just above it. The underlying
    // agreement is at the IEEE-754 float floor (deterministic EM over
    // bit-identical integer inputs), documented in lambda-port.md.
    let pin_average: f64 = pin_average.expect("average line").parse().expect("f64");
    let delta = (estimate.lambda.average - pin_average).abs();
    assert!(
        delta < 1e-6,
        "average λ: rust {:.17} vs pin {pin_average:.6}, |Δ| = {delta:.3e} ≥ 1e-6",
        estimate.lambda.average
    );
    assert_eq!(
        format!("{:.6}", estimate.lambda.average),
        format!("{pin_average:.6}"),
        "average λ six-decimal string"
    );

    eprintln!(
        "live parity: {} DELETED bigrams bit-exact; {} per-context λ byte-identical at 6dp; \
         average λ rust {:.17} vs pin {pin_average:.6} (|Δ| = {delta:.3e})",
        estimate.deleted.bigrams.len(),
        estimate.lambda.per_context.len(),
        estimate.lambda.average,
    );
}
