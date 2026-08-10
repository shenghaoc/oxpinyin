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

#[cfg(feature = "oracle-ffi")]
unsafe extern "C" {
    fn pinyin_init(sys: *const c_char, user: *const c_char) -> *mut std::os::raw::c_void;
    fn pinyin_fini(ctx: *mut std::os::raw::c_void);
    fn pinyin_set_options(ctx: *mut std::os::raw::c_void, opts: u32) -> bool;
    fn pinyin_alloc_instance(ctx: *mut std::os::raw::c_void) -> *mut std::os::raw::c_void;
    fn pinyin_free_instance(inst: *mut std::os::raw::c_void);
    fn pinyin_token_get_phrase(
        inst: *mut std::os::raw::c_void,
        token: u32,
        len: *mut u32,
        s: *mut *mut c_char,
    ) -> bool;
    fn g_free(p: *mut std::os::raw::c_void);
}

// ── safe wrapper ──────────────────────────────────────────────────────

struct TkrzwReader {
    db: *mut TkrzwDB,
}

impl TkrzwReader {
    fn open(path: &CStr) -> Result<Self, String> {
        // SAFETY: `path` is a live NUL-terminated C string for the duration of
        // the call. `tkrzw_open` returns an owned handle or null; null is
        // checked below before any use.
        let db = unsafe { tkrzw_open(path.as_ptr()) };
        if db.is_null() {
            return Err("tkrzw_open returned null".into());
        }
        // SAFETY: `db` is non-null (checked above). `tkrzw_error` returns a
        // borrowed NUL-terminated string owned by the bridge that stays valid
        // until the next bridge call on `db`; it is copied before any such call.
        let err = unsafe { CStr::from_ptr(tkrzw_error(db)) };
        let err_str = err.to_string_lossy();
        if !err_str.is_empty() {
            // SAFETY: `db` is non-null and owned here; closed exactly once on
            // this early-return path, and `Self` is never constructed, so
            // `Drop` cannot close it a second time.
            unsafe { tkrzw_close(db) };
            return Err(format!("failed to open: {err_str}"));
        }
        Ok(Self { db })
    }

    #[allow(clippy::type_complexity)]
    fn entries(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
        let mut result = Vec::new();
        // SAFETY: `self.db` is non-null for the lifetime of `Self` (checked in
        // `open`). The returned iterator is owned; null is checked below.
        let it = unsafe { tkrzw_iter_make(self.db) };
        if it.is_null() {
            return Err("failed to create iterator".into());
        }
        // SAFETY: `it` is non-null (checked above); returns a plain int status.
        let mut ok = unsafe { tkrzw_iter_first(it) };
        while ok != 0 {
            let mut klen: usize = 0;
            let mut vlen: usize = 0;
            // SAFETY: `it` is non-null and positioned on a record (`ok != 0`).
            // The out-pointers are valid, aligned `usize` locations. The
            // returned buffers are owned by the iterator and stay valid until
            // the next iterator call; they are copied before that below.
            let kptr = unsafe { tkrzw_iter_key(it, &mut klen) };
            // SAFETY: as for `tkrzw_iter_key` above.
            let vptr = unsafe { tkrzw_iter_value(it, &mut vlen) };
            if klen > 0 {
                // SAFETY: `kptr` points to at least `klen` readable bytes per
                // the bridge contract (`klen > 0` implies a live buffer); the
                // slice is copied to an owned Vec before the iterator advances.
                let key = unsafe { std::slice::from_raw_parts(kptr, klen) }.to_vec();
                // SAFETY: `vptr`/`vlen` come from the same positioned iterator
                // and follow the same borrow-then-copy contract as the key.
                let val = unsafe { std::slice::from_raw_parts(vptr, vlen) }.to_vec();
                result.push((key, val));
            }
            // SAFETY: `it` is non-null; invalidates the buffers borrowed above,
            // which have already been copied. Returns a plain int status.
            ok = unsafe { tkrzw_iter_next(it) };
        }
        // SAFETY: `it` is non-null and owned; freed exactly once, and never
        // used after this call.
        unsafe { tkrzw_iter_free(it) };
        Ok(result)
    }
}

impl Drop for TkrzwReader {
    fn drop(&mut self) {
        // SAFETY: `self.db` is non-null (invariant established in `open`, which
        // is the only constructor) and owned solely by `self`; closed exactly
        // once because `drop` runs once and the early-close path in `open`
        // never constructs `Self`.
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

    let path_c = std::ffi::CString::new(input.to_string_lossy().as_bytes())
        .map_err(|_| "input path contains null byte")?;
    let reader = TkrzwReader::open(&path_c)?;

    let mut entries = reader.entries()?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Fix for phrase_index: redb must be keyed by 4-byte LE token, not 6-byte table key.
    let input_str = input.to_string_lossy();
    let is_phrase_index = input_str.contains("phrase_index");
    if is_phrase_index {
        let data_dir = input.parent().unwrap_or_else(|| std::path::Path::new("."));
        let _pinyin_index_path = data_dir.join("pinyin_index.bin");
        let mut new_entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        #[cfg(feature = "oracle-ffi")]
        {
            let mut all_tokens = std::collections::HashSet::new();
            let pinyin_c =
                std::ffi::CString::new(_pinyin_index_path.to_string_lossy().as_bytes()).ok();
            if let Some(pc) = pinyin_c {
                if let Ok(p_reader) = TkrzwReader::open(&pc) {
                    if let Ok(p_entries) = p_reader.entries() {
                        for (_, v) in p_entries {
                            if v.len() < 4 {
                                continue;
                            }
                            let words: Vec<u32> = v
                                .chunks_exact(4)
                                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                                .collect();
                            let start = if words.first() == Some(&0) { 1 } else { 0 };
                            for &tok in &words[start..] {
                                if tok != 0 {
                                    all_tokens.insert(tok);
                                }
                            }
                        }
                    }
                }
            }
            for entry in
                std::fs::read_dir(&data_dir).unwrap_or_else(|_| std::fs::read_dir(".").unwrap())
            {
                let path = entry.unwrap().path();
                if path.extension().and_then(|s| s.to_str()) != Some("bin") {
                    continue;
                }
                let fname = path.file_name().unwrap().to_string_lossy();
                if fname == "pinyin_index.bin"
                    || fname == "phrase_index.bin"
                    || fname == "addon_pinyin_index.bin"
                    || fname == "addon_phrase_index.bin"
                    || fname == "punct.bin"
                    || fname == "bigram.db"
                {
                    continue;
                }
                if let Ok(data) = std::fs::read(&path) {
                    if let Ok(table) = pinyin_data::content::ContentTable::load(&data) {
                        for rec in table.iter() {
                            for pair in &rec.tokens {
                                all_tokens.insert(pair.token);
                            }
                        }
                    }
                }
            }
            let sys_dir = data_dir.to_string_lossy().to_string();
            let tmp_user =
                std::env::temp_dir().join(format!("pinyin-migrate-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&tmp_user);
            let sys_c = std::ffi::CString::new(sys_dir.as_bytes()).ok();
            let user_c = std::ffi::CString::new(tmp_user.to_string_lossy().as_bytes()).ok();
            if let (Some(sc), Some(uc)) = (sys_c, user_c) {
                // SAFETY: `sc` and `uc` are live NUL-terminated C strings for
                // the whole block. `pinyin_init` returns an owned context or
                // null, checked before use; the instance is likewise
                // null-checked. `pinyin_token_get_phrase` writes a borrowed
                // pointer only on success, `CStr::from_ptr` reads it while the
                // instance is not otherwise mutated, and the string is copied
                // before `g_free` releases the GLib allocation exactly once.
                // Instance and context are freed exactly once each, in that
                // order, before the block ends, and no pointer escapes it.
                unsafe {
                    let ctx = pinyin_init(sc.as_ptr(), uc.as_ptr());
                    if !ctx.is_null() {
                        pinyin_set_options(ctx, 0x18a);
                        let inst = pinyin_alloc_instance(ctx);
                        if !inst.is_null() {
                            for &tok in &all_tokens {
                                let mut len: u32 = 0;
                                let mut cstr: *mut c_char = std::ptr::null_mut();
                                let ok = pinyin_token_get_phrase(
                                    inst,
                                    tok,
                                    &mut len as *mut _,
                                    &mut cstr as *mut _,
                                );
                                if ok && !cstr.is_null() {
                                    let cstr_ref = std::ffi::CStr::from_ptr(cstr);
                                    if let Ok(s) = cstr_ref.to_str() {
                                        let key = tok.to_le_bytes().to_vec();
                                        let val = s.as_bytes().to_vec();
                                        new_entries.push((key, val));
                                    }
                                    g_free(cstr as *mut _);
                                }
                            }
                            pinyin_free_instance(inst);
                        }
                        pinyin_fini(ctx);
                    }
                }
                let _ = std::fs::remove_dir_all(&tmp_user);
            }
        }
        if !new_entries.is_empty() {
            new_entries.sort_by(|a, b| a.0.cmp(&b.0));
            new_entries.dedup_by(|a, b| a.0 == b.0);
            entries = new_entries;
        } else {
            let mut fallback: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
            for (k, v) in &entries {
                if k.len() == 6 {
                    let token_be = u32::from_be_bytes([k[2], k[3], k[4], k[5]]);
                    if token_be > 0
                        && token_be < 1000
                        && v.len() < 100
                        && std::str::from_utf8(v).is_ok()
                    {
                        let key4 = token_be.to_le_bytes().to_vec();
                        fallback.push((key4, v.clone()));
                    }
                }
            }
            if !fallback.is_empty() {
                entries = fallback;
            }
        }
    }

    if let Some(n) = limit {
        entries.truncate(n);
    }

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
