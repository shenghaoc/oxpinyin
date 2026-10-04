//! Configuration symbols: options, schemes, phrase library loading.

use std::os::raw::c_int;
use std::sync::atomic::Ordering;

use crate::state::{context_mut, context_ref};
use crate::types::{PinyinOptionT, ZhuyinContext};

/// `PHRASE_INDEX_LIBRARY_COUNT` (`phrase_index.h`): sixteen sub-indices.
const PHRASE_INDEX_LIBRARY_COUNT: u8 = 16;

/// `TSI_DICTIONARY` (`novel_types.h`): the one default library that stays.
const TSI_DICTIONARY: u8 = 1;

/// Set the zhuyin scheme.
///
/// # C signature
/// ```c
/// bool zhuyin_set_chewing_scheme(zhuyin_context_t * context,
///                                ZhuyinScheme scheme);
/// ```
///
/// The Rust parameter is `c_int`: callers may pass any `int`.
/// Every implemented Zhuyin keyboard is table-driven; the `ZHUYIN_STANDARD_DVORAK`
/// (7) upstream abort slot, like every out-of-enum value, reports `false`
/// and one warning instead of aborting (no-abort policy, divergence class
/// (c)).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_set_chewing_scheme(context: *mut ZhuyinContext, scheme: c_int) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_mut(context) };
    if !matches!(scheme, 1 | 2 | 3 | 4 | 5 | 6 | 8 | 9) {
        // Class (c): the dvorak slot (7) falls through to
        // `zhuyin_parser2.cpp:295`, every out-of-enum value to
        // `zhuyin.cpp:736`, both **`abort()`**.
        crate::ffi::log_warning(&format!(
            "zhuyin_set_chewing_scheme: scheme {scheme} aborts the parser table"
        ));
        return false;
    }
    ctx.core.live.zhuyin_scheme.store(scheme, Ordering::Relaxed);
    true
}

/// Set the full pinyin scheme.
///
/// # C signature
/// ```c
/// bool zhuyin_set_full_pinyin_scheme(zhuyin_context_t * context,
///                                    FullPinyinScheme scheme);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_set_full_pinyin_scheme(
    context: *mut ZhuyinContext,
    scheme: c_int,
) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_mut(context) };
    if !matches!(scheme, 1..=3) {
        // Class (c), `pinyin_parser2.cpp:398`: **`abort()`** in
        // `FullPinyinParser2::set_scheme`.
        crate::ffi::log_warning(&format!(
            "zhuyin_set_full_pinyin_scheme: scheme {scheme} aborts the parser table"
        ));
        return false;
    }
    ctx.core.live.full_scheme.store(scheme, Ordering::Relaxed);
    true
}

/// `zhuyin_set_options` — copies the caller's option word.
///
/// # C signature
/// ```c
/// bool zhuyin_set_options(zhuyin_context_t * context,
///                         pinyin_option_t options);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_set_options(context: *mut ZhuyinContext, options: PinyinOptionT) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_mut(context) };
    // The shared set_options law (word, mirrored bools, config key);
    // the pinyin facade's setter runs the same body.
    ctx.core.set_options(options);
    true
}

/// `zhuyin_mask_out`.
///
/// # C signature
/// ```c
/// bool zhuyin_mask_out(zhuyin_context_t * context,
///                      phrase_token_t mask,
///                      phrase_token_t value);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_mask_out(context: *mut ZhuyinContext, mask: u32, value: u32) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`;
    // the unique borrow lasts only for the mask call.
    let ctx = unsafe { context_mut(context) };
    ctx.mask_out(mask, value)
}

/// Load a default phrase library by index.
///
/// # C signature
/// ```c
/// bool zhuyin_load_phrase_library(zhuyin_context_t * context,
///                                 guint8 index);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_load_phrase_library(context: *mut ZhuyinContext, index: u8) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_ref(context) };
    // Class (c), `zhuyin.cpp:372`: the stock `table.conf` leaves library 0
    // (reserved) and 8..=15 unused, and the pin **`assert`**s that a loaded
    // library is a `SYSTEM_FILE` or `USER_FILE` (an index of 16 or more
    // answers `false` first, `:362`).
    if index == 0 || (8..PHRASE_INDEX_LIBRARY_COUNT).contains(&index) {
        crate::ffi::log_warning(
            "zhuyin_load_phrase_library: assertion 'SYSTEM_FILE == table_info->m_file_type \
             || USER_FILE == table_info->m_file_type' failed",
        );
        return false;
    }
    ctx.load_phrase_library(index as u32)
}

/// Unload a default phrase library by index.
///
/// # C signature
/// ```c
/// bool zhuyin_unload_phrase_library(zhuyin_context_t * context,
///                                   guint8 index);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_unload_phrase_library(context: *mut ZhuyinContext, index: u8) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_ref(context) };
    if index >= PHRASE_INDEX_LIBRARY_COUNT {
        // Class (c), `zhuyin.cpp:381`: **`assert`**.
        crate::ffi::log_warning(
            "zhuyin_unload_phrase_library: assertion 'index < PHRASE_INDEX_LIBRARY_COUNT' failed",
        );
        return false;
    }
    // `tsi.bin` can't be unloaded (`zhuyin.cpp:383-385`); any other library
    // is, and the pin ignores what `unload` returns.
    if index == TSI_DICTIONARY {
        return false;
    }
    ctx.unload_phrase_library(index)
}
