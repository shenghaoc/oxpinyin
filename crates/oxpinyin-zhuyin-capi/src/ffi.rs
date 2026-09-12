//! FFI boundary helpers — C-string conversion.
//!
//! Deliberately a per-facade file, not a shared module in
//! `oxpinyin-facade`: every helper below is itself the unsafe edge
//! (libc `malloc`/`free`, `CStr::from_ptr`), the facade crate is
//! `forbid(unsafe_code)`, and the constitution's unsafe allowlist
//! reaches only the two C-ABI crates — so this marshalling layer is
//! duplicated between the facades by design and kept in step by
//! review. The orchestration above it is shared; only the allocator
//! boundary is not.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::ptr;

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

/// Converts a nullable C string to an owned [`String`], `None` unless the
/// bytes are valid UTF-8.
pub fn cstr_to_strict(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: Caller guarantees `ptr` is null-terminated when non-null.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// Safe wrapper for C ABI entry points, which own the null/invalid-UTF-8
/// contract at the boundary.
pub fn cstr_to_owned_lossy(ptr: *const c_char) -> String {
    // SAFETY: `cstr_to_string` requires a null-terminated pointer when
    // non-null. The only callers are `extern "C"` entry points, whose
    // contract to C is exactly that; a violation is the caller's C-level
    // memory error, not a Rust lifetime escape.
    unsafe { cstr_to_string(ptr) }
}

// `void *malloc(size_t)` and `void free(void *)` from the host's libc.
unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    pub(crate) fn free(ptr: *mut c_void);
}

/// Duplicates `s` into a fresh, NUL-terminated buffer using libc `malloc`
/// (which `g_free`/`free` can release). Returns null on an interior NUL byte
/// or allocation failure.
pub fn owned_cstr(s: &str) -> *mut c_char {
    let Ok(cstr) = CString::new(s) else {
        return ptr::null_mut::<c_char>();
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

/// NULL-terminated array of [`owned_cstr`] pointers for `g_strfreev`.
///
/// Returns null if any allocation fails (and frees whatever was allocated).
pub fn owned_cstr_list(items: &[impl AsRef<str>]) -> *mut *mut c_char {
    let n = items.len();
    let bytes = n
        .checked_add(1)
        .and_then(|count| count.checked_mul(std::mem::size_of::<*mut c_char>()));
    let Some(bytes) = bytes else {
        return ptr::null_mut();
    };
    // SAFETY: `malloc` returns `bytes` writable bytes or null.
    let arr = unsafe { malloc(bytes) }.cast::<*mut c_char>();
    if arr.is_null() {
        return ptr::null_mut();
    }
    for (i, item) in items.iter().enumerate() {
        let s = owned_cstr(item.as_ref());
        if s.is_null() {
            for j in 0..i {
                // SAFETY: slots `0..i` were written by `owned_cstr`.
                unsafe {
                    free((*arr.add(j)).cast());
                }
            }
            // SAFETY: `arr` came from `malloc` above.
            unsafe {
                free(arr.cast());
            }
            return ptr::null_mut();
        }
        // SAFETY: `arr` has `n+1` slots; `i < n`.
        unsafe {
            *arr.add(i) = s;
        }
    }
    // SAFETY: terminator slot.
    unsafe {
        *arr.add(n) = ptr::null_mut();
    }
    arr
}

/// Logs `message` through `GLib` at warning level under the `libzhuyin`
/// domain. The library's only diagnostic channel: the C ABI's frozen
/// return shapes (`false` / NULL) carry no reason, and glib is already
/// linked for the ABI's `GArray`s. A message with an interior NUL is
/// dropped rather than truncated.
pub fn log_warning(message: &str) {
    let Ok(message) = CString::new(message) else {
        return;
    };
    // SAFETY: `g_log` is variadic; the `%s` format consumes exactly one
    // `const char *`, and both the domain and the message are
    // NUL-terminated buffers that outlive the call.
    unsafe {
        glib_sys::g_log(
            c"libzhuyin".as_ptr(),
            glib_sys::G_LOG_LEVEL_WARNING,
            c"%s".as_ptr(),
            message.as_ptr(),
        );
    }
}
