//! oxpinyin's external-I/O boundary (`docs/findings/external-io-boundary.md`):
//! the library and tool calls that cross the program's edge, made with the
//! pin's own calls through a small C shim (maintainer ruling 2026-10-28 UTC).
//!
//! Stage 1 holds the `table.conf` and `user.conf` readers and writer and their
//! `LC_NUMERIC` calls ([`config`]); stage 4 the library's other `stderr` lines
//! ([`diagnostic`]), so every library line is written to the C library's
//! stream. What the scanned values mean — the
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
pub mod diagnostic;
#[allow(unsafe_code)]
mod native;

use std::ffi::CString;
use std::path::Path;

/// `path` as the C string `fopen` receives: its bytes as the platform
/// encodes them, or `None` when they hold a NUL, which no C path can.
#[must_use]
pub fn c_path(path: &Path) -> Option<CString> {
    CString::new(path.as_os_str().as_encoded_bytes()).ok()
}
