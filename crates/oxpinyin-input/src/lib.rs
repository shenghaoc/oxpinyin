//! oxpinyin's external-I/O boundary (`docs/findings/external-io-boundary.md`):
//! the library and tool calls that cross the program's edge, made with the
//! pin's own calls (maintainer ruling 2026-10-28 UTC), through the `libc`
//! crate and no C shim (ruling 2026-10-31 UTC).
//!
//! Stage 1 holds the `table.conf` and `user.conf` readers and writer and their
//! `LC_NUMERIC` calls ([`config`]); stage 4 the library's other `stderr` lines
//! ([`diagnostic`]), so every library line is written to the C library's
//! stream. What the scanned values mean — the
//! `strcmp` mappings, the slots, the refusals where the pin aborts — stays
//! with the crates that own those records; this crate answers only what each
//! C call answered.
//!
//! Stage 2 prepares the chunk open/seek/header-read calls and a single
//! whole-file private mapping owner ([`chunk`]). Production still uses the
//! data mapper until the live-library ownership transition; interpretation
//! and checksums stay in data. Chunk loading into the heap remains pending.
//!
//! Stage 3 holds the save path's file-system calls: the pin's
//! `g_build_filename` ([`path`]), the chunk open/write/sync/close
//! ([`chunk`]), and `g_rename` and plain `unlink` (also [`path`]).
//!
//! Stage 5's tool boundary (`tool`, the `tools` feature) holds the pin
//! utilities' locale, GLib option, input-stream and text calls, through the
//! `libc` crate, `glib-sys` and `std::io` without a C or C++ shim
//! (maintainer ruling 2026-10-31 UTC); the shipping libraries never enable
//! it.
//!
//! Unsafe code is confined to the native module and `tool`; every block
//! carries a `// SAFETY:` comment and no raw pointer leaves them.

#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

pub mod chunk;
pub mod config;
pub mod diagnostic;
#[expect(
    unsafe_code,
    reason = "the library's libc and glib-sys calls behind owned handles; every block carries a SAFETY comment"
)]
mod native;
pub mod path;
#[cfg(feature = "tools")]
#[expect(
    unsafe_code,
    reason = "the pin utilities' libc and glib-sys calls; every block carries a SAFETY comment"
)]
pub mod tool;

use std::ffi::CString;
use std::path::Path;

/// The newline a native text-mode `fopen("w")` writes: CRLF on Windows,
/// LF on Unix. The pin's `user.conf` and generated text tables inherit it.
pub const TEXT_NEWLINE: &str = if cfg!(windows) { "\r\n" } else { "\n" };

/// `path` as the C string `fopen` receives: its bytes as the platform
/// encodes them, or `None` when they hold a NUL, which no C path can.
#[must_use]
pub fn c_path(path: &Path) -> Option<CString> {
    CString::new(path::path_to_bytes(path)).ok()
}

/// A filename from a Rust path or an internal C-string interface.
/// Rust paths retain their native encoding. C strings use the shared
/// filename decoder on Windows.
pub trait FileName {
    /// The path to open.
    fn as_path(&self) -> std::borrow::Cow<'_, Path>;
    /// The pin's narrow name for Unix calls and diagnostics.
    fn as_c_name(&self) -> std::borrow::Cow<'_, std::ffi::CStr>;
}

impl FileName for Path {
    fn as_path(&self) -> std::borrow::Cow<'_, Path> {
        std::borrow::Cow::Borrowed(self)
    }
    fn as_c_name(&self) -> std::borrow::Cow<'_, std::ffi::CStr> {
        std::borrow::Cow::Owned(path::c_string(self.as_os_str()))
    }
}

impl FileName for std::path::PathBuf {
    fn as_path(&self) -> std::borrow::Cow<'_, Path> {
        std::borrow::Cow::Borrowed(self.as_ref())
    }
    fn as_c_name(&self) -> std::borrow::Cow<'_, std::ffi::CStr> {
        std::borrow::Cow::Owned(path::c_string(self.as_os_str()))
    }
}

impl FileName for std::ffi::CStr {
    fn as_path(&self) -> std::borrow::Cow<'_, Path> {
        std::borrow::Cow::Owned(path::decode_filename(self))
    }
    fn as_c_name(&self) -> std::borrow::Cow<'_, std::ffi::CStr> {
        std::borrow::Cow::Borrowed(self)
    }
}

impl FileName for CString {
    fn as_path(&self) -> std::borrow::Cow<'_, Path> {
        self.as_c_str().as_path()
    }
    fn as_c_name(&self) -> std::borrow::Cow<'_, std::ffi::CStr> {
        std::borrow::Cow::Borrowed(self)
    }
}
