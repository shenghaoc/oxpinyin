//! Process-locale assertions run in their own test executable: init, save
//! and fini in the unit-test executable can also change the locale.
//! Keep every locale scenario in the single test below so the default
//! parallel test runner cannot interleave them within this process.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;

#[cfg(any(target_os = "linux", target_os = "android"))]
const LC_NUMERIC: c_int = 1;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const LC_NUMERIC: c_int = 4;

unsafe extern "C" {
    fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
}

fn numeric_locale() -> CString {
    // SAFETY: a NULL locale queries LC_NUMERIC. This executable has one
    // test and no other locale-changing threads; copy libc's string before
    // the next setlocale call can invalidate it.
    let name = unsafe { setlocale(LC_NUMERIC, ptr::null()) };
    assert!(!name.is_null(), "LC_NUMERIC query failed");
    // SAFETY: the successful query returns a NUL-terminated libc string
    // and no intervening call or other thread can invalidate it.
    unsafe { CStr::from_ptr(name) }.to_owned()
}

struct RestoreNumericLocale(CString);

impl Drop for RestoreNumericLocale {
    fn drop(&mut self) {
        // SAFETY: the saved name came from a successful LC_NUMERIC query
        // in this process, is NUL-terminated, and remains alive for the
        // call. The only test owns all locale changes, including unwind.
        unsafe { setlocale(LC_NUMERIC, self.0.as_ptr()) };
    }
}

/// A failed init still inherits table_info.cpp's LC_NUMERIC reset at the
/// pin (074a2219). C.UTF-8 is glibc's built-in UTF-8 locale (2.35+).
#[test]
fn a_failed_init_leaves_lc_numeric_at_c() {
    let _restore = RestoreNumericLocale(numeric_locale());
    // SAFETY: the literal is NUL-terminated and outlives the call. No
    // other test or thread in this executable can change the locale.
    let installed = unsafe { setlocale(LC_NUMERIC, c"C.UTF-8".as_ptr()) };
    assert!(!installed.is_null(), "missing input: the C.UTF-8 locale");
    assert_eq!(numeric_locale().as_c_str(), c"C.UTF-8");

    let context = zhuyin_capi::zhuyin_init(c"".as_ptr(), c"".as_ptr());
    assert!(context.is_null());
    assert_eq!(numeric_locale().as_c_str(), c"C");
}
