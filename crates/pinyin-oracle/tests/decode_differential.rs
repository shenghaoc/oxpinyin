//! The first decode-level differential run against the W2 parity corpus.
//!
//! `docs/findings/decode-differential.md` defines the report. Two populations,
//! because the pin gives us two different kinds of answer:
//!
//! - **Segmentation**, over all 10,459 recovered corpus paths. This is where
//!   the 483 `path-set` divergences and the 468 `tie-swap` baseline live, and
//!   it is the number W4 exists to move.
//! - **Candidates**, over the 55 F-A/F-C records that carry a candidate list.
//!   `top-1`, `top-5-set` and `prefix-10`, measured through the session API.
//!
//! Both run in portable CI with no oracle build.

use std::collections::BTreeSet;

use oxpinyin_core::SyllableKey;
use oxpinyin_core::fixture::{FixtureDictionary, FixtureLanguageModel};
use oxpinyin_core::graph::SegmentGraph;
use oxpinyin_core::kbest::k_best;
use oxpinyin_core::scoring::{Scorer, ScoringConfig};
use oxpinyin_engine::{EmptyConfigSource, KeyInput, Session, StoragePaths};
use pinyin_oracle::capture::unescape;

const PATHS: &str = include_str!("../../../fixtures/w4/oracle-paths.txt");
const F_A: &str = include_str!("../../../fixtures/foundation/f-a.txt");
const F_C: &str = include_str!("../../../fixtures/foundation/f-c.txt");
const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

/// How many segmentations the differential asks for, matching the session.
const K: usize = 8;

/// Depth the capture protocol records candidates to.
const CAPTURE_DEPTH: usize = 10;

/// Both halves of maintainer decision 3 are now closed. The doubled half
/// (`ni''hao`) closed 2026-08-21; the leading half (`'ni`) closed 2026-08-21
/// by the same zero-width step-propagation law. No open apostrophe cases
/// remain.
const OPEN_APOSTROPHE_CASES: [&str; 0] = [];

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.split('\t')
        .filter_map(|field| field.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// A segmentation as `(key, byte offset of its text)` pairs.
type Path = Vec<(SyllableKey, usize)>;

/// One recovered oracle path: the input, and the segmentation it chose.
fn corpus() -> Vec<(Vec<u8>, Path)> {
    PATHS
        .lines()
        .filter(|line| line.contains('\t'))
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let input = unescape(parts.next()?);
            let _consumed = parts.next()?;
            let path = parts.next()?;

            if path == "-" {
                return Some((input, Vec::new()));
            }
            let keys: Option<Path> = path
                .split(',')
                .map(|entry| {
                    let (text, rest) = entry.rsplit_once('@')?;
                    let begin = rest.split(':').next()?.parse().ok()?;
                    Some((SyllableKey::from_text(text)?, begin))
                })
                .collect();
            Some((input, keys?))
        })
        .collect()
}

/// The keys and offsets of one decoded path.
fn decoded(graph: &SegmentGraph, path: &oxpinyin_core::kbest::DecodedPath) -> Path {
    path.edges()
        .iter()
        .filter_map(|id| graph.edge(*id))
        .map(|edge| (edge.key(), edge.syllable_start()))
        .collect()
}

#[test]
fn segmentation_differential_over_the_parity_corpus() {
    let dictionary = FixtureDictionary::parse(VOCAB).expect("committed fixture");
    let model = FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixtures");
    let scorer = Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

    let mut total = 0_usize;
    let mut top1 = 0_usize;
    let mut top5 = 0_usize;
    let mut in_k = 0_usize;
    let mut absent: Vec<String> = Vec::new();

    for (input, wanted) in corpus() {
        total += 1;
        let graph = SegmentGraph::build(&input).expect("corpus inputs are short");
        let paths = k_best(&graph, &scorer, K).expect("k is small");

        let ranked: Vec<Path> = paths.iter().map(|path| decoded(&graph, path)).collect();
        match ranked.iter().position(|path| *path == wanted) {
            Some(0) => {
                top1 += 1;
                top5 += 1;
                in_k += 1;
            }
            Some(rank) => {
                if rank < 5 {
                    top5 += 1;
                }
                in_k += 1;
            }
            None => absent.push(String::from_utf8_lossy(&input).into_owned()),
        }
    }

    let share = |count: usize| count as f64 * 100.0 / total as f64;
    println!("decode differential — segmentation, W2 parity corpus");
    println!("  compared                {total}");
    println!("  top-1                   {top1:>6}  {:.2}%", share(top1));
    println!("  top-5                   {top5:>6}  {:.2}%", share(top5));
    println!("  present in the k best   {in_k:>6}  {:.2}%", share(in_k));
    println!(
        "  path absent             {:>6}  {:.2}%",
        absent.len(),
        share(absent.len())
    );
    for input in absent.iter().take(20) {
        println!("    absent  {input}");
    }

    assert_eq!(total, 10_459);
    assert_eq!(
        absent, OPEN_APOSTROPHE_CASES,
        "every corpus path should be reachable by the decoder"
    );
    assert!(
        top1 * 100 >= total * 80,
        "top-1 fell to {:.2}%",
        share(top1)
    );
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

        let mut session: Session<FixtureDictionary, FixtureLanguageModel> = Session::new(
            &EmptyConfigSource,
            StoragePaths::new("user"),
            FixtureDictionary::parse(VOCAB).expect("committed fixture"),
            FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixtures"),
        )
        .expect("the fixtures open");

        for character in case.input.chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }

        let ours: Vec<String> = session
            .candidates()
            .iter()
            .map(|candidate| candidate.text().to_owned())
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
            count as f64 * 100.0 / of as f64
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
