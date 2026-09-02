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
use std::fmt::Write as _;
use std::path::PathBuf;

use oxpinyin_counter::{Counts, count_ngseg, parse_interpolation_dump};
use oxpinyin_lambda::{DeletedCounts, Lambda, count_deleted, estimate_lambda};
use oxpinyin_segment::{PhraseLexicon, locate_export_dir};
use oxpinyin_testsupport::{PinDir, fnv1a64, locate_bin, locate_data, parse_estimate_stdout};

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
    let lexicon = PhraseLexicon::from_system_dir(&export).ok()?;
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
        let _ = writeln!(dump, "token:{prev} lambda:{value:.6}");
    }
    let _ = writeln!(dump, "average lambda:{:.6}", lambda.average);
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
    let Some(data) = locate_data("PINYIN_GEN_NGRAM_DATA") else {
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
