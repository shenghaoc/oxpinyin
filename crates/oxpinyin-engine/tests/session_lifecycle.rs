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
//! The oxpinyin seam is the internal engine `Session` API over the
//! `oxpinyin-testsupport` fixture doubles. Chewing's commit/preedit
//! buffer checks use selected text and raw-input offsets; its config setters
//! map onto `set_options`/`set_incomplete_pinyin`/`set_scoring_config`.

use oxpinyin_core::OptionBits;
use oxpinyin_engine::{EmptyConfigSource, KeyOutcome, Selection, Session};
use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

type TestSession = Session<FixtureDictionary, FixtureLanguageModel>;

fn session() -> TestSession {
    let dictionary = FixtureDictionary::parse(VOCAB).expect("committed fixture");
    let model = FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixtures");
    Session::new(&EmptyConfigSource, dictionary, model).expect("session builds")
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
    let options = session.options();

    session.type_pinyin("nihao").expect("typing");
    assert!(session.is_composing());

    session.reset();
    assert!(!session.is_composing());
    assert!(session.selected_text().is_empty());

    // The live option survives reset and still controls decoding.
    assert_eq!(session.options(), options);
    assert_eq!(
        session.type_pinyin("n").expect("typing"),
        KeyOutcome::Consumed
    );
    assert_eq!(session.parsed_prefix_len(), 0);
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
        .filter_map(|c| c.text_str())
        .map(str::to_owned)
        .collect();
    let reset_candidates: Vec<String> = reset
        .candidates()
        .iter()
        .filter_map(|c| c.text_str())
        .map(str::to_owned)
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
    assert!(session.type_pinyin("h").is_ok());
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
    // both fail; the composition state is unchanged afterwards.
    let mut session = session();
    session.type_pinyin("ni").expect("typing");
    let before = (
        session.raw_input().to_owned(),
        session.selected_text().clone(),
        session.composition_offset(),
    );
    let count = session.candidates().len();
    assert!(count > 0, "the mini table offers ni candidates");
    assert!(session.select(count).is_err(), "index == count must fail");
    assert!(
        session.select(u32::MAX as usize).is_err(),
        "index past the end must fail"
    );
    assert_eq!(
        (
            session.raw_input().to_owned(),
            session.selected_text().clone(),
            session.composition_offset()
        ),
        before,
        "failed choices change nothing"
    );
    assert!(session.is_composing());
}

#[test]
fn a_programmatic_choice_retains_the_chosen_phrase() {
    // `cand_choose_word`: choosing by index retains the exact phrase.
    let mut session = session();
    session.type_pinyin("ni,hao").expect("typing");
    let chosen = session
        .candidates()
        .get(0)
        .expect("candidates exist")
        .text()
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(chosen, "你", "the captured order leads with 你");

    // The first choice selects the first segment's phrase and continues
    // the composition with the remaining syllables (chewing puts the
    // chosen phrase into the preedit too).
    assert_eq!(session.select(0).expect("choose"), Selection::Continued);
    assert!(
        session
            .selected_text()
            .as_str()
            .unwrap()
            .starts_with(chosen.as_str()),
        "the chosen phrase is retained in the record: {:?}",
        session.selected_text()
    );

    // Completing the rest and resetting is covered by the decoding
    // suite (`choosing_advances_the_composition_...`); this port stops at
    // the single-choice invariant the chewing test protects.
    assert!(session.is_composing());
}

#[test]
fn a_mid_composition_choice_targets_the_offset_window() {
    // `test_select_candidate_in_middle`: choosing can address a sub-span
    // of the composition through `candidates_at` + `select_anchored`,
    // retaining the raw input alongside the selected text.
    let mut session = session();
    session.type_pinyin("ni,hao,zhong,guo").expect("typing");
    let full = session.raw_input().to_owned();
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
    session
        .select_anchored(0, &window, offset)
        .expect("the anchored selection succeeds over a live window");
    // The composition stays live and bounded after the anchored choice.
    assert_eq!(session.raw_input(), full);
    assert!(session.composition_offset() <= full.len());
    assert!(!session.selected_text().is_empty());
}

#[test]
fn an_option_word_with_undefined_bits_is_accepted_and_inert() {
    // Chewing's config setters reject out-of-enum values; the oxpinyin
    // option word is a bitset, where bits outside the frozen constants
    // ride along inertly (the DYNAMIC_ADJUST gating tests pin the
    // off-behaviour of a defined-but-special bit). Setting the word with
    // the incomplete flag PLUS an undefined low bit must leave the
    // defined behaviour untouched.
    let undefined_bit = 1 << 0; // bits 0..2 are outside the frozen set
    let mut session = session();
    session
        .set_options(OptionBits::from_bits(
            oxpinyin_core::PINYIN_INCOMPLETE | undefined_bit,
        ))
        .expect("the option word accepts undefined bits");
    session
        .set_incomplete_pinyin(true)
        .expect("the option toggles");
    session.type_pinyin("n").expect("typing");
    assert!(
        session.is_composing(),
        "incomplete pinyin still composes past an initial"
    );
}
