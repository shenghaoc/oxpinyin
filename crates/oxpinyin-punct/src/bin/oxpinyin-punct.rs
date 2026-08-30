//! `oxpinyin-punct` — the punctuation-table generator.
//!
//! Two stages, mirroring `genpunct.py`:
//!   count  {SEGMENTED}+   → per-index table (pruned at 500)
//!   merge  {TABLE}+       → global puncts.table (pruned at 10000)
//! The intermediate and final format is the `token word punct freq` table
//! (`puncts.table`), so per-index outputs feed straight into `merge`.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oxpinyin_punct::{ALL_INDEX_THRESHOLD, PER_INDEX_THRESHOLD, PunctCounts};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = match command.as_str() {
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        "count" => run(&args[1..], Stage::Count),
        "merge" => run(&args[1..], Stage::Merge),
        other => Err(format!("unknown subcommand: {other}\n\n{USAGE}").into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oxpinyin-punct: {error}");
            ExitCode::from(1)
        }
    }
}

const USAGE: &str = "\
Usage: oxpinyin-punct <count|merge> [--threshold N] [-o OUT] {INPUT}+

  count  segmented documents -> per-index table (default --threshold 500)
  merge  per-index tables    -> global puncts.table (default --threshold 10000)
";

#[derive(Clone, Copy)]
enum Stage {
    Count,
    Merge,
}

type Cli = Result<(), Box<dyn std::error::Error>>;

fn run(args: &[String], stage: Stage) -> Cli {
    let mut threshold: Option<u64> = None;
    let mut output: Option<PathBuf> = None;
    let mut inputs: Vec<PathBuf> = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--threshold" => {
                threshold = Some(
                    iter.next()
                        .ok_or("missing --threshold value")?
                        .parse()
                        .map_err(|_| "invalid --threshold")?,
                );
            }
            "-o" | "--output" => {
                output = Some(PathBuf::from(iter.next().ok_or("missing -o value")?));
            }
            other => inputs.push(PathBuf::from(other)),
        }
    }
    if inputs.is_empty() {
        return Err("no input files".into());
    }

    let mut counts = PunctCounts::new();
    match stage {
        Stage::Count => {
            for input in &inputs {
                counts.add_document(&read(input)?)?;
            }
            counts.prune(threshold.unwrap_or(PER_INDEX_THRESHOLD));
        }
        Stage::Merge => {
            for input in &inputs {
                counts.merge(&PunctCounts::from_table(&read(input)?)?);
            }
            counts.prune(threshold.unwrap_or(ALL_INDEX_THRESHOLD));
        }
    }

    let table = counts.to_table();
    match output {
        Some(path) => fs::write(&path, table.as_bytes())
            .map_err(|source| format!("cannot write {path:?}: {source}"))?,
        None => io::stdout().lock().write_all(table.as_bytes())?,
    }
    Ok(())
}

fn read(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    fs::read_to_string(path).map_err(|source| format!("cannot read {path:?}: {source}").into())
}
