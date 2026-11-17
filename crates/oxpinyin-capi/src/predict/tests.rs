#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::test_support::{close, open_at, wrap_facade_total_to_zero};

use super::select_prefix_rows;

use crate::candidates::{pinyin_choose_candidate, pinyin_get_candidate};

use crate::config::pinyin_set_options;

use crate::e2e_tests::{capture_rows, import_batch};

use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_end_add_phrases, pinyin_iterator_add_phrase,
};

use crate::predict::pinyin_guess_predicted_candidates;

use crate::sentence::pinyin_guess_predicted_candidates_with_punctuations;

use crate::state::instance_ref;

use crate::test_support::{
    DEFAULT_SORT, TempUserDir, assert_aborts, cstr, gram_bytes, lookup, open, plant_bigram,
};

use crate::types::{PinyinInstance, lookup_candidate_type_t};

use oxpinyin_core::PhraseText;

/// Predicted bigram rows on a zero facade total abort in the scorer
/// (register row 61).
#[test]
fn predicted_bigram_rows_abort_on_a_zero_facade_total() {
    assert_aborts(
        "predict::tests::predicted_bigram_rows_abort_on_a_zero_facade_total",
        || {
            let user = TempUserDir::new("row58-total-zero");
            let (context, instance) = open_at(&user.path);
            let (ni, hao) = (lookup(instance, "你"), lookup(instance, "好")[0]);
            close(context, instance);
            let gram = gram_bytes(20, &[(hao, 20)]);
            plant_bigram(
                &user.path,
                ni.iter()
                    .map(|token| (token.to_le_bytes().to_vec(), gram.clone()))
                    .collect(),
            );
            let (_context, instance) = open_at(&user.path);
            wrap_facade_total_to_zero(instance);
            unsafe { pinyin_guess_predicted_candidates(instance, c"你".as_ptr()) };
        },
    );
}

fn text(phrase: &str) -> oxpinyin_core::PhraseText {
    oxpinyin_core::PhraseText::from(phrase)
}

/// `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN`.
const OPTIONS: u32 = 0x18a;

/// `DYNAMIC_ADJUST`.
const DYNAMIC_ADJUST: u32 = 1 << 9;

/// The texts of the rows `pinyin_guess_predicted_candidates` lists
/// after `prefix`.
fn predicted_texts(instance: *mut PinyinInstance, prefix: &std::ffi::CStr) -> Vec<String> {
    assert!(unsafe { super::pinyin_guess_predicted_candidates(instance, prefix.as_ptr()) });
    capture_rows(instance)
        .into_iter()
        .map(|(_, text, _)| text)
        .collect()
}

/// `_compute_predicted_bigram_candidates` keeps one `PhraseItem` for
/// the whole walk (`pinyin.cpp:2342` at the pin `074a2219`). It skips a
/// token only on `ERROR_NO_SUB_PHRASE_INDEX` (`:2352-2355`); a token
/// with no item is compared by the length the previous read left
/// (`:2357-2358`) and appended with its own token (`:2360-2363`). The
/// item starts empty and carries over from the longer pass to the
/// shorter one (`:2344`).
#[test]
fn a_bigram_token_with_no_item_rides_on_the_previous_items_length() {
    use super::{ItemRead, select_bigram_rows};
    let (one, two) = (text("好"), text("你好"));
    let rows = |reads: &[(u32, ItemRead)]| -> Vec<(u32, bool)> {
        select_bigram_rows(reads)
            .into_iter()
            .map(|(token, text)| (token, text.is_some()))
            .collect()
    };
    // Before any read the cached item has length zero: no row.
    assert_eq!(rows(&[(9, ItemRead::NoItem)]), []);
    // After a one-character item, the token with no item is a row of
    // the one-character pass.
    assert_eq!(
        rows(&[(1, ItemRead::Item(one.clone())), (9, ItemRead::NoItem)]),
        [(1, true), (9, false)]
    );
    // The longer pass ends on the one-character item, so the shorter
    // pass starts with it cached: the token with no item comes first.
    assert_eq!(
        rows(&[(9, ItemRead::NoItem), (1, ItemRead::Item(one.clone()))]),
        [(9, false), (1, true)]
    );
    // A two-character item before it puts it in the longer pass; the
    // shorter pass then reads the same item again and skips both.
    assert_eq!(
        rows(&[(2, ItemRead::Item(two.clone())), (9, ItemRead::NoItem)]),
        [(2, true), (9, false)]
    );
    // A library with no sub-index is skipped and leaves the cached item.
    assert_eq!(
        rows(&[
            (1, ItemRead::Item(one)),
            (8, ItemRead::NoSubIndex),
            (9, ItemRead::NoItem),
            (2, ItemRead::Item(two)),
        ]),
        [(2, true), (1, true), (9, false)]
    );
}

#[rstest::rstest]
#[case::plain("你", false, false, true)]
#[case::missing("🦀", false, false, false)]
#[case::punctuated("你", true, false, true)]
#[case::borrowed("", false, true, false)]
#[case::borrowed_punctuated("", true, true, true)]
fn prediction_replaces_every_row_with_a_spanless_one(
    #[case] prefix: &str,
    #[case] punctuated: bool,
    #[case] borrowed: bool,
    #[case] accepted: bool,
) {
    let user = TempUserDir::new("prediction-window");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        unsafe { crate::parse::pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()) },
        5
    );
    assert!(unsafe { crate::sentence::pinyin_guess_candidates(instance, 2, DEFAULT_SORT) });
    // SAFETY: the live instance is read between serialized ABI calls.
    let prior_spans = unsafe { crate::state::instance_ref(instance) }
        .state
        .candidates
        .iter()
        .any(|row| (row.begin, row.end) != (0, 0));
    assert!(prior_spans);
    let owned = cstr(prefix);
    let prefix = if borrowed {
        let mut candidate = std::ptr::null_mut();
        let mut text = std::ptr::null();
        assert!(unsafe {
            crate::candidates::pinyin_get_candidate(instance, 0, &raw mut candidate)
        });
        assert!(unsafe {
            crate::candidates::pinyin_get_candidate_string(instance, candidate, &raw mut text)
        });
        assert!(!text.is_null());
        text
    } else {
        owned.as_ptr()
    };
    let answer = if punctuated {
        unsafe {
            crate::sentence::pinyin_guess_predicted_candidates_with_punctuations(instance, prefix)
        }
    } else {
        unsafe { super::pinyin_guess_predicted_candidates(instance, prefix) }
    };
    assert_eq!(answer, accepted);
    // The pin frees its former candidate list before reading prefixes
    // (pinyin.cpp:2420-2427, 2459), and every row that replaces it has
    // the default `m_begin = m_end = 0` (`:114`), even when the
    // snapshot is left empty.
    // SAFETY: the live instance is read between serialized ABI calls.
    let spanless = unsafe { crate::state::instance_ref(instance) }
        .state
        .candidates
        .iter()
        .all(|row| (row.begin, row.end) == (0, 0));
    assert!(spanless);
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

#[test]
fn predicted_sort_is_stable_and_uses_wrapping_frequency_after_length() {
    use super::{Predicted, sort_predicted};
    use crate::types::lookup_candidate_type_t;
    use oxpinyin_core::PhraseText;
    let mut items: Vec<_> = [(1, 2, 1), (2, 3, 0), (3, 2, u32::MAX), (4, 2, 1), (5, 2, 0)]
        .into_iter()
        .map(|(token, phrase_length, frequency)| Predicted {
            text: PhraseText::from("test"),
            phrase_length,
            token,
            candidate_type: lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE,
            frequency,
        })
        .collect();
    sort_predicted(&mut items);
    assert_eq!(
        items.iter().map(|i| i.token).collect::<Vec<_>>(),
        [2, 1, 4, 5, 3]
    );
}

#[test]
fn a_sole_missing_suggestion_survives_the_length_gate() {
    let (kept, sole) = select_prefix_rows(vec![(7, None)], 5, 0);
    assert!(kept.is_empty());
    assert_eq!(sole, Some(7));
}

#[test]
fn a_missing_suggestion_with_any_other_row_adds_no_candidate() {
    // A live row and the missing row: two pre-dedup rows, class (b).
    let (kept, sole) =
        select_prefix_rows(vec![(1, Some(PhraseText::from("abc"))), (2, None)], 5, 0);
    assert_eq!(kept.len(), 1);
    assert_eq!(sole, None);
    // A preexisting bigram row is enough even without a live prefix row.
    let (kept, sole) = select_prefix_rows(vec![(2, None)], 5, 1);
    assert!(kept.is_empty());
    assert_eq!(sole, None);
    // Two missing rows also reach the NULL comparison.
    let (kept, sole) = select_prefix_rows(vec![(2, None), (3, None)], 5, 0);
    assert!(kept.is_empty());
    assert_eq!(sole, None);
}

#[test]
fn the_stale_length_skips_a_missing_suggestion_after_a_skipped_read() {
    // The long live row is itself skipped, but its successful read still
    // updates the loop-wide length, so the following missing row is
    // skipped too rather than becoming a sole NULL candidate.
    let (kept, sole) =
        select_prefix_rows(vec![(1, Some(PhraseText::from("abcdef"))), (2, None)], 5, 0);
    assert!(kept.is_empty());
    assert_eq!(sole, None);
}

#[test]
fn a_short_live_row_after_the_stale_skip_still_replaces_the_gate() {
    // The skipped long row leaves its own length; the missing row is
    // skipped on it. A later short successful read replaces the gate and
    // is appended normally.
    let (kept, sole) = select_prefix_rows(
        vec![
            (1, Some(PhraseText::from("abcdef"))),
            (2, None),
            (3, Some(PhraseText::from("ab"))),
        ],
        5,
        0,
    );
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].0, 3);
    assert_eq!(sole, None);
}

/// The predicted scorer passes no predecessor, so neither DYNAMIC_ADJUST
/// nor the bigram counts move a row (`pinyin.cpp:2418`, `:2438`): two
/// successors of 你 rank the same whichever has the larger count.
#[test]
fn predicted_rows_ignore_dynamic_adjust_and_the_bigram_counts() {
    let user = TempUserDir::new("predict-bigram-terms");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let [ni, hao, zhong] = ["你", "好", "中"].map(|text| lookup(instance, text)[0]);
    let mut lists = Vec::new();
    for (hao_count, zhong_count) in [(1000, 20), (20, 1000)] {
        // SAFETY: the live context has no reader retained across this write.
        let ctx = unsafe { crate::state::context_mut(context) };
        let store = ctx.core.user_store_mut();
        store.set_bigram_count(ni, hao, hao_count).expect("set");
        store.set_bigram_count(ni, zhong, zhong_count).expect("set");
        for dynamic in [0, DYNAMIC_ADJUST] {
            assert!(unsafe { pinyin_set_options(context, OPTIONS | dynamic) });
            lists.push(predicted_texts(instance, c"你"));
        }
    }
    assert!(
        capture_rows(instance).iter().any(|(kind, text, _)| {
            *kind == lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE && text == "中"
        }),
        "the bigram rows are listed"
    );
    assert!(lists.iter().all(|list| *list == lists[0]), "{lists:?}");
    close(context, instance);
}

/// Prefix rows alone never reach the zero-total assert, which gates the
/// bigram rows only: on a facade total wrapped to zero the prediction
/// answers true with its rows.
#[test]
fn prefix_rows_rank_on_a_zero_facade_total() {
    let user = TempUserDir::new("predict-prefix-zero-total");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let before = predicted_texts(instance, c"你");
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let total = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    let total = u32::try_from(total).expect("a guint32 total");
    assert!(unsafe {
        crate::dict::pinyin_token_add_unigram_frequency(instance, 0x0100_0001, total.wrapping_neg())
    });
    let mut after = predicted_texts(instance, c"你");
    after.sort();
    let mut expected = before;
    expected.sort();
    assert_eq!(after, expected);
    close(context, instance);
}

/// A system-library import ranks by its live item count, runtime
/// additions included, against a user-library row.
#[test]
fn a_system_library_import_ranks_by_its_live_count() {
    let user = TempUserDir::new("predict-system-import");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        import_batch(context, 1, &[("你甲甲", "ni3'jia3'jia3", 100_000)]),
        [true]
    );
    assert_eq!(
        import_batch(context, 7, &[("你乙乙", "ni3'yi3'yi3", 110_000)]),
        [true]
    );
    let system = lookup(instance, "你甲甲")[0];
    assert_eq!(system >> 24, 1);
    let rank = |instance| {
        predicted_texts(instance, c"你")
            .into_iter()
            .filter(|text| text == "甲甲" || text == "乙乙")
            .collect::<Vec<_>>()
    };
    assert_eq!(rank(instance), ["乙乙", "甲甲"]);
    assert!(unsafe { crate::dict::pinyin_token_add_unigram_frequency(instance, system, 100_000) });
    assert_eq!(rank(instance), ["甲甲", "乙乙"]);
    close(context, instance);
}

/// Overwrites a user-bigram successor count by phrase text.
///
/// Public `pinyin_train` first-seeds 69 (`23 * 3`), so
/// the prediction filter edge (`pinyin.cpp:2311`, `:2349-2350`) cannot be
/// reached through the C ABI. Looks up `prev` and `cur` in the user
/// phrase index.
fn set_user_bigram(
    context: *mut crate::types::PinyinContext,
    prev: *const std::os::raw::c_char,
    cur: *const std::os::raw::c_char,
    count: u64,
) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `pinyin_init`.
    let ctx = unsafe { crate::state::context_mut(context) };
    let user = ctx.core.user_store_mut();
    let prev_text =
        oxpinyin_capi_marshal::input::scalar_text(oxpinyin_capi_marshal::borrow_c_string!(prev))
            .unwrap_or_default();
    let cur_text =
        oxpinyin_capi_marshal::input::scalar_text(oxpinyin_capi_marshal::borrow_c_string!(cur))
            .unwrap_or_default();
    let Some(prev_tok) = user.token_for_phrase(&prev_text).ok().flatten() else {
        return false;
    };
    let Some(cur_tok) = user.token_for_phrase(&cur_text).ok().flatten() else {
        return false;
    };
    user.set_bigram_count(prev_tok, cur_tok, count).is_ok()
}

#[test]
fn predicted_bigram_filter_drops_9_keeps_10() {
    let user_dir = TempUserDir::new("predict-edge");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let iter = unsafe { pinyin_begin_add_phrases(context, 7) };
    assert!(unsafe {
        pinyin_iterator_add_phrase(iter, cstr("测测").as_ptr(), cstr("cece").as_ptr(), 5)
    });
    assert!(unsafe {
        pinyin_iterator_add_phrase(iter, cstr("甲甲").as_ptr(), cstr("jiajia").as_ptr(), 5)
    });
    assert!(unsafe {
        pinyin_iterator_add_phrase(iter, cstr("乙乙").as_ptr(), cstr("yiyi").as_ptr(), 5)
    });
    unsafe { pinyin_end_add_phrases(iter) };

    assert!(set_user_bigram(
        context,
        cstr("测测").as_ptr(),
        cstr("甲甲").as_ptr(),
        9,
    ));
    assert!(set_user_bigram(
        context,
        cstr("测测").as_ptr(),
        cstr("乙乙").as_ptr(),
        10,
    ));

    let prefix = cstr("测测");
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, prefix.as_ptr())
    });
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

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
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
    let nihao = c"nihao";
    assert_eq!(
        unsafe { crate::parse::pinyin_parse_more_full_pinyins(instance, nihao.as_ptr()) },
        5
    );
    assert!(unsafe { crate::sentence::pinyin_guess_candidates(instance, 0, 0x1e) });
    let mut candidate: *mut crate::types::LookupCandidate = std::ptr::null_mut();
    assert!(unsafe { pinyin_get_candidate(instance, 0, &raw mut candidate) });
    assert!(unsafe { pinyin_choose_candidate(instance, 0, candidate) } > 0);
    assert!(unsafe { crate::sentence::pinyin_guess_sentence(instance) });
    assert!(unsafe { crate::candidates::pinyin_train(instance, 0) });

    // The plain variant for prefix 你: rows are the sliced suggestion
    // texts 们 (from 你们) and 好 (from 你好).
    let prefix = c"你";
    assert!(unsafe {
        crate::predict::pinyin_guess_predicted_candidates(instance, prefix.as_ptr())
    });
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
    assert!(unsafe {
        crate::dict::pinyin_token_add_unigram_frequency(
            instance,
            trailing_token.value(),
            50_000_000,
        )
    });

    assert!(unsafe {
        crate::predict::pinyin_guess_predicted_candidates(instance, prefix.as_ptr())
    });
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

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

#[test]
fn predicted_candidates_prepend_punctuation_for_hao() {
    let user_dir = TempUserDir::new("predict-punct");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let prefix = cstr("好");
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, prefix.as_ptr())
    });
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

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}
