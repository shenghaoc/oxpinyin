//! Configuration symbols: options, schemes, phrase library loading.

use std::os::raw::c_int;
use std::sync::atomic::Ordering;

use oxpinyin_engine::ConfigValue;

use crate::ffi::ffi_catch;
use crate::state::context_mut;
use crate::types::{PinyinContext, PinyinOptionT, PinyinTableFlag};

/// Set pinyin options on the context.
///
/// # C signature
/// ```c
/// bool pinyin_set_options(pinyin_context_t * context, pinyin_option_t options);
/// ```
///
/// Decodes `PINYIN_INCOMPLETE` into the live context flag and the
/// `incomplete-pinyin` config key, and `USE_TONE` into the bopomofo
/// parser. Already-allocated instances observe the remask on the next
/// parse or guess. Other bits are accepted without effect until their
/// engine backends exist.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_options(context: *mut PinyinContext, options: PinyinOptionT) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`.
        let ctx = unsafe { context_mut(context) };
        let enabled = (options & (PinyinTableFlag::PINYIN_INCOMPLETE as u32)) != 0;
        let use_tone = (options & (PinyinTableFlag::USE_TONE as u32)) != 0;
        ctx.config
            .set("incomplete-pinyin", ConfigValue::Bool(enabled));
        ctx.incomplete.store(enabled, Ordering::Relaxed);
        ctx.use_tone.store(use_tone, Ordering::Relaxed);
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
/// The scheme value is the `DoublePinyinScheme` header discriminant
/// (`pinyin_custom2.h:108-117`). `DOUBLE_PINYIN_CUSTOMIZED` (30) has no
/// compiled table; oxpinyin reports `false` and keeps the previous scheme
/// rather than aborting like upstream.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_double_pinyin_scheme(
    context: *mut PinyinContext,
    scheme: c_int,
) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`.
        let ctx = unsafe { context_mut(context) };
        let scheme = match scheme {
            1 => oxpinyin_core::DoublePinyinScheme::Zrm,
            2 => oxpinyin_core::DoublePinyinScheme::Ms,
            3 => oxpinyin_core::DoublePinyinScheme::Ziguang,
            4 => oxpinyin_core::DoublePinyinScheme::Abc,
            5 => oxpinyin_core::DoublePinyinScheme::Pyjj,
            6 => oxpinyin_core::DoublePinyinScheme::Xhe,
            _ => return false,
        };
        ctx.double_scheme.store(scheme as i32, Ordering::Relaxed);
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
/// The first W13 bopomofo pass implements STANDARD only. Other valid
/// `ZhuyinScheme` header discriminants report `false` and keep the previous
/// scheme instead of aborting.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_zhuyin_scheme(context: *mut PinyinContext, scheme: c_int) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`.
        let ctx = unsafe { context_mut(context) };
        if scheme != oxpinyin_core::ZhuyinScheme::Standard as i32 {
            return false;
        }
        ctx.zhuyin_scheme.store(scheme, Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::pinyin_set_options;
    use crate::candidates::{pinyin_get_candidate, pinyin_get_candidate_string};
    use crate::parse::pinyin_parse_more_full_pinyins;
    use crate::sentence::pinyin_guess_candidates;
    use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, open};
    use crate::types::{GChar, PinyinTableFlag};

    fn first_candidate_text(instance: *mut crate::types::PinyinInstance) -> String {
        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let mut cand = std::ptr::null_mut();
        assert!(pinyin_get_candidate(instance, 0, &mut cand));
        let mut text: *const GChar = std::ptr::null();
        assert!(pinyin_get_candidate_string(instance, cand, &mut text));
        assert!(!text.is_null());
        // SAFETY: `text` borrows the snapshot until the next guess.
        unsafe { std::ffi::CStr::from_ptr(text) }
            .to_str()
            .expect("utf-8 candidate")
            .to_owned()
    }

    #[test]
    fn set_options_before_alloc_controls_incomplete_parse_length() {
        let user_dir = TempUserDir::new("set-options-before");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        crate::instance::pinyin_free_instance(instance);

        assert!(pinyin_set_options(context, 0));
        let instance = crate::instance::pinyin_alloc_instance(context);
        let nih = cstr("nih");
        assert_eq!(pinyin_parse_more_full_pinyins(instance, nih.as_ptr()), 2);
        assert_eq!(first_candidate_text(instance), "你");

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

    #[test]
    fn set_options_remasks_a_live_instance() {
        let user_dir = TempUserDir::new("set-options-live");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        let nih = cstr("nih");
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, nih.as_ptr()),
            3,
            "default incomplete-on consumes the tail"
        );

        assert!(pinyin_set_options(context, 0));
        assert_eq!(pinyin_parse_more_full_pinyins(instance, nih.as_ptr()), 2);
        assert_eq!(first_candidate_text(instance), "你");

        assert!(pinyin_set_options(
            context,
            PinyinTableFlag::PINYIN_INCOMPLETE as u32
        ));
        assert_eq!(pinyin_parse_more_full_pinyins(instance, nih.as_ptr()), 3);

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}
