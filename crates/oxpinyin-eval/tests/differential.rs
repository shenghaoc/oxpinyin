//! Differential parity: the native correction-rate evaluator vs pin-built
//! `eval_correction_rate` (`utils/training/eval_correction_rate.cpp`).
//!
//! This isolates exactly what `oxpinyin-eval` adds over the λ estimator
//! (`oxpinyin-lambda`, which already has its own bit-exact differential):
//! the **decode + correction-rate** path. So the native side is fed the
//! *same* λ the pin reads — `system_table_info.get_lambda()`, i.e. the value
//! `table.conf` carries (`eval_correction_rate.cpp:157`) — rather than
//! re-estimating it, and the *same* interpolation model the pin's runtime
//! `SYSTEM_BIGRAM` was compiled from. Under identical (model, λ, phrase
//! index, corpus), the native `correction_rate` must print the same
//! `correction rate:%f` the pin prints.
//!
//! The gate is env-driven and skips cleanly where the pin and its model data
//! are absent (as in CI here). The operator supplies a model the pin binary
//! has baked into its `SYSTEM_*` paths, plus the matching inputs for the
//! native side:
//!
//! - `PINYIN_EVAL_CORRECTION_RATE` — a built `eval_correction_rate`. It reads
//!   `evals2.text` from its working directory and its own `SYSTEM_PINYIN_INDEX`
//!   / `SYSTEM_BIGRAM` / `SYSTEM_TABLE_INFO`.
//! - `PINYIN_EVAL_DATA` — the working directory to run it in; must contain
//!   `evals2.text`.
//! - `PINYIN_EVAL_INTERPOLATION2` — the `interpolation2.text` the pin's
//!   runtime `SYSTEM_BIGRAM` was compiled from (the native model counts).
//! - `PINYIN_EVAL_TABLE_CONF` — the `table.conf` whose λ the pin reads.
//! - `PINYIN_EVAL_PINYIN_INDEX`, `PINYIN_EVAL_PHRASE_INDEX` — the oxpinyin
//!   export (`oxpinyin-datagen compile`) of the same model the pin loads,
//!   for the native `SystemDictionary` (which reads oxpinyin-format tables
//!   in the compiled-in backend, not the pin's `.bin` files).

use std::path::PathBuf;
use std::process::{Command, Stdio};

use oxpinyin_data::{SystemDictionary, parse_table_conf_lambda};
use oxpinyin_eval::{
    PhraseSource, SystemPhraseSource, build_model, correction_rate, parse_eval_corpus,
    parse_interpolation2,
};

fn env_path(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name)?);
    path.exists().then_some(path)
}

/// Runs the pin `eval_correction_rate` in `data_dir` and returns its
/// `correction rate:%f` line as a six-decimal string. The tool takes no
/// arguments; it reads `evals2.text` from the cwd and its own baked-in model.
fn pin_correction_rate(binary: &PathBuf, data_dir: &PathBuf) -> Result<String, String> {
    let output = Command::new(binary)
        .current_dir(data_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "eval_correction_rate exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    for line in stdout.lines() {
        if let Some(rate) = line.strip_prefix("correction rate:") {
            // Re-render at %f width so the compare is against the same six
            // decimals the pin's printf produced.
            let rate: f64 = rate.trim().parse().map_err(|_| "bad rate".to_string())?;
            return Ok(format!("{rate:.6}"));
        }
    }
    Err(format!(
        "no 'correction rate:' line in pin stdout:\n{stdout}"
    ))
}

#[test]
fn native_correction_rate_matches_pin_eval_correction_rate() {
    let (
        Some(binary),
        Some(data_dir),
        Some(interpolation2),
        Some(table_conf),
        Some(pinyin_index),
        Some(phrase_index),
    ) = (
        env_path("PINYIN_EVAL_CORRECTION_RATE"),
        env_path("PINYIN_EVAL_DATA"),
        env_path("PINYIN_EVAL_INTERPOLATION2"),
        env_path("PINYIN_EVAL_TABLE_CONF"),
        env_path("PINYIN_EVAL_PINYIN_INDEX"),
        env_path("PINYIN_EVAL_PHRASE_INDEX"),
    )
    else {
        eprintln!(
            "skipping live eval_correction_rate differential: set PINYIN_EVAL_CORRECTION_RATE, \
             PINYIN_EVAL_DATA (with evals2.text), PINYIN_EVAL_INTERPOLATION2, \
             PINYIN_EVAL_TABLE_CONF, PINYIN_EVAL_PINYIN_INDEX, PINYIN_EVAL_PHRASE_INDEX"
        );
        return;
    };

    // Pin side: run the oracle, capture its correction rate at %f width.
    let pin_rate = pin_correction_rate(&binary, &data_dir).expect("pin eval_correction_rate");

    // Native side: same model counts, same λ from table.conf, same phrase
    // index, same evals2.text — no re-estimation (that is the λ crate's job).
    let counts = parse_interpolation2(
        &std::fs::read_to_string(&interpolation2).expect("interpolation2.text"),
    );
    let lambda =
        parse_table_conf_lambda(&std::fs::read_to_string(&table_conf).expect("table.conf"))
            .expect("λ from table.conf");
    let library_dir = phrase_index
        .parent()
        .expect("phrase index has a parent directory");
    let dictionary = SystemDictionary::open_files(&pinyin_index, &phrase_index, library_dir)
        .expect("system index");
    let source = SystemPhraseSource::new(&dictionary);
    let model = build_model(&counts, lambda, source.lexicon_tokens());
    let evals = std::fs::read_to_string(data_dir.join("evals2.text")).expect("evals2.text");
    let sentences = parse_eval_corpus(&evals).expect("eval corpus");
    let report = correction_rate(&dictionary, &model, &source, &sentences).expect("evaluate");

    let native_rate = format!("{:.6}", report.rate);
    if native_rate != pin_rate {
        // Diagnostics before the assertion: which sentences the native decode
        // got wrong (the pin prints its own to stderr).
        for (expected, decoded) in &report.mismatches {
            eprintln!("native mismatch: expected {expected} decoded {decoded}");
        }
    }
    assert_eq!(
        native_rate, pin_rate,
        "native correction rate {native_rate} vs pin {pin_rate} \
         ({} tested, {} passed)",
        report.tested, report.passed
    );

    eprintln!(
        "live parity: native correction rate {native_rate} == pin {pin_rate} \
         ({} sentences, {} passed)",
        report.tested, report.passed
    );
}
