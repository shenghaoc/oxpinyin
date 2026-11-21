use std::ffi::CStr;
use std::ptr::{null, null_mut};

use super::bindings::pinyin::{
    ChewingKey, Pinyin, SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY, USER_DICTIONARY,
    null_token, pinyin_context_t, pinyin_instance_t,
};
use super::exercises::{KeyCase, Mapping, first_token, key_strings, keyboard};
use super::{Coverage, sentinel};

fn choose(
    api: &Pinyin,
    slots: &mut Coverage,
    instance: *mut pinyin_instance_t,
    cursor: usize,
    wanted: &CStr,
) -> i32 {
    // SAFETY: The caller owns this live instance. Each candidate and its string are borrowed
    // only until a selection changes the instance or the next candidate window opens.
    unsafe {
        assert!(api.pinyin_guess_candidates(
            instance,
            cursor,
            SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY,
        ));
        let mut count = 0;
        assert!(api.pinyin_get_n_candidate(instance, &mut count));
        for i in 0..count {
            let mut candidate = null_mut();
            assert!(api.pinyin_get_candidate(instance, i, &mut candidate) && !candidate.is_null());
            let mut text = null();
            assert!(api.pinyin_get_candidate_string(instance, candidate, &mut text));
            if !text.is_null() && CStr::from_ptr(text) == wanted {
                slots.borrowed("pinyin_get_candidate", "candidate", candidate);
                slots.borrowed("pinyin_get_candidate_string", "utf8_str", text);
                return api.pinyin_choose_candidate(instance, cursor, candidate);
            }
        }
    }
    panic!("missing candidate {wanted:?}");
}

fn render_keys(
    api: &Pinyin,
    slots: &mut Coverage,
    instance: *mut pinyin_instance_t,
    key: *mut ChewingKey,
    case: KeyCase,
) {
    // SAFETY: Both keys came from this instance's current parse. All outputs point to
    // writable locals, and the sentinel is used only to observe writes on refusal.
    unsafe {
        key_strings(
            slots,
            instance,
            key,
            case,
            &[
                ("pinyin_get_pinyin_string", api.pinyin_get_pinyin_string),
                ("pinyin_get_zhuyin_string", api.pinyin_get_zhuyin_string),
                (
                    "pinyin_get_luoma_pinyin_string",
                    api.pinyin_get_luoma_pinyin_string,
                ),
                (
                    "pinyin_get_secondary_zhuyin_string",
                    api.pinyin_get_secondary_zhuyin_string,
                ),
            ],
        );
        let mut shengmu = sentinel();
        let mut yunmu = sentinel();
        let ok = api.pinyin_get_pinyin_strings(instance, key, &mut shengmu, &mut yunmu);
        for (name, text) in [("shengmu", shengmu), ("yunmu", yunmu)] {
            match case {
                KeyCase::Parsed => {
                    assert!(ok && !text.is_null() && text != sentinel());
                    slots.free("pinyin_get_pinyin_strings", name, text);
                }
                KeyCase::Separator => {
                    let allocated = slots.false_out("pinyin_get_pinyin_strings", name, ok, text);
                    if !allocated.is_null() {
                        slots.free("pinyin_get_pinyin_strings", name, allocated);
                    }
                }
            }
        }
    }
}

fn auxiliary_text(
    api: &Pinyin,
    slots: &mut Coverage,
    instance: *mut pinyin_instance_t,
    cursor: Option<usize>,
) {
    for (symbol, getter) in [
        (
            "pinyin_get_full_pinyin_auxiliary_text",
            api.pinyin_get_full_pinyin_auxiliary_text,
        ),
        (
            "pinyin_get_double_pinyin_auxiliary_text",
            api.pinyin_get_double_pinyin_auxiliary_text,
        ),
        (
            "pinyin_get_chewing_auxiliary_text",
            api.pinyin_get_chewing_auxiliary_text,
        ),
    ] {
        let mut text = sentinel();
        // SAFETY: The instance has the parse corresponding to cursor, or has just been reset.
        // The getter writes into a live local. Both paths hand the caller a GLib allocation.
        unsafe {
            let ok = getter(instance, cursor.unwrap_or(0), &mut text);
            if cursor.is_none() {
                text = slots.false_out(symbol, "aux_text", ok, text);
            } else {
                assert!(ok && !text.is_null() && text != sentinel());
            }
            slots.free(symbol, "aux_text", text);
        }
    }
}

fn iterators(api: &Pinyin, slots: &mut Coverage, context: *mut pinyin_context_t) {
    // SAFETY: The context owns the user dictionary. Each iterator is used before its own
    // end call, and next is called only on a pending row. Out-pointers are writable locals.
    unsafe {
        let import = api.pinyin_begin_add_phrases(context, USER_DICTIONARY.try_into().unwrap());
        assert!(!import.is_null());
        api.pinyin_iterator_add_phrase(import, c"你好".as_ptr(), c"ni'hao".as_ptr(), 3);
        release_handle!(
            slots,
            api,
            pinyin_begin_add_phrases,
            pinyin_end_add_phrases,
            import
        );

        let phrases = api.pinyin_begin_get_phrases(context, USER_DICTIONARY);
        assert!(!phrases.is_null());
        while api.pinyin_iterator_has_next_phrase(phrases) {
            let mut phrase = null_mut();
            let mut pinyin = null_mut();
            let mut count = 0;
            assert!(api.pinyin_iterator_get_next_phrase(
                phrases,
                &mut phrase,
                &mut pinyin,
                &mut count
            ));
            slots.free("pinyin_iterator_get_next_phrase", "phrase", phrase);
            slots.free("pinyin_iterator_get_next_phrase", "pinyin", pinyin);
        }
        release_handle!(
            slots,
            api,
            pinyin_begin_get_phrases,
            pinyin_end_get_phrases,
            phrases
        );

        let bigrams = api.pinyin_begin_get_bigram_phrases(context);
        assert!(!bigrams.is_null());
        while api.pinyin_bigram_iterator_has_next_phrase(bigrams) {
            let mut phrase = sentinel();
            let mut pinyin = sentinel();
            let mut count = 0;
            let more = api.pinyin_bigram_iterator_get_next_phrase(
                bigrams,
                &mut phrase,
                &mut pinyin,
                &mut count,
            );
            for (name, mut text) in [("phrase", phrase), ("pinyin", pinyin)] {
                let symbol = "pinyin_bigram_iterator_get_next_phrase";
                if more {
                    assert!(!text.is_null() && text != sentinel());
                } else {
                    // The terminal row returns false after allocating both strings.
                    text = slots.false_out(symbol, name, more, text);
                }
                slots.free(symbol, name, text);
            }
            if !more {
                break;
            }
        }
        release_handle!(
            slots,
            api,
            pinyin_begin_get_bigram_phrases,
            pinyin_end_get_bigram_phrases,
            bigrams
        );
    }
}

fn refusals(api: &Pinyin, slots: &mut Coverage, instance: *mut pinyin_instance_t) {
    // SAFETY: The instance stays live. Each refusal uses a valid output slot and an
    // in-contract argument. The key getters require a nonempty matrix even on refusal.
    unsafe {
        assert!(api.pinyin_reset(instance));
        auxiliary_text(api, slots, instance, None);

        let mut text = sentinel();
        let ok = api.pinyin_get_sentence(instance, 0, &mut text);
        slots.false_out("pinyin_get_sentence", "sentence", ok, text);
        let mut candidate = sentinel();
        let ok = api.pinyin_get_candidate(instance, 0, &mut candidate);
        slots.false_out("pinyin_get_candidate", "candidate", ok, candidate);

        assert_eq!(
            api.pinyin_parse_more_full_pinyins(instance, c"ni".as_ptr()),
            2
        );
        let mut key = sentinel();
        let ok = api.pinyin_get_pinyin_key(instance, 2, &mut key);
        slots.false_out("pinyin_get_pinyin_key", "key", ok, key);
        let mut rest = sentinel();
        let ok = api.pinyin_get_pinyin_key_rest(instance, 2, &mut rest);
        slots.false_out("pinyin_get_pinyin_key_rest", "key_rest", ok, rest);

        assert!(api.pinyin_reset(instance));
        let mut len = 0;
        let mut text = sentinel();
        let ok = api.pinyin_token_get_phrase(instance, null_token, &mut len, &mut text);
        slots.false_out("pinyin_token_get_phrase", "utf8_str", ok, text);
        keyboard(
            slots,
            "pinyin_in_chewing_keyboard",
            Mapping::Unmapped,
            |key, out| api.pinyin_in_chewing_keyboard(instance, key, out),
        );

        // A trailing separator yields the zero key that the display renderers refuse.
        api.pinyin_parse_more_full_pinyins(instance, c"xi'".as_ptr());
        let mut zero = null_mut();
        assert!(api.pinyin_get_pinyin_key(instance, 2, &mut zero) && !zero.is_null());
        render_keys(api, slots, instance, zero, KeyCase::Separator);
    }
}

pub(super) fn lifecycle(api: &Pinyin, slots: &mut Coverage, system: &CStr, user: &CStr) {
    // SAFETY: The directories and all other C strings live throughout their calls. The
    // context outlives the instance and all iterators. Borrowed keys are consumed before
    // the next parse, and each owned output is returned through its designated release.
    unsafe {
        let context = api.pinyin_init(system.as_ptr(), user.as_ptr());
        assert!(!context.is_null());
        let instance = api.pinyin_alloc_instance(context);
        assert!(!instance.is_null());
        let owner = api.pinyin_get_context(instance);
        assert_eq!(owner, context);
        slots.borrowed("pinyin_get_context", "return", owner);

        assert_eq!(api.pinyin_get_parsed_input_length(instance), 0);
        let consumed = api.pinyin_parse_more_full_pinyins(instance, c"nihao".as_ptr());
        assert_eq!(consumed, c"nihao".to_bytes().len());
        assert_eq!(api.pinyin_get_parsed_input_length(instance), consumed);
        assert!(api.pinyin_reset(instance));
        assert_eq!(api.pinyin_get_parsed_input_length(instance), 0);

        api.pinyin_parse_more_full_pinyins(instance, c"nihao".as_ptr());
        assert!(api.pinyin_guess_sentence(instance));

        let mut key = null_mut();
        assert!(api.pinyin_get_pinyin_key(instance, 0, &mut key) && !key.is_null());
        slots.borrowed("pinyin_get_pinyin_key", "key", key);
        let mut rest = null_mut();
        assert!(api.pinyin_get_pinyin_key_rest(instance, 0, &mut rest));
        slots.borrowed("pinyin_get_pinyin_key_rest", "key_rest", rest);
        render_keys(api, slots, instance, key, KeyCase::Parsed);
        let token =
            first_token(|tokens| api.pinyin_lookup_tokens(instance, c"你好".as_ptr(), tokens));
        let mut len = 0;
        let mut phrase = null_mut();
        assert!(api.pinyin_token_get_phrase(instance, token, &mut len, &mut phrase));
        slots.free("pinyin_token_get_phrase", "utf8_str", phrase);

        keyboard(
            slots,
            "pinyin_in_chewing_keyboard",
            Mapping::Mapped,
            |key, out| api.pinyin_in_chewing_keyboard(instance, key, out),
        );
        let mut sentence = null_mut();
        assert!(api.pinyin_get_sentence(instance, 0, &mut sentence));
        slots.free("pinyin_get_sentence", "sentence", sentence);

        api.pinyin_remember_user_input(instance, c"你好".as_ptr(), -1);
        // Two character selections train a 你 → 好 bigram for the export iterator.
        let cursor = choose(api, slots, instance, 0, c"你");
        assert!(cursor > 0);
        choose(api, slots, instance, cursor.try_into().unwrap(), c"好");
        api.pinyin_train(instance, 0);
        auxiliary_text(api, slots, instance, Some(2));
        iterators(api, slots, context);
        refusals(api, slots, instance);
        release_handle!(
            slots,
            api,
            pinyin_alloc_instance,
            pinyin_free_instance,
            instance
        );
        release_handle!(slots, api, pinyin_init, pinyin_fini, context);
    }
}
