//! Configuration symbols: options, schemes, phrase library loading.

use std::os::raw::c_int;

use oxpinyin_engine::ConfigValue;

use crate::ffi::ffi_catch;
use crate::state::{context_mut, context_ref};
use crate::types::{PinyinContext, PinyinOptionT, PinyinTableFlag};

/// Set pinyin options on the context.
///
/// # C signature
/// ```c
/// bool pinyin_set_options(pinyin_context_t * context, pinyin_option_t options);
/// ```
///
/// W8 fork-bootstrap wiring: the fork passes its GSettings-derived option
/// mask before allocating any instance.  oxpinyin-engine currently has a
/// session key for `PINYIN_INCOMPLETE`, so that bit is decoded into
/// `incomplete-pinyin`; the correction/fuzzy/dynamic-adjust bits still have
/// no engine backend and are accepted without effect (see the W8 report).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_options(context: *mut PinyinContext, options: PinyinOptionT) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`.
        let ctx = unsafe { context_mut(context) };
        ctx.config.set(
            "incomplete-pinyin",
            ConfigValue::Bool((options & (PinyinTableFlag::PINYIN_INCOMPLETE as u32)) != 0),
        );
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
/// The upstream predicate (`pinyin.cpp:1224`, `ngram_kyotodb.cpp:199`,
/// `phrase_index.cpp:689`): every entry whose token satisfies
/// `(token & mask) == value` is deleted — bigram rows (a matching
/// predecessor drops its whole gram), unigram deltas, and user phrases.
/// Immediate and durable (upstream writes the masked chunks/diff logs
/// directly); it does **not** arm `m_modified`, matching upstream's
/// set-sites (`pinyin_train` and `pinyin_end_add_phrases` only).
///
/// Returns `false` for a null context, a context without a user store, or
/// a store failure.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_mask_out(context: *mut PinyinContext, mask: u32, value: u32) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`;
        // the unique borrow lasts only for the mask call.
        let ctx = unsafe { context_mut(context) };
        ctx.mask_out(mask, value)
    })
}
