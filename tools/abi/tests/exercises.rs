//! Consumer operations shared by the pinyin and zhuyin probes.

use std::ffi::c_char;
use std::mem::size_of;
use std::ptr;

use super::{Coverage, sentinel};

/// The first token returned by the C facade for a fixture phrase.
pub(super) fn first_token(lookup: impl FnOnce(*mut glib_sys::GArray) -> bool) -> u32 {
    // SAFETY: The caller's array holds phrase_token_t (guint32). GLib owns its storage
    // until the final unref. A successful nonempty lookup initialized the first token.
    unsafe {
        let array = glib_sys::g_array_new(0, 0, size_of::<u32>() as u32);
        let token = (lookup(array) && (*array).len > 0).then(|| *(*array).data.cast::<u32>());
        glib_sys::g_array_unref(array);
        token.expect("fixture phrase must have a token")
    }
}

#[derive(Clone, Copy)]
pub(super) enum KeyCase {
    Parsed,
    Separator,
}

type KeyGetter<I, K> = unsafe extern "C" fn(*mut I, *mut K, *mut *mut c_char) -> bool;

/// Probe the same renderers on a parsed key and the zero key at a trailing separator.
pub(super) unsafe fn key_strings<I, K>(
    slots: &mut Coverage,
    instance: *mut I,
    key: *mut K,
    case: KeyCase,
    getters: &[(&str, KeyGetter<I, K>)],
) {
    assert!(!key.is_null());
    for &(symbol, render) in getters {
        let mut text = match case {
            KeyCase::Parsed => ptr::null_mut(),
            KeyCase::Separator => sentinel(),
        };
        // SAFETY: The caller supplies the instance's current borrowed key and its facade's
        // renderers. The output slot is writable. A live returned string is caller-owned.
        unsafe {
            let ok = render(instance, key, &mut text);
            match case {
                KeyCase::Parsed => {
                    assert!(ok, "{symbol} refused a parsed key");
                    assert!(!text.is_null(), "{symbol} returned an empty pointer");
                    slots.free(symbol, "utf8_str", text);
                }
                KeyCase::Separator => {
                    let allocation = slots.false_out(symbol, "utf8_str", ok, text);
                    if !allocation.is_null() {
                        slots.free(symbol, "utf8_str", allocation);
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Mapping {
    Mapped,
    Unmapped,
}

/// Find a key of the requested kind in the active keyboard scheme.
pub(super) unsafe fn keyboard(
    slots: &mut Coverage,
    symbol: &str,
    mapping: Mapping,
    mut lookup: impl FnMut(c_char, *mut *mut *mut c_char) -> bool,
) {
    for key in b'!'..=b'~' {
        let mut symbols = sentinel();
        let mapped = lookup(key as c_char, &mut symbols);
        if mapped {
            assert!(
                !symbols.is_null() && symbols != sentinel(),
                "{symbol} returned no vector"
            );
            // SAFETY: A successful lookup transfers the NULL-terminated string vector.
            unsafe { slots.free_vector(symbol, "symbols", symbols) };
            if matches!(mapping, Mapping::Mapped) {
                return;
            }
        } else {
            let allocation = slots.false_out(symbol, "symbols", mapped, symbols);
            if !allocation.is_null() {
                // SAFETY: A false-allocates result transfers the vector just as on success.
                unsafe { slots.free_vector(symbol, "symbols", allocation) };
            }
            if matches!(mapping, Mapping::Unmapped) {
                return;
            }
        }
    }
    panic!("{symbol} has no printable key with the required mapping");
}
