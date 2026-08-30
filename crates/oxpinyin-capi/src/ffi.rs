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

/// Converts a nullable C string to an owned [`String`], `None` unless the
/// bytes are valid UTF-8 — the shape `pinyin_phrase_segment` needs,
/// because the pin's `g_return_val_if_fail(num_of_chars == ucs4_len,
/// FALSE)` gate (`pinyin.cpp:1450-1452`) rejects exactly the inputs a
/// lossy conversion would paper over.
pub(crate) fn cstr_to_strict(ptr: *const c_char) -> Option<String> {
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
/// contract at the boundary and never let those inputs unwind.
pub fn cstr_to_owned_lossy(ptr: *const c_char) -> String {
    // SAFETY: `cstr_to_string` requires a null-terminated pointer when
    // non-null. The only callers are `extern "C"` entry points, whose
    // contract to C is exactly that; a violation is the caller's C-level
    // memory error, not a Rust lifetime escape.
    unsafe { cstr_to_string(ptr) }
}

// `void *malloc(size_t)` and `void free(void *)` from the host's libc. The
// C consumer links GLib, which itself links libc, so these symbols are already
// resolvable at runtime without adding a dependency.
unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

/// Converts one caller-owned export-iterator buffer into a Rust string and
/// frees it with the matching libc `free` (the iterator allocates with
/// [`owned_cstr`], i.e. libc `malloc`).
pub fn take_owned_cstr(ptr: *mut c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let text = unsafe {
        // SAFETY: The export iterators only hand out NUL-terminated, valid
        // UTF-8 buffers allocated by [`owned_cstr`].
        CStr::from_ptr(ptr)
    }
    .to_string_lossy()
    .into_owned();
    // SAFETY: `ptr` came from `owned_cstr`, which uses libc `malloc`; libc
    // `free` is the matching deallocator.
    unsafe { free(ptr.cast()) };
    text
}

/// Duplicates `s` into a fresh, NUL-terminated buffer using libc `malloc`
/// (which `g_free`/`free` can release). Returns null on an interior NUL byte
/// or allocation failure.
///
/// The caller owns the returned pointer and frees it with `g_free`.
pub fn owned_cstr(s: &str) -> *mut c_char {
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

/// NULL-terminated array of [`owned_cstr`] pointers for `g_strfreev`.
///
/// The array and every string are libc `malloc`. Returns null if any
/// allocation fails (and frees whatever was already allocated).
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
