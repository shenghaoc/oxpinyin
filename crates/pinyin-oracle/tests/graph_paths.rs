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
//! inputs, recovered from historical W2 logs by the `oracle-paths` importer. No
//! oracle build is needed to run this, which is the point: the check belongs
//! in ordinary portable CI, not behind a Linux-only feature.

use oxpinyin_core::graph::SegmentGraph;
use oxpinyin_core::{Completeness, SyllableKey};
use pinyin_oracle::capture::unescape;

const PATHS: &str = include_str!("../../../fixtures/w4/oracle-paths.txt");

/// Inputs whose oracle path the graph deliberately does not admit.
///
/// Maintainer decision 3 in
/// Both halves of maintainer decision 3 in
/// `parser-spec-contradiction-incomplete-keys.md` are now closed. The doubled
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
    // Keep the named W2 witnesses on the graph path that shipping uses. Their
    // selected paths are already in the frozen fixture; no live oracle is
    // needed to check these formerly live-only assertions in per-PR CI.
    let records = records();
    for (input, path) in [
        (
            "yingchon",
            "ying@0:4:complete,ch@4:6:partial,o@6:7:complete,n@7:8:partial",
        ),
        (
            "fanrufe",
            "fan@0:3:complete,ru@3:5:complete,f@5:6:partial,e@6:7:complete",
        ),
        ("ngannve", "n@0:1:partial,gan@1:4:complete,nve@4:7:complete"),
        (
            "zzzzzzzz",
            "z@0:1:partial,z@1:2:partial,z@2:3:partial,z@3:4:partial,z@4:5:partial,z@5:6:partial,z@6:7:partial,z@7:8:partial",
        ),
    ] {
        let record = records
            .iter()
            .find(|record| record.input == input.as_bytes())
            .expect("named W2 witness is in the frozen corpus");
        assert_eq!(record.segments, parse_path(path), "{input}");
        let graph = SegmentGraph::build(input.as_bytes()).expect("short input");
        assert_eq!(walk(&graph, &record.segments), Ok(()), "{input}");
        assert_eq!(graph.consumed(), input.len(), "{input}");
    }

    // The population the graph was built for: paths carrying an initial-only
    // key that is not the final segment, which no Vec<Parse> path set can
    // hold. Every one of them must walk.
    let mut checked = 0;

    for record in records {
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

// These six sentinel inputs cannot appear in the ordinary path fixture. Keep
// the live oracle's guard assertions when retiring the W2 comparator, and
// check the production graph beside them. See oracle-apostrophe-abort.md.
#[cfg(feature = "oracle-ffi")]
mod live_sentinels {
    use super::{SegmentGraph, parse_path, walk};
    use pinyin_oracle::capture::segments_to_wire;
    use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix, OracleSegment};

    fn oracle() -> Oracle {
        let prefix = OraclePrefix::locate().expect("pin-verified oracle prefix");
        Oracle::open_with_temp_user_dir(prefix).expect("oracle context opens")
    }

    #[test]
    fn the_abort_guard_fires_exactly_on_apostrophe_only_inputs() {
        let mut oracle = oracle();
        for input in ["'", "''", "'''"] {
            let observed = oracle
                .observe(input.as_bytes(), OracleFlags::DEFAULT)
                .expect("guarded, so observable");
            assert_eq!(
                observed.segments,
                vec![OracleSegment::NoKeyColumns {
                    parsed_len: input.len()
                }],
                "{input:?}"
            );
            assert!(observed.has_sentinel_segment());
            assert_eq!(observed.parsed_input_length, input.len());
            let graph = SegmentGraph::build(input.as_bytes()).expect("short input");
            assert_eq!(graph.consumed(), input.len());
            assert!(graph.edges().is_empty());
        }

        for input in ["'ni", "a'a", "ni'h", "xi'an", "q"] {
            let observed = oracle
                .observe(input.as_bytes(), OracleFlags::DEFAULT)
                .expect("observable");
            assert!(
                !observed
                    .segments
                    .iter()
                    .any(|segment| matches!(segment, OracleSegment::NoKeyColumns { .. })),
                "guard fired on {input:?}"
            );
            let graph = SegmentGraph::build(input.as_bytes()).expect("short input");
            let path = parse_path(&segments_to_wire(&observed.segments));
            assert_eq!(walk(&graph, &path), Ok(()), "{input:?}");
            assert_eq!(graph.consumed(), observed.parsed_input_length);
        }
    }

    #[test]
    fn the_live_f_e_01_shape_is_recorded_rather_than_crashing() {
        let mut oracle = oracle();
        for input in ["ni'", "ni'!", "ni'i"] {
            let observed = oracle
                .observe(input.as_bytes(), OracleFlags::DEFAULT)
                .expect("observable");
            assert_eq!(
                segments_to_wire(&observed.segments),
                "ni@0:2:complete,<missing-pinyin>@2",
                "{input:?}"
            );
            let graph = SegmentGraph::build(input.as_bytes()).expect("short input");
            assert_eq!(graph.consumed(), 3, "{input:?}");
            assert_eq!(
                walk(&graph, &parse_path("ni@0:2:complete")),
                Ok(()),
                "{input:?}"
            );
        }
    }
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
