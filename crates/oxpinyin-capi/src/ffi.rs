//! Output allocation and diagnostics for the exposed C boundary.
//! Input conversion is shared by `oxpinyin_capi_marshal::input`.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use oxpinyin_core::KeyText;

// `void *malloc(size_t)` and `void free(void *)` from the host's libc. The
// C consumer links GLib, which itself links libc, so these symbols are already
// resolvable at runtime without adding a dependency.
unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

/// Converts one caller-owned returned string into a Rust string and frees
/// it with `g_free`, which releases both the GLib allocations and the
/// encoder's `malloc` buffers. Test-only; the tests read ABI-allocated
/// buffers through it.
#[cfg(test)]
pub fn take_owned_cstr(ptr: *mut c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let text = unsafe {
        // SAFETY: The callers pass NUL-terminated buffers this library
        // returned.
        CStr::from_ptr(ptr)
    }
    .to_string_lossy()
    .into_owned();
    // SAFETY: `ptr` is a returned string the caller owns; `g_free` is its
    // deallocator.
    unsafe { glib_sys::g_free(ptr.cast()) };
    text
}

oxpinyin_capi_marshal::ffi_output_helpers! {
    glib: glib_sys, key_text: KeyText, malloc: malloc, free: free, domain: c"libpinyin"
}

/// `g_strconcat(prefix, middle, postfix, NULL)`, the auxiliary texts'
/// final concatenation (`pinyin.cpp:3431`, `:3509`, `:3569`). The pin's
/// `middle` is never NULL there: every walk that would leave it NULL dies
/// first at `assert(matrix.get_column_size(offset) >= 1)` (`:3410`,
/// `:3467`, `:3545`), which the renderers reproduce (register row 63).
pub fn strconcat_aux(prefix: &str, middle: &str, postfix: &str) -> *mut c_char {
    let (prefix, middle, postfix) = (
        c_arg(prefix.as_bytes()),
        c_arg(middle.as_bytes()),
        c_arg(postfix.as_bytes()),
    );
    // SAFETY: Every argument is a NUL-terminated buffer that outlives the
    // call, and the list ends with NULL.
    unsafe {
        glib_sys::g_strconcat(
            prefix.as_ptr().cast::<c_char>(),
            middle.as_ptr().cast::<c_char>(),
            postfix.as_ptr().cast::<c_char>(),
            ptr::null::<c_char>(),
        )
    }
}

/// `g_strconcat(first, second, NULL)` over two C strings, either NULL: a
/// NULL `first` answers NULL and a NULL `second` ends the list, as in the
/// pin's bigram phrase (`pinyin.cpp:830`).
///
/// # Safety
///
/// Each argument is NULL or a NUL-terminated string.
pub unsafe fn strconcat_pair(first: *const c_char, second: *const c_char) -> *mut c_char {
    // SAFETY: The caller guarantees both are NULL or NUL-terminated; the
    // list ends with NULL, and `g_strconcat` answers NULL for a NULL first
    // argument.
    unsafe { glib_sys::g_strconcat(first, second, ptr::null::<c_char>()) }
}

/// `g_strjoinv(separator, strings)` over `items`: the pin's join of a
/// pronunciation's key strings (`pinyin.cpp:733`). The elements and the
/// array are this call's own temporaries; only the joined string, which
/// GLib allocates, is handed out.
pub fn strjoinv(separator: &CStr, items: impl Iterator<Item = impl AsRef<[u8]>>) -> *mut c_char {
    let items: Vec<Vec<u8>> = items.map(|item| c_arg(item.as_ref())).collect();
    let mut array: Vec<*mut c_char> = items
        .iter()
        .map(|item| item.as_ptr().cast_mut().cast())
        .collect();
    array.push(ptr::null_mut());
    // SAFETY: `array` is a NULL-terminated array of NUL-terminated strings,
    // all of which outlive the call; `g_strjoinv` only reads them.
    unsafe { glib_sys::g_strjoinv(separator.as_ptr(), array.as_mut_ptr()) }
}
