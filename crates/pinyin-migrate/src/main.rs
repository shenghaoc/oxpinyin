//! Converter: reads oracle Tkrzw HashDB files via FFI and writes redb tables.
//!
//! Usage:
//!   pinyin-migrate <input.tkh> -o <output.redb>
//!
//! The input is a Tkrzw HashDB file (e.g. pinyin_index.bin).
//! The output is a redb database with a single `data` table mapping
//! raw key bytes → raw value bytes.

#![warn(missing_docs)]

use std::ffi::{CStr, c_char, c_int};
use std::fs;
use std::path::PathBuf;

// ── FFI declarations ──────────────────────────────────────────────────

#[repr(C)]
struct TkrzwDB {
    _private: [u8; 0],
}

#[repr(C)]
struct TkrzwIter {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn tkrzw_open(path: *const c_char) -> *mut TkrzwDB;
    fn tkrzw_close(db: *mut TkrzwDB);
    fn tkrzw_error(db: *const TkrzwDB) -> *const c_char;

    fn tkrzw_iter_make(db: *mut TkrzwDB) -> *mut TkrzwIter;
    fn tkrzw_iter_free(it: *mut TkrzwIter);
    fn tkrzw_iter_first(it: *mut TkrzwIter) -> c_int;
    fn tkrzw_iter_next(it: *mut TkrzwIter) -> c_int;
    fn tkrzw_iter_key(it: *const TkrzwIter, len: *mut usize) -> *const u8;
    fn tkrzw_iter_value(it: *const TkrzwIter, len: *mut usize) -> *const u8;
}

// ── safe wrapper ──────────────────────────────────────────────────────

struct TkrzwReader {
    db: *mut TkrzwDB,
}

impl TkrzwReader {
    /// Open a Tkrzw HashDB file for reading.
    ///
    /// # Safety
    ///
    /// `path` must point to a valid Tkrzw HashDB file readable by libtkrzw.
    fn open(path: &CStr) -> Result<Self, String> {
        // SAFETY: path is a valid null-terminated C string.
        let db = unsafe { tkrzw_open(path.as_ptr()) };
        if db.is_null() {
            return Err("tkrzw_open returned null".into());
        }
        // SAFETY: db is non-null and was just created.
        let err = unsafe { CStr::from_ptr(tkrzw_error(db)) };
        let err_str = err.to_string_lossy();
        if !err_str.is_empty() {
            // SAFETY: db is valid.
            unsafe { tkrzw_close(db) };
            return Err(format!("failed to open: {err_str}"));
        }
        Ok(Self { db })
    }

    /// Iterate over all (key, value) pairs.
    #[allow(clippy::type_complexity)]
    fn entries(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
        let mut result = Vec::new();

        // SAFETY: db is valid and open.
        let it = unsafe { tkrzw_iter_make(self.db) };
        if it.is_null() {
            return Err("failed to create iterator".into());
        }

        // SAFETY: it is non-null.
        let mut ok = unsafe { tkrzw_iter_first(it) };
        while ok != 0 {
            let mut klen: usize = 0;
            let mut vlen: usize = 0;

            // SAFETY: it is valid, on a record.
            let kptr = unsafe { tkrzw_iter_key(it, &mut klen) };
            let vptr = unsafe { tkrzw_iter_value(it, &mut vlen) };

            if klen > 0 {
                // SAFETY: kptr and vptr point to valid buffers of klen/vlen bytes.
                let key = unsafe { std::slice::from_raw_parts(kptr, klen) }.to_vec();
                let val = unsafe { std::slice::from_raw_parts(vptr, vlen) }.to_vec();
                result.push((key, val));
            }

            // SAFETY: it is valid.
            ok = unsafe { tkrzw_iter_next(it) };
        }

        // SAFETY: it is valid.
        unsafe { tkrzw_iter_free(it) };
        Ok(result)
    }
}

impl Drop for TkrzwReader {
    fn drop(&mut self) {
        // SAFETY: db was created by tkrzw_open and is valid.
        unsafe { tkrzw_close(self.db) };
    }
}

// ── main ──────────────────────────────────────────────────────────────

const REDB_TABLE: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("data");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut limit: Option<usize> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                if i < args.len() {
                    output = Some(PathBuf::from(&args[i]));
                }
            }
            "-n" => {
                i += 1;
                if i < args.len() {
                    limit = Some(args[i].parse().map_err(|_| "invalid -n value")?);
                }
            }
            arg if !arg.starts_with('-') => {
                input = Some(PathBuf::from(arg));
            }
            _ => {
                eprintln!("Usage: pinyin-migrate <input.tkh> [-o <output.redb>] [-n <limit>]");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let input = input.ok_or_else(|| {
        eprintln!("Usage: pinyin-migrate <input.tkh> [-o <output.redb>] [-n <limit>]");
        "missing input path"
    })?;

    let output = output.unwrap_or_else(|| {
        let mut p = input.clone();
        p.set_extension("redb");
        p
    });

    // Read Tkrzw.
    let path_c = std::ffi::CString::new(input.to_string_lossy().as_bytes())
        .map_err(|_| "input path contains null byte")?;
    let reader = TkrzwReader::open(&path_c)?;

    let mut entries = reader.entries()?;
    // Sort by key for deterministic output.
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Apply record limit (for generating mini fixtures).
    if let Some(n) = limit {
        entries.truncate(n);
    }

    // Write redb.
    if output.exists() {
        fs::remove_file(&output)?;
    }

    let db = redb::Database::create(&output)?;
    {
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(REDB_TABLE)?;
            for (key, value) in &entries {
                table.insert(key.as_slice(), value.as_slice())?;
            }
        }
        txn.commit()?;
    }

    let out_size = fs::metadata(&output)?.len();
    eprintln!(
        "Wrote {} records ({out_size} bytes) → {}",
        entries.len(),
        output.display()
    );
    Ok(())
}
