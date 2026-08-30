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
use std::path::PathBuf;
use std::process::ExitCode;

use oxpinyin_counter::count_ngseg;
use oxpinyin_lambda::{count_deleted, estimate_lambda};
use oxpinyin_segment::PhraseLexicon;
use oxpinyin_segment::tool_cli::{self, ToolAction, ToolArgs};

fn main() -> ExitCode {
    tool_cli::run(run)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut tool = ToolArgs::default();
    let mut deleted_path: Option<PathBuf> = None;
    if let ToolAction::Help =
        tool.parse(std::env::args().skip(1), |tool, flag, rest| match flag {
            "--skip-pi-gram-training" => {
                tool.skip_pi_gram = true;
                Ok(true)
            }
            "--deleted" | "--deleted-bigram-file" => {
                deleted_path = Some(PathBuf::from(
                    rest.next().ok_or("missing --deleted argument")?,
                ));
                Ok(true)
            }
            _ => Ok(false),
        })?
    {
        print_help();
        return Ok(());
    }

    let phrase_index = tool_cli::locate_phrase_index(tool.export_dir.as_deref())?;
    let lexicon = PhraseLexicon::from_phrase_index(&phrase_index)?;

    let system_text = String::from_utf8(tool_cli::read_input(tool.input.as_deref())?)?;
    let train_pi_gram = !tool.skip_pi_gram;
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
           --export-dir DIR    system-table export (the phrase_index table) for the freq-1 floor\n\
           --skip-pi-gram-training\n\
                               drop sentence-start boundary bigrams on both streams\n"
    );
}
