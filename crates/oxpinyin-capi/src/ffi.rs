//! Output allocation and diagnostics for the exposed C boundary.
//! Input conversion is shared by `oxpinyin_capi_marshal::input`.

#[cfg(test)]
use std::ffi::CStr;
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::ptr;

// `void *malloc(size_t)` and `void free(void *)` from the host's libc. The
// C consumer links GLib, which itself links libc, so these symbols are already
// resolvable at runtime without adding a dependency.
unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    pub(crate) fn free(ptr: *mut c_void);
}

/// Converts one caller-owned export-iterator buffer into a Rust string and
/// frees it with the matching libc `free` (the iterator allocates with
/// [`owned_cstr`], i.e. libc `malloc`). Test-only since the §9 snapshot
/// wrappers moved out; the tests read ABI-allocated buffers through it.
#[cfg(test)]
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
/// wrote: one allocation per string, the shape of the pin's
/// `g_ucs4_to_utf8` and `g_strjoinv` results that `pinyin_iterator_get_next_phrase`
/// hands out (`pinyin.cpp:716`, `:733`). NULL when the allocation fails,
/// when `fill` wrote another length, or when the bytes carry a NUL, which
/// [`owned_cstr`] refuses the same way. The buffer is zeroed before `fill`
/// sees it, so the slice it writes through is initialized memory.
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
/// The array and every string are libc `malloc`. Returns null if any
/// allocation fails (and frees whatever was already allocated).
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

/// Logs `message` through `GLib` at warning level under the `libpinyin`
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
            c"libpinyin".as_ptr(),
            glib_sys::G_LOG_LEVEL_WARNING,
            c"%s".as_ptr(),
            message.as_ptr(),
        );
    }
}

/// The pin's `g_return_val_if_fail` line: domain NULL (the pin leaves
/// `G_LOG_DOMAIN` undefined there) at `G_LOG_LEVEL_CRITICAL`, exactly the
/// channel `SubPhraseIndex::load` uses (`phrase_index.cpp:354-356`). A
/// message with an interior NUL is dropped rather than truncated.
pub fn log_critical(message: &str) {
    let Ok(message) = CString::new(message) else {
        return;
    };
    // SAFETY: `g_log` is variadic; the `%s` format consumes exactly one
    // `const char *`. The NULL domain is the pin's own, and the message is
    // a NUL-terminated buffer that outlives the call.
    unsafe {
        glib_sys::g_log(
            std::ptr::null(),
            glib_sys::G_LOG_LEVEL_CRITICAL,
            c"%s".as_ptr(),
            message.as_ptr(),
        );
    }
}
