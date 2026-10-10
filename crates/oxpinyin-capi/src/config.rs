//! Configuration symbols: options, schemes, phrase library loading.

use std::os::raw::c_int;
use std::sync::atomic::Ordering;

use crate::state::{PHRASE_INDEX_LIBRARY_COUNT, context_mut, context_ref};
use crate::types::{PinyinContext, PinyinOptionT};

/// `GBK_DICTIONARY` (`novel_types.h`): the one default library that can be
/// unloaded.
const GBK_DICTIONARY: u8 = 2;

/// Set pinyin options on the context.
///
/// # C signature
/// ```c
/// bool pinyin_set_options(pinyin_context_t * context, pinyin_option_t options);
/// ```
///
/// Decodes the full option word into the live context. `PINYIN_INCOMPLETE`
/// and `USE_TONE` stay extracted for the W13 scheme parsers; the rest of
/// the word remasks already-allocated full-pinyin sessions.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_options(context: *mut PinyinContext, options: PinyinOptionT) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { context_mut(context) };
    // The shared set_options law (word, mirrored bools, config key);
    // the zhuyin facade's setter runs the same body.
    ctx.core.set_options(options);
    true
}

/// Set the full pinyin scheme.
///
/// # C signature
/// ```c
/// bool pinyin_set_full_pinyin_scheme(pinyin_context_t * context,
///                                    FullPinyinScheme scheme);
/// ```
///
/// Exported by
/// libpinyin.ver, never called by ibus-libpinyin 1.16.5, and added to
/// the oxpinyin surface post-bootstrap. The Rust parameter is `c_int`:
/// callers may pass any `int`. HANYU (1) is the default and keeps the
/// frozen full-pinyin parser surface; LUOMA (2) and `SECONDARY_ZHUYIN`
/// (3) switch the parse onto their pinned indexes. Other values report
/// `false` and keep the previous scheme instead of aborting (the
/// out-of-enum contract-lock is a separate workstream).
///
/// Exported in every build: it is in the pin's `libpinyin.ver`. It was
/// compiled out of the shipped artifact while the subset scoping
/// (exception (d), retired 2026-09-06) stood; the export gate
/// (`tools/abi/check-exports.sh --shipped`) found the packaged library
/// one symbol short of the version script on 2026-09-08.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_full_pinyin_scheme(
    context: *mut PinyinContext,
    scheme: c_int,
) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { context_mut(context) };
    if !matches!(scheme, 1..=3) {
        // Class (c), `pinyin_parser2.cpp:398`: `set_scheme`'s `default:` is
        // an **`abort()`**.
        crate::ffi::log_warning("pinyin_set_full_pinyin_scheme: scheme is not a FullPinyinScheme");
        return false;
    }
    ctx.core.live.full_scheme.store(scheme, Ordering::Relaxed);
    true
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
///
/// **Out-of-enum values** (negatives, 0, 7–29, 31+) reproduce
/// upstream's half-mutation (`pinyin_parser2.cpp:580,614`;
/// `pinyin.cpp:1155–1159`): the live scheme's fallback table is
/// cleared (the unconditional `m_fallback_table = NULL` at `:580`) and
/// the wrapper returns `true` (ignoring the parser's `false` at
/// `:614`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_double_pinyin_scheme(
    context: *mut PinyinContext,
    scheme: c_int,
) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { context_mut(context) };
    let scheme = match scheme {
        1 => oxpinyin_core::DoublePinyinScheme::Zrm,
        2 => oxpinyin_core::DoublePinyinScheme::Ms,
        3 => oxpinyin_core::DoublePinyinScheme::Ziguang,
        4 => oxpinyin_core::DoublePinyinScheme::Abc,
        5 => oxpinyin_core::DoublePinyinScheme::Pyjj,
        6 => oxpinyin_core::DoublePinyinScheme::Xhe,
        // CUSTOMIZED (30): no compiled table, upstream aborts;
        // oxpinyin reports `false` and leaves the live scheme intact.
        30 => {
            // Class (c), `pinyin_parser2.cpp:611`: **`abort()`**.
            crate::ffi::log_warning(
                "pinyin_set_double_pinyin_scheme: CUSTOMIZED has no compiled table",
            );
            return false;
        }
        // Out-of-enum: upstream's unconditional `m_fallback_table = NULL`
        // (`:580`) fires before the switch falls through to `return false`
        // (`:614`), and the wrapper ignores that `false` and answers `true`
        // (`pinyin.cpp:1155-1159`).  Set the packed fallback-cleared bit.
        _ => {
            ctx.core
                .live
                .double_scheme
                .fetch_or(oxpinyin_facade::FALLBACK_CLEARED_BIT, Ordering::Relaxed);
            return true;
        }
    };
    // A valid scheme (1-6) never has bit 30, so this store clears the
    // fallback-cleared flag implicitly.
    ctx.core
        .live
        .double_scheme
        .store(scheme as i32, Ordering::Relaxed);
    true
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
/// Every implemented Zhuyin keyboard is table-driven: the Simple
/// keyboards (STANDARD 1, IBM 3, GINYIEH 4, ETEN 5), the Discrete
/// keyboards (HSU 2, ETEN26 6, `HSU_DVORAK` 8), and `DACHEN_CP26` 9. The
/// remaining `ZhuyinScheme` header discriminant — the `STANDARD_DVORAK`
/// abort slot — reports `false` and keeps the previous scheme instead
/// of aborting.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_set_zhuyin_scheme(context: *mut PinyinContext, scheme: c_int) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { context_mut(context) };
    if !matches!(scheme, 1 | 2 | 3 | 4 | 5 | 6 | 8 | 9) {
        // Class (c): `pinyin.cpp:1189` (`default:`) and, for the
        // `STANDARD_DVORAK` slot, `zhuyin_parser2.cpp:295`; both are
        // **`abort()`**, with the old parser already deleted.
        crate::ffi::log_warning("pinyin_set_zhuyin_scheme: scheme is not a ZhuyinScheme");
        return false;
    }
    ctx.core.live.zhuyin_scheme.store(scheme, Ordering::Relaxed);
    true
}

/// Load an addon phrase library by index.
///
/// # C signature
/// ```c
/// bool pinyin_load_addon_phrase_library(pinyin_context_t * context,
///                                       guint8 index);
/// ```
///
/// Loads addon library `index` from `addon_{index}_*` next to the
/// system tables. Returns `false` when the context is null, the library is
/// already loaded, or the exported tables are missing
/// (`docs/findings/phrase-union.md` §3.5).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_load_addon_phrase_library(context: *mut PinyinContext, index: u8) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { context_ref(context) };
    ctx.load_addon(index)
}

/// Unload an addon phrase library.
///
/// # C signature
/// ```c
/// bool pinyin_unload_addon_phrase_library(pinyin_context_t * context,
///                                         guint8 index);
/// ```
///
/// The pin (`pinyin.cpp:124-131`) asserts `index <
/// PHRASE_INDEX_LIBRARY_COUNT`, calls `m_addon_phrase_index->unload(index)`
/// and answers `true` unconditionally — including for a library that was
/// never loaded. This mirrors that, except that the assert becomes `false`
/// per the availability class of `docs/findings/compatibility-policy.md`.
///
/// Live call site: fcitx-libpinyin (`eim.cpp`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_unload_addon_phrase_library(
    context: *mut PinyinContext,
    index: u8,
) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { context_ref(context) };
    ctx.unload_addon(index)
}

/// Load a default phrase library by index.
///
/// # C signature
/// ```c
/// bool pinyin_load_phrase_library(pinyin_context_t * context,
///                                 guint8 index);
/// ```
///
/// Upstream answers `false` for an out-of-range index, `false` when the
/// library is already loaded (the system tables load at init), and
/// asserts on non-dictionary file types — all `false` here under the
/// no-abort policy. The only `true` on a healthy context is GBK (2)
/// after an unload, re-attaching from disk upstream and clearing the
/// visibility mask here.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_load_phrase_library(context: *mut PinyinContext, index: u8) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { context_ref(context) };
    // Class (c), `pinyin.cpp:457`: the pin **`assert`**s that the row of a
    // loaded library is a `SYSTEM_FILE` or `USER_FILE` (an index of 16 or
    // more answers `false` first, `:448`); which rows are is the
    // context's `table.conf`.
    match ctx.core.load_phrase_library(u32::from(index)) {
        Ok(loaded) => loaded,
        Err(_) => {
            crate::ffi::log_warning(
                "pinyin_load_phrase_library: assertion 'SYSTEM_FILE == table_info->m_file_type \
                 || USER_FILE == table_info->m_file_type' failed",
            );
            false
        }
    }
}

/// Unload a default phrase library by index.
///
/// # C signature
/// ```c
/// bool pinyin_unload_phrase_library(pinyin_context_t * context,
///                                   guint8 index);
/// ```
///
/// The GBK-only gate, verbatim: upstream asserts the index in range and
/// refuses every non-GBK library before unloading
/// (`pinyin.cpp:464-472`) — `false` here for both shapes (no-abort) —
/// and for GBK calls `unload` and answers `true` whatever it returned
/// (`:473-474`), so a repeat unload answers `true` as well.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_unload_phrase_library(context: *mut PinyinContext, index: u8) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { context_ref(context) };
    if index >= PHRASE_INDEX_LIBRARY_COUNT {
        // Class (c), `pinyin.cpp:466`: **`assert`**.
        crate::ffi::log_warning(
            "pinyin_unload_phrase_library: assertion 'index < PHRASE_INDEX_LIBRARY_COUNT' failed",
        );
        return false;
    }
    // Only the GBK table can be unloaded (`pinyin.cpp:468-470`).
    if index != GBK_DICTIONARY {
        return false;
    }
    ctx.unload_phrase_library(index)
}

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
///
/// The user table's `mask_out` walks every `user_pinyin_index.bin` record
/// and dies of SIGABRT at the `switch` whose `default` is `abort()` when a
/// key is past `MAX_PHRASE_LENGTH` syllables
/// (`chewing_large_table2_bdb.cpp:529`); the facade answers `false` and one
/// `libpinyin` warning for that shape (class (c)).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_mask_out(context: *mut PinyinContext, mask: u32, value: u32) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`;
    // the unique borrow lasts only for the mask call.
    let ctx = unsafe { context_mut(context) };
    match ctx.mask_out(mask, value) {
        oxpinyin_facade::MaskOutOutcome::Done(answer) => answer,
        oxpinyin_facade::MaskOutOutcome::OverlongIndexKey => {
            // Class (c), `chewing_large_table2_bdb.cpp:529`: the user
            // pinyin index carries a key past MAX_PHRASE_LENGTH syllables
            // and the pin's `mask_out` switch falls to `default: abort()`.
            crate::ffi::log_warning(
                "pinyin_mask_out: a user pinyin index key is longer than MAX_PHRASE_LENGTH \
                 syllables (upstream aborts, chewing_large_table2_bdb.cpp:529)",
            );
            false
        }
        oxpinyin_facade::MaskOutOutcome::NonTokenUserBigramKey => {
            // Class (c), `ngram_bdb.cpp:199`: the user bigram carries a key
            // that is not a phrase_token_t and the pin's `get_all_items`
            // assert dies.
            crate::ffi::log_warning(
                "pinyin_mask_out: a user bigram key is not a phrase_token_t \
                 (upstream aborts, ngram_bdb.cpp:199)",
            );
            false
        }
        oxpinyin_facade::MaskOutOutcome::ShortUserBigramValue => {
            // Class (c), `memory_chunk.h:390`: the user bigram carries a
            // value shorter than a guint32 total_freq and the pin's
            // `get_total_freq` read asserts.
            crate::ffi::log_warning(
                "pinyin_mask_out: a user bigram value is shorter than a guint32 total_freq \
                 (upstream aborts, memory_chunk.h:390)",
            );
            false
        }
        oxpinyin_facade::MaskOutOutcome::ResidualUserBigramGram => {
            // Class (c), `ngram.cpp:70`: masking a user bigram removed
            // every item of a gram whose total its items did not cover,
            // and the pin's `get_length` assert dies
            // (`Bigram::mask_out`, `ngram_bdb.cpp:243`).
            crate::ffi::log_warning(
                "pinyin_mask_out: masking a user bigram leaves a residual total_freq \
                 (upstream aborts, ngram.cpp:70)",
            );
            false
        }
    }
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
        assert!(pinyin_get_candidate(instance, 0, &raw mut cand));
        let mut text: *const GChar = std::ptr::null();
        assert!(pinyin_get_candidate_string(instance, cand, &raw mut text));
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
            2,
            "the USE_TONE default leaves the incomplete tail"
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

    #[test]
    fn set_options_use_tone_reaches_the_hanyu_parser() {
        // `pinyin_parser2.cpp:164-214`: with USE_TONE the trailing digit
        // is consumed with the syllable; without it the digit is junk.
        let user_dir = TempUserDir::new("set-options-use-tone");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        let zai4 = cstr("zai4");
        assert!(pinyin_set_options(
            context,
            PinyinTableFlag::PINYIN_INCOMPLETE as u32
        ));
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, zai4.as_ptr()),
            3,
            "a toneless word leaves the digit unparsed"
        );

        assert!(pinyin_set_options(
            context,
            PinyinTableFlag::PINYIN_INCOMPLETE as u32 | PinyinTableFlag::USE_TONE as u32
        ));
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, zai4.as_ptr()),
            4,
            "USE_TONE consumes the digit with the syllable"
        );

        let zhuang4 = cstr("zhuang4");
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, zhuang4.as_ptr()),
            7,
            "the 7-byte window admits zhuang4 whole"
        );

        let rejected = cstr("zai6");
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, rejected.as_ptr()),
            3,
            "6 is not a tone: the digit stays junk"
        );

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}
