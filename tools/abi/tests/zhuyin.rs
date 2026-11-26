//! libzhuyin consumer lifecycle over its exported C functions.

use std::ffi::CStr;
use std::ptr;

use super::bindings::zhuyin::{ChewingKey, USER_DICTIONARY, Zhuyin, null_token, zhuyin_instance_t};
use super::exercises::{KeyCase, Mapping, first_token, key_strings, keyboard};
use super::{Coverage, sentinel};

fn render_key(
    api: &Zhuyin,
    slots: &mut Coverage,
    instance: *mut zhuyin_instance_t,
    key: *mut ChewingKey,
    case: KeyCase,
) {
    // SAFETY: Both call sites pass the borrowed key from this live instance's current parse.
    unsafe {
        key_strings(
            slots,
            instance,
            key,
            case,
            &[
                ("zhuyin_get_zhuyin_string", api.zhuyin_get_zhuyin_string),
                ("zhuyin_get_pinyin_string", api.zhuyin_get_pinyin_string),
            ],
        );
    }
}

fn false_contracts(api: &Zhuyin, slots: &mut Coverage, instance: *mut zhuyin_instance_t) {
    // SAFETY: The instance is live throughout. Each writable output slot initially
    // holds a non-heap sentinel used only for comparison. Borrowed pointers are
    // discarded before changing the parse that owns them.
    unsafe {
        assert!(api.zhuyin_reset(instance));
        let mut sentence = sentinel();
        let ok = api.zhuyin_get_sentence(instance, &mut sentence);
        slots.false_out("zhuyin_get_sentence", "sentence", ok, sentence);

        let mut candidate = sentinel();
        let ok = api.zhuyin_get_candidate(instance, 0, &mut candidate);
        slots.false_out("zhuyin_get_candidate", "candidate", ok, candidate);

        // The key accessors abort on an empty matrix. Probe the end of a real parse.
        api.zhuyin_parse_more_chewings(instance, c"su3cl3".as_ptr());
        let mut key = sentinel();
        let ok = api.zhuyin_get_zhuyin_key(instance, 6, &mut key);
        slots.false_out("zhuyin_get_zhuyin_key", "key", ok, key);
        let mut rest = sentinel();
        let ok = api.zhuyin_get_zhuyin_key_rest(instance, 6, &mut rest);
        slots.false_out("zhuyin_get_zhuyin_key_rest", "key_rest", ok, rest);

        assert!(api.zhuyin_reset(instance));
        let mut len = 0;
        let mut text = sentinel();
        let ok = api.zhuyin_token_get_phrase(instance, null_token, &mut len, &mut text);
        slots.false_out("zhuyin_token_get_phrase", "utf8_str", ok, text);
        keyboard(
            slots,
            "zhuyin_in_chewing_keyboard",
            Mapping::Unmapped,
            |key, out| api.zhuyin_in_chewing_keyboard(instance, key, out),
        );

        assert!(api.zhuyin_reset(instance));
        api.zhuyin_parse_more_full_pinyins(instance, c"xi1'".as_ptr());
        let mut zero = ptr::null_mut();
        assert!(api.zhuyin_get_zhuyin_key(instance, 3, &mut zero));
        render_key(api, slots, instance, zero, KeyCase::Separator);
    }
}

pub(super) fn lifecycle(api: &Zhuyin, slots: &mut Coverage, system: &CStr, user: &CStr) {
    // SAFETY: The directories and the loaded library outlive the handles. These
    // serial calls use writable output slots, consume borrowed views before the
    // next mutation, and release caller-owned objects through the consumer ABI.
    unsafe {
        let context = api.zhuyin_init(system.as_ptr(), user.as_ptr());
        assert!(!context.is_null());
        let instance = api.zhuyin_alloc_instance(context);
        assert!(!instance.is_null());

        // Standard-scheme keystrokes for 你好 in the mini fixture.
        api.zhuyin_parse_more_chewings(instance, c"su3cl3".as_ptr());
        assert!(api.zhuyin_guess_sentence(instance));
        assert!(api.zhuyin_guess_candidates_after_cursor(instance, 0));
        let mut count = 0;
        assert!(api.zhuyin_get_n_candidate(instance, &mut count) && count > 0);
        let mut candidate = ptr::null_mut();
        assert!(api.zhuyin_get_candidate(instance, 0, &mut candidate));
        slots.borrowed("zhuyin_get_candidate", "candidate", candidate);
        let mut text = ptr::null();
        assert!(api.zhuyin_get_candidate_string(instance, candidate, &mut text));
        slots.borrowed("zhuyin_get_candidate_string", "utf8_str", text);

        let mut key = ptr::null_mut();
        assert!(api.zhuyin_get_zhuyin_key(instance, 0, &mut key));
        slots.borrowed("zhuyin_get_zhuyin_key", "key", key);
        let mut rest = ptr::null_mut();
        assert!(api.zhuyin_get_zhuyin_key_rest(instance, 0, &mut rest));
        slots.borrowed("zhuyin_get_zhuyin_key_rest", "key_rest", rest);
        render_key(api, slots, instance, key, KeyCase::Parsed);

        let token =
            first_token(|tokens| api.zhuyin_lookup_tokens(instance, c"你好".as_ptr(), tokens));
        let mut len = 0;
        let mut phrase = ptr::null_mut();
        assert!(api.zhuyin_token_get_phrase(instance, token, &mut len, &mut phrase));
        slots.free("zhuyin_token_get_phrase", "utf8_str", phrase);
        keyboard(
            slots,
            "zhuyin_in_chewing_keyboard",
            Mapping::Mapped,
            |key, out| api.zhuyin_in_chewing_keyboard(instance, key, out),
        );

        let mut sentence = ptr::null_mut();
        assert!(api.zhuyin_get_sentence(instance, &mut sentence));
        slots.free("zhuyin_get_sentence", "sentence", sentence);
        api.zhuyin_train(instance);
        let iter = api.zhuyin_begin_add_phrases(context, USER_DICTIONARY.try_into().unwrap());
        assert!(!iter.is_null());
        api.zhuyin_iterator_add_phrase(iter, c"你好".as_ptr(), c"ni'hao".as_ptr(), 3);
        release_handle!(
            slots,
            api,
            zhuyin_begin_add_phrases,
            zhuyin_end_add_phrases,
            iter
        );

        false_contracts(api, slots, instance);
        release_handle!(
            slots,
            api,
            zhuyin_alloc_instance,
            zhuyin_free_instance,
            instance
        );
        release_handle!(slots, api, zhuyin_init, zhuyin_fini, context);
    }
}
