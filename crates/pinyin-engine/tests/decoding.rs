//! The composed decoder, driven through the frozen session API.
//!
//! Parse → graph → k-best → lookup, behind the signatures W4-T0 froze. The
//! expectations here are the pin's own candidate lists from
//! `fixtures/foundation/f-a.txt`, restricted to what the mini vocabulary can
//! reach.

use pinyin_core::fixture::{FixtureDictionary, FixtureLanguageModel};
use pinyin_engine::{
    Candidate, CandidateKind, EmptyConfigSource, KeyInput, LogicalKey, Selection, Session,
    StoragePaths,
};

const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

type Fixtures = Session<FixtureDictionary, FixtureLanguageModel>;

fn session() -> Fixtures {
    Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        FixtureDictionary::parse(VOCAB).expect("committed fixture"),
        FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixtures"),
    )
    .expect("the fixtures open")
}

fn typed(text: &str) -> Fixtures {
    let mut session = session();
    for character in text.chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    session
}

fn texts(session: &Fixtures) -> Vec<String> {
    session
        .candidates()
        .iter()
        .map(|candidate| candidate.text().to_owned())
        .collect()
}

#[test]
fn a_two_key_phrase_leads_its_own_first_syllable() {
    // Pin: 你好 你 尼 呢 泥 妮 拟 逆 倪 腻
    //
    // Only the lead is asserted. Below it the mini vocabulary's weights take
    // over, and they are derived from captured *rank* rather than frequency:
    // 霓虹 was the pin's second candidate for `nih`, so it carries a high
    // weight here that a real unigram table would never give it. That is a
    // property of the fixture, recorded in fixture-adapters.md, not of the
    // decoder.
    let session = typed("nihao");
    let texts = texts(&session);
    assert_eq!(texts[0], "你好");
    for wanted in ["你", "尼", "呢", "泥", "妮", "拟"] {
        assert!(texts.contains(&wanted.to_owned()), "missing {wanted}");
    }
}

#[test]
fn a_three_key_phrase_leads_its_own_prefixes() {
    // Pin: 中国人 中国 中 种 重 众 钟 忠 终 仲
    let session = typed("zhongguoren");
    let texts = texts(&session);
    assert_eq!(&texts[..2], ["中国人", "中国"]);
    for wanted in ["中", "种", "重"] {
        assert!(texts.contains(&wanted.to_owned()), "missing {wanted}");
    }
}

#[test]
fn candidates_come_from_every_segmentation_not_only_the_selected_one() {
    // Pin: 西安 西岸 锡安 县 见 线 先 现 仙 贤.
    // 西安 is xi + an; the pin's *selected* path for this input is the single
    // key xian. A decoder that kept one segmentation could not offer it.
    //
    // Only the first two ranks are asserted exactly. This test once asserted
    // the exact top-3 `西安 西岸 锡安`; freezing ScoringConfig to seg=750
    // inc=999 bonus=1000 (docs/findings/scoring-constant-sweep.md, the same
    // commit that relaxed this) reordered ranks 3-4, so this decoder now offers
    // `西安 西岸 县 锡安 …` where the pin has `西安 西岸 锡安 县` — the single-key
    // 县 overtakes the two-key 锡安. Rank-3 is scorer-dependent and would
    // re-break on the next sweep; presence of both segmentations is the stable
    // property this test freezes.
    let session = typed("xian");
    let offered = texts(&session);
    assert_eq!(&offered[..2], ["西安", "西岸"]);
    for wanted in ["锡安", "县"] {
        assert!(
            offered.contains(&wanted.to_owned()),
            "missing {wanted} in {offered:?}"
        );
    }

    // Pin: 方案 反感 方 房 放 防 芳 坊 访 仿 — two segmentations interleaved.
    let session = typed("fangan");
    let texts = texts(&session);
    assert_eq!(&texts[..2], ["方案", "反感"]);
    assert!(texts.contains(&"方".to_owned()));
}

#[test]
fn an_initial_only_tail_reaches_the_phrases_it_prefixes() {
    // Pin: 你好 霓虹 拟合 泥孩 你还 你和 你很 你会 你 尼
    let session = typed("nih");
    let offered = texts(&session);
    assert_eq!(offered[0], "你好");
    for wanted in ["霓虹", "拟合", "泥孩", "你还", "你和", "你很", "你会"] {
        assert!(offered.contains(&wanted.to_owned()), "missing {wanted}");
    }

    // Pin: 中国 中共 中古 重工 忠告 中港 终归 中 种 重
    let session = typed("zhongg");
    let texts = texts(&session);
    assert_eq!(texts[0], "中国");
    for wanted in ["中共", "中古", "重工", "忠告", "中港", "终归"] {
        assert!(texts.contains(&wanted.to_owned()), "missing {wanted}");
    }
}

#[test]
fn an_apostrophe_is_a_boundary_the_decoder_honours() {
    // Pin for chang'an: 长安 长 场 常 厂 唱 昌 尝 肠 畅
    let session = typed("chang'an");
    let offered = texts(&session);
    assert_eq!(offered[0], "长安");
    assert!(offered.contains(&"长".to_owned()));

    // Pin for xi'an: 西安 西岸 锡安 西 系 戏 溪 希 喜 细
    let session = typed("xi'an");
    let texts = texts(&session);
    assert_eq!(&texts[..3], ["西安", "西岸", "锡安"]);
}

#[test]
fn a_sentence_spans_more_than_one_dictionary_phrase() {
    let session = typed("nihaozhongguo");
    let texts = texts(&session);
    assert!(
        texts.contains(&"你好中国".to_owned()),
        "the sentence builder must compose across phrases: {texts:?}"
    );

    let sentence = session
        .candidates()
        .iter()
        .find(|candidate| candidate.text() == "你好中国")
        .expect("the sentence is offered");
    assert_eq!(sentence.kind(), CandidateKind::Sentence);
    assert_eq!(sentence.consumed_keys(), 4);
    assert_eq!(sentence.consumed_bytes(), "nihaozhongguo".len());
}

#[test]
fn choosing_advances_the_composition_and_feeds_the_bigram() {
    let mut session = typed("nihaozhongguo");
    let first = session
        .candidates()
        .iter()
        .position(|candidate| candidate.text() == "你好")
        .expect("你好 is offered");

    assert_eq!(
        session.select(first).expect("the index is live"),
        Selection::Continued
    );
    assert_eq!(session.preedit().text(), "你好zhongguo");

    let texts = texts(&session);
    assert_eq!(texts[0], "中国", "the remaining input decodes on its own");

    assert_eq!(
        session.select(0).expect("the index is live"),
        Selection::Completed
    );
    assert_eq!(session.commit().expect("committing"), "你好中国");
    assert!(!session.is_composing());
}

#[test]
fn unknown_input_still_offers_itself_back() {
    let session = typed("qqq");
    let candidates = session.candidates();
    assert!(!candidates.is_empty());
    let fallback = candidates.get(0).expect("one candidate");
    assert_eq!(fallback.kind(), CandidateKind::Fallback);
    assert_eq!(fallback.text(), "qqq");
}

#[test]
fn an_expansion_past_the_limit_offers_no_phrase_for_that_span() {
    // `z` prefixes 36 complete syllables, so a single initial expands (36 ≤ the
    // default limit of 64) and reaches the phrases it spells — `zhuan` and
    // `zhong` are both in the mini vocabulary, so `zzzzzzzz` does get
    // candidates, led by 传. What it cannot get is a *multi-key* phrase: two
    // initials are 36 × 36 = 1,296 sequences, `expand_keys` returns empty
    // rather than a subset, and no phrase is offered for that span.
    let session = typed("zzzzzzzz");
    let candidates = session.candidates();
    assert!(!candidates.is_empty(), "one initial still expands");
    assert_eq!(candidates.get(0).map(Candidate::text), Some("传"));

    for candidate in candidates
        .iter()
        .filter(|candidate| candidate.kind() == CandidateKind::Phrase)
    {
        assert_eq!(
            candidate.consumed_keys(),
            1,
            "{} is one dictionary phrase spanning several keys, so an \
             over-limit expansion returned a subset",
            candidate.text()
        );
    }

    // The sentence builder is the exception, and legitimately so: it composes
    // several one-key phrases rather than looking up one multi-key phrase, so
    // it spans the whole input without ever expanding two initials at once.
    let sentence = candidates
        .iter()
        .find(|candidate| candidate.kind() == CandidateKind::Sentence)
        .expect("a sentence over the eight keys");
    assert_eq!(sentence.consumed_keys(), 8);
    assert_eq!(sentence.text().chars().count(), 8);
}

#[test]
fn an_input_no_expansion_can_reach_falls_back_to_itself() {
    // The other side of the same mechanism, and the path `refresh` takes when
    // scoring yields nothing at all: `q` prefixes only syllables the mini
    // vocabulary does not hold, so the one-key expansion finds no phrase, and
    // every longer prefix is past the limit and returns empty. With no
    // candidate from any span, the session must still offer the raw input
    // rather than an empty list.
    for input in ["q", "qq", "qqq", "qqqq"] {
        let session = typed(input);
        let candidates = session.candidates();
        assert_eq!(candidates.len(), 1, "{input}: {:?}", texts(&session));
        let only = candidates.get(0).expect("the fallback");
        assert_eq!(only.kind(), CandidateKind::Fallback);
        assert_eq!(only.text(), input);
        assert_eq!(only.consumed_bytes(), input.len());
    }
}

#[test]
fn decoding_is_deterministic() {
    for input in ["nihao", "xian", "fangan", "nih", "chang'an", "zhongguoren"] {
        let first: Vec<String> = texts(&typed(input));
        let second: Vec<String> = texts(&typed(input));
        assert_eq!(first, second, "input: {input}");
    }
}

#[test]
fn batch_typing_matches_key_by_key() {
    // `type_pinyin` refreshes once for the whole string; a real shell calls
    // `process_key` per character. With no selection mid-composition the two
    // must produce identical candidate lists — `real_tables_integration` types
    // the corpus in one refresh per input on exactly that promise. The `#` in
    // "b#ing" also checks that a non-syntax key is dropped the same way on both
    // paths (skipped in the batch, ignored key-by-key).
    for input in ["nihao", "zhongguo", "b#ing", "xian", "chang'an"] {
        let batch = {
            let mut session = session();
            session
                .type_pinyin(input)
                .expect("batch typing cannot fail");
            texts(&session)
        };
        let keyed = texts(&typed(input));
        assert_eq!(batch, keyed, "batch vs key-by-key diverged for {input:?}");
    }
}

#[test]
fn every_candidate_reports_a_span_it_could_really_absorb() {
    for input in [
        "nihao",
        "xian",
        "fangan",
        "nih",
        "chang'an",
        "nihaozhongguo",
    ] {
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
}

#[test]
fn turning_incomplete_pinyin_off_stops_at_the_initial() {
    use pinyin_engine::{Config, ConfigLayer, ConfigValue, merge};

    let layer = ConfigLayer::new("user").with("incomplete-pinyin", ConfigValue::Bool(false));
    let config = merge(&Config::default(), &[layer]).expect("well typed");
    let mut session = Session::new(
        &config,
        StoragePaths::new("user"),
        FixtureDictionary::parse(VOCAB).expect("committed fixture"),
        FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixtures"),
    )
    .expect("the fixtures open");

    for character in "nih".chars() {
        session
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }

    let texts: Vec<&str> = session.candidates().iter().map(Candidate::text).collect();
    assert_eq!(texts[0], "你", "without the incomplete key, nih is just ni");
    assert!(!texts.contains(&"你好"));
}

#[test]
fn enter_commits_the_raw_text_when_nothing_is_chosen() {
    let mut session = typed("nihao");
    assert_eq!(
        session
            .process_key(&KeyInput::plain(LogicalKey::Enter))
            .expect("no failure"),
        pinyin_engine::KeyOutcome::Commit("nihao".to_owned())
    );
}
