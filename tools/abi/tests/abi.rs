//! Exercise the shared libraries as C consumers under AddressSanitizer.

#![expect(
    unsafe_code,
    reason = "this test calls the real C ABI and its allocators"
)]

use std::env;
use std::ffi::{CStr, CString, c_char};
use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::process::Command;
use std::ptr;

// Bindgen emits C typedef aliases alongside their underlying types.
#[allow(nonstandard_style, dead_code, unused_imports, clippy::all)]
mod bindings {
    pub(super) mod pinyin {
        include!(concat!(env!("OUT_DIR"), "/pinyin.rs"));
    }

    pub(super) mod zhuyin {
        include!(concat!(env!("OUT_DIR"), "/zhuyin.rs"));
    }
}

// Stringify the same destructor that the consumer calls. A register edit cannot
// change the actual operation that the test compares with the declared class.
macro_rules! release_handle {
    ($slots:expr, $api:expr, $source:ident, $release:ident, $ptr:expr) => {{
        let handle = $ptr;
        assert!(!handle.is_null());
        $slots.record(
            stringify!($source),
            "return",
            concat!("handle:", stringify!($release)),
            handle,
        );
        $api.$release(handle);
    }};
}

// Return the same GLib function whose name is recorded for the slot.
macro_rules! glib_release {
    ($slots:expr, $symbol:expr, $name:expr, $ptr:expr, $release:ident) => {{
        $slots.record($symbol, $name, stringify!($release), $ptr);
        glib_sys::$release
    }};
}

mod constants;
mod exercises;
mod pinyin;
mod zhuyin;

#[derive(Clone)]
struct Slot {
    symbol: &'static str,
    name: &'static str,
    release: &'static str,
    on_false: &'static str,
    hit: bool,
    probed: bool,
}

const PINYIN: &[Slot] = include!(concat!(env!("OUT_DIR"), "/pinyin-slots.rs"));
const ZHUYIN: &[Slot] = include!(concat!(env!("OUT_DIR"), "/zhuyin-slots.rs"));

// A real address outside the caller's heap. It is only compared for equality.
fn sentinel<T>() -> *mut T {
    static UNTOUCHED: u8 = 0;
    ptr::from_ref(&UNTOUCHED).cast_mut().cast()
}

struct Coverage {
    slots: Vec<Slot>,
}

impl Coverage {
    fn new(register: &[Slot]) -> Self {
        Self {
            slots: register.to_vec(),
        }
    }

    fn slot(&mut self, symbol: &str, name: &str) -> &mut Slot {
        self.slots
            .iter_mut()
            .find(|slot| slot.symbol == symbol && slot.name == name)
            .unwrap_or_else(|| panic!("unregistered {symbol}/{name}"))
    }

    fn record<T>(&mut self, symbol: &str, name: &str, actual: &str, pointer: *const T) {
        let slot = self.slot(symbol, name);
        assert_eq!(actual, slot.release, "release used by {symbol}/{name}");
        slot.hit |= !pointer.is_null();
    }

    fn borrowed<T>(&mut self, symbol: &str, name: &str, pointer: *const T) {
        assert!(!pointer.is_null(), "{symbol}/{name} returned NULL");
        self.record(symbol, name, "borrowed", pointer);
    }

    /// Check the register's contract. Return only a real allocation that the caller must free.
    fn false_out<T>(&mut self, symbol: &str, name: &str, ok: bool, out: *mut T) -> *mut T {
        assert!(!ok, "{symbol}/{name} unexpectedly succeeded");
        let slot = self.slot(symbol, name);
        let allocation = match slot.on_false {
            "false-nulls" => {
                assert!(out.is_null(), "{symbol}/{name} did not write NULL");
                ptr::null_mut()
            }
            "false-untouched" => {
                assert_eq!(out, sentinel(), "{symbol}/{name} overwrote its output");
                ptr::null_mut()
            }
            "false-allocates" => {
                assert!(
                    !out.is_null() && out != sentinel(),
                    "{symbol}/{name} did not allocate"
                );
                out
            }
            other => panic!("{symbol}/{name} returned false with contract {other}"),
        };
        slot.probed = true;
        allocation
    }

    unsafe fn free(&mut self, symbol: &str, name: &str, string: *mut c_char) {
        let release = glib_release!(self, symbol, name, string, g_free);
        if string.is_null() {
            return;
        }

        // SAFETY: The caller obtained an owned NUL-terminated string from the C
        // facade. Resize it and append a character before returning it to GLib (#924).
        unsafe {
            let len = CStr::from_ptr(string).to_bytes().len();
            let string: *mut c_char = glib_sys::g_realloc(string.cast(), len + 2).cast();
            string.add(len).write(b'!' as c_char);
            string.add(len + 1).write(0);
            release(string.cast());
        }
    }

    unsafe fn free_vector(&mut self, symbol: &str, name: &str, vector: *mut *mut c_char) {
        let release = glib_release!(self, symbol, name, vector, g_strfreev);
        assert!(!vector.is_null(), "{symbol}/{name} returned NULL");
        // SAFETY: The facade transfers a NULL-terminated GLib vector and its
        // strings. Replace the first string and grow the vector before releasing both.
        unsafe {
            let count = glib_sys::g_strv_length(vector) as usize;
            assert!(count > 0, "{symbol}/{name} returned no keyboard symbols");
            glib_sys::g_free((*vector).cast());
            vector.write(glib_sys::g_strdup(c"caller replacement".as_ptr()));
            let vector: *mut *mut c_char =
                glib_sys::g_realloc_n(vector.cast(), count + 2, size_of::<*mut c_char>()).cast();
            vector
                .add(count)
                .write(glib_sys::g_strdup(c"caller append".as_ptr()));
            vector.add(count + 1).write(ptr::null_mut());
            release(vector);
        }
    }

    fn finish(&self) {
        for slot in &self.slots {
            assert!(
                slot.hit,
                "{}/{} was never exercised",
                slot.symbol, slot.name
            );
            if !matches!(slot.on_false, "n/a" | "false-unreachable") {
                assert!(
                    slot.probed,
                    "{}/{} {} was not probed",
                    slot.symbol, slot.name, slot.on_false
                );
            }
        }
    }
}

unsafe extern "C" {
    fn __lsan_disable();
    fn __lsan_enable();
    fn __lsan_do_recoverable_leak_check() -> std::ffi::c_int;
}

fn artifact_path(var: &str) -> PathBuf {
    let path = PathBuf::from(env::var_os(var).unwrap_or_else(|| panic!("{var} is required")));
    assert!(path.is_absolute(), "{var} must name an absolute path");
    path
}

fn checked_library_path(var: &str) -> PathBuf {
    let path = artifact_path(var);
    // Run the resolver here so it inherits nextest's loader path and working directory.
    let status = Command::new(env::var_os("OXPINYIN_ABI_PYTHON").expect("ABI gate interpreter"))
        .args(["-X", "utf8"])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/check_runtime.py"))
        .arg(&path)
        .status()
        .expect("start the ABI runtime check");
    assert!(
        status.success(),
        "runtime check for {var} failed ({status})"
    );
    path
}

fn check_ownership(register: &[Slot], lifecycle: impl Fn(&mut Coverage, &CStr, &CStr)) {
    warm_then_measure(register, lifecycle);
    // SAFETY: All per-call objects and the measured stack frame are gone. LSan
    // scans this live process now, and can check again when the process exits.
    let leaks = unsafe { __lsan_do_recoverable_leak_check() };
    assert_eq!(leaks, 0, "LeakSanitizer found a per-call leak");
}

#[inline(never)]
fn warm_then_measure(register: &[Slot], lifecycle: impl Fn(&mut Coverage, &CStr, &CStr)) {
    let system = artifact_path("OXPINYIN_ABI_FIXTURE");
    let system = CString::new(system.as_os_str().as_encoded_bytes()).unwrap();
    let user = tempfile::tempdir().unwrap();
    let user_path = CString::new(user.path().as_os_str().as_encoded_bytes()).unwrap();

    // SAFETY: LSan's pair applies to the current thread. All first-use work
    // happens here before the second lifecycle starts with fresh coverage.
    unsafe { __lsan_disable() };
    lifecycle(&mut Coverage::new(register), &system, &user_path);
    // SAFETY: This balances the __lsan_disable call above on the same thread.
    unsafe { __lsan_enable() };

    let mut measured = Coverage::new(register);
    lifecycle(&mut measured, &system, &user_path);
    measured.finish();
}

#[test]
fn pinyin_ownership() {
    let library = checked_library_path("OXPINYIN_CAPI_SO");
    // SAFETY: The caller supplies the built pinyin cdylib. Its function signatures
    // come from pinyin.h. Keep the library and its process-lifetime data loaded
    // for LSan's exit check, just as with an ordinarily linked C application.
    let api = unsafe { bindings::pinyin::Pinyin::new(library) }.unwrap();
    let api = ManuallyDrop::new(api);
    check_ownership(PINYIN, |slots, system, user| {
        pinyin::lifecycle(&api, slots, system, user)
    });
}

#[test]
fn zhuyin_ownership() {
    let library = checked_library_path("OXPINYIN_ZHUYIN_SO");
    // SAFETY: This is the zhuyin cdylib, loaded with the types from zhuyin.h.
    // Its process-lifetime data remains live when LSan checks the process at exit.
    let api = unsafe { bindings::zhuyin::Zhuyin::new(library) }.unwrap();
    let api = ManuallyDrop::new(api);
    check_ownership(ZHUYIN, |slots, system, user| {
        zhuyin::lifecycle(&api, slots, system, user)
    });
}
