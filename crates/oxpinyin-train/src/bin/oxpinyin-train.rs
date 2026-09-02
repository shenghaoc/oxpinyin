//! `oxpinyin-train` — the one native command that runs the whole trainer main
//! workflow, no Python, `make`, SQLite, or libpinyin binaries.
//!
//! ```text
//! raw corpus → segment → generate candidates → estimate + sort →
//!   merge top N → validate → prune → validate → export →
//!   interpolation2.text → estimate λ → apply λ → correction rate
//! ```
//!
//! Usage:
//!   oxpinyin-train --text-dir DIR --model-dir DIR --final-dir DIR \
//!                  --index corpus.index --held-out held.segmented \
//!                  --evals evals2.text --pinyin-index pinyin_index.<ext> \
//!                  --phrase-index phrase_index.<ext> \
//!                  [--bigram bigram.<ext> --interpolation2 interpolation2.text \
//!                   --table-conf table.conf] \
//!                  [--merge N] [-k N] [--CDF F] [--fast] [--skip-pi-gram] NAME
//!
//! The segmenter model (phrase index + bigram + interpolation2 + λ) is
//! discovered from the system-table export unless the `--phrase-index` /
//! `--bigram` / `--interpolation2` paths are given explicitly. `--held-out` is
//! a segmented held-out corpus: it seeds both the candidate-scoring deleted
//! model and the final λ estimation's deleted counts.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oxpinyin_data::SystemDictionary;
use oxpinyin_eval::SystemPhraseSource;
use oxpinyin_kmm::{GenerateParams, KMixtureModel};
use oxpinyin_lambda::count_deleted;
use oxpinyin_segment::{PINNED_LAMBDA, Segmenter, SegmenterPaths, load_lambda};
use oxpinyin_train::{CorpusIndex, EvalInputs, SegmentMethod, TrainConfig, Trainer, TrainerPaths};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oxpinyin-train: {error}");
            ExitCode::from(1)
        }
    }
}

type Cli<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct Args {
    text_dir: Option<PathBuf>,
    model_dir: Option<PathBuf>,
    final_dir: Option<PathBuf>,
    index: Option<PathBuf>,
    held_out: Option<PathBuf>,
    evals: Option<PathBuf>,
    pinyin_index: Option<PathBuf>,
    phrase_index: Option<PathBuf>,
    bigram: Option<PathBuf>,
    interpolation2: Option<PathBuf>,
    table_conf: Option<PathBuf>,
    merge: Option<usize>,
    prune_k: Option<u32>,
    cdf: Option<f64>,
    fast: bool,
    skip_pi_gram: bool,
    tryname: Option<String>,
}

fn run() -> Cli {
    let args = parse_args()?;

    let mut config = TrainConfig::default();
    if let Some(merge) = args.merge {
        if merge == 0 {
            return Err("--merge must be at least 1 (0 would select no candidate)".into());
        }
        config.merge_number = merge;
    }
    if let Some(k) = args.prune_k {
        config.prune_k = k;
    }
    if let Some(cdf) = args.cdf {
        if !cdf.is_finite() || !(0.0..=1.0).contains(&cdf) {
            return Err(format!("--CDF must be finite and in [0, 1], got {cdf}").into());
        }
        config.prune_cdf = cdf;
    }
    if args.skip_pi_gram {
        config.train_pi_gram = false;
    }
    let method = if args.fast {
        SegmentMethod::Spseg
    } else {
        SegmentMethod::Ngseg
    };

    // The segmenter model: explicit paths if given, else discovery. Built
    // before any field is moved out of `args` below.
    let segmenter = build_segmenter(&args)?;

    let paths = TrainerPaths {
        text_dir: require(args.text_dir, "--text-dir")?,
        model_dir: require(args.model_dir, "--model-dir")?,
        final_dir: require(args.final_dir, "--final-dir")?,
    };
    let index = CorpusIndex::load(&require(args.index, "--index")?)?;
    let tryname = require(args.tryname, "NAME")?;

    // The held-out corpus seeds both the scoring deleted model (a KMM) and the
    // final λ estimation's deleted counts.
    let held_out = require(args.held_out, "--held-out")?;
    let held_text = read(&held_out)?;
    let mut scoring_deleted = KMixtureModel::new();
    let generate_params = GenerateParams {
        train_pi_gram: config.train_pi_gram,
        ..config.generate_params()
    };
    scoring_deleted.add_document(&held_text, generate_params)?;
    let deleted_counts = count_deleted(&held_text, config.train_pi_gram)?;

    // The evaluation decode target.
    let dictionary = SystemDictionary::open(
        &require(args.pinyin_index, "--pinyin-index")?,
        &require(args.phrase_index.clone(), "--phrase-index")?,
    )?;
    let source = SystemPhraseSource::new(&dictionary);
    let evals_text = read(&require(args.evals, "--evals")?)?;

    let eval = EvalInputs {
        dictionary: &dictionary,
        source: &source,
        evals_text: &evals_text,
        deleted: &deleted_counts,
    };

    let trainer = Trainer::new(config, paths, method);
    let outcome = trainer.run(&segmenter, &index, &scoring_deleted, &eval, &tryname)?;

    println!("candidates:{}", outcome.candidate_count);
    println!("average lambda:{:.6}", outcome.average_lambda.as_f64());
    println!("correction rate:{:.6}", outcome.correction_rate);
    eprintln!(
        "final model {} bytes; {} sentences tested, {} passed",
        outcome.interpolation2.len(),
        outcome.report.tested,
        outcome.report.passed
    );
    Ok(())
}

/// Builds the segmenter from explicit `--phrase-index`/`--bigram`/
/// `--interpolation2` when all three are given, else from discovery.
/// `--phrase-index` alone is legitimate (the evaluation dictionary needs
/// it); `--bigram` or `--interpolation2` without the full triple is an
/// error rather than a silent fall-back to a discovered model.
fn build_segmenter(args: &Args) -> Cli<Segmenter> {
    let lambda = load_lambda(args.table_conf.as_deref()).unwrap_or(PINNED_LAMBDA);
    match (&args.phrase_index, &args.bigram, &args.interpolation2) {
        (Some(phrase_index), Some(bigram), Some(interpolation2)) => {
            let paths = SegmenterPaths {
                phrase_index: phrase_index.clone(),
                bigram: bigram.clone(),
                interpolation2: interpolation2.clone(),
            };
            Ok(Segmenter::open(&paths, lambda)?)
        }
        _ => {
            if args.bigram.is_some() || args.interpolation2.is_some() {
                return Err(
                    "--phrase-index, --bigram, and --interpolation2 must be given together".into(),
                );
            }
            Ok(Segmenter::discover(args.table_conf.as_deref())?)
        }
    }
}

fn parse_args() -> Cli<Args> {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "--text-dir" => args.text_dir = Some(next(&mut iter, &arg)?.into()),
            "--model-dir" => args.model_dir = Some(next(&mut iter, &arg)?.into()),
            "--final-dir" => args.final_dir = Some(next(&mut iter, &arg)?.into()),
            "--index" => args.index = Some(next(&mut iter, &arg)?.into()),
            "--held-out" => args.held_out = Some(next(&mut iter, &arg)?.into()),
            "--evals" => args.evals = Some(next(&mut iter, &arg)?.into()),
            "--pinyin-index" => args.pinyin_index = Some(next(&mut iter, &arg)?.into()),
            "--phrase-index" => args.phrase_index = Some(next(&mut iter, &arg)?.into()),
            "--bigram" => args.bigram = Some(next(&mut iter, &arg)?.into()),
            "--interpolation2" => args.interpolation2 = Some(next(&mut iter, &arg)?.into()),
            "--table-conf" => args.table_conf = Some(next(&mut iter, &arg)?.into()),
            "--merge" => {
                args.merge = Some(
                    next(&mut iter, &arg)?
                        .parse()
                        .map_err(|_| "invalid --merge")?,
                );
            }
            "-k" => {
                args.prune_k = Some(next(&mut iter, &arg)?.parse().map_err(|_| "invalid -k")?);
            }
            "--CDF" => {
                args.cdf = Some(
                    next(&mut iter, &arg)?
                        .parse()
                        .map_err(|_| "invalid --CDF")?,
                );
            }
            "--fast" => args.fast = true,
            "--skip-pi-gram" => args.skip_pi_gram = true,
            other if other.starts_with('-') => {
                return Err(format!("unknown option: {other}").into());
            }
            other => args.tryname = Some(other.to_owned()),
        }
    }
    Ok(args)
}

fn require<T>(value: Option<T>, flag: &str) -> Cli<T> {
    value.ok_or_else(|| format!("{flag} is required").into())
}

fn read(path: &Path) -> Cli<String> {
    std::fs::read_to_string(path).map_err(|source| format!("cannot read {path:?}: {source}").into())
}

fn next(iter: &mut impl Iterator<Item = String>, flag: &str) -> Cli<String> {
    iter.next()
        .ok_or_else(|| format!("missing value for {flag}").into())
}

fn print_help() {
    print!(
        "Usage: oxpinyin-train --text-dir DIR --model-dir DIR --final-dir DIR \\\n\
         \x20                   --index corpus.index --held-out held.segmented \\\n\
         \x20                   --evals evals2.text --pinyin-index FILE --phrase-index FILE \\\n\
         \x20                   [--bigram FILE --interpolation2 FILE --table-conf FILE] \\\n\
         \x20                   [--merge N] [-k N] [--CDF F] [--fast] [--skip-pi-gram] NAME\n\
         \n\
         Runs the whole native trainer main workflow (segment → generate →\n\
         estimate → merge/prune → evaluate) and prints the average λ and the\n\
         correction rate. The segmenter model is discovered unless the phrase\n\
         index / bigram / interpolation2 paths are given.\n"
    );
}
