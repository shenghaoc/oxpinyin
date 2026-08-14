//! FFI boundary helpers — panic catch, C-string conversion.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

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
