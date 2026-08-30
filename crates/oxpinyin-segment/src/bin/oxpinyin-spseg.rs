//! `spseg`-compatible CLI: fewest-words shortest-path segmentation.
//!
//! Usage: oxpinyin-spseg [--generate-extra-enter] [-o outputfile] [inputfile]
//!
//! Consults only the phrase table (`phrase_index`), so it needs an export
//! directory (`--export-dir` / `PINYIN_EXPORT_DIR` / `/tmp/oxpinyin-export`)
//! but no model20 cache or bigram.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use oxpinyin_segment::{
    DEFAULT_EXPORT_DIR, EXPORT_DIR_ENV, PhraseLexicon, default_store_file, locate_export_dir, spseg,
};

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
    let mut extra_enter = false;
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
            "--generate-extra-enter" => extra_enter = true,
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

    let export = export_dir.or_else(locate_export_dir).ok_or_else(|| {
        format!("no export at ${EXPORT_DIR_ENV} or {DEFAULT_EXPORT_DIR}; pass --export-dir")
    })?;
    let phrase_index = export.join(default_store_file("phrase_index"));
    let lexicon = PhraseLexicon::from_phrase_index(&phrase_index)?;

    let bytes = match input {
        Some(path) => fs::read(&path)?,
        None => {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf)?;
            buf
        }
    };
    let rendered = spseg::segment_bytes(&lexicon, &bytes, extra_enter);

    match output {
        Some(path) => fs::write(path, rendered.as_bytes())?,
        None => io::stdout().lock().write_all(rendered.as_bytes())?,
    }
    Ok(())
}

fn print_help() {
    print!(
        "Usage: oxpinyin-spseg [--generate-extra-enter] [-o outputfile] [inputfile]\n\
         \n\
         Fewest-words shortest-path segmentation (libpinyin spseg).\n\
         \n\
         Extra options (not in spseg):\n\
           --export-dir DIR    export directory holding the phrase_index table\n"
    );
}
