//! Authored state transitions driven through direct session operations.
//!
//! The historical F-D scenarios were authored, never captured; their
//! expected candidates come from the pin's real lists in
//! `fixtures/foundation/f-a.txt`, restricted to what the mini vocabulary can
//! reach.
//!
//! This is the first cross-platform consumer of the session seam. Nothing in
//! it is Linux-specific: no paths are discovered, no environment is read, no
//! `cfg(target_os)` appears anywhere beneath it.

use oxpinyin_core::{Dictionary, SyllableKey};
use oxpinyin_engine::{EngineError, Selection, SelectionTarget, Session, SessionSettings};

#[path = "../../oxpinyin-core/tests/common/fixture_doubles.rs"]
mod fixture_doubles;

use fixture_doubles::{FixtureDictionary, FrequencyFixtureModel};

const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

type Fixtures = Session<FixtureDictionary, FrequencyFixtureModel>;

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
        SessionSettings::default(),
        dictionary,
        FrequencyFixtureModel::parse(VOCAB, BIGRAM).expect("committed fixtures"),
    )
}

/// Candidate and state transitions from the authored F-D scenarios. The
/// retired key/preedit layer is not needed to exercise these operations.
#[test]
fn recorded_candidate_scenarios_use_direct_operations() {
    for (input, first) in [
        ("nihao", "你好"),
        ("xian", "西安"),
        ("nih", "你好"),
        ("chang'an", "长安"),
        ("zhuan", "传"),
        ("qqq", "qqq"),
    ] {
        let mut session = session();
        session.replace_raw(input).expect("type");
        assert_eq!(session.raw_input(), input);
        assert_eq!(session.candidates().iter().next().unwrap().text(), first);
        if input == "xian" {
            assert_eq!(session.candidates().iter().nth(1).unwrap().text(), "西岸");
        }
        if input == "qqq" {
            assert!(
                session
                    .candidates()
                    .iter()
                    .all(|candidate| candidate.text() != "你好")
            );
        }
        assert_eq!(
            select_row(&mut session, 0).expect("select"),
            Selection::Completed
        );
        assert_eq!(session.selected_text(), first);
        assert_eq!(session.composition_offset(), input.len());
        session.reset();
        assert!(!session.is_composing());
        assert!(session.selected_text().is_empty());
    }

    let mut session = session();
    session.replace_raw("nihaozhongguo").expect("type");
    assert_eq!(
        select_row(&mut session, 0).expect("select prefix"),
        Selection::Continued
    );
    assert_eq!(session.selected_text(), "你好");
    assert_eq!(
        &session.raw_input()[session.composition_offset()..],
        "zhongguo"
    );
    assert_eq!(session.candidates().iter().next().unwrap().text(), "中国");
    assert_eq!(
        select_row(&mut session, 0).expect("select tail"),
        Selection::Completed
    );
    assert_eq!(session.selected_text(), "你好中国");
    session.reset();
    assert!(!session.is_composing());
}

/// F-E-12 — the `zhuan` assertion, at session level.
///
/// `robustness-evidence.md` asks for `f_e_12_zhuan_session_replay`: the full
/// frontend-equivalent sequence rather than a parse in isolation. Parse it,
/// take a candidate, read the retained text, reset, and keep going.
#[test]
fn f_e_12_zhuan_session_replay() {
    let mut session = session();
    session.replace_raw("zhuan").expect("typing cannot fail");
    assert_eq!(
        session
            .candidates()
            .iter()
            .next()
            .and_then(oxpinyin_engine::Candidate::text_str),
        Some("传"),
        "the pin's first candidate for zhuan"
    );

    select_row(&mut session, 0).expect("the index is live");
    assert_eq!(session.selected_text(), "传");
    session.reset();

    // A second request on the same session, which is what the upstream
    // assertion path never reached.
    session.replace_raw("zhuan").expect("typing cannot fail");
    assert_eq!(session.raw_input(), "zhuan");
    assert!(!session.candidates().is_empty());
}

/// Every byte sequence a shell could send survives the session.
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
    assert!(session.selected_text().is_empty());
}

/// Chooses row `index` of the session's cached list at the composition
/// offset, through [`Session::select_target`].
fn select_row(session: &mut Fixtures, index: usize) -> Result<Selection, EngineError> {
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
