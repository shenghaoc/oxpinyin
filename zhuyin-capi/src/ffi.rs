//! Output allocation and diagnostics for the exposed C boundary.
//! Input conversion is shared by `oxpinyin::marshal::input`.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use oxpinyin::KeyText;

// `void *malloc(size_t)` and `void free(void *)` from the host's libc.
unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

oxpinyin::borrow_c_string_helpers!();

oxpinyin::ffi_output_helpers! {
    glib: glib_sys, key_text: KeyText, malloc: malloc, free: free, domain: c"libzhuyin"
}
