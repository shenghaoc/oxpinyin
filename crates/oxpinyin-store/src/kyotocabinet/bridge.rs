//! The cxx bridge over Kyoto Cabinet's classes, declared here and generated
//! at build time by `build.rs` (`cxx_build::bridge`); nothing generated is
//! committed.
//!
//! The bridge's C++ side is `adapter.h`/`adapter.cc`, the semantic adapter
//! the Rust/C++ interoperability ruling permits
//! (`docs/findings/compatibility-policy.md#rust-cpp-interoperability`):
//! the `Visitor` subclass, the empty-region pointer and the read of
//! `BasicDB::error()`, with the
//! constructors, the version string and shared-reference forwards cxx has no syntax
//! for. `open`, `close` and the cursor's `jump()` are bound directly as
//! members. Each claim a declaration rests on (safety point 1 of the
//! ruling) is stated in `super::ffi`, the one module that calls the
//! bridge.
//!
//! # What cxx checks
//!
//! Each signature below is checked against the adapter's declaration when
//! the C++ side compiles: a `&BasicDB` is a `const kyotocabinet::BasicDB&`,
//! a `Pin<&mut BasicDB>` a `kyotocabinet::BasicDB&`, a `&[u8]` a
//! `rust::Slice<const uint8_t>`, a `&CxxString` a `const std::string&`, and
//! a `UniquePtr<T>` a `std::unique_ptr<T>` whose destruction runs the C++
//! destructor — `BasicDB`'s is virtual (`kcdb.h:1172`), so the concrete
//! object behind it is destroyed. The member bindings are checked against
//! the member's own type (`kcdb.h:202,1208,1213`). The two enums are bound
//! to Kyoto Cabinet's own (`kcdb.h:921-932,1157-1160`): cxx asserts every
//! variant's value and the underlying type.
//!
//! [`sys::DbCursor`] carries the lifetime of the database it was made from,
//! so the borrow checker refuses a cursor that outlives its database (safety
//! point 3).
//!
//! # Status and exceptions
//!
//! No function returns `Result`. Every native answer comes back as the
//! library returns it — a `bool`, a size of -1 or a count — and the reason
//! for a failure is the thread's own `BasicDB::error()`: [`sys::error_code`]
//! reads its code alone, and [`sys::last_error`] copies its name and message
//! for a reported failure.
//! The pin catches no exception around any Kyoto Cabinet call, and Kyoto
//! Cabinet's allocation helpers throw `std::bad_alloc` (`kcutil.h:2863-2889`),
//! so neither does the bridge: an exception reaches the generated `noexcept`
//! function and terminates the process, as it ends the pin's. A Rust panic
//! inside [`Sink`]'s callback aborts at cxx's own unwind guard.
//!
//! # Sentinels
//!
//! Kyoto Cabinet recognises `Visitor::NOP` (0) and `Visitor::REMOVE` (1) by
//! address (`kcdb.h:44,46`). Only the adapter's visitor returns one, and it
//! returns `NOP` by name. An empty slice crosses as address 1, so the
//! adapter substitutes a static object's address for every empty region it
//! passes on (`adapter.cc`, `bytes`). Nothing on the Rust side holds,
//! copies or compares a sentinel.
#![expect(
    unsafe_code,
    reason = "the cxx bridge declares the adapter's functions; the claims are in the ffi module"
)]

use super::ffi::Sink;

#[cxx::bridge(namespace = "oxpinyin::kyotocabinet")]
pub(super) mod sys {
    /// `kyotocabinet::BasicDB::Error::Code` (`kcdb.h:921-932`).
    #[repr(u32)]
    enum ErrorCode {
        SUCCESS = 0,
        NOIMPL = 1,
        INVALID = 2,
        NOREPOS = 3,
        NOPERM = 4,
        BROKEN = 5,
        DUPREC = 6,
        NOREC = 7,
        LOGIC = 8,
        SYSTEM = 9,
        MISC = 15,
    }

    /// The `kyotocabinet::BasicDB::OpenMode` bits the pin passes
    /// (`kcdb.h:1157-1160`, `kyotodb_utils.h:32-45`).
    #[repr(u32)]
    enum OpenMode {
        OREADER = 1,
        OWRITER = 2,
        OCREATE = 4,
    }

    /// One `BasicDB::error()`: its code, the library's name for the code,
    /// and its message, copied in the call that reads them.
    struct NativeError {
        code: ErrorCode,
        name: String,
        message: String,
    }

    extern "Rust" {
        /// Where the adapter's visitor copies each record it visits. It
        /// borrows the caller's buffers for `'a`. C++ sees an opaque
        /// type, which it holds by reference for one call.
        type Sink<'a>;
        /// One record's key and value, valid for this call only.
        fn full(self: &mut Sink<'_>, key: &[u8], value: &[u8]);
    }

    extern "Rust" {
        type Rewriter<'a, 'f>;
        fn replacement(self: &mut Rewriter<'_, '_>, key: &[u8], value: &[u8]) -> Vec<u8>;
    }

    unsafe extern "C++" {
        include!("oxpinyin-store/src/kyotocabinet/adapter.h");

        /// `kyotocabinet::BasicDB`, the base of the four concrete classes
        /// the pin allocates; the pin's members have this static type.
        #[namespace = "kyotocabinet"]
        type BasicDB;
        /// `kyotocabinet::BasicDB::Cursor`, under the alias `adapter.h`
        /// declares, borrowing the database it was made from.
        type DbCursor<'db>;
        type ErrorCode;
        type OpenMode;

        /// `kyotocabinet::VERSION` (`kcutil.h:93`).
        fn version() -> String;

        fn new_tree_db() -> UniquePtr<BasicDB>;
        fn new_hash_db() -> UniquePtr<BasicDB>;
        fn new_stash_db() -> UniquePtr<BasicDB>;
        fn new_proto_tree_db() -> UniquePtr<BasicDB>;

        fn open(self: Pin<&mut BasicDB>, path: &CxxString, mode: u32) -> bool;
        fn close(self: Pin<&mut BasicDB>) -> bool;
        fn error_code(db: &BasicDB) -> ErrorCode;
        fn last_error(db: &BasicDB) -> NativeError;

        /// # Safety
        ///
        /// `key.len()` is at most `MEM_MAX_SIZE` (`ffi.rs`).
        unsafe fn check(db: &BasicDB, key: &[u8]) -> i32;
        /// # Safety
        ///
        /// `key.len()` is at most `MEM_MAX_SIZE` (`ffi.rs`).
        unsafe fn get(db: &BasicDB, key: &[u8], sink: &mut Sink<'_>) -> bool;
        /// # Safety
        ///
        /// `key.len()` and `value.len()` are at most `MEM_MAX_SIZE` (`ffi.rs`).
        unsafe fn set(db: &BasicDB, key: &[u8], value: &[u8]) -> bool;
        /// # Safety
        ///
        /// `key.len()` is at most `MEM_MAX_SIZE` (`ffi.rs`).
        unsafe fn remove(db: &BasicDB, key: &[u8]) -> bool;
        fn iterate(db: &BasicDB, sink: &mut Sink<'_>) -> bool;
        fn count(db: &BasicDB) -> i64;
        fn rewrite(db: &BasicDB, rewriter: &mut Rewriter<'_, '_>) -> bool;
        fn synchronize(db: &BasicDB, hard: bool) -> bool;
        fn begin_transaction(db: &BasicDB, hard: bool) -> bool;
        fn end_transaction(db: &BasicDB, commit: bool) -> bool;
        fn load_snapshot(db: &BasicDB, path: &CxxString) -> bool;
        fn dump_snapshot(db: &BasicDB, path: &CxxString) -> bool;

        fn make_cursor<'db>(db: &'db BasicDB) -> UniquePtr<DbCursor<'db>>;
        fn jump(self: Pin<&mut DbCursor<'_>>) -> bool;
        /// # Safety
        ///
        /// `key.len()` is at most `MEM_MAX_SIZE` (`ffi.rs`).
        unsafe fn jump_to(cursor: Pin<&mut DbCursor<'_>>, key: &[u8]) -> bool;
        fn cursor_accept(cursor: Pin<&mut DbCursor<'_>>, sink: &mut Sink<'_>, step: bool) -> bool;
    }
}

use crate::Rewriter;
