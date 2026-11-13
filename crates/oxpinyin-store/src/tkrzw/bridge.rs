//! The cxx bridge over tkrzw's classes, declared here and generated at
//! build time by `build.rs` (`cxx_build::bridge`); nothing generated is
//! committed.
//!
//! The bridge's C++ side is `adapter.h`/`adapter.cc`, the semantic adapter
//! the Rust/C++ interoperability ruling permits
//! (`docs/findings/compatibility-policy.md#rust-cpp-interoperability`):
//! the concrete constructors, byte-view and status conversion, one
//! `RecordProcessor` subclass and
//! the shared-reference mutation the classes' locking permits. Every
//! function in `unsafe extern "C++"` is declared safe to call; `super`
//! names the claim each one rests on (safety point 1 of the ruling), and
//! this file states what the generated glue itself guarantees.
//!
//! # What cxx checks
//!
//! Each signature below is checked against the adapter's declaration when
//! the C++ side compiles: a `&DBM` is a `const tkrzw::DBM&`, a
//! `Pin<&mut DBM>` a `tkrzw::DBM&`, a `&[u8]` a `rust::Slice<const
//! uint8_t>`, a `Pin<&mut CxxString>` a `std::string&`, and a
//! `UniquePtr<T>` a `std::unique_ptr<T>` whose destruction runs the C++
//! destructor — `DBM`'s is virtual (`tkrzw_dbm.h:987`), so the concrete
//! object behind it is destroyed. A borrowed slice crosses as a pointer
//! and a length together; the adapter never retains one.
//!
//! # Status and exceptions
//!
//! No function returns `Result`. Every native status comes back as
//! [`ffi::NativeStatus`], tkrzw's own code and message, for the caller to
//! check, ignore or map as the pin does at the corresponding site. The pin
//! catches no exception around any DBM call (its `check_result` is an
//! assertion, `pinyin_utils.h:27-31`; tkrzw's `xmalloc` family throws
//! `std::bad_alloc`, `tkrzw_lib_common.h:147-183`), so neither does the
//! bridge: an exception reaches the generated `noexcept` shim and
//! terminates the process — the abnormal termination constitution §4
//! prescribes where the pin aborts.
//!
//! # Sentinels
//!
//! The library recognises `RecordProcessor::NOOP` and `REMOVE` by the
//! address of their backing arrays (`tkrzw_dbm.cc:26-37`). Only the
//! adapter's own processor returns one, and it returns the header's
//! `NOOP` object itself (`adapter.cc`, `KeyCollector`); nothing on the
//! Rust side holds, copies or compares a sentinel.
#![expect(
    unsafe_code,
    reason = "the cxx bridge declares the adapter's functions safe to call; the claims are in the backend module"
)]

#[cxx::bridge(namespace = "oxpinyin::tkrzw")]
pub(super) mod ffi {
    /// One `tkrzw::Status`: its code (`Status::Code`, `tkrzw_lib_common.h:
    /// 301-317`) and its message when the library attached one, null
    /// otherwise, so a success allocates nothing.
    struct NativeStatus {
        code: i32,
        message: UniquePtr<CxxString>,
    }

    unsafe extern "C++" {
        include!("oxpinyin-store/src/tkrzw/adapter.h");

        /// `tkrzw::DBM`, the base of the four concrete classes the pin
        /// allocates; every operation below is one of its virtual methods,
        /// as the pin's `m_db->` calls are.
        #[namespace = "tkrzw"]
        type DBM;
        /// `tkrzw::DBM::Iterator`, under the alias `adapter.h` declares.
        type DbmIterator;

        fn new_tree_dbm() -> UniquePtr<DBM>;
        fn new_hash_dbm() -> UniquePtr<DBM>;
        fn new_baby_dbm() -> UniquePtr<DBM>;
        fn new_tiny_dbm() -> UniquePtr<DBM>;

        fn open(dbm: Pin<&mut DBM>, path: &[u8], writable: bool, options: i32) -> NativeStatus;
        fn close(dbm: Pin<&mut DBM>) -> NativeStatus;
        fn is_open(dbm: &DBM) -> bool;

        fn get(dbm: &DBM, key: &[u8], value: Pin<&mut CxxString>) -> NativeStatus;
        fn check(dbm: &DBM, key: &[u8]) -> NativeStatus;
        fn set(dbm: &DBM, key: &[u8], value: &[u8]) -> NativeStatus;
        fn remove(dbm: &DBM, key: &[u8]) -> NativeStatus;
        fn synchronize(dbm: &DBM, hard: bool) -> NativeStatus;
        fn count(dbm: &DBM, count: &mut i64) -> NativeStatus;
        fn collect_keys(dbm: &DBM, keys: Pin<&mut CxxVector<CxxString>>) -> NativeStatus;

        fn make_iterator(dbm: &DBM) -> UniquePtr<DbmIterator>;
        fn iter_first(iter: Pin<&mut DbmIterator>) -> NativeStatus;
        fn iter_jump(iter: Pin<&mut DbmIterator>, key: &[u8]) -> NativeStatus;
        fn iter_next(iter: Pin<&mut DbmIterator>) -> NativeStatus;
        fn iter_get(
            iter: Pin<&mut DbmIterator>,
            key: Pin<&mut CxxString>,
            value: Pin<&mut CxxString>,
        ) -> NativeStatus;
    }
}
