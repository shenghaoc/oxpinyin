//! `export_interpolation`-compatible CLI over the Rust emitter.
//!
//! Usage: oxpinyin-emitter [--skip-pi-gram-training] [-o outputfile] [inputfile]
//!
//! Reads the ngseg segmented-token stream (T1's output), counts it with
//! T2's counter (freq-1 floor from the `phrase_index` table in the
//! compiled-in backend's format), and writes
//! `interpolation2.text` in the grammar `parse_interpolation2` reads.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::process::ExitCode;

use oxpinyin_counter::count_ngseg;
use oxpinyin_emitter::emit_interpolation2;
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

    let system_dir = tool_cli::locate_system_dir(tool.export_dir.as_deref())?;
    let lexicon = PhraseLexicon::from_system_dir(&system_dir)?;

    let bytes = tool_cli::read_input(tool.input.as_deref())?;
    let text = std::str::from_utf8(&bytes)?;
    let counts = count_ngseg(&lexicon, text, !tool.skip_pi_gram)?;
    let dump = emit_interpolation2(&counts, &lexicon);

    tool_cli::write_output(tool.output.as_deref(), dump.as_bytes())?;
    Ok(())
}

fn print_help() {
    print!(
        "Usage: oxpinyin-emitter [--skip-pi-gram-training] [-o outputfile] [inputfile]\n\
         \n\
         Extra options (not in export_interpolation):\n\
           --export-dir DIR    system data directory (the chunk files) for phrase text\n\
           --skip-pi-gram-training\n\
                               drop sentence-start boundary bigrams when counting\n"
    );
}
