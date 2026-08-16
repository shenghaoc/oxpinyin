//! Read-only iteration over Tkrzw HashDBM files via the C++ bridge.
//!
//! The bridge (`bridge.cpp`) exposes open/iterate/close with C linkage;
//! records come back as raw key and value bytes, copied before the
//! iterator advances. Keys and values are never interpreted here.

use std::ffi::{CStr, c_char, c_int};

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

/// A Tkrzw HashDBM opened read-only through the bridge.
pub struct TkrzwReader {
    db: *mut TkrzwDB,
}

impl TkrzwReader {
    /// Opens `path` read-only.
    pub fn open(path: &CStr) -> Result<Self, String> {
        // SAFETY: `path` is a live NUL-terminated C string for the duration of
        // the call. `tkrzw_open` returns an owned handle or null; null is
        // checked below.
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

    /// Copies every record out as `(key, value)` byte pairs.
    #[allow(clippy::type_complexity)]
    pub fn entries(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
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
