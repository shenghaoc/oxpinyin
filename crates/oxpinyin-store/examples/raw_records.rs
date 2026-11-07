//! Every raw record of one libpinyin DBM file, as hex, one per line.
//!
//! `just w3-fixtures check` compares a regenerated `fixtures/w3/<ext>`
//! with the committed tree record by record: the container bytes of a
//! Kyoto Cabinet, tkrzw or Berkeley DB file are the writing library's own
//! layout and change with its version, while the records a libpinyin
//! build of that DBM reads do not. This example is the reader that check
//! uses; it links the backend the build selects and nothing else.
//!
//! ```text
//! cargo run -p oxpinyin-store --example raw_records \
//!     --no-default-features --features tkrzw -- tree pinyin_index.bin
//! cargo run -p oxpinyin-store --example raw_records \
//!     --no-default-features --features tkrzw -- hash bigram.db
//! ```
//!
//! `tree` walks a tree container (`pinyin_index.bin`, `phrase_index.bin`,
//! `punct.bin`, the `addon_*` pair) in its ascending key order; `hash`
//! collects a hash container's keys (`bigram.db`, `deleted_bigram.db`),
//! point-reads each and prints the rows sorted by key bytes, since the
//! walk order of a hash container is the library's own. Each line is
//! `<key hex> <value hex>`; an empty value prints as `<key hex> ` with
//! the trailing space, so a prefix marker row stays distinguishable from
//! a missing one.

use std::ops::Bound;
use std::path::Path;
use std::process::ExitCode;

use oxpinyin_store::{BackendError, DefaultStore, RawReadStore, StoreError};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (kind, path) = match args.as_slice() {
        [kind, path] if kind == "tree" || kind == "hash" => (kind.as_str(), Path::new(path)),
        _ => {
            eprintln!("usage: raw_records <tree|hash> <file>");
            return ExitCode::from(2);
        }
    };
    match dump(kind, path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("raw_records: {}: {error}", path.display());
            ExitCode::FAILURE
        }
    }
}

fn dump(kind: &str, path: &Path) -> Result<(), StoreError> {
    let mut rows: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    if kind == "tree" {
        let store = <DefaultStore as RawReadStore>::open_read_only(path)?;
        store.range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            rows.push((key.to_vec(), value.to_vec()));
            Ok(())
        })?;
    } else {
        let store = <DefaultStore as RawReadStore>::open_hash_read_only(path)?;
        let mut value = Vec::new();
        for key in store.native_keys()? {
            if !store.get_raw(&key, &mut value)? {
                return Err(StoreError::Backend(BackendError::from(format!(
                    "key {} listed but not readable",
                    hex(&key)
                ))));
            }
            rows.push((key, value.clone()));
        }
        rows.sort();
    }
    let mut out = String::new();
    for (key, value) in &rows {
        out.push_str(&hex(key));
        out.push(' ');
        out.push_str(&hex(value));
        out.push('\n');
    }
    print!("{out}");
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
