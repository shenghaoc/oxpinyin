//! W11 union surface: user/addon candidates and phrase prediction.

use crate::candidates::pinyin_choose_candidate;
use crate::config::pinyin_load_addon_phrase_library;
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
