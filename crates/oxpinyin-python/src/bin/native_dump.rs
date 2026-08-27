//! Native-side corpus transcript generator for the Python parity tests.
//!
//! Reads a parity corpus (JSON), replays it through the pure-Rust public
//! API — the same runtime types the PyO3 binding wraps, with no Python in
//! the process — and writes the transcript document for `tests_py/test_parity.py`
//! to compare against.
//!
//! ```text
//! cargo run -p oxpinyin-python --bin native-dump -- \
//!     crates/oxpinyin-python/parity-corpus.json fixtures/w3 native.json
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    // `args_os`, not `args`: platform-native paths need not be UTF-8, and
    // `args` panics on them; `PathBuf: From<OsString>` takes them as-is.
    let mut args = std::env::args_os().skip(1);
    let (corpus_path, system_dir, out_path) =
        match (args.next(), args.next(), args.next(), args.next().is_none()) {
            (Some(c), Some(s), Some(o), true) => {
                (PathBuf::from(c), PathBuf::from(s), PathBuf::from(o))
            }
            _ => {
                eprintln!("usage: native-dump <corpus.json> <system-dir> <out.json>");
                return ExitCode::from(2);
            }
        };

    let Ok(corpus_text) = std::fs::read_to_string(&corpus_path) else {
        eprintln!("cannot read corpus {}", corpus_path.display());
        return ExitCode::from(2);
    };
    let Ok(corpus) = serde_json::from_str(&corpus_text) else {
        eprintln!("corpus {} is not valid JSON", corpus_path.display());
        return ExitCode::from(2);
    };

    match oxpinyin_python::dump::run_corpus(&corpus, &system_dir) {
        Ok(transcript) => {
            let rendered = serde_json::to_string_pretty(&transcript).unwrap_or_default();
            if std::fs::write(&out_path, rendered + "\n").is_err() {
                eprintln!("cannot write {}", out_path.display());
                return ExitCode::from(2);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
