//! The session's unit tests, moved verbatim from `session.rs` (2026-09-08).

#[test]
fn ignored_type_input_preserves_exact_mode() {
    let mut session = session();
    let segments: Vec<oxpinyin_core::graph::ExactSegment> = {
        use oxpinyin_core::graph::ExactSegment;
        let ni = oxpinyin_core::SyllableKey::from_text("ni").expect("ni");
        let hao = oxpinyin_core::SyllableKey::from_text("hao").expect("hao");
        vec![
            ExactSegment::new(0, 2, ni, 0),
            ExactSegment::new(3, 6, hao, 0),
        ]
    };
    session
        .replace_raw_exact("ni'hao", &segments)
        .expect("replace");
    // A batch input whose every character is filtered (the space) is
    // Ignored and must not exit exact mode.
    assert_eq!(
        session.type_pinyin("  ").expect("ignored input"),
        KeyOutcome::Ignored
    );
    assert!(session.input.exact().len() == 2);
    // An accepted character does exit exact mode.
    assert_eq!(
        session.type_pinyin("h").expect("typed"),
        KeyOutcome::Consumed
    );
    assert!(session.input.exact().is_empty());
}

#[test]
fn a_rejected_character_preserves_exact_mode_and_backspace_clears_it() {
    let mut session = session();
    let segments: Vec<oxpinyin_core::graph::ExactSegment> = {
        use oxpinyin_core::graph::ExactSegment;
        let ni = oxpinyin_core::SyllableKey::from_text("ni").expect("ni");
        vec![ExactSegment::new(0, 2, ni, 0)]
    };
    session.replace_raw_exact("ni", &segments).expect("replace");
    // An over-capacity or non-input character is Ignored: exact mode
    // survives because raw never changed.
    assert_eq!(
        session
            .process_key(&KeyInput::plain(LogicalKey::Backspace))
            .expect("erase"),
        KeyOutcome::Consumed
    );
    assert!(
        session.input.exact().is_empty(),
        "erase must drop the exact chain"
    );
}

#[test]
fn full_parsed_len_reflects_the_exact_chain() {
    let mut session = session();
    let segments: Vec<oxpinyin_core::graph::ExactSegment> = {
        use oxpinyin_core::graph::ExactSegment;
        let ni = oxpinyin_core::SyllableKey::from_text("ni").expect("ni");
        let hao = oxpinyin_core::SyllableKey::from_text("hao").expect("hao");
        vec![
            ExactSegment::new(0, 2, ni, 0),
            ExactSegment::new(3, 6, hao, 0),
        ]
    };
    session
        .replace_raw_exact("ni'hao", &segments)
        .expect("replace");
    assert_eq!(session.full_parsed_len(), 6);
    assert_eq!(session.parsed_prefix_len(), 6);
}

#[test]
fn an_anchor_inside_an_exact_segment_decodes_nothing() {
    use oxpinyin_core::graph::ExactSegment;
    let mut session = session();
    let ni_hao: Vec<ExactSegment> = {
        let ni = oxpinyin_core::SyllableKey::from_text("ni").expect("ni");
        let hao = oxpinyin_core::SyllableKey::from_text("hao").expect("hao");
        vec![
            ExactSegment::new(0, 2, ni, 0),
            ExactSegment::new(3, 6, hao, 0),
        ]
    };
    session
        .replace_raw_exact("ni'hao", &ni_hao)
        .expect("replace");
    // Anchor 1 sits inside the `ni` segment: the tail `hao` must not
    // decode across the skipped `i'` bytes.
    let raw = session.input.as_str().to_owned();
    let graph = session
        .build_graph_at(1, &raw.as_bytes()[1..])
        .expect("anchor inside a segment answers an empty graph");
    assert!(graph.edges().is_empty());
    assert_eq!(graph.consumed(), 0);
    // A boundary anchor (2, the end of the first segment) still decodes.
    let graph = session
        .build_graph_at(2, &raw.as_bytes()[2..])
        .expect("boundary anchor builds");
    assert_eq!(graph.edges().len(), 1);
}

use oxpinyin_core::{
    Cost, Dictionary, LanguageModel, NbestStepCosts, PhraseEntry, PhraseToken, SyllableKey,
    UserModel,
};
use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

use super::{KeyOutcome, MAX_INPUT_BYTES, Selection, Session};
use crate::config::EmptyConfigSource;
use crate::error::EngineError;
use crate::key::{KeyInput, LogicalKey, Modifiers};
use crate::preedit::SpanStyle;
use crate::storage::StoragePaths;

/// A backend that answers nothing, so these tests measure the state
/// machine and not a data set.
struct Silent;

impl Dictionary for Silent {
    type Entry = PhraseEntry;
    type Error = EngineError;
    type Syllable = SyllableKey;

    fn lookup(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
        Ok(Vec::new())
    }
}

impl LanguageModel for Silent {
    type Error = EngineError;
    type Token = PhraseToken;

    fn score(
        &self,
        _history: &[PhraseToken],
        _token: &PhraseToken,
        edge_cost: Cost,
    ) -> Result<Cost, EngineError> {
        Ok(edge_cost)
    }
}

fn session() -> Session<Silent, Silent> {
    Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        Silent,
        Silent,
    )
    .expect("opening a session cannot fail yet")
}

fn type_text(session: &mut Session<Silent, Silent>, text: &str) {
    for character in text.chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
}

#[test]
fn only_parser_syntax_extends_the_composition() {
    let mut session = session();
    type_text(&mut session, "ni'hao");
    assert_eq!(session.raw_input(), "ni'hao");
    assert!(session.is_composing());

    for ignored in ['N', '1', ' ', '!', '\u{4f60}'] {
        assert_eq!(
            session
                .process_key(&KeyInput::character(ignored))
                .expect("ignored keys cannot fail"),
            KeyOutcome::Ignored,
            "character: {ignored:?}"
        );
    }
    assert_eq!(session.raw_input(), "ni'hao");
}

#[test]
fn type_pinyin_keeps_printable_junk_in_the_raw_buffer() {
    // Batch path (parity harness): printable ASCII including junk is kept
    // so the decoder sees the fixture string; process_key still ignores it.
    let mut session = session();
    session
        .type_pinyin("b#ing")
        .expect("batch typing cannot fail");
    assert_eq!(session.raw_input(), "b#ing");
}

#[test]
fn type_pinyin_still_filters_stop_bytes() {
    // The frozen half of the parse-termination split: `type_pinyin`
    // keeps its printable-ASCII accept set (F1), so the corpus and
    // sentence pins that feed through it cannot reach the loosened
    // `replace_raw` seam — a space never enters `raw` here.
    let mut session = session();
    session
        .type_pinyin("ni hao")
        .expect("batch typing cannot fail");
    assert_eq!(session.raw_input(), "nihao");
}

#[test]
fn replace_raw_keeps_the_bytes_the_pin_stops_at() {
    // Class B2: the pin accepts any input string and stops consuming
    // at the first byte no key matches (pinyin_parser2.cpp:237-328);
    // the capi parse seam must let those bytes reach the decoder.
    // Measured on the rebuilt pin: `ni hao` parses 2, `，nihao`
    // parses 0 (uncovered-surface differential, phase B).
    let mut session = session();
    session.replace_raw("ni hao").expect("cannot fail");
    assert_eq!(session.raw_input(), "ni hao");
    assert_eq!(session.full_parsed_len(), 2, "the space stops the parse");

    session.replace_raw("\u{ff0c}nihao").expect("cannot fail");
    assert_eq!(
        session.full_parsed_len(),
        0,
        "the full-width comma stops at byte 0"
    );

    session.replace_raw("nihao").expect("cannot fail");
    assert_eq!(session.full_parsed_len(), 5, "clean input unaffected");
}

#[test]
fn replace_raw_consumes_trailing_and_standalone_apostrophe_runs() {
    // The inherited apostrophe class (ledgered on
    // fix/cursor-offset-normalization, folded into the termination
    // law): the pin's DP propagation consumes `'` bytes no key
    // covers — `ni'` parses 3, `nihao'` parses 6, `'''` parses 3
    // (the F-E-14 table).
    let mut session = session();
    session.replace_raw("ni'").expect("cannot fail");
    assert_eq!(session.full_parsed_len(), 3);

    session.replace_raw("nihao'").expect("cannot fail");
    assert_eq!(session.full_parsed_len(), 6);

    session.replace_raw("'''").expect("cannot fail");
    assert_eq!(session.full_parsed_len(), 3);

    session.replace_raw("ni'hao").expect("cannot fail");
    assert_eq!(
        session.full_parsed_len(),
        6,
        "internal runs stay covered by edges"
    );
}

#[test]
fn command_modifiers_leave_the_session_alone() {
    let mut session = session();
    type_text(&mut session, "ni");

    for modifier in [Modifiers::CONTROL, Modifiers::ALT, Modifiers::SUPER] {
        let input = KeyInput::new(LogicalKey::Character('h'), modifier, "h");
        assert_eq!(
            session.process_key(&input).expect("no failure"),
            KeyOutcome::Ignored
        );
    }
    assert_eq!(session.raw_input(), "ni");

    let shifted = KeyInput::new(LogicalKey::Character('h'), Modifiers::SHIFT, "H");
    assert_eq!(
        session.process_key(&shifted).expect("no failure"),
        KeyOutcome::Consumed
    );
    assert_eq!(session.raw_input(), "nih");
}

#[test]
fn backspace_erases_then_reports_nothing_to_do() {
    let mut session = session();
    type_text(&mut session, "ni");

    let backspace = KeyInput::plain(LogicalKey::Backspace);
    assert_eq!(
        session.process_key(&backspace).expect("no failure"),
        KeyOutcome::Consumed
    );
    assert_eq!(session.raw_input(), "n");
    session.process_key(&backspace).expect("no failure");
    assert_eq!(session.raw_input(), "");
    assert_eq!(
        session.process_key(&backspace).expect("no failure"),
        KeyOutcome::Ignored
    );
}

#[test]
fn enter_commits_and_escape_discards() {
    let mut session = session();
    type_text(&mut session, "nihao");
    assert_eq!(
        session
            .process_key(&KeyInput::plain(LogicalKey::Enter))
            .expect("no failure"),
        KeyOutcome::Commit("nihao".to_owned())
    );
    assert!(!session.is_composing());
    assert_eq!(
        session
            .process_key(&KeyInput::plain(LogicalKey::Enter))
            .expect("no failure"),
        KeyOutcome::Ignored
    );

    type_text(&mut session, "nihao");
    assert_eq!(
        session
            .process_key(&KeyInput::plain(LogicalKey::Escape))
            .expect("no failure"),
        KeyOutcome::Consumed
    );
    assert_eq!(session.raw_input(), "");
}

#[test]
fn keys_the_session_does_not_use_change_nothing() {
    let mut session = session();
    type_text(&mut session, "ni");

    for key in [
        LogicalKey::Tab,
        LogicalKey::Delete,
        LogicalKey::Left,
        LogicalKey::Right,
        LogicalKey::Up,
        LogicalKey::Down,
        LogicalKey::Home,
        LogicalKey::End,
        LogicalKey::PageUp,
        LogicalKey::PageDown,
        LogicalKey::Unknown,
    ] {
        assert_eq!(
            session
                .process_key(&KeyInput::plain(key))
                .expect("no failure"),
            KeyOutcome::Ignored,
            "key: {key:?}"
        );
    }
    assert_eq!(session.raw_input(), "ni");
}

#[test]
fn the_preedit_covers_its_text_exactly() {
    let mut session = session();
    assert!(session.preedit().is_empty());

    type_text(&mut session, "nihao");
    let preedit = session.preedit();
    assert_eq!(preedit.text(), "nihao");
    assert_eq!(preedit.cursor(), 5);
    assert_eq!(preedit.spans().len(), 1);
    assert_eq!(preedit.spans()[0].style(), SpanStyle::Raw);
    assert_eq!(preedit.spans()[0].start(), 0);
    assert_eq!(preedit.spans()[0].end(), preedit.text().len());
}

#[test]
fn a_stale_candidate_index_is_an_error_not_a_panic() {
    let mut session = session();
    type_text(&mut session, "nihao");
    let len = session.candidates().len();
    assert_eq!(len, 1, "only the raw fallback exists before the decoder");

    for index in [len, len + 1, usize::MAX] {
        assert_eq!(
            session.select(index),
            Err(EngineError::CandidateIndexOutOfRange { index, len })
        );
    }
    assert_eq!(session.raw_input(), "nihao");
    assert!(session.candidates().get(usize::MAX).is_none());
}

#[test]
fn choosing_the_fallback_completes_the_composition() {
    let mut session = session();
    type_text(&mut session, "nihao");
    assert_eq!(
        session.select(0).expect("the fallback exists"),
        Selection::Completed
    );

    let preedit = session.preedit();
    assert_eq!(preedit.text(), "nihao");
    assert_eq!(preedit.spans().len(), 1);
    assert_eq!(preedit.spans()[0].style(), SpanStyle::Selected);
    assert!(session.candidates().is_empty());
    assert_eq!(session.commit().expect("no failure"), "nihao");
}

#[test]
fn space_accepts_the_first_candidate_and_commits() {
    let mut session = session();
    type_text(&mut session, "nihao");
    assert_eq!(
        session
            .process_key(&KeyInput::plain(LogicalKey::Space))
            .expect("no failure"),
        KeyOutcome::Commit("nihao".to_owned())
    );
    assert!(!session.is_composing());
    assert_eq!(
        session
            .process_key(&KeyInput::plain(LogicalKey::Space))
            .expect("no failure"),
        KeyOutcome::Ignored
    );
}

#[test]
fn backspace_undoes_a_selection_before_reporting_nothing_to_do() {
    let mut session = session();
    type_text(&mut session, "nihao");
    session.select(0).expect("the fallback exists");

    let backspace = KeyInput::plain(LogicalKey::Backspace);
    assert_eq!(
        session.process_key(&backspace).expect("no failure"),
        KeyOutcome::Consumed
    );
    assert_eq!(session.preedit().text(), "nihao");
    assert_eq!(session.preedit().spans()[0].style(), SpanStyle::Raw);
}

#[test]
fn a_full_buffer_ignores_further_input() {
    // Apostrophes on purpose: they fill the buffer without building a
    // decodable graph, so this measures the bound and not the decoder.
    let mut session = session();
    for _ in 0..MAX_INPUT_BYTES {
        session
            .process_key(&KeyInput::character('\''))
            .expect("no failure");
    }
    assert_eq!(session.raw_input().len(), MAX_INPUT_BYTES);
    assert_eq!(
        session
            .process_key(&KeyInput::character('\''))
            .expect("no failure"),
        KeyOutcome::Ignored
    );
    assert_eq!(session.raw_input().len(), MAX_INPUT_BYTES);
}

#[test]
fn configuration_and_paths_are_the_injected_data() {
    let session = session();
    assert_eq!(session.page_size(), 5);
    assert_eq!(session.paths().user_data_dir().to_str(), Some("user"));
}

#[test]
fn commit_on_an_empty_session_is_empty_text() {
    let mut session = session();
    assert_eq!(session.commit().expect("no failure"), "");
}

/// Authored mini vocabulary for the training tests: two single-key
/// phrases, no model bytes (`docs/testing/fixture-adapters.md`).
const TRAIN_VOCAB: &str =
    "token=1\tkeys=ni\ttext=你\tunigram=1000\ntoken=2\tkeys=hao\ttext=好\tunigram=900\n";

/// A [`UserModel`] that records every `observe` call instead of storing.
struct Recorder {
    observed: Vec<(Vec<PhraseToken>, PhraseToken)>,
}

impl UserModel for Recorder {
    type Token = PhraseToken;
    type Error = EngineError;

    fn score(&self, _history: &[Self::Token], _token: &Self::Token) -> Result<Cost, Self::Error> {
        Ok(0)
    }

    fn observe(&mut self, history: &[Self::Token], token: &Self::Token) -> Result<(), Self::Error> {
        self.observed.push((history.to_vec(), *token));
        Ok(())
    }
}

fn train_session() -> Session<FixtureDictionary, FixtureLanguageModel> {
    Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        FixtureDictionary::parse(TRAIN_VOCAB).expect("authored fixture"),
        FixtureLanguageModel::parse(TRAIN_VOCAB, "").expect("authored fixture"),
    )
    .expect("the fixtures open")
}

/// Selects the candidate carrying `token` after typing `text` (the
/// selection must exist: this is the sentence record the training path
/// walks).
fn type_and_select(
    session: &mut Session<FixtureDictionary, FixtureLanguageModel>,
    text: &str,
    token: u32,
) {
    for character in text.chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    let index = session
        .candidates()
        .iter()
        .position(|candidate| candidate.token() == Some(PhraseToken::new(token)))
        .expect("the fixture candidate is offered");
    session.select(index).expect("selection cannot fail");
}

#[test]
fn train_observes_each_recorded_token_after_its_prefix() {
    let mut session = train_session();
    type_and_select(&mut session, "ni", 1);
    type_and_select(&mut session, "hao", 2);
    assert_eq!(
        session.selected_tokens(),
        [PhraseToken::new(1), PhraseToken::new(2)]
    );

    let mut recorder = Recorder {
        observed: Vec::new(),
    };
    session.train(&mut recorder).expect("training cannot fail");
    // First token observes against an empty history (the store maps that
    // to sentence_start); the second observes after the first.
    assert_eq!(
        recorder.observed,
        vec![
            (Vec::new(), PhraseToken::new(1)),
            (vec![PhraseToken::new(1)], PhraseToken::new(2)),
        ]
    );

    // Re-training re-observes the same sentence: upstream has no guard
    // (a second pinyin_train doubles the counts), and neither does this.
    session.train(&mut recorder).expect("training cannot fail");
    assert_eq!(recorder.observed.len(), 4);
}

#[test]
fn train_reports_a_failing_user_model() {
    struct Failing;
    impl UserModel for Failing {
        type Token = PhraseToken;
        type Error = EngineError;

        fn score(
            &self,
            _history: &[Self::Token],
            _token: &Self::Token,
        ) -> Result<Cost, Self::Error> {
            Ok(0)
        }

        fn observe(
            &mut self,
            _history: &[Self::Token],
            _token: &Self::Token,
        ) -> Result<(), Self::Error> {
            Err(EngineError::UserModel("closed".to_owned()))
        }
    }

    let mut session = train_session();
    type_and_select(&mut session, "ni", 1);
    // The engine renders the model's error at the boundary: the failing
    // model reports an `EngineError::UserModel`, so the wrap doubles the
    // prefix — the point is that the failure surfaces, not that the text
    // is pretty.
    let error = session.train(&mut Failing).expect_err("the model fails");
    assert_eq!(
        error.to_string(),
        "user model error: user model error: closed"
    );
}

#[test]
fn composition_keys_report_the_selected_parse() {
    let mut plain = session();
    type_text(&mut plain, "nihao");
    let keys = plain.composition_keys().expect("the graph builds");
    let texts: Vec<&str> = keys.iter().map(|key| key.text()).collect();
    assert_eq!(texts, ["ni", "hao"]);

    // The apostrophe keeps xi'an from collapsing into xian.
    let mut split = session();
    type_text(&mut split, "xi'an");
    let keys = split.composition_keys().expect("the graph builds");
    let texts: Vec<&str> = keys.iter().map(|key| key.text()).collect();
    assert_eq!(texts, ["xi", "an"]);
}

#[test]
fn a_fallback_sentence_never_records_row_tokens() {
    use crate::nbest::NbestRow;

    let mut session = train_session();
    session.type_pinyin("nihao").expect("typing cannot fail");
    assert!(!session.sentence_lookup_active(), "no lookup has run yet");

    // One authored row whose text differs from the DP sentence the
    // fallback list also offers, so a fallback sentence candidate sits
    // beyond the row at a known place.
    session.sentence.rows = vec![NbestRow {
        text: "\u{884}".into(),
        tokens: vec![PhraseToken::new(9)],
        spans: Vec::new(),
        keys: 1,
        span: 2,
        cost: 0,
    }];
    session.refresh().expect("refresh cannot fail");
    assert!(
        !session.sentence_lookup_active(),
        "only guess_sentence activates the lookup"
    );

    // The row itself records its whole token path.
    assert_eq!(
        session.select(0).expect("the row is live"),
        Selection::Continued
    );
    assert_eq!(session.selected_tokens(), [PhraseToken::new(9)]);

    // A fallback sentence candidate — kind Sentence beyond the rows —
    // records nothing, and in particular never defaults through to
    // row zero's tokens.
    session.reset();
    session.type_pinyin("nihao").expect("typing cannot fail");
    session.sentence.rows = vec![NbestRow {
        text: "\u{884}".into(),
        tokens: vec![PhraseToken::new(9)],
        spans: Vec::new(),
        keys: 1,
        span: 2,
        cost: 0,
    }];
    session.refresh().expect("refresh cannot fail");
    let fallback = session
        .candidates()
        .iter()
        .position(|candidate| {
            candidate.kind() == crate::CandidateKind::Sentence
                && candidate.text() == "\u{4f60}\u{597d}"
        })
        .expect("the fallback sentence is offered beyond the row");
    assert!(fallback >= session.sentence.rows.len());
    session.select(fallback).expect("the index is live");
    assert!(
        session.selected_tokens().is_empty(),
        "a fallback sentence records no tokens"
    );
}

#[test]
fn a_shifted_row_records_its_own_rank_not_its_position() {
    use crate::nbest::NbestRow;

    let mut session = train_session();
    session.type_pinyin("nihao").expect("typing cannot fail");

    // Rows [好, 好, 浩]: the NBEST-wins dedup keeps the lower-index 好
    // and drops row 1, so the surviving 浩 row sits at list position 1
    // while its rank is 2 — the shape a positional record gets wrong
    // (the 你→浩 training divergence, `sentence-surface.md` §8).
    session.sentence.rows = vec![
        NbestRow {
            text: "\u{597d}".into(),
            tokens: vec![PhraseToken::new(0x100)],
            spans: Vec::new(),
            keys: 1,
            span: 3,
            cost: 10,
        },
        NbestRow {
            text: "\u{597d}".into(),
            tokens: vec![PhraseToken::new(0x101)],
            spans: Vec::new(),
            keys: 1,
            span: 3,
            cost: 20,
        },
        NbestRow {
            text: "\u{6d69}".into(),
            tokens: vec![PhraseToken::new(0x102)],
            spans: Vec::new(),
            keys: 1,
            span: 3,
            cost: 30,
        },
    ];
    session.refresh().expect("refresh cannot fail");

    let hao = session
        .candidates()
        .iter()
        .position(|candidate| candidate.nbest_row() == Some(2))
        .expect("the 浩 row survived the dedup");
    assert_eq!(
        hao, 1,
        "the deduped 好 row shifts the 浩 row off its own rank"
    );

    session.select(hao).expect("the row is live");
    assert_eq!(
        session.selected_tokens(),
        [PhraseToken::new(0x102)],
        "the chosen row records its own token path, not the deduped row 1's"
    );
}

#[test]
fn an_nbest_row_chosen_from_a_reanchored_window_commits_only_its_text() {
    use crate::nbest::NbestRow;

    let mut session = train_session();
    session.type_pinyin("nihao").expect("typing cannot fail");

    // A single whole-composition sentence hypothesis: its span is the
    // full input, so selecting it from a re-anchored window must commit
    // the row's text alone — never the typed-but-unselected gap (which
    // would duplicate the raw prefix).
    session.sentence.rows = vec![NbestRow {
        text: "你好".into(),
        tokens: vec![PhraseToken::new(0x100), PhraseToken::new(0x101)],
        spans: Vec::new(),
        keys: 2,
        span: 5,
        cost: 10,
    }];
    session.refresh().expect("refresh cannot fail");

    // Re-anchor the window at offset 2 (mid-composition, before any
    // choose). The n-best row rides the prepend into the window.
    let window = session.candidates_at(2).expect("offset 2 is in range");
    let nbest = window
        .iter()
        .position(|candidate| candidate.nbest_row() == Some(0))
        .expect("the sentence row is prepended at a re-anchored offset");

    session
        .select_anchored(nbest, &window, 2)
        .expect("selection cannot fail");
    // The row's span is the whole composition, so the selection consumes
    // everything; commit() returns the row text with no raw prefix.
    assert_eq!(
        session.commit().expect("commit cannot fail"),
        "你好",
        "an n-best row from a re-anchored window commits its own text,\n\
         not the gap-prefixed duplicate"
    );
}

#[test]
fn a_lookup_activates_the_sentence_gate_even_without_rows() {
    let mut session = session();
    assert!(!session.sentence_lookup_active());
    // The Silent backends answer nothing: the lookup runs, finds no
    // rows, and still counts as active — upstream clears
    // m_nbest_results before every attempt.
    session
        .type_pinyin("qqq")
        .expect("batch typing cannot fail");
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "the lookup ran"
    );
    assert!(session.sentence_lookup_active());
    assert_eq!(session.sentence_text(0), None);
    session.reset();
    assert!(!session.sentence_lookup_active());
}

#[test]
fn normalized_lookup_offset_walks_the_zero_run_and_refuses_a_leading_one() {
    let mut session = session();
    session
        .type_pinyin("ni'hao")
        .expect("batch typing cannot fail");
    assert_eq!(session.normalized_lookup_offset(0), Ok(0));
    assert_eq!(
        session.normalized_lookup_offset(3),
        Ok(2),
        "one past the separator normalizes to the zero key's own byte"
    );
    assert_eq!(session.normalized_lookup_offset(2), Ok(2));

    session.reset();
    session
        .type_pinyin("ni''hao")
        .expect("batch typing cannot fail");
    assert_eq!(
        session.normalized_lookup_offset(4),
        Ok(2),
        "the whole run collapses to its first byte"
    );

    session.reset();
    session
        .type_pinyin("'ni")
        .expect("batch typing cannot fail");
    assert_eq!(
        session.normalized_lookup_offset(1),
        Err(EngineError::LookupOffsetPastSeparator {
            offset: 1,
            normalized: 1
        }),
        "the walk never crosses byte 0, so a leading run refuses \
         (_check_offset aborts upstream)"
    );
    assert_eq!(session.normalized_lookup_offset(0), Ok(0));

    session.reset();
    session
        .type_pinyin("ni'")
        .expect("batch typing cannot fail");
    assert_eq!(
        session.normalized_lookup_offset(3),
        Ok(2),
        "a trailing run normalizes without reading past the buffer"
    );
    assert_eq!(
        session.normalized_lookup_offset(9),
        Err(EngineError::LookupOffsetOutOfRange { offset: 9, len: 3 }),
        "an offset beyond one-past-end refuses before the walk"
    );
}

#[test]
fn dynamic_adjust_folds_the_bigram_term_only_with_the_bit_and_a_gram() {
    use oxpinyin_core::DYNAMIC_ADJUST;
    use oxpinyin_core::MergedGram;
    use oxpinyin_core::OptionBits;

    let clear = OptionBits::default();
    let set = OptionBits::default().with(DYNAMIC_ADJUST, true);
    // 500 of a 1000-count row: the pin's bigram_freq / total.
    let gram = MergedGram::new(1_000, vec![(42, 500), (7, 100)]);

    assert_eq!(
        super::dynamic_adjust_bigram_possibility(clear, Some(&gram), 42),
        0.0,
        "bit clear omits the term however populated the row is"
    );
    assert_eq!(
        super::dynamic_adjust_bigram_possibility(set, None, 42),
        0.0,
        "no previous token means no gram was merged, so no term"
    );
    assert_eq!(
        super::dynamic_adjust_bigram_possibility(set, Some(&gram), 43),
        0.0,
        "a token the row misses contributes nothing"
    );
    assert_eq!(
        super::dynamic_adjust_bigram_possibility(set, Some(&gram), 42),
        0.5,
        "bit set with a merged row is bigram_freq / total"
    );

    // The term must actually move the frequency, not merely exist. This
    // is what a stub cannot satisfy: returning a constant zero
    // possibility leaves `adjusted` equal to `base`.
    let (unigram, total) = (1_234_u64, 51_051_831_u64);
    let base = super::amplified_frequency_with_bigram(unigram, total, 0.0);
    let adjusted = super::amplified_frequency_with_bigram(unigram, total, 0.5);
    assert!(
        adjusted > base,
        "a non-zero possibility must raise the amplified frequency ({adjusted} vs {base})"
    );
    assert_eq!(
        base,
        super::amplified_frequency(unigram, total),
        "the bit-clear path is the pre-existing unigram law exactly"
    );
}

/// The frozen candidate pins were measured with `DYNAMIC_ADJUST` clear on
/// both sides, which is the whole reason implementing it cannot move
/// them. That safety argument is **not** "the term is zero at offset 0"
/// — upstream's `_get_previous_token` answers `sentence_start` (1) there,
/// not `null_token`, so Gate 2 fires and a real gram is merged. The
/// argument is only that the bit is clear in every frozen word.
///
/// So this reads the harness's own option words rather than a copy of
/// them: adding the bit to a frozen profile fails here instead of
/// silently moving pins.
#[test]
fn no_frozen_option_word_sets_dynamic_adjust() {
    use std::path::Path;

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/bisection");
    let mut checked = 0_usize;
    for entry in std::fs::read_dir(&dir).expect("the bisection harness is in-tree") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("c") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("readable source");
        for line in source.lines() {
            let Some(rest) = line.strip_prefix("#define PARITY_OPTIONS") else {
                continue;
            };
            let hex = rest
                .split("0x")
                .nth(1)
                .expect("PARITY_OPTIONS is written in hex")
                .trim_end_matches(|c: char| !c.is_ascii_hexdigit());
            let word = u32::from_str_radix(hex, 16).expect("parsable hex option word");
            assert_eq!(
                word & 0x200,
                0,
                "{}: frozen option word 0x{word:x} sets DYNAMIC_ADJUST (1<<9). \
                 The frozen candidate pins were measured with it clear; setting it \
                 changes candidate ranking at every offset, including 0.",
                path.display()
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "found no PARITY_OPTIONS definitions to check — the guard would be vacuous"
    );
}

/// A system-only facade with a caller-chosen entry count, so a guess's
/// merge count can be measured against its candidate count.
struct SystemPhrases(Vec<PhraseEntry>);

impl Dictionary for SystemPhrases {
    type Entry = PhraseEntry;
    type Error = EngineError;
    type Syllable = SyllableKey;

    fn lookup(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
        Ok(self.0.clone())
    }
}

/// Fixed unigrams plus one merged bigram row, counting how often the
/// engine asks for that row and recording which token it asked about.
///
/// The counter is the point: upstream merges ONE gram per
/// `pinyin_guess_candidates` and indexes it per candidate
/// (`pinyin.cpp:2200-2224`). A count that tracks the candidate list is
/// the complexity regression the source policy forbids, and it is
/// invisible to any output-only assertion.
struct CountingBigrams {
    unigrams: FixedUnigrams,
    row: Option<oxpinyin_core::MergedGram>,
    merges: std::rc::Rc<std::cell::Cell<usize>>,
    asked_about: std::rc::Rc<std::cell::Cell<u32>>,
}

impl LanguageModel for CountingBigrams {
    type Error = EngineError;
    type Token = PhraseToken;

    fn score(
        &self,
        history: &[PhraseToken],
        token: &PhraseToken,
        edge_cost: Cost,
    ) -> Result<Cost, EngineError> {
        self.unigrams.score(history, token, edge_cost)
    }

    fn has_real_unigrams(&self) -> bool {
        true
    }

    fn unigram_freq(&self, token: &PhraseToken) -> Result<Option<u64>, EngineError> {
        self.unigrams.unigram_freq(token)
    }

    fn unigram_total(&self) -> Result<Option<u64>, EngineError> {
        self.unigrams.unigram_total()
    }

    fn addon_unigram_freq(&self, token: &PhraseToken) -> Result<Option<u64>, EngineError> {
        self.unigrams.addon_unigram_freq(token)
    }

    fn addon_unigram_total(&self) -> Result<Option<u64>, EngineError> {
        self.unigrams.addon_unigram_total()
    }

    fn merged_successors(
        &self,
        prev: &PhraseToken,
    ) -> Result<Option<oxpinyin_core::MergedGram>, EngineError> {
        self.merges.set(self.merges.get() + 1);
        self.asked_about.set(prev.value());
        Ok(self.row.clone())
    }
}

/// The six fixture phrases of the dynamic-adjust probes.
const DYNAMIC_ADJUST_TEXTS: [&str; 6] = ["系", "统", "习", "题", "集", "锦"];
/// The first fixture token.
const DYNAMIC_ADJUST_FIRST: u32 = 0x0100_0001;
/// The second fixture token.
const DYNAMIC_ADJUST_SECOND: u32 = 0x0100_0002;

/// One dynamic-adjust probe's observable state: the candidate texts,
/// the merge count, and the token the model was asked about.
struct DynamicAdjustRun {
    texts: Vec<String>,
    merges: usize,
    asked_about: u32,
}

/// Runs one dynamic-adjust probe: a session over `entries` system
/// phrases with the optional merged row, `dynamic` selecting the bit.
fn dynamic_adjust_run(
    entries: usize,
    dynamic: bool,
    row: Option<oxpinyin_core::MergedGram>,
) -> DynamicAdjustRun {
    use oxpinyin_core::{DYNAMIC_ADJUST, OptionBits, PINYIN_INCOMPLETE};
    use std::cell::Cell;
    use std::rc::Rc;

    let merges = Rc::new(Cell::new(0_usize));
    let asked_about = Rc::new(Cell::new(u32::MAX));
    let phrases = (0..entries)
        .map(|index| {
            PhraseEntry::new(
                PhraseToken::new(DYNAMIC_ADJUST_FIRST + u32::try_from(index).expect("small index")),
                DYNAMIC_ADJUST_TEXTS[index].to_owned(),
            )
        })
        .collect();
    let mut session = Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        SystemPhrases(phrases),
        CountingBigrams {
            unigrams: FixedUnigrams {
                system: 13,
                addon: 0,
                total: 51_051_831,
                addon_total: 1,
            },
            row,
            merges: Rc::clone(&merges),
            asked_about: Rc::clone(&asked_about),
        },
    )
    .expect("Session::new");
    // Both arms set the same word apart from the one bit, so nothing
    // else about the parse can differ between them.
    session
        .set_options(
            OptionBits::default()
                .with(PINYIN_INCOMPLETE, true)
                .with(DYNAMIC_ADJUST, dynamic),
        )
        .expect("set_options");
    session.type_pinyin("a").expect("typing cannot fail");
    DynamicAdjustRun {
        texts: session
            .candidates()
            .iter()
            .map(|candidate| candidate.text().to_owned())
            .collect(),
        merges: merges.get(),
        asked_about: asked_about.get(),
    }
}

/// The three gates end to end through `Session`, which the C-level
/// differential cannot demonstrate here: the in-tree `fixtures/w3` mini
/// tables answer `no-first-candidate` for nearly every input, so that
/// differential self-skips without the oracle and proves nothing on its
/// own. This test needs no oracle and no data set.
///
/// Gate 1 — the previous token: at offset 0 upstream answers
/// `sentence_start` (1), not `null_token`. Gate 2 — one merge per
/// guess, never one per candidate. Gate 3 — the possibility joins the
/// unigram term inside the pin's single truncation, and only for the
/// token the row credits.
#[test]
fn dynamic_adjust_merges_one_row_per_guess_and_lifts_only_the_credited_token() {
    use oxpinyin_core::MergedGram;

    // A row that credits the SECOND phrase with half its mass. The
    // unigram answer is a constant across tokens, so with the bit clear
    // the two candidates tie on all three RankKeys and hold collection
    // order; only the bigram term can separate them.
    let credits_second = || MergedGram::new(1_000, vec![(DYNAMIC_ADJUST_SECOND, 500)]);

    // Gate 1, bit clear: the model is never consulted at all, and the
    // order is the pre-existing unigram law's.
    let clear = dynamic_adjust_run(2, false, Some(credits_second()));
    assert_eq!(
        clear.merges, 0,
        "with the bit clear upstream never reaches the merge, so neither may this"
    );
    assert_eq!(
        clear.texts,
        ["系", "统"],
        "the bit-clear order is the frozen unigram-only order"
    );

    // Gate 1, bit set: offset 0 resolves to `sentence_start`, not a null
    // token — the premise that offset 0 is safe by construction is false,
    // and this is the assertion that says so.
    let no_row = dynamic_adjust_run(2, true, None);
    assert_eq!(
        no_row.asked_about,
        crate::nbest::SENTENCE_START,
        "offset 0 asks about sentence_start, exactly as `_get_previous_token` answers"
    );
    assert_eq!(
        no_row.texts,
        ["系", "统"],
        "a model with no row for the previous token contributes no term"
    );

    // Gate 3: the credited token overtakes a candidate it ties with on
    // every other key.
    let lifted = dynamic_adjust_run(2, true, Some(credits_second()));
    assert_eq!(
        lifted.texts,
        ["统", "系"],
        "the bigram term must actually move the credited candidate above its tie peer"
    );

    // Gate 2: the merge count is a property of the guess, not of the
    // candidate list. Tripling the candidates must not change it.
    assert!(
        lifted.merges > 0,
        "the bit is set and a previous token exists, so a merge must happen"
    );
    let wider = dynamic_adjust_run(6, true, Some(credits_second()));
    assert_eq!(
        wider.merges,
        lifted.merges,
        "merging is once per guess ({} candidates merged {} times, {} candidates merged {} \
         times): a count that tracks the candidate list is the O(candidates) regression the \
         source policy forbids",
        lifted.texts.len(),
        lifted.merges,
        wider.texts.len(),
        wider.merges
    );
    assert_eq!(
        wider.texts.first().map(String::as_str),
        Some("统"),
        "the credited token leads a wider list too"
    );
}

#[test]
fn amplified_frequency_pins_the_class_a_probe_values() {
    // The denominator is the pin's phrase-index total over model20:
    // interpolation2 sum 50_913_735 + 138_096 items, each item's baked
    // unigram being its interpolation2 count + 1 (probe-verified over
    // the whole index; `docs/testing/corpus-tail.md` Class A). The
    // values are the amplified keys the 12 top-1 tie-swaps collapse on.
    const PIN_TOTAL: u64 = 51_051_831;
    // 0: the 量比/两笔, 建仓/减仓, 拜倒/白道, 冰坝/并把, 长着/唱着 pairs.
    assert_eq!(super::amplified_frequency(1, PIN_TOTAL), 0);
    assert_eq!(super::amplified_frequency(3, PIN_TOTAL), 0);
    // 3: 写歌 16 vs 写稿 14 (`xiego`).
    assert_eq!(super::amplified_frequency(14, PIN_TOTAL), 3);
    assert_eq!(super::amplified_frequency(16, PIN_TOTAL), 3);
    // 4: 古稀 21 vs 股息 20 (`guxi`), 酸楚 20 vs 算出 18 (`suanch`).
    assert_eq!(super::amplified_frequency(18, PIN_TOTAL), 4);
    assert_eq!(super::amplified_frequency(20, PIN_TOTAL), 4);
    assert_eq!(super::amplified_frequency(21, PIN_TOTAL), 4);
    // 17: 每家 78 vs 美加 77 (`meijia…`).
    assert_eq!(super::amplified_frequency(77, PIN_TOTAL), 17);
    assert_eq!(super::amplified_frequency(78, PIN_TOTAL), 17);
    // 19: 狗狗 = 沟谷 = 87 (`goug`).
    assert_eq!(super::amplified_frequency(87, PIN_TOTAL), 19);
    assert_eq!(super::amplified_frequency(0, PIN_TOTAL), 0);
    assert_eq!(
        super::amplified_frequency(20, 0),
        0,
        "no index total ranks as zero"
    );
}

#[test]
fn amplified_frequency_is_c_float_not_f64() {
    // 2_349_890 is a corpus-scale count (interpolation2 tops out at
    // 3_081_671) where the C float chain and the same chain in f64
    // truncate apart — 530_766 vs 530_765 — so this pins the f32
    // arithmetic the oracle's m_freq runs in.
    assert_eq!(super::amplified_frequency(2_349_890, 51_051_831), 530_766);
}

#[test]
fn window_flush_is_token_ascending_and_one_row_per_token() {
    use super::{CandidateKind, flush_window_batch};
    use crate::candidate::Candidate;

    let mut batch = vec![
        Candidate::new(
            compact_str::CompactString::from("狗狗"),
            CandidateKind::Phrase,
            2,
            4,
            0,
            Some(PhraseToken::new(0x0300_16df)),
            None,
        ),
        Candidate::new(
            compact_str::CompactString::from("沟谷"),
            CandidateKind::Phrase,
            2,
            4,
            0,
            Some(PhraseToken::new(0x0100_4c41)),
            None,
        ),
        Candidate::new(
            compact_str::CompactString::from("沟谷"),
            CandidateKind::Phrase,
            1,
            4,
            0,
            Some(PhraseToken::new(0x0100_4c41)),
            None,
        ),
    ];
    let mut into = Vec::new();
    flush_window_batch(&mut batch, &mut into);
    let tokens: Vec<u32> = into
        .iter()
        .filter_map(|c| c.token().map(oxpinyin_core::PhraseToken::value))
        .collect();
    assert_eq!(tokens, [0x0100_4c41, 0x0300_16df]);
    assert_eq!(
        into[0].consumed_keys(),
        2,
        "the first collected row of a duplicated token is the one kept"
    );
}

/// A model with fixed facade answers, so the frequency table's
/// per-branch inputs are visible without a real table.
struct FixedUnigrams {
    system: u64,
    addon: u64,
    total: u64,
    addon_total: u64,
}
impl LanguageModel for FixedUnigrams {
    type Error = EngineError;
    type Token = PhraseToken;

    fn score(
        &self,
        _history: &[PhraseToken],
        _token: &PhraseToken,
        edge_cost: Cost,
    ) -> Result<Cost, EngineError> {
        Ok(edge_cost)
    }

    fn has_real_unigrams(&self) -> bool {
        true
    }

    fn unigram_freq(&self, _token: &PhraseToken) -> Result<Option<u64>, EngineError> {
        Ok(Some(self.system))
    }

    fn unigram_total(&self) -> Result<Option<u64>, EngineError> {
        Ok(Some(self.total))
    }

    fn addon_unigram_freq(&self, _token: &PhraseToken) -> Result<Option<u64>, EngineError> {
        Ok(Some(self.addon))
    }

    fn addon_unigram_total(&self) -> Result<Option<u64>, EngineError> {
        Ok(Some(self.addon_total))
    }
}

// The key-cost table is read only by the pre-frequency fallback scorer:
// both `Scorer::with_key_costs` sites sit on the `else` of a
// `has_real_unigrams()` gate, so a model carrying real frequencies can
// never consult one and must not pay to build one. That walk is the
// whole 42–57 ms first-alloc penalty in
// `docs/findings/perf-backend-matrix-2026-09.md`, and upstream has no
// analogue in any configuration — `pinyin_alloc_instance` allocates
// instance state and performs no dictionary or model read
// (`src/pinyin.cpp:1310-1333` at the pin `0c5e80e1`). The fallback must
// keep its full table, so this pins both sides of the gate.
#[test]
fn real_unigrams_skip_the_key_cost_walk_but_the_fallback_keeps_it() {
    let real = Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        Silent,
        FixedUnigrams {
            system: 14,
            addon: 14,
            total: 51_051_831,
            addon_total: 25_525_916,
        },
    )
    .expect("Session::new");
    assert!(real.model.has_real_unigrams());
    assert!(
        real.key_costs.is_empty(),
        "a real-frequency model must not walk the key inventory it cannot read"
    );

    let fallback = Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        Silent,
        Silent,
    )
    .expect("Session::new");
    assert!(!fallback.model.has_real_unigrams());
    assert_eq!(
        fallback.key_costs.len(),
        oxpinyin_core::SYLLABLE_KEY_COUNT,
        "the fallback scorer still needs every frozen key priced at construction"
    );
}

// Non-vacuity for the `Session::init` invariant assert, both directions.
// Gated on `debug_assertions`: the assert compiles out in release, so an
// ungated `should_panic` would fail under `cargo test --release` rather than
// report anything about the code.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "key_costs must be empty exactly when")]
fn fallback_model_with_an_empty_key_cost_table_is_rejected() {
    // The harmful direction: without the table the fallback scorer prices
    // every edge at UNKNOWN_COST and decodes wrongly in silence.
    let _ = Session::new_with_key_costs(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        Silent,
        Silent,
        Vec::new(),
    );
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "key_costs must be empty exactly when")]
fn real_unigram_model_with_a_populated_key_cost_table_is_rejected() {
    // The wasteful direction: this is what reverting either gate looks like
    // from the constructor's side.
    let _ = Session::new_with_key_costs(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        Silent,
        FixedUnigrams {
            system: 14,
            addon: 14,
            total: 51_051_831,
            addon_total: 25_525_916,
        },
        vec![oxpinyin_core::cost::UNKNOWN_COST; oxpinyin_core::SYLLABLE_KEY_COUNT],
    );
}

#[test]
fn addon_candidates_rank_on_their_own_amplified_scale() {
    use super::CandidateKind;
    use crate::candidate::Candidate;

    // The pin's two amplified branches (`pinyin.cpp:1829-1843` for the
    // addon, `:1855-1866` for the system): both read the item's stored
    // unigram (a model20 count 13 is stored as 14, `gen_unigram`'s +1)
    // — the system branch over the default facade's total, the addon
    // branch over the addon facade's. The same stored 14 therefore
    // lands on 3 over 51,051,831 but 6 over the half-size addon total.
    let session = Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        Silent,
        FixedUnigrams {
            system: 14,
            addon: 14,
            total: 51_051_831,
            addon_total: 25_525_916,
        },
    )
    .expect("Session::new");
    let collected = vec![
        Candidate::new(
            compact_str::CompactString::from("股"),
            CandidateKind::Phrase,
            1,
            3,
            0,
            Some(PhraseToken::new(1)),
            None,
        ),
        Candidate::new(
            compact_str::CompactString::from("附"),
            CandidateKind::Addon,
            1,
            3,
            0,
            Some(PhraseToken::new(2)),
            None,
        ),
    ];
    let frequencies = session
        .candidate_frequencies(&collected, None)
        .expect("frequency reads cannot fail here");
    assert_eq!(
        frequencies,
        Some(vec![3, 6]),
        "system = amplified(14, 51_051_831) = 3; addon = amplified(14, 25_525_916) = 6"
    );
}

/// One entry per facade, so a scan's two batches are exactly one
/// system and one addon candidate.
struct TwoFacadeDict {
    system: PhraseEntry,
    addon: PhraseEntry,
}

impl Dictionary for TwoFacadeDict {
    type Entry = PhraseEntry;
    type Error = EngineError;
    type Syllable = SyllableKey;

    fn lookup(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
        Ok(vec![self.system.clone()])
    }

    fn lookup_addon(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
        Ok(vec![self.addon.clone()])
    }
}

#[test]
fn window_scan_emits_system_candidates_before_addon_candidates() {
    // Two one-character candidates whose three RankKeys tie (length 1,
    // span 1, amplified 3: system item 14 over 51,051,831; addon 7 over
    // the half-size addon total). The stable sort must therefore keep
    // the scan's flush order — the default facade's batch before the
    // addon facade's, the array order `_append_items`
    // (`pinyin.cpp:1769-1791`) lays down.
    let mut session = Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        TwoFacadeDict {
            system: PhraseEntry::new(PhraseToken::new(0x0100_0001), "系".to_owned()),
            addon: PhraseEntry::new(PhraseToken::new(0x0500_0002), "附".to_owned()),
        },
        FixedUnigrams {
            system: 14,
            addon: 7,
            total: 51_051_831,
            addon_total: 25_525_916,
        },
    )
    .expect("Session::new");
    session.type_pinyin("a").expect("typing cannot fail");
    let texts: Vec<&str> = session
        .candidates()
        .iter()
        .map(super::super::candidate::Candidate::text)
        .collect();
    assert_eq!(
        texts,
        ["系", "附"],
        "a full three-key tie must keep the pin's system-before-addon array order"
    );
}

#[test]
fn scan_matrix_tone_rides_fuzzy_and_locks_the_split_tables() {
    use oxpinyin_core::graph::SegmentGraph;
    use oxpinyin_core::{OptionBits, PINYIN_AMB_Z_ZH, PINYIN_INCOMPLETE, USE_TONE};

    let incomplete = OptionBits::from_bits(PINYIN_INCOMPLETE);
    let toned = OptionBits::from_bits(PINYIN_INCOMPLETE | USE_TONE);

    // Fuzzy alternates inherit the tone: upstream copies the whole
    // ChewingKey before swapping the initial
    // (`phonetic_key_matrix.cpp:250-259`).
    let graph = SegmentGraph::build_with_options(b"zai4", toned).expect("valid");
    let columns = super::build_scan_matrix(
        &graph,
        OptionBits::from_bits(PINYIN_INCOMPLETE | USE_TONE | PINYIN_AMB_Z_ZH),
        true,
    );
    let column: Vec<_> = columns[0]
        .iter()
        .map(|key| (key.key.text(), key.to, key.tone))
        .collect();
    assert!(column.contains(&("zai", 4, 4)));
    assert!(column.contains(&("zhai", 4, 4)));

    // Resplit: ("a", "nan") is a live pair on the toneless walk; a toned
    // member locks it, because the table structs are zero-tone and the
    // pin matches the full ChewingKey.
    let toneless = SegmentGraph::build_with_options(b"anan", incomplete).expect("valid");
    let columns = super::build_scan_matrix(&toneless, incomplete, true);
    assert!(
        columns[0]
            .iter()
            .any(|key| key.key.text() == "an" && key.to == 2)
    );

    let toned_pair = SegmentGraph::build_with_options(b"a4nan", toned).expect("valid");
    let columns = super::build_scan_matrix(&toned_pair, toned, true);
    assert!(!columns[0].iter().any(|key| key.key.text() == "an"));

    // Divided: "bian" divides toneless; "bian4" carries its tone instead.
    let toneless = SegmentGraph::build_with_options(b"bian", incomplete).expect("valid");
    let columns = super::build_scan_matrix(&toneless, incomplete, true);
    assert!(
        columns[0]
            .iter()
            .any(|key| key.key.text() == "bi" && key.to == 2)
    );

    let toned_key = SegmentGraph::build_with_options(b"bian4", toned).expect("valid");
    let columns = super::build_scan_matrix(&toned_key, toned, true);
    assert!(
        columns[0]
            .iter()
            .any(|key| key.key.text() == "bian" && key.tone == 4)
    );
    assert!(!columns[0].iter().any(|key| key.key.text() == "bi"));
}

/// A model whose n-best step costs exist, so the trellis runs: both
/// branches at a fixed cost, `score` passes the edge through.
struct TrellisModel {
    blended: Cost,
    unigram: Cost,
}

impl LanguageModel for TrellisModel {
    type Error = EngineError;
    type Token = PhraseToken;

    fn score(
        &self,
        _history: &[PhraseToken],
        _token: &PhraseToken,
        edge_cost: Cost,
    ) -> Result<Cost, EngineError> {
        Ok(edge_cost)
    }

    fn has_real_unigrams(&self) -> bool {
        true
    }

    fn nbest_step_costs(
        &self,
        _prev: &PhraseToken,
        _token: &PhraseToken,
    ) -> Result<NbestStepCosts, EngineError> {
        Ok(NbestStepCosts {
            blended: Some(self.blended),
            unigram: Some(self.unigram),
        })
    }
}

fn trellis_session() -> Session<FixtureDictionary, TrellisModel> {
    Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        FixtureDictionary::parse(TRAIN_VOCAB).expect("authored fixture"),
        TrellisModel {
            blended: 100,
            unigram: 200,
        },
    )
    .expect("the fixtures open")
}

/// Types `text` and selects the candidate carrying `token`.
fn type_and_select_over<M>(session: &mut Session<FixtureDictionary, M>, text: &str, token: u32)
where
    M: LanguageModel<Token = PhraseToken>,
    M::Error: std::fmt::Display,
{
    for character in text.chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    let index = session
        .candidates()
        .iter()
        .position(|candidate| candidate.token() == Some(PhraseToken::new(token)))
        .expect("the fixture candidate is offered");
    session.select(index).expect("selection cannot fail");
}

/// §3: a chosen candidate forces its span — the constrained walk pins
/// the chosen 你 and decodes the continuation, so the row carries the
/// full sentence.
#[test]
fn a_selection_forces_its_span_in_the_sentence_walk() {
    let mut session = trellis_session();
    type_and_select_over(&mut session, "nihao", 1);
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "the constrained walk runs"
    );
    assert_eq!(
        session.sentence_text(0).expect("row 0 exists"),
        "\u{4f60}\u{597d}",
        "the forced 你 leads the decoded continuation"
    );
}

/// L1: a terminal selection still answers — the walk covers the full
/// matrix, so a fully-consumed composition has rows.
#[test]
fn a_terminal_selection_still_answers_the_full_matrix() {
    let mut session = trellis_session();
    type_and_select_over(&mut session, "ni", 1);
    assert_eq!(session.raw_input(), "ni");
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "the fully-consumed composition walks the full matrix"
    );
    let rows: Vec<&str> = (0..3)
        .filter_map(|index| session.sentence_text(index))
        .collect();
    assert_eq!(
        rows,
        ["\u{4f60}", "\u{4f60}"],
        "the forced phrase is the row — twice: the bigram and unigram branch \
         lineages of the same token, the shape the oracle's terminal-choose \
         rows show (the candidate window dedups them)"
    );
}

/// L2: the forcing survives further typing and is released only by
/// the full reset.
#[test]
fn the_forcing_survives_typing_and_releases_only_on_reset() {
    let mut session = trellis_session();
    type_and_select_over(&mut session, "nihao", 1);
    for character in "s".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "the walk runs over the extended buffer"
    );
    assert!(
        session
            .sentence_text(0)
            .expect("row 0 exists")
            .starts_with('\u{4f60}'),
        "the forcing survived the keystroke"
    );

    session.reset();
    assert!(
        !session.clear_constraint(0),
        "the reset released the forcing — the store is empty"
    );
    for character in "nihaos".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "the post-reset walk runs"
    );
}

/// `pinyin_clear_constraint`'s engine half: a hit inside a run
/// un-forces the whole run, the selection record follows the
/// survivors, and a free or out-of-range offset answers false.
#[test]
fn clear_constraint_unforces_the_run_and_rebuilds_the_record() {
    let mut session = train_session();
    type_and_select(&mut session, "nihao", 1);
    type_and_select(&mut session, "hao", 2);
    assert_eq!(
        session.selected_tokens(),
        [PhraseToken::new(1), PhraseToken::new(2)]
    );

    // Out of range answers false, never panic.
    assert!(!session.clear_constraint(999));

    // A hit anywhere inside the head run — its NoSearch interior
    // included — un-forces the whole run; the record follows the
    // survivor.
    assert!(session.clear_constraint(1));
    assert_eq!(session.selected_tokens(), [PhraseToken::new(2)]);
    assert!(!session.clear_constraint(0), "the head run is already free");

    assert!(session.clear_constraint(2));
    assert!(session.selected_tokens().is_empty());
    assert!(!session.clear_constraint(0));
}

/// Review regression: clearing a tail forcing re-opens a committed
/// composition. The rebuild once left `selection_committed` set, so
/// the next compatible re-parse started fresh and silently dropped
/// the surviving head forcing.
#[test]
fn clearing_a_tail_forcing_reopens_a_committed_composition() {
    let mut session = trellis_session();
    for character in "nihaoshijie".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    let select_token = |session: &mut Session<FixtureDictionary, TrellisModel>, token| {
        let index = session
            .candidates()
            .iter()
            .position(|candidate| candidate.token() == Some(PhraseToken::new(token)))
            .expect("the fixture candidate is offered");
        session.select(index).expect("selection cannot fail");
    };
    select_token(&mut session, 1); // 你 over [0,2)
    select_token(&mut session, 2); // 好 over [2,5)
    // The remainder has no window candidates: select the raw-text
    // fallback to consume the buffer — the commit-branch shape.
    let index = session
        .candidates()
        .iter()
        .position(|candidate| candidate.kind() == crate::CandidateKind::Fallback)
        .expect("the fallback covers the remainder");
    session.select(index).expect("selection cannot fail");
    assert!(
        session.selection_committed(),
        "the last selection consumed the buffer"
    );

    // Un-force the tail run; the record rebuilds over the survivor
    // and the composition is open again.
    assert!(session.clear_constraint(2));
    assert!(
        !session.selection_committed(),
        "the rebuild re-opened the composition"
    );
    assert_eq!(session.selected_tokens(), [PhraseToken::new(1)]);

    // Further typing keeps the surviving 你 forcing alive.
    for character in "s".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "the walk runs"
    );
    assert!(
        session
            .sentence_text(0)
            .expect("row 0 exists")
            .starts_with('\u{4f60}'),
        "the surviving head forcing outlived the clearing and the typing"
    );
}

/// Review regression: the clamp reconcile must not depend on a
/// forcing being dropped. A row-0 n-best choose writes the RECORD
/// (the whole row's text, tokens, and span-end cursor) while
/// `diff_result` writes no run — the store stays empty. A shrinking
/// re-parse then clamps the cursor backward with nothing to drop,
/// and the rebuild must run anyway: the record's coverage exceeds
/// the new input, and Enter would commit the stale row text for an
/// input that can no longer produce it.
#[test]
fn a_shrinking_reparse_reconciles_a_constraint_free_row_selection() {
    let mut session = trellis_session();
    for character in "nihao".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    assert!(session.guess_sentence().expect("guess cannot fail"));
    let row0 = session
        .candidates()
        .iter()
        .position(|candidate| {
            candidate.kind() == crate::CandidateKind::Sentence && candidate.nbest_row() == Some(0)
        })
        .expect("the row-0 sentence is offered");
    assert_eq!(
        session.select(row0).expect("selection cannot fail"),
        Selection::Completed
    );
    assert!(session.selection_committed());
    assert!(
        !session.constraints.is_active(),
        "the row-0 choose wrote no forcing"
    );

    // The shrinking re-parse: the clamp moves the cursor backward
    // with an empty store — the rebuild runs regardless.
    session.replace_raw("ni").expect("replace cannot fail");
    assert!(
        session.selected_tokens().is_empty(),
        "the constraint-free row record reconciled away"
    );
    assert_eq!(
        session.composition_offset(),
        0,
        "the composition re-opened at 0"
    );

    let outcome = session
        .process_key(&KeyInput::plain(LogicalKey::Enter))
        .expect("enter on a composing session cannot fail");
    assert_eq!(
        outcome,
        KeyOutcome::Commit("ni".to_owned()),
        "the commit answers the current input, never the stale row text"
    );
}

/// Review regression: the reconcile must fire on canonical
/// divergence, not only on a backward clamp. A committed selection
/// covers `raw[..consumed]`; a replacement that does not extend
/// those bytes — here the same-length "mihao" over "nihao" — leaves
/// the cursor unclamped but the selection stale, and the forcing no
/// longer spells over the new bytes. The spell-probe validate drops
/// it at the replacement and the record follows; Enter commits the
/// current input, never the stale \u{4f60}\u{597d}. (Through the
/// transformed seams this is the shape a scheme-coordinate
/// continuation passing while the canonical spelling diverges — a
/// live scheme switch — produces.)
#[test]
fn a_divergent_replacement_reconciles_the_committed_selection() {
    let mut session = trellis_session();
    for character in "nihao".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    for token in [1_u32, 2] {
        let index = session
            .candidates()
            .iter()
            .position(|candidate| candidate.token() == Some(PhraseToken::new(token)))
            .expect("the fixture candidate is offered");
        session.select(index).expect("selection cannot fail");
    }
    assert!(session.selection_committed());

    // Same length, divergent inside the covered span: no clamp, so
    // only the continuity check can reach the reconcile. The 你 run
    // no longer spells over "mi" (the spell probe drops it) and the
    // 好 run overruns the spellable bound (the bounds drop).
    session.replace_raw("mixxo").expect("replace cannot fail");
    assert!(
        session.selected_tokens().is_empty(),
        "the diverged selection reconciled away"
    );
    assert_eq!(
        session.composition_offset(),
        0,
        "the composition re-opened at 0"
    );

    let outcome = session
        .process_key(&KeyInput::plain(LogicalKey::Enter))
        .expect("enter on a composing session cannot fail");
    assert_eq!(
        outcome,
        KeyOutcome::Commit("mixxo".to_owned()),
        "the commit answers the current input, never the stale selection"
    );
}

/// The engine-internal backspace keeps the forcing: erase shrinks
/// the raw buffer one keystroke at a time (the engine's own
/// backspace path — the capi's shrink is the same rule through
/// `begin_parse`), the store survives, and the next guess
/// re-validates: down to the forcing's own floor the row is the
/// forced phrase alone, and re-typing continues the composition
/// with the forcing intact.
#[test]
fn the_forcing_survives_the_engine_backspace_and_retype() {
    let mut session = trellis_session();
    for character in "nihaoshijie".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    let index = session
        .candidates()
        .iter()
        .position(|candidate| candidate.token() == Some(PhraseToken::new(1)))
        .expect("the fixture candidate is offered");
    session.select(index).expect("selection cannot fail");

    // Backspace the buffer down to the forcing's floor.
    for _ in 0..9 {
        assert_eq!(
            session.process_key(&KeyInput::plain(LogicalKey::Backspace)),
            Ok(KeyOutcome::Consumed),
            "erase pops while input remains"
        );
    }
    assert_eq!(session.raw_input(), "ni");
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "the floor still walks"
    );
    assert_eq!(
        session.sentence_text(0).expect("row 0 exists"),
        "\u{4f60}",
        "the forced phrase is the floor's row"
    );

    // The re-type continues the open composition with the forcing.
    for character in "haoshijie".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "the retype walks"
    );
    assert!(
        session
            .sentence_text(0)
            .expect("row 0 exists")
            .starts_with('\u{4f60}'),
        "the forcing survived the backspace and the retype"
    );

    // The all-or-nothing erase (nothing left to pop) un-selects
    // everything, store included.
    for _ in 0..11 {
        let _ = session.process_key(&KeyInput::plain(LogicalKey::Backspace));
    }
    assert!(
        !session.clear_constraint(0),
        "the un-select cleared the store"
    );
}

/// The train fallback's boundary: a row-0 choose constrains nothing
/// (upstream-faithful), so the record — not the result — carries the
/// training; a result that sits on a forcing takes the constrained
/// walk instead (the test above). This pins the trigger, not just the
/// outcome: taking the constrained path here would observe nothing.
/// Review regression: a rank-greater-than-zero row choose on a FRESH
/// composition records its forcing. The row branch once called
/// `diff_result` on a store that was never sized — `add` refused every
/// span past an empty cell count, and the forcing silently never
/// landed.
#[test]
fn a_fresh_composition_row_choose_records_its_forcing() {
    let mut session = train_session();
    session.type_pinyin("nihao").expect("typing cannot fail");
    // Hand-crafted rows whose rank-1 phrase differs from row 0's —
    // the shifted-row shape (`sentence-surface.md` §8).
    session.sentence.rows = vec![
        crate::nbest::NbestRow {
            text: "\u{597d}".into(),
            tokens: vec![PhraseToken::new(0x100)],
            spans: vec![crate::constraint::PhraseSpan {
                start: 0,
                token: PhraseToken::new(0x100),
                text: "\u{597d}".into(),
            }],
            keys: 1,
            span: 3,
            cost: 10,
        },
        crate::nbest::NbestRow {
            text: "\u{6d69}".into(),
            tokens: vec![PhraseToken::new(0x102)],
            spans: vec![crate::constraint::PhraseSpan {
                start: 0,
                token: PhraseToken::new(0x102),
                text: "\u{6d69}".into(),
            }],
            keys: 1,
            span: 3,
            cost: 30,
        },
    ];
    session.refresh().expect("refresh cannot fail");
    let index = session
        .candidates()
        .iter()
        .position(|candidate| candidate.nbest_row() == Some(1))
        .expect("the rank-1 row is offered");
    session.select(index).expect("the row is choosable");
    assert!(
        session.clear_constraint(0),
        "the differing phrase's forcing landed on the fresh composition"
    );
}

/// Review regression: the record rebuild keeps the raw text of the
/// gaps between forcings — `diff_result` leaves unchanged phrases
/// free, and the preedit must not drop exactly those bytes.
#[test]
fn a_record_rebuild_keeps_the_gap_text_between_forcings() {
    let mut session = train_session();
    session
        .type_pinyin("nihaoshijie")
        .expect("typing cannot fail");
    // A gapped store straight from diff_result's shape: 你 over
    // [0,2), a free gap "ha" over [2,5), a forcing over [5,10).
    session.constraints.resize(12);
    session
        .constraints
        .add(0, 2, PhraseToken::new(1), "\u{4f60}".into());
    session
        .constraints
        .add(5, 10, PhraseToken::new(2), "\u{4e16}\u{754c}".into());
    session.rebuild_selection_from_constraints();
    let preedit = session.preedit();
    assert!(
        preedit.text().starts_with("\u{4f60}hao\u{4e16}\u{754c}"),
        "the gap's raw bytes survived the rebuild: got {:?}",
        preedit.text()
    );
}

#[test]
fn a_row_zero_choose_trains_through_the_record() {
    let mut session = trellis_session();
    for character in "nihao".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "rows exist before the choose"
    );
    let row = session
        .candidates()
        .iter()
        .position(|candidate| candidate.nbest_row() == Some(0))
        .expect("the rank-0 row is offered at the head");
    session.select(row).expect("the row is choosable");
    assert!(
        !session.clear_constraint(0),
        "a row-0 choose recorded no forcing"
    );
    let row_tokens: Vec<PhraseToken> = session.selected_tokens().to_vec();
    assert_eq!(
        row_tokens,
        vec![PhraseToken::new(1), PhraseToken::new(2)],
        "the row's whole path is the record"
    );

    let mut recorder = Recorder {
        observed: Vec::new(),
    };
    session.train(&mut recorder).expect("train cannot fail");
    assert_eq!(
        recorder.observed,
        vec![
            (Vec::new(), PhraseToken::new(1)),
            (vec![PhraseToken::new(1)], PhraseToken::new(2)),
        ],
        "the record walked — the forcing-less result trained nothing by itself"
    );
}

/// L3: the constraint-aware train walk — the forced phrase and the
/// first decoded phrase after it train, with the predecessor threading
/// over every phrase.
#[test]
fn a_decoded_continuation_trains_through_the_constraint_walk() {
    let mut session = trellis_session();
    type_and_select_over(&mut session, "nihao", 1);
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "the constrained decode ran"
    );
    let mut recorder = Recorder {
        observed: Vec::new(),
    };
    session.train(&mut recorder).expect("train cannot fail");
    assert_eq!(
        recorder.observed,
        vec![
            (Vec::new(), PhraseToken::new(1)),
            (vec![PhraseToken::new(1)], PhraseToken::new(2)),
        ],
        "你 (forced) then 好 (first decoded after the run) train, 你→好 included"
    );
}

#[test]
fn replace_raw_walks_consumed_back_to_a_char_boundary() {
    // A one-byte composition selected to consumed 1, then replaced by
    // `，` (three bytes): the stale consumed sits inside the character
    // and `refresh` slices `raw[consumed..]`. The clamp must walk back
    // to the boundary before it — nothing panics on any input.
    let mut session = session();
    session.replace_raw("a").expect("cannot fail");
    session.select(0).expect("the fallback row selects");
    session.replace_raw("\u{ff0c}").expect("cannot fail");
    assert_eq!(session.composition_offset(), 0);
}

#[test]
fn candidates_at_rejects_mid_character_offsets() {
    // The full-width comma occupies bytes 0..3, so offsets 1 and 2 sit
    // inside it: no window exists under a mid-character slice, and the
    // offset is refused with the inside-character error — not rounded
    // to a neighbour and not the out-of-range error, whose contract is
    // past-one-past-end only.
    let mut session = session();
    session.replace_raw("\u{ff0c}nihao").expect("cannot fail");
    for offset in [1, 2] {
        assert!(
            matches!(
                session.candidates_at(offset),
                Err(EngineError::LookupOffsetInsideCharacter { .. })
            ),
            "offset {offset} is inside the character and must be refused"
        );
    }
    assert!(session.candidates_at(3).is_ok(), "offset 3 is a boundary");
}

#[test]
fn candidates_at_mid_syllable_offsets_answer_the_empty_column() {
    use super::CandidateKind;

    // "nihaoshijie" parses ni|hao|shi|jie: the matrix keys start at
    // 0/2/5/8, so bytes 1/3/4/6/7/9 are the pin's empty columns —
    // `search_matrix` matches nothing from them (`pinyin.cpp:2224-2262`),
    // and the window is the raw-suffix fallback alone, never the suffix
    // re-parse (offset 3 must not answer the `ao…` window, 6 not the
    // `h…` window). The syllable starts keep their windows.
    let mut session = train_session();
    session
        .type_pinyin("nihaoshijie")
        .expect("typing cannot fail");
    for offset in [1usize, 3, 4, 6, 7, 9] {
        let window = session
            .candidates_at(offset)
            .expect("a mid-syllable offset is in range");
        assert!(
            window
                .iter()
                .all(|cand| cand.kind() == CandidateKind::Fallback),
            "offset {offset}: only the fallback row, no suffix re-parse"
        );
        assert_eq!(
            window.iter().count(),
            1,
            "offset {offset}: the fallback alone"
        );
    }
    assert!(
        session
            .candidates_at(2)
            .expect("offset 2 is a syllable start")
            .iter()
            .any(|cand| cand.kind() != CandidateKind::Fallback),
        "offset 2 keeps the hao window"
    );
}

#[test]
fn candidates_at_mid_syllable_keeps_the_prepended_nbest_rows() {
    use super::CandidateKind;
    use crate::nbest::NbestRow;

    // The pin prepends `m_nbest_results` whether or not the span search
    // finds anything, so a post-sentence mid-syllable window is the
    // n-best rows (measured: `nihaoshijie@3` after `guess_sentence`
    // answers n=3 on the pin) over the fallback — not an empty list.
    let mut session = train_session();
    session
        .type_pinyin("nihaoshijie")
        .expect("typing cannot fail");
    session.sentence.rows = vec![NbestRow {
        text: "你好世界".into(),
        tokens: vec![
            PhraseToken::new(0x100),
            PhraseToken::new(0x101),
            PhraseToken::new(0x102),
        ],
        spans: Vec::new(),
        keys: 3,
        span: 11,
        cost: 10,
    }];
    session.refresh().expect("refresh cannot fail");

    let window = session.candidates_at(3).expect("offset 3 is in range");
    assert!(
        window
            .iter()
            .any(|cand| cand.kind() == CandidateKind::Sentence && cand.text() == "你好世界"),
        "the n-best row rides the prepend at the empty column"
    );
    assert!(
        window
            .iter()
            .filter(|cand| cand.kind() != CandidateKind::Fallback)
            .count()
            == 1,
        "no phrase rows join the n-best row at the empty column"
    );
}

#[test]
fn the_zhuyin_display_law_prepends_only_the_one_best_row() {
    use super::CandidateKind;
    use crate::nbest::NbestRow;

    let row = |text: &str, cost: i64| NbestRow {
        text: text.into(),
        tokens: vec![PhraseToken::new(0x100)],
        spans: Vec::new(),
        keys: 2,
        span: 5,
        cost,
    };

    // libzhuyin fills every BEST_MATCH row from `zhuyin_get_sentence`
    // (always `get_result(0)`) and its string dedup removes the
    // duplicates, so exactly one sentence row is observable regardless of
    // the decoded n-best count (`zhuyin.cpp:1272-1291`, `1327-1330`,
    // `1425-1438` at the pin); the pinyin surface keeps one row per
    // sentence (`pinyin.cpp:2004-2007`).
    let mut session = train_session();
    session
        .type_pinyin("nihaoshijie")
        .expect("typing cannot fail");
    session.sentence.rows = vec![row("你好世界", 10), row("你", 12)];

    // Default — the pinyin law: both rows ride the prepend, and the
    // phrase 你 collides with the second row's own text and is dropped.
    session.refresh().expect("refresh cannot fail");
    let sentences: Vec<&str> = session
        .candidates()
        .iter()
        .filter(|cand| cand.kind() == CandidateKind::Sentence)
        .map(super::super::candidate::Candidate::text)
        .collect();
    assert_eq!(sentences, ["你好世界", "你"]);
    assert!(
        !session
            .candidates()
            .iter()
            .any(|cand| cand.kind() == CandidateKind::Phrase && cand.text() == "你"),
        "the phrase colliding with the second row's own text is absorbed"
    );

    // The zhuyin law: only the 1-best row is prepended, so the phrase
    // survives — upstream's dedup never sees a second string to collide
    // it with.
    session.set_collapse_sentence_rows_to_best(true);
    session.refresh().expect("refresh cannot fail");
    let sentences: Vec<&str> = session
        .candidates()
        .iter()
        .filter(|cand| cand.kind() == CandidateKind::Sentence)
        .map(super::super::candidate::Candidate::text)
        .collect();
    assert_eq!(sentences, ["你好世界"]);
    assert!(
        session
            .candidates()
            .iter()
            .any(|cand| cand.kind() == CandidateKind::Phrase && cand.text() == "你"),
        "the phrase survives the collapsed prepend"
    );
}

/// A before-cursor row carries its own span start (upstream's
/// `m_begin`, `zhuyin.cpp:1595`), and choosing it constrains exactly
/// `[m_begin, m_end)`: the leading key is neither absorbed into the
/// chosen text nor lost, and the composition advances to the span's end.
#[test]
fn choosing_a_before_cursor_row_constrains_its_own_span() {
    use super::CandidateKind;
    use oxpinyin_facade_anchor::BEFORE_CURSOR_ANCHOR;
    mod oxpinyin_facade_anchor {
        pub const BEFORE_CURSOR_ANCHOR: usize = 0;
    }

    let mut session = train_session();
    session.type_pinyin("nihao").expect("typing cannot fail");
    let window = session
        .candidates_ending_at(5)
        .expect("offset 5 is in range");
    let (index, row) = window
        .iter()
        .enumerate()
        .find(|(_, cand)| cand.kind() == CandidateKind::Phrase && cand.text() == "好")
        .expect("好 ends at 5 and is offered");
    assert_eq!(row.span_start(), 2, "好 starts where `hao` starts");
    assert_eq!(row.consumed_bytes(), 5, "…and ends at the lookup offset");
    let after_cursor = session.candidates_at(2).expect("offset 2 is in range");
    assert!(
        after_cursor.iter().all(|cand| cand.span_start() == 0),
        "an after-cursor window measures every row from its anchor"
    );

    session
        .select_anchored(index, &window, BEFORE_CURSOR_ANCHOR)
        .expect("the row is selectable");
    assert_eq!(session.composition_offset(), 5);
    let runs = session.constraints.runs();
    assert_eq!(runs.len(), 1, "one forcing: {runs:?}");
    assert_eq!(
        (runs[0].0, runs[0].1, runs[0].3.as_str()),
        (2, 5, "好"),
        "the constraint is the row's own span, not [0, offset)"
    );
    assert_eq!(
        session.preedit().text(),
        "ni好",
        "the leading key stays typed-but-unselected, as an after-cursor re-anchor keeps it"
    );
}

#[test]
fn candidates_ending_at_walks_the_spans_that_end_there() {
    use super::CandidateKind;
    use crate::nbest::NbestRow;

    // The pin's before-cursor walk answers spans (start, offset) for
    // every live start, longest first, with the sentence rows prepended
    // (measured on the instrumented oracle: su3u3 before(3) is the first
    // syllable's 125 rows plus one BEST_MATCH row).
    let mut session = train_session();
    session.type_pinyin("nihao").expect("typing cannot fail");

    let window = session
        .candidates_ending_at(2)
        .expect("offset 2 is in range");
    assert!(
        window.iter().any(|cand| cand.text() == "你"),
        "你 ends at 2 and is offered"
    );
    assert!(
        !window.iter().any(|cand| cand.text() == "好"),
        "好 ends at 5, not 2 — the walk keeps only spans ending at the offset"
    );

    // Nothing precedes the first key: the window is empty without a
    // sentence guess (the prepend has no rows to ride).
    let at_start = session
        .candidates_ending_at(0)
        .expect("offset 0 is in range");
    assert!(at_start.is_empty());

    // A guessed sentence rides the prepend at any offset.
    session.sentence.rows = vec![NbestRow {
        text: "你好".into(),
        tokens: vec![PhraseToken::new(0x100)],
        spans: Vec::new(),
        keys: 2,
        span: 5,
        cost: 10,
    }];
    session.set_collapse_sentence_rows_to_best(true);
    let window = session
        .candidates_ending_at(2)
        .expect("offset 2 is in range");
    assert_eq!(
        window.get(0).map(|cand| (cand.kind(), cand.text())),
        Some((CandidateKind::Sentence, "你好")),
        "the collapsed sentence row heads the before-cursor window"
    );
    assert!(window.iter().any(|cand| cand.text() == "你"));
}

#[test]
fn candidates_at_the_apostrophe_column_is_transparent() {
    // `ni'hao` parses ni|hao with the zero-key column at 2; the pin's
    // span search steps over it (measured: `ni'hao@2` answers the hao
    // window, n=93), so the apostrophe byte answers the next key's
    // window instead of collapsing to the empty-column law.
    let mut session = train_session();
    session.type_pinyin("ni'hao").expect("typing cannot fail");
    let at_separator = session
        .candidates_at(2)
        .expect("the separator byte is in range");
    let at_start = session.candidates_at(3).expect("the hao start is in range");
    assert_eq!(
        at_separator
            .iter()
            .map(super::super::candidate::Candidate::text)
            .collect::<Vec<_>>(),
        at_start
            .iter()
            .map(super::super::candidate::Candidate::text)
            .collect::<Vec<_>>(),
        "the zero-key column answers the following key's window"
    );
    assert!(
        !at_separator.is_empty(),
        "the stepped-over window is the hao window, not the empty column"
    );
}

#[test]
fn candidates_at_an_incomplete_tail_column_stays_empty() {
    use super::CandidateKind;

    // "nihaozh" under INCOMPLETE keeps `zh` as one matrix key at
    // 5..7 (measured: `nihaozh@6` answers n-best rows only on the
    // pin), so byte 6 is an empty column even though the lone suffix
    // `h` could start a parse of its own.
    let mut session = train_session();
    session.type_pinyin("nihaozh").expect("typing cannot fail");
    let window = session.candidates_at(6).expect("byte 6 is in range");
    assert!(
        window
            .iter()
            .all(|cand| cand.kind() == CandidateKind::Fallback),
        "byte 6 inside the `zh` key is an empty column, not an h window"
    );
}

#[test]
fn candidates_at_an_apostrophe_beyond_the_parse_span_stays_empty() {
    use super::CandidateKind;

    // "ni,'hao" parses only `ni` — the comma stops the parse — so the
    // apostrophe at byte 3 sits outside the matrix: the pin aborts
    // there (measured SIGABRT, the same out-of-matrix landmine as one
    // past a lone zero-key run), and the empty-column window is the
    // no-abort answer, not a re-parse of the `'hao` suffix. The
    // in-span apostrophe of `ni'hao` stays transparent.
    let mut session = train_session();
    session.replace_raw("ni,'hao").expect("cannot fail");
    let window = session.candidates_at(3).expect("byte 3 is in range");
    assert!(
        window
            .iter()
            .all(|cand| cand.kind() == CandidateKind::Fallback),
        "byte 3 is outside the parse span — no suffix re-parse window"
    );

    let mut session = train_session();
    session.replace_raw("ni'hao").expect("cannot fail");
    let at_separator = session
        .candidates_at(2)
        .expect("the in-span separator byte is transparent");
    assert!(
        at_separator
            .iter()
            .any(|cand| cand.kind() != CandidateKind::Fallback),
        "the in-span apostrophe keeps the hao window"
    );
}

#[test]
fn candidates_at_a_divided_split_column_answers_the_split_window() {
    use super::CandidateKind;

    // The pin's divided table splits `jie` into `ji` + `e`
    // (`special_table.h:16`, `inner_split_step` under
    // `USE_DIVIDED_TABLE`), so byte 10 of `nihaoshijie` — the `e`
    // half's start — is a live matrix column, and the pin answers the
    // e-family window there (measured: fresh n=190, 阿 first). The
    // mid-chunk bytes 3/4/6 stay empty: `hao`/`shi` are not divided
    // entries. `fangan` resplits `fan`+`gan` into `fang`+`an`
    // (`resplit_step`), making byte 4 live the same way.
    //
    // The plain fixture carries no `e`-key token, which would make the
    // live column indistinguishable from an empty one through
    // `candidates_at` (both answer the fallback alone), so this test's
    // session adds one.
    const SPLIT_VOCAB: &str = "token=1\tkeys=ni\ttext=你\tunigram=1000\n\
                               token=2\tkeys=hao\ttext=好\tunigram=900\n\
                               token=3\tkeys=e\ttext=恶\tunigram=800\n";
    let split_session = || {
        Session::new(
            &EmptyConfigSource,
            StoragePaths::new("user"),
            FixtureDictionary::parse(SPLIT_VOCAB).expect("authored fixture"),
            FixtureLanguageModel::parse(SPLIT_VOCAB, "").expect("authored fixture"),
        )
        .expect("the fixtures open")
    };

    let mut session = split_session();
    session
        .type_pinyin("nihaoshijie")
        .expect("typing cannot fail");
    assert!(
        session.spans_a_matrix_key(10).expect("byte 10 is in range"),
        "byte 10 is the ji|e divided-split column"
    );
    for offset in [3usize, 4, 6] {
        assert!(
            !session.spans_a_matrix_key(offset).expect("in range"),
            "byte {offset} is an empty column"
        );
    }

    // Through the public surface: byte 10 answers the e-family window
    // (the fixture's 恶 row), the mid-chunk offsets stay fallback-only.
    let e_window = session.candidates_at(10).expect("byte 10 is in range");
    assert!(
        e_window
            .iter()
            .any(|cand| cand.kind() != CandidateKind::Fallback && cand.text() == "恶"),
        "byte 10 answers the divided e window"
    );
    for offset in [3usize, 4, 6] {
        let window = session.candidates_at(offset).expect("in range");
        assert!(
            window
                .iter()
                .all(|cand| cand.kind() == CandidateKind::Fallback),
            "offset {offset} is an empty column — fallback only"
        );
    }

    let mut session = split_session();
    session.type_pinyin("fangan").expect("typing cannot fail");
    assert!(
        session.spans_a_matrix_key(4).expect("byte 4 is in range"),
        "byte 4 is the fang|an resplit column"
    );
    assert!(
        !session.spans_a_matrix_key(5).expect("in range"),
        "byte 5 is an empty column"
    );
    let window = session.candidates_at(5).expect("byte 5 is in range");
    assert!(
        window
            .iter()
            .all(|cand| cand.kind() == CandidateKind::Fallback),
        "byte 5 is an empty column — fallback only"
    );
}

/// Off-by-default diagnostic for issue #403 — no assertion, prints only.
///
/// Simulates the RSS-report's keystroke cycle (CYCLE_INPUTS, one character
/// added at a time) and, for each keystroke's scan matrix, enumerates every
/// complete key-path from node 0 to every reachable end position without
/// consulting a dictionary. Emits per-input rows and an aggregate JSON blob
/// under a `[403]` prefix. Enable with `--features diagnostic_403` and pass
/// `--nocapture` to see the output; nothing is asserted, so it always passes.
///
/// Two caveats to read the numbers with:
/// (a) The real scan short-circuits window widening on `phrase_prefix_exists`
///     returning false. This diagnostic never probes: it widens up to
///     `graph.consumed()`. Absolute path counts are therefore upper bounds
///     against the RSS report's 4,262 (oxpinyin) / 1,536 (upstream) figures;
///     the *ratio* of paths-per-window and the unique/repeat histogram are
///     what carry.
/// (b) The unique / repeat set is keyed by the sequence of `(text, tone)`
///     pairs, which is the same shape upstream's `ChewingTable::search` keys
///     on (`chewing_key.h`), so a memoization-headroom judgement is
///     appropriate.
#[cfg(feature = "diagnostic_403")]
#[test]
fn diagnostic_403_scan_matrix_fanout() {
    use std::collections::HashMap;

    use oxpinyin_core::graph::SegmentGraph;
    use oxpinyin_core::scoring::expand_keys;
    use oxpinyin_core::{Completeness, OptionBits, SyllableKey};

    // The 20-line RSS-report corpus, order preserved (matches
    // `crates/pinyin-oracle/benches/support/mod.rs::CYCLE_INPUTS`).
    const CYCLE_INPUTS: &[&str] = &[
        "ni",
        "wo",
        "de",
        "nihao",
        "zhongguo",
        "xian",
        "fangan",
        "xi'an",
        "bu'tian",
        "fan'gan",
        "n",
        "zh",
        "chongke",
        "caisho",
        "paolen",
        "waimenggu",
        "lenglan",
        "naoxion",
        "liangniejue",
        "chuaipengdengzaimiu",
    ];

    // Parity word (`docs/findings/option-bits.md`): PINYIN_INCOMPLETE (0x8)
    // | USE_DIVIDED_TABLE (0x80) | USE_RESPLIT_TABLE (0x100) plus the
    // harness's 0x2 bit — 0x18a. Fuzzy is off; USE_TONE is off.
    const PARITY: u32 = 0x18a;

    /// One complete key-path through a scan matrix, keyed on (text, tone).
    type PathKey = Vec<(String, u8)>;

    /// Enumerate every path from `node` to `end` under
    /// [`super::lookup::visit_scan_key`] semantics, driving `out` and
    /// stopping a branch whenever a scan key would overhang the window.
    fn walk(
        matrix: &[Vec<super::ScanKey>],
        node: usize,
        end: usize,
        stack: &mut PathKey,
        out: &mut Vec<PathKey>,
    ) {
        let Some(column) = matrix.get(node) else {
            return;
        };
        for scan_key in column.iter().copied() {
            let to = scan_key.to;
            if to > end {
                // Overhanging keys set `continued = true` and stop the branch.
                continue;
            }
            stack.push((scan_key.key.text().to_string(), scan_key.tone));
            if to == end {
                out.push(stack.clone());
            } else if stack.len() < super::MAX_PHRASE_LENGTH {
                walk(matrix, to, end, stack, out);
            }
            stack.pop();
        }
    }

    let options = OptionBits::from_bits(PARITY);

    let mut agg_windows: u64 = 0;
    let mut agg_paths: u64 = 0;
    let mut agg_probes: u64 = 0;
    let mut agg_matrix_entries: u64 = 0;
    let mut sequence_counts: HashMap<PathKey, u32> = HashMap::new();
    // Post-`expand_keys` probe keys: this is what actually hits
    // `Dictionary::lookup_into` per path. If any key on the path is
    // `Partial`, the path expands into every completion of that initial
    // (`SCAN_EXPANSION_LIMIT` bounds the Cartesian product; a limit trip
    // returns an empty list, i.e. zero probes).
    let mut probe_key_counts: HashMap<Vec<String>, u32> = HashMap::new();

    println!(
        "[403] cycle inputs: {} lines, parity 0x{:x}",
        CYCLE_INPUTS.len(),
        PARITY
    );
    println!("[403] input,prefix_len,consumed,total_matrix_entries,windows,paths,unique_paths");
    let mut total_keystrokes: u64 = 0;

    for &input in CYCLE_INPUTS {
        for prefix_len in 1..=input.len() {
            // Respect UTF-8 boundaries: for ASCII inputs (all of CYCLE_INPUTS
            // are ASCII-plus-apostrophe), byte length equals char length.
            let prefix = &input[..prefix_len];
            total_keystrokes += 1;

            let Ok(graph) = SegmentGraph::build_with_options(prefix.as_bytes(), options) else {
                continue;
            };
            let matrix = super::build_scan_matrix(&graph, options, true);
            let consumed = graph.consumed();

            let matrix_entries: usize = matrix.iter().map(std::vec::Vec::len).sum();
            agg_matrix_entries += matrix_entries as u64;

            let mut windows_here: u64 = 0;
            let mut paths_here: u64 = 0;
            let mut probes_here: u64 = 0;
            let mut unique_here: std::collections::HashSet<PathKey> =
                std::collections::HashSet::new();
            for end in 1..=consumed {
                // Skip windows the real widening loop skips: a window whose
                // added byte is an apostrophe repeats the previous key
                // sequence (see `collect_window_scan`'s inner `while`).
                if prefix.as_bytes().get(end - 1) == Some(&b'\'') {
                    continue;
                }
                // Do NOT skip on the end-column being empty: the real scan
                // still calls `scan_paths(0, end)`, and only uses matrix[end]
                // to decide whether to continue widening.
                windows_here += 1;
                let mut paths_at_end: Vec<PathKey> = Vec::new();
                let mut stack: PathKey = Vec::new();
                walk(&matrix, 0, end, &mut stack, &mut paths_at_end);
                paths_here += paths_at_end.len() as u64;
                for path in paths_at_end {
                    *sequence_counts.entry(path.clone()).or_insert(0) += 1;
                    unique_here.insert(path.clone());
                    // Post-`expand_keys` probe count for this path (models
                    // `search_scan_path` → `lookup_and_append` calls).
                    let syllables: Vec<SyllableKey> = path
                        .iter()
                        .filter_map(|(text, _)| SyllableKey::from_text(text))
                        .collect();
                    if syllables.len() != path.len() {
                        // Toneful spellings won't round-trip via
                        // `from_text`; parity has USE_TONE off so this
                        // should never fire.
                        probes_here += 1;
                        continue;
                    }
                    let has_partial = syllables
                        .iter()
                        .any(|key| key.completeness() == Completeness::Partial);
                    let expansions: Vec<Vec<SyllableKey>> = if has_partial {
                        expand_keys(&syllables, super::SCAN_EXPANSION_LIMIT)
                            .into_iter()
                            .map(|expanded| expanded.into_vec())
                            .collect()
                    } else {
                        vec![syllables.clone()]
                    };
                    probes_here += expansions.len() as u64;
                    for expanded in &expansions {
                        let probe_key: Vec<String> =
                            expanded.iter().map(|key| key.text().to_string()).collect();
                        *probe_key_counts.entry(probe_key).or_insert(0) += 1;
                    }
                }
            }
            agg_windows += windows_here;
            agg_paths += paths_here;
            agg_probes += probes_here;
            println!(
                "[403] {},{},{},{},{},{},{},{}",
                input,
                prefix_len,
                consumed,
                matrix_entries,
                windows_here,
                paths_here,
                probes_here,
                unique_here.len(),
            );
        }
    }

    let unique_paths = sequence_counts.len() as u64;
    let unique_probes = probe_key_counts.len() as u64;
    let mut seen_1x = 0_u64;
    let mut seen_2x = 0_u64;
    let mut seen_3x = 0_u64;
    let mut seen_4x = 0_u64;
    let mut seen_5plus = 0_u64;
    for count in sequence_counts.values() {
        match *count {
            1 => seen_1x += 1,
            2 => seen_2x += 1,
            3 => seen_3x += 1,
            4 => seen_4x += 1,
            _ => seen_5plus += 1,
        }
    }
    // Histogram over the post-`expand_keys` probe count — the memoization
    // question the RSS report calls out: this is the count of distinct
    // syllable-key sequences that reach `Dictionary::lookup_into`, and how
    // often each one repeats across the run.
    let mut probe_seen_1x = 0_u64;
    let mut probe_seen_2x = 0_u64;
    let mut probe_seen_3x = 0_u64;
    let mut probe_seen_4x = 0_u64;
    let mut probe_seen_5plus = 0_u64;
    for count in probe_key_counts.values() {
        match *count {
            1 => probe_seen_1x += 1,
            2 => probe_seen_2x += 1,
            3 => probe_seen_3x += 1,
            4 => probe_seen_4x += 1,
            _ => probe_seen_5plus += 1,
        }
    }
    let paths_per_window = if agg_windows == 0 {
        0.0_f64
    } else {
        agg_paths as f64 / agg_windows as f64
    };
    let unique_ratio = if agg_paths == 0 {
        0.0_f64
    } else {
        unique_paths as f64 / agg_paths as f64
    };

    let probes_per_window = if agg_windows == 0 {
        0.0_f64
    } else {
        agg_probes as f64 / agg_windows as f64
    };
    let unique_probe_ratio = if agg_probes == 0 {
        0.0_f64
    } else {
        unique_probes as f64 / agg_probes as f64
    };

    println!("[403] --- aggregate ---");
    println!("[403] keystrokes = {}", total_keystrokes);
    println!("[403] windows    = {}", agg_windows);
    println!("[403] paths      = {}", agg_paths);
    println!("[403] probes     = {}", agg_probes);
    println!("[403] unique paths  = {}", unique_paths);
    println!("[403] unique probes = {}", unique_probes);
    println!("[403] paths/window   = {:.3}", paths_per_window);
    println!("[403] probes/window  = {:.3}", probes_per_window);
    println!("[403] unique/paths   = {:.3}", unique_ratio);
    println!("[403] unique/probes  = {:.3}", unique_probe_ratio);
    println!("[403] matrix_entries_total = {}", agg_matrix_entries);
    println!(
        "[403] paths hist: 1x={} 2x={} 3x={} 4x={} 5+x={}",
        seen_1x, seen_2x, seen_3x, seen_4x, seen_5plus
    );
    println!(
        "[403] probes hist: 1x={} 2x={} 3x={} 4x={} 5+x={}",
        probe_seen_1x, probe_seen_2x, probe_seen_3x, probe_seen_4x, probe_seen_5plus,
    );
    println!(
        "[403-JSON] {{\"keystrokes\":{},\"windows\":{},\"paths\":{},\"probes\":{},\
         \"unique_paths\":{},\"unique_probes\":{},\
         \"paths_per_window\":{:.6},\"probes_per_window\":{:.6},\
         \"unique_paths_ratio\":{:.6},\"unique_probes_ratio\":{:.6},\
         \"matrix_entries\":{},\
         \"paths_hist\":{{\"1x\":{},\"2x\":{},\"3x\":{},\"4x\":{},\"5+x\":{}}},\
         \"probes_hist\":{{\"1x\":{},\"2x\":{},\"3x\":{},\"4x\":{},\"5+x\":{}}}}}",
        total_keystrokes,
        agg_windows,
        agg_paths,
        agg_probes,
        unique_paths,
        unique_probes,
        paths_per_window,
        probes_per_window,
        unique_ratio,
        unique_probe_ratio,
        agg_matrix_entries,
        seen_1x,
        seen_2x,
        seen_3x,
        seen_4x,
        seen_5plus,
        probe_seen_1x,
        probe_seen_2x,
        probe_seen_3x,
        probe_seen_4x,
        probe_seen_5plus,
    );
}
