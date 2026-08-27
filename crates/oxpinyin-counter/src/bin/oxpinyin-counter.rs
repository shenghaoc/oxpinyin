//! `gen_ngram`-compatible CLI over the Rust counter.
//!
//! Usage: oxpinyin-counter [--skip-pi-gram-training] [-o outputfile] [inputfile]
//!
//! Reads the ngseg segmented-token stream (T1's output) and emits the
//! canonical integer-count dump. The freq-1 floor comes from
//! `phrase_index.redb` (`PINYIN_EXPORT_DIR` / `/tmp/oxpinyin-export`).

#![allow(missing_docs)]

use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use oxpinyin_counter::count_ngseg;
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

/// Runs the command-line interface and writes canonical integer-count output.
///
/// # Examples
///
/// ```no_run
/// run()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Returns
///
/// `Ok(())` when processing completes successfully; an error if command-line
/// arguments, the phrase index, input, encoding, counting, or output cannot be
/// processed.
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut skip_pi_gram = false;
    let mut output: Option<PathBuf> = None;
    let mut input: Option<PathBuf> = None;
    let mut export_dir: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "--skip-pi-gram-training" => skip_pi_gram = true,
            "-o" | "--outputfile" => {
                output = Some(PathBuf::from(args.next().ok_or("missing -o argument")?));
            }
            "--export-dir" => {
                export_dir = Some(PathBuf::from(args.next().ok_or("missing --export-dir")?));
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option: {flag}").into());
            }
            path => {
                if input.is_some() {
                    return Err("too many arguments".into());
                }
                input = Some(PathBuf::from(path));
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

    let bytes = match input {
        Some(path) => fs::read(&path)?,
        None => {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf)?;
            buf
        }
    };
    let text = std::str::from_utf8(&bytes)?;
    let counts = count_ngseg(&lexicon, text, !skip_pi_gram)?;
    let dump = counts.dump();

    match output {
        Some(path) => fs::write(path, dump.as_bytes())?,
        None => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(dump.as_bytes())?;
        }
    }
    Ok(())
}

/// Prints the command-line usage information and supported options.
///
/// # Examples
///
/// ```
/// print_help();
/// ```
fn print_help() {
    print!(
        "Usage: oxpinyin-counter [--skip-pi-gram-training] [-o outputfile] [inputfile]\n\
         \n\
         Extra options (not in gen_ngram):\n\
           --export-dir DIR    system-table export (phrase_index.redb) for the freq-1 floor\n\
           --skip-pi-gram-training\n\
                               drop sentence-start boundary bigrams\n"
    );
}
