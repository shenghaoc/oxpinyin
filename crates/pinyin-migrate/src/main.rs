//! Converter and exporter: oracle data → portable redb tables.
//!
//! Two commands, per `docs/findings/data-layer-export.md`:
//!
//! - `convert <input.tkh> [-o <output.redb>] [-n <limit>]` — verbatim
//!   record-for-record copy of a Tkrzw HashDBM into a redb `data` table.
//!   Used for the system bigram and for ad-hoc inspection.
//! - `export --out-dir <dir> [--mini]` — full public-ABI export of the
//!   four system phrase libraries plus the bigram copy. Requires the
//!   `oracle-ffi` feature and the pin-built oracle (Linux-first).

#![warn(missing_docs)]

mod tkrzw;

#[cfg(feature = "oracle-ffi")]
mod export;

use std::fs;
use std::path::{Path, PathBuf};

const REDB_TABLE: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("data");

/// Writes `entries` (already ordered) into a fresh redb at `path`.
fn write_redb(
    path: &Path,
    entries: &[(Vec<u8>, Vec<u8>)],
) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    let db = redb::Database::create(path)?;
    {
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(REDB_TABLE)?;
            for (key, value) in entries {
                table.insert(key.as_slice(), value.as_slice())?;
            }
        }
        txn.commit()?;
    }
    let out_size = fs::metadata(path)?.len();
    eprintln!(
        "wrote {} records ({out_size} bytes) → {}",
        entries.len(),
        path.display()
    );
    Ok(())
}

fn usage() -> ! {
    eprintln!(
        "Usage:\n  pinyin-migrate convert <input.tkh> [-o <output.redb>] [-n <limit>]\n  \
         pinyin-migrate export --out-dir <dir> [--mini]   (requires --features oracle-ffi)"
    );
    std::process::exit(2);
}

fn convert(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut limit: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                output = Some(PathBuf::from(args.get(i).unwrap_or_else(|| usage())));
            }
            "-n" => {
                i += 1;
                limit = Some(
                    args.get(i)
                        .unwrap_or_else(|| usage())
                        .parse()
                        .map_err(|_| "invalid -n value")?,
                );
            }
            arg if !arg.starts_with('-') => {
                input = Some(PathBuf::from(arg));
            }
            _ => usage(),
        }
        i += 1;
    }

    let input = input.unwrap_or_else(|| usage());
    let output = output.unwrap_or_else(|| {
        let mut path = input.clone();
        path.set_extension("redb");
        path
    });

    let path_c = std::ffi::CString::new(input.to_string_lossy().as_bytes())
        .map_err(|_| "input path contains a NUL byte")?;
    let reader = tkrzw::TkrzwReader::open(&path_c)?;

    let mut entries = reader.entries()?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(n) = limit {
        entries.truncate(n);
    }

    write_redb(&output, &entries)
}

#[cfg(feature = "oracle-ffi")]
fn export(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut out_dir: Option<PathBuf> = None;
    let mut mini = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out-dir" => {
                i += 1;
                out_dir = Some(PathBuf::from(args.get(i).unwrap_or_else(|| usage())));
            }
            "--mini" => mini = true,
            _ => usage(),
        }
        i += 1;
    }

    export::run(&out_dir.unwrap_or_else(|| usage()), mini)
}

#[cfg(not(feature = "oracle-ffi"))]
fn export(_args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    Err(
        "`export` requires building with `--features oracle-ffi` on Linux \
         with the pin-built oracle installed"
            .into(),
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("convert") => convert(&args[2..]),
        Some("export") => export(&args[2..]),
        // The bare form `pinyin-migrate <input.tkh> [-o …] [-n …]` is the
        // invocation frozen in docs/findings/data-formats.md §1.1; keep it
        // as an alias for `convert`.
        Some(first) if !first.starts_with('-') => convert(&args[1..]),
        _ => usage(),
    }
}
