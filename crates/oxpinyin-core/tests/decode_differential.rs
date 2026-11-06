//! The decode-level segmentation differential against the W2 parity corpus.
//!
//! `docs/findings/decode-differential.md` defines the report. Over all 10,459
//! recovered corpus paths: this is where the 483 `path-set` divergences and the
//! 468 `tie-swap` baseline live. The candidate half of the report is
//! `oxpinyin-engine`'s `tests/decoding.rs`.

use oxpinyin_core::SyllableKey;
use oxpinyin_core::graph::SegmentGraph;
use oxpinyin_core::kbest::k_best;
use oxpinyin_core::scoring::{Scorer, ScoringConfig};

use crate::common::capture_text::unescape;
use crate::common::fixture_doubles::{FixtureDictionary, FixtureLanguageModel};

const PATHS: &str = include_str!("../../../fixtures/w4/oracle-paths.txt");
const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

/// How many segmentations the differential asks for, matching the session.
const K: usize = 8;

/// Both halves of maintainer decision 3 are now closed. The doubled half
/// (`ni''hao`) closed 2026-08-21; the leading half (`'ni`) closed 2026-08-21
/// by the same zero-width step-propagation law. No open apostrophe cases
/// remain.
const OPEN_APOSTROPHE_CASES: [&str; 0] = [];

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

    let share = |count: usize| {
        f64::from(u32::try_from(count).unwrap_or(u32::MAX)) * 100.0
            / f64::from(u32::try_from(total).unwrap_or(u32::MAX))
    };
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
