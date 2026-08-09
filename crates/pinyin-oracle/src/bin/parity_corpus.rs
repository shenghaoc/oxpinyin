//! Writes the parity corpus to disk.
//!
//! Generation itself lives in `pinyin_oracle::corpus` so the reproducibility
//! test can call it without touching the filesystem. This binary is only the
//! I/O shell.
//!
//! ```bash
//! cargo run -p pinyin-oracle --bin parity-corpus -- tests/parity/corpus/inputs
//! git diff --exit-code tests/parity/corpus/inputs
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use pinyin_oracle::corpus::{self, CORPUS_DIR};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let target = args
        .next()
        .map_or_else(|| PathBuf::from(CORPUS_DIR), PathBuf::from);

    if args.next().is_some() {
        eprintln!("usage: parity-corpus [OUTPUT_DIR]");
        eprintln!("default OUTPUT_DIR is {CORPUS_DIR}");
        return ExitCode::from(2);
    }

    if let Err(error) = std::fs::create_dir_all(&target) {
        eprintln!("cannot create {}: {error}", target.display());
        return ExitCode::FAILURE;
    }

    let strata = corpus::generate();
    let mut total = 0_usize;

    for stratum in &strata {
        let path = target.join(stratum.file_name);
        if let Err(error) = std::fs::write(&path, stratum.to_file_bytes()) {
            eprintln!("cannot write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        total += stratum.inputs.len();
        println!(
            "{:>6}  {}  {}",
            stratum.inputs.len(),
            stratum.file_name,
            stratum.description
        );
    }

    println!("{total:>6}  total across {} strata", strata.len());
    ExitCode::SUCCESS
}
