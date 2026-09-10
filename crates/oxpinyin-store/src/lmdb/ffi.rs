//! Raw bindgen declarations for the **system** LMDB C API (`lmdb.h`),
//! plus the few conversions that turn its calling convention into Rust.
//!
//! Generated at build time by `build.rs` from the installed header —
//! nothing generated is committed — and allowlisted down to exactly the
//! entry points `super` calls. The declarations are raw and unsafe by
//! design; every invariant that makes calling them sound is documented
//! at the call site that relies on it, not here.
//!
//! # Self-contained on purpose
//!
//! `benches/lmdb_bulk_load.rs` pulls this file in with `#[path]` so the
//! bench drives the same declarations the backend does instead of
//! keeping a second copy of them. That only works while nothing here
//! names `crate::` — a `#[path]` include resolves `crate` to whichever
//! crate did the including. Keep this module free of `StoreError` and
//! of everything else that lives in the library root.
//!
//! # What crosses the ABI
//!
//! Unlike Kyoto Cabinet's opaque one-pointer handles, LMDB's ABI
//! exposes layout. `MDB_val` is `{ size_t mv_size; void *mv_data; }`
//! and carries every key and value in both directions; `MDB_stat`'s six
//! integer fields are what `is_empty` reads. Both are generated from
//! the same header as the linked library, so the layouts agree by
//! construction rather than by assumption.
//!
//! Data is **borrowed both ways**. An `MDB_val` a read hands back points
//! into the transaction's memory map and stays valid only while that
//! transaction lives, so anything outliving the call is copied. An
//! `MDB_val` handed to `mdb_put` is read during the call and copied by
//! LMDB, so a caller-owned slice is enough.
//!
//! # Result codes
//!
//! Every entry point returns `int`: `MDB_SUCCESS` (0) on success, a
//! **negative** `MDB_*` constant for LMDB's own failures, and a
//! **positive** `errno` for system failures. [`Code`] is that
//! three-way split, and it is why the backend can keep classifying
//! I/O failures as `StoreError::Io` without matching on error text.
#![expect(unsafe_code, reason = "raw FFI declarations; see the module docs")]
#![allow(dead_code, missing_docs, non_camel_case_types)]
#![allow(non_snake_case, non_upper_case_globals)]

include!(concat!(env!("OUT_DIR"), "/lmdb_bindings.rs"));

use std::ffi::CStr;

/// An `MDB_val` borrowing `bytes` for the duration of one call.
///
/// LMDB never retains the pointer past the call it is passed to: a key
/// or value handed to `mdb_put`/`mdb_get`/`mdb_del` is read (and copied,
/// for `put`) before the call returns. The lifetime is not expressible
/// in the generated struct, so it is the caller's obligation to keep
/// `bytes` alive across the call — every call site here does, by passing
/// a borrow that outlives the statement.
#[must_use]
pub fn val(bytes: &[u8]) -> MDB_val {
    MDB_val {
        mv_size: bytes.len(),
        // A zero-length slice still has a non-null dangling pointer,
        // which LMDB accepts for a zero-size value. The empty *key* is
        // refused higher up (MDB_BAD_VALSIZE), so this only ever carries
        // an empty value.
        mv_data: bytes.as_ptr().cast::<std::ffi::c_void>().cast_mut(),
    }
}

/// An all-zero `MDB_val`, for the out-parameters LMDB fills in.
#[must_use]
pub fn empty_val() -> MDB_val {
    MDB_val {
        mv_size: 0,
        mv_data: std::ptr::null_mut(),
    }
}

/// Borrows the bytes an `MDB_val` LMDB filled in points at.
///
/// # Safety
///
/// `v` must have been written by a successful LMDB read on a
/// transaction that is still live, and the returned slice must not
/// outlive that transaction: the memory belongs to the environment's
/// map, not to the caller. A zero-length result is returned as an empty
/// slice without dereferencing the pointer, which LMDB may leave null.
#[must_use]
pub unsafe fn as_slice<'a>(v: &MDB_val) -> &'a [u8] {
    if v.mv_size == 0 {
        return &[];
    }
    // SAFETY: the caller's contract above — a live transaction's record
    // memory, with `mv_size` its true length.
    unsafe { std::slice::from_raw_parts(v.mv_data.cast::<u8>(), v.mv_size) }
}

/// The three-way meaning of an LMDB result code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// `MDB_SUCCESS`.
    Success,
    /// A negative `MDB_*` constant — LMDB's own failure taxonomy.
    Mdb(i32),
    /// A positive value: a plain `errno` from the C library.
    Errno(i32),
}

/// Classifies a raw result code.
#[must_use]
pub fn classify(rc: i32) -> Code {
    match rc {
        0 => Code::Success,
        // LMDB's own codes occupy a negative block starting at
        // MDB_KEYEXIST (-30799); anything positive came from the system.
        rc if rc < 0 => Code::Mdb(rc),
        rc => Code::Errno(rc),
    }
}

/// LMDB's own message for a result code.
///
/// `mdb_strerror` returns a pointer to a static string for its own
/// codes and delegates to `strerror` for `errno` values; neither
/// allocates and neither is ours to free.
#[must_use]
pub fn strerror(rc: i32) -> String {
    // SAFETY: `mdb_strerror` is total over `int` — it answers with a
    // static string for its own codes and with `strerror`'s buffer
    // otherwise — and always returns a valid NUL-terminated pointer.
    let ptr = unsafe { mdb_strerror(rc) };
    if ptr.is_null() {
        return format!("LMDB error {rc}");
    }
    // SAFETY: non-null and NUL-terminated per `mdb_strerror`'s contract;
    // copied out immediately, so a `strerror` buffer a later call could
    // overwrite is not retained.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}
