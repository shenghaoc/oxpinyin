//! The facade-orchestration laws both C-ABI facades rely on, driven over
//! the committed `fixtures/w3` mini set: the parse-continuation rule a
//! re-parse applies, the lookup-offset validation per parse mode, the
//! key-at and cursor laws, and the reset shapes.
//!
//! Fixture selection follows the compiled backend. Initialization succeeds
//! even after a failed system attach, so it cannot identify a file format.

use crate::ToneForwarding;

use crate::test_fixture::{pinyin_instance, zhuyin_instance};

#[test]
fn a_fresh_instance_is_empty_and_answers_nothing() {
    let (mut context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parsed_len, 0);
    assert!(!inst.bind(&context.runtime).session.is_composing());
    assert!(inst.bind(&context.runtime).key_at(0).is_none());
    assert!(
        !context.train(&inst, 0).expect("fresh train"),
        "no user store and no selection: train refuses"
    );
}

#[test]
fn lookup_offset_validation_follows_the_active_mode() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    // Plain full pinyin: in range, normalised by the session's own law.
    assert!(
        inst.bind(&context.runtime)
            .validate_lookup_offset(0)
            .is_ok()
    );
    assert!(
        inst.bind(&context.runtime)
            .validate_lookup_offset(5)
            .is_ok()
    );
    assert!(
        inst.bind(&context.runtime)
            .validate_lookup_offset(6)
            .is_err(),
        "past one-past-end"
    );

    let (zhuyin_context, mut zhuyin) = zhuyin_instance();
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_chewing_more("su3cl3", ToneForwarding::ZhuyinFacade),
        6
    );
    // The zhuyin mode validates against the parse's consumed length.
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .validate_lookup_offset(3)
            .expect("in range"),
        3
    );
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .validate_lookup_offset(6)
            .expect("terminal"),
        6
    );
    assert!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .validate_lookup_offset(7)
            .is_err()
    );
}

#[test]
fn a_re_parse_that_extends_the_buffer_continues_the_composition() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("ni"), 2);
    // The frontend re-sends the whole buffer every keystroke.
    inst.bind(&context.runtime).begin_parse(b"nihao");
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    assert_eq!(inst.bind(&context.runtime).parsed_len, 5);
    // Backspace: the buffer shrinks into the composition and stays open.
    inst.bind(&context.runtime).begin_parse(b"niha");
    assert_eq!(inst.bind(&context.runtime).parse_full_more("niha"), 4);
    assert_eq!(inst.bind(&context.runtime).parsed_len, 4);
}

#[test]
fn a_divergent_buffer_starts_a_fresh_composition() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    inst.bind(&context.runtime).begin_parse(b"women");
    assert_eq!(inst.bind(&context.runtime).parse_full_more("women"), 5);
    assert_eq!(inst.bind(&context.runtime).session.raw_input(), "women");
    assert_eq!(inst.bind(&context.runtime).session.composition_offset(), 0);
}

#[test]
fn the_parse_reset_keeps_the_session_and_the_full_reset_clears_it() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    inst.bind(&context.runtime).reset_parse_state();
    assert_eq!(inst.bind(&context.runtime).parsed_len, 0);
    // The parse path resets the composition's parse state only: the raw
    // buffer (which the frontend re-sends every keystroke), the selection
    // record and the constraint store survive.
    assert!(inst.bind(&context.runtime).session.is_composing());
    assert_eq!(inst.bind(&context.runtime).session.raw_input(), "nihao");
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    inst.bind(&context.runtime).full_reset();
    assert_eq!(inst.bind(&context.runtime).parsed_len, 0);
    assert!(!inst.bind(&context.runtime).session.is_composing());
    assert!(inst.bind(&context.runtime).phrase_result.is_empty());
}
