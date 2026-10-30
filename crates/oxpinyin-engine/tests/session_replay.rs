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
use oxpinyin_engine::{EmptyConfigSource, EngineError, Selection, Session};
use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

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
        dictionary,
        FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixtures"),
    )
    .expect("the fixtures open")
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
        for character in input.chars() {
            session
                .type_pinyin(character.encode_utf8(&mut [0; 4]))
                .expect("type");
        }
        assert_eq!(session.raw_input(), input);
        assert_eq!(session.candidates().get(0).unwrap().text(), first);
        if input == "xian" {
            assert_eq!(session.candidates().get(1).unwrap().text(), "西岸");
        }
        if input == "qqq" {
            assert!(
                session
                    .candidates()
                    .iter()
                    .all(|candidate| candidate.text() != "你好")
            );
        }
        assert_eq!(session.select(0).expect("select"), Selection::Completed);
        assert_eq!(session.selected_text(), first);
        assert_eq!(session.composition_offset(), input.len());
        session.reset();
        assert!(!session.is_composing());
        assert!(session.selected_text().is_empty());
    }

    let mut session = session();
    for character in "nihaozhongguo".chars() {
        session
            .type_pinyin(character.encode_utf8(&mut [0; 4]))
            .expect("type");
    }
    assert_eq!(
        session.select(0).expect("select prefix"),
        Selection::Continued
    );
    assert_eq!(session.selected_text(), "你好");
    assert_eq!(
        &session.raw_input()[session.composition_offset()..],
        "zhongguo"
    );
    assert_eq!(session.candidates().get(0).unwrap().text(), "中国");
    assert_eq!(
        session.select(0).expect("select tail"),
        Selection::Completed
    );
    assert_eq!(session.selected_text(), "你好中国");
    session.erase().expect("undo selection");
    assert_eq!(session.composition_offset(), 0);
    assert!(session.selected_text().is_empty());
    assert_eq!(session.raw_input(), "nihaozhongguo");
    session.reset();
    assert!(!session.is_composing());
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
            .type_pinyin(character.encode_utf8(&mut [0; 4]))
            .expect("typing cannot fail");
    }
    let stale = session.candidates().len();
    assert!(stale > 1, "the snapshot needs several candidates");

    // Regenerate: a shorter composition offers fewer candidates.
    session.reset();
    for character in "ni".chars() {
        session
            .type_pinyin(character.encode_utf8(&mut [0; 4]))
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
    assert!(!session.selected_text().is_empty());
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
    for character in "zhuan".chars() {
        session
            .type_pinyin(character.encode_utf8(&mut [0; 4]))
            .expect("typing cannot fail");
    }
    assert_eq!(
        session
            .candidates()
            .get(0)
            .and_then(oxpinyin_engine::Candidate::text_str),
        Some("传"),
        "the pin's first candidate for zhuan"
    );

    session.select(0).expect("the index is live");
    assert_eq!(session.selected_text(), "传");
    session.reset();

    // A second request on the same session, which is what the upstream
    // assertion path never reached.
    for character in "zhuan".chars() {
        session
            .type_pinyin(character.encode_utf8(&mut [0; 4]))
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
            .type_pinyin(character.encode_utf8(&mut [0; 4]))
            .expect("no input is an error");
    }
    session.erase().expect("erase after arbitrary input");
    assert!(session.composition_offset() <= session.raw_input().len());
    session.reset();
    assert!(session.raw_input().is_empty());
    assert!(session.selected_text().is_empty());
}
