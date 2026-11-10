//! A frozen case's pin evidence as a Rust value, and the one comparison every frozen case makes.
//! Invocation, setup, masks and the inventory stay in the tests.
use super::Snapshot;
use assert_cmd::prelude::*;
use std::collections::BTreeSet;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt as _;
use std::process::Output;

pub use File::{Rows, Sha256};
pub use Status::Exit;
#[cfg(unix)]
pub use Status::Signal;
pub use Stdout::{Exact, GlibOption};

/// What the pin did, as its `tests/cases` JSON entry froze it.
pub struct Run {
    pub status: Status,
    pub stdout: Stdout,
    pub stderr: &'static [u8],
    /// Each path whose state after the run is not its pre-run bytes: a new or changed file's
    /// SHA-256 and length, or a DBM file's rows. Every other pre-run file must be byte-identical,
    /// every pre-run directory must remain, and no other path may exist.
    pub files: &'static [(&'static str, File)],
}

#[derive(Clone, Copy, Debug)]
pub enum Status {
    Exit(i32),
    /// Unix only: a wait status that names the signal; Windows has no signal statuses.
    #[cfg(unix)]
    Signal(i32),
}

#[derive(Clone, Copy, Debug)]
pub enum Stdout {
    Exact(&'static [u8]),
    /// GLib help text frozen with the pin's `[OPTION?]` placeholder; the run may print any
    /// spelling in `GLIB_OPTION` in its place.
    GlibOption(&'static str),
}

/// GLib's English usage placeholder: the pin's frozen spelling, then the older and non-UTF-8 variants.
const GLIB_OPTION: [&str; 3] = ["[OPTION?]", "[OPTION\u{2026}]", "[OPTION...]"];

#[derive(Clone, Copy, Debug)]
pub enum File {
    /// The masked bytes' SHA-256 and length.
    Sha256(&'static str, u64),
    /// The SHA-256 of the sorted key and value rows (`rows_digest`).
    Rows(&'static str),
}

impl Stdout {
    /// The bytes the run must print: for `GlibOption`, the spelling the run used, else the pin's.
    fn expected(self, actual: &[u8]) -> Vec<u8> {
        match self {
            Exact(bytes) => bytes.to_vec(),
            GlibOption(text) => {
                assert!(
                    text.contains(GLIB_OPTION[0]),
                    "GlibOption without {}",
                    GLIB_OPTION[0]
                );
                GLIB_OPTION
                    .iter()
                    .map(|spelling| text.replace(GLIB_OPTION[0], spelling).into_bytes())
                    .find(|spelled| spelled == actual)
                    .unwrap_or_else(|| text.as_bytes().to_vec())
            }
        }
    }
}

impl Run {
    /// `inventory()`'s lines for the pre-run tree with `files` applied.
    fn inventory(&self, before: &Snapshot) -> String {
        let named: BTreeSet<&str> = self.files.iter().map(|(path, _)| *path).collect();
        let mut lines: Vec<String> = before
            .files
            .keys()
            .filter(|path| !named.contains(path.as_str()))
            .map(|path| format!("{path} unchanged"))
            .chain(before.dirs.iter().map(|path| format!("{path}/ directory")))
            .collect();
        lines.extend(self.files.iter().map(|(path, file)| match file {
            Sha256(digest, length) => format!("{path} sha256 {digest} {length}"),
            Rows(digest) => format!("{path} rows {digest}"),
        }));
        lines.sort_unstable();
        lines.iter().map(|line| format!("{line}\n")).collect()
    }
}

/// Compares a run with its expectation: the status, both streams byte for byte, and the whole
/// tree. `files` is `inventory()` after the run; `before` is `snapshot_inputs()` before it.
pub fn check(output: Output, files: &str, before: &Snapshot, expected: &Run) {
    let stdout = expected.stdout.expected(&output.stdout);
    check_stdout(output, stdout, files, before, expected);
}

fn check_stdout(output: Output, stdout: Vec<u8>, files: &str, before: &Snapshot, expected: &Run) {
    let assertion = output.assert();
    let assertion = match expected.status {
        Exit(code) => assertion.code(code),
        #[cfg(unix)]
        Signal(signal) => {
            let assertion = assertion.interrupted();
            assert_eq!(
                assertion.get_output().status.signal(),
                Some(signal),
                "status"
            );
            assertion
        }
    };
    assertion.stdout(stdout).stderr(expected.stderr);
    assert_eq!(files, expected.inventory(before), "files");
}

/// Preserve the captured product rows and file effects when moving a driver
/// into the REPL. Clock lines belonged solely to the retired benchmark loop.
pub fn check_repl(output: Output, files: &str, before: &Snapshot, expected: &Run) {
    let stdout = expected.stdout.expected(&output.stdout);
    let mut lines = stdout.split_inclusive(|byte| *byte == b'\n').peekable();
    let mut product = Vec::new();
    while let Some(line) = lines.next() {
        if line.starts_with(b"Spent <TIME> us for ") {
            if lines.peek() == Some(&b"\n".as_slice()) {
                lines.next();
            }
        } else {
            product.extend_from_slice(line);
        }
    }
    check_stdout(output, product, files, before, expected);
}

// Values that expectations in more than one suite share.
pub const AVERAGE_LAMBDA_NAN: Stdout = if cfg!(target_arch = "aarch64") {
    Exact(b"average lambda:nan\n")
} else {
    Exact(b"average lambda:-nan\n")
};
pub const CANT_OPEN_ABSENT_TXT: &[u8] = b"Can't open file absent.txt.\n";
pub const CANT_WRITE_MISSING_OUT_TABLE: &[u8] = b"Can't write file missing/out.table.\n";
pub const EMPTY_FILE: File = Sha256(
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    0,
);
pub const EMPTY_FILE_INPUT: &[u8] = b"empty file input.\n";
pub const EMPTY_TEMP_OUT_FILES: &[(&str, File)] = &[("temp.out", EMPTY_FILE)];
pub const KC_EMPTY_TREE_DB: File = Sha256(
    "15a18ea438d9cb5e750a69d154f776da8e62ecfb7ab699c1b852ceff9f93be63",
    6297720,
);
pub const LOAD_GB_CHAR_BIN_FAILED: &[u8] = b"load gb_char.bin failed!\n";
pub const NNODES_0: &[u8] = b"nnodes: 0\n";
pub const NNODES_1: &[u8] = b"nnodes: 1\n";
pub const NNODES_2: &[u8] = b"nnodes: 2\n";
pub const NNODES_5: &[u8] = b"nnodes: 5\n";
pub const NO_ROWS: File = Rows("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
pub const OPEN_TABLE_CONF_FAILED: &[u8] = b"open table.conf failed.\nload table.conf failed.\n";
pub const UNKNOWN_OPTION_BOGUS: &[u8] = b"option parsing failed:Unknown option --bogus\n";
