//! The composed decoder, driven through the frozen session API.
//!
//! Parse → graph → k-best → lookup, behind the signatures W4-T0 froze. The
//! expectations here are the pin's own candidate lists from
//! `fixtures/foundation/f-a.txt`, restricted to what the mini vocabulary can
//! reach.

use oxpinyin_engine::{
    Candidate, CandidateKind, EmptyConfigSource, KeyInput, LogicalKey, Selection, Session,
    StoragePaths,
};
use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

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

fn session_from(vocab: &str) -> Fixtures {
    Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        FixtureDictionary::parse(vocab).expect("test vocabulary"),
        FixtureLanguageModel::parse(vocab, "# no bigrams\n").expect("test model"),
    )
    .expect("the test fixtures open")
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
fn a_row_choice_after_a_normal_selection_records_the_row_path_only() {
    // Choosing an n-best row replaces `selected` outright (sentence-surface
    // §10: the row text already carries the prefix), so a normal selection
    // made between the sentence lookup and the row choice is rolled back out
    // of the text. The record must follow the text: the row selection
    // restores the history snapshot taken at the lookup before extending
    // with the row's tokens, leaving no stale token from the abandoned
    // normal selection. The control run chooses the row with nothing in
    // between; the interleaved run must land on the same text and record.
    let (text, tokens) = {
        let mut session = typed("nihaozhongguo");
        assert!(session.guess_sentence().expect("the lookup cannot fail"));
        let row = session
            .candidates()
            .iter()
            .position(|candidate| candidate.nbest_row().is_some())
            .expect("the lookup offers a row");
        assert_eq!(
            session.select(row).expect("the row is live"),
            Selection::Completed
        );
        let tokens = session.selected_tokens().to_vec();
        (session.commit().expect("committing"), tokens)
    };
    assert_eq!(text, "你好中国");
    assert!(
        !tokens.is_empty(),
        "the row spells a token path: {tokens:?}"
    );

    let mut session = typed("nihaozhongguo");
    assert!(session.guess_sentence().expect("the lookup cannot fail"));
    let normal = session
        .candidates()
        .iter()
        .position(|candidate| candidate.text() == "你好" && candidate.token().is_some())
        .expect("the phrase 你好 is offered alongside the rows");
    let normal_token = session
        .candidates()
        .get(normal)
        .and_then(Candidate::token)
        .expect("a phrase candidate carries a token");
    assert_eq!(
        session.select(normal).expect("the phrase is live"),
        Selection::Continued
    );
    assert_eq!(
        session.selected_tokens(),
        &[normal_token],
        "the normal selection on its own still records its token"
    );

    let row = session
        .candidates()
        .iter()
        .position(|candidate| candidate.nbest_row().is_some())
        .expect("the rows survive the selection");
    assert_eq!(
        session.select(row).expect("the row is live"),
        Selection::Completed
    );
    let recorded = session.selected_tokens().to_vec();
    assert_eq!(session.commit().expect("committing"), text);
    assert_eq!(
        recorded, tokens,
        "the row choice must replace the record, not extend the stale one"
    );
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
    // `zh` expands to 19 complete syllables after the phonetic-initial fix,
    // so a single initial expands (19 ≤ the default limit of 64) and reaches
    // the phrases it spells — `zhuan` and `zhong` are both in the mini
    // vocabulary, so `zhzh` gets candidates, led by 传. What it cannot get
    // is a *two-key* dictionary phrase: 19 × 19 = 361 sequences,
    // `expand_keys` returns empty rather than a subset, and no phrase is
    // offered for that span.
    let session = typed("zhzh");
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
            "{} is one dictionary phrase spanning two keys, so an \
             over-limit expansion returned a subset",
            candidate.text()
        );
    }

    // The sentence builder is the exception, and legitimately so: it composes
    // two one-key phrases rather than looking up one multi-key phrase, so it
    // spans the whole input without ever expanding two initials at once.
    let sentence = candidates
        .iter()
        .find(|candidate| candidate.kind() == CandidateKind::Sentence)
        .expect("a sentence over the two keys");
    assert_eq!(sentence.consumed_keys(), 2);
    assert_eq!(sentence.text().chars().count(), 2);
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
fn batch_and_key_by_key_agree_on_composing_inputs() {
    // Pure a-z/`'` only: `type_pinyin` and per-key `process_key` must agree
    // when no mid-composition selection intervenes. The parity harness relies
    // on one refresh per input matching that final state.
    for input in ["nihao", "zhongguo", "xian", "chang'an"] {
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
fn batch_accepts_junk_key_by_key_ignores_it() {
    // F1: intentional interactive-vs-batch divergence on junk-bearing inputs.
    // type_pinyin keeps printable junk; process_key ignores non-a-z/`'`.
    let mut batch = session();
    batch
        .type_pinyin("b#ing")
        .expect("batch typing cannot fail");
    assert_eq!(batch.raw_input(), "b#ing");

    let mut keyed = session();
    for character in "b#ing".chars() {
        keyed
            .process_key(&KeyInput::character(character))
            .expect("typing cannot fail");
    }
    assert_eq!(keyed.raw_input(), "bing");
    assert_ne!(
        texts(&batch),
        texts(&keyed),
        "junk must make batch and key-by-key candidate lists diverge"
    );
}

/// Authored mini-vocab so hard-stop-at-`b` and full `bing` can be told apart.
///
/// Incomplete `b` expands to complete keys (`bu`, `bing`, …). `不` rides on
/// `bu` (cheaper unigram) and `并` on `bing`. Fixture justification:
/// `fixtures/w4/oracle-candidates.txt` has `b#ing\t1\t不` and `bing\t1\t并`.
const JUNK_VOCAB: &str = "\
token=1\tkeys=bu\ttext=不\tunigram=1000
token=2\tkeys=bing\ttext=并\tunigram=100
";

#[test]
fn type_pinyin_b_hash_ing_prefers_bu_family_not_bing() {
    // Oracle fixture rank-1 for b#ing is 不 (b-family), not 并 (bing).
    // Re-filtering junk in type_pinyin would collapse to bing and regress.
    let mut session = Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        FixtureDictionary::parse(JUNK_VOCAB).expect("junk vocab"),
        FixtureLanguageModel::parse(JUNK_VOCAB, "").expect("junk unigrams"),
    )
    .expect("session opens");

    session
        .type_pinyin("b#ing")
        .expect("batch typing cannot fail");
    assert_eq!(session.raw_input(), "b#ing");

    let top = session
        .candidates()
        .get(0)
        .map(Candidate::text)
        .expect("at least one candidate");
    assert_eq!(
        top, "不",
        "fixture: b#ing rank-1 is 不, not 并; got {top:?}"
    );

    // Contrast: filtered collapse would type as bing and prefer 并.
    let mut collapsed = Session::new(
        &EmptyConfigSource,
        StoragePaths::new("user"),
        FixtureDictionary::parse(JUNK_VOCAB).expect("junk vocab"),
        FixtureLanguageModel::parse(JUNK_VOCAB, "").expect("junk unigrams"),
    )
    .expect("session opens");
    collapsed
        .type_pinyin("bing")
        .expect("batch typing cannot fail");
    let collapsed_top = collapsed
        .candidates()
        .get(0)
        .map(Candidate::text)
        .expect("at least one candidate");
    assert_eq!(collapsed_top, "并", "clean bing must still rank 并 first");
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
    use oxpinyin_engine::{Config, ConfigLayer, ConfigValue, merge};

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
        oxpinyin_engine::KeyOutcome::Commit("nihao".to_owned())
    );
}

#[test]
fn incomplete_initials_expand_by_phonetic_initial() {
    // A deliberately tiny vocabulary that separates what string-prefix
    // expansion used to conflate.  `n` must reach the N-initial syllable
    // `nei` (and therefore 㐻) but never the zero-initial syllable `ng`;
    // `z`/`c`/`s` must not cross into the retroflex `zh`/`ch`/`sh` initials.
    let vocab = concat!(
        "token=1\tkeys=ng\ttext=嗯\tunigram=100\n",
        "token=2\tkeys=ng\ttext=唔\tunigram=90\n",
        "token=3\tkeys=ng\ttext=唵\tunigram=80\n",
        "token=4\tkeys=ng\ttext=㕶\tunigram=70\n",
        "token=5\tkeys=nei\ttext=㐻\tunigram=100\n",
        "token=6\tkeys=za\ttext=匝\tunigram=100\n",
        "token=7\tkeys=zha\ttext=扎\tunigram=100\n",
        "token=8\tkeys=ca\ttext=擦\tunigram=100\n",
        "token=9\tkeys=cha\ttext=叉\tunigram=100\n",
        "token=10\tkeys=sa\ttext=撒\tunigram=100\n",
        "token=11\tkeys=sha\ttext=沙\tunigram=100\n",
    );

    let session = session_from(vocab);
    let mut n = session.clone();
    n.type_pinyin("n").expect("n types");
    let offered = texts(&n);
    assert!(
        offered.contains(&"㐻".to_owned()),
        "n keeps nei: {offered:?}"
    );
    for ng_only in ["嗯", "唔", "唵", "㕶"] {
        assert!(
            !offered.contains(&ng_only.to_owned()),
            "n must not reach ng-only {ng_only}: {offered:?}"
        );
    }

    for (input, kept, excluded) in [("z", "匝", "扎"), ("c", "擦", "叉"), ("s", "撒", "沙")] {
        let mut session = session.clone();
        session.type_pinyin(input).expect("initial types");
        let offered = texts(&session);
        assert!(
            offered.contains(&kept.to_owned()),
            "{input} must keep {kept}: {offered:?}"
        );
        assert!(
            !offered.contains(&excluded.to_owned()),
            "{input} must exclude {excluded}: {offered:?}"
        );
    }
}
