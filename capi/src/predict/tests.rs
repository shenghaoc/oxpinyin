#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

// Nextest isolates the process-wide read-fault key by running each test in its own process.
use crate::test_support::{PREDICTED_BIGRAM, PREDICTED_PREFIX, predicted_texts};

use crate::test_support::{close, open_at, wrap_facade_total_to_zero};

use crate::candidates::{pinyin_choose_candidate, pinyin_get_candidate};

use crate::config::pinyin_set_options;

use crate::test_support::{capture_rows, import_batch};

use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_end_add_phrases, pinyin_iterator_add_phrase,
};

use crate::predict::pinyin_guess_predicted_candidates;

use crate::sentence::pinyin_guess_predicted_candidates_with_punctuations;

use crate::state::instance_ref;

use crate::test_support::{
    DEFAULT_SORT, TempUserDir, assert_aborts, cstr, gram_bytes, lookup, open, plant_bigram,
};

use crate::types::lookup_candidate_type_t;

use oxpinyin::__internal::vocab::phrase_index_library_index;

/// Predicted bigram rows on a zero facade total abort in the scorer.
#[test]
fn predicted_bigram_rows_abort_on_a_zero_facade_total() {
    assert_aborts(
        "predict::tests::predicted_bigram_rows_abort_on_a_zero_facade_total",
        || {
            let user = TempUserDir::new("row58-total-zero");
            let (context, instance) = open_at(&user.path);
            let (ni, hao) = (
                // SAFETY: `instance` was opened above and is not yet freed; only this thread calls
                // into it.
                unsafe { lookup(instance, "你") },
                // SAFETY: `instance` was opened above and is not yet freed; only this thread calls
                // into it.
                unsafe { lookup(instance, "好") }[0],
            );
            // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above
            // and is not yet freed; nothing uses either handle after this call; only this thread
            // calls into them.
            unsafe { close(context, instance) };
            let gram = gram_bytes(20, &[(hao, 20)]);
            plant_bigram(
                &user.path,
                &ni.iter()
                    .map(|token| (token.to_le_bytes().to_vec(), gram.clone()))
                    .collect::<Vec<_>>(),
            );
            let (_context, instance) = open_at(&user.path);
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { wrap_facade_total_to_zero(instance) };
            unsafe { pinyin_guess_predicted_candidates(instance, c"你".as_ptr()) };
        },
    );
}

/// `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN`.
const OPTIONS: u32 = 0x18a;

/// `DYNAMIC_ADJUST`.
const DYNAMIC_ADJUST: u32 = 1 << 9;

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
    use oxpinyin::facade::PhraseText;
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

/// The predicted scorer passes no predecessor, so neither DYNAMIC_ADJUST
/// nor the bigram counts move a row (`pinyin.cpp:2418`, `:2438`): two
/// successors of 你 rank the same whichever has the larger count.
#[test]
fn predicted_rows_ignore_dynamic_adjust_and_the_bigram_counts() {
    let user = TempUserDir::new("predict-bigram-terms");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let [ni, hao, zhong] = ["你", "好", "中"].map(|text| unsafe { lookup(instance, text) }[0]);
    // SAFETY: the setup handles are live and are unused after this call.
    unsafe { close(context, instance) };
    let mut lists = Vec::new();
    for (hao_count, zhong_count) in [(1000, 20), (20, 1000)] {
        let mut successors = [(hao, hao_count), (zhong, zhong_count)];
        successors.sort_unstable_by_key(|&(token, _)| token);
        crate::test_support::plant_bigram(
            &user.path,
            &[(
                ni.to_le_bytes().to_vec(),
                crate::test_support::gram_bytes(hao_count + zhong_count, &successors),
            )],
        );
        let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
        for dynamic in [0, DYNAMIC_ADJUST] {
            assert!(unsafe { pinyin_set_options(context, OPTIONS | dynamic) });
            // SAFETY: the instance is live; this thread serializes the calls.
            lists.push(unsafe { predicted_texts(instance, c"你") });
        }
        assert!(
            // SAFETY: prediction populated this live instance's rows.
            unsafe { capture_rows(instance) }
                .iter()
                .any(|(kind, text, _)| {
                    *kind == lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE && text == "中"
                }),
            "the bigram rows are listed"
        );
        // SAFETY: both handles are live and are unused after this call.
        unsafe { close(context, instance) };
    }
    assert!(lists.iter().all(|list| *list == lists[0]), "{lists:?}");
}

/// Prefix rows alone never reach the zero-total assert, which gates the
/// bigram rows only: on a facade total wrapped to zero the prediction
/// answers true with its rows.
#[test]
fn prefix_rows_rank_on_a_zero_facade_total() {
    let user = TempUserDir::new("predict-prefix-zero-total");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let before = unsafe { predicted_texts(instance, c"你") };
    unsafe { wrap_facade_total_to_zero(instance) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let mut after = unsafe { predicted_texts(instance, c"你") };
    after.sort();
    let mut expected = before;
    expected.sort();
    assert_eq!(after, expected);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A system-library import ranks by its live item count, runtime
/// additions included, against a user-library row.
#[test]
fn a_system_library_import_ranks_by_its_live_count() {
    let user = TempUserDir::new("predict-system-import");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { import_batch(context, 1, &[("你甲甲", "ni3'jia3'jia3", 100_000)]) },
        [true]
    );
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { import_batch(context, 7, &[("你乙乙", "ni3'yi3'yi3", 110_000)]) },
        [true]
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let system = unsafe { lookup(instance, "你甲甲") }[0];
    assert_eq!(system >> 24, 1);
    let rank = |instance| {
        // SAFETY: the test calls `rank` only with its own `instance`, which was opened above and
        // is not yet freed; only this thread calls into it.
        (unsafe { predicted_texts(instance, c"你") })
            .into_iter()
            .filter(|text| text == "甲甲" || text == "乙乙")
            .collect::<Vec<_>>()
    };
    assert_eq!(rank(instance), ["乙乙", "甲甲"]);
    assert!(unsafe { crate::dict::pinyin_token_add_unigram_frequency(instance, system, 100_000) });
    assert_eq!(rank(instance), ["甲甲", "乙乙"]);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
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

    // SAFETY: the live context has no reader retained across this write.
    let store = unsafe { crate::state::context_mut(context) }
        .core
        .user_store_mut();
    let [prev, nine, ten] = ["测测", "甲甲", "乙乙"].map(|text| {
        store
            .token_for_phrase(text)
            .expect("read")
            .expect("imported")
    });
    // Training starts at 69. Load the exact stored counts around the filter at 10.
    assert!(unsafe { crate::context::pinyin_save(context) });
    // SAFETY: both import handles are live and are unused after closing them.
    unsafe { close(context, instance) };
    crate::test_support::plant_bigram(
        &user_dir.path,
        &[(
            prev.to_le_bytes().to_vec(),
            crate::test_support::gram_bytes(19, &[(nine, 9), (ten, 10)]),
        )],
    );
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let prefix = cstr("测测");
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, prefix.as_ptr())
    });
    // SAFETY: live instance after guess_predicted.
    let inst = unsafe { instance_ref(instance) };
    let bigrams: Vec<&str> = inst
        .state
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
        .state
        .candidates
        .iter()
        .map(|row| String::from_utf8_lossy(row.text_bytes()).into_owned())
        .collect();
    let leader_before = before[0].clone();
    let trailing_token = inst
        .state
        .candidates
        .iter()
        .skip(1)
        .find_map(|row| row.token.get())
        .expect("no trailing row carries a token");

    // The huge engineered delta through the ABI write path.
    assert!(unsafe {
        crate::dict::pinyin_token_add_unigram_frequency(instance, trailing_token, 50_000_000)
    });

    assert!(unsafe {
        crate::predict::pinyin_guess_predicted_candidates(instance, prefix.as_ptr())
    });
    // SAFETY: live instance after guess_predicted.
    let inst = unsafe { instance_ref(instance) };
    let after: Vec<String> = inst
        .state
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
        .state
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

use crate::candidates::pinyin_choose_predicted_candidate;
use crate::context::pinyin_save;
use crate::test_support::{
    replace_user_bigram, row_with, saved_user_phrase, set_phrase_table_row, unigram,
};
const TAG: u32 = 0x8000_0000;

/// A prefix row keeps the token the phrase table holds
/// (`pinyin.cpp:2384-2404`; `phrase_large_table3.cpp:42-48`). Choosing it
/// adds `23 * 3 * 7 = 483` to the unigram of the item its masked library
/// and slot name (`pinyin.cpp:2608-2616`, `phrase_index.h:629`,
/// `phrase_index.cpp:154`).
#[test]
fn a_prefix_row_under_high_bits_is_listed_and_trains_its_item() {
    let user = TempUserDir::new("tagged-prefix");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        unsafe { import_batch(context, 7, &[("龘", "da2", 1), ("龘龘", "da2'da2", 1)]) },
        [true, true]
    );
    assert!(unsafe { pinyin_save(context) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { lookup(instance, "龘龘") }[0];
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    set_phrase_table_row(&user.path, "龘龘", &[token | TAG]);
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let (found, base) = unsafe { unigram(instance, token) };
    assert!(found);
    assert!(unsafe { pinyin_guess_predicted_candidates(instance, c"龘".as_ptr()) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { row_with(instance, PREDICTED_PREFIX, "龘") };
    assert!(unsafe { pinyin_choose_predicted_candidate(instance, row) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { unigram(instance, token) }, (true, base + 483));
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A user gram item whose token has high bits set. The bigram rows keep
/// the item's token and read its phrase through `get_phrase_item`, which
/// masks the library and the slot (`pinyin.cpp:2352-2364`,
/// `phrase_index.h:647`, `phrase_index.cpp:186` at the pin `074a2219`).
/// Choosing the row adds `23 * 3 * 7 = 483` to that item's unigram
/// (`pinyin.cpp:2608-2611`, `phrase_index.h:629`) and `23 * 3 = 69` to the
/// gram item of the whole token and to the gram's total
/// (`pinyin.cpp:2627-2637`).
#[test]
fn a_bigram_row_under_high_bits_is_listed_and_trains_its_item() {
    let user = TempUserDir::new("tagged-bigram-row");
    let successor = saved_user_phrase(&user.path);
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let prefixes = unsafe { lookup(instance, "你") };
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    let gram = gram_bytes(20, &[(successor | TAG, 20)]);
    let rows: Vec<_> = prefixes
        .iter()
        .map(|token| (token.to_le_bytes().to_vec(), gram.clone()))
        .collect();
    replace_user_bigram(&user.path, &rows);
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let (found, base) = unsafe { unigram(instance, successor) };
    assert!(found);
    assert!(unsafe { pinyin_guess_predicted_candidates(instance, c"你".as_ptr()) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { row_with(instance, PREDICTED_BIGRAM, "龘龘") };
    assert!(unsafe { pinyin_choose_predicted_candidate(instance, row) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { unigram(instance, successor) }, (true, base + 483));
    let trained: Vec<_> = {
        // SAFETY: the instance is live and this read ends before the close.
        let inst = unsafe { crate::state::instance_ref(instance) };
        prefixes
            .iter()
            .map(|prefix| inst.core.user().bigram_successors(*prefix).expect("a gram"))
            .filter(|items| *items != [(successor | TAG, 20)])
            .collect()
    };
    assert_eq!(trained, [[(successor | TAG, 89)]]);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The prediction walks the user phrase table by raw key bytes and takes
/// the row's text from the live item: a key 你 + U+110000 lists 龘龘's
/// suffix 龘.
#[test]
fn a_raw_user_key_predicts_its_live_item() {
    let user = TempUserDir::new("raw-key-prediction");
    let token = saved_user_phrase(&user.path);
    crate::test_support::craft_user_index(
        &user.path.join("user_phrase_index.bin"),
        &[
            (crate::test_support::ucs4("你"), Vec::new()),
            (
                [
                    crate::test_support::ucs4("你"),
                    0x0011_0000_u32.to_le_bytes().to_vec(),
                ]
                .concat(),
                token.to_le_bytes().to_vec(),
            ),
        ],
    );
    crate::test_support::write_user_conf(&user.path, &crate::test_support::system_dir());
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_guess_predicted_candidates(instance, c"你".as_ptr()) });
    assert!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { capture_rows(instance) }
            .iter()
            .any(|(kind, text, _)| *kind == PREDICTED_PREFIX && text == "龘")
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The pin sorts the predicted rows with the stable `g_array_sort_with_data`
/// and `compare_item_with_sort_option` under `SORT_BY_PHRASE_LENGTH |
/// SORT_BY_FREQUENCY` (`pinyin.cpp:2443-2447` at `074a2219`; GLib's sort
/// is stable since 2.32), so tied rows keep their arrival order:
/// library by library (`reduce_tokens`, `phrase_large_table3.h:77-97`),
/// and inside one library the phrase table's key order over the
/// little-endian UCS-4 keys of the stored phrase text.
#[test]
fn predicted_tie_groups_keep_library_then_key_order_including_user_rows() {
    use crate::sentence::pinyin_guess_predicted_candidates_with_punctuations;
    use crate::types::lookup_candidate_type_t;

    /// Mirror of the engine's `amplified_frequency` at the pinned λ, so
    /// the expected order is computed without the production code.
    fn amplified(baked: u64, total: u64) -> u64 {
        if total == 0 {
            return 0;
        }
        let possibility = (1.0_f32 - 0.312_699_f32) * baked as f32 / total as f32;
        u64::from((possibility * 256.0 * 256.0 * 256.0) as u32)
    }

    /// The phrase table's key for `text`: its UCS-4 words, little-endian.
    fn ucs4_le_key(text: &str) -> Vec<u8> {
        text.chars()
            .flat_map(|ch| u32::from(ch).to_le_bytes())
            .collect()
    }

    let user_dir = TempUserDir::new("pred-defined-order");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // Populate the USER seam: the prefix phrase 中 (a fixture phrase too,
    // so the system side also contributes), then two pairs of user
    // phrases under it with equal counts, so each pair ties.
    // - 中年 is added before 中华, so token order would put 年 first; key
    //   order (and text order) put 华 first.
    // - 中型 (型 U+578B, low byte 8B) and 中文 (文 U+6587, low byte 87):
    //   text order puts 型 first, little-endian key order puts 文 first.
    //   (The fixture already holds 中国 as a system phrase, so importing it
    //   would train that system item rather than add a user row.)
    //   This pair tells the pin's key order apart from text order.
    let iter = unsafe { pinyin_begin_add_phrases(context, 7) };
    assert!(!iter.is_null());
    for (phrase, pinyin, count) in [
        ("\u{4e2d}", "zhong", 9),
        ("\u{4e2d}\u{5e74}", "zhongnian", 7),
        ("\u{4e2d}\u{534e}", "zhonghua", 7),
        ("\u{4e2d}\u{578b}", "zhongxing", 7),
        ("\u{4e2d}\u{6587}", "zhongwen", 7),
    ] {
        assert!(unsafe {
            pinyin_iterator_add_phrase(iter, cstr(phrase).as_ptr(), cstr(pinyin).as_ptr(), count)
        });
    }
    unsafe { pinyin_end_add_phrases(iter) };

    let prefix = "\u{4e2d}";
    let prefix_c = cstr(prefix);
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, prefix_c.as_ptr())
    });

    // SAFETY: live instance immediately after the guess.
    let inst = unsafe { instance_ref(instance) };
    let total = inst.core.lm().amplified_total();
    // (text, length, amplified frequency, library, stored-text key)
    let rows: Vec<(String, usize, u64, usize, Vec<u8>)> = inst
        .state
        .candidates
        .iter()
        .filter(|c| c.candidate_type() == lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE)
        .map(|c| {
            let token = c.token.get().expect("a predicted row has a token");
            let baked = inst
                .core
                .dict()
                .live_items()
                .effective_unigram(token)
                .unwrap_or(0)
                + inst.core.user().unigram_delta(token);
            let text = c.text_str().expect("candidate text is UTF-8").to_owned();
            // A predicted row shows the phrase after the prefix; the
            // phrase table keys the whole stored phrase.
            let key = ucs4_le_key(&format!("{prefix}{text}"));
            let length = text.chars().count();
            (
                text,
                length,
                amplified(baked, total),
                usize::from(phrase_index_library_index(token)),
                key,
            )
        })
        .collect();

    // The user rows survived the prefix slice and reached the list.
    let user_position = |text: &str| {
        rows.iter()
            .position(|row| {
                row.0 == text && row.3 == usize::from(oxpinyin::facade::USER_DICTIONARY)
            })
            .unwrap_or_else(|| panic!("user row {text:?} missing from {rows:?}"))
    };
    let hua = user_position("\u{534e}");
    let nian = user_position("\u{5e74}");
    let xing = user_position("\u{578b}");
    let wen = user_position("\u{6587}");

    // THE PIN'S ORDER: the list is sorted by the comparator's keys (length
    // desc, amplified frequency desc), and inside every tie group the rows
    // keep their arrival order: library ascending, then the little-endian
    // UCS-4 key of the stored phrase text.
    for (i, (text_a, len_a, freq_a, library_a, key_a)) in rows.iter().enumerate() {
        for (text_b, len_b, freq_b, library_b, key_b) in rows.iter().skip(i + 1) {
            if len_a == len_b && freq_a == freq_b {
                assert!(
                    (library_a, key_a) <= (library_b, key_b),
                    "tie group out of the pin's order: {text_a:?} before {text_b:?} \
                     in {rows:?}"
                );
            }
        }
    }
    // Both pairs tie inside the user library.
    assert_eq!((rows[hua].1, rows[hua].2), (rows[nian].1, rows[nian].2));
    assert_eq!((rows[xing].1, rows[xing].2), (rows[wen].1, rows[wen].2));
    assert!(hua < nian, "key order, not token order: 华 before 年");
    assert!(
        wen < xing,
        "little-endian key order, not text order: 文 before 型"
    );
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}
