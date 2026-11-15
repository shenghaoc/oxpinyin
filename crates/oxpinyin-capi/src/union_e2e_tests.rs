//! W11 union surface: user/addon candidates and phrase prediction.

use crate::candidates::{
    pinyin_choose_candidate, pinyin_get_candidate, pinyin_get_candidate_string,
    pinyin_get_candidate_type,
};
use crate::config::{pinyin_load_addon_phrase_library, pinyin_unload_addon_phrase_library};
use crate::context::{oxpinyin_test_set_user_bigram, pinyin_init};
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
        .find(|c| c.text_bytes() == "测测".as_bytes());
    let found = found.expect("imported user phrase must surface");
    assert!(
        found
            .token
            .get()
            .is_some_and(|token| oxpinyin_user::is_user_token(token.value()))
    );
    assert_eq!(
        found.candidate_type(),
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
    let context = pinyin_init(
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
    assert!(
        !pinyin_load_addon_phrase_library(context, 16),
        "out-of-range load answers false, as the pin's own guard does (pinyin.cpp:479-480)"
    );
    // pinyin_unload_addon_phrase_library mirrors the pin (`pinyin.cpp:497-503`):
    // unconditional `true` in range, including for a library that was never
    // loaded, and a reload afterwards must succeed again. The pin's
    // `assert(index < PHRASE_INDEX_LIBRARY_COUNT)` (`:499`) aborts the
    // process;
    // `e2e_tests::null_context_unload_addon_phrase_library_aborts_on_index_16`
    // holds it.
    assert!(pinyin_unload_addon_phrase_library(context, 4));
    assert!(
        pinyin_unload_addon_phrase_library(context, 4),
        "unloading an already-unloaded library is still true, as upstream"
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
        inst.state.candidates.iter().any(|c| {
            c.candidate_type() == lookup_candidate_type_t::ADDON_CANDIDATE
                && (c.text_bytes() == "二簧".as_bytes() || c.text_bytes() == "二黄".as_bytes())
        }),
        "loaded art addon must offer 二簧/二黄 for erhuang"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[rstest::rstest]
fn chosen_addon_candidate_is_promoted_into_default_nibble_5(
    #[values(false, true)] finalized: bool,
) {
    let system = TempSystemDir::new("addon-promote");
    system.write(
        "interpolation2.text",
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    );
    let user = TempUserDir::new("addon-promote-user");
    let context = pinyin_init(
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
            .find(|(_, c)| c.candidate_type() == lookup_candidate_type_t::ADDON_CANDIDATE)
            .expect("art addon offers an ADDON candidate for erhuang");
        (index, cand.text_str().expect("UTF-8 text").to_owned())
    };

    // Choose it through the C ABI: this must promote it into default nibble 5.
    let mut cand: *mut crate::types::LookupCandidate = std::ptr::null_mut();
    assert!(pinyin_get_candidate(
        instance,
        u32::try_from(index).expect("small candidate index"),
        &raw mut cand
    ));
    assert!(!cand.is_null());
    if finalized {
        crate::test_support::finalize_public_context(context, instance);
    }
    assert!(pinyin_choose_candidate(instance, 0, cand) > 0);
    let mut kind = lookup_candidate_type_t::ADDON_CANDIDATE;
    assert!(pinyin_get_candidate_type(instance, cand, &raw mut kind));
    assert_eq!(kind, lookup_candidate_type_t::NORMAL_CANDIDATE);
    let mut rendered = std::ptr::null();
    assert!(pinyin_get_candidate_string(
        instance,
        cand,
        &raw mut rendered
    ));
    assert!(!rendered.is_null());
    // SAFETY: the original candidate still owns a terminated display string.
    let rendered = unsafe { std::ffi::CStr::from_ptr(rendered) };
    assert_eq!(rendered.to_bytes(), text.as_bytes());

    // The snapshot candidate is rewritten to a NORMAL candidate at a nibble-5
    // token (`pinyin.cpp:2559-2560`).
    let promoted = {
        // SAFETY: snapshot untouched since the choose (no intervening guess).
        let inst = unsafe { instance_ref(instance) };
        let snapshot = &inst.state.candidates[index];
        assert_eq!(
            snapshot.candidate_type(),
            lookup_candidate_type_t::NORMAL_CANDIDATE,
            "the chosen addon candidate becomes NORMAL"
        );
        let token = snapshot
            .token
            .get()
            .expect("promoted candidate carries a token");
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
    let store = inst.core.user;
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
    assert_eq!(
        phrase.words(),
        text.chars().map(u32::from).collect::<Vec<_>>()
    );
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
    let lookup = store.lookup();
    assert!(
        lookup
            .lookup(&keys)
            .iter()
            .any(|entry| entry.token().value() == promoted
                && entry.text().as_str() == Some(text.as_str())),
        "promoted phrase is found through the default-facade lookup"
    );

    crate::instance::pinyin_free_instance(instance);
    if !finalized {
        crate::context::pinyin_fini(context);
    }
}

#[test]
fn predicted_candidates_include_trained_bigram_successors() {
    let user_dir = TempUserDir::new("predict");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let first = candidate(instance, "nihao", 0);
    assert!(pinyin_choose_candidate(instance, 0, first) > 0);
    assert!(crate::sentence::pinyin_guess_sentence(instance));
    assert!(crate::candidates::pinyin_train(instance, 0));
    // First train already seeds 69, which is above the copied filter of 10.
    for _ in 0..4 {
        assert!(crate::instance::pinyin_reset(instance));
        let again = candidate(instance, "nihao", 0);
        assert!(pinyin_choose_candidate(instance, 0, again) > 0);
        assert!(crate::sentence::pinyin_guess_sentence(instance));
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
        !inst.state.candidates.is_empty(),
        "prediction must return at least the prefix suggestions or bigrams"
    );
    assert!(
        inst.state.candidates.iter().all(|c| {
            matches!(
                c.candidate_type(),
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
        .filter(|c| c.candidate_type() == lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE)
        .map(|c| c.text_str().unwrap_or(""))
        .collect();
    assert_eq!(puncts, ["，", "。"]);
    assert_eq!(
        inst.state.candidates[0].candidate_type(),
        lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE
    );
    assert!(inst.state.candidates[0].token.get().is_none());

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
        .filter(|c| c.candidate_type() == lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE)
        .map(|c| c.text_str().unwrap_or(""))
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
        .filter(|c| c.candidate_type() == lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE)
        .map(|c| c.text_str().unwrap_or(""))
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
/// remainder — both frontends commit the library string verbatim.
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
        .filter(|c| c.candidate_type() == lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE)
        .map(|c| c.text_str().unwrap_or(""))
        .collect();
    assert_eq!(
        suggestions,
        ["\u{56fd}"],
        "中国 suggests 国: the prefix 中 is sliced, not part of the text"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// The engineered order-flip probe for the Tier-C unigram overlay — an
/// ARTIFICIAL construction, labeled as such: the fixture tables'
/// amplified-law total is small, so a large
/// `pinyin_token_add_unigram_frequency` delta on one predicted
/// candidate visibly reorders the plain-variant list. On the real
/// model20 tables the same mechanism shifts nothing observable after
/// the ×2²⁴ truncation (the real-table differential pins retvals and
/// freq reads instead). White-box because the prediction surface's
/// display text is prefix-sliced (the B1 law) — no external caller can
/// map a candidate row back to its full-phrase token, so the token
/// comes from the snapshot directly.
#[test]
fn tier_c_overlay_delta_flips_predicted_order_on_fixture_scale() {
    let user_dir = crate::test_support::TempUserDir::new("tier-c-flip");
    let (context, instance) =
        crate::test_support::open(user_dir.path.to_str().expect("UTF-8 path"));

    // Plant the trained bigram (first-seed 69 clears the filter of 10).
    let nihao = std::ffi::CString::new("nihao").expect("no interior NUL");
    assert_eq!(
        crate::parse::pinyin_parse_more_full_pinyins(instance, nihao.as_ptr()),
        5
    );
    assert!(crate::sentence::pinyin_guess_candidates(instance, 0, 0x1e));
    let mut candidate: *mut crate::types::LookupCandidate = std::ptr::null_mut();
    assert!(pinyin_get_candidate(instance, 0, &raw mut candidate));
    assert!(pinyin_choose_candidate(instance, 0, candidate) > 0);
    assert!(crate::sentence::pinyin_guess_sentence(instance));
    assert!(crate::candidates::pinyin_train(instance, 0));

    // The plain variant for prefix 你: rows are the sliced suggestion
    // texts 们 (from 你们) and 好 (from 你好).
    let prefix = std::ffi::CString::new("你").expect("no interior NUL");
    assert!(crate::predict::pinyin_guess_predicted_candidates(
        instance,
        prefix.as_ptr()
    ));
    // SAFETY: live instance after guess_predicted.
    let inst = unsafe { instance_ref(instance) };
    assert!(
        inst.state.candidates.len() >= 2,
        "both suggestion rows present"
    );
    let before: Vec<String> = inst
        .candidates
        .iter()
        .map(|row| String::from_utf8_lossy(row.text_bytes()).into_owned())
        .collect();
    let leader_before = before[0].clone();
    let trailing_token = inst
        .candidates
        .iter()
        .skip(1)
        .find_map(|row| row.token.get())
        .expect("no trailing row carries a token");

    // The huge engineered delta through the ABI write path.
    assert!(crate::dict::pinyin_token_add_unigram_frequency(
        instance,
        trailing_token.value(),
        50_000_000
    ));

    assert!(crate::predict::pinyin_guess_predicted_candidates(
        instance,
        prefix.as_ptr()
    ));
    // SAFETY: live instance after guess_predicted.
    let inst = unsafe { instance_ref(instance) };
    let after: Vec<String> = inst
        .candidates
        .iter()
        .map(|row| String::from_utf8_lossy(row.text_bytes()).into_owned())
        .collect();
    assert_ne!(
        after[0], leader_before,
        "the engineered delta flipped the predicted order: {before:?} -> {after:?}"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// An addon row's choose is refused after a shortening reparse, once the
/// promotion has run; the full input is parsed again and the same handle is
/// chosen again. The pin's first choose rewrote the row to its promoted
/// `m_token` in place (`pinyin.cpp:2559-2561`), so the second forces that
/// token over the row's span and answers `0 + 7` (`:2577-2590`), and
/// `pinyin_train` adds `69 * 7 = 483` to its unigram: the bigram from
/// `sentence_start` is new, so the seed is the initial one
/// (`phonetic_lookup.h:858-929`).
#[test]
fn a_repeat_choose_of_a_refused_promoted_row_forces_the_promoted_token() {
    const ADDON: u32 = 0x0400_0001;
    const PROMOTED: u32 = 0x0500_0001;
    let system = TempSystemDir::new("promoted-repeat");
    system.write(
        "interpolation2.text",
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    );
    let user = TempUserDir::new("promoted-repeat-user");
    let context = pinyin_init(
        cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
        cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
    );
    assert!(!context.is_null());
    assert!(pinyin_load_addon_phrase_library(context, 4));
    let instance = pinyin_alloc_instance(context);
    let input = cstr("erhuang");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 7);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let index = {
        // SAFETY: live instance; snapshot valid until the next guess.
        let inst = unsafe { instance_ref(instance) };
        inst.candidates
            .iter()
            .position(|c| {
                c.candidate_type() == lookup_candidate_type_t::ADDON_CANDIDATE
                    && c.token.get().map(oxpinyin_core::PhraseToken::value) == Some(ADDON)
            })
            .expect("the 二簧 ADDON row")
    };
    let mut cand: *mut crate::types::LookupCandidate = std::ptr::null_mut();
    assert!(pinyin_get_candidate(
        instance,
        u32::try_from(index).expect("small"),
        &raw mut cand
    ));
    let short = cstr("er");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, short.as_ptr()), 2);
    assert_eq!(pinyin_choose_candidate(instance, 0, cand), 0);
    // SAFETY: as above.
    let promoted = unsafe { instance_ref(instance) }.state.candidates[index]
        .token
        .get()
        .map(oxpinyin_core::PhraseToken::value);
    assert_eq!(promoted, Some(PROMOTED));
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 7);
    assert_eq!(pinyin_choose_candidate(instance, 0, cand), 7);
    // SAFETY: as above.
    let tokens: Vec<u32> = unsafe { instance_ref(instance) }
        .core
        .session
        .selected_tokens()
        .iter()
        .map(|token| token.value())
        .collect();
    assert_eq!(tokens, [PROMOTED]);
    assert!(crate::sentence::pinyin_guess_sentence(instance));
    let mut sentence: *mut std::ffi::c_char = std::ptr::null_mut();
    assert!(crate::sentence::pinyin_get_sentence(
        instance,
        0,
        &raw mut sentence
    ));
    // SAFETY: a NUL-terminated GLib string the caller owns.
    let text = unsafe { std::ffi::CStr::from_ptr(sentence) }
        .to_str()
        .expect("UTF-8")
        .to_owned();
    // SAFETY: freed once.
    unsafe { glib_sys::g_free(sentence.cast()) };
    assert_eq!(text, "二簧");
    let unigram = |token: u32| {
        let mut freq = 0;
        let found = crate::dict::pinyin_token_get_unigram_frequency(instance, token, &raw mut freq);
        (found, freq)
    };
    assert_eq!(unigram(PROMOTED), (true, 1));
    assert!(crate::candidates::pinyin_train(instance, 0));
    assert_eq!(unigram(PROMOTED), (true, 1 + 483));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// With row 5 `NOT_USED`, the pin reads an unwritten token (074a2219
/// phrase_index.cpp:610-613, pinyin.cpp:2541-2545). The class (b) outcome
/// leaves the addon candidate and default sub-indices as they were.
#[test]
fn choosing_an_addon_without_sub_index_5_skips_promotion() {
    let system = TempSystemDir::new("addon-promote-not-used");
    let conf = std::fs::read_to_string(system.path.join("table.conf")).expect("table.conf");
    let row = "default ADDON_DICTIONARY NULL NULL addon.bin USER_FILE";
    assert!(conf.contains(row));
    system.write(
        "table.conf",
        &conf.replace(row, "default ADDON_DICTIONARY NULL NULL NULL NOT_USED"),
    );
    system.write(
        "interpolation2.text",
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    );
    let user = TempUserDir::new("addon-promote-not-used-user");
    let context = pinyin_init(
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
    let (index, token, text, keys, total) = {
        // SAFETY: live instance; the snapshot is valid until the next guess.
        let inst = unsafe { instance_ref(instance) };
        let store = inst.core.user;
        assert_eq!(store.addon_promotion_range_end(), None);
        let (index, cand) = inst
            .candidates
            .iter()
            .enumerate()
            .find(|(_, c)| c.candidate_type() == lookup_candidate_type_t::ADDON_CANDIDATE)
            .expect("art addon offers an ADDON candidate for erhuang");
        let token = cand.token.get().expect("addon token");
        let item = inst
            .core
            .dict
            .addon_phrase_item(token.value())
            .expect("addon item");
        let view = oxpinyin_data::phrase_library::PhraseItemView::from_bytes(&item)
            .expect("addon item bytes");
        let keys: Vec<_> = view
            .pronunciation(0)
            .expect("first reading")
            .keys
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                let packed = oxpinyin_core::ChewingKey::from_packed(u16::from_le_bytes(*pair));
                oxpinyin_core::SyllableKey::from_text(packed.pinyin_spelling()).expect("stored key")
            })
            .collect();
        (
            index,
            token,
            cand.text_str().expect("addon text").to_owned(),
            keys,
            store.unigram_total().unwrap(),
        )
    };
    let mut cand = std::ptr::null_mut();
    assert!(pinyin_get_candidate(
        instance,
        index.try_into().unwrap(),
        &raw mut cand
    ));
    assert!(pinyin_choose_candidate(instance, 0, cand) > 0);

    let mut kind = lookup_candidate_type_t::NORMAL_CANDIDATE;
    assert!(pinyin_get_candidate_type(instance, cand, &raw mut kind));
    assert_eq!(kind, lookup_candidate_type_t::ADDON_CANDIDATE);
    {
        // SAFETY: the live instance still owns this snapshot.
        let inst = unsafe { instance_ref(instance) };
        assert_eq!(inst.state.candidates[index].token.get(), Some(token));
        let store = inst.core.user;
        assert_eq!(store.addon_promotion_range_end(), None);
        assert!(
            store
                .phrase_table_tokens_for_text(&text)
                .unwrap()
                .is_empty()
        );
        assert!(store.lookup().lookup(&keys).is_empty());
        assert!(store.phrases().unwrap().is_empty());
        assert_eq!(store.unigram_total().unwrap(), total);
        assert!(!store.is_modified());
    }
    assert!(!crate::context::pinyin_save(context));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
    let files: Vec<_> = std::fs::read_dir(&user.path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files, [std::ffi::OsString::from("user.conf")]);
}

/// The packed key of `spelling` at `tone`, as an addon item stores it.
fn packed_key(spelling: &str, tone: u8) -> u16 {
    oxpinyin_core::ChewingKey::from_pinyin(spelling)
        .expect("frozen syllable")
        .with_tone(tone)
        .to_packed()
}

/// A private system whose addon library 4 holds 二簧 with two toned
/// readings, an item whose first word is the surrogate 0xD800, and 三次 indexed under `san ci` with no reading in its body.
pub(crate) fn promotion_system(parent: &std::path::Path) -> std::path::PathBuf {
    use crate::test_support::{AddonItem, addon_system, system_dir};
    let readings: [(&[u16], u32); 2] = [
        (&[packed_key("er", 2), packed_key("huang", 2)], 7),
        (&[packed_key("er", 4), packed_key("huang", 1)], 3),
    ];
    let system = addon_system(
        &system_dir(),
        parent,
        &[
            AddonItem {
                text: &[0x4e8c, 0x7c27],
                readings: &readings,
                indexed_as: &[packed_key("er", 0), packed_key("huang", 0)],
                unigram: 11,
            },
            AddonItem {
                text: &[0xD800, 0x4e8c],
                readings: &[(&[packed_key("ba", 1), packed_key("ma", 3)], 5)],
                indexed_as: &[packed_key("ba", 0), packed_key("ma", 0)],
                unigram: 5,
            },
            AddonItem {
                text: &[0x4e09, 0x6b21],
                readings: &[],
                indexed_as: &[packed_key("san", 0), packed_key("ci", 0)],
                unigram: 9,
            },
        ],
    );
    std::fs::write(
        system.join("interpolation2.text"),
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    )
    .expect("interpolation2.text");
    system
}

/// Chooses the only addon row of `input` and answers the context, the
/// instance and the promoted token.
fn choose_addon_row(
    system: &std::path::Path,
    user: &std::path::Path,
    input: &str,
) -> (
    *mut crate::types::PinyinContext,
    *mut crate::types::PinyinInstance,
    u32,
) {
    let context = pinyin_init(
        cstr(system.to_str().expect("UTF-8 path")).as_ptr(),
        cstr(user.to_str().expect("UTF-8 path")).as_ptr(),
    );
    assert!(!context.is_null());
    assert!(pinyin_load_addon_phrase_library(context, 4));
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    let text = cstr(input);
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, text.as_ptr()),
        input.len()
    );
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let index = {
        // SAFETY: live instance; the snapshot is valid until the next guess.
        let inst = unsafe { instance_ref(instance) };
        inst.candidates
            .iter()
            .position(|c| c.candidate_type() == lookup_candidate_type_t::ADDON_CANDIDATE)
            .expect("the crafted addon offers an ADDON candidate")
    };
    let mut cand: *mut crate::types::LookupCandidate = std::ptr::null_mut();
    assert!(pinyin_get_candidate(
        instance,
        u32::try_from(index).expect("small candidate index"),
        &raw mut cand
    ));
    assert!(pinyin_choose_candidate(instance, 0, cand) > 0);
    let mut kind = lookup_candidate_type_t::ADDON_CANDIDATE;
    assert!(pinyin_get_candidate_type(instance, cand, &raw mut kind));
    assert_eq!(kind, lookup_candidate_type_t::NORMAL_CANDIDATE);
    // `pinyin.cpp:2561-2562`: the promoted token is the new default token.
    let promoted = {
        // SAFETY: no guess since the choose.
        let inst = unsafe { instance_ref(instance) };
        inst.state.candidates[index]
            .token
            .get()
            .expect("the rewritten candidate has a token")
            .value()
    };
    (context, instance, promoted)
}

/// The tokens `pinyin_lookup_tokens` finds for `phrase`.
fn looked_up_tokens(instance: *mut crate::types::PinyinInstance, phrase: &str) -> Vec<u32> {
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    let text = cstr(phrase);
    let found = crate::dict::pinyin_lookup_tokens(instance, text.as_ptr(), array.cast());
    // SAFETY: a live GArray of four-byte elements.
    let tokens = unsafe {
        let view = &*array;
        std::slice::from_raw_parts(view.data.cast::<u32>(), view.len as usize).to_vec()
    };
    assert_eq!(found, !tokens.is_empty());
    // SAFETY: allocated above, freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    tokens
}

/// Promotion copies each stored reading with its tones
/// (074a2219 `pinyin.cpp:2547-2558`): the promoted token answers the same
/// packed words the addon item stores.
#[test]
fn a_promoted_addon_item_keeps_every_toned_reading() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = promotion_system(scratch.path());
    let user = TempUserDir::new("addon-promote-toned");
    let (context, instance, promoted) = choose_addon_row(&system, &user.path, "erhuang");
    assert_eq!(
        oxpinyin_user::phrase_index_library_index(promoted),
        oxpinyin_user::ADDON_DICTIONARY
    );
    assert_eq!(looked_up_tokens(instance, "二簧"), [promoted]);

    let mut count = 0;
    assert!(crate::dict::pinyin_token_get_n_pronunciation(
        instance,
        promoted,
        &raw mut count
    ));
    assert_eq!(count, 2);
    let stored = [
        [packed_key("er", 2), packed_key("huang", 2)],
        [packed_key("er", 4), packed_key("huang", 1)],
    ];
    for (nth, keys) in stored.iter().enumerate() {
        // SAFETY: a fresh GArray of two-byte elements, freed below.
        let array = unsafe { glib_sys::g_array_new(0, 0, 2) };
        assert!(crate::dict::pinyin_token_get_nth_pronunciation(
            instance,
            promoted,
            u32::try_from(nth).expect("small index"),
            array.cast()
        ));
        // SAFETY: a live GArray of two-byte elements.
        let words = unsafe {
            let view = &*array;
            std::slice::from_raw_parts(view.data.cast::<u16>(), view.len as usize).to_vec()
        };
        // SAFETY: allocated above, freed once.
        unsafe { glib_sys::g_array_free(array, 1) };
        assert_eq!(words, keys, "reading {nth} keeps its tones");
    }
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// An indexed addon item whose body holds no reading skips the pinyin-index
/// loop and is still copied and rewritten (`pinyin.cpp:2548-2562`).
#[test]
fn an_addon_item_with_no_reading_is_still_promoted() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = promotion_system(scratch.path());
    let user = TempUserDir::new("addon-promote-bare");
    let (context, instance, promoted) = choose_addon_row(&system, &user.path, "sanci");
    assert_eq!(
        oxpinyin_user::phrase_index_library_index(promoted),
        oxpinyin_user::ADDON_DICTIONARY
    );
    assert_eq!(looked_up_tokens(instance, "三次"), [promoted]);
    let mut text = std::ptr::null_mut();
    let mut length = 0;
    assert!(crate::dict::pinyin_token_get_phrase(
        instance,
        promoted,
        &raw mut length,
        &raw mut text
    ));
    assert_eq!(length, 2);
    // SAFETY: a NUL-terminated GLib string the caller owns.
    let rendered = unsafe { std::ffi::CStr::from_ptr(text) }
        .to_bytes()
        .to_vec();
    assert_eq!(rendered, "三次".as_bytes());
    // SAFETY: freed once.
    unsafe { glib_sys::g_free(text.cast()) };
    let mut count = 7;
    assert!(crate::dict::pinyin_token_get_n_pronunciation(
        instance,
        promoted,
        &raw mut count
    ));
    assert_eq!(count, 0);
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// The pin copies an item's words as stored (`pinyin.cpp:2555-2558`), a
/// word that is no Unicode scalar among them.
#[test]
fn an_addon_item_with_a_non_scalar_word_is_promoted_as_stored() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = promotion_system(scratch.path());
    let user = TempUserDir::new("addon-promote-non-scalar");
    let (context, instance, promoted) = choose_addon_row(&system, &user.path, "bama");
    assert_eq!(
        oxpinyin_user::phrase_index_library_index(promoted),
        oxpinyin_user::ADDON_DICTIONARY
    );
    {
        // SAFETY: live instance; no guess since the choose.
        let inst = unsafe { instance_ref(instance) };
        assert_eq!(
            inst.core.user.with_user_item(promoted, |item| item
                .phrase_codes()
                .collect::<Option<Vec<u32>>>()),
            Some(Some(vec![0xD800, 0x4e8c])),
            "the stored words, exactly"
        );
    }
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}
