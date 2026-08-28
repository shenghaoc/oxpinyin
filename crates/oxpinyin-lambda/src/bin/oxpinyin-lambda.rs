//! `estimate_interpolation`-compatible CLI over the Rust λ estimator.
//!
//! Usage: oxpinyin-lambda [--deleted HELDOUT] [--export-dir DIR] SYSTEM
//!
//! Reads the system ngseg stream (T1's output) into the integer system
//! model — `SYSTEM_BIGRAM` + the freq-1-floored phrase-index unigram, via
//! the W9-T2 counter — and a held-out ngseg stream into `DELETED_BIGRAM`,
//! then runs the deleted-interpolation EM. Prints the per-context and
//! average λ in `estimate_interpolation`'s own stdout format
//! (`token:%d lambda:%f`, `average lambda:%f`) so the two are diffable.
//!
//! With no `--deleted`, the held-out slice is the system stream itself
//! (the maximal-overlap held-out configuration; see
//! `docs/findings/lambda-port.md`).

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use oxpinyin_counter::count_ngseg;
use oxpinyin_lambda::{count_deleted, estimate_lambda};
use oxpinyin_segment::{PhraseLexicon, locate_export_dir};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut skip_pi_gram = false;
    let mut deleted_path: Option<PathBuf> = None;
    let mut system_path: Option<PathBuf> = None;
    let mut export_dir: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "--skip-pi-gram-training" => skip_pi_gram = true,
            "--deleted" | "--deleted-bigram-file" => {
                deleted_path = Some(PathBuf::from(
                    args.next().ok_or("missing --deleted argument")?,
                ));
            }
            "--export-dir" => {
                export_dir = Some(PathBuf::from(args.next().ok_or("missing --export-dir")?));
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option: {flag}").into());
            }
            path => {
                if system_path.is_some() {
                    return Err("too many arguments".into());
                }
                system_path = Some(PathBuf::from(path));
            }
        }
    }

    let phrase_index = match export_dir {
        Some(dir) => dir.join("phrase_index.redb"),
        None => locate_export_dir()
            .map(|dir| dir.join("phrase_index.redb"))
            .ok_or(
                "no system-table export (phrase_index.redb); set --export-dir or PINYIN_EXPORT_DIR",
            )?,
    };
    let lexicon = PhraseLexicon::from_phrase_index(&phrase_index)?;

    let read = |path: &Option<PathBuf>| -> Result<String, Box<dyn std::error::Error>> {
        let bytes = match path {
            Some(path) => fs::read(path)?,
            None => {
                let mut buf = Vec::new();
                io::stdin().read_to_end(&mut buf)?;
                buf
            }
        };
        Ok(String::from_utf8(bytes)?)
    };

    let system_text = read(&system_path)?;
    let train_pi_gram = !skip_pi_gram;
    let system = count_ngseg(&lexicon, &system_text, train_pi_gram)?;

    // Held-out slice: the --deleted file, or the system stream itself.
    let deleted_text = match &deleted_path {
        Some(path) => fs::read_to_string(path)?,
        None => system_text.clone(),
    };
    let deleted = count_deleted(&deleted_text, train_pi_gram)?;

    let lambda = estimate_lambda(&system, &deleted)?;

    // `estimate_interpolation.cpp:129` / `:139` stdout shape.
    for (prev, value) in &lambda.per_context {
        println!("token:{prev} lambda:{value:.6}");
    }
    println!("average lambda:{:.6}", lambda.average);
    Ok(())
}

fn print_help() {
    print!(
        "Usage: oxpinyin-lambda [--deleted HELDOUT] [--export-dir DIR] [--skip-pi-gram-training] SYSTEM\n\
         \n\
         Reproduces gen_deleted_ngram + estimate_interpolation over ngseg streams.\n\
         SYSTEM feeds SYSTEM_BIGRAM + the floored unigram; --deleted feeds\n\
         DELETED_BIGRAM (default: the SYSTEM stream itself).\n\
         \n\
           --deleted FILE      held-out ngseg stream (DELETED_BIGRAM)\n\
           --export-dir DIR    system-table export (phrase_index.redb) for the freq-1 floor\n\
           --skip-pi-gram-training\n\
                               drop sentence-start boundary bigrams on both streams\n"
    );
}
