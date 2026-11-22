//! Native I/O for the library. This module owns configuration
//! reads and writes, diagnostics, [`chunk`] mappings and writes, and path operations.
//!
//! [`c_path`] converts a whole path for a C call. [`c_string`] converts the prefix
//! ending at its first NUL.

// The external I/O boundary is described in docs/findings/external-io-boundary.md.

pub mod chunk;
pub(crate) mod config;
pub(crate) mod diagnostic;
#[expect(
    unsafe_code,
    reason = "the library's libc and glib-sys calls behind owned handles; every block carries a SAFETY comment"
)]
mod native;
mod path;
pub use native::{c_stderr, fopen};
#[cfg(windows)]
pub use native::{errno_location, wide_path};
pub use path::{
    build_filename, build_path, c_string, decode_filename, filename, path_from_bytes,
    path_to_bytes, rename, unlink,
};

use std::ffi::CString;
use std::path::Path;

/// The newline a native text-mode `fopen("w")` writes: CRLF on Windows,
/// LF on Unix. The pin's `user.conf` and generated text tables inherit it.
#[cfg(any(test, feature = "test-hooks"))]
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

pub(crate) use path::{build_bytes, build_chunk_path};
