//! Every path the pinned oracle selected is a path through our graph.
//!
//! This is the W4-T1 acceptance check. The pin admits initial-only keys
//! at any position, including repeated and non-final keys. The graph
//! represents those keys without enumerating every path.
//!
//! The fixture is the pin's own answer for 1,788 corpus records, recovered
//! from historical W2 logs: every record with a partial segment, a consumed
//! length short of the input, or nothing consumed (1,744), plus one witness
//! per remaining segmentation shape (44). No oracle build is needed to run
//! this, which is the point: the check belongs in ordinary portable CI, not
//! behind a Linux-only feature.

use oxpinyin_core::graph::{ExactSegment, GraphError, SegmentGraph};
use oxpinyin_core::{
    Completeness, OptionBits, PINYIN_CORRECT_GN_NG, PINYIN_CORRECT_IOU_IU, PINYIN_CORRECT_ON_ONG,
    SyllableKey,
};

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
/// The `docs/findings/parser-spec.md` correction log aligns the doubled
/// (`ni''hao`) and leading (`'ni`) apostrophe cases with the pin's
/// zero-width step-propagation law. No open apostrophe cases remain.
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

/// One pass over the fixture: it holds the retained corpus records from the
/// recorded pin, and for each record the graph walks the oracle's path and
/// consumes what the oracle consumed.
#[test]
fn every_captured_oracle_path_is_a_graph_path() {
    assert!(PATHS.contains("pinyin-oracle-path-v1"));
    assert!(PATHS.contains(EXPECTED_PIN_REF));
    let records = records();
    assert_eq!(
        records.len(),
        1_788,
        "1,744 predicate records plus the 44 segmentation-shape witnesses"
    );

    let mut failures: Vec<(String, String)> = Vec::new();
    let mut disagreements: Vec<(String, usize, usize)> = Vec::new();
    for record in records {
        let graph = SegmentGraph::build_with_options(&record.input, OptionBits::default())
            .expect("corpus inputs are short");
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

#[test]
fn exact_graph_has_one_edge_per_segment_and_no_resplits() {
    // "bie" pre-parsed as one key: no bi+e edge may appear.
    let bie = SyllableKey::from_canonical_text("bie").expect("bie");
    let graph = SegmentGraph::build_exact(b"bie", &[ExactSegment::new(0, 3, bie, 0)])
        .expect("valid segments");
    let edges: Vec<String> = graph
        .edges()
        .iter()
        .map(|edge| {
            format!(
                "{}-{}:{}:{}",
                edge.from(),
                edge.to(),
                edge.key().text(),
                edge.kind().as_wire()
            )
        })
        .collect();
    assert_eq!(edges, ["0-3:bie:exact"]);
    assert_eq!(graph.consumed(), 3);
    let walk: Vec<&str> = graph
        .fewest_keys(false)
        .iter()
        .map(|edge| edge.key().text())
        .collect();
    assert_eq!(walk, ["bie"]);
}

#[test]
fn exact_graph_rejects_bad_segments() {
    let bie = SyllableKey::from_canonical_text("bie").expect("bie");
    // Overlapping.
    assert_eq!(
        SegmentGraph::build_exact(
            b"bie",
            &[
                ExactSegment::new(0, 3, bie, 0),
                ExactSegment::new(2, 3, bie, 0)
            ]
        ),
        Err(GraphError::ExactSegmentsInvalid)
    );
    // Out of bounds.
    assert_eq!(
        SegmentGraph::build_exact(b"bie", &[ExactSegment::new(0, 9, bie, 0)]),
        Err(GraphError::ExactSegmentsInvalid)
    );
    // Empty span.
    assert_eq!(
        SegmentGraph::build_exact(b"bie", &[ExactSegment::new(2, 2, bie, 0)]),
        Err(GraphError::ExactSegmentsInvalid)
    );
}

#[test]
fn exact_graph_rejects_non_apostrophe_gaps() {
    let ni = SyllableKey::from_canonical_text("ni").expect("ni");
    let hao = SyllableKey::from_canonical_text("hao").expect("hao");
    // "nixhao": the gap byte 'x' is not a separator — riding it would
    // decode hao across unverified input.
    assert_eq!(
        SegmentGraph::build_exact(
            b"nixhao",
            &[
                ExactSegment::new(0, 2, ni, 0),
                ExactSegment::new(3, 6, hao, 0)
            ]
        ),
        Err(GraphError::ExactSegmentsInvalid)
    );
    // Leading junk is equally refused.
    assert_eq!(
        SegmentGraph::build_exact(b"xni", &[ExactSegment::new(1, 3, ni, 0)]),
        Err(GraphError::ExactSegmentsInvalid)
    );
    // A separator run between spans still rides, as in the parsed graph.
    let graph = SegmentGraph::build_exact(
        b"ni''hao",
        &[
            ExactSegment::new(0, 2, ni, 0),
            ExactSegment::new(4, 7, hao, 0),
        ],
    )
    .expect("apostrophe gap is a separator ride");
    assert_eq!(graph.consumed(), 7);
}

// Pin 074a2219 pinyin_parser2.cpp:237–251 propagates each apostrophe
// from a reachable position, including at the start and end.
#[test]
fn a_leading_apostrophe_run_is_consumed_like_a_separator() {
    for (input, path) in [
        ("'ni", &["ni"][..]),
        ("''ni", &["ni"]),
        ("ni''hao", &["ni", "hao"]),
        ("ni'", &["ni"]),
        ("'''", &[]),
    ] {
        let graph = SegmentGraph::build_with_options(input.as_bytes(), OptionBits::default())
            .expect("short input");
        assert_eq!(graph.consumed(), input.len(), "{input:?}");
        assert!(graph.fully_consumed(), "{input:?}");
        assert!(
            graph
                .fewest_keys(false)
                .iter()
                .map(|edge| edge.key().text())
                .eq(path.iter().copied()),
            "{input:?}"
        );
    }
}

#[test]
fn fewest_keys_picks_the_shortest_complete_path() {
    for (input, complete, with_incomplete) in [
        ("nihao", &["ni", "hao"][..], &["ni", "hao"][..]),
        ("nihaoXYZ", &["ni", "hao"], &["ni", "hao"]),
        ("ni'hao", &["ni", "hao"], &["ni", "hao"]),
        ("n", &[], &["n"]),
        ("nih", &["ni"], &["ni", "h"]),
        ("xian", &["xian"], &["xian"]),
        ("", &[], &[]),
    ] {
        let graph = SegmentGraph::build_with_options(input.as_bytes(), OptionBits::default())
            .expect("short input");
        for (allow_incomplete, expected) in [(false, complete), (true, with_incomplete)] {
            let path: Vec<_> = graph
                .fewest_keys(allow_incomplete)
                .iter()
                .map(|edge| edge.key().text())
                .collect();
            assert_eq!(
                path, expected,
                "{input:?}, allow_incomplete={allow_incomplete}"
            );
        }
    }
}

/// `text@start-end` of each selected key under `options`.
fn selected(input: &str, options: OptionBits) -> Vec<String> {
    SegmentGraph::build_with_options(input.as_bytes(), options)
        .expect("the test inputs are short")
        .fewest_keys(false)
        .iter()
        .map(|edge| {
            format!(
                "{}@{}-{}",
                edge.key().text(),
                edge.syllable_start(),
                edge.to()
            )
        })
        .collect()
}

#[test]
fn fewest_keys_breaks_equal_key_counts_by_correction_distance() {
    // Pin 074a2219 `pinyin_parser2.cpp:305-309`: `chon` is a distance-1
    // `PINYIN_CORRECT_ON_ONG` row, so `chong'a` (0) beats `chon'ga` (1).
    let on_ong = OptionBits::from_bits(PINYIN_CORRECT_ON_ONG);
    assert_eq!(selected("chonga", on_ong), ["chong@0-5", "a@5-6"]);
    assert_eq!(selected("chongan", on_ong), ["chong@0-5", "an@5-7"]);
    assert_eq!(selected("zhonge", on_ong), ["zhong@0-5", "e@5-6"]);
    assert_eq!(
        selected("chonga", OptionBits::default()),
        ["chong@0-5", "a@5-6"]
    );

    // Totals, not counts of corrected keys: `chong'agn` is 1, `chon'gagn` 2.
    let two = OptionBits::from_bits(PINYIN_CORRECT_ON_ONG | PINYIN_CORRECT_GN_NG);
    assert_eq!(selected("chongagn", two), ["chong@0-5", "ang@5-8"]);

    // The key count still comes first: `liou` is one key at distance 1
    // rather than `li'ou`, two keys at 0.
    let iou_iu = OptionBits::from_bits(PINYIN_CORRECT_IOU_IU);
    assert_eq!(selected("liou", iou_iu), ["liu@0-4"]);
}
