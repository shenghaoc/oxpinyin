//! The facade-orchestration laws both C-ABI facades rely on, driven over
//! the committed `fixtures/w3` mini set: the parse-continuation rule a
//! re-parse applies, the lookup-offset validation per parse mode, the
//! key-at and cursor laws, and the reset shapes.
//!
//! The fixture directory for the compiled backend is the one the runtime
//! opens: this crate has no direct dependency on the store extension, so
//! the helper probes the four committed sets and keeps the one that opens.

use std::path::PathBuf;

use oxpinyin_facade::{
    ContextCore, InstanceCore, PINYIN_DEFAULT_OPTION_WORD, ToneForwarding,
    ZHUYIN_DEFAULT_OPTION_WORD,
};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
}

/// Opens the committed fixture set the compiled backend reads.
fn open_context(option_word: u32) -> ContextCore {
    let root = fixture_root();
    for ext in ["tkt", "kct", "lmdb", "redb"] {
        let dir = root.join(ext);
        if !dir.is_dir() {
            continue;
        }
        if let Some(context) = ContextCore::open(dir.to_str().expect("UTF-8 path"), "", option_word)
        {
            return context;
        }
    }
    panic!("no fixtures/w3/<backend> set opens under the compiled backend");
}

fn pinyin_instance() -> InstanceCore {
    open_context(PINYIN_DEFAULT_OPTION_WORD)
        .alloc_instance()
        .expect("instance")
}

fn zhuyin_instance() -> InstanceCore {
    open_context(ZHUYIN_DEFAULT_OPTION_WORD)
        .alloc_instance()
        .expect("instance")
}

#[test]
fn a_fresh_instance_is_empty_and_answers_nothing() {
    let mut inst = pinyin_instance();
    assert_eq!(inst.parsed_len, 0);
    assert!(inst.anchored_window.is_none());
    assert!(!inst.session.is_composing());
    assert!(inst.key_at(0).is_none());
    assert!(
        !inst.train(),
        "no user store and no selection: train refuses"
    );
}

#[test]
fn full_pinyin_parse_consumes_and_keys_sit_at_their_syllable_starts() {
    let mut inst = pinyin_instance();
    assert_eq!(inst.parse_full_more("nihao"), 5);
    assert_eq!(inst.parsed_len, 5);
    assert!(inst.session.is_composing());
    let ni = inst.key_at(0).expect("a key starts at 0");
    assert_eq!((ni.text, ni.begin, ni.end), ("ni", 0, 2));
    let hao = inst.key_at(2).expect("a key starts at 2");
    assert_eq!((hao.text, hao.begin, hao.end), ("hao", 2, 5));
    assert!(inst.key_at(1).is_none(), "a mid-syllable column is empty");
    assert!(
        inst.key_at(5).is_none(),
        "the reserved slot answers nothing"
    );
}

#[test]
fn full_pinyin_skips_a_consumed_separator_column() {
    let mut inst = pinyin_instance();
    assert_eq!(inst.parse_full_more("ni'hao"), 6);
    // Offset 2 is the apostrophe: a lone zero-key column the pin walks past.
    let hao = inst.key_at(2).expect("the walk lands on `hao`");
    assert_eq!((hao.text, hao.begin, hao.end), ("hao", 3, 6));
}

#[test]
fn lookup_offset_validation_follows_the_active_mode() {
    let mut inst = pinyin_instance();
    assert_eq!(inst.parse_full_more("nihao"), 5);
    // Plain full pinyin: in range, normalised by the session's own law.
    assert!(inst.validate_lookup_offset(0).is_ok());
    assert!(inst.validate_lookup_offset(5).is_ok());
    assert!(inst.validate_lookup_offset(6).is_err(), "past one-past-end");

    let mut zhuyin = zhuyin_instance();
    assert_eq!(
        zhuyin.parse_chewing_more("su3cl3", ToneForwarding::ZhuyinFacade),
        6
    );
    // The zhuyin mode validates against the parse's consumed length.
    assert_eq!(zhuyin.validate_lookup_offset(3).expect("in range"), 3);
    assert_eq!(zhuyin.validate_lookup_offset(6).expect("terminal"), 6);
    assert!(zhuyin.validate_lookup_offset(7).is_err());
}

#[test]
fn cursor_laws_step_by_key_in_both_modes() {
    let mut inst = pinyin_instance();
    assert_eq!(inst.parse_full_more("nihao"), 5);
    assert_eq!(inst.lookup_offset(0).expect("cursor 0"), 0);
    assert_eq!(inst.left_offset(2).expect("left of hao"), 0);
    assert_eq!(inst.right_offset(0).expect("right of ni"), Some(2));

    let mut zhuyin = zhuyin_instance();
    assert_eq!(
        zhuyin.parse_chewing_more("su3cl3", ToneForwarding::ZhuyinFacade),
        6
    );
    let source = zhuyin.span_source().expect("a zhuyin parse is active");
    assert_eq!(source.spans, [(0, 3), (3, 6)]);
    assert!(!source.separators, "zhuyin holds no zero-key columns");
    assert_eq!(zhuyin.left_offset(3).expect("left of the second key"), 0);
    assert_eq!(
        zhuyin.right_offset(0).expect("right of the first key"),
        Some(3)
    );
    let ni = zhuyin.key_at(0).expect("first key");
    assert_eq!((ni.text, ni.tone, ni.begin, ni.end), ("ni", 3, 0, 3));
}

#[test]
fn a_re_parse_that_extends_the_buffer_continues_the_composition() {
    let mut inst = pinyin_instance();
    assert_eq!(inst.parse_full_more("ni"), 2);
    // The frontend re-sends the whole buffer every keystroke.
    inst.begin_parse(b"nihao");
    assert_eq!(inst.parse_full_more("nihao"), 5);
    assert_eq!(inst.parsed_len, 5);
    // Backspace: the buffer shrinks into the composition and stays open.
    inst.begin_parse(b"niha");
    assert_eq!(inst.parse_full_more("niha"), 4);
    assert_eq!(inst.parsed_len, 4);
}

#[test]
fn a_divergent_buffer_starts_a_fresh_composition() {
    let mut inst = pinyin_instance();
    assert_eq!(inst.parse_full_more("nihao"), 5);
    inst.begin_parse(b"women");
    assert_eq!(inst.parse_full_more("women"), 5);
    assert_eq!(inst.session.raw_input(), "women");
    assert_eq!(inst.session.composition_offset(), 0);
}

#[test]
fn the_parse_reset_keeps_the_session_and_the_full_reset_clears_it() {
    let mut inst = pinyin_instance();
    assert_eq!(inst.parse_full_more("nihao"), 5);
    inst.reset_parse_state();
    assert_eq!(inst.parsed_len, 0);
    assert!(inst.anchored_window.is_none());
    // The parse path resets the composition's parse state only: the raw
    // buffer (which the frontend re-sends every keystroke), the selection
    // record and the constraint store survive.
    assert!(inst.session.is_composing());
    assert_eq!(inst.session.raw_input(), "nihao");
    assert_eq!(inst.parse_full_more("nihao"), 5);
    inst.full_reset();
    assert_eq!(inst.parsed_len, 0);
    assert!(!inst.session.is_composing());
    assert!(inst.phrase_result.is_empty());
}

#[test]
fn one_key_probes_follow_the_live_schemes() {
    let inst = pinyin_instance();
    assert!(inst.parse_one_full_pinyin("ni", false).is_some());
    assert!(inst.parse_one_full_pinyin("xyz", false).is_none());
    let zhuyin = zhuyin_instance();
    // Standard keyboard: `s` is ㄋ; `q` is not a chewing symbol.
    assert!(!zhuyin.in_keyboard(b's').is_empty());
    assert!(zhuyin.parse_one_chewing("su3").is_some());
    assert!(zhuyin.parse_one_chewing("").is_none());
}
