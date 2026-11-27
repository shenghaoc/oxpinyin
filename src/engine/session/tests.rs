//! The session's unit tests.

use super::ScanKey;
use super::selection::training_readings;
use crate::graph::{ExactSegment, SegmentGraph};

// Basis: Rust session parse intake contract and pinyin.cpp:1497–1525 at 074a2219.
#[test]
fn full_parsed_len_reflects_the_exact_chain() {
    let mut session = session();
    let segments: Vec<crate::graph::ExactSegment> = {
        use crate::graph::ExactSegment;
        let ni = crate::vocab::SyllableKey::from_text("ni").expect("ni");
        let hao = crate::vocab::SyllableKey::from_text("hao").expect("hao");
        vec![
            ExactSegment::new(0, 2, ni, 0),
            ExactSegment::new(3, 6, hao, 0),
        ]
    };
    session
        .replace_raw_exact("ni'hao", &segments)
        .expect("replace");
    assert_eq!(session.full_parsed_len(), 6);

    let reparsed = session
        .replace_raw("nihao")
        .map(|()| session.full_parsed_len());
    assert_eq!(
        reparsed,
        Ok(5),
        "a raw replacement must drop the exact chain"
    );
}

use crate::{
    model::{Dictionary, LanguageModel, PhraseList, UserModel},
    vocab::{PhraseEntry, PhraseToken, SyllableKey},
};

use super::{Selection, SelectionTarget, Session};
use crate::engine::SessionSettings;
use crate::engine::error::{EngineError, TrainingStop};
use crate::fixture_doubles::{
    FixtureDictionary, FixtureError, FixtureLanguageModel, FrequencyFixtureModel,
};

/// A backend that answers nothing, so these tests measure the state
/// machine and not a data set.
struct Silent;

impl PhraseList for Silent {
    type Error = EngineError;
    type Syllable = SyllableKey;

    fn lookup(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
        Ok(Vec::new())
    }
}

impl LanguageModel for Silent {
    fn amplification_lambda(&self) -> f32 {
        0.312_699
    }

    type Error = EngineError;
}

fn session() -> Session<Silent, Silent> {
    Session::new(SessionSettings::default(), Silent, Silent)
}

fn type_text(session: &mut Session<Silent, Silent>, text: &str) {
    let mut typed = String::new();
    for character in text.chars() {
        typed.push(character);
        session.replace_raw(&typed).expect("typing cannot fail");
    }
}

// Basis: pinyin_parser2.cpp:237–328 at 074a2219 and the Rust raw-buffer contract.
#[test]
fn replace_raw_keeps_the_bytes_the_pin_stops_at() {
    let mut session = session();
    for (raw, consumed) in [
        ("ni hao", 2),
        ("，nihao", 0),
        ("nihao", 5),
        ("nihao'", 6),
        ("'''", 3),
    ] {
        session.replace_raw(raw).expect("parse");
        assert_eq!(session.raw_input(), raw);
        assert_eq!(session.full_parsed_len(), consumed, "{raw}");
    }
}

// Basis: Rust selection record contract and pinyin.cpp:2501–2588 at 074a2219.
#[test]
fn a_refused_choose_after_a_fallback_selection_keeps_the_record() {
    let mut session = session();
    type_text(&mut session, "nihao");
    assert_eq!(
        select_row(&mut session, 0).expect("the fallback exists"),
        Selection::Completed
    );
    assert!(session.selected_tokens().is_empty());
    assert!(session.candidates().is_empty());

    assert_eq!(session.refuse_selection().unwrap(), Selection::Refused);
    assert!(!session.constraints.is_active());
    assert_eq!(session.composition_offset(), 5);
    assert!(session.selected_tokens().is_empty());

    session.guess_sentence().expect("the guess runs");
    assert_eq!(session.composition_offset(), 5);
}

// Basis: Rust selection record contract and pinyin.cpp:2501–2588 at 074a2219.
#[test]
fn a_refused_choose_still_rebuilds_a_record_that_lost_its_runs() {
    let mut session = trellis_session();
    type_and_select_over(&mut session, "ni", 1);
    type_and_select_over(&mut session, "hao", 2);
    session.replace_raw("n").expect("temporary parse");
    session.replace_raw("nihao").expect("restore input");
    assert!(session.selected_tokens().is_empty());
    assert!(session.constraints.is_active());

    assert_eq!(session.refuse_selection().unwrap(), Selection::Refused);
    assert_eq!(session.selected_tokens(), [1, 2]);
    assert_eq!(session.composition_offset(), 5);
}

// Basis: Divergence register entry for the full-pinyin parse's 16-bit steps.
#[test]
fn candidates_come_from_keys_past_the_pins_gint16_bound() {
    use super::CandidateKind;

    let mut session = train_session();
    session
        .replace_raw(&"ni".repeat(16_500))
        .expect("no failure");
    assert_eq!(session.full_parsed_len(), 33_000);
    let window = session
        .candidates_at(32_768)
        .expect("a syllable start is in range");
    assert!(window.iter().any(|candidate| {
        candidate.kind() != CandidateKind::Fallback && candidate.token() == Some(1)
    }));
}

/// §9's authored vocabulary: a one-key phrase with a two-key extension,
/// so the suggestion walk has exactly one strictly-longer token to find.
const LONGER_VOCAB: &str = "token=1\tkeys=fang\ttext=方\tunigram=1000\n\
token=2\tkeys=fang,mian\ttext=方面\tunigram=800\n\
token=3\tkeys=fang\ttext=房\tunigram=700\n";

/// The parity word: every gate shut, every key on — the shape the capi
/// ran before §9 and the one the differentials must not move.
const SORT_PARITY: u32 = 0x1e;
/// ibus preset 1 (the GSettings default): LONGER rows surface.
const SORT_IBUS_DEFAULT: u32 = 0x1c;

// Basis: Rust session API contract and pinyin.cpp:2184–2203 at 074a2219.
#[test]
fn sort_word_defaults_to_parity_and_sets_cleanly() {
    let mut session = session();
    assert_eq!(
        session.sort_options(),
        SORT_PARITY,
        "the engine default is the parity word"
    );
    session
        .set_sort_options(SORT_IBUS_DEFAULT)
        .expect("set cannot fail");
    assert_eq!(session.sort_options(), SORT_IBUS_DEFAULT);
    type_text(&mut session, "fang");
    assert_eq!(session.sort_options(), SORT_IBUS_DEFAULT);
}

// Basis: pinyin.cpp:1870–1933,2292–2300 at 074a2219.
#[test]
fn a_longer_row_surfaces_when_bit_2_clear_and_vanishes_when_set() {
    let mut session = Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(LONGER_VOCAB).expect("authored fixture"),
        FrequencyFixtureModel::parse(LONGER_VOCAB, "").expect("authored fixture"),
    );
    session.replace_raw("fang").expect("typing cannot fail");
    session
        .set_sort_options(SORT_IBUS_DEFAULT)
        .expect("set cannot fail");
    session.guess_sentence().expect("guess cannot fail");
    let longer = session
        .candidates()
        .iter()
        .find(|candidate| candidate.token().is_some() && candidate.consumed_bytes() == 0)
        .expect("the LONGER row is offered at 0x1c");
    assert_eq!(longer.text(), "方面");
    assert_eq!(longer.span_start(), 0);
    session
        .set_sort_options(SORT_PARITY)
        .expect("set cannot fail");
    assert!(
        session
            .candidates()
            .iter()
            .all(|candidate| !(candidate.token().is_some() && candidate.consumed_bytes() == 0)),
        "no LONGER row at the parity word"
    );
}

/// A session over the §9 vocabulary that builds its candidate list
/// eagerly or, as the C facades do, defers it to the first reader.
fn longer_session(defer: bool) -> Session<FixtureDictionary, FrequencyFixtureModel> {
    let mut session = Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(LONGER_VOCAB).expect("authored fixture"),
        FrequencyFixtureModel::parse(LONGER_VOCAB, "").expect("authored fixture"),
    );
    session.set_defer_list_builds(defer);
    session
}

// Basis: Rust session API contract and pinyin.cpp:2184–2203,2578–2590 at 074a2219.
#[test]
fn a_deferred_list_is_the_eager_list_at_every_read() {
    let mut eager = longer_session(false);
    let mut deferred = longer_session(true);
    eager.replace_raw("fangmian").expect("parse");
    deferred.replace_raw("fangmian").expect("parse");
    assert!(deferred.candidates().is_empty());
    assert!(!eager.candidates().is_empty());
    deferred.refresh_candidates().expect("deferred build");
    assert_eq!(deferred.candidates(), eager.candidates());
    assert_eq!(deferred.full_parsed_len(), eager.full_parsed_len());

    assert!(eager.guess_sentence().expect("guess"));
    assert!(deferred.guess_sentence().expect("guess"));
    assert_eq!(
        select_row(&mut deferred, 0).expect("the row is live"),
        select_row(&mut eager, 0).expect("the row is live")
    );
    assert_eq!(deferred.selected_tokens(), eager.selected_tokens());
    // The pin's choose writes no list (pinyin.cpp:2578-2590): the deferred
    // session keeps the list it built before the selection until a reader.
    assert_ne!(deferred.candidates(), eager.candidates());
    deferred.refresh_candidates().expect("deferred build");
    assert_eq!(deferred.candidates(), eager.candidates());

    eager.reset();
    deferred.reset();
    eager.replace_raw("fang").expect("parse");
    deferred.replace_raw("fang").expect("parse");
    assert_eq!(
        select_row(&mut deferred, 0).expect("select"),
        select_row(&mut eager, 0).expect("select")
    );
    assert_eq!(deferred.selected_tokens(), eager.selected_tokens());

    eager.reset();
    deferred.reset();
    eager.replace_raw("fang").expect("parse");
    deferred.replace_raw("fang").expect("parse");
    deferred.refresh_candidates().expect("deferred build");

    eager.set_sort_options(SORT_IBUS_DEFAULT).expect("sort");
    deferred.set_sort_options(SORT_IBUS_DEFAULT).expect("sort");
    assert_eq!(deferred.sort_options(), eager.sort_options());
    assert_ne!(deferred.candidates(), eager.candidates());
    deferred.refresh_candidates().expect("deferred build");
    assert_eq!(deferred.candidates(), eager.candidates());
}

/// A [`FixtureDictionary`] whose next `failures` lookups fail and whose
/// later ones serve the fixture: a backend that recovers.
struct Flaky {
    inner: FixtureDictionary,
    failures: core::cell::Cell<u32>,
}

impl PhraseList for Flaky {
    type Error = FixtureError;
    type Syllable = SyllableKey;

    fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, Self::Error> {
        let failures = self.failures.get();
        if failures > 0 {
            self.failures.set(failures - 1);
            return Err(FixtureError::MissingField {
                line: 0,
                field: "flaky",
            });
        }
        self.inner.lookup(syllables)
    }

    fn suggest_extension_tokens(
        &self,
        syllables: &[SyllableKey],
    ) -> Result<Vec<PhraseToken>, Self::Error> {
        self.inner.suggest_extension_tokens(syllables)
    }

    fn phrase_text_for_token(&self, token: u32) -> Option<crate::vocab::PhraseText> {
        PhraseList::phrase_text_for_token(&self.inner, token)
    }
}

/// A [`FixtureDictionary`] whose `unloaded` token has no readable item, as a
/// token of an unloaded library reads in the runtime.
struct UnloadedToken {
    inner: FixtureDictionary,
    unloaded: u32,
}

impl PhraseList for UnloadedToken {
    type Error = FixtureError;
    type Syllable = SyllableKey;

    fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, Self::Error> {
        self.inner.lookup(syllables)
    }

    fn phrase_text_for_token(&self, token: u32) -> Option<crate::vocab::PhraseText> {
        PhraseList::phrase_text_for_token(&self.inner, token)
    }

    fn phrase_length_for_token(&self, token: u32) -> Option<usize> {
        (token != self.unloaded)
            .then(|| PhraseList::phrase_length_for_token(&self.inner, token))
            .flatten()
    }
}

// Basis: pinyin.cpp:1712–1731 at 074a2219.
#[test]
fn an_unreadable_prefix_is_not_the_predecessor() {
    let predecessor = |unloaded: u32, prefixes: &[u32]| {
        let mut session = Session::new(
            SessionSettings::default(),
            UnloadedToken {
                inner: FixtureDictionary::parse(LONGER_VOCAB).expect("authored fixture"),
                unloaded,
            },
            FixtureLanguageModel::parse(LONGER_VOCAB, "").expect("authored fixture"),
        );
        session.set_candidate_prefixes(prefixes.iter().copied());
        session.prefix_predecessor()
    };
    assert_eq!(predecessor(0, &[1, 2]), 2, "the longest readable prefix");
    assert_eq!(
        predecessor(2, &[1, 2]),
        1,
        "the unreadable prefix is skipped"
    );
    assert_eq!(
        predecessor(2, &[2]),
        crate::vocab::SENTENCE_START,
        "no readable prefix"
    );
}

// Basis: Rust session API contract and pinyin.cpp:2184–2203 at 074a2219.
#[test]
fn a_failed_deferred_build_stays_stale_and_is_retried() {
    let open = |defer: bool| {
        let mut session = Session::new(
            SessionSettings::default(),
            Flaky {
                inner: FixtureDictionary::parse(LONGER_VOCAB).expect("authored fixture"),
                failures: core::cell::Cell::new(0),
            },
            FrequencyFixtureModel::parse(LONGER_VOCAB, "").expect("authored fixture"),
        );
        session.set_defer_list_builds(defer);
        session
    };
    let mut eager = open(false);
    let mut deferred = open(true);
    eager.replace_raw("fangmian").expect("parse");
    deferred.replace_raw("fangmian").expect("parse");

    deferred.dictionary().failures.set(1);
    deferred
        .refresh_candidates()
        .expect_err("the first scan fails");
    assert_eq!(deferred.dictionary().failures.get(), 0);

    deferred.refresh_candidates().expect("the retry succeeds");
    assert!(!eager.candidates().is_empty());
    assert_eq!(deferred.candidates(), eager.candidates());
    assert_eq!(deferred.full_parsed_len(), eager.full_parsed_len());
}

// Basis: pinyin.cpp:1870–1933,2292–2300 at 074a2219.
#[test]
fn a_longer_row_rides_an_empty_column_and_the_reserved_slot() {
    let mut session = Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(LONGER_VOCAB).expect("authored fixture"),
        FrequencyFixtureModel::parse(LONGER_VOCAB, "").expect("authored fixture"),
    );
    session.replace_raw("fang").expect("typing cannot fail");
    session
        .set_sort_options(SORT_IBUS_DEFAULT)
        .expect("set cannot fail");
    session.guess_sentence().expect("guess cannot fail");

    for offset in [3usize, 4] {
        let window = session
            .candidates_at(offset)
            .expect("every one of these offsets is in range");
        let longer = window
            .iter()
            .find(|candidate| candidate.token().is_some() && candidate.consumed_bytes() == 0)
            .unwrap_or_else(|| panic!("offset {offset} carries the LONGER row"));
        assert_eq!(longer.text(), "方面", "offset {offset}");
    }
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
    type Error = EngineError;

    fn observe(&mut self, history: &[PhraseToken], token: &PhraseToken) -> Result<(), Self::Error> {
        self.observed.push((history.to_vec(), *token));
        Ok(())
    }
}

fn train_session() -> Session<FixtureDictionary, FixtureLanguageModel> {
    Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(TRAIN_VOCAB).expect("authored fixture"),
        FixtureLanguageModel::parse(TRAIN_VOCAB, "").expect("authored fixture"),
    )
}

/// Trains row `index` through the production walk
/// ([`SessionState::train`]). `Runtime::train_session` terminates on
/// [`TrainingStop::PinAbort`]; the tests read the stop itself.
fn train_prepared<D, L, U>(
    session: &Session<D, L>,
    index: u8,
    user: &mut U,
) -> Result<bool, TrainingStop<U::Error>>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
    U: UserModel,
{
    let state: &crate::engine::SessionState = session;
    state.train(index, user)
}

/// Selects the candidate carrying `token` after typing `text` (the
/// selection must exist: this is the sentence record the training path
/// walks).
fn type_and_select(
    session: &mut Session<FixtureDictionary, FixtureLanguageModel>,
    text: &str,
    token: u32,
) {
    let mut typed = session.raw_input().to_owned();
    for character in text.chars() {
        typed.push(character);
        session.replace_raw(&typed).expect("typing cannot fail");
    }
    let index = session
        .candidates()
        .iter()
        .position(|candidate| candidate.token() == Some(token))
        .expect("the fixture candidate is offered");
    select_row(session, index).expect("selection cannot fail");
}

// Basis: phonetic_lookup.h:847–932 and phonetic_key_matrix.cpp:603–664 at 074a2219.
#[test]
fn train_observes_each_recorded_token_after_its_prefix() {
    let mut session = train_session();
    type_and_select(&mut session, "ni", 1);
    type_and_select(&mut session, "hao", 2);
    assert_eq!(session.selected_tokens(), [1, 2]);

    session.sentence.rows = vec![crate::engine::nbest::NbestRow {
        text: Some("\u{4f60}".into()),
        spans: Vec::new(),
        keys: 1,
        span: 2,
    }];
    let mut recorder = Recorder {
        observed: Vec::new(),
    };
    assert!(train_prepared(&session, 0, &mut recorder).expect("training cannot fail"));
    assert_eq!(recorder.observed, vec![(Vec::new(), 1), (vec![1], 2),]);

    assert!(train_prepared(&session, 0, &mut recorder).expect("training cannot fail"));
    assert_eq!(recorder.observed.len(), 4);
}

// Basis: pinyin.cpp:2670–2690 and zhuyin.cpp:1696–1715 at 074a2219.
#[test]
fn training_without_results_does_not_observe_selection_history() {
    let mut session = train_session();
    type_and_select(&mut session, "ni", 1);
    assert!(!session.selected_tokens().is_empty());
    assert!(session.sentence.rows.is_empty());
    let mut recorder = Recorder {
        observed: Vec::new(),
    };
    assert!(!train_prepared(&session, 0, &mut recorder).expect("empty results are quiet"));
    assert!(recorder.observed.is_empty());
}

// Basis: phonetic_lookup.h:847–932 and phonetic_key_matrix.cpp:603–664 at 074a2219.
#[test]
fn training_checks_reparsed_span_columns() {
    for (raw, expected) in [
        ("ni", Err(TrainingStop::PinAbort)),
        ("'ni", Err(TrainingStop::PinAbort)),
        ("ni'hao", Err(TrainingStop::PinAbort)),
        ("ni'ha", Ok(true)),
    ] {
        let mut session = trellis_session();
        session.replace_raw("nihao").expect("initial parse");
        assert!(session.guess_sentence().expect("guess"));
        select_row(&mut session, 1).expect("choose 你");
        session.replace_raw(raw).expect("reparse");
        let mut recorder = Recorder {
            observed: Vec::new(),
        };
        assert_eq!(
            train_prepared(&session, 0, &mut recorder),
            expected,
            "{raw}"
        );
    }
}

// Basis: phonetic_lookup.h:847–932 and phonetic_key_matrix.cpp:603–664 at 074a2219.
#[test]
fn train_after_a_reparse_and_a_choose_stops_the_walk_at_the_store_length() {
    const VOCAB: &str = "token=1\tkeys=ni\ttext=你\tunigram=1000\n\
                         token=2\tkeys=hao\ttext=好\tunigram=900\n\
                         token=3\tkeys=ma\ttext=吗\tunigram=800\n";
    let mut session = Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(VOCAB).expect("authored fixture"),
        TrellisModel {
            blended: 0.5,
            unigram: 0.25,
        },
    );
    let choose = |session: &mut Session<FixtureDictionary, TrellisModel>, offset, token| {
        let window = session.candidates_at(offset).expect("offset in range");
        let index = window
            .iter()
            .position(|candidate| candidate.token() == Some(token))
            .expect("the fixture candidate is offered");
        session
            .select_target(&SelectionTarget::of_candidate(
                window.iter().nth(index).expect("the row is in the window"),
                offset,
                None,
            ))
            .expect("selection cannot fail");
    };
    session.replace_raw("nihaoma").expect("initial parse");
    assert!(session.guess_sentence().expect("guess"));
    choose(&mut session, 2, 2);
    assert!(session.guess_sentence().expect("guess"));
    assert_eq!(session.sentence_text(0).expect("row 0 exists"), "你好吗");
    session.replace_raw("niha").expect("shrink");
    choose(&mut session, 0, 1);
    let mut recorder = Recorder {
        observed: Vec::new(),
    };
    assert!(
        train_prepared(&session, 0, &mut recorder).expect("the walk stops at the store length"),
        "the stale row trains"
    );
    assert!(
        recorder.observed.iter().all(|(_, token)| *token != 3),
        "the span of 吗 lies past the store"
    );
}

// Basis: phonetic_lookup.h:858–929 and phonetic_key_matrix.cpp:661–664 at 074a2219.
#[test]
fn an_earlier_steps_store_refusal_precedes_a_later_pin_abort() {
    struct Failing;
    impl UserModel for Failing {
        type Error = EngineError;

        fn observe(
            &mut self,
            _history: &[PhraseToken],
            _token: &PhraseToken,
        ) -> Result<(), Self::Error> {
            Err(EngineError::UserModel)
        }
    }

    let mut session = trellis_session();
    session.replace_raw("nihao").expect("initial parse");
    assert!(session.guess_sentence().expect("guess"));
    select_row(&mut session, 1).expect("choose 你");
    session.replace_raw("ni").expect("shrink");
    // The steps run in order: the first span's store refuses before the
    // walk reaches the stale second span, which aborts once the store
    // takes the first.
    assert_eq!(
        train_prepared(&session, 0, &mut Failing),
        Err(TrainingStop::User(EngineError::UserModel))
    );
    let mut recorder = Recorder {
        observed: Vec::new(),
    };
    assert_eq!(
        train_prepared(&session, 0, &mut recorder),
        Err(TrainingStop::PinAbort)
    );
    assert_eq!(recorder.observed.len(), 1, "the first step stays recorded");
    assert!(session.training_aborts(0));
}

// Basis: phonetic_lookup.h:847–932 and phonetic_key_matrix.cpp:603–664 at 074a2219.
#[test]
fn training_refuses_a_stale_forced_token_at_a_later_span() {
    let mut session = trellis_session();
    session.replace_raw("nihao").expect("parse");
    assert!(session.guess_sentence().expect("guess"));
    session.constraints.resize(6);
    session.constraints.add(0, 2, 1);
    session.constraints.add(2, 4, 9);
    let mut recorder = Recorder {
        observed: Vec::new(),
    };
    let error = train_prepared(&session, 0, &mut recorder).expect_err("the later forcing refuses");
    assert_eq!(error, TrainingStop::PinAbort);
}

// Basis: phonetic_lookup.h:847–932 and phonetic_key_matrix.cpp:603–664 at 074a2219.
#[test]
fn a_stored_phrase_trains_with_its_stored_length() {
    use crate::engine::constraint::PhraseSpan;

    struct Readings(Vec<Vec<crate::parser::ChewingKey>>);
    impl UserModel for Readings {
        type Error = EngineError;

        fn observe(
            &mut self,
            _history: &[PhraseToken],
            _token: &PhraseToken,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn observe_with_keys(
            &mut self,
            _last: Option<PhraseToken>,
            _token: &PhraseToken,
            readings: &mut impl crate::model::ReadingWalk,
        ) -> Result<(), Self::Error> {
            while let Some(reading) = readings.next_reading() {
                self.0.push(reading.to_vec());
            }
            Ok(())
        }
    }

    let text = crate::vocab::PhraseText::from_words([0x4F60, 0]).expect("stored words");
    assert_eq!((text.phrase_length(), text.outward_words().count()), (2, 1));
    let mut session = trellis_session();
    session.replace_raw("nihao").expect("parse");
    duplicate_the_first_key(&mut session);
    session.constraints.resize(6);
    session.constraints.add(0, 5, 1);
    let spans = vec![PhraseSpan {
        start: 0,
        token: 1,
        phrase_length: text.phrase_length(),
    }];
    session.sentence.rows = vec![crate::engine::nbest::NbestRow {
        text: Some("你".into()),
        spans,
        keys: 1,
        span: 2,
    }];
    let mut model = Readings(Vec::new());
    assert!(train_prepared(&session, 0, &mut model).expect("the forced span trains"));
    assert_eq!(model.0.len(), 2, "each complete matrix path is observed");
    assert_eq!(model.0[0], model.0[1], "identical paths both write");
    assert!(!model.0.is_empty(), "the two-key reading of nihao trains");
    assert!(
        model.0.iter().all(|reading| reading.len() == 2),
        "every trained reading has the stored length: {:?}",
        model.0
    );
}

// Basis: Rust SessionState::train error propagation contract.
#[test]
fn train_reports_a_failing_user_model() {
    struct Failing;
    impl UserModel for Failing {
        type Error = EngineError;

        fn observe(
            &mut self,
            _history: &[PhraseToken],
            _token: &PhraseToken,
        ) -> Result<(), Self::Error> {
            Err(EngineError::UserModel)
        }
    }

    let mut session = train_session();
    type_and_select(&mut session, "ni", 1);
    session.sentence.rows = vec![crate::engine::nbest::NbestRow {
        text: Some("你".into()),
        spans: Vec::new(),
        keys: 1,
        span: 2,
    }];
    assert_eq!(
        train_prepared(&session, 0, &mut Failing),
        Err(TrainingStop::User(EngineError::UserModel)),
        "a user-model failure is the model's own, not a pin abort"
    );
}

// Basis: Rust selection record contract and pinyin.cpp:2501–2588 at 074a2219.
#[test]
fn a_row_records_its_whole_token_path() {
    use crate::engine::nbest::NbestRow;

    let mut session = train_session();
    session.replace_raw("nihao").expect("typing cannot fail");
    assert!(!session.sentence_lookup_active(), "no lookup has run yet");

    session.sentence.rows = vec![NbestRow {
        text: Some("\u{884}".into()),
        spans: vec![crate::engine::constraint::PhraseSpan {
            start: 0,
            token: 9,
            phrase_length: 1,
        }],
        keys: 1,
        span: 2,
    }];
    session.refresh().expect("refresh cannot fail");
    assert!(
        !session.sentence_lookup_active(),
        "only guess_sentence activates the lookup"
    );

    assert_eq!(
        select_row(&mut session, 0).expect("the row is live"),
        Selection::Continued
    );
    assert_eq!(session.selected_tokens(), [9]);
}

// Basis: Sentence-surface SPEC §8 and pinyin.cpp:2116–2125,2515–2520 at 074a2219.
#[test]
fn a_shifted_row_records_its_own_rank_not_its_position() {
    use crate::engine::nbest::NbestRow;

    let mut session = train_session();
    session.replace_raw("nihao").expect("typing cannot fail");

    session.sentence.rows = vec![
        NbestRow {
            text: Some("\u{597d}".into()),
            spans: vec![crate::engine::constraint::PhraseSpan {
                start: 0,
                token: 0x100,
                phrase_length: 1,
            }],
            keys: 1,
            span: 3,
        },
        NbestRow {
            text: Some("\u{597d}".into()),
            spans: vec![crate::engine::constraint::PhraseSpan {
                start: 0,
                token: 0x101,
                phrase_length: 1,
            }],
            keys: 1,
            span: 3,
        },
        NbestRow {
            text: Some("\u{6d69}".into()),
            spans: vec![crate::engine::constraint::PhraseSpan {
                start: 0,
                token: 0x102,
                phrase_length: 1,
            }],
            keys: 1,
            span: 3,
        },
    ];
    session.constraints.resize(6);
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

    select_row(&mut session, hao).expect("the row is live");
    assert_eq!(
        session.selected_tokens(),
        [0x102],
        "the chosen row records its own token path, not the deduped row 1's"
    );
}

// Basis: Rust sentence lookup gate and pinyin.cpp:1372–1425 at 074a2219.
#[test]
fn a_lookup_activates_the_sentence_gate_even_without_rows() {
    let mut session = session();
    assert!(!session.sentence_lookup_active());
    session
        .replace_raw("qqq")
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

// Basis: Rust normalized lookup offset contract and pinyin.cpp:2987–3005 at 074a2219.
#[test]
fn normalized_lookup_offset_walks_only_physical_separator_runs() {
    let mut session = session();
    for (raw, offset, expected) in [("ni''hao", 4, 2), ("'ni", 1, 1), ("ni'", 3, 2)] {
        session.replace_raw(raw).expect("parse");
        assert_eq!(
            session.normalized_lookup_offset(offset),
            Ok(expected),
            "{raw}"
        );
    }
    assert_eq!(
        session.normalized_lookup_offset(4),
        Err(EngineError::LookupOffsetOutOfRange { offset: 4, len: 3 })
    );
}

// Basis: pinyin.cpp:1712–1866 and zhuyin.cpp:1250–1261 at 074a2219.
#[test]
fn dynamic_adjust_folds_the_bigram_term_only_with_the_bit_and_a_gram() {
    use crate::{OptionBits, model::MergedGram};

    let clear = OptionBits::default();
    let set = OptionBits::DYNAMIC_ADJUST;
    let gram = MergedGram::new(1_000, vec![(7, 100), (42, 500)]);

    for (options, row, token, expected) in [
        (set, None, 42, 0.0),
        (set, Some(&gram), 43, 0.0),
        (set, Some(&gram), 42, 0.5),
    ] {
        assert_eq!(
            super::dynamic_adjust_bigram_possibility(options, row, token),
            Ok(expected)
        );
    }
    let short = MergedGram::merge(None, Some(vec![0, 0])).expect("a user gram");
    assert_eq!(
        super::dynamic_adjust_bigram_possibility(set, Some(&short), 42),
        Err(crate::single_gram::GramFault::Short)
    );
    let mut tailed_value = MergedGram::new(4, vec![(7, 4)]).to_bytes();
    tailed_value.push(1);
    let tailed = MergedGram::merge(None, Some(tailed_value)).expect("a user gram");
    assert_eq!(
        super::dynamic_adjust_bigram_possibility(set, Some(&tailed), 7),
        Ok(1.0)
    );
    assert_eq!(
        super::dynamic_adjust_bigram_possibility(set, Some(&tailed), 42),
        Err(crate::single_gram::GramFault::OverRead)
    );
    assert_eq!(
        super::dynamic_adjust_bigram_possibility(clear, Some(&short), 42),
        Ok(0.0),
        "bit clear reads nothing"
    );

    let (unigram, total) = (1_234_u64, 51_051_831_u64);
    let base = super::amplified_frequency_with_bigram(unigram, total, 0.0, super::PIN_LAMBDA_F32);
    let adjusted =
        super::amplified_frequency_with_bigram(unigram, total, 0.5, super::PIN_LAMBDA_F32);
    assert!(
        adjusted > base,
        "a non-zero possibility must raise the amplified frequency ({adjusted} vs {base})"
    );
    assert_eq!(
        base,
        super::amplified_frequency(unigram, total, super::PIN_LAMBDA_F32),
        "the bit-clear path is the pre-existing unigram law exactly"
    );
}

/// A system-only facade with a caller-chosen entry count, so a guess's
/// merge count can be measured against its candidate count.
struct SystemPhrases(Vec<PhraseEntry>);

impl PhraseList for SystemPhrases {
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
    row: Option<crate::model::MergedGram>,
    merges: std::rc::Rc<std::cell::Cell<usize>>,
    asked_about: std::rc::Rc<std::cell::Cell<u32>>,
}

impl LanguageModel for CountingBigrams {
    fn amplification_lambda(&self) -> f32 {
        0.312_699
    }

    type Error = EngineError;

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
    ) -> Result<Option<crate::model::MergedGram>, EngineError> {
        self.merges.set(self.merges.get() + 1);
        self.asked_about.set(*prev);
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
    row: Option<crate::model::MergedGram>,
) -> DynamicAdjustRun {
    use crate::OptionBits;
    use std::cell::Cell;
    use std::rc::Rc;

    let merges = Rc::new(Cell::new(0_usize));
    let asked_about = Rc::new(Cell::new(u32::MAX));
    let phrases = (0..entries)
        .map(|index| {
            PhraseEntry::new(
                DYNAMIC_ADJUST_FIRST + u32::try_from(index).expect("small index"),
                DYNAMIC_ADJUST_TEXTS[index].to_owned(),
            )
        })
        .collect();
    let mut session = Session::new(
        SessionSettings::default(),
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
    );
    // Both arms set the same word apart from the one bit, so nothing
    // else about the parse can differ between them.
    session
        .set_options(if dynamic {
            OptionBits::PINYIN_INCOMPLETE | OptionBits::DYNAMIC_ADJUST
        } else {
            OptionBits::PINYIN_INCOMPLETE
        })
        .expect("set_options");
    session.replace_raw("a").expect("typing cannot fail");
    DynamicAdjustRun {
        texts: session
            .candidates()
            .iter()
            .filter_map(|candidate| candidate.text_str())
            .map(str::to_owned)
            .collect(),
        merges: merges.get(),
        asked_about: asked_about.get(),
    }
}

// Basis: pinyin.cpp:1712–1866 and zhuyin.cpp:1250–1261 at 074a2219.
#[test]
fn dynamic_adjust_merges_one_row_per_guess_and_lifts_only_the_credited_token() {
    use crate::model::MergedGram;

    let credits_second = || MergedGram::new(1_000, vec![(DYNAMIC_ADJUST_SECOND, 500)]);

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

    let no_row = dynamic_adjust_run(2, true, None);
    assert_eq!(
        no_row.asked_about,
        crate::vocab::SENTENCE_START,
        "offset 0 asks about sentence_start, exactly as `_get_previous_token` answers"
    );
    assert_eq!(
        no_row.texts,
        ["系", "统"],
        "a model with no row for the previous token contributes no term"
    );

    let lifted = dynamic_adjust_run(2, true, Some(credits_second()));
    assert_eq!(
        lifted.texts,
        ["统", "系"],
        "the bigram term must actually move the credited candidate above its tie peer"
    );

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

// Basis: Frozen class-A frequency probes in the candidate-construction SPEC §8.4.
#[test]
fn amplified_frequency_pins_the_class_a_probe_values() {
    const PIN_TOTAL: u64 = 51_051_831;
    for (count, expected) in [(0, 0), (14, 3), (16, 3), (2_349_890, 530_766)] {
        assert_eq!(
            super::amplified_frequency(count, PIN_TOTAL, super::PIN_LAMBDA_F32),
            expected
        );
    }
}

// Basis: Rust candidate window batching and reuse contracts.
#[test]
fn window_flush_is_token_ascending_and_one_row_per_token() {
    use super::{CandidateKind, flush_window_batch};
    use crate::engine::candidate::Candidate;

    let mut batch = vec![
        Candidate::new(
            compact_str::CompactString::from("狗狗"),
            CandidateKind::Phrase,
            2,
            4,
            Some(0x0300_16df),
            None,
        ),
        Candidate::new(
            compact_str::CompactString::from("沟谷"),
            CandidateKind::Phrase,
            2,
            4,
            Some(0x0100_4c41),
            None,
        ),
        Candidate::new(
            compact_str::CompactString::from("沟谷"),
            CandidateKind::Phrase,
            1,
            4,
            Some(0x0100_4c41),
            None,
        ),
    ];
    let mut into = Vec::new();
    flush_window_batch(&mut batch, &mut into);
    let tokens: Vec<u32> = into.iter().filter_map(|c| c.token()).collect();
    assert_eq!(tokens, [0x0100_4c41, 0x0300_16df]);
    assert_eq!(
        into[0].consumed_keys(),
        2,
        "the first collected row of a duplicated token is the one kept"
    );
}

fn window_row(text: &str, token: Option<u32>) -> crate::engine::candidate::Candidate {
    crate::engine::candidate::Candidate::new(text, super::CandidateKind::Phrase, 1, 2, token, None)
}

// Basis: Rust candidate window batching and reuse contracts.
#[test]
fn unordered_windows_keep_the_existing_sort_and_dedup() {
    use super::{order_window_tail, sort_dedup_window_tail, window_ascends_by_library_then_token};

    let first = window_row("first", Some(0x0100_0002));
    let smaller = window_row("smaller", Some(0x0100_0001));
    let other_library = window_row("other library", Some(0x0200_0001));
    let tokenless = window_row("tokenless", None);
    let duplicate = window_row("later duplicate", Some(0x0100_0002));
    let tagged = window_row("tagged", Some(0x8100_0005));
    for (ordered, window, expected) in [
        (true, vec![], vec![]),
        (true, vec![first.clone()], vec![first.clone()]),
        (
            true,
            vec![smaller.clone(), first.clone(), other_library.clone()],
            vec![smaller.clone(), first.clone(), other_library.clone()],
        ),
        (false, vec![tokenless.clone()], vec![tokenless]),
        (false, vec![first.clone(), duplicate], vec![first.clone()]),
        (
            false,
            vec![first.clone(), smaller.clone()],
            vec![first.clone(), smaller.clone()],
        ),
        (
            false,
            vec![first.clone(), other_library.clone(), smaller.clone()],
            vec![first, smaller, other_library.clone()],
        ),
        (
            false,
            vec![other_library.clone(), tagged.clone()],
            vec![tagged, other_library],
        ),
    ] {
        assert_eq!(window_ascends_by_library_then_token(&window), ordered);
        let prefix = window_row("prior window", Some(0x0100_0002));
        let mut rows = vec![prefix.clone()];
        rows.extend(window);
        let mut sorted = rows.clone();
        sort_dedup_window_tail(&mut sorted, 1);
        order_window_tail(&mut rows, 1);
        assert_eq!(rows, sorted);
        assert_eq!(rows[0], prefix);
        assert_eq!(rows[1..], expected);
    }
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
    fn amplification_lambda(&self) -> f32 {
        0.312_699
    }

    type Error = EngineError;

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

// Basis: pinyin.cpp:1772–1788,1794–1866,2224–2286 at 074a2219.
#[test]
fn addon_candidates_rank_on_their_own_amplified_scale() {
    use super::CandidateKind;
    use crate::engine::candidate::Candidate;

    let session = Session::new(
        SessionSettings::default(),
        Silent,
        FixedUnigrams {
            system: 14,
            addon: 14,
            total: 51_051_831,
            addon_total: 25_525_916,
        },
    );
    let collected = vec![
        Candidate::new(
            compact_str::CompactString::from("股"),
            CandidateKind::Phrase,
            1,
            3,
            Some(1),
            None,
        ),
        Candidate::new(
            compact_str::CompactString::from("附"),
            CandidateKind::Addon,
            1,
            3,
            Some(2),
            None,
        ),
    ];
    let mut frequencies = vec![7; 5];
    session
        .candidate_frequencies(
            &collected,
            None,
            super::Scan::ExplicitGuess,
            false,
            &mut frequencies,
        )
        .expect("frequency reads cannot fail here");
    assert_eq!(
        frequencies,
        [3, 6],
        "system = amplified(14, 51_051_831) = 3; addon = amplified(14, 25_525_916) = 6"
    );
}

/// One entry per facade, so a scan's two batches are exactly one
/// system and one addon candidate.
struct TwoFacadeDict {
    system: PhraseEntry,
    addon: PhraseEntry,
}

impl PhraseList for TwoFacadeDict {
    type Error = EngineError;
    type Syllable = SyllableKey;

    fn lookup(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
        Ok(vec![self.system.clone()])
    }

    fn lookup_addon(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
        Ok(vec![self.addon.clone()])
    }
}

// Basis: Rust candidate window batching and reuse contracts.
#[test]
fn a_refused_partial_predecessor_scan_leaves_the_next_window_as_a_fresh_one() {
    use crate::{OptionBits, model::MergedGram};

    let whole = MergedGram::new(4, vec![(DYNAMIC_ADJUST_FIRST, 4)]).to_bytes();
    let mut partial = whole.clone();
    partial.push(1);
    let row = MergedGram::merge(Some(whole), Some(partial));

    let build = || {
        let mut session = Session::new(
            SessionSettings::default(),
            TwoFacadeDict {
                system: PhraseEntry::new(DYNAMIC_ADJUST_FIRST, "系"),
                addon: PhraseEntry::new(0x0500_0002, "附"),
            },
            CountingBigrams {
                unigrams: FixedUnigrams {
                    system: 14,
                    addon: 7,
                    total: 51_051_831,
                    addon_total: 25_525_916,
                },
                row: None,
                merges: Default::default(),
                asked_about: Default::default(),
            },
        );
        session.set_defer_list_builds(true);
        session
            .set_options(OptionBits::DYNAMIC_ADJUST)
            .expect("set options");
        session.replace_raw("a").expect("parse");
        session
    };

    for before_cursor in [false, true] {
        let guess = if before_cursor {
            |session: &mut Session<TwoFacadeDict, CountingBigrams>| session.candidates_ending_at(1)
        } else {
            |session: &mut Session<TwoFacadeDict, CountingBigrams>| session.candidates_at(0)
        };
        let mut session = build();
        let window = guess(&mut session).expect("warm scan");
        assert!(!window.is_empty(), "before cursor: {before_cursor}");
        let fresh = guess(&mut build()).expect("fresh scan");
        session.recycle_candidates(window);
        let reused = guess(&mut session).expect("clean reuse");
        assert_eq!(reused, fresh);
        session.recycle_candidates(reused);
        session.model.row.clone_from(&row);

        assert_eq!(
            guess(&mut session).expect_err("partial predecessor must refuse"),
            EngineError::UnsafeLookupPredecessor,
        );

        session.model.row = None;
        let after = guess(&mut session).expect("scan after the refusals");
        assert_eq!(after, fresh, "before cursor: {before_cursor}");
    }
}

// Basis: pinyin.cpp:1772–1788,1794–1866,2224–2286 at 074a2219.
#[test]
fn window_scan_emits_system_candidates_before_addon_candidates() {
    let mut session = Session::new(
        SessionSettings::default(),
        TwoFacadeDict {
            system: PhraseEntry::new(0x0100_0001, "系".to_owned()),
            addon: PhraseEntry::new(0x0500_0002, "附".to_owned()),
        },
        FixedUnigrams {
            system: 14,
            addon: 7,
            total: 51_051_831,
            addon_total: 25_525_916,
        },
    );
    session.replace_raw("a").expect("typing cannot fail");
    let texts: Vec<&str> = session
        .candidates()
        .iter()
        .filter_map(super::super::candidate::Candidate::text_str)
        .collect();
    assert_eq!(
        texts,
        ["系", "附"],
        "a full three-key tie must keep the pin's system-before-addon array order"
    );
}

/// Two one-key entries, ranked by a model whose system unigrams a test
/// moves between scans, as a training write moves the live item.
struct TrainedUnigrams {
    counts: std::rc::Rc<std::cell::Cell<(u64, u64)>>,
}

impl LanguageModel for TrainedUnigrams {
    fn amplification_lambda(&self) -> f32 {
        0.312_699
    }

    type Error = EngineError;

    fn unigram_freq(&self, token: &PhraseToken) -> Result<Option<u64>, EngineError> {
        let (first, second) = self.counts.get();
        Ok(Some(if *token == 0x0100_0001 { first } else { second }))
    }

    fn unigram_total(&self) -> Result<Option<u64>, EngineError> {
        Ok(Some(51_051_831))
    }
}

// Basis: pinyin.cpp:1772–1788,1794–1866,2224–2286 at 074a2219.
#[test]
fn a_window_ranked_after_a_training_write_reads_the_rewritten_counts() {
    fn texts(list: &crate::engine::candidate::CandidateList) -> Vec<&str> {
        list.iter()
            .filter_map(crate::engine::candidate::Candidate::text_str)
            .collect()
    }

    let counts = std::rc::Rc::new(std::cell::Cell::new((5_000_000, 10_000_000)));
    let mut session = Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(
            "token=16777217\tkeys=a\ttext=一\tunigram=1\n\
             token=16777218\tkeys=a\ttext=二\tunigram=1\n",
        )
        .expect("authored fixture"),
        TrainedUnigrams {
            counts: std::rc::Rc::clone(&counts),
        },
    );
    session.replace_raw("a").expect("typing cannot fail");
    assert_eq!(
        texts(session.candidates()),
        ["二", "一"],
        "the larger count ranks first"
    );

    counts.set((5_000_000, 1));
    assert_eq!(
        texts(session.candidates()),
        ["二", "一"],
        "the retained list is not ranked again"
    );
    let window = session.candidates_at(0).expect("window at 0");
    assert_eq!(
        texts(&window),
        ["一", "二"],
        "a new window reads the new counts"
    );
}

// Basis: pinyin.cpp:1635–1637,2053 at 074a2219 and the Rust unreadable-row contract.
#[test]
fn a_window_whose_lookup_met_an_unreadable_item_says_so() {
    struct UnreadableDict;
    impl PhraseList for UnreadableDict {
        type Error = EngineError;
        type Syllable = SyllableKey;

        fn lookup(&self, _: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
            Ok(vec![PhraseEntry::new(0x0100_0001, "你")])
        }

        fn lookup_into_flagged(
            &self,
            syllables: &[SyllableKey],
            out: &mut Vec<PhraseEntry>,
        ) -> Result<bool, EngineError> {
            *out = self.lookup(syllables)?;
            Ok(true)
        }
    }
    let mut session = Session::new(SessionSettings::default(), UnreadableDict, Silent);
    session.replace_raw("ni").expect("typing cannot fail");
    let window = session.candidates_at(0).expect("window at 0");
    assert!(window.had_unreadable_item());
    assert!(
        window.iter().any(|row| row.text_str() == Some("你")),
        "the readable rows stay listed"
    );
}

// Basis: phonetic_key_matrix.cpp:30–84,87–318 at 074a2219 and the Rust scan-matrix contract.
#[test]
fn exact_scan_matrix_adds_fuzzy_without_full_pinyin_splits() {
    use crate::OptionBits;
    use crate::graph::{ExactSegment, SegmentGraph};

    let options =
        OptionBits::PINYIN_AMB_L_N | OptionBits::USE_DIVIDED_TABLE | OptionBits::USE_RESPLIT_TABLE;
    let ni = SyllableKey::from_text("ni").expect("ni key");
    let graph =
        SegmentGraph::build_exact(b"ni", &[ExactSegment::new(0, 2, ni, 3)]).expect("exact ni");
    let columns = super::build_scan_matrix(&graph, options, false);
    let keys: Vec<_> = columns[0]
        .iter()
        .map(|key| (key.key.text(), key.from, key.to, key.tone))
        .collect();
    assert_eq!(keys, [("ni", 0, 2, 3), ("li", 0, 2, 3)]);

    let bian = SyllableKey::from_text("bian").expect("bian key");
    let graph = SegmentGraph::build_exact(b"bian", &[ExactSegment::new(0, 4, bian, 0)])
        .expect("exact bian");
    let columns = super::build_scan_matrix(&graph, options, false);
    assert_eq!(columns[0].len(), 1);
    assert_eq!(columns[0][0].key, bian);
    assert!(columns[1..4].iter().all(Vec::is_empty));

    let fan = SyllableKey::from_text("fan").expect("fan key");
    let gan = SyllableKey::from_text("gan").expect("gan key");
    let graph = SegmentGraph::build_exact(
        b"fangan",
        &[
            ExactSegment::new(0, 3, fan, 0),
            ExactSegment::new(3, 6, gan, 0),
        ],
    )
    .expect("exact fan gan");
    let columns = super::build_scan_matrix(&graph, options, false);
    assert_eq!(columns[0].len(), 1);
    assert_eq!(columns[0][0].key, fan);
    assert!(columns[4].is_empty());
}

// Basis: phonetic_key_matrix.cpp:30–84,87–318 at 074a2219 and the Rust scan-matrix contract.
#[test]
fn scan_matrix_tone_rides_fuzzy_and_locks_the_split_tables() {
    use crate::OptionBits;
    use crate::graph::SegmentGraph;

    let tables = OptionBits::USE_DIVIDED_TABLE | OptionBits::USE_RESPLIT_TABLE;
    let incomplete = OptionBits::PINYIN_INCOMPLETE | tables;
    let toned = OptionBits::PINYIN_INCOMPLETE | OptionBits::USE_TONE | tables;

    let graph = SegmentGraph::build_with_options(b"zai4", toned).expect("valid");
    let columns = super::build_scan_matrix(
        &graph,
        OptionBits::PINYIN_INCOMPLETE | OptionBits::USE_TONE | OptionBits::PINYIN_AMB_Z_ZH | tables,
        true,
    );
    let column: Vec<_> = columns[0]
        .iter()
        .map(|key| (key.key.text(), key.to, key.tone))
        .collect();
    assert!(column.contains(&("zai", 4, 4)));
    assert!(column.contains(&("zhai", 4, 4)));

    let toneless = SegmentGraph::build_with_options(b"anan", incomplete).expect("valid");
    let columns = super::build_scan_matrix(&toneless, incomplete, true);
    assert!(
        columns[0]
            .iter()
            .any(|key| key.key.text() == "an" && key.to == 2)
    );

    let after_separator =
        SegmentGraph::build_with_options(b"chui'lianai", incomplete).expect("valid");
    let columns = super::build_scan_matrix(&after_separator, incomplete, true);
    let lian = columns[4]
        .iter()
        .find(|key| key.key.text() == "lian")
        .expect("lian is added at the apostrophe column");
    assert_eq!((lian.from, lian.syllable_start, lian.to), (4, 5, 9));
    assert!(lian.crosses_separator);
    let ai = columns[9]
        .iter()
        .find(|key| key.key.text() == "ai")
        .expect("ai is added at the split column");
    assert_eq!((ai.from, ai.to, ai.crosses_separator), (9, 11, false));

    let toned_pair = SegmentGraph::build_with_options(b"a4nan", toned).expect("valid");
    let columns = super::build_scan_matrix(&toned_pair, toned, true);
    assert!(!columns[0].iter().any(|key| key.key.text() == "an"));

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

/// A model whose n-best step probabilities exist, so the trellis runs.
#[derive(Clone)]
struct TrellisModel {
    blended: f64,
    unigram: f64,
}

/// Every predecessor's row holds every fixture token at `freq / total`
/// of 1, so each ranged token takes a bigram step at `blended`: the f32
/// weight `blended - unigram` plus the unigram branch, both exact here.
impl LanguageModel for TrellisModel {
    fn amplification_lambda(&self) -> f32 {
        0.312_699
    }

    type Error = EngineError;

    fn nbest_successors_in_row<'a>(
        &self,
        _prev: &PhraseToken,
        row: &'a mut crate::model::NbestStepRow,
    ) -> Result<Option<crate::model::NbestSuccessors<'a>>, EngineError> {
        let every = || {
            crate::model::MergedGram::new(
                1,
                (1..=16)
                    .chain(0x0100_0001..=0x0100_0010)
                    .map(|token| (token, 1))
                    .collect(),
            )
        };
        Ok(Some(
            row.get_or_try_init::<EngineError>(|| Ok(Some(every())))?
                .map_or(
                    crate::model::NbestSuccessors::Miss,
                    crate::model::NbestSuccessors::Row,
                ),
        ))
    }

    fn nbest_token_costs(
        &self,
        _token: &PhraseToken,
    ) -> Result<crate::model::NbestTokenCosts, EngineError> {
        Ok(crate::model::NbestTokenCosts {
            elem: Some(self.unigram),
            lambda: (self.blended - self.unigram) as f32,
            unigram_lambda: 1.0,
        })
    }
}

fn trellis_session() -> Session<FixtureDictionary, TrellisModel> {
    Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(TRAIN_VOCAB).expect("authored fixture"),
        TrellisModel {
            blended: 0.5,
            unigram: 0.25,
        },
    )
}

/// Types `text` and selects the candidate carrying `token`.
fn type_and_select_over<M>(session: &mut Session<FixtureDictionary, M>, text: &str, token: u32)
where
    M: LanguageModel,
    M::Error: std::fmt::Display,
{
    let mut typed = session.raw_input().to_owned();
    for character in text.chars() {
        typed.push(character);
        session.replace_raw(&typed).expect("typing cannot fail");
    }
    let index = session
        .candidates()
        .iter()
        .position(|candidate| candidate.token() == Some(token))
        .expect("the fixture candidate is offered");
    select_row(session, index).expect("selection cannot fail");
}

// Basis: pinyin.cpp:1362–1425,1497–1525,2515–2520 and phonetic_lookup.cpp:120–205 at 074a2219.
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

// Basis: pinyin.cpp:1362–1425,1497–1525,2515–2520 and phonetic_lookup.cpp:120–205 at 074a2219.
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
        .filter_map(|text| text.as_str())
        .collect();
    assert_eq!(
        rows,
        ["\u{4f60}", "\u{4f60}"],
        "the forced phrase is the row — twice: the bigram and unigram branch \
         lineages of the same token, the shape the oracle's terminal-choose \
         rows show (the candidate window dedups them)"
    );
}

// Basis: pinyin.cpp:2643–2668 and phonetic_lookup.cpp:88–117 at 074a2219.
#[test]
fn clear_constraint_unforces_the_run_and_rebuilds_the_record() {
    let mut session = train_session();
    type_and_select(&mut session, "nihao", 1);
    type_and_select(&mut session, "hao", 2);
    assert_eq!(session.selected_tokens(), [1, 2]);

    assert!(!session.clear_constraint(999));

    assert!(session.clear_constraint(1));
    assert_eq!(session.selected_tokens(), [2]);
    assert!(!session.clear_constraint(0), "the head run is already free");

    assert!(session.clear_constraint(2));
    assert!(session.selected_tokens().is_empty());
    assert!(!session.clear_constraint(0));
}

// Basis: Rust session reconciliation and selection record contracts.
#[test]
fn clearing_a_tail_forcing_reopens_a_committed_composition() {
    let mut session = trellis_session();
    session
        .replace_raw("nihaoshijie")
        .expect("typing cannot fail");
    let select_token = |session: &mut Session<FixtureDictionary, TrellisModel>, token| {
        let index = session
            .candidates()
            .iter()
            .position(|candidate| candidate.token() == Some(token))
            .expect("the fixture candidate is offered");
        select_row(session, index).expect("selection cannot fail");
    };
    select_token(&mut session, 1); // 你 over [0,2)
    select_token(&mut session, 2); // 好 over [2,5)
    let index = session
        .candidates()
        .iter()
        .position(|candidate| candidate.kind() == crate::engine::CandidateKind::Fallback)
        .expect("the fallback covers the remainder");
    select_row(&mut session, index).expect("selection cannot fail");

    assert!(session.clear_constraint(2));
    assert_eq!(session.selected_tokens(), [1]);

    session
        .replace_raw("nihaoshijies")
        .expect("typing cannot fail");
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "the walk runs"
    );
    assert!(session.sentence_text(0).is_none());
    assert_eq!(
        session.selected_tokens(),
        [1],
        "the surviving head forcing outlived the clearing and the typing"
    );
}

// Basis: Rust session reconciliation and selection record contracts.
#[test]
fn a_shrinking_reparse_reconciles_a_constraint_free_row_selection() {
    let mut session = trellis_session();
    session.replace_raw("nihao").expect("typing cannot fail");
    assert!(session.guess_sentence().expect("guess cannot fail"));
    let row0 = session
        .candidates()
        .iter()
        .position(|candidate| {
            candidate.kind() == crate::engine::CandidateKind::Sentence
                && candidate.nbest_row() == Some(0)
        })
        .expect("the row-0 sentence is offered");
    assert_eq!(
        select_row(&mut session, row0).expect("selection cannot fail"),
        Selection::Completed
    );
    assert!(
        !session.constraints.is_active(),
        "the row-0 choose wrote no forcing"
    );

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

    assert_eq!(session.raw_input(), "ni");
}

// Basis: Rust session reconciliation and selection record contracts.
#[test]
fn a_divergent_replacement_reconciles_the_committed_selection() {
    let mut session = trellis_session();
    session.replace_raw("nihao").expect("typing cannot fail");
    for token in [1_u32, 2] {
        let index = session
            .candidates()
            .iter()
            .position(|candidate| candidate.token() == Some(token))
            .expect("the fixture candidate is offered");
        select_row(&mut session, index).expect("selection cannot fail");
    }

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

    assert_eq!(session.raw_input(), "mixxo");
}

// Basis: pinyin.cpp:1362–1425,1497–1525,2515–2520 and phonetic_lookup.cpp:120–205 at 074a2219.
#[test]
fn temporary_reparses_keep_constraints_until_a_guess_validates_them() {
    for intermediate in ["mihao", "n", "", "ni"] {
        for validate in [false, true] {
            let mut session = trellis_session();
            session.replace_raw("nihao").expect("initial parse");
            let index = session
                .candidates()
                .iter()
                .position(|candidate| candidate.token() == Some(1))
                .expect("the fixture offers 你");
            select_row(&mut session, index).expect("choose 你");
            session.replace_raw(intermediate).expect("temporary parse");
            assert!(session.constraints.is_one_step_at(0));
            if validate {
                session.guess_sentence().expect("intermediate guess");
            }
            session.replace_raw("nihao").expect("restore input");
            assert!(session.guess_sentence().expect("restored guess"));
            assert_eq!(
                session.sentence_text(0).and_then(|text| text.as_str()),
                Some("你好")
            );
            assert_eq!(
                session.constraints.is_one_step_at(0),
                !validate || intermediate == "ni",
                "constraint after {intermediate:?}, intervening guess={validate}",
            );
        }
    }
}

// Basis: pinyin.cpp:1362–1425,1497–1525,2515–2520 and phonetic_lookup.cpp:120–205 at 074a2219.
#[test]
fn the_forcing_survives_the_shrinking_parse_and_retype() {
    let mut session = trellis_session();
    session
        .replace_raw("nihaoshijie")
        .expect("typing cannot fail");
    let index = session
        .candidates()
        .iter()
        .position(|candidate| candidate.token() == Some(1))
        .expect("the fixture candidate is offered");
    select_row(&mut session, index).expect("selection cannot fail");

    for end in [10, 2] {
        session
            .replace_raw(&"nihaoshijie"[..end])
            .expect("shrink cannot fail");
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

    for end in [3, 11] {
        session
            .replace_raw(&"nihaoshijie"[..end])
            .expect("retype cannot fail");
    }
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "the retype walks"
    );
    assert!(session.sentence_text(0).is_none());
    assert_eq!(
        session.selected_tokens(),
        [1],
        "the forcing survived the shrink and the retype"
    );

    session.reset();
    assert!(!session.clear_constraint(0));
}

// Basis: pinyin.cpp:1362–1425,1497–1525,2515–2520 and phonetic_lookup.cpp:120–205 at 074a2219.
#[test]
fn a_shorter_reparse_keeps_the_guessed_store_for_a_differing_row_write() {
    let mut session = train_session();
    session.replace_raw("nihao").expect("typing cannot fail");
    session.guess_sentence().expect("guess");
    session.sentence.rows = vec![
        crate::engine::nbest::NbestRow {
            text: Some("\u{597d}".into()),
            spans: vec![crate::engine::constraint::PhraseSpan {
                start: 0,
                token: 0x100,
                phrase_length: 1,
            }],
            keys: 1,
            span: 3,
        },
        crate::engine::nbest::NbestRow {
            text: Some("\u{6d69}".into()),
            spans: vec![crate::engine::constraint::PhraseSpan {
                start: 0,
                token: 0x102,
                phrase_length: 1,
            }],
            keys: 1,
            span: 3,
        },
    ];
    session.refresh().expect("refresh cannot fail");
    let index = session
        .candidates()
        .iter()
        .position(|candidate| candidate.nbest_row() == Some(1))
        .expect("the rank-1 row is offered");
    let chosen = session.candidates().iter().nth(index).expect("row").clone();
    session.replace_raw("ni").expect("shorter parse");
    session
        .select_target(&SelectionTarget::of_candidate(&chosen, 0, None))
        .expect("the retained row is choosable");
    assert!(matches!(
        session.constraints.cell(0),
        Some(crate::engine::constraint::Cell::OneStep { end: 3, .. })
    ));
    assert!(
        session.clear_constraint(0),
        "the differing phrase's write lands after a shorter reparse"
    );
}

// Basis: phonetic_lookup.h:847–932 and phonetic_key_matrix.cpp:603–664 at 074a2219.
#[test]
fn a_row_zero_choose_trains_nothing_even_with_recorded_tokens() {
    let mut session = trellis_session();
    session.replace_raw("nihao").expect("typing cannot fail");
    assert!(
        session.guess_sentence().expect("guess cannot fail"),
        "rows exist before the choose"
    );
    let row = session
        .candidates()
        .iter()
        .position(|candidate| candidate.nbest_row() == Some(0))
        .expect("the rank-0 row is offered at the head");
    select_row(&mut session, row).expect("the row is choosable");
    assert!(
        !session.clear_constraint(0),
        "a row-0 choose recorded no forcing"
    );
    let row_tokens: Vec<PhraseToken> = session.selected_tokens().to_vec();
    assert_eq!(row_tokens, vec![1, 2], "the row's whole path is the record");

    let mut recorder = Recorder {
        observed: Vec::new(),
    };
    assert!(train_prepared(&session, 0, &mut recorder).expect("train cannot fail"));
    assert!(
        recorder.observed.is_empty(),
        "a result without OneStep trains nothing"
    );
}

// Basis: phonetic_lookup.h:847–932 and phonetic_key_matrix.cpp:603–664 at 074a2219.
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
    assert!(train_prepared(&session, 0, &mut recorder).expect("train cannot fail"));
    assert_eq!(
        recorder.observed,
        vec![(Vec::new(), 1), (vec![1], 2),],
        "你 (forced) then 好 (first decoded after the run) train, 你→好 included"
    );
}

// Basis: Rust raw-buffer and lookup offset UTF-8 boundary contracts.
#[test]
fn replace_raw_walks_consumed_back_to_a_char_boundary() {
    let mut session = session();
    session.replace_raw("a").expect("cannot fail");
    select_row(&mut session, 0).expect("the fallback row selects");
    session.replace_raw("\u{ff0c}").expect("cannot fail");
    assert_eq!(session.composition_offset(), 0);
}

// Basis: Rust raw-buffer and lookup offset UTF-8 boundary contracts.
#[test]
fn candidates_at_rejects_mid_character_offsets() {
    let mut session = session();
    session.replace_raw("\u{ff0c}nihao").expect("cannot fail");
    for offset in [1, 2] {
        assert!(
            matches!(session.candidates_at(offset), Err(EngineError::Failure)),
            "offset {offset} is inside the character and must be refused"
        );
    }
    assert!(session.candidates_at(3).is_ok(), "offset 3 is a boundary");
}

// Basis: pinyin.cpp:2184–2308 at 074a2219 and the Rust fallback contract.
#[test]
fn candidates_at_mid_syllable_keeps_the_prepended_nbest_rows() {
    use super::CandidateKind;
    use crate::engine::nbest::NbestRow;

    let mut session = train_session();
    session
        .replace_raw("nihaoshijie")
        .expect("typing cannot fail");
    assert_eq!(
        session
            .candidates_at(3)
            .expect("unsearched empty column")
            .iter()
            .map(|row| row.kind())
            .collect::<Vec<_>>(),
        [CandidateKind::Fallback]
    );
    assert!(
        session
            .candidates_at(2)
            .expect("live column")
            .iter()
            .any(|row| row.kind() == CandidateKind::Phrase)
    );
    session.sentence.rows = vec![NbestRow {
        text: Some("你好世界".into()),
        spans: Vec::new(),
        keys: 3,
        span: 11,
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

// Basis: pinyin.cpp:2004–2007 and zhuyin.cpp:1272–1291,1327–1330,1394–1438 at 074a2219.
#[test]
fn the_zhuyin_display_law_prepends_only_the_one_best_row() {
    use super::CandidateKind;
    use crate::engine::nbest::NbestRow;

    let row = |text: &str| NbestRow {
        text: Some(text.into()),
        spans: Vec::new(),
        keys: 2,
        span: 5,
    };

    let mut session = train_session();
    session
        .replace_raw("nihaoshijie")
        .expect("typing cannot fail");
    session.sentence.rows = vec![row("你好世界"), row("你")];

    session.refresh().expect("refresh cannot fail");
    let sentences: Vec<&str> = session
        .candidates()
        .iter()
        .filter(|cand| cand.kind() == CandidateKind::Sentence)
        .filter_map(super::super::candidate::Candidate::text_str)
        .collect();
    assert_eq!(sentences, ["你好世界", "你"]);
    assert!(
        !session
            .candidates()
            .iter()
            .any(|cand| cand.kind() == CandidateKind::Phrase && cand.text() == "你"),
        "the phrase colliding with the second row's own text is absorbed"
    );

    session.set_law(crate::engine::Law::Zhuyin);
    session.refresh().expect("refresh cannot fail");
    let sentences: Vec<&str> = session
        .candidates()
        .iter()
        .filter(|cand| cand.kind() == CandidateKind::Sentence)
        .filter_map(super::super::candidate::Candidate::text_str)
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

// Basis: zhuyin.cpp:1460–1668 at 074a2219.
#[test]
fn choosing_a_before_cursor_row_constrains_its_own_span() {
    use super::BEFORE_CURSOR_ANCHOR;
    use super::CandidateKind;

    let mut session = train_session();
    session.replace_raw("nihao").expect("typing cannot fail");
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
        .select_target(&SelectionTarget::of_candidate(
            window.iter().nth(index).expect("the row is in the window"),
            BEFORE_CURSOR_ANCHOR,
            None,
        ))
        .expect("the row is selectable");
    assert_eq!(session.composition_offset(), 5);
    let runs = session.constraints.runs();
    assert_eq!(runs.len(), 1, "one forcing: {runs:?}");
    assert_eq!(
        runs[0],
        (2, 5, row.token().expect("token row")),
        "the constraint is the row's own span, not [0, offset)"
    );
    assert_eq!(session.selected_tokens(), [row.token().expect("token row")]);
}

// Basis: zhuyin.cpp:1460–1668 at 074a2219.
#[test]
fn candidates_ending_at_walks_the_spans_that_end_there() {
    use super::CandidateKind;
    use crate::engine::nbest::NbestRow;

    let mut session = train_session();
    session.replace_raw("nihao").expect("typing cannot fail");

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

    let at_start = session
        .candidates_ending_at(0)
        .expect("offset 0 is in range");
    assert!(at_start.is_empty());

    session.sentence.rows = vec![NbestRow {
        text: Some("你好".into()),
        spans: Vec::new(),
        keys: 2,
        span: 5,
    }];
    session.set_law(crate::engine::Law::Zhuyin);
    let window = session
        .candidates_ending_at(2)
        .expect("offset 2 is in range");
    assert_eq!(
        window
            .iter()
            .next()
            .map(|cand| (cand.kind(), cand.text_str())),
        Some((CandidateKind::Sentence, Some("你好"))),
        "the collapsed sentence row heads the before-cursor window"
    );
    assert!(window.iter().any(|cand| cand.text() == "你"));
}

// Basis: pinyin.cpp:2184–2308 at 074a2219 and the Rust fallback contract.
#[test]
fn candidates_at_the_apostrophe_column_is_transparent() {
    use super::CandidateKind;

    let mut session = train_session();
    session.replace_raw("ni'hao").expect("typing cannot fail");
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

    session.replace_raw("ni,'hao").expect("parse");
    let outside = session.candidates_at(3).expect("byte 3 is in range");
    assert_eq!(
        outside.iter().map(|row| row.kind()).collect::<Vec<_>>(),
        [CandidateKind::Fallback]
    );
}

// Basis: phonetic_key_matrix.cpp:30–84,87–318 at 074a2219 and the Rust scan-matrix contract.
#[test]
fn candidates_at_a_divided_split_column_answers_the_split_window() {
    use crate::OptionBits;

    const SPLIT_VOCAB: &str = "token=1\tkeys=ni\ttext=你\tunigram=1000\n\
                               token=2\tkeys=hao\ttext=好\tunigram=900\n\
                               token=3\tkeys=e\ttext=恶\tunigram=800\n";
    for (options, expected) in [
        (
            OptionBits::PINYIN_INCOMPLETE | OptionBits::USE_DIVIDED_TABLE,
            Some("恶"),
        ),
        (OptionBits::PINYIN_INCOMPLETE, None),
    ] {
        let mut session = Session::new(
            SessionSettings::default(),
            FixtureDictionary::parse(SPLIT_VOCAB).expect("authored fixture"),
            FixtureLanguageModel::parse(SPLIT_VOCAB, "").expect("authored fixture"),
        );
        session.set_options(options).expect("set split options");
        session.replace_raw("nihaoshijie").expect("parse");
        assert_eq!(session.spans_a_matrix_key(10), expected.is_some());
        let window = session.candidates_at(10).expect("byte 10 is in range");
        assert_eq!(
            window
                .iter()
                .find(|row| row.kind() != super::CandidateKind::Fallback)
                .and_then(|row| row.text_str()),
            expected
        );
    }
}

// Basis: Rust selection record contract and pinyin.cpp:2501–2588 at 074a2219.
#[test]
fn choosing_behind_the_composition_moves_the_record_back() {
    use super::CandidateKind;

    const VOCAB: &str = "token=1\tkeys=ni\ttext=你\tunigram=1000\n\
                         token=2\tkeys=hao\ttext=好\tunigram=900\n\
                         token=3\tkeys=ni,hao\ttext=你好\tunigram=800\n";
    let mut session = Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(VOCAB).expect("authored fixture"),
        FixtureLanguageModel::parse(VOCAB, "").expect("authored fixture"),
    );
    session.replace_raw("nihao").expect("typing cannot fail");
    let whole = session
        .candidates()
        .iter()
        .position(|cand| cand.kind() == CandidateKind::Phrase && cand.text() == "你好")
        .expect("你好 is offered at 0");
    select_row(&mut session, whole).expect("你好 is selectable");
    assert_eq!(session.composition_offset(), 5);

    let window = session.candidates_at(0).expect("offset 0 is in range");
    let (index, row) = window
        .iter()
        .enumerate()
        .find(|(_, cand)| cand.kind() == CandidateKind::Phrase && cand.text() == "你")
        .expect("你 is offered at 0");
    session
        .select_target(&SelectionTarget::of_candidate(
            window.iter().nth(index).expect("the row is in the window"),
            0,
            None,
        ))
        .expect("a choose behind the composition is a selection, not a refusal");
    assert_eq!(session.composition_offset(), 2, "the record moves back");
    let runs = session.constraints.runs();
    assert_eq!(runs.len(), 1, "你好 was overlapped and cleared: {runs:?}");
    assert_eq!(runs[0], (0, 2, row.token().expect("token row")));
    assert_eq!(session.selected_tokens(), [row.token().expect("token row")]);
    assert_eq!(&session.raw_input()[session.composition_offset()..], "hao");
}

/// One phrase stored as `ni3'hao3`:
/// `lookup` is toneless; `trellis_records` and the candidate search apply
/// the pin's tone match (`pinyin_compare_tone3`, `pinyin_phrase3.h:97-105`).
struct StoredTone3;

impl PhraseList for StoredTone3 {
    type Error = EngineError;
    type Syllable = SyllableKey;

    fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
        let texts: Vec<&str> = syllables.iter().map(|key| key.text()).collect();
        Ok(if texts == ["ni", "hao"] {
            vec![PhraseEntry::new(0x0f00_0001, "你鎄".to_owned())]
        } else {
            Vec::new()
        })
    }

    fn trellis_records(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
    ) -> Result<Vec<PhraseEntry>, EngineError> {
        let matches_stored = tones.iter().all(|tone| matches!(*tone, 0 | 3));
        if matches_stored {
            self.lookup(syllables)
        } else {
            Ok(Vec::new())
        }
    }

    fn candidate_search_into(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        out: &mut Vec<PhraseEntry>,
        _row: &mut Vec<u8>,
    ) -> Result<(crate::model::UnreadableRows, bool), EngineError> {
        *out = self.trellis_records(syllables, tones)?;
        Ok((crate::model::UnreadableRows::default(), true))
    }
}

// Basis: pinyin_phrase3.h:97–142 at 074a2219 and the Rust candidate search tone seam.
#[test]
fn typed_tones_keep_only_user_phrases_with_a_matching_stored_tone() {
    let texts = |input: &str| -> Vec<String> {
        let mut session = Session::new(
            SessionSettings::default(),
            StoredTone3,
            FixedUnigrams {
                system: 14,
                addon: 7,
                total: 51_051_831,
                addon_total: 25_525_916,
            },
        );
        session
            .set_options(crate::OptionBits::from_bits_retain(0x1aa))
            .expect("options apply");
        session.replace_raw(input).expect("typing cannot fail");
        session
            .candidates()
            .iter()
            .filter_map(|candidate| candidate.text_str())
            .map(str::to_owned)
            .collect()
    };
    for (raw, offered) in [("ni3hao3", true), ("nihao", true), ("ni2hao3", false)] {
        assert_eq!(texts(raw).contains(&"你鎄".to_owned()), offered, "{raw}");
    }
}

// Basis: Rust session parse intake contract and pinyin.cpp:1497–1525 at 074a2219.
#[test]
fn parsed_full_pinyin_retains_incomplete_keys_without_enabling_incomplete_input() {
    use crate::graph::ExactSegment;
    let mut session = session();
    session
        .set_options(crate::OptionBits::USE_TONE)
        .expect("options");
    let sh = SyllableKey::from_canonical_text("sh").expect("initial");
    session
        .replace_raw_full_pinyin("sh", &[ExactSegment::new(0, 2, sh, 0)])
        .expect("parsed key");
    assert_eq!(session.full_parsed_len(), 2);
    assert!(session.spans_a_matrix_key(0));
    session.replace_raw("sh").expect("ordinary input");
    assert_eq!(session.full_parsed_len(), 0);
}

// Basis: pinyin.cpp:2924,2987–3095 and phonetic_key_matrix.cpp:30–84 at 074a2219.
#[test]
fn retained_full_pinyin_cursor_keys_match_the_pin_spans() {
    use crate::graph::ExactSegment;
    let mut session = session();
    session
        .set_options(crate::OptionBits::USE_TONE)
        .expect("options");
    let segments: Vec<_> = [(0, 2, "li"), (3, 5, "sh"), (6, 8, "ba"), (9, 12, "kua")]
        .into_iter()
        .map(|(start, end, text)| {
            ExactSegment::new(
                start,
                end,
                SyllableKey::from_canonical_text(text).expect("key"),
                0,
            )
        })
        .collect();
    session
        .replace_raw_full_pinyin("li'sh'ba'kua", &segments)
        .expect("intake");
    let keys: Vec<_> = session.matrix_keys().iter().collect();
    let parsed = session.input.parsed.retained.matrix.bound;
    assert_eq!(parsed, 12);
    assert_eq!(
        keys.iter()
            .map(|k| (k.key().text(), k.syllable_start(), k.end()))
            .collect::<Vec<_>>(),
        vec![("li", 0, 2), ("sh", 3, 5), ("ba", 6, 8), ("kua", 9, 12)]
    );
    assert_eq!(session.right_word_offset(2).expect("right"), Some(5));
    assert_eq!(session.left_word_offset(5).expect("left"), 2);
    assert_eq!(session.lookup_offset_for_cursor(4).expect("lookup"), 2);
}

// Basis: phonetic_key_matrix.cpp:30–84,87–318 at 074a2219 and the Rust scan-matrix contract.
#[test]
fn retained_matrix_keeps_leading_gaps_empty_and_follows_the_last_parse() {
    let mut session = session();
    session.replace_raw("'ni''hao'").expect("parsed input");
    let matrix = &session.input.parsed.retained.matrix;
    assert!(matrix.get(0).expect("a placed column").is_empty());
    assert!(!matrix.zeros[0], "leading gap cannot carry a zero-key hop");
    assert!(
        !matrix.get(1).expect("a placed column").is_empty(),
        "ni remains live at its own begin"
    );
    assert!(matrix.zeros[3] && matrix.zeros[4], "internal zero-key run");
    assert!(
        !matrix.get(5).expect("a placed column").is_empty(),
        "hao retains its raw begin"
    );
    assert!(
        matrix.zeros[8] && matrix.zeros[9],
        "tail and reserved column"
    );

    session.replace_raw("nihao").expect("replacement");
    let matrix = &session.input.parsed.retained.matrix;
    assert!(
        !matrix.get(0).expect("a placed column").is_empty(),
        "old leading gap must disappear"
    );
    assert!(
        !matrix.zeros[3],
        "old separator must not survive replacement"
    );
}

// Basis: phonetic_key_matrix.cpp:30–84,87–318 at 074a2219 and the Rust scan-matrix contract.
#[test]
fn the_collapse_mode_reaches_the_split_windows_at_the_next_parse() {
    use crate::OptionBits;

    const HALF_VOCAB: &str = "token=1\tkeys=an\ttext=安\tunigram=1000\n\
                              token=2\tkeys=ang\ttext=昂\tunigram=900\n";
    for (raw, offset, wanted, end) in [("banangang", 6, "昂", 9), ("lianai", 2, "安", 4)] {
        let mut session = Session::new(
            SessionSettings::default(),
            FixtureDictionary::parse(HALF_VOCAB).expect("fixture"),
            FixedUnigrams {
                system: 1000,
                addon: 0,
                total: 10000,
                addon_total: 1,
            },
        );
        session
            .set_options(OptionBits::from_bits_retain(0x18a))
            .expect("options");
        session.set_law(crate::engine::Law::Zhuyin);
        session.replace_raw(raw).expect("full pinyin parse");
        assert!(
            !session
                .candidates_at(offset)
                .expect("zhuyin window")
                .iter()
                .any(|row| row.text() == wanted),
            "zhuyin's full-pinyin matrix omits the split passes"
        );

        session.set_law(crate::engine::Law::Pinyin);
        assert!(
            !session
                .candidates_at(offset)
                .expect("unparsed window")
                .iter()
                .any(|row| row.text() == wanted),
            "the mode alone does not re-parse"
        );
        session.replace_raw(raw).expect("pinyin parse");
        let rows = session.candidates_at(offset).expect("pinyin window");
        let row = rows.iter().find(|row| row.text() == wanted).expect(wanted);
        assert_eq!(row.consumed_bytes(), end - offset);
        assert_eq!(session.raw_input(), raw);

        session.set_law(crate::engine::Law::Zhuyin);
        session.replace_raw(raw).expect("zhuyin parse");
        assert!(
            !session
                .candidates_at(offset)
                .expect("rebuilt zhuyin window")
                .iter()
                .any(|row| row.text() == wanted),
            "switching back must remove the pinyin split topology"
        );
    }
}

// Basis: phonetic_key_matrix.cpp:30–84,87–318 at 074a2219 and the Rust scan-matrix contract.
#[test]
fn collapse_mode_selects_the_one_matrix_every_reader_projects() {
    use crate::OptionBits;

    let mut session = session();
    session
        .set_options(OptionBits::from_bits_retain(0x18a))
        .expect("options");
    let spans = |session: &Session<Silent, Silent>| {
        let keys: Vec<_> = session.matrix_keys().iter().collect();
        let parsed = session.input.parsed.retained.matrix.bound;
        (
            keys.iter()
                .map(|key| (key.syllable_start(), key.end()))
                .collect::<Vec<_>>(),
            parsed,
        )
    };
    session.replace_raw("xian").expect("parse");
    let (pinyin_keys, parsed) = spans(&session);
    assert_eq!(parsed, 4);
    assert!(pinyin_keys.contains(&(2, 4)), "the divided `an` is a key");
    assert!(session.spans_a_matrix_key(2));

    session.set_law(crate::engine::Law::Zhuyin);
    session.replace_raw("xian").expect("zhuyin parse");
    let (zhuyin_keys, parsed) = spans(&session);
    assert_eq!(parsed, 4);
    assert_eq!(
        zhuyin_keys,
        [(0, 4)],
        "no divided pair in the zhuyin matrix"
    );
    assert!(!session.spans_a_matrix_key(2));
    assert_eq!(session.full_parsed_len(), 4);

    session.set_law(crate::engine::Law::Pinyin);
    session.replace_raw("xian").expect("pinyin parse");
    assert_eq!(spans(&session).0, pinyin_keys, "pinyin topology returns");
    assert!(session.spans_a_matrix_key(2));
}

// Basis: phonetic_key_matrix.cpp:30–84,87–318 at 074a2219 and the Rust scan-matrix contract.
#[test]
fn retained_exact_key_windows_search_the_reserved_end_slot() {
    use crate::graph::ExactSegment;

    const EXACT_VOCAB: &str = "token=1\tkeys=ni\ttext=你\tunigram=1000\n\
                               token=2\tkeys=hao\ttext=好\tunigram=900\n";

    let mut session = Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(EXACT_VOCAB).expect("fixture"),
        FixedUnigrams {
            system: 1000,
            addon: 0,
            total: 10000,
            addon_total: 1,
        },
    );
    let segments = [
        ExactSegment::new(
            0,
            2,
            crate::vocab::SyllableKey::from_text("ni").expect("ni"),
            0,
        ),
        ExactSegment::new(
            2,
            4,
            crate::vocab::SyllableKey::from_text("hao").expect("hao"),
            0,
        ),
    ];
    session.replace_raw_exact("nihk", &segments).expect("parse");
    assert!(session.input.parsed.retained.matrix.zeros[4]);
    let rows = session.candidates_at(2).expect("hao window");
    assert!(rows.iter().any(|row| row.text() == "好"));
}

// Basis: Divergence register row 83 and the Rust float-to-u32 conversion contract.
#[test]
fn candidate_scores_follow_rusts_saturating_conversion() {
    for (unigram, total, lambda, expected) in [
        (7, 3, 0.5, 19_573_418),
        (10, 2, 1.5, 0),
        (10, 2, f32::NAN, 0),
        (1_000, 1, 0.0, u32::MAX),
        (10, 0, 0.5, 0),
    ] {
        assert_eq!(super::amplified_frequency(unigram, total, lambda), expected);
    }
}

// Basis: pinyin.cpp:1678–1709,2128–2138 at 074a2219 and the Rust rank-key alignment contract.
#[test]
fn candidate_rank_respects_every_sort_word_and_disabled_dimension() {
    use std::cmp::Ordering;
    for word in [0, 4, 8, 16, 28] {
        let base = super::RankKey::for_sort_word(2, 4, 7, word);
        for (length, span, frequency, bit) in [(3, 4, 7, 4), (2, 5, 7, 8), (2, 4, 8, 16)] {
            let higher = super::RankKey::for_sort_word(length, span, frequency, word);
            assert_eq!(
                higher.compare_descending(&base),
                if word & bit == 0 {
                    Ordering::Equal
                } else {
                    Ordering::Less
                },
                "sort word {word:#x}, dimension {bit:#x}"
            );
        }
        assert_eq!(base.m_freq, 7, "dedup always keeps the original frequency");
        let mut other = base;
        other.m_freq = u32::MAX;
        assert!(
            base.compare_descending(&other).is_eq(),
            "m_freq is never a sort key"
        );
    }
}

// Basis: pinyin.cpp:1678–1709,2128–2138 at 074a2219 and the Rust rank-key alignment contract.
#[test]
fn candidate_rank_sort_keeps_equal_keys_and_aligns_dedup_frequencies() {
    use crate::engine::candidate::{Candidate, CandidateKind};
    let mut rows: Vec<_> = ["first", "long", "second", "short"]
        .into_iter()
        .map(|text| Candidate::new(text, CandidateKind::Phrase, 1, 1, None, None))
        .collect();
    let mut keys: Vec<_> = [(2, 7), (3, 5), (2, 9), (1, 11)]
        .into_iter()
        .enumerate()
        .map(|(i, (length, frequency))| {
            (
                super::RankKey::for_sort_word(length, 1, frequency, 4),
                i as u32,
            )
        })
        .collect();
    super::rank_candidates(&mut rows, &mut keys);
    assert_eq!(
        rows.iter()
            .map(|r| r.text_str().unwrap())
            .collect::<Vec<_>>(),
        ["long", "first", "second", "short"]
    );
    assert_eq!(
        keys.iter().map(|(k, _)| k.m_freq).collect::<Vec<_>>(),
        [5, 7, 9, 11]
    );
}

// Basis: pinyin.cpp:1678–1709,2128–2138 at 074a2219 and the Rust rank-key alignment contract.
#[test]
fn the_pinyin_length_key_wraps_at_eight_bits() {
    let key = |span| super::RankKey::for_sort_word(1, span, 0, super::SORT_BY_PINYIN_LENGTH);
    assert!(key(256).compare_descending(&key(2)).is_gt());
}

// Basis: pinyin.cpp:1678–1709,2128–2138 at 074a2219 and the Rust rank-key alignment contract.
#[test]
fn the_string_dedup_keeps_the_larger_m_freq_and_the_earlier_tie() {
    use crate::engine::candidate::{Candidate, CandidateKind};

    let row = |text: &str, kind, m_freq| {
        let key = super::RankKey {
            phrase_length: 2,
            pinyin_span: 6,
            frequency: 0,
            m_freq,
        };
        (key, Candidate::new(text, kind, 2, 6, None, None))
    };
    let (keys, mut ranked): (Vec<_>, Vec<_>) = [
        row("书学", CandidateKind::Phrase, 3),
        row("数学", CandidateKind::Phrase, 9),
        row("书学", CandidateKind::Addon, 7),
        row("数学", CandidateKind::Addon, 9),
        row("书学", CandidateKind::Phrase, 7),
    ]
    .into_iter()
    .unzip();
    let keys: Vec<_> = keys.into_iter().map(|key| (key, 0)).collect();
    super::dedup_ranked_by_text(&mut ranked, &keys);
    let rows: Vec<_> = ranked
        .iter()
        .map(|candidate| {
            (
                candidate.text_str().unwrap_or_default().to_owned(),
                candidate.kind(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("数学".to_owned(), CandidateKind::Phrase),
            ("书学".to_owned(), CandidateKind::Addon),
        ],
        "the addon row beats the lower normal row; equal frequencies keep the earlier row"
    );
}

// Basis: pinyin.cpp:1299–1306 at 074a2219 and the Rust retained-matrix option contract.
#[test]
fn an_option_change_reaches_the_matrix_at_the_next_parse() {
    let mut session = session();
    session.replace_raw("sh").expect("parse");
    assert_eq!(session.full_parsed_len(), 2);
    let keys: Vec<_> = session.matrix_keys().iter().collect();
    let parsed = session.input.parsed.retained.matrix.bound;
    assert_eq!((keys.len(), parsed), (1, 2));

    let options = session.options - crate::OptionBits::PINYIN_INCOMPLETE;
    session.set_options(options).expect("options");
    assert_eq!(session.full_parsed_len(), 2, "no re-parse");
    assert_eq!(session.input.parsed.retained.matrix.bound, 2);

    session.replace_raw("sh").expect("parse under the new word");
    assert_eq!(session.full_parsed_len(), 0);
    assert_eq!(session.matrix_keys().len(), 0);

    session.reset();
    assert_eq!(session.full_parsed_len(), 0);
}

// Basis: pinyin.cpp:2924,2987–3095 and phonetic_key_matrix.cpp:30–84 at 074a2219.
#[test]
fn the_matrix_keys_are_the_parse_in_column_order() {
    use crate::OptionBits;
    use crate::engine::cursor::KeySpans as _;

    let mut session = session();
    session
        .set_options(OptionBits::from_bits_retain(0x18a))
        .expect("options");
    session.replace_raw("ni'hao").expect("parse");
    let spans: Vec<_> = session
        .matrix_keys()
        .iter()
        .map(|key| (key.syllable_start(), key.end()))
        .collect();
    assert_eq!(spans, [(0, 2), (3, 6)]);
    for raw in ["ni''hao", "'xian'", "banangang", "lianai", "zhong'guo'ren"] {
        session.replace_raw(raw).expect("parse");
        let keys = session.matrix_keys();
        let starts: Vec<usize> = keys.iter().map(|key| key.syllable_start()).collect();
        assert!(starts.is_sorted(), "{raw}: {starts:?}");
        for index in 0..keys.len() {
            let key = keys.get(index);
            assert_eq!(keys.span(index), (key.syllable_start(), key.end()), "{raw}");
        }
    }
}

/// A fixture facade that also keeps phrase items, so a forcing is
/// revalidated against stored pronunciations the way the pin does
/// (`phonetic_lookup.cpp:120-165` at 074a2219).
struct ItemDictionary {
    inner: FixtureDictionary,
    items: std::collections::HashMap<u32, Rows>,
    /// Every item read fails, as a backend that cannot serve it would.
    failing: bool,
}

impl PhraseList for ItemDictionary {
    type Error = EngineError;
    type Syllable = SyllableKey;

    fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
        Ok(self
            .inner
            .lookup(syllables)
            .expect("fixture lookups cannot fail"))
    }

    fn read_phrase_item<R>(
        &self,
        token: PhraseToken,
        read: impl FnOnce(crate::data::phrase_library::PhraseItemView<'_>) -> R,
    ) -> Result<Option<R>, EngineError> {
        if self.failing {
            return Err(EngineError::Failure);
        }
        Ok(self.items.get(&token).map(|rows| {
            let bytes = item_bytes(rows);
            read(crate::data::phrase_library::PhraseItemView::from_bytes(&bytes).expect("an item"))
        }))
    }
}

/// A phrase item's readings in stored order: each one's keys, tones
/// included, and its frequency.
type Rows = Vec<(Vec<crate::parser::ChewingKey>, u32)>;

/// The `PhraseItem` bytes `rows` stand for (074a2219 `phrase_index.h:56`):
/// a placeholder text as long as the readings, then each reading's packed
/// keys and frequency.
fn item_bytes(rows: &[(Vec<crate::parser::ChewingKey>, u32)]) -> Vec<u8> {
    let length = rows.first().map_or(0, |(keys, _)| keys.len());
    assert!(
        rows.iter().all(|(keys, _)| keys.len() == length),
        "one reading length"
    );
    let mut bytes = vec![
        u8::try_from(length).expect("a phrase length"),
        u8::try_from(rows.len()).expect("a reading count"),
    ];
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    for _ in 0..length {
        bytes.extend_from_slice(&0x4e00_u32.to_le_bytes());
    }
    for (keys, frequency) in rows {
        for key in keys {
            bytes.extend_from_slice(&key.to_packed().to_le_bytes());
        }
        bytes.extend_from_slice(&frequency.to_le_bytes());
    }
    bytes
}

/// 中华人民 and 种花人民 as one item: `zhong1 hua2 ren2 min2` (frequency 50)
/// and `zhong3 hua1 ren2 min2` (frequency 1).
fn wide_abbreviation_item() -> Rows {
    let reading = |spellings: [(&str, u8); 4]| -> Vec<crate::parser::ChewingKey> {
        spellings
            .into_iter()
            .map(|(spelling, tone)| {
                crate::parser::ChewingKey::from_pinyin(spelling)
                    .expect("a pinyin spelling")
                    .with_tone(tone)
            })
            .collect()
    };
    vec![
        (
            reading([("zhong", 1), ("hua", 2), ("ren", 2), ("min", 2)]),
            50,
        ),
        (
            reading([("zhong", 3), ("hua", 1), ("ren", 2), ("min", 2)]),
            1,
        ),
    ]
}

/// A session over `zhhrm`, five keys of which four are incomplete, with
/// token 7 forced over all of it and the selection record rebuilt from that
/// forcing.
fn wide_abbreviation_session(item: Option<Rows>) -> Session<ItemDictionary, TrellisModel> {
    let mut session = Session::new(
        SessionSettings::default(),
        ItemDictionary {
            inner: FixtureDictionary::parse(TRAIN_VOCAB).expect("authored fixture"),
            items: item.into_iter().map(|rows| (7, rows)).collect(),
            failing: false,
        },
        TrellisModel {
            blended: 0.5,
            unigram: 0.25,
        },
    );
    session
        .set_options(session.options() | crate::OptionBits::PINYIN_INCOMPLETE)
        .expect("incomplete on");
    session.replace_raw("zhhrm").expect("parse");
    session.constraints.resize(6);
    assert_eq!(session.constraints.add(0, 5, 7), 5);
    session.rebuild_selection_from_constraints();
    session
}

// Basis: phonetic_lookup.cpp:142–163 and phonetic_key_matrix.cpp:534–600 at 074a2219.
#[test]
fn a_forcing_over_a_wide_abbreviation_survives_the_next_guess() {
    for replacing in [false, true] {
        let mut session = wide_abbreviation_session(Some(wide_abbreviation_item()));
        if replacing {
            session.reconcile_replaced_selection().expect("reconcile");
        } else {
            assert!(session.guess_sentence().expect("guess"));
        }
        assert!(session.constraints.is_one_step_at(0));
        assert_eq!(session.selected_tokens(), [7]);
    }
}

// Basis: phonetic_lookup.cpp:142–163 and phonetic_key_matrix.cpp:534–600 at 074a2219.
#[test]
fn a_forcing_the_stored_pronunciations_do_not_spell_is_dropped() {
    let chao = |tone| {
        crate::parser::ChewingKey::from_pinyin("chao")
            .expect("chao")
            .with_tone(tone)
    };
    let mismatched = vec![(vec![chao(0); 4], 9_u32)];
    for item in [Some(mismatched), None] {
        let mut session = wide_abbreviation_session(item.clone());
        session.guess_sentence().expect("guess cannot fail");
        assert!(!session.constraints.is_one_step_at(0));
        let mut session = wide_abbreviation_session(item);
        session.reconcile_replaced_selection().expect("reconcile");
        assert!(session.selected_tokens().is_empty());
    }
}

// Basis: phonetic_lookup.cpp:142–163 and phonetic_key_matrix.cpp:534–600 at 074a2219.
#[test]
fn an_overrunning_forcing_whose_item_read_fails_is_cleared() {
    for fits in [false, true] {
        let mut session = wide_abbreviation_session(Some(wide_abbreviation_item()));
        session.dictionary.failing = true;
        if !fits {
            session.replace_raw("zh").expect("shorten");
        }
        assert!(session.constraints.is_one_step_at(0));
        let guess = session.guess_sentence();
        assert_eq!(guess.is_err(), fits);
        if !fits {
            assert!(!session.constraints.is_one_step_at(0));
        }
    }
}

/// Chooses row `index` of the session's cached list at the composition
/// offset, through [`Session::select_target`].
pub(super) fn select_row<D, L>(
    session: &mut Session<D, L>,
    index: usize,
) -> Result<Selection, EngineError>
where
    D: crate::model::Dictionary<Syllable = crate::vocab::SyllableKey>,
    L: crate::model::LanguageModel,
{
    session.refresh_candidates()?;
    let candidate = session
        .candidates()
        .iter()
        .nth(index)
        .cloned()
        .expect("the row is listed");
    let anchor = session.composition_offset();
    session.select_target(&SelectionTarget::of_candidate(&candidate, anchor, None))
}

// Basis: pinyin.cpp:1870–1933,2292–2300 at 074a2219.
#[test]
fn longer_unigram_ties_follow_library_order_before_matrix_path_order() {
    for (first_token, first_frequency, expected) in [
        (33554433, 100, "西安好"),
        (33554433, 101, "先好"),
        (16777218, 100, "先好"),
    ] {
        let vocab = format!(
            "token={first_token}\tkeys=xian,hao\ttext=先好\tunigram={first_frequency}\n\
             token=16777217\tkeys=xi,an,hao\ttext=西安好\tunigram=100\n"
        );
        let mut session = Session::new(
            SessionSettings::default(),
            FixtureDictionary::parse(&vocab).expect("fixture"),
            FrequencyFixtureModel::parse(&vocab, "").expect("fixture"),
        );
        session.replace_raw("xian").expect("parse");
        let (_, candidate) = session
            .longer_candidate(super::Scan::EagerRefresh)
            .expect("lookup")
            .expect("row");
        assert_eq!(candidate.text(), expected);
    }
}

// Basis: phonetic_key_matrix.cpp:30–84,87–318 at 074a2219 and the Rust scan-matrix contract.
#[test]
fn matrix_appends_keep_multiplicity_and_column_wide_fuzzy_order() {
    use crate::OptionBits;
    use crate::parser::ChewingKey;
    let options =
        OptionBits::PINYIN_AMB_L_N | OptionBits::PINYIN_AMB_L_R | OptionBits::PINYIN_AMB_AN_ANG;
    let lan = SyllableKey::from_text("lan").unwrap();
    let key = ScanKey {
        key: lan,
        from: 0,
        to: 3,
        syllable_start: 0,
        crosses_separator: false,
        tone: 2,
    };
    let mut scan = vec![vec![key, key], vec![], vec![], vec![]];
    super::extend_matrix(&mut scan, options, false);
    let expected = [
        "lan", "lan", "ran", "nan", "ran", "nan", "lang", "lang", "rang", "nang", "rang", "nang",
    ];
    assert_eq!(
        scan[0].iter().map(|k| k.key.text()).collect::<Vec<_>>(),
        expected
    );
    assert!(scan[0].iter().all(|k| k.tone == 2));
    let keys = [crate::engine::cursor::MatrixKey::new(lan, 2, 0, 3); 2];
    let parsed = super::parse_time_keys(&keys, 3, options, false);
    assert_eq!(
        parsed.iter().map(|k| k.key().text()).collect::<Vec<_>>(),
        expected
    );
    assert!(parsed.iter().all(|k| k.tone() == 2));
    let placed = super::matrix::ParsedMatrix::from_scan(
        &super::matrix::ScanMatrix::from_columns(scan),
        3,
        true,
    );
    assert_eq!(
        placed.get(0).expect("a placed column").len(),
        expected.len()
    );
    assert!(placed.get(3).expect("a placed column").is_empty() && placed.zeros[3]);
    assert!(!placed.zeros[0]);
    assert_eq!(
        parsed[0].key(),
        ChewingKey::from_pinyin("lan").unwrap().with_tone(2)
    );
}

// Basis: phonetic_key_matrix.cpp:30–84,87–318 at 074a2219 and the Rust scan-matrix contract.
#[test]
fn a_packed_fuzzy_final_without_a_syllable_row_stays_in_both_matrices() {
    use crate::OptionBits;
    use crate::parser::ChewingKey;
    let shun = SyllableKey::from_text("shun").unwrap();
    let options = OptionBits::PINYIN_AMB_EN_ENG;
    let graph = SegmentGraph::build_exact(b"shun", &[ExactSegment::new(0, 4, shun, 3)]).unwrap();
    let scan = super::build_scan_matrix(&graph, options, false);
    let alternate = ChewingKey {
        final_: 10,
        ..shun.chewing_key().with_tone(3)
    };
    assert!(SyllableKey::from_chewing_key(alternate).is_none());
    assert_eq!(scan[0].len(), 2);
    assert_eq!(
        scan[0][1].key.chewing_key().with_tone(scan[0][1].tone),
        alternate
    );
    let keys = super::parse_time_keys(
        &[crate::engine::cursor::MatrixKey::new(shun, 3, 0, 4)],
        4,
        options,
        false,
    );
    assert_eq!(keys[1].key(), alternate);
    assert_eq!((keys[0].end(), keys[1].end()), (4, 4));
    assert_eq!(
        training_readings(
            &super::matrix::ParsedMatrix::from_scan(
                &super::matrix::ScanMatrix::from_columns(scan),
                4,
                true
            ),
            b"shun",
            0,
            4,
            1,
            &mut super::selection::ReadingStacks::default()
        )
        .collect_all(),
        [vec![shun.chewing_key().with_tone(3)], vec![alternate]]
    );
}

// Basis: phonetic_lookup.cpp:142–163 and phonetic_key_matrix.cpp:534–600 at 074a2219.
#[test]
fn constraint_spelling_sums_duplicate_paths_at_the_f32_boundary() {
    use crate::parser::ChewingKey;
    let ni = SyllableKey::from_text("ni").unwrap();
    let edge = ScanKey {
        key: ni,
        from: 0,
        to: 2,
        syllable_start: 0,
        crosses_separator: false,
        tone: 0,
    };
    for (paths, total, kept) in [
        (1, 1 << 24, false),
        (2, 1 << 24, true),
        (3, 1 << 24, true),
        (1, 1 << 23, true),
        (1, (1 << 23) + 1, false),
    ] {
        let scan = vec![vec![edge; paths], vec![], vec![]];
        let matrix = super::matrix::ParsedMatrix::from_scan(
            &super::matrix::ScanMatrix::from_columns(scan),
            2,
            true,
        );
        let dictionary = ItemDictionary {
            inner: FixtureDictionary::parse(TRAIN_VOCAB).unwrap(),
            items: [(
                1,
                vec![
                    (vec![ni.chewing_key()], 1),
                    (vec![ChewingKey::from_pinyin("hao").unwrap()], total - 1),
                ],
            )]
            .into(),
            failing: false,
        };
        let spelling = crate::engine::nbest::ConstraintSpelling::new(&matrix);
        let mut constraints = crate::engine::constraint::ConstraintStore::default();
        constraints.resize(3);
        constraints.add(0, 2, 1);
        constraints
            .validate(3, |start, end, token| {
                spelling.spells(&dictionary, start, end, token)
            })
            .unwrap();
        assert_eq!(
            constraints.is_one_step_at(0),
            kept,
            "{paths} paths / {total}"
        );
        assert!(!spelling.spells(&dictionary, 0, 2, 2).unwrap());
    }
}

/// Appends a copy of the first key to its column in the retained parse: the
/// matrix holds one path twice. The scan matrix and the placed topology
/// read the same records, as the pin's one matrix does.
fn duplicate_the_first_key<D, L>(session: &mut Session<D, L>)
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    let physical = session.input.physical_separators();
    let parse = &mut session.input.parsed.retained;
    let mut columns = parse
        .scan
        .iter()
        .map(<[ScanKey]>::to_vec)
        .collect::<Vec<_>>();
    let key = columns[0][0];
    columns[0].push(key);
    parse.scan = super::matrix::ScanMatrix::from_columns(columns);
    parse.matrix =
        super::matrix::ParsedMatrix::from_scan(&parse.scan, parse.matrix.bound, physical);
}

// Basis: Rust candidate window batching and reuse contracts.
#[test]
fn duplicated_matrix_paths_leave_ordinary_rows_unchanged() {
    let mut session = trellis_session();
    session.replace_raw("ni").unwrap();
    session.guess_sentence().unwrap();
    let ordinary = |session: &mut Session<FixtureDictionary, TrellisModel>| {
        session
            .candidates_at(0)
            .unwrap()
            .iter()
            .filter(|c| c.nbest_row().is_none())
            .cloned()
            .collect::<Vec<_>>()
    };
    let rows = ordinary(&mut session);
    duplicate_the_first_key(&mut session);
    session.guess_sentence().unwrap();
    assert_eq!(ordinary(&mut session), rows);
}

pub(super) mod session_lifecycle {
    //! Behavioural ports of the upstream session-lifecycle suites.
    //!
    //! - `libchewing/tests/test-reset.c`: `Reset` clears session state but
    //!   must not drop loaded data or configuration — the next conversion
    //!   after a reset behaves like the first.
    //! - `libchewing/tests/test-keyboardless.c` (selection subset): the
    //!   programmatic choice surface — out-of-range choices fail without
    //!   disturbing state, choosing without a candidate window fails, and a
    //!   mid-composition choice commits the exact chosen phrase. These
    //!   selection ports are in the `session_lifecycle` module of
    //!   `selection/tests.rs`.
    //!
    //! The oxpinyin seam is the internal engine `Session` API over the
    //! `oxpinyin/tests/common/fixture_doubles.rs` doubles. Chewing's commit/preedit
    //! buffer checks use selected text and raw-input offsets; its config setters
    //! map onto `set_options`/`set_incomplete_pinyin`/`set_scoring_config`.

    use crate::OptionBits;

    use crate::engine::{Session, SessionSettings};
    use crate::fixture_doubles::{FixtureDictionary, FrequencyFixtureModel};

    use super::select_row;

    pub(in crate::engine::session) const VOCAB: &str =
        include_str!("../../../fixtures/w4/mini-vocab.txt");

    pub(in crate::engine::session) const BIGRAM: &str =
        include_str!("../../../fixtures/w4/mini-bigram.txt");

    pub(in crate::engine::session) type TestSession =
        Session<FixtureDictionary, FrequencyFixtureModel>;

    /// The committed oracle fixtures as one session: the builder that the
    /// lookup, selection, lifecycle and replay tests share.
    pub(in crate::engine::session) fn session() -> TestSession {
        let dictionary = FixtureDictionary::parse(VOCAB).expect("committed fixture");
        let model = FrequencyFixtureModel::parse(VOCAB, BIGRAM).expect("committed fixtures");
        Session::new(SessionSettings::default(), dictionary, model)
    }

    // ── libchewing test-reset.c ─────────────────────────────────────────

    // Basis: Rust session reset and deterministic list contracts.
    #[test]
    fn reset_clears_the_composition_but_not_the_configuration() {
        let mut session = session();
        assert!(!session.is_composing());
        session
            .set_options(session.options() - OptionBits::PINYIN_INCOMPLETE)
            .expect("the option toggles");
        let options = session.options();

        session.replace_raw("nihao").expect("typing");
        assert!(session.is_composing());
        select_row(&mut session, 0).expect("choose before reset");

        session.reset();
        assert!(!session.is_composing());
        assert!(session.selected_tokens().is_empty());

        assert_eq!(session.options(), options);
        session.replace_raw("n").expect("typing");
        assert_eq!(session.full_parsed_len(), 0);
        let mut fresh_session = self::session();
        fresh_session.set_options(options).expect("match options");
        fresh_session.replace_raw("zhong,guo").expect("typing");
        session
            .replace_raw("zhong,guo")
            .expect("typing after reset");
        assert_eq!(session.candidates(), fresh_session.candidates());
    }

    // Basis: Rust option-word and raw-input contracts in the session API.
    #[test]
    fn an_option_word_with_undefined_bits_is_accepted_and_inert() {
        let undefined_bit = 1 << 0; // bits 0..2 are outside the frozen set
        let mut session = session();
        session
            .set_options(
                OptionBits::PINYIN_INCOMPLETE | OptionBits::from_bits_retain(undefined_bit),
            )
            .expect("the option word accepts undefined bits");
        session
            .set_options(session.options() | OptionBits::PINYIN_INCOMPLETE)
            .expect("the option toggles");
        session.replace_raw("n").expect("typing");
        assert!(
            session.is_composing(),
            "incomplete pinyin still composes past an initial"
        );
    }
}

/// The F-D replay fixture session and the session-level replay tests; the
/// scenarios themselves are in `selection/tests.rs`.
pub(super) mod session_replay {
    use crate::model::PhraseList;
    use crate::vocab::SyllableKey;

    use crate::engine::Session;
    use crate::fixture_doubles::{FixtureDictionary, FrequencyFixtureModel};

    use super::session_lifecycle::VOCAB;

    type Fixtures = Session<FixtureDictionary, FrequencyFixtureModel>;

    pub(in crate::engine::session) fn session() -> Fixtures {
        let dictionary = FixtureDictionary::parse(VOCAB).expect("committed fixture");

        // The scenarios name phrases the vocabulary has to hold — `f-e-12` expects
        // `传` for `zhuan`. Core's fixture-provenance tests check the vocabulary against the
        // captures but knows nothing about these scenarios, so a key dropped from
        // the fixture would otherwise surface as a mystifying candidate mismatch
        // several steps into a replay. Fail here instead, naming the key.
        for keys in ["zhuan", "ni", "ni,hao", "xi,an", "chang,an", "zhong,guo"] {
            let sequence: Vec<SyllableKey> = keys
                .split(',')
                .map(|key| SyllableKey::from_text(key).expect("a frozen pinyin key"))
                .collect();
            assert!(
                !dictionary
                    .lookup(&sequence)
                    .expect("fixture lookups cannot fail")
                    .is_empty(),
                "fixtures/w4/mini-vocab.txt has no entry for {keys:?}, which a \
             replay scenario depends on"
            );
        }

        super::session_lifecycle::session()
    }

    // Basis: Rust option-word and raw-input contracts in the session API.
    #[test]
    fn arbitrary_characters_never_panic() {
        let mut session = session();
        let mut text = String::new();
        for character in ('\u{0}'..='\u{ff}').chain(['\u{4f60}', '\u{10ffff}']) {
            text.push(character);
            session.replace_raw(&text).expect("no input is an error");
        }
        assert!(session.composition_offset() <= session.raw_input().len());
        session.reset();
        assert!(session.raw_input().is_empty());
        assert!(session.selected_tokens().is_empty());
    }
}

/// Decoding tests whose decision is the session's parse and settings.
mod decoding {
    use crate::engine::{Candidate, Session, SessionSettings};
    use crate::fixture_doubles::{FixtureDictionary, FrequencyFixtureModel};

    use super::session_lifecycle::{BIGRAM, VOCAB};

    /// Authored mini-vocab so hard-stop-at-`b` and full `bing` can be told apart.
    ///
    /// Incomplete `b` expands to complete keys (`bu`, `bing`, …). `不` rides on
    /// `bu` (cheaper unigram) and `并` on `bing`. Fixture justification: the full
    /// oracle capture had `b#ing\t1\t不` and `bing\t1\t并`; neither input is in
    /// `fixtures/w4/oracle-candidate-structure.txt`.
    const JUNK_VOCAB: &str = "\
token=1\tkeys=bu\ttext=不\tunigram=1000
token=2\tkeys=bing\ttext=并\tunigram=100
";

    // Basis: pinyin_parser2.cpp:237–328 at 074a2219 and the Rust raw-buffer contract.
    #[test]
    fn replace_raw_b_hash_ing_prefers_bu_family_not_bing() {
        let mut session = Session::new(
            SessionSettings::default(),
            FixtureDictionary::parse(JUNK_VOCAB).expect("junk vocab"),
            FrequencyFixtureModel::parse(JUNK_VOCAB, "").expect("junk unigrams"),
        );

        session.replace_raw("b#ing").expect("parse cannot fail");
        assert_eq!(session.raw_input(), "b#ing");

        let top = session
            .candidates()
            .iter()
            .next()
            .map(Candidate::text)
            .expect("at least one candidate");
        assert_eq!(
            top, "不",
            "fixture: b#ing rank-1 is 不, not 并; got {top:?}"
        );

        let mut collapsed = Session::new(
            SessionSettings::default(),
            FixtureDictionary::parse(JUNK_VOCAB).expect("junk vocab"),
            FrequencyFixtureModel::parse(JUNK_VOCAB, "").expect("junk unigrams"),
        );
        collapsed.replace_raw("bing").expect("parse cannot fail");
        let collapsed_top = collapsed
            .candidates()
            .iter()
            .next()
            .map(Candidate::text)
            .expect("at least one candidate");
        assert_eq!(collapsed_top, "并", "clean bing must still rank 并 first");
    }

    // Basis: pinyin_parser2.cpp:237–328 at 074a2219 and the Rust raw-buffer contract.
    #[test]
    fn turning_incomplete_pinyin_off_stops_at_the_initial() {
        let mut session = Session::new(
            SessionSettings {
                incomplete_pinyin: false,
            },
            FixtureDictionary::parse(VOCAB).expect("committed fixture"),
            FrequencyFixtureModel::parse(VOCAB, BIGRAM).expect("committed fixtures"),
        );

        session.replace_raw("nih").expect("typing cannot fail");

        let texts: Vec<&str> = session
            .candidates()
            .iter()
            .filter_map(Candidate::text_str)
            .collect();
        assert_eq!(texts[0], "你", "without the incomplete key, nih is just ni");
        assert!(!texts.contains(&"你好"));
    }
}
