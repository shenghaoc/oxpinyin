//! The fault classification of the cursor and lookup laws, and the
//! candidate-lookup offset of both facades.

use oxpinyin_engine::EngineError;

use super::{CursorFault, CursorLaw};
use crate::ToneForwarding;
use crate::test_fixture::{open_context, pinyin_instance, zhuyin_instance};

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
    let (context, mut inst) = pinyin_instance();
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
    let (context, mut inst) = zhuyin_instance();
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
    let (context, mut inst) = pinyin_instance();
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

// From the facade-orchestration laws (see instance/tests.rs).
/// At a separator column the zhuyin accessor reads the zero key itself;
/// the pinyin accessor walks on to the next real key.
#[test]
fn abi_key_at_a_separator_reads_the_zero_key_only_for_zhuyin() {
    let context = open_context(oxpinyin_core::OptionBits::from_bits_retain(0x18a));
    let mut inst = context.alloc_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("'ni'hao"), 7);
    let mut packed = |zhuyin| {
        inst.bind(&context.runtime)
            .abi_key_at(3, zhuyin)
            .expect("no zero-column fault")
            .expect("a key")
            .0
    };
    assert_eq!(packed(true).to_le_bytes(), [0x00, 0x00]);
    assert_eq!(packed(false).to_le_bytes(), [0x86, 0x02]);
}

#[test]
fn cursor_laws_step_by_key_in_both_modes() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    assert_eq!(
        inst.bind(&context.runtime)
            .lookup_offset(0)
            .expect("cursor 0"),
        0
    );
    assert_eq!(
        inst.bind(&context.runtime)
            .left_offset(2)
            .expect("left of hao"),
        0
    );
    assert_eq!(
        inst.bind(&context.runtime)
            .right_offset(0)
            .expect("right of ni"),
        Some(2)
    );

    let (zhuyin_context, mut zhuyin) = zhuyin_instance();
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_chewing_more("su3cl3", ToneForwarding::ZhuyinFacade),
        6
    );
    let read = zhuyin.bind(&zhuyin_context.runtime);
    let source = read.span_source().expect("a zhuyin parse is active");
    let spans: Vec<_> = source
        .spans
        .iter()
        .map(|key| (key.begin, key.end))
        .collect();
    assert_eq!(spans, [(0, 3), (3, 6)]);
    assert!(!source.separators, "zhuyin holds no zero-key columns");
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .left_offset(3)
            .expect("left of the second key"),
        0
    );
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .right_offset(0)
            .expect("right of the first key"),
        Some(3)
    );
    let ni = zhuyin
        .bind(&zhuyin_context.runtime)
        .key_at(0)
        .expect("first key");
    assert_eq!((ni.text, ni.tone, ni.begin, ni.end), ("ni", 3, 0, 3));
}
