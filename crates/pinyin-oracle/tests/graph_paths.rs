//! Every path the pinned oracle selected is a path through our graph.
//!
//! This is the W4-T1 acceptance check, and the reason the graph exists. The
//! 483 `path-set` divergences measured in
//! `docs/findings/parser-spec-contradiction-incomplete-keys.md` all have the
//! same shape — an initial-only key somewhere other than the end — which the
//! foundation parser's `Vec<Parse>` contract cannot represent and a graph
//! represents without trying.
//!
//! The fixture is the pin's own answer for all 10,459 non-sentinel corpus
//! inputs, recovered by `cargo run -p pinyin-oracle --bin oracle-paths`. No
//! oracle build is needed to run this, which is the point: the check belongs
//! in ordinary portable CI, not behind a Linux-only feature.

use oxpinyin_core::graph::SegmentGraph;
use oxpinyin_core::{Completeness, SyllableKey};
use pinyin_oracle::capture::unescape;

const PATHS: &str = include_str!("../../../fixtures/w4/oracle-paths.txt");

/// Inputs whose oracle path the graph deliberately does not admit.
///
/// Maintainer decision 3 in
/// `docs/findings/parser-spec-contradiction-incomplete-keys.md` leaves
/// apostrophe tolerance open: the pin skips a leading apostrophe and a doubled
/// one, `parser-path-set.md` freezes both as remainder-starting, and nobody
/// has decided between them. The graph follows the frozen SPEC rather than
/// settling an open question by accident, so these two stay divergent — and
/// they stay *named*, so the day the decision lands this list is where it
/// shows up.
const OPEN_APOSTROPHE_CASES: [&str; 2] = ["'ni", "ni''hao"];

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

        let id = graph
            .find_edge(node, key, *begin)
            .ok_or_else(|| format!("no edge for {text}@{begin}:{end} leaving node {node}"))?;
        let edge = graph.edge(id).expect("the id came from this graph");
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

#[test]
fn the_fixture_covers_the_whole_corpus() {
    let records = records();
    assert_eq!(
        records.len(),
        10_459,
        "10,465 corpus inputs less the six oracle-sentinel records"
    );
    assert!(PATHS.contains("pinyin-oracle-path-v1"));
    assert!(PATHS.contains(pinyin_oracle::EXPECTED_PIN_REF));
}

#[test]
fn the_fixture_paths_are_internally_consistent() {
    for record in records() {
        let mut previous_end = 0;
        for (text, begin, end, _) in &record.segments {
            assert!(
                begin >= &previous_end,
                "segments overlap in {:?}",
                record.input
            );
            assert_eq!(end - begin, text.len(), "range disagrees with text");
            assert!(*end <= record.input.len(), "segment runs past the input");
            assert_eq!(
                &record.input[*begin..*end],
                text.as_bytes(),
                "segment text is not the input it claims"
            );
            previous_end = *end;
        }
        assert!(record.consumed <= record.input.len());
        assert!(previous_end <= record.consumed);
    }
}

#[test]
fn every_captured_oracle_path_is_a_graph_path() {
    let mut failures: Vec<(String, String)> = Vec::new();

    for record in records() {
        let graph = SegmentGraph::build(&record.input).expect("corpus inputs are short");
        if let Err(reason) = walk(&graph, &record.segments) {
            failures.push((String::from_utf8_lossy(&record.input).into_owned(), reason));
        }
    }

    let inputs: Vec<&str> = failures.iter().map(|(input, _)| input.as_str()).collect();
    assert_eq!(
        inputs, OPEN_APOSTROPHE_CASES,
        "unexpected graph gaps: {failures:?}"
    );
}

#[test]
fn the_incomplete_key_divergences_are_resolved() {
    // The population the graph was built for: paths carrying an initial-only
    // key that is not the final segment, which no Vec<Parse> path set can
    // hold. Every one of them must walk.
    let mut checked = 0;

    for record in records() {
        let has_mid_partial =
            record
                .segments
                .iter()
                .enumerate()
                .any(|(index, (_, _, _, completeness))| {
                    *completeness == Completeness::Partial && index + 1 < record.segments.len()
                });
        if !has_mid_partial {
            continue;
        }

        checked += 1;
        let graph = SegmentGraph::build(&record.input).expect("corpus inputs are short");
        assert_eq!(
            walk(&graph, &record.segments),
            Ok(()),
            "{:?}",
            String::from_utf8_lossy(&record.input)
        );
    }

    assert!(
        checked >= 481,
        "expected the measured path-set population, saw {checked}"
    );
}

#[test]
fn the_graph_consumes_what_the_oracle_consumed() {
    let mut disagreements: Vec<(String, usize, usize)> = Vec::new();

    for record in records() {
        let graph = SegmentGraph::build(&record.input).expect("corpus inputs are short");
        if graph.consumed() != record.consumed {
            disagreements.push((
                String::from_utf8_lossy(&record.input).into_owned(),
                graph.consumed(),
                record.consumed,
            ));
        }
    }

    let inputs: Vec<&str> = disagreements
        .iter()
        .map(|(input, ..)| input.as_str())
        .collect();
    assert_eq!(
        inputs, OPEN_APOSTROPHE_CASES,
        "unexpected consumed-length gaps: {disagreements:?}"
    );
}
