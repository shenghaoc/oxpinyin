//! The phrase-result surface: `pinyin_phrase_segment` and the two
//! getters over `m_phrase_result` (`pinyin.cpp:3269-3287`).
//!
//! The segmenter is the engine's span DP (`oxpinyin_engine::Session::
//! phrase_segment`, the `PhraseLookup::get_best_match` port); this
//! module only stores its result on the instance and hands out reads.
//! The result shape is the pin's: a character-length array with each
//! phrase's token at its span's start position and `null_token` between
//! phrases — and, on a failed match, the fully sized all-null array
//! (`PhraseLookup::final_step` sizes and null-fills before its
//! empty-last-step `false`, `phrase_lookup.cpp:382-428`). `pinyin_reset`
//! clears the array (`pinyin.cpp:2699`); nothing else removes from it.

use std::os::raw::c_char;

use crate::state::{instance_mut, instance_ref};
use crate::types::{GUint, PhraseTokenT, PinyinInstance};

/// Segment an arbitrary sentence string into phrase tokens.
///
/// # C signature
/// ```c
/// bool pinyin_phrase_segment(pinyin_instance_t * instance,
///                            const char * sentence);
/// ```
///
/// Upstream gates on GLib UCS4 conversion length
/// (`g_return_val_if_fail(num_of_chars == ucs4_len, FALSE)`, `pinyin.cpp:1454`) and stores the best phrase
/// path into the instance's result array — including on a failed match,
/// where the array stays fully sized and all-null.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `sentence` is a
/// string. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_phrase_segment(
    instance: *mut PinyinInstance,
    sentence: *const c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // The pin's first call, `g_utf8_strlen(sentence, -1)` (074a2219
    // pinyin.cpp:1450), made through GLib: on NULL, GLib's own precondition
    // logs its critical (GLib 2.84.4 gutf8.c:240). The pin then dereferences
    // NULL in `g_utf8_to_ucs4`; take the ruled no-input outcome instead
    // (074a2219 phrase_lookup.cpp:119-157,382-428).
    if sentence.is_null() {
        // SAFETY: GLib checks `p != NULL || max == 0` itself, logs and
        // returns 0 without reading `p`.
        unsafe {
            glib_sys::g_utf8_strlen(sentence, -1);
        }
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let mut inst = unsafe { instance_mut(instance) };
        return match inst.core.segment_phrases("") {
            Ok(matched) => matched,
            Err(error) => {
                crate::sentence::refuse_lookup_gram("phrase_segment", &error);
                false
            }
        };
    }
    let Some(text) = oxpinyin_capi_marshal::input::utf8_to_ucs4(
        oxpinyin_capi_marshal::borrow_c_string!(sentence),
    ) else {
        // 074a2219 pinyin.cpp:1454: `g_return_val_if_fail(num_of_chars ==
        // ucs4_len, FALSE)` answers false and leaves the result as it was. Its
        // critical is not reproduced (2026-10-31 UTC ruling,
        // docs/findings/upstream-divergences.md#phrase-segmentation-writes-no-invalid-utf-8-critical).
        return false;
    };
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let mut inst = unsafe { instance_mut(instance) };
    let outcome = match text.as_str() {
        Some(text) => inst.core.segment_phrases(text),
        None => inst
            .core
            .segment_phrases_ucs4(text.stored_words().unwrap_or_default()),
    };
    match outcome {
        Ok(matched) => matched,
        Err(error) => {
            crate::sentence::refuse_lookup_gram("pinyin_phrase_segment", &error);
            false
        }
    }
}

/// Get the number of phrase tokens in the phrase result.
///
/// # C signature
/// ```c
/// bool pinyin_get_n_phrase(pinyin_instance_t * instance, guint * num);
/// ```
///
/// Upstream answers `true` unconditionally over whatever
/// `pinyin_phrase_segment` last stored (`pinyin.cpp:3269-3274`); without
/// a prior segment call the count is 0.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `num` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_n_phrase(
    instance: *mut PinyinInstance,
    num: *mut GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    let count = inst.core.phrase_count();
    if !num.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *num = GUint::try_from(count).unwrap_or(GUint::MAX);
        }
    }
    true
}

/// Get the phrase token at an index of the phrase result.
///
/// # C signature
/// ```c
/// bool pinyin_get_phrase_token(pinyin_instance_t * instance,
///                              guint index,
///                              phrase_token_t * token);
/// ```
///
/// The out-param is zeroed before the bounds check — a `false` still
/// delivers `null_token` (`pinyin.cpp:3276-3287`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle and `token` is an
/// out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_phrase_token(
    instance: *mut PinyinInstance,
    index: GUint,
    token: *mut PhraseTokenT,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    if !token.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *token = crate::types::null_token;
        }
    }
    let Some(value) = inst.core.phrase_token(index as usize) else {
        return false;
    };
    if !token.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *token = value;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::undocumented_unsafe_blocks,
        reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
    )]

    use super::*;

    // Real model rows feed the accumulation in the span DP. The pin
    // weights the unigram by 1 - lambda and takes log after weighting;
    // it also divides by a present bigram's stored total before blending
    // (074a2219 phrase_lookup.cpp:288-344).
    #[rstest::rstest]
    #[case::zero_weight("1", false)]
    #[case::negative_weight("2", false)]
    #[case::nan_weight("nan", false)]
    #[case::infinite_bigram("1", true)]
    fn nonfinite_segment_scores_keep_the_token_path(#[case] lambda: &str, #[case] infinite: bool) {
        use crate::test_support::{
            TempUserDir, craft_hash, cstr, gram_bytes, lookup, private_system,
        };

        // These are the committed w3 tokens for 你 and 好.
        const NI: u32 = 0x0100_1225;
        const HAO: u32 = 16_779_429;
        let scratch = tempfile::tempdir().expect("scratch");
        let system = private_system(scratch.path());
        let conf = system.join("table.conf");
        let text = std::fs::read_to_string(&conf).expect("fixture table.conf");
        std::fs::remove_file(&conf).expect("unlink fixture config");
        std::fs::write(
            &conf,
            text.replace(
                "lambda parameter:0.312699",
                &format!("lambda parameter:{lambda}"),
            ),
        )
        .expect("private table.conf");
        let rows = if infinite {
            vec![
                (1_u32.to_le_bytes().to_vec(), gram_bytes(0, &[(NI, 1)])),
                (NI.to_le_bytes().to_vec(), gram_bytes(0, &[(HAO, 1)])),
            ]
        } else {
            Vec::new()
        };
        craft_hash(&system.join("bigram.db"), &rows);

        let user = TempUserDir::new("segment-ieee");
        let system = cstr(system.to_str().expect("UTF-8 path"));
        let user_path = cstr(user.path.to_str().expect("UTF-8 path"));
        let context = unsafe { crate::context::pinyin_init(system.as_ptr(), user_path.as_ptr()) };
        assert!(!context.is_null());
        let instance = unsafe { crate::instance::pinyin_alloc_instance(context) };
        assert!(!instance.is_null());
        assert_eq!(lookup(instance, "你"), [NI]);
        assert!(lookup(instance, "你你").is_empty());

        // The infinite path must beat the one-phrase 你好 path, whose
        // unigram gets zero weight. The other cases have only the two
        // successive unigram steps and no bigrams.
        let (sentence, expected) = if infinite {
            assert_eq!(lookup(instance, "好"), [HAO]);
            assert!(!lookup(instance, "你好").is_empty());
            (c"你好", [NI, HAO])
        } else {
            (c"你你", [NI, NI])
        };
        assert!(unsafe { pinyin_phrase_segment(instance, sentence.as_ptr()) });
        let mut count = 0;
        assert!(unsafe { pinyin_get_n_phrase(instance, &raw mut count) });
        assert_eq!(count, 2);
        for (index, expected_token) in (0..count).zip(expected) {
            let mut token = 0;
            assert!(unsafe { pinyin_get_phrase_token(instance, index, &raw mut token) });
            assert_eq!(token, expected_token);
        }
        unsafe { crate::instance::pinyin_free_instance(instance) };
        unsafe { crate::context::pinyin_fini(context) };
    }
}
