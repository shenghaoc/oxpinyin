//! The cxx bridge to `shim.cc` — the crate's only FFI surface.
//!
//! `#[cxx::bridge]` expands to `unsafe extern "C"` declarations and the
//! `unsafe` blocks that call them, which the workspace's
//! `unsafe_code = "deny"` would otherwise reject. The allow is scoped to
//! the tkrzw backend's modules and nothing else: `super` adds a
//! hand-written pair of its own below (the callback token derefs), under
//! the backend's sanctioned safety waiver — waived safety is not waived
//! correctness, and the invariants are documented at each `unsafe`.
//!
//! The read path borrows tkrzw's record memory across this bridge:
//! `db_get` and `db_scan` take an `fn` callback plus an opaque `usize`
//! token, and the shim hands each record's key and value to the callback
//! as raw (pointer, length) word pairs — assembled into `&[u8]` on the
//! Rust side, valid only for the callback's duration. Raw words rather
//! than `rust::Slice` because every Slice construction is an out-of-line
//! call into the cxx runtime, which is measurable on a per-record path.
//! The token is a pointer the Rust caller keeps alive for the whole
//! call — see `super`'s `scan_row` and `get_value` for the discipline.
//!
//! # Safety
//!
//! `db_apply`, `db_synchronize` and `db_rebuild` mutate through a shared
//! reference. That is sound because tkrzw documents every TreeDBM
//! operation as thread-safe — the database does its own locking — which
//! is also what lets the backend keep `get` and `write` on `&self` like
//! its redb and LMDB peers.
#![allow(unsafe_code)]

#[cxx::bridge(namespace = "oxpinyin_tkrzw")]
pub(crate) mod ffi {
    /// One tkrzw `Status`: its code and its message.
    ///
    /// The codes are `tkrzw::Status::Code`; only `SUCCESS` and
    /// `SYSTEM_ERROR` are named on the Rust side, and everything else is
    /// reported verbatim.
    struct ShimStatus {
        code: i32,
        message: String,
    }

    /// One buffered write, applied as part of a `ProcessMulti` batch.
    ///
    /// `remove` picks between writing `value` and deleting the record;
    /// `value` is ignored when `remove` is set.
    struct Mutation {
        key: Vec<u8>,
        value: Vec<u8>,
        remove: bool,
    }

    unsafe extern "C++" {
        include!("oxpinyin-store/src/tkrzw/shim.h");

        /// An open TreeDBM, closed when the handle drops.
        type Db;

        fn open_db(
            path: &[u8],
            writable: bool,
            no_create: bool,
            status: &mut ShimStatus,
        ) -> UniquePtr<Db>;

        /// Reads one record, invoking `visit` once as
        /// `visit(ctx, value_ptr, value_len)` over the borrowed value —
        /// or not at all when the key is absent. The pointers are
        /// tkrzw's record memory, valid only for the call.
        fn db_get(
            db: &Db,
            key: &[u8],
            visit: unsafe fn(usize, *const u8, usize),
            ctx: usize,
        ) -> ShimStatus;

        /// Walks records from the lower-bound `start` in ascending key
        /// order, invoking `visit` per record as
        /// `visit(ctx, key_ptr, key_len, value_ptr, value_len)` over the
        /// borrowed key and value; `false` from `visit` stops the walk.
        /// The pointers are tkrzw's record memory, valid only for each
        /// call.
        fn db_scan(
            db: &Db,
            start: &[u8],
            visit: unsafe fn(usize, *const u8, usize, *const u8, usize) -> bool,
            ctx: usize,
        ) -> ShimStatus;

        fn db_apply(db: &Db, mutations: &[Mutation]) -> ShimStatus;

        fn db_synchronize(db: &Db, hard: bool) -> ShimStatus;

        fn db_rebuild(db: &Db) -> ShimStatus;
    }
}

// SAFETY: `Db` wraps a TreeDBM, and tkrzw documents every DBM operation
// as thread-safe — the database carries its own locking — so a handle
// can be moved between threads and shared by reference (which is also
// what lets the store keep `get` and `write` on `&self`). `Iter`
// deliberately keeps neither marker: it is a raw cursor over one `Db`
// that tkrzw does not synchronise, and its lifetime is tied to the
// method that made it, per the module-level safety notes above.
unsafe impl Send for ffi::Db {}
unsafe impl Sync for ffi::Db {}
