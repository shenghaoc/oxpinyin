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
    let conf = oxpinyin_runtime::TableConf::load_from_dir(system)
        .unwrap_or_else(|_| oxpinyin_runtime::TableConf::stock());
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

/// The sentence start's user gram is three bytes and the fixture has no
/// system gram for it, so the merged row is that value as stored
/// (`ngram.cpp:286-290` at the pin `074a2219`). Forcing 你 makes the
/// guess's first step a one-step constraint, whose `get_freq` starts four
/// bytes in, past the value's end, and walks on (`ngram.cpp:239-248`,
/// through `phonetic_lookup.h:605`; register row 98). The guess refuses
/// before any training could load the gram, so the train-side refusal is
/// unreachable here; `a_short_predecessor_value_refuses_both_training_paths`
/// (`store_libpinyin.rs`) holds it on the store.
#[test]
fn a_short_start_gram_refuses_the_guess_before_any_training() {
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
    let list = inst.session.candidates_at(0).expect("candidates");
    let index = list
        .iter()
        .position(|candidate| candidate.text() == "你")
        .expect("你 is a candidate");
    inst.session.select(index).expect("choose 你");
    let generation = store.generation();
    assert_eq!(
        inst.session.guess_sentence(),
        Err(EngineError::UnsafeLookupPredecessor {
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

/// An unpartitioned gram has no promised search position. Reads preserve
/// its bytes and order and return only valid items; repeating the same
/// lookup and training from the same state gives the same result. The
/// pin's observable difference is covered by the policy contract cases.
#[test]
fn unsorted_gram_reads_and_training_are_deterministic_without_reordering() {
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
    let gram = unsorted_neighbor_gram(ni);
    let original: Vec<_> = oxpinyin_core::single_gram::retrieve_all(&gram).collect();
    let mut previous = None;
    for run in 0..2 {
        let user = user_dir_with_bigram_row(
            &system,
            &format!("unsorted-training-{run}"),
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
        for probe in ni - 4..=ni + 2 {
            let row = store
                .bigram_row(SENTENCE_START)
                .expect("row")
                .expect("gram");
            assert_eq!(row, gram);
            let first = oxpinyin_core::single_gram::lower_bound(&row, probe);
            assert!(first <= original.len());
            assert_eq!(oxpinyin_core::single_gram::lower_bound(&row, probe), first);
            let found = oxpinyin_core::single_gram::get_freq(&row, probe);
            assert_eq!(oxpinyin_core::single_gram::get_freq(&row, probe), found);
            if let Some(freq) = found {
                assert!(original.contains(&(probe, freq)));
            }
            let count = store.bigram_count(SENTENCE_START, probe).expect("count");
            assert_eq!(
                store.bigram_count(SENTENCE_START, probe).expect("repeat"),
                count
            );
            assert!(
                count == 0
                    || original
                        .iter()
                        .any(|&(item, freq)| item == probe && u64::from(freq) == count)
            );
        }
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
        assert!(!store.is_modified());
        assert_eq!(
            store
                .bigram_row(SENTENCE_START)
                .expect("reads preserve bytes"),
            Some(gram.clone())
        );
        let result = inst.train(0);
        let trained = store
            .bigram_row(SENTENCE_START)
            .expect("row")
            .expect("gram");
        match &result {
            Ok(true) => {
                let retained: Vec<_> = oxpinyin_core::single_gram::retrieve_all(&trained)
                    .filter(|&(token, _)| token != ni)
                    .collect();
                assert_eq!(
                    retained, original,
                    "training retains existing records in order"
                );
            }
            Err(EngineError::TrainingSetFreqMiss { token }) => {
                assert_eq!(*token, SENTENCE_START);
                assert_eq!(store.generation(), generation, "refusal is atomic");
                assert!(!store.is_modified());
                assert_eq!(trained, gram);
            }
            other => panic!("unexpected training result: {other:?}"),
        }
        let observation = (result, trained);
        if let Some(previous) = &previous {
            assert_eq!(
                &observation, previous,
                "same input and state are deterministic"
            );
        }
        previous = Some(observation);
        drop(inst);
        drop(context);
        let _ = std::fs::remove_dir_all(&user);
    }
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

/// An unpartitioned row around an absent token, kept in stored order.
fn unsorted_neighbor_gram(token: u32) -> Vec<u8> {
    let mut gram = 5_u32.to_le_bytes().to_vec();
    for item in [token - 4, token - 3, token - 2, token + 1, token - 1] {
        gram.extend_from_slice(&item.to_le_bytes());
        gram.extend_from_slice(&1_u32.to_le_bytes());
    }
    gram
}

/// Sorted records around an absent token have an exact search contract.
fn sorted_neighbor_gram(token: u32) -> Vec<u8> {
    let mut gram = 5_u32.to_le_bytes().to_vec();
    for item in [token - 4, token - 3, token - 2, token - 1, token + 1] {
        gram.extend_from_slice(&item.to_le_bytes());
        gram.extend_from_slice(&1_u32.to_le_bytes());
    }
    gram
}

/// Plants `row`, forces 你 on `nihao` and guesses; the row trains two
/// steps — the sentence start → 你, then 你 → 好. `reparse`, when given,
/// replaces the input without a fresh guess, leaving the stale result the
/// pin still trains (`phonetic_lookup.h:921`). Answers the train's result
/// — or the guess's, when the guess refuses and nothing is trained — and
/// whether the store stayed untouched.
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
    let generation = store.generation();
    let result = match inst.session.guess_sentence() {
        Ok(guessed) => {
            assert!(guessed, "guess");
            if let Some(text) = reparse {
                inst.session.replace_raw(text).expect("re-parse");
            }
            inst.train(0)
        }
        Err(error) => Err(error),
    };
    let untouched = store.generation() == generation && !store.is_modified();
    drop(inst);
    drop(context);
    let _ = std::fs::remove_dir_all(&user);
    (result, untouched)
}

/// With `DYNAMIC_ADJUST` (`1 << 9`) set, the pin's parse merges no gram
/// (`pinyin_parse_more_full_pinyins`, `pinyin.cpp:1497-1520` at the pin
/// `074a2219`), so a sentence-start gram it cannot read leaves the parse
/// whole. Only the candidate guess merges it, before its search
/// (`pinyin.cpp:2200-2216`), and reads its total per candidate
/// (`:1848`), where the pin asserts (`ngram.cpp:49`, `memory_chunk.h:390`;
/// register row 85). The eager refresh every parse runs must not refuse it.
#[test]
fn a_short_start_gram_leaves_the_parse_and_refuses_the_candidate_guess() {
    let system = fixture_dir();
    let short = vec![0xa5, 0x5a, 0x01];
    let user = user_dir_with_bigram_row(
        &system,
        "short-dynamic-adjust",
        (SENTENCE_START.to_le_bytes().to_vec(), short.clone()),
    );
    let context = ContextCore::open(
        &system,
        Some(&user),
        PINYIN_DEFAULT_OPTION_WORD | (1 << 9),
        UserConfLaw::Pinyin,
    )
    .expect("context");
    let store = context.user_store().expect("user store");
    let mut inst = context.alloc_instance().expect("instance");
    assert_eq!(inst.parse_full_more("nihao"), 5);
    let refusal = Err(EngineError::ShortLookupPredecessor {
        token: SENTENCE_START,
    });
    assert_eq!(
        inst.session.candidates_at(0).map(|list| list.len()),
        refusal
    );
    assert_eq!(
        inst.session.candidates_ending_at(5).map(|list| list.len()),
        refusal
    );
    assert_eq!(store.bigram_row(SENTENCE_START).expect("row"), Some(short));
    drop(inst);
    drop(context);
    let _ = std::fs::remove_dir_all(&user);
}

/// `train_result3` runs each step's checks in order — the constraint
/// (`phonetic_lookup.h:868`), the bigram half (`get_total_freq` :884, the
/// `get_freq` walk :888, `set_freq` :903), then the span asserts through
/// `increase_pronunciation_possibility` (:925, `phonetic_key_matrix.cpp:661-664`)
/// — so the first failing check in step order wins. A short gram never
/// reaches it: the guess before training merges and reads the same grams
/// first. 你's short
/// gram, with no system gram beside it, is the merged row as stored, and
/// `search` asserts on its total at the step after 你 (`ngram.cpp:165`,
/// through `phonetic_lookup.h:625`; register row 85), with or without a
/// later stale span.
#[test]
fn a_short_gram_at_step_two_refuses_the_guess_first() {
    let (ni, _) = ni_and_hao(&fixture_dir());
    let short = vec![0xa5, 0x5a, 0x01];
    for (name, reparse) in [
        ("order-short-2-control", None),
        ("order-short-2", Some("'ni")),
    ] {
        let (result, untouched) = train_two_steps(name, (ni, short.clone()), reparse);
        assert_eq!(
            result,
            Err(EngineError::ShortLookupPredecessor { token: ni })
        );
        assert!(untouched);
    }
}

/// The sentence start's short gram at step one: the forced 你 makes the
/// guess's first step a one-step `get_freq` on it, which reads past the
/// value (`ngram.cpp:239-248`, through `phonetic_lookup.h:605`; register
/// row 98), so the guess refuses before the stale span at step two is
/// trained. The stale span alone still refuses the train.
#[test]
fn a_short_gram_at_step_one_refuses_the_guess_first() {
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
        Err(EngineError::UnsafeLookupPredecessor {
            token: SENTENCE_START
        })
    );
    assert!(untouched);
}

/// A stale span at step one refuses before training step two. The sorted
/// gram control trains successfully, so it introduces no competing fault.
#[test]
fn a_stale_span_at_step_one_precedes_training_a_sorted_gram_at_step_two() {
    let (ni, hao) = ni_and_hao(&fixture_dir());
    let gram = sorted_neighbor_gram(hao);
    let (control, _) = train_two_steps("order-miss-2-control", (ni, gram.clone()), None);
    assert_eq!(control, Ok(true));
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

/// Sorted-gram training at step one passes preflight, so the stale span
/// at step two refuses the whole train without applying the first step.
#[test]
fn a_sorted_gram_at_step_one_keeps_a_later_stale_span_refusal_atomic() {
    let (ni, _) = ni_and_hao(&fixture_dir());
    let (result, untouched) = train_two_steps(
        "order-miss-1",
        (SENTENCE_START, sorted_neighbor_gram(ni)),
        Some("ni"),
    );
    assert!(
        matches!(
            result,
            Err(EngineError::StaleTrainingSpan { position: 2, .. })
        ),
        "{result:?}"
    );
    assert!(untouched);
}
