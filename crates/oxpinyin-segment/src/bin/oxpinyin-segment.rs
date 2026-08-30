//! `ngseg`-compatible CLI over the Rust segmenter.
//!
//! Usage: oxpinyin-segment [--generate-extra-enter] [-o outputfile] [inputfile]
//!
//! Paths default to the export directory (`PINYIN_EXPORT_DIR` /
//! `/tmp/oxpinyin-export`) and the fetched model20 cache
//! (`PINYIN_MODEL_DIR` / `tools/model/fetch-model.sh`).

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::path::PathBuf;
use std::process::ExitCode;

use oxpinyin_segment::tool_cli::{self, ToolAction, ToolArgs};
use oxpinyin_segment::{PINNED_LAMBDA, Segmenter, SegmenterPaths, load_lambda};

fn main() -> ExitCode {
    tool_cli::run(run)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut tool = ToolArgs::default();
    let mut extra_enter = false;
    let mut model_dir: Option<PathBuf> = None;
    let mut table_conf: Option<PathBuf> = None;
    let mut lambda_override: Option<f32> = None;
    if let ToolAction::Help =
        tool.parse(std::env::args().skip(1), |tool, flag, rest| match flag {
            "--generate-extra-enter" => {
                extra_enter = true;
                Ok(true)
            }
            "-o" | "--outputfile" => {
                tool.output = Some(PathBuf::from(rest.next().ok_or("missing -o argument")?));
                Ok(true)
            }
            "--model-dir" => {
                model_dir = Some(PathBuf::from(rest.next().ok_or("missing --model-dir")?));
                Ok(true)
            }
            "--table-conf" => {
                table_conf = Some(PathBuf::from(rest.next().ok_or("missing --table-conf")?));
                Ok(true)
            }
            "--lambda" => {
                let raw = rest.next().ok_or("missing --lambda")?;
                lambda_override = Some(raw.parse().map_err(|_| "invalid --lambda")?);
                Ok(true)
            }
            _ => Ok(false),
        })?
    {
        print_help();
        return Ok(());
    }

    let paths = match (tool.export_dir.as_deref(), model_dir.as_deref()) {
        (Some(export), Some(model)) => SegmenterPaths::from_dirs(export, model),
        (None, None) => SegmenterPaths::discover()?,
        _ => {
            return Err("both --export-dir and --model-dir are required when either is set".into());
        }
    };
    let lambda = lambda_override
        .or_else(|| load_lambda(table_conf.as_deref()))
        .unwrap_or(PINNED_LAMBDA);
    let segmenter = Segmenter::open(&paths, lambda)?;

    let bytes = tool_cli::read_input(tool.input.as_deref())?;
    let rendered = segmenter.segment_bytes(&bytes, extra_enter)?;

    tool_cli::write_output(tool.output.as_deref(), rendered.as_bytes())?;
    Ok(())
}

fn print_help() {
    print!(
        "Usage: oxpinyin-segment [--generate-extra-enter] [-o outputfile] [inputfile]\n\
         \n\
         Extra options (not in ngseg):\n\
           --export-dir DIR    export directory (the phrase_index and bigram tables)\n\
           --model-dir DIR     fetched model20 cache (interpolation2.text)\n\
           --table-conf FILE   read lambda parameter from table.conf\n\
           --lambda FLOAT      override lambda (default: pin 0.312699)\n"
    );
}
