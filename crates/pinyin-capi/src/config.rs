//! Configuration symbols: options, schemes, phrase library loading.

use std::os::raw::c_int;

use crate::types::{PinyinContext, PinyinOptionT};

/// Set pinyin options on the context.
///
/// # C signature
/// ```c
/// bool pinyin_set_options(pinyin_context_t * context, pinyin_option_t options);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_options(context: *mut PinyinContext, _options: PinyinOptionT) -> bool {
    if context.is_null() {
        return false;
    }
    // STUB: T4 will wire to Config.
    false
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
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_double_pinyin_scheme(
    context: *mut PinyinContext,
    _scheme: c_int,
) -> bool {
    if context.is_null() {
        return false;
    }
    // STUB: T4 will wire to Config.
    false
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
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_zhuyin_scheme(context: *mut PinyinContext, _scheme: c_int) -> bool {
    if context.is_null() {
        return false;
    }
    // STUB: T4 will wire to Config.
    false
}

/// Load an addon phrase library by index.
///
/// # C signature
/// ```c
/// bool pinyin_load_addon_phrase_library(pinyin_context_t * context,
///                                       guint8 index);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_load_addon_phrase_library(
    context: *mut PinyinContext,
    _index: u8,
) -> bool {
    if context.is_null() {
        return false;
    }
    // STUB: T4 will wire to data loading.
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
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_mask_out(context: *mut PinyinContext, _mask: u32, _value: u32) -> bool {
    if context.is_null() {
        return false;
    }
    // STUB: T4 will implement.
    false
}
