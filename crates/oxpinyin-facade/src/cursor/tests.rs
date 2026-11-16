//! The fault classification of the cursor and lookup laws, and the
//! candidate-lookup offset of both facades.

use std::path::PathBuf;

use oxpinyin_engine::EngineError;
use oxpinyin_user::UserConfLaw;

use super::{CursorFault, CursorLaw};
use crate::{
    ContextCore, InstanceCore, PINYIN_DEFAULT_OPTION_WORD, ToneForwarding,
    ZHUYIN_DEFAULT_OPTION_WORD,
};

/// Opens the committed `fixtures/w3` set the compiled backend reads.
fn instance(option_word: oxpinyin_core::OptionBits) -> (ContextCore, InstanceCore) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT);
    let context = ContextCore::open(&dir, None, option_word, UserConfLaw::Pinyin)
        .expect("committed backend fixtures");
    let instance = context.alloc_instance();
    (context, instance)
}

#[rstest::rstest]
#[case::out_of_range(EngineError::LookupOffsetOutOfRange { offset: 7, len: 5 }, true, true)]
#[case::zero_key(EngineError::ZeroKeyOffsetCheck { offset: 4 }, true, true)]
#[case::matrix_column(EngineError::MatrixColumnAssert { offset: 4 }, false, true)]
#[case::reduce_tokens(EngineError::ReduceTokensCount { count: 5 }, false, true)]
#[case::past_separator(
    EngineError::LookupOffsetPastSeparator { offset: 1, normalized: 0 },
    false,
    false
)]
#[case::toned_incomplete(EngineError::TonedIncompleteKey, false, false)]
fn each_law_aborts_at_its_own_pin_asserts(
    #[case] error: EngineError,
    #[case] check_offset_asserts: bool,
    #[case] character_offset_asserts: bool,
) {
    let fault = |asserts| {
        if asserts {
            CursorFault::PinAssert
        } else {
            CursorFault::Unreadable
        }
    };
    assert_eq!(
        CursorFault::of(&error, CursorLaw::CheckOffset),
        fault(check_offset_asserts)
    );
    assert_eq!(
        CursorFault::of(&error, CursorLaw::CharacterOffset),
        fault(character_offset_asserts)
    );
}

#[test]
fn the_pinyin_candidate_offset_one_past_the_reserved_slot_is_the_end_of_input() {
    let (context, mut inst) = instance(PINYIN_DEFAULT_OPTION_WORD);
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    let read = inst.bind(&context.runtime);
    assert_eq!(read.pin_matrix_columns(), 6);
    assert_eq!(read.abi_candidate_offset(2, false), Ok(2));
    // `pinyin.cpp:2226` ignores `_check_offset`: one past the reserved
    // slot is the end-of-input lookup.
    assert_eq!(read.abi_candidate_offset(6, false), Ok(5));
    // Past the last column, `get_column_size` asserts.
    assert_eq!(
        read.abi_candidate_offset(7, false),
        Err(CursorFault::PinAssert)
    );
}

#[test]
fn the_zhuyin_candidate_offset_one_past_the_reserved_slot_asserts() {
    let (context, mut inst) = instance(ZHUYIN_DEFAULT_OPTION_WORD);
    assert_eq!(
        inst.bind(&context.runtime)
            .parse_chewing_more("su3cl3", ToneForwarding::ZhuyinFacade),
        6
    );
    let read = inst.bind(&context.runtime);
    assert_eq!(read.pin_matrix_columns(), 7);
    assert_eq!(read.abi_candidate_offset(3, true), Ok(3));
    // `zhuyin.cpp:1441-1456` asserts on the reserved slot's zero key.
    assert_eq!(
        read.abi_candidate_offset(7, true),
        Err(CursorFault::PinAssert)
    );
    assert_eq!(
        read.abi_candidate_offset(8, true),
        Err(CursorFault::PinAssert)
    );
}

#[test]
fn an_empty_matrix_asserts_on_a_candidate_offset_past_column_zero() {
    let (context, mut inst) = instance(PINYIN_DEFAULT_OPTION_WORD);
    let read = inst.bind(&context.runtime);
    assert_eq!(read.pin_matrix_columns(), 0);
    assert_eq!(
        read.abi_candidate_offset(1, false),
        Err(CursorFault::PinAssert)
    );
    assert_eq!(
        read.abi_candidate_offset(1, true),
        Err(CursorFault::PinAssert)
    );
}
