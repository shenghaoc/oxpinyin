//! The library's `fprintf(stderr, …)` lines outside the `table.conf` and
//! `user.conf` reader and writer (whose lines are [`crate::config`]'s), each
//! made with the pin's own call (stage 4 of
//! `docs/findings/external-io-boundary.md`). Every library line therefore
//! goes through the host C library's `stderr` stream, as the pin's do, in the
//! order the callers make them.

use std::ffi::CStr;

use crate::native;

/// A save's failed rename, with the pin's `fprintf(stderr, "rename %s to %s
/// failed.\n", …)` (074a2219 `pinyin.cpp:1061`, `:1080`, `:1096`, `:1110`,
/// `:1123`; `zhuyin.cpp:611`, `:637`, `:657`, `:674`, `:689`). The save's
/// `mark_version` then writes its `write %s failed.` line
/// ([`crate::config::save_user_table_info`]) on the same stream.
pub fn report_rename_failed(from: &CStr, to: &CStr) {
    native::report_rename_failed(from, to);
}

/// The init's `load %s failed!` for a `table.conf` that
/// `SystemTableInfo2::load` refused, with the pin's `fprintf(stderr, …)`
/// (074a2219 `pinyin.cpp:338`, `zhuyin.cpp:282`), after the load's own
/// `open %s failed.` line ([`crate::config::SystemTableInfoReader::open`])
/// when the file did not open.
pub fn report_load_failed(filename: &CStr) {
    native::report_load_failed(filename);
}

/// `mmap %s failed!` for a library `MemoryChunk::mmap` refused, with the
/// pin's `fprintf(stderr, …)`: in the init's library loop and the library
/// load calls (074a2219 `pinyin.cpp:256`, `:290`; `zhuyin.cpp:200`, `:234`),
/// the save's `_write_files` (`pinyin.cpp:956`, `zhuyin.cpp:589`) and
/// `mask_out` (`pinyin.cpp:1265`, `zhuyin.cpp:800`).
pub fn report_mmap_failed(filename: &CStr) {
    native::report_mmap_failed(filename);
}
