//! Process-locale assertions run in their own test executable: init, save
//! and fini in the unit-test executable can also change the locale.
//! Keep every locale scenario in the single test below so the default
//! parallel test runner cannot interleave them within this process.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
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

/// LC_NUMERIC after each `table_info.cpp` site the C API reaches, the host's UTF-8 locale
/// installed before each. Each site sets "C" and restores the name `setlocale` answered, which
/// is "C" itself (`table_info.cpp:197`/`:291`, `:328`/`:372`, `:378`/`:394` at 074a2219), so a
/// site leaves "C" behind:
/// - a failed init reaches the system-table site;
/// - fini writes `user.conf` (`pinyin.cpp:1194-1200`), and init reads and writes it
///   (`:185-187`);
/// - an unmodified save returns false before any write (`pinyin.cpp:1136-1137`) and leaves the
///   installed locale;
/// - a modified save writes `user.conf`.
///
/// Pin capture of 2026-11-07, runs 1 and 2 identical, observation `locale/sites`.
#[test]
fn lc_numeric_follows_the_pins_table_info_sites() {
    let _restore = RestoreNumericLocale(numeric_locale());
    let utf8_locale = if cfg!(windows) { c".UTF-8" } else { c"C.UTF-8" };
    // SAFETY: the literal is NUL-terminated and outlives the call. No
    // other test or thread in this executable can change the locale.
    let installed = unsafe { setlocale(LC_NUMERIC, utf8_locale.as_ptr()) };
    assert!(
        !installed.is_null(),
        "missing input: {utf8_locale:?} locale"
    );
    #[cfg(not(windows))]
    assert_eq!(numeric_locale().as_c_str(), utf8_locale);
    // UCRT expands .UTF-8 to the current language and UTF-8 code page.
    #[cfg(windows)]
    assert_ne!(numeric_locale().as_c_str(), c"C");

    let context = pinyin_capi::pinyin_init(c"".as_ptr(), c"".as_ptr());
    assert!(context.is_null());
    assert_eq!(numeric_locale().as_c_str(), c"C");

    let install = || {
        // SAFETY: as above; this test owns every locale change in the executable.
        assert!(!unsafe { setlocale(LC_NUMERIC, utf8_locale.as_ptr()) }.is_null());
        numeric_locale()
    };
    let system = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT);
    let system = CString::new(system.to_str().expect("UTF-8 path")).expect("no NUL");
    let user = tempfile::tempdir().expect("user directory");
    let user = CString::new(user.path().to_str().expect("UTF-8 path")).expect("no NUL");
    let open = || {
        let context = pinyin_capi::oxpinyin_init_for_fixtures(system.as_ptr(), user.as_ptr());
        assert!(!context.is_null());
        context
    };
    let context = open();
    install();
    pinyin_capi::pinyin_fini(context);
    assert_eq!(numeric_locale().as_c_str(), c"C", "after fini");
    install();
    let context = open();
    assert_eq!(numeric_locale().as_c_str(), c"C", "after init");
    let installed = install();
    assert!(!pinyin_capi::pinyin_save(context));
    assert_eq!(numeric_locale(), installed, "after an unmodified save");
    let import = pinyin_capi::pinyin_begin_add_phrases(context, 7);
    assert!(pinyin_capi::pinyin_iterator_add_phrase(
        import,
        c"你好".as_ptr(),
        c"nihao".as_ptr(),
        1
    ));
    pinyin_capi::pinyin_end_add_phrases(import);
    install();
    assert!(pinyin_capi::pinyin_save(context));
    assert_eq!(numeric_locale().as_c_str(), c"C", "after a modified save");
    pinyin_capi::pinyin_fini(context);
}
