//! `oxpinyin-eval` — the native correction-rate evaluator.
//!
//! Usage:
//!   oxpinyin-eval --interpolation2 interpolation2.text \
//!                 --held-out held.seg --evals evals2.text \
//!                 --pinyin-index pinyin_index.<ext> \
//!                 --phrase-index phrase_index.<ext> [--skip-pi-gram]
//!
//! Reproduces evaluate.py with no Python, libpinyin, `make`, or external
//! evaluator: parse interpolation2.text → estimate λ over the held-out
//! slice → apply λ → decode evals2.text against the system phrase index →
//! print the average λ and the correction rate.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oxpinyin_data::SystemDictionary;
use oxpinyin_eval::{
    PhraseSource, SystemPhraseSource, build_model, correction_rate, estimate_lambda,
    parse_eval_corpus, parse_interpolation2,
};
use oxpinyin_lambda::count_deleted;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oxpinyin-eval: {error}");
            ExitCode::from(1)
        }
    }
}

type Cli = Result<(), Box<dyn std::error::Error>>;

fn run() -> Cli {
    let mut interpolation2: Option<PathBuf> = None;
    let mut held_out: Option<PathBuf> = None;
    let mut evals: Option<PathBuf> = None;
    let mut pinyin_index: Option<PathBuf> = None;
    let mut phrase_index: Option<PathBuf> = None;
    let mut train_pi_gram = true;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "--interpolation2" => interpolation2 = Some(PathBuf::from(next(&mut args, &arg)?)),
            "--held-out" => held_out = Some(PathBuf::from(next(&mut args, &arg)?)),
            "--evals" => evals = Some(PathBuf::from(next(&mut args, &arg)?)),
            "--pinyin-index" => pinyin_index = Some(PathBuf::from(next(&mut args, &arg)?)),
            "--phrase-index" => phrase_index = Some(PathBuf::from(next(&mut args, &arg)?)),
            "--skip-pi-gram" => train_pi_gram = false,
            other => return Err(format!("unknown option: {other}").into()),
        }
    }

    let interpolation2 = interpolation2.ok_or("--interpolation2 is required")?;
    let held_out = held_out.ok_or("--held-out is required")?;
    let evals = evals.ok_or("--evals is required")?;
    let pinyin_index = pinyin_index.ok_or("--pinyin-index is required")?;
    let phrase_index = phrase_index.ok_or("--phrase-index is required")?;

    // interpolation2.text → counts → estimate λ → apply λ.
    let counts = parse_interpolation2(&read(&interpolation2)?);
    let deleted = count_deleted(&read(&held_out)?, train_pi_gram)?;
    let lambda = estimate_lambda(&counts, &deleted)?;

    // Decode the evaluation corpus against the system phrase index; the
    // model is floored over that index's lexicon, as `make` would.
    let dictionary = SystemDictionary::open(&pinyin_index, &phrase_index)?;
    let source = SystemPhraseSource::new(&dictionary);
    let model = build_model(&counts, lambda, source.lexicon_tokens());
    let sentences = parse_eval_corpus(&read(&evals)?)?;
    let report = correction_rate(&dictionary, &model, &source, &sentences)?;

    println!("average lambda:{:.6}", lambda.as_f64());
    println!("{}", report.correction_rate_line());
    eprintln!(
        "tested {} sentences, {} passed, {} mismatched",
        report.tested,
        report.passed,
        report.mismatches.len()
    );
    Ok(())
}

fn read(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    fs::read_to_string(path).map_err(|source| format!("cannot read {path:?}: {source}").into())
}

fn next(
    args: &mut std::iter::Skip<std::env::Args>,
    flag: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}").into())
}

fn print_help() {
    print!(
        "Usage: oxpinyin-eval --interpolation2 FILE --held-out FILE --evals FILE \\\n\
         \x20               --pinyin-index FILE --phrase-index FILE [--skip-pi-gram]\n\
         \n\
         Native evaluate.py: estimate λ, apply it, decode the evaluation\n\
         corpus against the system phrase index, print the correction rate.\n"
    );
}
