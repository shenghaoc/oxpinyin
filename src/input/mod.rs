//! Native I/O for the library. This module owns configuration
//! reads and writes, diagnostics, [`chunk`] mappings and writes, and path operations.
//!
//! [`c_string`] converts the prefix of a path that ends at its first NUL.

// The external I/O boundary is described in docs/findings/external-io-boundary.md.

pub mod chunk;
pub(crate) mod config;
pub(crate) mod diagnostic;
#[expect(
    unsafe_code,
    reason = "the library's libc and glib-sys calls behind owned handles; every block carries a SAFETY comment"
)]
pub(crate) mod native;
pub(crate) mod path;
pub use path::{
    build_filename, c_string, decode_filename, filename, path_from_bytes, path_to_bytes, rename,
    unlink,
};

use std::ffi::CString;
use std::path::Path;

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

pub(crate) use path::{build_c_string, build_path};
