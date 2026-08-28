//! W11 union surface: user/addon candidates and phrase prediction.

use crate::candidates::{pinyin_choose_candidate, pinyin_get_candidate};
use crate::config::{pinyin_load_addon_phrase_library, pinyin_unload_addon_phrase_library};
use crate::context::{oxpinyin_test_set_user_bigram, pinyin_init_for_fixtures};
use crate::instance::pinyin_alloc_instance;
use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_end_add_phrases, pinyin_iterator_add_phrase,
};
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::sentence::{
    pinyin_guess_candidates, pinyin_guess_predicted_candidates_with_punctuations,
};
use crate::state::instance_ref;
use crate::test_support::{DEFAULT_SORT, TempSystemDir, TempUserDir, candidate, cstr, open};
use crate::types::lookup_candidate_type_t;

#[test]
fn imported_user_phrase_surfaces_as_a_user_candidate() {
    let user_dir = TempUserDir::new("user-surface");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let iter = pinyin_begin_add_phrases(context, 7);
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("测测").as_ptr(),
        cstr("cece").as_ptr(),
        5,
    ));
    pinyin_end_add_phrases(iter);

    let input = cstr("cece");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 4);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    // SAFETY: live instance; snapshot is valid until the next guess.
    let inst = unsafe { instance_ref(instance) };
    let found = inst
        .candidates
        .iter()
        .find(|c| c.text.as_bytes() == "测测".as_bytes());
    let found = found.expect("imported user phrase must surface");
    assert!(
        found
            .token
            .is_some_and(|token| oxpinyin_user::is_user_token(token.value()))
    );
    assert_eq!(
        found.candidate_type,
        lookup_candidate_type_t::NORMAL_CANDIDATE
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn addon_library_load_is_idempotent_and_surfaces_addon_candidates() {
    let system = TempSystemDir::new("addon-load");
    system.write(
        "interpolation2.text",
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    );
    let user = TempUserDir::new("addon-load-user");
    let context = pinyin_init_for_fixtures(
        cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
        cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
    );
    assert!(!context.is_null());
    assert!(pinyin_load_addon_phrase_library(context, 4));
    assert!(
        !pinyin_load_addon_phrase_library(context, 4),
        "second load of the same index is false"
    );
    assert!(
        !pinyin_load_addon_phrase_library(context, 15),
        "missing library is false"
    );
    // pinyin_unload_addon_phrase_library mirrors the pin (`pinyin.cpp:124-131`):
    // unconditional `true` in range, including for a library that was never
    // loaded, and a reload afterwards must succeed again. The pin's
    // `assert(index < PHRASE_INDEX_LIBRARY_COUNT)` becomes `false` here, per
    // the availability class of docs/findings/compatibility-policy.md.
    assert!(pinyin_unload_addon_phrase_library(context, 4));
    assert!(
        pinyin_unload_addon_phrase_library(context, 4),
        "unloading an already-unloaded library is still true, as upstream"
    );
    assert!(
        !pinyin_unload_addon_phrase_library(context, 16),
        "out of range answers false where the pin asserts"
    );
    assert!(
        pinyin_load_addon_phrase_library(context, 4),
        "reload after unload succeeds"
    );

    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    let input = cstr("erhuang");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 7);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    // SAFETY: live instance.
    let inst = unsafe { instance_ref(instance) };
    assert!(
        inst.candidates.iter().any(|c| {
            c.candidate_type == lookup_candidate_type_t::ADDON_CANDIDATE
                && (c.text.as_bytes() == "二簧".as_bytes()
                    || c.text.as_bytes() == "二黄".as_bytes())
        }),
        "loaded art addon must offer 二簧/二黄 for erhuang"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn chosen_addon_candidate_is_promoted_into_default_nibble_5() {
    let system = TempSystemDir::new("addon-promote");
    system.write(
        "interpolation2.text",
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    );
    let user = TempUserDir::new("addon-promote-user");
    let context = pinyin_init_for_fixtures(
        cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
        cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
    );
    assert!(!context.is_null());
    assert!(pinyin_load_addon_phrase_library(context, 4));

    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    let input = cstr("erhuang");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 7);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));

    // Find the addon candidate: its snapshot index and display text.
    let (index, text) = {
        // SAFETY: live instance; snapshot valid until the next guess.
        let inst = unsafe { instance_ref(instance) };
        let (index, cand) = inst
            .candidates
            .iter()
            .enumerate()
            .find(|(_, c)| c.candidate_type == lookup_candidate_type_t::ADDON_CANDIDATE)
            .expect("art addon offers an ADDON candidate for erhuang");
        (index, cand.text.to_str().expect("UTF-8 text").to_owned())
    };

    // Choose it through the C ABI: this must promote it into default nibble 5.
    let mut cand: *mut crate::types::LookupCandidate = std::ptr::null_mut();
    assert!(pinyin_get_candidate(instance, index as u32, &mut cand));
    assert!(!cand.is_null());
    assert!(pinyin_choose_candidate(instance, 0, cand) > 0);

    // The snapshot candidate is rewritten to a NORMAL candidate at a nibble-5
    // token (`pinyin.cpp:2559-2560`).
    let promoted = {
        // SAFETY: snapshot untouched since the choose (no intervening guess).
        let inst = unsafe { instance_ref(instance) };
        let snapshot = &inst.candidates[index];
        assert_eq!(
            snapshot.candidate_type,
            lookup_candidate_type_t::NORMAL_CANDIDATE,
            "the chosen addon candidate becomes NORMAL"
        );
        let token = snapshot.token.expect("promoted candidate carries a token");
        assert_eq!(
            oxpinyin_user::phrase_index_library_index(token.value()),
            oxpinyin_user::ADDON_DICTIONARY,
            "promoted token lives in default nibble 5"
        );
        token.value()
    };

    // The user store now carries the phrase under nibble 5, with a reading.
    // SAFETY: live instance.
    let inst = unsafe { instance_ref(instance) };
    let store = inst
        .user
        .as_ref()
        .expect("fixture context has a user store");
    assert_eq!(
        store
            .token_for_phrase_in(oxpinyin_user::ADDON_DICTIONARY, &text)
            .expect("store read"),
        Some(promoted),
        "the promoted phrase resolves under nibble 5"
    );
    let phrase = store
        .phrase(promoted)
        .expect("store read")
        .expect("promoted phrase exists");
    assert_eq!(phrase.text(), text);
    let reading = phrase
        .pronunciations()
        .first()
        .expect("promoted phrase copied at least one reading");

    // It surfaces through the default-facade user lookup as a normal candidate.
    let keys: Vec<oxpinyin_core::SyllableKey> = reading
        .keys()
        .iter()
        .map(|k| oxpinyin_core::SyllableKey::from_index(usize::from(*k)).expect("stored key"))
        .collect();
    let lookup = oxpinyin_user::UserLookup::from_store(store).expect("build lookup");
    assert!(
        lookup
            .lookup(&keys)
            .iter()
            .any(|entry| entry.token().value() == promoted && entry.text() == text),
        "promoted phrase is found through the default-facade lookup"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn predicted_candidates_include_trained_bigram_successors() {
    let user_dir = TempUserDir::new("predict");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let first = candidate(instance, "nihao", 0);
    assert!(pinyin_choose_candidate(instance, 0, first) > 0);
    assert!(crate::candidates::pinyin_train(instance, 0));
    // First train already seeds 69, which is above the copied filter of 10.
    for _ in 0..4 {
        assert!(crate::instance::pinyin_reset(instance));
        let again = candidate(instance, "nihao", 0);
        assert!(pinyin_choose_candidate(instance, 0, again) > 0);
        assert!(crate::candidates::pinyin_train(instance, 0));
    }

    let prefix = cstr("你");
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        prefix.as_ptr()
    ));
    // SAFETY: live instance after guess_predicted.
    let inst = unsafe { instance_ref(instance) };
    assert!(
        !inst.candidates.is_empty(),
        "prediction must return at least the prefix suggestions or bigrams"
    );
    assert!(
        inst.candidates.iter().all(|c| {
            matches!(
                c.candidate_type,
                lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE
                    | lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE
            )
        }),
        "你 has no punct.table rows, so the list stays phrase-only"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn predicted_candidates_prepend_punctuation_for_hao() {
    let user_dir = TempUserDir::new("predict-punct");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let prefix = cstr("好");
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        prefix.as_ptr()
    ));
    // SAFETY: live instance after guess_predicted.
    let inst = unsafe { instance_ref(instance) };
    let puncts: Vec<&str> = inst
        .candidates
        .iter()
        .filter(|c| c.candidate_type == lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE)
        .map(|c| c.text.to_str().unwrap_or(""))
        .collect();
    assert_eq!(puncts, ["，", "。"]);
    assert_eq!(
        inst.candidates[0].candidate_type,
        lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE
    );
    assert!(inst.candidates[0].token.is_none());

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn predicted_punctuation_uses_shortest_suffix_first_and_dedups() {
    let user_dir = TempUserDir::new("predict-punct-suffix");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let prefix = cstr("中国");
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        prefix.as_ptr()
    ));
    // SAFETY: live instance after guess_predicted.
    let inst = unsafe { instance_ref(instance) };
    let puncts: Vec<&str> = inst
        .candidates
        .iter()
        .filter(|c| c.candidate_type == lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE)
        .map(|c| c.text.to_str().unwrap_or(""))
        .collect();
    // last-1 国 (，) then last-2 中国 (none). 中 is not a suffix.
    assert_eq!(puncts, ["，"]);

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn predicted_bigram_filter_drops_9_keeps_10() {
    let user_dir = TempUserDir::new("predict-edge");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let iter = pinyin_begin_add_phrases(context, 7);
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("测测").as_ptr(),
        cstr("cece").as_ptr(),
        5,
    ));
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("甲甲").as_ptr(),
        cstr("jiajia").as_ptr(),
        5,
    ));
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("乙乙").as_ptr(),
        cstr("yiyi").as_ptr(),
        5,
    ));
    pinyin_end_add_phrases(iter);

    assert!(oxpinyin_test_set_user_bigram(
        context,
        cstr("测测").as_ptr(),
        cstr("甲甲").as_ptr(),
        9,
    ));
    assert!(oxpinyin_test_set_user_bigram(
        context,
        cstr("测测").as_ptr(),
        cstr("乙乙").as_ptr(),
        10,
    ));

    let prefix = cstr("测测");
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        prefix.as_ptr()
    ));
    // SAFETY: live instance after guess_predicted.
    let inst = unsafe { instance_ref(instance) };
    let bigrams: Vec<&str> = inst
        .candidates
        .iter()
        .filter(|c| c.candidate_type == lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE)
        .map(|c| c.text.to_str().unwrap_or(""))
        .collect();
    assert!(
        !bigrams.contains(&"甲甲"),
        "count 9 must be skipped (pinyin.cpp:2349-2350)"
    );
    assert!(
        bigrams.contains(&"乙乙"),
        "count 10 must be kept (pinyin.cpp:2349-2350)"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// The pin slices the predicted prefix candidate's phrase string from
/// `m_begin = prefix_len` (`pinyin.cpp:1976-1980, 2018-2023`): the user
/// already committed the prefix, so the suggestion must carry only the
/// remainder — both frontends commit the library string verbatim
/// (`docs/findings/uncovered-surface-differentials.md` B1).
#[test]
fn predicted_prefix_candidates_slice_the_prefix_from_the_text() {
    let user_dir = TempUserDir::new("predict-slice");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let prefix = cstr("\u{4e2d}"); // 中
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        prefix.as_ptr()
    ));
    // SAFETY: live instance after guess_predicted.
    let inst = unsafe { instance_ref(instance) };
    let suggestions: Vec<&str> = inst
        .candidates
        .iter()
        .filter(|c| c.candidate_type == lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE)
        .map(|c| c.text.to_str().unwrap_or(""))
        .collect();
    assert_eq!(
        suggestions,
        ["\u{56fd}"],
        "中国 suggests 国: the prefix 中 is sliced, not part of the text"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}
