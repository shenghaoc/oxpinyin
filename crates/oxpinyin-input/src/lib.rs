//! oxpinyin's external-I/O boundary (`docs/findings/external-io-boundary.md`):
//! the library and tool calls that cross the program's edge, made with the
//! pin's own calls through a small C shim (maintainer ruling 2026-10-28 UTC).
//!
//! Stage 1 holds the `table.conf` and `user.conf` readers and writer and their
//! `LC_NUMERIC` calls ([`config`]), and the init's and the save's lines that
//! share a failure sequence with those, so one sequence writes to one stream
//! ([`report_load_failed`], [`report_mmap_failed`], [`report_rename_failed`]).
//! What the scanned values mean — the
//! `strcmp` mappings, the slots, the refusals where the pin aborts — stays
//! with the crates that own those records; this crate answers only what each
//! C call answered.
//!
//! Unsafe code is confined to one module; every block carries a `// SAFETY:`
//! comment and no raw pointer leaves it.

#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

pub mod config;
#[allow(unsafe_code)]
mod native;

use std::ffi::{CStr, CString};
use std::path::Path;

/// `path` as the C string `fopen` receives: its bytes as the platform
/// encodes them, or `None` when they hold a NUL, which no C path can.
#[must_use]
pub fn c_path(path: &Path) -> Option<CString> {
    CString::new(path.as_os_str().as_encoded_bytes()).ok()
}

/// A save's failed rename, with the pin's `fprintf(stderr, "rename %s to %s
/// failed.\n", …)` (074a2219 `pinyin.cpp:1061`, `:1080`, `:1096`, `:1110`,
/// `:1123`; `zhuyin.cpp:611`, `:637`, `:657`, `:674`, `:689`). The save's
/// `mark_version` then writes its `write %s failed.` line
/// ([`config::save_user_table_info`]) on the same stream.
pub fn report_rename_failed(from: &CStr, to: &CStr) {
    native::report_rename_failed(from, to);
}

/// The init's `load %s failed!` for a `table.conf` that
/// `SystemTableInfo2::load` refused, with the pin's `fprintf(stderr, …)`
/// (074a2219 `pinyin.cpp:338`, `zhuyin.cpp:282`), after the load's own
/// `open %s failed.` line ([`config::SystemTableInfoReader::open`]) when the
/// file did not open.
pub fn report_load_failed(filename: &CStr) {
    native::report_load_failed(filename);
}

/// `mmap %s failed!` for a library the init's library loop (074a2219
/// `pinyin.cpp:256`, `:290`; `zhuyin.cpp:200`, `:234`) or the save's
/// `_write_files` (`pinyin.cpp:956`, `zhuyin.cpp:589`) cannot map, with the
/// pin's `fprintf(stderr, …)`: the stream `check_format`'s and
/// `mark_version`'s lines in the same call use.
pub fn report_mmap_failed(filename: &CStr) {
    native::report_mmap_failed(filename);
}
