#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::parsed_key_and_rest;

use crate::support::{
    DEFAULT_SORT, PinyinInstance, TempUserDir, candidate, cstr, lookup_candidate_type_t, open,
    pinyin_alloc_instance, pinyin_begin_add_phrases, pinyin_choose_candidate,
    pinyin_clear_constraint, pinyin_end_add_phrases, pinyin_get_candidate, pinyin_guess_candidates,
    pinyin_guess_sentence, pinyin_init, pinyin_is_user_candidate, pinyin_iterator_add_phrase,
    pinyin_iterator_has_next_phrase, pinyin_mask_out, pinyin_parse_more_full_pinyins,
    pinyin_remember_user_input, pinyin_remove_user_candidate, pinyin_save, pinyin_set_options,
    pinyin_train, system_dir,
};

use oxpinyin_core::user::USER_DICTIONARY;

use std::ptr;

#[test]
fn null_user_directory_refuses_training_but_keeps_mutable_state() {
    // A NULL user dir creates transient state. An empty string is a user dir —
    // the working directory, as the pin's is (#619).
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let context = unsafe { pinyin_init(system.as_ptr(), ptr::null()) };
    assert!(!context.is_null(), "a NULL user dir is not an init failure");
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());

    // Upstream pinyin_train refuses without a user dir (pinyin.cpp:2669);
    // the other training entry points degrade the same way.
    assert!(!unsafe { pinyin_train(instance, 0) });
    // pinyin_save refuses without a user dir too (pinyin.cpp:1133).
    assert!(!unsafe { pinyin_save(context) });

    // (`pinyin_choose_predicted_candidate` takes a predicted row only; a
    // normal row is the pin's assert, `pinyin.cpp:2593`.)
    let cand = candidate(instance, "nihao", 0);
    assert!(unsafe { pinyin_remember_user_input(instance, cstr("你好").as_ptr(), -1) });
    assert!(unsafe { pinyin_mask_out(context, 0, 0) });

    // Selection still works without a user directory.
    assert!(unsafe { pinyin_choose_candidate(instance, 0, cand) } > 0);

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// A private system directory under `parent` whose addon row 4 names no
/// chunk (`art.table NULL`), and a context opened on it.
#[cfg(unix)]
fn null_addon_chunk_context(
    parent: &std::path::Path,
) -> (std::path::PathBuf, *mut crate::support::PinyinContext) {
    let system = crate::support::private_system(parent);
    let conf = system.join("table.conf");
    let text = std::fs::read_to_string(&conf).expect("fixture table.conf");
    std::fs::remove_file(&conf).expect("remove the fixture symlink");
    std::fs::write(&conf, text.replace("art.table art.bin", "art.table NULL"))
        .expect("table.conf with NULL addon chunk name");
    let path = cstr(system.to_str().expect("UTF-8 path"));
    let context = unsafe { pinyin_init(path.as_ptr(), ptr::null()) };
    assert!(!context.is_null());
    (system, context)
}

/// A NULL addon filename maps the system directory itself
/// (`g_build_filename(dir, NULL, NULL)`). Where the directory reads as a
/// chunk, the pin asserts on the read of its length word
/// (`memory_chunk.h:493`) and the process aborts. Elsewhere (tmpfs), `mmap`
/// refuses it (`memory_chunk.h:486-489`): the load answers `false` with one
/// warning.
// Unix only, for the reason `oxpinyin-core`'s `save_stops_at_the_directory_with_the_pins_files` gives.
#[cfg(unix)]
#[test]
fn null_addon_chunk_name_aborts_or_logs_once() {
    let scratch = tempfile::tempdir().expect("scratch");
    let (system, context) = null_addon_chunk_context(scratch.path());
    if oxpinyin_core::facade::null_chunk_asserts(&system) {
        unsafe { crate::support::pinyin_fini(context) };
        crate::support::assert_aborts(
            "null_arguments::null_addon_chunk_name_aborts_or_logs_once",
            || {
                let child = tempfile::tempdir().expect("child scratch");
                let (_system, context) = null_addon_chunk_context(child.path());
                let _ = unsafe { crate::support::pinyin_load_addon_phrase_library(context, 4) };
            },
        );
    } else {
        let logs = crate::support::LogCapture::new();
        assert!(!unsafe { crate::support::pinyin_load_addon_phrase_library(context, 4) });
        assert_eq!(
            logs.take(),
            [(Some("libpinyin".to_owned()), glib_sys::G_LOG_LEVEL_WARNING)]
        );
        unsafe { crate::support::pinyin_fini(context) };
    }
}

/// `pinyin_get_candidate_type` never reads `instance` (`pinyin.cpp:2866-2871`):
/// it writes `*type` and answers `true`.
#[test]
fn null_instance_candidate_type_answers() {
    let user = TempUserDir::new("null-instance-candidate-type");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let cand = candidate(instance, "nihao", 0);
    let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
    assert!(unsafe {
        crate::support::pinyin_get_candidate_type(ptr::null_mut(), cand, &raw mut kind)
    });
    assert_eq!(kind, lookup_candidate_type_t::NORMAL_CANDIDATE);
}

/// `pinyin_get_candidate_string` never reads `instance` (`pinyin.cpp:2873-2878`).
#[test]
fn null_instance_candidate_string_answers() {
    let user = TempUserDir::new("null-instance-candidate-string");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let cand = candidate(instance, "nihao", 0);
    let mut text = ptr::null();
    assert!(unsafe {
        crate::support::pinyin_get_candidate_string(ptr::null_mut(), cand, &raw mut text)
    });
    assert!(!text.is_null());
    // SAFETY: the candidate text is NUL-terminated and borrowed from the instance.
    let text = unsafe { std::ffi::CStr::from_ptr(text) };
    assert_eq!(text.to_str().expect("UTF-8"), "你好");
}

/// `pinyin_get_candidate_nbest_index` never reads `instance`
/// (`pinyin.cpp:2880-2886`; the assert at `:2883` reads the candidate).
#[test]
fn null_instance_candidate_nbest_index_answers() {
    let user = TempUserDir::new("null-instance-nbest-index");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let input = cstr("nihao");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        5
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let mut cand = ptr::null_mut();
    assert!(unsafe { pinyin_get_candidate(instance, 0, &raw mut cand) });
    let mut index = u8::MAX;
    assert!(unsafe {
        crate::support::pinyin_get_candidate_nbest_index(ptr::null_mut(), cand, &raw mut index)
    });
    assert_eq!(index, 0);
}

/// `pinyin_is_user_candidate` never reads `instance` (`pinyin.cpp:3712-3724`):
/// a normal candidate in `USER_DICTIONARY` answers `true`.
#[test]
fn null_instance_is_user_candidate_answers() {
    let user = TempUserDir::new("null-instance-is-user");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let iter = unsafe { pinyin_begin_add_phrases(context, USER_DICTIONARY) };
    assert!(unsafe {
        pinyin_iterator_add_phrase(iter, cstr("你号").as_ptr(), cstr("ni3'hao4").as_ptr(), 1)
    });
    unsafe { pinyin_end_add_phrases(iter) };
    let _ = candidate(instance, "nihao", 0);
    let mut count = 0;
    assert!(unsafe { crate::support::pinyin_get_n_candidate(instance, &raw mut count) });
    let mut found = false;
    for index in 0..count {
        let mut cand = ptr::null_mut();
        assert!(unsafe { pinyin_get_candidate(instance, index, &raw mut cand) });
        if unsafe { pinyin_is_user_candidate(instance, cand) } {
            assert!(unsafe { pinyin_is_user_candidate(ptr::null_mut(), cand) });
            found = true;
        }
    }
    assert!(found, "the imported user phrase is a candidate");
}

/// `pinyin_get_pinyin_is_incomplete` never reads `instance`
/// (`pinyin.cpp:2765-2774`): an initial-only key answers `true`.
#[test]
fn null_instance_is_incomplete_answers() {
    let _slots = crate::support::key_slot_guard();
    let user = TempUserDir::new("null-instance-is-incomplete");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    assert!(unsafe { pinyin_set_options(context, 0x28) });
    let (key, _) = parsed_key_and_rest(instance, "n");
    assert!(unsafe { crate::support::pinyin_get_pinyin_is_incomplete(ptr::null_mut(), key) });
    let (complete, _) = parsed_key_and_rest(instance, "ni3");
    assert!(!unsafe { crate::support::pinyin_get_pinyin_is_incomplete(ptr::null_mut(), complete) });
}

/// `pinyin_get_pinyin_key_rest_length` never reads `instance`
/// (`pinyin.cpp:2979-2984`).
#[test]
fn null_instance_key_rest_length_answers() {
    let _slots = crate::support::key_slot_guard();
    let user = TempUserDir::new("null-instance-key-rest-length");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let (_, rest) = parsed_key_and_rest(instance, "ni3");
    let mut length = u16::MAX;
    assert!(unsafe {
        crate::support::pinyin_get_pinyin_key_rest_length(ptr::null_mut(), rest, &raw mut length)
    });
    assert_eq!(length, 3);
}

/// The NULL handles of the constraint, export and removal entries answer
/// `false` (or NULL) and read nothing (register row 41).
#[test]
fn null_handles_answer_false() {
    assert!(!unsafe { pinyin_clear_constraint(ptr::null_mut(), 0) });
    assert!(unsafe { crate::support::pinyin_begin_get_phrases(ptr::null_mut(), 7) }.is_null());
    assert!(unsafe { crate::support::pinyin_begin_get_bigram_phrases(ptr::null_mut()) }.is_null());
    assert!(!unsafe { pinyin_iterator_has_next_phrase(ptr::null_mut()) });
    assert!(!unsafe { crate::support::pinyin_bigram_iterator_has_next_phrase(ptr::null_mut()) });
    let user = TempUserDir::new("null-handles");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    assert!(!unsafe { pinyin_remove_user_candidate(instance, ptr::null_mut()) });
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// `pinyin_begin_add_phrases` stores the context unread and returns the
/// iterator (`pinyin.cpp:506-512`). `pinyin_iterator_add_phrase` returns
/// `false` before it reads the context for a NULL phrase (`:624-625`) and
/// where it would read it (`_add_phrase`, `:526`) the pin crashes (row 41);
/// `pinyin_end_add_phrases` reads it at `:657` (row 41), so the handle is freed.
#[test]
fn null_context_import_handle_is_returned_and_freed() {
    let iter = unsafe { pinyin_begin_add_phrases(ptr::null_mut(), 1) };
    assert!(!iter.is_null());
    assert!(!unsafe {
        pinyin_iterator_add_phrase(iter, ptr::null(), cstr("ni3'hao3").as_ptr(), 1)
    });
    assert!(!unsafe {
        pinyin_iterator_add_phrase(iter, cstr("你好").as_ptr(), cstr("ni3'hao3").as_ptr(), 1)
    });
    unsafe { pinyin_end_add_phrases(iter) };
}

/// The index assert (`pinyin.cpp:466`) precedes any read of the context, so a
/// NULL context aborts for index 16; an in-range non-GBK index answers
/// `false` (`:470-471`) without reading it.
#[test]
fn null_context_unload_phrase_library_orders_assert_first() {
    assert!(!unsafe { crate::support::pinyin_unload_phrase_library(ptr::null_mut(), 2) });
    crate::support::assert_aborts(
        "null_arguments::null_context_unload_phrase_library_orders_assert_first",
        || {
            unsafe { crate::support::pinyin_unload_phrase_library(ptr::null_mut(), 16) };
        },
    );
}

/// `pinyin_unload_addon_phrase_library`: the assert at `pinyin.cpp:499`
/// precedes the context read at `:502`.
#[test]
fn null_context_unload_addon_phrase_library_aborts_on_index_16() {
    crate::support::assert_aborts(
        "null_arguments::null_context_unload_addon_phrase_library_aborts_on_index_16",
        || {
            unsafe { crate::support::pinyin_unload_addon_phrase_library(ptr::null_mut(), 16) };
        },
    );
}

/// The four string renderers never read `instance` (`pinyin.cpp:2707-2749`).
#[test]
fn null_instance_key_string_renderers_answer() {
    let _slots = crate::support::key_slot_guard();
    let user = TempUserDir::new("null-instance-key-strings");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let (key, _) = parsed_key_and_rest(instance, "ni3");
    let render = |getter: unsafe extern "C" fn(
        *mut PinyinInstance,
        *mut crate::support::ChewingKey,
        *mut *mut std::ffi::c_char,
    ) -> bool| {
        let mut out = ptr::null_mut();
        assert!(unsafe { getter(ptr::null_mut(), key, &raw mut out) });
        crate::support::take_owned_cstr(out)
    };
    assert_eq!(render(crate::support::pinyin_get_pinyin_string), "ni3"); // :2718-2727
    assert_eq!(render(crate::support::pinyin_get_zhuyin_string), "ㄋㄧˇ"); // :2707-2716
    assert_eq!(
        render(crate::support::pinyin_get_luoma_pinyin_string),
        "ni3"
    ); // :2729-2738
    assert_eq!(
        render(crate::support::pinyin_get_secondary_zhuyin_string),
        "ni3"
    ); // :2740-2749
}

/// `pinyin_get_pinyin_strings` never reads `instance` (`pinyin.cpp:2751-2763`);
/// each of `shengmu` and `yunmu` is optional.
#[test]
fn null_instance_pinyin_strings_answer() {
    let _slots = crate::support::key_slot_guard();
    let user = TempUserDir::new("null-instance-pinyin-strings");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let (key, _) = parsed_key_and_rest(instance, "ni3");
    let (mut shengmu, mut yunmu) = (ptr::null_mut(), ptr::null_mut());
    assert!(unsafe {
        crate::support::pinyin_get_pinyin_strings(
            ptr::null_mut(),
            key,
            &raw mut shengmu,
            &raw mut yunmu,
        )
    });
    assert_eq!(crate::support::take_owned_cstr(shengmu), "n");
    assert_eq!(crate::support::take_owned_cstr(yunmu), "i");
    assert!(unsafe {
        crate::support::pinyin_get_pinyin_strings(
            ptr::null_mut(),
            key,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    });
}
