//! Output allocation and diagnostics for the exposed C boundary.
//! Input conversion is shared by `oxpinyin_capi_marshal::input`.

use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::ptr;

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
        log_warning("owned_cstr: allocation failed");
        return ptr::null_mut::<c_char>();
    }
    // SAFETY: `dst` points to `bytes.len()` writable bytes and
    // `bytes.as_ptr()` points to `bytes.len()` readable bytes.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
    }
    dst.cast::<c_char>()
}

/// Hands C one `malloc`-allocated, NUL-terminated string of `len` bytes
/// that `fill` writes into the buffer it is given, returning how many it
/// wrote. NULL when the allocation fails, when `fill` wrote another length,
/// or when the bytes carry a NUL.
pub fn malloc_cstr_with(len: usize, fill: impl FnOnce(&mut [u8]) -> usize) -> *mut c_char {
    malloc_cstr_with_policy(len, fill, false)
}

/// The final string allocation at pin g_malloc/g_strdup/g_strconcat sites.
/// Class (c): one warning on allocation failure; the caller returns false.
pub fn malloc_cstr_with_warning(len: usize, fill: impl FnOnce(&mut [u8]) -> usize) -> *mut c_char {
    malloc_cstr_with_policy(len, fill, true)
}

fn malloc_cstr_with_policy(
    len: usize,
    fill: impl FnOnce(&mut [u8]) -> usize,
    warn: bool,
) -> *mut c_char {
    let Some(size) = len.checked_add(1) else {
        if warn {
            log_warning("malloc_cstr_with: allocation size overflow");
        }
        return ptr::null_mut();
    };
    // SAFETY: `malloc` returns `size` writable bytes or null.
    let dst = unsafe { malloc(size) }.cast::<u8>();
    if dst.is_null() {
        if warn {
            log_warning("malloc_cstr_with: allocation failed");
        }
        return ptr::null_mut();
    }
    // SAFETY: `dst` points to `size` writable bytes that nothing else
    // references; zeroing them makes every byte an initialized `u8`.
    unsafe {
        ptr::write_bytes(dst, 0, size);
    }
    // SAFETY: `dst` points to `size` initialized bytes this function alone
    // owns until it returns, so the exclusive slice is valid for its scope.
    let buffer = unsafe { std::slice::from_raw_parts_mut(dst, size) };
    let written = fill(&mut buffer[..len]);
    if written != len || buffer[..len].contains(&0) {
        // SAFETY: `dst` came from `malloc` above and was handed out nowhere.
        unsafe { free(dst.cast()) };
        return ptr::null_mut();
    }
    buffer[len] = 0;
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
        log_warning("owned_cstr_list: allocation size overflow");
        return ptr::null_mut();
    };
    // SAFETY: `malloc` returns `bytes` writable bytes or null.
    let arr = unsafe { malloc(bytes) }.cast::<*mut c_char>();
    if arr.is_null() {
        log_warning("owned_cstr_list: allocation failed");
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
