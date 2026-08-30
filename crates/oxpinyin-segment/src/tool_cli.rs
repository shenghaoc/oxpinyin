//! Shared CLI scaffold for the training-tool binaries.
//!
//! `oxpinyin-segment`, `oxpinyin-counter`, `oxpinyin-emitter`, and
//! `oxpinyin-lambda` each ship one `gen_*`-compatible binary built from the
//! same mechanical skeleton: an argument loop over a shared flag vocabulary,
//! export-dir resolution for the phrase-index table, a stdin-or-file input
//! read, an output-or-stdout write, and the exit-code runner. This module
//! holds that skeleton once; each binary contributes only its own flags and
//! help text. The binaries' observable behavior is unchanged: same flags,
//! same error strings, same exit codes, same output bytes.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// What the tool should do after parsing its command line.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ToolAction {
    /// `-h`/`--help` was given: print the tool's usage and exit 0.
    Help,
    /// Parsing succeeded: run the tool.
    Run,
}

/// The arguments every training tool shares.
///
/// Bin-specific flags live in the caller's closure, so a tool only ever
/// observes the fields its own parser accepts.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct ToolArgs {
    /// `--skip-pi-gram-training`: drop sentence-start boundary bigrams.
    pub skip_pi_gram: bool,
    /// `-o`/`--outputfile`: write here instead of standard output.
    pub output: Option<PathBuf>,
    /// The single positional input path, instead of standard input.
    pub input: Option<PathBuf>,
    /// `--export-dir`: the system-table export, else discovery.
    pub export_dir: Option<PathBuf>,
}

impl ToolArgs {
    /// Parses the training-tool CLI.
    ///
    /// Handles the vocabulary every tool shares — `-h`/`--help`,
    /// `--export-dir`, one positional input, and the unknown-option
    /// rejection — and delegates every other flag to `extra`, which
    /// receives the tool's own state plus the rest of the argument stream
    /// (so a flag that takes a value can consume it) and reports whether
    /// it consumed the flag.
    ///
    /// # Errors
    ///
    /// Mirrors the per-tool messages: a shared flag missing its value, a
    /// tool flag missing its value (from `extra`), an unknown option, or a
    /// second positional input.
    pub fn parse(
        &mut self,
        args: impl Iterator<Item = String>,
        mut extra: impl FnMut(&mut Self, &str, &mut dyn Iterator<Item = String>) -> Result<bool, String>,
    ) -> Result<ToolAction, String> {
        let mut args = args;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Ok(ToolAction::Help),
                "--export-dir" => {
                    self.export_dir =
                        Some(PathBuf::from(args.next().ok_or("missing --export-dir")?));
                }
                _ => {
                    if extra(self, &arg, &mut args)? {
                        continue;
                    }
                    if arg.starts_with('-') {
                        return Err(format!("unknown option: {arg}"));
                    }
                    if self.input.is_some() {
                        return Err("too many arguments".to_owned());
                    }
                    self.input = Some(PathBuf::from(arg));
                }
            }
        }
        Ok(ToolAction::Run)
    }

    /// Parses the counting tools' CLI: [`Self::parse`] plus
    /// `--skip-pi-gram-training` and `-o`/`--outputfile` — the exact flag
    /// set `oxpinyin-counter` and `oxpinyin-emitter` accept.
    ///
    /// # Errors
    ///
    /// Same as [`Self::parse`].
    pub fn parse_counting(
        &mut self,
        args: impl Iterator<Item = String>,
    ) -> Result<ToolAction, String> {
        self.parse(args, |tool, flag, rest| match flag {
            "--skip-pi-gram-training" => {
                tool.skip_pi_gram = true;
                Ok(true)
            }
            "-o" | "--outputfile" => {
                tool.output = Some(PathBuf::from(rest.next().ok_or("missing -o argument")?));
                Ok(true)
            }
            _ => Ok(false),
        })
    }
}

/// Resolves the phrase-index table path the counting tools read:
/// `export_dir` when given, else the discovered export directory.
///
/// # Errors
///
/// When no system-table export can be located; the message names the two
/// ways to fix it (`--export-dir` or `PINYIN_EXPORT_DIR`).
pub fn locate_phrase_index(export_dir: Option<&Path>) -> Result<PathBuf, String> {
    export_dir.map_or_else(
        || {
            crate::locate_export_dir()
                .map(|dir| dir.join(crate::default_store_file("phrase_index")))
                .ok_or_else(|| {
                    "no system-table export (the phrase_index table); \
                     set --export-dir or PINYIN_EXPORT_DIR"
                        .to_owned()
                })
        },
        |dir| Ok(dir.join(crate::default_store_file("phrase_index"))),
    )
}

/// Reads the tool input: the file at `path`, or standard input when `None`.
///
/// # Errors
///
/// Propagates I/O errors from the file read or the stdin drain.
pub fn read_input(path: Option<&Path>) -> io::Result<Vec<u8>> {
    if let Some(path) = path {
        std::fs::read(path)
    } else {
        let mut buffer = Vec::new();
        io::stdin().read_to_end(&mut buffer)?;
        Ok(buffer)
    }
}

/// Writes `bytes` to `path`, or to standard output when `None`.
///
/// # Errors
///
/// Propagates I/O errors from the file write or the stdout write.
pub fn write_output(path: Option<&Path>, bytes: &[u8]) -> io::Result<()> {
    path.map_or_else(
        || {
            let mut stdout = io::stdout().lock();
            stdout.write_all(bytes)
        },
        |path| std::fs::write(path, bytes),
    )
}

/// Runs one tool body and maps its result onto the process exit code:
/// success is 0, any error prints on stderr and exits 1.
pub fn run(run: fn() -> Result<(), Box<dyn std::error::Error>>) -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
