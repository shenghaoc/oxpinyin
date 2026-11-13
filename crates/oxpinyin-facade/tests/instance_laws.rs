//! The facade-orchestration laws both C-ABI facades rely on, driven over
//! the committed `fixtures/w3` mini set: the parse-continuation rule a
//! re-parse applies, the lookup-offset validation per parse mode, the
//! key-at and cursor laws, and the reset shapes.
//!
//! Fixture selection follows the compiled backend. Initialization succeeds
//! even after a failed system attach, so it cannot identify a file format.

use std::path::PathBuf;

use oxpinyin_facade::{
    ContextCore, InstanceCore, PINYIN_DEFAULT_OPTION_WORD, PinyinCorrections, ToneForwarding,
    ZHUYIN_DEFAULT_OPTION_WORD,
};
use oxpinyin_user::UserConfLaw;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
}

/// Opens the committed fixture set the compiled backend reads.
fn open_context(option_word: u32) -> ContextCore {
    ContextCore::open(&fixture_dir(), None, option_word, UserConfLaw::Pinyin)
        .expect("committed backend fixtures")
}

fn pinyin_instance() -> (ContextCore, InstanceCore) {
    let context = open_context(PINYIN_DEFAULT_OPTION_WORD);
    let instance = context.alloc_instance();
    (context, instance)
}

fn zhuyin_instance() -> (ContextCore, InstanceCore) {
    let context = open_context(ZHUYIN_DEFAULT_OPTION_WORD);
    let instance = context.alloc_instance();
    (context, instance)
}

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
fn full_pinyin_parse_consumes_and_keys_sit_at_their_syllable_starts() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    assert_eq!(inst.bind(&context.runtime).parsed_len, 5);
    assert!(inst.bind(&context.runtime).session.is_composing());
    let ni = inst
        .bind(&context.runtime)
        .key_at(0)
        .expect("a key starts at 0");
    assert_eq!((ni.text, ni.begin, ni.end), ("ni", 0, 2));
    let hao = inst
        .bind(&context.runtime)
        .key_at(2)
        .expect("a key starts at 2");
    assert_eq!((hao.text, hao.begin, hao.end), ("hao", 2, 5));
    assert!(
        inst.bind(&context.runtime).key_at(1).is_none(),
        "a mid-syllable column is empty"
    );
    assert!(
        inst.bind(&context.runtime).key_at(5).is_none(),
        "the reserved slot answers nothing"
    );
}

#[test]
fn full_pinyin_skips_a_consumed_separator_column() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("ni'hao"), 6);
    // Offset 2 is the apostrophe: a lone zero-key column the pin walks past.
    let hao = inst
        .bind(&context.runtime)
        .key_at(2)
        .expect("the walk lands on `hao`");
    assert_eq!((hao.text, hao.begin, hao.end), ("hao", 3, 6));
}

/// At a separator column the zhuyin accessor reads the zero key itself;
/// the pinyin accessor walks on to the next real key.
#[test]
fn abi_key_at_a_separator_reads_the_zero_key_only_for_zhuyin() {
    let context = open_context(0x18a);
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

#[test]
fn one_key_probes_follow_the_live_schemes() {
    let (context, mut inst) = pinyin_instance();
    assert!(
        inst.bind(&context.runtime)
            .parse_one_full_pinyin("ni", PinyinCorrections::FromOptions)
            .is_some()
    );
    assert!(
        inst.bind(&context.runtime)
            .parse_one_full_pinyin("xyz", PinyinCorrections::FromOptions)
            .is_none()
    );
    let (zhuyin_context, mut zhuyin) = zhuyin_instance();
    // Standard keyboard: `s` is ㄋ; `q` is not a chewing symbol.
    assert!(
        !zhuyin
            .bind(&zhuyin_context.runtime)
            .in_keyboard(b's')
            .is_empty()
    );
    assert!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_one_chewing("su3")
            .is_some()
    );
    assert!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_one_chewing("")
            .is_none()
    );
}

/// The committed fixture directory the compiled backend opens.
fn fixture_dir() -> PathBuf {
    let extension = oxpinyin_data::DEFAULT_STORE_EXT;
    fixture_root().join(extension)
}
