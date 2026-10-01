//! The pin's process-locale side effect (register row 39, #539; the libzhuyin half).
//!
//! `SystemTableInfo2::load`, `UserTableInfo::load` and `UserTableInfo::save`
//! each open with `char * locale = setlocale(LC_NUMERIC, "C");` and close
//! with `setlocale(LC_NUMERIC, locale);` (`storage/table_info.cpp:197,291`,
//! `:328,372` and `:378,394` at `074a2219`). `setlocale` given a locale
//! name answers the name of the locale it has just installed, so `locale`
//! holds `"C"` and the closing call installs `"C"` a second time; every
//! early return (`:199-202`, `:207-224`, `:330-348`, `:380-383`) skips the
//! closing call with the same result. Each of the three methods therefore
//! leaves the process's `LC_NUMERIC` at `"C"`, on success and on failure
//! alike, and the entry points that reach them inherit the side effect:
//!
//! - `zhuyin_init`: `load` on `table.conf` (`zhuyin.cpp:281`) before any
//!   other step can fail, then `check_format`'s `load` of
//!   `user.conf` (`:132`; libzhuyin's `check_format` writes nothing);
//! - `zhuyin_save`: `mark_version`'s `save` (`:695`), reached only past
//!   the two guards (`:548-552`: a user dir and `m_modified`);
//! - `zhuyin_fini`: nothing — it writes no `user.conf` (`:741-757`).
//!
//! A consumer that adopted a `zh_CN.UTF-8` environment with
//! `setlocale(LC_ALL, "")` finds `LC_NUMERIC` reset to `"C"` after any of
//! them. The ruling (`docs/findings/upstream-divergences.md` row 39) is to
//! reproduce it bug-for-bug, so each entry point above installs the same
//! locale at the same point. Only the C library's own `setlocale` can do
//! that, which is why this lives in the C-ABI crate and not in the
//! storage crates the pin's `table_info.cpp` corresponds to. The pinyin
//! facade's `oxpinyin-capi/src/locale.rs` is this module's twin.

use std::os::raw::{c_char, c_int};

/// `LC_NUMERIC`'s value in `<locale.h>`: 1 under glibc and musl, 4 on the
/// BSD-derived libcs and MSVCRT.
#[cfg(any(target_os = "linux", target_os = "android"))]
const LC_NUMERIC: c_int = 1;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const LC_NUMERIC: c_int = 4;

unsafe extern "C" {
    fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
}

/// Installs `"C"` as the process's `LC_NUMERIC`, as one call of the pin's
/// `SystemTableInfo2::load` / `UserTableInfo::load` / `UserTableInfo::save`
/// leaves it (`table_info.cpp:197`, `:328`, `:378` with their `:291`,
/// `:372`, `:394` re-installs of the same name).
pub(crate) fn pin_table_info_locale() {
    // SAFETY: `setlocale` is the C library's; `LC_NUMERIC` is a category
    // it defines and the locale name is a NUL-terminated string literal
    // that outlives the call. The returned pointer refers to the C
    // library's own storage and is not kept.
    unsafe {
        setlocale(LC_NUMERIC, c"C".as_ptr());
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;
    use std::ptr;

    use super::{LC_NUMERIC, setlocale};

    fn numeric_locale() -> String {
        // SAFETY: a NULL locale queries without installing; the answer is a
        // NUL-terminated string owned by the C library, copied out at once.
        unsafe { CStr::from_ptr(setlocale(LC_NUMERIC, ptr::null())) }
            .to_string_lossy()
            .into_owned()
    }

    /// `zhuyin_init` on an empty system dir fails before any file opens,
    /// and the pin's `table_info.cpp:197` has already reset `LC_NUMERIC`
    /// by then. `C.UTF-8` is glibc's built-in UTF-8 locale (2.35+), so the
    /// step away from "C" needs no generated locale on the host.
    #[test]
    fn a_failed_init_leaves_lc_numeric_at_c() {
        // SAFETY: `setlocale` with a NUL-terminated literal; the answer is
        // only tested for NULL.
        let installed = unsafe { setlocale(LC_NUMERIC, c"C.UTF-8".as_ptr()) };
        assert!(!installed.is_null(), "missing input: the C.UTF-8 locale");
        assert_eq!(numeric_locale(), "C.UTF-8");

        let context = crate::context::zhuyin_init(c"".as_ptr(), c"".as_ptr());
        assert!(context.is_null());
        assert_eq!(numeric_locale(), "C");
    }
}
