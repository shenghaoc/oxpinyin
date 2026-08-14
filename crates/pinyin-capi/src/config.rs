//! Configuration symbols: options, schemes, phrase library loading.

use std::os::raw::c_int;

use crate::ffi::ffi_catch;
use crate::state::context_ref;
use crate::types::{PinyinContext, PinyinOptionT};

/// Set pinyin options on the context.
///
/// # C signature
/// ```c
/// bool pinyin_set_options(pinyin_context_t * context, pinyin_option_t options);
/// ```
///
/// Provisional: accepts the call but does not yet decode the bitmask into
/// individual config keys.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_options(context: *mut PinyinContext, _options: PinyinOptionT) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`.
        let _ctx = unsafe { context_ref(context) };
        true
    })
}

/// Set the double pinyin scheme.
///
/// # C signature
/// ```c
/// bool pinyin_set_double_pinyin_scheme(pinyin_context_t * context,
///                                      DoublePinyinScheme scheme);
/// ```
///
/// The Rust parameter is `c_int`: callers may pass any `int`, and a closed
/// `#[repr(C)]` enum would be UB for an unknown discriminant.
///
/// Provisional: accepts the call; scheme routing arrives with a dedicated
/// double-pinyin parser.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_double_pinyin_scheme(
    context: *mut PinyinContext,
    _scheme: c_int,
) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`.
        let _ctx = unsafe { context_ref(context) };
        true
    })
}

/// Set the zhuyin scheme.
///
/// # C signature
/// ```c
/// bool pinyin_set_zhuyin_scheme(pinyin_context_t * context,
///                                ZhuyinScheme scheme);
/// ```
///
/// The Rust parameter is `c_int`: callers may pass any `int`, and a closed
/// `#[repr(C)]` enum would be UB for an unknown discriminant.
///
/// Provisional: accepts the call; scheme routing arrives with a dedicated
/// chewing parser.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_zhuyin_scheme(context: *mut PinyinContext, _scheme: c_int) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`.
        let _ctx = unsafe { context_ref(context) };
        true
    })
}

/// Load an addon phrase library by index.
///
/// # C signature
/// ```c
/// bool pinyin_load_addon_phrase_library(pinyin_context_t * context,
///                                       guint8 index);
/// ```
///
/// Provisional: always returns false (no addon phrase system yet).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_load_addon_phrase_library(
    context: *mut PinyinContext,
    _index: u8,
) -> bool {
    if context.is_null() {
        return false;
    }
    false
}

/// Mask out phrase tokens matching a pattern.
///
/// # C signature
/// ```c
/// bool pinyin_mask_out(pinyin_context_t * context,
///                      phrase_token_t mask,
///                      phrase_token_t value);
/// ```
///
/// Provisional: always returns false (no token masking yet).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_mask_out(context: *mut PinyinContext, _mask: u32, _value: u32) -> bool {
    if context.is_null() {
        return false;
    }
    false
}
