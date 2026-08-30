//! `gen_ngram`-compatible CLI over the Rust counter.
//!
//! Usage: oxpinyin-counter [--skip-pi-gram-training] [-o outputfile] [inputfile]
//!
//! Reads the ngseg segmented-token stream (T1's output) and emits the
//! canonical integer-count dump. The freq-1 floor comes from
//! the `phrase_index` table in the compiled-in backend's format
//! (`PINYIN_EXPORT_DIR` / `/tmp/oxpinyin-export`).

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::process::ExitCode;

use oxpinyin_counter::count_ngseg;
use oxpinyin_segment::PhraseLexicon;
use oxpinyin_segment::tool_cli::{self, ToolAction, ToolArgs};

fn main() -> ExitCode {
    tool_cli::run(run)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut tool = ToolArgs::default();
    if let ToolAction::Help = tool.parse_counting(std::env::args().skip(1))? {
        print_help();
        return Ok(());
    }

    let phrase_index = tool_cli::locate_phrase_index(tool.export_dir.as_deref())?;
    let lexicon = PhraseLexicon::from_phrase_index(&phrase_index)?;

    let bytes = tool_cli::read_input(tool.input.as_deref())?;
    let text = std::str::from_utf8(&bytes)?;
    let counts = count_ngseg(&lexicon, text, !tool.skip_pi_gram)?;
    let dump = counts.dump();

    tool_cli::write_output(tool.output.as_deref(), dump.as_bytes())?;
    Ok(())
}

fn print_help() {
    print!(
        "Usage: oxpinyin-counter [--skip-pi-gram-training] [-o outputfile] [inputfile]\n\
         \n\
         Extra options (not in gen_ngram):\n\
           --export-dir DIR    system-table export (the phrase_index table) for the freq-1 floor\n\
           --skip-pi-gram-training\n\
                               drop sentence-start boundary bigrams\n"
    );
}
