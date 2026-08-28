//! `ngseg`-compatible CLI over the Rust segmenter.
//!
//! Usage: oxpinyin-segment [--generate-extra-enter] [-o outputfile] [inputfile]
//!
//! Paths default to the export directory (`PINYIN_EXPORT_DIR` /
//! `/tmp/oxpinyin-export`) and the fetched model20 cache
//! (`PINYIN_MODEL_DIR` / `tools/model/fetch-model.sh`).

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use oxpinyin_segment::{PINNED_LAMBDA, Segmenter, SegmenterPaths, load_lambda};

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
    let mut model_dir: Option<PathBuf> = None;
    let mut table_conf: Option<PathBuf> = None;
    let mut lambda_override: Option<f32> = None;

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
            "--model-dir" => {
                model_dir = Some(PathBuf::from(args.next().ok_or("missing --model-dir")?));
            }
            "--table-conf" => {
                table_conf = Some(PathBuf::from(args.next().ok_or("missing --table-conf")?));
            }
            "--lambda" => {
                let raw = args.next().ok_or("missing --lambda")?;
                lambda_override = Some(raw.parse().map_err(|_| "invalid --lambda")?);
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

    let paths = match (export_dir, model_dir) {
        (Some(export), Some(model)) => SegmenterPaths::from_dirs(&export, &model),
        (None, None) => SegmenterPaths::discover()?,
        _ => {
            return Err("both --export-dir and --model-dir are required when either is set".into());
        }
    };
    let lambda = lambda_override
        .or_else(|| load_lambda(table_conf.as_deref()))
        .unwrap_or(PINNED_LAMBDA);
    let segmenter = Segmenter::open(&paths, lambda)?;

    let bytes = match input {
        Some(path) => fs::read(&path)?,
        None => {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf)?;
            buf
        }
    };
    let rendered = segmenter.segment_bytes(&bytes, extra_enter)?;

    match output {
        Some(path) => fs::write(path, rendered.as_bytes())?,
        None => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(rendered.as_bytes())?;
        }
    }
    Ok(())
}

fn print_help() {
    print!(
        "Usage: oxpinyin-segment [--generate-extra-enter] [-o outputfile] [inputfile]\n\
         \n\
         Extra options (not in ngseg):\n\
           --export-dir DIR    export directory (phrase_index.redb, bigram.redb)\n\
           --model-dir DIR     fetched model20 cache (interpolation2.text)\n\
           --table-conf FILE   read lambda parameter from table.conf\n\
           --lambda FLOAT      override lambda (default: pin 0.312699)\n"
    );
}
