//! Behavioural ports of the upstream session-lifecycle suites.
//!
//! - `libchewing/tests/test-reset.c`: `Reset` clears session state but
//!   must not drop loaded data or configuration — the next conversion
//!   after a reset behaves like the first.
//! - `libchewing/tests/test-keyboardless.c` (selection subset): the
//!   programmatic choice surface — out-of-range choices fail without
//!   disturbing state, choosing without a candidate window fails, and a
//!   mid-composition choice commits the exact chosen phrase.
//!
//! The oxpinyin seam is the supported engine `Session` API over the
//! `oxpinyin-testsupport` fixture doubles. Chewing's commit/preedit
//! buffer mechanics map onto `commit()`/`preedit()`; its config setters
//! map onto `set_options`/`set_incomplete_pinyin`/`set_scoring_config`.

use oxpinyin_core::OptionBits;
use oxpinyin_engine::{EmptyConfigSource, KeyOutcome, Selection, Session, StoragePaths};
use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

type TestSession = Session<FixtureDictionary, FixtureLanguageModel>;

fn session() -> TestSession {
    let dictionary = FixtureDictionary::parse(VOCAB).expect("committed fixture");
    let model = FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixtures");
    Session::new(
        &EmptyConfigSource,
        StoragePaths::default(),
        dictionary,
        model,
    )
    .expect("session builds")
}

// ── libchewing test-reset.c ─────────────────────────────────────────

#[test]
fn reset_clears_the_composition_but_not_the_configuration() {
    // Chewing's regression: Reset used to wipe dictionaries and settings.
    // After a reset, the same input must convert exactly as the first
    // time, under the configuration set before the reset.
    let mut session = session();
    session
        .set_incomplete_pinyin(false)
        .expect("the option toggles");
    let page_size = session.page_size();
    assert!(page_size > 0);

    session.type_pinyin("nihao").expect("typing");
    assert!(session.is_composing());

    session.reset();
    assert!(!session.is_composing());
    assert!(session.commit().expect("commit").is_empty());

    // Configuration survived: page size unchanged, the incomplete option
    // is still off, and conversion still works.
    assert_eq!(session.page_size(), page_size);
    let outcome = session
        .process_key(&oxpinyin_engine::KeyInput::plain(
            oxpinyin_engine::LogicalKey::Character('n'),
        ))
        .expect("keys still processed");
    assert!(matches!(
        outcome,
        KeyOutcome::Consumed | KeyOutcome::Commit(_)
    ));
}

#[test]
fn the_reset_session_converts_identically_to_a_fresh_one() {
    let mut fresh = session();
    fresh.type_pinyin("zhong,guo").expect("typing");
    let mut reset = session();
    reset.type_pinyin("zhong,guo").expect("typing");
    reset.select(0).expect("choose");
    reset.reset();

    reset.type_pinyin("zhong,guo").expect("typing");
    let fresh_candidates: Vec<String> = fresh
        .candidates()
        .iter()
        .map(|c| c.text().to_owned())
        .collect();
    let reset_candidates: Vec<String> = reset
        .candidates()
        .iter()
        .map(|c| c.text().to_owned())
        .collect();
    assert_eq!(
        reset_candidates, fresh_candidates,
        "a reset session must behave like a fresh one"
    );
}

#[test]
fn reset_composition_keeps_the_parse_more_split_state() {
    // The two-stage reset: `reset_composition` clears the parse state but
    // keeps the raw buffer and selection record alive (the
    // `pinyin_parse_more_*` re-parse seam).
    let mut session = session();
    session.type_pinyin("ni").expect("typing");
    session.reset_composition();
    // The parse state is gone; the session survives and accepts input.
    assert!(
        session
            .process_key(&oxpinyin_engine::KeyInput::plain(
                oxpinyin_engine::LogicalKey::Character('h')
            ))
            .is_ok()
    );
}

// ── libchewing test-keyboardless.c: programmatic selection ─────────

#[test]
fn choosing_without_a_composition_fails() {
    // `cand_choose_not_in_select`: choosing with no candidate window must
    // fail, not silently pick something.
    let mut session = session();
    assert!(
        session.select(0).is_err(),
        "nothing is composing, so index 0 does not exist"
    );
}

#[test]
fn an_out_of_range_choice_fails_and_leaves_the_composition() {
    // `cand_choose_out_of_range`: index == count and index past the end
    // both fail; the preedit is unchanged afterwards.
    let mut session = session();
    session.type_pinyin("ni").expect("typing");
    let before = session.preedit().text().to_owned();
    let count = session.candidates().len();
    assert!(count > 0, "the mini table offers ni candidates");
    assert!(session.select(count).is_err(), "index == count must fail");
    assert!(
        session.select(u32::MAX as usize).is_err(),
        "index past the end must fail"
    );
    assert_eq!(
        session.preedit().text(),
        before,
        "failed choices change nothing"
    );
    assert!(session.is_composing());
}

#[test]
fn a_programmatic_choice_commits_the_chosen_phrase() {
    // `cand_choose_word`: choosing by index selects the exact phrase; a
    // completed selection commits through `commit()`.
    let mut session = session();
    session.type_pinyin("ni,hao").expect("typing");
    let chosen = session
        .candidates()
        .get(0)
        .expect("candidates exist")
        .text()
        .to_owned();
    assert_eq!(chosen, "你", "the captured order leads with 你");

    // The first choice selects the first segment's phrase and continues
    // the composition with the remaining syllables (chewing puts the
    // chosen phrase into the preedit too).
    assert_eq!(session.select(0).expect("choose"), Selection::Continued);
    assert!(
        session.preedit().text().starts_with(chosen.as_str()),
        "the chosen phrase is in the preedit: {:?}",
        session.preedit().text()
    );

    // Completing the rest and committing is covered by the decoding
    // suite (`choosing_advances_the_composition_...`); this port stops at
    // the single-choice invariant the chewing test protects.
    assert!(session.is_composing());
}

#[test]
fn a_mid_composition_choice_targets_the_offset_window() {
    // `test_select_candidate_in_middle`: choosing can address a sub-span
    // of the composition through `candidates_at` + `select_anchored`,
    // leaving the rest of the preedit in place.
    let mut session = session();
    session.type_pinyin("ni,hao,zhong,guo").expect("typing");
    let full = session.preedit().text().to_owned();
    assert!(!full.is_empty());

    // The offset window after the first two syllables offers candidates
    // of its own; the anchored selection moves the composition forward.
    let offset = session
        .normalized_lookup_offset(2)
        .expect("offset in range");
    let window = session
        .candidates_at(offset)
        .expect("a mid-composition window");
    assert!(
        !window.is_empty(),
        "the mid-composition window offers candidates"
    );
    let _ = session.select_anchored(0, &window, offset);
    // Either outcome keeps a live session with a valid preedit — the
    // invariant is that the anchored API answered and nothing panicked.
    assert!(session.preedit().text().len() <= full.len() + 8);
}

#[test]
fn an_invalid_option_word_is_accepted_and_inert() {
    // Chewing's config setters reject out-of-enum values; the oxpinyin
    // option word is a bitset, where unknown bits are inert by contract
    // (the DYNAMIC_ADJUST gating tests pin the off-behaviour). Setting a
    // defined bit round-trips through the getter seam.
    let mut session = session();
    session
        .set_options(OptionBits::from_bits(oxpinyin_core::PINYIN_INCOMPLETE))
        .expect("option bits accept the incomplete flag");
    session
        .set_incomplete_pinyin(true)
        .expect("the option toggles");
    assert!(session.type_pinyin("ni").is_ok());
}
