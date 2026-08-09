//! Recorded key sequences driven through the session API.
//!
//! `docs/findings/session-replay.md` freezes the scenario format. The
//! scenarios themselves are authored — F-D was never captured — and their
//! expected candidates come from the pin's real lists in
//! `fixtures/foundation/f-a.txt`, restricted to what the mini vocabulary can
//! reach.
//!
//! This is the first cross-platform consumer of the session seam. Nothing in
//! it is Linux-specific: no paths are discovered, no environment is read, no
//! `cfg(target_os)` appears anywhere beneath it.

use pinyin_core::fixture::{FixtureDictionary, FixtureLanguageModel};
use pinyin_core::{Dictionary, SyllableKey};
use pinyin_engine::{
    EmptyConfigSource, EngineError, KeyInput, KeyOutcome, LogicalKey, Session, StoragePaths,
};

const SCENARIOS: &str = include_str!("../../../fixtures/w4/f-d-session.txt");
const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

type Fixtures = Session<FixtureDictionary, FixtureLanguageModel>;

fn session() -> Fixtures {
    let dictionary = FixtureDictionary::parse(VOCAB).expect("committed fixture");

    // The scenarios name phrases the vocabulary has to hold — `f-e-12` expects
    // `传` for `zhuan`. `fixture_provenance` checks the vocabulary against the
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

    Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        dictionary,
        FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixtures"),
    )
    .expect("the fixtures open")
}

/// One scenario: a name and its ordered steps.
struct Scenario {
    name: String,
    steps: Vec<Vec<(String, String)>>,
}

fn scenarios() -> Vec<Scenario> {
    let mut scenarios: Vec<Scenario> = Vec::new();

    for raw in SCENARIOS.lines() {
        let line = raw.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<(String, String)> = line
            .split('\t')
            .filter_map(|field| field.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();

        match fields.first() {
            Some((key, value)) if key == "scenario" => scenarios.push(Scenario {
                name: value.clone(),
                steps: Vec::new(),
            }),
            Some((key, _)) if key == "step" => scenarios
                .last_mut()
                .expect("a step follows its scenario")
                .steps
                .push(fields),
            _ => panic!("unrecognised replay line: {line}"),
        }
    }

    scenarios
}

fn value<'a>(step: &'a [(String, String)], name: &str) -> Option<&'a str> {
    step.iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn candidate_texts(session: &Fixtures) -> Vec<String> {
    session
        .candidates()
        .iter()
        .map(|candidate| candidate.text().to_owned())
        .collect()
}

fn run(scenario: &Scenario) {
    let mut session = session();
    let name = &scenario.name;

    for (index, step) in scenario.steps.iter().enumerate() {
        let kind = value(step, "step").expect("every step names itself");
        let where_ = format!("{name} step {} ({kind})", index + 1);

        match kind {
            "type" => {
                for character in value(step, "text").expect("type needs text").chars() {
                    session
                        .process_key(&KeyInput::character(character))
                        .unwrap_or_else(|error| panic!("{where_}: {error}"));
                }
            }
            "key" => {
                let key = match value(step, "name").expect("key needs a name") {
                    "enter" => LogicalKey::Enter,
                    "escape" => LogicalKey::Escape,
                    "space" => LogicalKey::Space,
                    "backspace" => LogicalKey::Backspace,
                    other => panic!("{where_}: unknown key {other}"),
                };
                session
                    .process_key(&KeyInput::plain(key))
                    .unwrap_or_else(|error| panic!("{where_}: {error}"));
            }
            "select" => {
                let index: usize = value(step, "index")
                    .expect("select needs an index")
                    .parse()
                    .expect("an index");
                session
                    .select(index)
                    .unwrap_or_else(|error| panic!("{where_}: {error}"));
            }
            "select-text" => {
                let wanted = value(step, "text").expect("select-text needs text");
                let position = candidate_texts(&session)
                    .iter()
                    .position(|text| text == wanted)
                    .unwrap_or_else(|| {
                        panic!(
                            "{where_}: {wanted} is not offered: {:?}",
                            candidate_texts(&session)
                        )
                    });
                session
                    .select(position)
                    .unwrap_or_else(|error| panic!("{where_}: {error}"));
            }
            "expect-preedit" => {
                let wanted = value(step, "text").expect("expect-preedit needs text");
                assert_eq!(session.preedit().text(), wanted, "{where_}");
                let preedit = session.preedit();
                let mut covered = 0;
                for span in preedit.spans() {
                    assert_eq!(span.start(), covered, "{where_}: spans are contiguous");
                    covered = span.end();
                }
                assert_eq!(
                    covered,
                    preedit.text().len(),
                    "{where_}: spans cover the text"
                );
            }
            "expect-candidate" => {
                let index: usize = value(step, "index")
                    .expect("expect-candidate needs an index")
                    .parse()
                    .expect("an index");
                let wanted = value(step, "text").expect("expect-candidate needs text");
                let offered = candidate_texts(&session);
                assert_eq!(
                    offered.get(index).map(String::as_str),
                    Some(wanted),
                    "{where_}: offered {offered:?}"
                );
            }
            "expect-absent" => {
                let wanted = value(step, "text").expect("expect-absent needs text");
                assert!(
                    !candidate_texts(&session).iter().any(|text| text == wanted),
                    "{where_}: {wanted} should not be offered"
                );
            }
            "expect-empty" => {
                assert!(!session.is_composing(), "{where_}");
                assert!(session.preedit().is_empty(), "{where_}");
            }
            "commit" => {
                let wanted = value(step, "text").expect("commit needs text");
                let committed = session
                    .commit()
                    .unwrap_or_else(|error| panic!("{where_}: {error}"));
                assert_eq!(committed, wanted, "{where_}");
            }
            other => panic!("{where_}: unknown step {other}"),
        }
    }
}

#[test]
fn the_scenario_file_is_readable() {
    let scenarios = scenarios();
    assert!(scenarios.len() >= 8, "saw {} scenarios", scenarios.len());
    assert!(scenarios.iter().all(|scenario| !scenario.steps.is_empty()));
}

#[test]
fn every_recorded_scenario_replays() {
    for scenario in scenarios() {
        run(&scenario);
    }
}

#[test]
fn replaying_twice_gives_the_same_answer() {
    for scenario in scenarios() {
        run(&scenario);
        run(&scenario);
    }
}

/// F-E-02 — candidate-processing invalid access.
///
/// `docs/findings/robustness-evidence.md` registers this as
/// `f_e_02_candidate_replay`: take a candidate snapshot, regenerate the list,
/// then replay every index from the stale snapshot including `len` and
/// `usize::MAX`. Upstream's defect is an unchecked index; here every one is a
/// checked `Err` and the session stays usable.
#[test]
fn f_e_02_candidate_replay() {
    let mut session = session();
    for character in "nihaozhongguo".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    let stale = session.candidates().len();
    assert!(stale > 1, "the snapshot needs several candidates");

    // Regenerate: a shorter composition offers fewer candidates.
    session.reset();
    for character in "ni".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    let live = session.candidates().len();

    for index in [live, live + 1, stale, stale + 1, usize::MAX] {
        assert_eq!(
            session.select(index),
            Err(EngineError::CandidateIndexOutOfRange { index, len: live }),
            "index {index}"
        );
    }

    // Still usable for a second request.
    assert_eq!(session.candidates().len(), live);
    assert!(session.select(0).is_ok());
    assert!(!session.commit().expect("committing").is_empty());
}

/// F-E-12 — the `zhuan` assertion, at session level.
///
/// `robustness-evidence.md` asks for `f_e_12_zhuan_session_replay`: the full
/// frontend-equivalent sequence rather than a parse in isolation. Parse it,
/// take a candidate, commit, and keep going.
#[test]
fn f_e_12_zhuan_session_replay() {
    let mut session = session();
    for character in "zhuan".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    assert_eq!(
        session.candidates().get(0).map(|c| c.text()),
        Some("传"),
        "the pin's first candidate for zhuan"
    );

    session.select(0).expect("the index is live");
    assert_eq!(session.commit().expect("committing"), "传");

    // A second request on the same session, which is what the upstream
    // assertion path never reached.
    for character in "zhuan".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    assert_eq!(session.raw_input(), "zhuan");
    assert!(!session.candidates().is_empty());
}

/// Every byte sequence a shell could send survives the session.
#[test]
fn arbitrary_characters_never_panic() {
    let mut session = session();
    for character in ('\u{0}'..='\u{ff}').chain(['\u{4f60}', '\u{10ffff}']) {
        session
            .process_key(&KeyInput::character(character))
            .expect("no input is an error");
    }
    for key in [
        LogicalKey::Backspace,
        LogicalKey::Delete,
        LogicalKey::Enter,
        LogicalKey::Escape,
        LogicalKey::Space,
        LogicalKey::Tab,
        LogicalKey::Unknown,
    ] {
        session
            .process_key(&KeyInput::plain(key))
            .expect("no key is an error");
    }
    assert!(matches!(
        session
            .process_key(&KeyInput::plain(LogicalKey::Enter))
            .expect("no failure"),
        KeyOutcome::Ignored | KeyOutcome::Commit(_)
    ));
}
