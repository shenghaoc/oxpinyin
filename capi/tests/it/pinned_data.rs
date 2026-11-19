#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{PARITY, close};

use crate::support::{
    DEFAULT_SORT, LookupCandidate, PinyinContext, PinyinInstance, TempUserDir, cstr,
    pinyin_alloc_instance, pinyin_get_candidate, pinyin_guess_candidates, pinyin_guess_sentence,
    pinyin_parse_more_full_pinyins, pinyin_set_options, pinyin_train,
};

use contract_data::contract_data_dir;

use std::ptr;

#[path = "../../../tests/common/contract_data.rs"]
mod contract_data;

/// A context and instance on the pinned data with a fresh user directory.
fn open_pinned(user_dir: &TempUserDir) -> (*mut PinyinContext, *mut PinyinInstance) {
    let system = cstr(contract_data_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir.path.to_str().expect("UTF-8 path"));
    let context = unsafe { crate::support::pinyin_init(system.as_ptr(), user.as_ptr()) };
    assert!(!context.is_null(), "pinyin_init opens the pinned data");
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    (context, instance)
}

/// S4: index n, one past the list, answers false with the out-param NULLed first
/// (`pinyin.cpp:2856-2859` at 074a2219). Observation `past-end/nihao`.
#[test]
fn the_row_one_past_the_list_is_refused() {
    let user_dir = TempUserDir::new("pinned-past-end");
    let (context, instance) = open_pinned(&user_dir);
    assert!(unsafe { pinyin_set_options(context, PARITY) });
    let input = cstr("nihao");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        5
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let mut count = 0;
    assert!(unsafe { crate::support::pinyin_get_n_candidate(instance, &raw mut count) });
    assert_eq!(count, 126);
    let mut cand = ptr::NonNull::<LookupCandidate>::dangling().as_ptr();
    assert!(!unsafe { pinyin_get_candidate(instance, count, &raw mut cand) });
    assert!(cand.is_null());
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// D10: `pinyin.cpp:2678-2688` at 074a2219 asserts that the train index is
/// below the number of decoded results before training it.
#[test]
fn training_the_last_two_nbest_results_succeeds() {
    for index in [1, 2] {
        let user = TempUserDir::new("train-index");
        let (context, instance) = open_pinned(&user);
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, c"nihao".as_ptr()) },
            5
        );
        assert_eq!(
            unsafe { crate::support::pinyin_get_parsed_input_length(instance) },
            5
        );
        assert!(unsafe { pinyin_guess_sentence(instance) });
        assert!(unsafe { pinyin_train(instance, index) });
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        unsafe { close(context, instance) };
    }
}

/// Each valid index trains its own result row (`pinyin.cpp:2683-2688`).
/// With 你 forced on the first key, every row starts with 你 and the rows
/// differ in the token after it. `train_result3` trains the forced token
/// and the token after it (`phonetic_lookup.h:866-872`): each gets
/// `initial_seed * unigram_factor` = 69 * 7 (`:847-849`, `:926-929`). The
/// trained token is one of the requested row, no token of another row gets
/// anything, and the indexes do not all train the same token.
#[test]
fn each_nbest_index_trains_its_own_row() {
    let mut trained = Vec::new();
    for index in 0..3_u8 {
        let user = TempUserDir::new("train-own-row");
        let (context, instance) = open_pinned(&user);
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, c"nihao".as_ptr()) },
            5
        );
        // SORT_WITHOUT_LONGER_CANDIDATE.
        assert!(unsafe { pinyin_guess_candidates(instance, 0, 0x2) });
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let forced = unsafe { crate::support::row_with(instance, crate::support::NORMAL, "你") };
        // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
        assert_eq!(
            unsafe { crate::support::pinyin_choose_candidate(instance, 0, forced) },
            2
        );
        assert!(unsafe { pinyin_guess_sentence(instance) });
        // The tokens of the phrase after 你 in each of the three rows.
        let mut tails = Vec::new();
        for row in 0..3_u8 {
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            let (found, text) = unsafe { crate::support::sentence_out(instance, row) };
            assert!(found, "row {row}");
            let text = text.expect("a sentence");
            let tail = text
                .strip_prefix("你")
                .unwrap_or_else(|| panic!("{text} starts with 你"));
            // SAFETY: as above.
            let tokens = unsafe { crate::support::lookup(instance, tail) };
            assert!(!tokens.is_empty(), "{tail}");
            tails.push(tokens);
        }
        let own = tails[usize::from(index)].clone();
        let all: std::collections::BTreeSet<u32> = tails.into_iter().flatten().collect();
        let unigrams = |tokens: &std::collections::BTreeSet<u32>| -> Vec<u32> {
            tokens
                .iter()
                .map(|&token| {
                    // SAFETY: `instance` was opened above and is not yet freed; only this thread
                    // calls into it.
                    let (found, unigram) = unsafe { crate::support::unigram(instance, token) };
                    assert!(found);
                    unigram
                })
                .collect()
        };
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let forced_tokens: std::collections::BTreeSet<u32> =
            unsafe { crate::support::lookup(instance, "你") }
                .into_iter()
                .collect();
        let before = unigrams(&all);
        let forced_before: u32 = unigrams(&forced_tokens).iter().sum();
        assert!(unsafe { pinyin_train(instance, index) });
        assert_eq!(
            unigrams(&forced_tokens).iter().sum::<u32>(),
            forced_before + 69 * 7,
            "index {index}"
        );
        let gained: Vec<(u32, u32)> = all
            .iter()
            .zip(before.iter().copied().zip(unigrams(&all)))
            .filter(|(_, (before, after))| before != after)
            .map(|(&token, (before, after))| (token, after - before))
            .collect();
        assert_eq!(gained.len(), 1, "index {index}: {gained:?}");
        assert_eq!(gained[0].1, 69 * 7, "index {index}");
        assert!(own.contains(&gained[0].0), "index {index}: {gained:?}");
        trained.push(gained[0].0);
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        unsafe { close(context, instance) };
    }
    trained.sort_unstable();
    trained.dedup();
    assert!(trained.len() > 1, "{trained:x?}");
}

#[test]
fn training_one_past_the_nbest_results_aborts() {
    crate::support::assert_aborts(
        "pinned_data::training_one_past_the_nbest_results_aborts",
        || {
            let user = TempUserDir::new("train-index-past-end");
            let (_context, instance) = open_pinned(&user);
            assert_eq!(
                unsafe { pinyin_parse_more_full_pinyins(instance, c"nihao".as_ptr()) },
                5
            );
            assert_eq!(
                unsafe { crate::support::pinyin_get_parsed_input_length(instance) },
                5
            );
            assert!(unsafe { pinyin_guess_sentence(instance) });
            unsafe { pinyin_train(instance, 3) };
        },
    );
}
