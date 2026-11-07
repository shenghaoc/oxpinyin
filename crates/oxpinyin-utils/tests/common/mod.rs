//! Byte-exact helpers for ordinary utility integration tests. No capture or fixture writer.
#![allow(dead_code)]
pub mod expected;
use oxpinyin_store::{DefaultStore, RawReadStore as _};
use regex::bytes::Regex;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
pub type Failure = Box<dyn Error>;
pub const CELL: &str = match oxpinyin_data::DEFAULT_STORE_EXT.as_bytes() {
    b"db" => "bdb",
    b"kct" => "kc",
    _ => "tkrzw",
};
fn collect_tree(base: &Path, at: &Path, into: &mut Vec<PathBuf>) -> Result<(), Failure> {
    let mut entries = std::fs::read_dir(at)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            into.push(path.strip_prefix(base)?.to_path_buf());
            collect_tree(base, &path, into)?;
        } else {
            into.push(path.strip_prefix(base)?.to_path_buf());
        }
    }
    Ok(())
}

/// `tests/timer.h:36-44`: only the clock-derived fields, never the operation count.
pub fn mask_print_time(stdout: &[u8]) -> Vec<u8> {
    const FLOAT: &str = r"[+-]?(?:[0-9]+\.[0-9]{6}|inf|nan)";
    static TIME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r"(?m-u)^Spent -?[0-9]+ us for (-?[0-9]+) operations, {FLOAT} us/op, {FLOAT} times/s\.\n"
        ))
        .expect("print-time pattern")
    });
    TIME.replace_all(
        stdout,
        &b"Spent <TIME> us for $1 operations, <TIME> us/op, <TIME> times/s.\n"[..],
    )
    .into_owned()
}

/// The process ID and clock of a GLib diagnostic prefix.
pub fn mask_glib_prefix(stderr: &[u8]) -> Vec<u8> {
    static PREFIX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?m-u)^((?:\*\* )?\([^():\n]+:)[0-9]+(\): (?:[A-Za-z0-9_.-]+-)?(?:CRITICAL|WARNING|MESSAGE|ERROR|DEBUG|INFO) \*\*: )[0-9][0-9]:[0-9][0-9]:[0-9][0-9]\.[0-9]+: ",
        )
        .expect("GLib prefix pattern")
    });
    PREFIX
        .replace_all(stderr, &b"${1}PID${2}TIME: "[..])
        .into_owned()
}

/// Only `PinyinIndexItem2<N>`'s two tail bytes for odd N (`pinyin_phrase3.h:181-195`,
/// `chewing_large_table2.h:208-230`); a malformed length fails.
pub fn mask_pinyin_index_row(key: &[u8], value: &mut [u8]) -> Result<(), Failure> {
    if key.is_empty() || !key.len().is_multiple_of(2) || key.len() > 32 {
        return Err("invalid pinyin-index key length".into());
    }
    let fields = 4 + key.len();
    let stride = (fields + 3) & !3;
    if !value.len().is_multiple_of(stride) {
        return Err("invalid pinyin-index item length".into());
    }
    for item in value.chunks_exact_mut(stride) {
        item[fields..].fill(0);
    }
    Ok(())
}

/// Undoes the harness's own `/tmp` override in a diagnostic: the program and its
/// DBM library print the path they open, which the pin opens as `/tmp/<name>`.
pub fn undo_tmp_override(stderr: &[u8], directory: &Path) -> Vec<u8> {
    let directory = directory.as_os_str().as_encoded_bytes();
    let mut out = Vec::with_capacity(stderr.len());
    let mut rest = stderr;
    while let Some(at) = rest
        .windows(directory.len())
        .position(|part| part == directory)
    {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(b"/tmp");
        rest = &rest[at + directory.len()..];
    }
    out.extend_from_slice(rest);
    out
}

/// Eight clock bytes at offset 48 of an unclosed tkrzw HashDBM
/// (`tkrzw_dbm_hash.cc:43,1143,1252` at 9ee84164); anything else is unchanged.
pub fn mask_tkrzw_timestamp(data: &mut [u8]) {
    if data.len() >= 128 && data.starts_with(b"TkrzwHDB\n") {
        data[48..56].fill(0);
    }
}

/// One DBM file's key and value pairs.
type Rows = Vec<(Vec<u8>, Vec<u8>)>;

pub fn rows(path: &Path) -> Result<Rows, Failure> {
    let mut rows = Vec::new();
    // A tkrzw TreeDBM is backed by a HashDBM: opening it as a hash can
    // succeed but yields internal pages, not the tree's logical records.
    if let Ok(store) = DefaultStore::open_read_only(path) {
        store.range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            rows.push((key.to_vec(), value.to_vec()));
            Ok(())
        })?;
    } else {
        let store = DefaultStore::open_hash_read_only(path)?;
        for key in store.native_keys()? {
            let mut value = Vec::new();
            if !store.get_raw(&key, &mut value)? {
                return Err("a listed hash key has no value".into());
            }
            rows.push((key, value));
        }
    }
    rows.sort_unstable();
    Ok(rows)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn rows_digest(path: &Path, pinyin_index: bool) -> Result<String, Failure> {
    let mut text = String::new();
    for (key, mut value) in rows(path)? {
        if pinyin_index {
            mask_pinyin_index_row(&key, &mut value)?;
        }
        let _ = writeln!(text, "{} {}", hex(&key), hex(&value));
    }
    Ok(sha256(text.as_bytes()))
}

fn is_sealed_link(path: &Path) -> bool {
    std::fs::read_link(path).is_ok_and(|target| target.starts_with("/proc/self/fd"))
}

/// The pre-run tree, keyed by the same relative paths as `inventory()`.
pub struct Snapshot {
    pub files: BTreeMap<String, Vec<u8>>,
    pub dirs: BTreeSet<String>,
}

/// Pre-run file bytes and directories.
/// Follow file links so the sealed-input cases retain their original bytes.
pub fn snapshot_inputs(root: &Path) -> Snapshot {
    let mut inputs = Snapshot {
        files: BTreeMap::new(),
        dirs: BTreeSet::new(),
    };
    let mut pending = vec![root.to_path_buf()];
    while let Some(at) = pending.pop() {
        for entry in std::fs::read_dir(at).expect("input paths") {
            let entry = entry.expect("input entry");
            let path = entry.path();
            let name = path
                .strip_prefix(root)
                .expect("relative input")
                .to_str()
                .expect("fixture name")
                .to_owned();
            if entry.file_type().expect("input type").is_dir() {
                inputs.dirs.insert(name);
                pending.push(path);
            } else {
                inputs
                    .files
                    .insert(name, std::fs::read(path).expect("input bytes"));
            }
        }
    }
    inputs
}

/// The complete sorted file list, with only the explicitly named DBM and clock masks.
pub fn inventory(
    root: &Path,
    inputs: &Snapshot,
    dbm: &[&str],
    pinyin: &[&str],
    sealed: &[&str],
    clocks: &[&str],
) -> String {
    let mut names = Vec::new();
    collect_tree(root, root, &mut names).expect("output paths");
    let mut lines = Vec::new();
    for name in names {
        let path = root.join(&name);
        let label = name.to_str().expect("UTF-8 output path");
        let metadata = std::fs::symlink_metadata(&path).expect("output metadata");
        let line = if metadata.is_dir() {
            format!("{label}/ directory")
        } else if metadata.is_symlink() && !(sealed.contains(&label) && is_sealed_link(&path)) {
            format!(
                "{label} symlink {}",
                std::fs::read_link(&path).expect("link").display()
            )
        } else {
            assert!(
                metadata.is_file() || metadata.is_symlink(),
                "unsupported output file kind: {label}"
            );
            if pinyin.contains(&label) {
                format!(
                    "{label} rows {}",
                    rows_digest(&path, true).expect("pinyin rows")
                )
            } else if dbm.contains(&label) {
                format!(
                    "{label} rows {}",
                    rows_digest(&path, false).expect("DBM rows")
                )
            } else {
                let mut data = std::fs::read(&path).expect("output bytes");
                if inputs.files.get(label).is_some_and(|input| *input == data) {
                    format!("{label} unchanged")
                } else {
                    if clocks.contains(&label) {
                        mask_tkrzw_timestamp(&mut data);
                    }
                    format!("{label} sha256 {} {}", sha256(&data), data.len())
                }
            }
        };
        lines.push(line);
    }
    lines.sort_unstable();
    lines.iter().map(|line| format!("{line}\n")).collect()
}
