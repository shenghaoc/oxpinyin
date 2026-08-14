//! FFI boundary helpers — panic catch, C-string conversion.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

/// Runs `body` inside [`catch_unwind`], returning `fallback` on panic.
///
/// Every `extern "C"` entry point wraps its real work in this so that a
/// bug in Rust code never unwinds across the C ABI (which is UB).
pub fn ffi_catch<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(fallback)
}

/// Converts a nullable C string to an owned [`String`].
///
/// Returns an empty string for null or invalid UTF-8.
pub unsafe fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: Caller guarantees `ptr` is null-terminated when non-null.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .unwrap_or("")
        .to_owned()
}

// `void *malloc(size_t)` from the host's libc. The C consumer links GLib,
// which itself links libc, so this symbol is already resolvable at runtime
// without adding a dependency.
unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
}

/// Duplicates `s` into a fresh, NUL-terminated buffer using libc `malloc`
/// (which `g_free`/`free` can release). Returns null on an interior NUL byte
/// or allocation failure.
///
/// The caller owns the returned pointer and frees it with `g_free`.
pub(crate) fn owned_cstr(s: &str) -> *mut c_char {
    let cstr = match CString::new(s) {
        Ok(c) => c,
        Err(_) => return ptr::null_mut::<c_char>(),
    };
    let bytes = cstr.as_bytes_with_nul();
    // SAFETY: `malloc` returns a valid `bytes.len()`-byte allocation or null.
    let dst = unsafe { malloc(bytes.len()) }.cast::<u8>();
    if dst.is_null() {
        return ptr::null_mut::<c_char>();
    }
    // SAFETY: `dst` points to `bytes.len()` writable bytes and
    // `bytes.as_ptr()` points to `bytes.len()` readable bytes.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
    }
    dst.cast::<c_char>()
}
