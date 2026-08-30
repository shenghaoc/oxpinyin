//! Configuration symbols: options, schemes, phrase library loading.

use std::os::raw::c_int;
use std::sync::atomic::Ordering;

use oxpinyin_engine::ConfigValue;

use crate::ffi::ffi_catch;
use crate::state::{context_mut, context_ref};
use crate::types::{PinyinOptionT, ZhuyinContext};

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
/// (7) upstream abort slot reports `false` instead of aborting (no-abort
/// policy, divergence class (c)).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_set_chewing_scheme(context: *mut ZhuyinContext, scheme: c_int) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
        let ctx = unsafe { context_mut(context) };
        if !matches!(scheme, 1 | 2 | 3 | 4 | 5 | 6 | 8 | 9) {
            return false;
        }
        ctx.zhuyin_scheme.store(scheme, Ordering::Relaxed);
        true
    })
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
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
        let ctx = unsafe { context_mut(context) };
        if !matches!(scheme, 1..=3) {
            return false;
        }
        ctx.full_scheme.store(scheme, Ordering::Relaxed);
        true
    })
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
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
        let ctx = unsafe { context_mut(context) };
        let enabled = (options & oxpinyin_core::PINYIN_INCOMPLETE) != 0;
        let use_tone = (options & oxpinyin_core::USE_TONE) != 0;
        let force_tone = (options & oxpinyin_core::FORCE_TONE) != 0;
        ctx.config
            .set("incomplete-pinyin", ConfigValue::Bool(enabled));
        ctx.incomplete.store(enabled, Ordering::Relaxed);
        ctx.use_tone.store(use_tone, Ordering::Relaxed);
        ctx.force_tone.store(force_tone, Ordering::Relaxed);
        ctx.options.store(options, Ordering::Relaxed);
        true
    })
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
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `zhuyin_init`;
        // the unique borrow lasts only for the mask call.
        let ctx = unsafe { context_mut(context) };
        ctx.mask_out(mask, value)
    })
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
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
        let ctx = unsafe { context_ref(context) };
        ctx.load_phrase_library(index as u32)
    })
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
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
        let ctx = unsafe { context_ref(context) };
        ctx.unload_phrase_library(index)
    })
}
