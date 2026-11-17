#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{PARITY, close};

use crate::support::{
    DEFAULT_SORT, LookupCandidate, NBEST, NORMAL, PinyinContext, PinyinInstance, TempUserDir,
    candidate_at, capture_rows, cstr, decoded, key_at, pinyin_alloc_instance,
    pinyin_choose_candidate, pinyin_get_candidate, pinyin_guess_candidates, pinyin_guess_sentence,
    pinyin_parse_more_full_pinyins, pinyin_set_options, pinyin_train, sentence,
};

use contract_data::contract_data_dir;

use std::ptr;

#[path = "../../../oxpinyin-runtime/tests/common/contract_data.rs"]
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
    close(context, instance);
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
        close(context, instance);
    }
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

/// Constraint revalidation: 整个 forced over its abbreviation `zhg`, one initial-only key per
/// syllable, under 0x18a. Observation `constraint-revalidation/pinyin/zhg-整个` of the pin
/// capture of 2026-11-16 (runs 1 and 2 identical), at 074a2219.
///
/// `fill_matrix` puts `zh` at column 0 and `g` at column 2 (`phonetic_key_matrix.cpp:30-84`).
/// The choose answers `offset + add_constraint(0, 3)` (`pinyin.cpp:2578-2588`;
/// `lookup/phonetic_lookup.cpp:61-86`). `validate_constraint` keeps the forcing at the next guess
/// (`phonetic_lookup.cpp:120-167`), because an incomplete key matches any final
/// (`pinyin_phrase3.h:80-86`), and the one forced path leaves one sentence row
/// (`lookup/phonetic_lookup.h:552-554,601-612,776-788`). A parse never touches the constraints
/// (`pinyin.cpp:1497-1525`), so the forcing survives `zh` and `zhg` parsed with no guess between.
/// A guess on `zh` shrinks the store to 3 and clears the forcing that ends at 3
/// (`phonetic_lookup.cpp:136-138,145-148`).
#[test]
fn a_forcing_over_an_abbreviation_is_revalidated_as_the_pin_does() {
    let user = TempUserDir::new("constraint-revalidation");
    let (context, instance) = open_pinned(&user);
    assert!(unsafe { pinyin_set_options(context, PARITY) });
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, c"zhg".as_ptr()) },
        3
    );
    assert_eq!(
        unsafe { crate::support::pinyin_get_parsed_input_length(instance) },
        3
    );
    assert_eq!(
        (0..=3)
            .map(|offset| key_at(instance, offset))
            .collect::<Vec<_>>(),
        [
            Some((23, (0, 2), "zh".to_owned())),
            None,
            Some((7, (2, 3), "g".to_owned())),
            None,
        ]
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence(instance, 0), "中国");
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let rows = capture_rows(instance);
    assert_eq!(rows.len(), 2025);
    assert_eq!(
        &rows[..3],
        &[
            (NBEST, "中国".to_owned(), Some(0)),
            (NBEST, "这个".to_owned(), Some(2)),
            (NORMAL, "整个".to_owned(), None),
        ]
    );
    assert_eq!(
        unsafe { pinyin_choose_candidate(instance, 0, candidate_at(instance, 2)) },
        3
    );

    // Revalidated by the guess.
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence(instance, 0), "整个");
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let rows = capture_rows(instance);
    assert_eq!(rows.len(), 2025);
    assert_eq!(
        &rows[..3],
        &[
            (NBEST, "整个".to_owned(), Some(0)),
            (NORMAL, "中国".to_owned(), None),
            (NORMAL, "这个".to_owned(), None),
        ]
    );

    // A replaced selection: no guess sees the shorter parse.
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, c"zh".as_ptr()) },
        2
    );
    assert_eq!(
        unsafe { crate::support::pinyin_get_parsed_input_length(instance) },
        2
    );
    assert_eq!(decoded(instance, c"zhg"), "整个");

    // A guess on the shorter parse clears the forcing.
    assert_eq!(decoded(instance, c"zh"), "中");
    assert_eq!(decoded(instance, c"zhg"), "中国");
    close(context, instance);
}
