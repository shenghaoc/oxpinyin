use crate::engine::{CandidateKind, Session};
use crate::fixture_doubles::{FixtureDictionary, FrequencyFixtureModel};

use crate::engine::session::tests::session_lifecycle::session;

type Fixtures = Session<FixtureDictionary, FrequencyFixtureModel>;

fn typed(text: &str) -> Fixtures {
    let mut session = session();
    session.replace_raw(text).expect("typing cannot fail");
    session
}

fn texts(session: &Fixtures) -> Vec<String> {
    session
        .candidates()
        .iter()
        .filter_map(|candidate| candidate.text_str())
        .map(str::to_owned)
        .collect()
}

// Basis: pinyin.cpp:2184–2308 at 074a2219 and frozen F-A candidate lists.
#[test]
fn a_three_key_phrase_leads_its_own_prefixes() {
    let session = typed("zhongguoren");
    let texts = texts(&session);
    assert_eq!(&texts[..2], ["中国人", "中国"]);
    for wanted in ["中", "种", "重"] {
        assert!(texts.contains(&wanted.to_owned()), "missing {wanted}");
    }
}

// Basis: pinyin.cpp:2184–2308 at 074a2219 and frozen F-A candidate lists.
#[test]
fn candidates_come_from_every_segmentation_not_only_the_selected_one() {
    let session = typed("xian");
    let offered = texts(&session);
    assert_eq!(&offered[..2], ["西安", "西岸"]);
    for wanted in ["锡安", "县"] {
        assert!(
            offered.contains(&wanted.to_owned()),
            "missing {wanted} in {offered:?}"
        );
    }
}

// Basis: pinyin.cpp:2184–2308 at 074a2219 and frozen F-A candidate lists.
#[test]
fn an_initial_only_tail_reaches_the_phrases_it_prefixes() {
    let session = typed("nih");
    let offered = texts(&session);
    assert_eq!(offered[0], "你好");
    for wanted in ["霓虹", "拟合", "泥孩", "你还", "你和", "你很", "你会"] {
        assert!(offered.contains(&wanted.to_owned()), "missing {wanted}");
    }
}

// Basis: pinyin.cpp:2184–2308 at 074a2219 and frozen F-A candidate lists.
#[test]
fn an_apostrophe_is_a_boundary_the_decoder_honours() {
    let session = typed("xi'an");
    assert_eq!(&texts(&session)[..3], ["西安", "西岸", "锡安"]);
}

// Basis: Rust fallback contract in docs/findings/session-api.md.
#[test]
fn an_input_no_phrase_can_reach_falls_back_to_itself() {
    let input = "qqq";
    let session = typed(input);
    let candidates = session.candidates();
    assert_eq!(candidates.len(), 1, "{input}: {:?}", texts(&session));
    let only = candidates.iter().next().expect("the fallback");
    assert_eq!(only.kind(), CandidateKind::Fallback);
    assert_eq!(only.text(), input);
    assert_eq!(only.consumed_bytes(), input.len());
}

// Basis: Rust Candidate span contract and pinyin.cpp:2224–2262 at 074a2219.
#[test]
fn every_candidate_reports_a_span_it_could_really_absorb() {
    let input = "nihaozhongguo";
    let session = typed(input);
    for candidate in session.candidates() {
        assert!(
            candidate.consumed_bytes() <= input.len(),
            "{input}: {} claims {} bytes",
            candidate.text(),
            candidate.consumed_bytes()
        );
        assert!(candidate.consumed_bytes() > 0);
        assert!(!candidate.text().is_empty());
    }
}
