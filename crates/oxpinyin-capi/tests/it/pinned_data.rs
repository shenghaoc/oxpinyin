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

#[path = "../../../oxpinyin-core/tests/common/contract_data.rs"]
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
