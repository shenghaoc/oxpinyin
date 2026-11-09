//! Every path the pinned oracle selected is a path through our graph.
//!
//! This is the W4-T1 acceptance check, and the reason the graph exists. The
//! 483 `path-set` divergences measured on the W2 corpus all have the
//! same shape — an initial-only key somewhere other than the end — which the
//! foundation parser's `Vec<Parse>` contract cannot represent and a graph
//! represents without trying.
//!
//! The fixture is the pin's own answer for all 10,459 non-sentinel corpus
//! inputs, recovered from historical W2 logs. No
//! oracle build is needed to run this, which is the point: the check belongs
//! in ordinary portable CI, not behind a Linux-only feature.

use oxpinyin_core::graph::SegmentGraph;
use oxpinyin_core::{Completeness, SyllableKey};

use crate::common::capture_text::unescape;

const PATHS: &str = include_str!("../../../fixtures/w4/oracle-paths.txt");

/// The pin the paths were captured from.
const EXPECTED_PIN_REF: &str = concat!(
    "libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99",
    "+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155",
    "+dbm-tkrzw"
);

/// Inputs whose oracle path the graph deliberately does not admit.
///
/// Maintainer decision 3 in
/// Both halves of maintainer decision 3 on apostrophes are now closed. The doubled
/// half (`ni''hao`) closed 2026-08-21; the leading half (`'ni`) closed
/// 2026-08-21 by the same zero-width step-propagation law. No open
/// apostrophe cases remain.
const OPEN_APOSTROPHE_CASES: [&str; 0] = [];

/// One record: the input, what the oracle consumed, and the path it chose.
struct Record {
    input: Vec<u8>,
    consumed: usize,
    segments: Vec<(String, usize, usize, Completeness)>,
}

fn records() -> Vec<Record> {
    // A data line is identified by its TAB separators, not by the absence of
    // a leading `#`: the junk stratum contains inputs like `#nishaozhan`, and
    // escaping never produces a raw TAB, so this cannot confuse the two.
    PATHS
        .lines()
        .filter(|line| line.contains('\t'))
        .map(|line| {
            let mut parts = line.split('\t');
            let input = unescape(parts.next().expect("an input field"));
            let consumed = parts
                .next()
                .expect("a consumed field")
                .parse()
                .expect("consumed is a number");
            let path = parts.next().expect("a path field");
            Record {
                input,
                consumed,
                segments: parse_path(path),
            }
        })
        .collect()
}

/// Parses `text@begin:end:complete|partial` entries, or `-` for no segment.
fn parse_path(path: &str) -> Vec<(String, usize, usize, Completeness)> {
    if path == "-" {
        return Vec::new();
    }
    path.split(',')
        .map(|entry| {
            let (text, rest) = entry.rsplit_once('@').expect("a segment has an @");
            let mut parts = rest.split(':');
            let begin = parts.next().expect("begin").parse().expect("begin");
            let end = parts.next().expect("end").parse().expect("end");
            let completeness = match parts.next().expect("completeness") {
                "complete" => Completeness::Complete,
                "partial" => Completeness::Partial,
                other => panic!("unknown completeness {other:?}"),
            };
            (
                String::from_utf8(unescape(text)).expect("syllables are ASCII"),
                begin,
                end,
                completeness,
            )
        })
        .collect()
}

/// Walks `segments` through the graph, returning where it failed.
fn walk(
    graph: &SegmentGraph,
    segments: &[(String, usize, usize, Completeness)],
) -> Result<(), String> {
    let mut node = 0;
    for (text, begin, end, completeness) in segments {
        let key =
            SyllableKey::from_text(text).ok_or_else(|| format!("{text:?} is not a frozen key"))?;
        if key.completeness() != *completeness {
            return Err(format!(
                "{text:?} is {:?} in the inventory but {completeness:?} in the capture",
                key.completeness()
            ));
        }

        let edge = graph
            .edges()
            .iter()
            .find(|edge| {
                edge.from() == node && edge.key() == key && edge.syllable_start() == *begin
            })
            .ok_or_else(|| format!("no edge for {text}@{begin}:{end} leaving node {node}"))?;
        if edge.to() != *end {
            return Err(format!(
                "edge for {text}@{begin} ends at {} not {end}",
                edge.to()
            ));
        }
        node = edge.to();
    }
    Ok(())
}

/// One pass over the fixture: it holds the whole corpus from the recorded
/// pin, and for each record the graph walks the oracle's path and consumes
/// what the oracle consumed.
#[test]
fn every_captured_oracle_path_is_a_graph_path() {
    assert!(PATHS.contains("pinyin-oracle-path-v1"));
    assert!(PATHS.contains(EXPECTED_PIN_REF));
    let records = records();
    assert_eq!(
        records.len(),
        10_459,
        "10,465 corpus inputs less the six oracle-sentinel records"
    );

    let mut failures: Vec<(String, String)> = Vec::new();
    let mut disagreements: Vec<(String, usize, usize)> = Vec::new();
    for record in records {
        let graph = SegmentGraph::build(&record.input).expect("corpus inputs are short");
        let input = String::from_utf8_lossy(&record.input).into_owned();
        if let Err(reason) = walk(&graph, &record.segments) {
            failures.push((input.clone(), reason));
        }
        if graph.consumed() != record.consumed {
            disagreements.push((input, graph.consumed(), record.consumed));
        }
    }

    let inputs: Vec<&str> = failures.iter().map(|(input, _)| input.as_str()).collect();
    assert_eq!(
        inputs, OPEN_APOSTROPHE_CASES,
        "unexpected graph gaps: {failures:?}"
    );
    let inputs: Vec<&str> = disagreements
        .iter()
        .map(|(input, ..)| input.as_str())
        .collect();
    assert_eq!(
        inputs, OPEN_APOSTROPHE_CASES,
        "unexpected consumed-length gaps: {disagreements:?}"
    );
}
