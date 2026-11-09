//! The composed decoder, driven through the frozen session API.
//!
//! Parse → graph → lookup, behind the signatures W4-T0 froze. The
//! expectations here are the pin's own candidate lists from
//! `fixtures/foundation/f-a.txt`, restricted to what the mini vocabulary can
//! reach.

use std::collections::BTreeSet;

use oxpinyin_engine::{Candidate, CandidateKind, Selection, Session, SessionSettings};

#[path = "../../oxpinyin-core/tests/common/fixture_doubles.rs"]
mod fixture_doubles;

use fixture_doubles::{FixtureDictionary, FrequencyFixtureModel};

const F_A: &str = include_str!("../../../fixtures/foundation/f-a.txt");
const F_C: &str = include_str!("../../../fixtures/foundation/f-c.txt");
const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

/// Depth the capture protocol records candidates to.
const CAPTURE_DEPTH: usize = 10;

type Fixtures = Session<FixtureDictionary, FrequencyFixtureModel>;

fn session() -> Fixtures {
    Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(VOCAB).expect("committed fixture"),
        FrequencyFixtureModel::parse(VOCAB, BIGRAM).expect("committed fixtures"),
    )
}

fn typed(text: &str) -> Fixtures {
    let mut session = session();
    session.replace_raw(text).expect("typing cannot fail");
    session
}

fn session_from(vocab: &str) -> Fixtures {
    Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(vocab).expect("test vocabulary"),
        FrequencyFixtureModel::parse(vocab, "# no bigrams\n").expect("test model"),
    )
}

fn texts(session: &Fixtures) -> Vec<String> {
    session
        .candidates()
        .iter()
        .filter_map(|candidate| candidate.text_str())
        .map(str::to_owned)
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
    // Only the first two ranks are asserted exactly; presence of both
    // segmentations is the stable property this test freezes.
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
    for wanted in ["方案", "反感", "方"] {
        assert!(
            texts.contains(&wanted.to_owned()),
            "missing {wanted} in {texts:?}"
        );
    }
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
    assert_eq!(session.selected_text(), "你好");
    assert_eq!(
        &session.raw_input()[session.composition_offset()..],
        "zhongguo"
    );

    let texts = texts(&session);
    assert_eq!(texts[0], "中国", "the remaining input decodes on its own");

    assert_eq!(
        session.select(0).expect("the index is live"),
        Selection::Completed
    );
    assert_eq!(session.selected_text().clone(), "你好中国");
    session.reset();
    assert!(!session.is_composing());
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
    assert_eq!(candidates.get(0).and_then(Candidate::text_str), Some("传"));

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

/// Authored mini-vocab so hard-stop-at-`b` and full `bing` can be told apart.
///
/// Incomplete `b` expands to complete keys (`bu`, `bing`, …). `不` rides on
/// `bu` (cheaper unigram) and `并` on `bing`. Fixture justification: the full
/// W4 capture (`fixtures/w4/oracle-candidate-structure.txt` before the #1764
/// reduction) had `b#ing\t1\t不` and `bing\t1\t并`; neither input is in the
/// retained subset.
const JUNK_VOCAB: &str = "\
token=1\tkeys=bu\ttext=不\tunigram=1000
token=2\tkeys=bing\ttext=并\tunigram=100
";

#[test]
fn replace_raw_b_hash_ing_prefers_bu_family_not_bing() {
    // Oracle fixture rank-1 for b#ing is 不 (b-family), not 并 (bing):
    // the junk byte reaches the decoder, which stops at it.
    let mut session = Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(JUNK_VOCAB).expect("junk vocab"),
        FrequencyFixtureModel::parse(JUNK_VOCAB, "").expect("junk unigrams"),
    );

    session.replace_raw("b#ing").expect("parse cannot fail");
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

    // Contrast: clean bing prefers 并.
    let mut collapsed = Session::new(
        SessionSettings::default(),
        FixtureDictionary::parse(JUNK_VOCAB).expect("junk vocab"),
        FrequencyFixtureModel::parse(JUNK_VOCAB, "").expect("junk unigrams"),
    );
    collapsed.replace_raw("bing").expect("parse cannot fail");
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
    n.replace_raw("n").expect("n types");
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
        session.replace_raw(input).expect("initial types");
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

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.split('\t')
        .filter_map(|field| field.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// A capture record that carries a candidate list.
struct Candidates {
    input: String,
    theirs: Vec<String>,
}

fn candidate_cases() -> Vec<Candidates> {
    F_A.lines()
        .chain(F_C.lines())
        .filter_map(|line| {
            let input = field(line, "input")?;
            let encoded = field(line, "candidates_hex")?;
            if encoded == "-" || input.contains('\\') {
                return None;
            }
            let theirs = encoded
                .split(',')
                .map(|item| {
                    let bytes: Vec<u8> = (0..item.len() / 2)
                        .map(|index| {
                            u8::from_str_radix(&item[index * 2..index * 2 + 2], 16)
                                .expect("capture hex is valid")
                        })
                        .collect();
                    String::from_utf8(bytes).expect("capture candidates are UTF-8")
                })
                .collect();
            Some(Candidates {
                input: input.to_owned(),
                theirs,
            })
        })
        .collect()
}

#[test]
fn candidate_differential_over_the_capture_families() {
    let mut total = 0_usize;
    let mut covered = 0_usize;
    let mut top1 = 0_usize;
    let mut top5 = 0_usize;
    let mut overlap = 0_usize;
    let mut theirs_depth = 0_usize;

    for case in candidate_cases() {
        total += 1;

        let mut session = session();
        session
            .replace_raw(&case.input)
            .expect("typing cannot fail");

        let ours: Vec<String> = session
            .candidates()
            .iter()
            .map(|candidate| candidate.text().as_str().unwrap_or_default().to_owned())
            .collect();
        // A record the mini vocabulary knows nothing about would otherwise
        // drag every rate toward zero while measuring the fixture, not the
        // decoder.
        if ours.iter().all(|text| !case.theirs.contains(text)) {
            continue;
        }
        covered += 1;

        let theirs_first = case.theirs.first().expect("at least one candidate");
        if ours.first() == Some(theirs_first) {
            top1 += 1;
        }
        if ours.iter().take(5).any(|text| text == theirs_first) {
            top5 += 1;
        }

        let ours_prefix: BTreeSet<&String> = ours.iter().take(CAPTURE_DEPTH).collect();
        let theirs_prefix: BTreeSet<&String> = case.theirs.iter().take(CAPTURE_DEPTH).collect();
        overlap += ours_prefix.intersection(&theirs_prefix).count();
        theirs_depth += theirs_prefix.len();
    }

    let share = |count: usize, of: usize| {
        if of == 0 {
            0.0
        } else {
            f64::from(u32::try_from(count).unwrap_or(u32::MAX)) * 100.0
                / f64::from(u32::try_from(of).unwrap_or(u32::MAX))
        }
    };
    println!("decode differential — candidates, F-A and F-C");
    println!("  records with a candidate list  {total}");
    println!("  reachable by the mini vocab    {covered}");
    println!(
        "  top-1                          {top1:>4}  {:.2}%",
        share(top1, covered)
    );
    println!(
        "  top-5-set                      {top5:>4}  {:.2}%",
        share(top5, covered)
    );
    println!(
        "  prefix-10 overlap              {overlap:>4} of {theirs_depth}  {:.2}%",
        share(overlap, theirs_depth)
    );

    assert!(covered >= 8, "the fixture must reach something");
    assert!(
        top5 * 100 >= covered * 80,
        "top-5-set fell to {:.2}%",
        share(top5, covered)
    );
}
