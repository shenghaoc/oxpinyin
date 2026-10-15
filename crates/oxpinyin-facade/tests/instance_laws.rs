//! The facade-orchestration laws both C-ABI facades rely on, driven over
//! the committed `fixtures/w3` mini set: the parse-continuation rule a
//! re-parse applies, the lookup-offset validation per parse mode, the
//! key-at and cursor laws, and the reset shapes.
//!
//! The fixture directory for the compiled backend is the one the runtime
//! opens: this crate has no direct dependency on the store extension, so
//! the helper probes the four committed sets and keeps the one that opens.

use std::path::PathBuf;

use oxpinyin_engine::EngineError;
use oxpinyin_facade::{
    ContextCore, InstanceCore, PINYIN_DEFAULT_OPTION_WORD, ToneForwarding,
    ZHUYIN_DEFAULT_OPTION_WORD,
};
use oxpinyin_user::{SENTENCE_START, SystemVersions, UserConfLaw, persistence};

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
    for ext in ["tkt", "kct", "db"] {
        let dir = root.join(ext);
        if !dir.is_dir() {
            continue;
        }
        if let Some(context) = ContextCore::open(&dir, None, option_word, UserConfLaw::Pinyin) {
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
        !inst.train(0).expect("fresh train"),
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

/// The committed fixture directory the compiled backend opens.
fn fixture_dir() -> PathBuf {
    let root = fixture_root();
    ["tkt", "kct", "db"]
        .into_iter()
        .map(|ext| root.join(ext))
        .find(|dir| {
            dir.is_dir()
                && ContextCore::open(dir, None, PINYIN_DEFAULT_OPTION_WORD, UserConfLaw::Pinyin)
                    .is_some()
        })
        .expect("a fixtures/w3/<backend> set opens under the compiled backend")
}

/// A user dir holding one user-bigram row, `(key, value)` verbatim, under a
/// conform marker for `system`.
fn user_dir_with_bigram_row(
    system: &std::path::Path,
    name: &str,
    row: (Vec<u8>, Vec<u8>),
) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxpinyin-facade-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("user dir");
    let conf = oxpinyin_runtime::TableConf::read(&system.join("table.conf"))
        .and_then(Result::ok)
        .unwrap_or_else(oxpinyin_runtime::TableConf::stock);
    let state = persistence::UserState {
        bigram: vec![row],
        ..persistence::UserState::default()
    };
    persistence::save(
        &dir,
        &state,
        &std::collections::BTreeMap::new().into(),
        &SystemVersions::from_conf(&conf),
        1,
    )
    .expect("write the profile");
    dir
}

/// N2: a training step reads its predecessor's total first
/// (`phonetic_lookup.h:884` at the pin `074a2219`), and a value too short
/// for one is the pin's assert (`memory_chunk.h:390`). The pin's guess dies
/// on the same gram before a train can be reached
/// (`phonetic_lookup.h:594-598`), so no contract case holds this; the
/// facade refuses the train with the short-predecessor error and observes
/// nothing.
#[test]
fn training_over_a_short_predecessor_value_refuses_and_observes_nothing() {
    let system = fixture_dir();
    let short = vec![0xa5, 0x5a, 0x01];
    let user = user_dir_with_bigram_row(
        &system,
        "short-train",
        (SENTENCE_START.to_le_bytes().to_vec(), short.clone()),
    );
    let context = ContextCore::open(
        &system,
        Some(&user),
        PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .expect("context");
    let store = context.user_store().expect("user store");
    assert!(store.has_short_bigram_value_for(SENTENCE_START));
    let mut inst = context.alloc_instance().expect("instance");
    assert_eq!(inst.parse_full_more("nihao"), 5);
    // A forced first phrase: the step that trains it loads the sentence
    // start's gram (`phonetic_lookup.h:866-884`).
    let list = inst.session.candidates_at(0).expect("candidates");
    let index = list
        .iter()
        .position(|candidate| candidate.text() == "你")
        .expect("你 is a candidate");
    inst.session.select(index).expect("choose 你");
    assert!(inst.session.guess_sentence().expect("guess"));
    let generation = store.generation();
    assert_eq!(
        inst.train(0),
        Err(EngineError::ShortTrainingPredecessor {
            token: SENTENCE_START
        })
    );
    assert_eq!(store.generation(), generation, "nothing was observed");
    assert!(!store.is_modified());
    assert_eq!(store.bigram_row(SENTENCE_START).expect("row"), Some(short));
    drop(inst);
    drop(context);
    let _ = std::fs::remove_dir_all(&user);
}

/// Q1's abort at the pin: the sentence start's gram holds the forced 你's
/// neighbours in the order `[1, 2, 3, 6, 4]` around it (你 = 5), so the
/// training step's `get_freq` misses, `insert_freq` appends, and `set_freq`
/// lands on the larger token and misses — `check_result` dies
/// (`phonetic_lookup.h:903` at the pin `074a2219`). The facade refuses the
/// train with the set-freq error and observes nothing.
#[test]
fn training_that_would_miss_its_inserted_item_refuses_and_observes_nothing() {
    let system = fixture_dir();
    let ni = {
        let context = ContextCore::open(
            &system,
            None,
            PINYIN_DEFAULT_OPTION_WORD,
            UserConfLaw::Pinyin,
        )
        .expect("context");
        let mut inst = context.alloc_instance().expect("instance");
        assert_eq!(inst.parse_full_more("nihao"), 5);
        let list = inst.session.candidates_at(0).expect("candidates");
        list.iter()
            .find(|candidate| candidate.text() == "你")
            .and_then(oxpinyin_engine::Candidate::token)
            .expect("你 has a token")
            .value()
    };
    let mut gram = 5_u32.to_le_bytes().to_vec();
    for token in [ni - 4, ni - 3, ni - 2, ni + 1, ni - 1] {
        gram.extend_from_slice(&token.to_le_bytes());
        gram.extend_from_slice(&1_u32.to_le_bytes());
    }
    let user = user_dir_with_bigram_row(
        &system,
        "set-freq-miss",
        (SENTENCE_START.to_le_bytes().to_vec(), gram.clone()),
    );
    let context = ContextCore::open(
        &system,
        Some(&user),
        PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .expect("context");
    let store = context.user_store().expect("user store");
    let mut inst = context.alloc_instance().expect("instance");
    assert_eq!(inst.parse_full_more("nihao"), 5);
    let list = inst.session.candidates_at(0).expect("candidates");
    let index = list
        .iter()
        .position(|candidate| candidate.text() == "你")
        .expect("你 is a candidate");
    inst.session.select(index).expect("choose 你");
    assert!(inst.session.guess_sentence().expect("guess"));
    let generation = store.generation();
    assert_eq!(
        inst.train(0),
        Err(EngineError::TrainingSetFreqMiss {
            token: SENTENCE_START
        })
    );
    assert_eq!(store.generation(), generation, "nothing was observed");
    assert!(!store.is_modified());
    assert_eq!(store.bigram_row(SENTENCE_START).expect("row"), Some(gram));
    drop(inst);
    drop(context);
    let _ = std::fs::remove_dir_all(&user);
}

/// The tokens of 你 and 好 in the committed fixture, from a context without
/// a user dir.
fn ni_and_hao(system: &std::path::Path) -> (u32, u32) {
    let context = ContextCore::open(
        system,
        None,
        PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .expect("context");
    let mut inst = context.alloc_instance().expect("instance");
    assert_eq!(inst.parse_full_more("nihao"), 5);
    let token = |inst: &mut InstanceCore, offset: usize, text: &str| {
        let list = inst.session.candidates_at(offset).expect("candidates");
        list.iter()
            .find(|candidate| candidate.text() == text)
            .and_then(oxpinyin_engine::Candidate::token)
            .expect("the fixture phrase has a token")
            .value()
    };
    (token(&mut inst, 0, "你"), token(&mut inst, 2, "好"))
}

/// The pin's `[1, 2, 3, 6, 4]` order around `token` (= 5): training it
/// misses, appends, and `set_freq` misses the appended item
/// (`phonetic_lookup.h:903`).
fn set_freq_miss_gram(token: u32) -> Vec<u8> {
    let mut gram = 5_u32.to_le_bytes().to_vec();
    for item in [token - 4, token - 3, token - 2, token + 1, token - 1] {
        gram.extend_from_slice(&item.to_le_bytes());
        gram.extend_from_slice(&1_u32.to_le_bytes());
    }
    gram
}

/// Plants `row`, forces 你 on `nihao` and guesses; the row trains two
/// steps — the sentence start → 你, then 你 → 好. `reparse`, when given,
/// replaces the input without a fresh guess, leaving the stale result the
/// pin still trains (`phonetic_lookup.h:921`). Answers the train's result
/// and whether the store stayed untouched.
fn train_two_steps(
    name: &str,
    row: (u32, Vec<u8>),
    reparse: Option<&str>,
) -> (Result<bool, EngineError>, bool) {
    let system = fixture_dir();
    let user = user_dir_with_bigram_row(&system, name, (row.0.to_le_bytes().to_vec(), row.1));
    let context = ContextCore::open(
        &system,
        Some(&user),
        PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .expect("context");
    let store = context.user_store().expect("user store");
    let mut inst = context.alloc_instance().expect("instance");
    assert_eq!(inst.parse_full_more("nihao"), 5);
    let list = inst.session.candidates_at(0).expect("candidates");
    let index = list
        .iter()
        .position(|candidate| candidate.text() == "你")
        .expect("你 is a candidate");
    inst.session.select(index).expect("choose 你");
    assert!(inst.session.guess_sentence().expect("guess"));
    if let Some(text) = reparse {
        inst.session.replace_raw(text).expect("re-parse");
    }
    let generation = store.generation();
    let result = inst.train(0);
    let untouched = store.generation() == generation && !store.is_modified();
    drop(inst);
    drop(context);
    let _ = std::fs::remove_dir_all(&user);
    (result, untouched)
}

/// `train_result3` runs each step's checks in order — the constraint
/// (`phonetic_lookup.h:868`), the bigram half (`get_total_freq` :884, the
/// `get_freq` walk :888, `set_freq` :903), then the span asserts through
/// `increase_pronunciation_possibility` (:925, `phonetic_key_matrix.cpp:661-664`)
/// — so the first failing check in step order wins. A stale span at step
/// one (`'ni` empties the start column) comes before a short gram at step
/// two (你's own gram).
#[test]
fn a_stale_span_at_step_one_wins_over_a_short_gram_at_step_two() {
    let (ni, _) = ni_and_hao(&fixture_dir());
    let short = vec![0xa5, 0x5a, 0x01];
    let (control, _) = train_two_steps("order-short-2-control", (ni, short.clone()), None);
    assert_eq!(
        control,
        Err(EngineError::ShortTrainingPredecessor { token: ni })
    );
    let (result, untouched) = train_two_steps("order-short-2", (ni, short), Some("'ni"));
    assert!(
        matches!(
            result,
            Err(EngineError::StaleTrainingSpan { position: 0, .. })
        ),
        "{result:?}"
    );
    assert!(untouched);
}

/// The reverse: a short gram at step one (the sentence start's) comes
/// before the stale span at step two (`ni` leaves 好's span past the
/// matrix).
#[test]
fn a_short_gram_at_step_one_wins_over_a_stale_span_at_step_two() {
    let short = vec![0xa5, 0x5a, 0x01];
    let (stale, _) = train_two_steps("order-stale-2-control", (2, vec![0; 4]), Some("ni"));
    assert!(
        matches!(
            stale,
            Err(EngineError::StaleTrainingSpan { position: 2, .. })
        ),
        "{stale:?}"
    );
    let (result, untouched) = train_two_steps("order-short-1", (SENTENCE_START, short), Some("ni"));
    assert_eq!(
        result,
        Err(EngineError::ShortTrainingPredecessor {
            token: SENTENCE_START
        })
    );
    assert!(untouched);
}

/// The same pair for the `set_freq` miss: a stale span at step one comes
/// before a miss at step two (你 → 好 on 你's unsorted gram).
#[test]
fn a_stale_span_at_step_one_wins_over_a_set_freq_miss_at_step_two() {
    let (ni, hao) = ni_and_hao(&fixture_dir());
    let gram = set_freq_miss_gram(hao);
    let (control, _) = train_two_steps("order-miss-2-control", (ni, gram.clone()), None);
    assert_eq!(control, Err(EngineError::TrainingSetFreqMiss { token: ni }));
    let (result, untouched) = train_two_steps("order-miss-2", (ni, gram), Some("'ni"));
    assert!(
        matches!(
            result,
            Err(EngineError::StaleTrainingSpan { position: 0, .. })
        ),
        "{result:?}"
    );
    assert!(untouched);
}

/// And a miss at step one (the sentence start → 你) comes before the stale
/// span at step two.
#[test]
fn a_set_freq_miss_at_step_one_wins_over_a_stale_span_at_step_two() {
    let (ni, _) = ni_and_hao(&fixture_dir());
    let (result, untouched) = train_two_steps(
        "order-miss-1",
        (SENTENCE_START, set_freq_miss_gram(ni)),
        Some("ni"),
    );
    assert_eq!(
        result,
        Err(EngineError::TrainingSetFreqMiss {
            token: SENTENCE_START
        })
    );
    assert!(untouched);
}
