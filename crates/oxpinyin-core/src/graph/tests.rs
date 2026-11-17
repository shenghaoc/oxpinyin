use super::{Edge, EdgeKind, ExactSegment, GraphError, SegmentGraph};
use crate::{Completeness, OptionBits, SyllableKey};

/// Renders the graph as `from-to:key:kind:tone` under `options`.
fn rendered_with_options(input: &str, options: OptionBits) -> Vec<String> {
    SegmentGraph::build_with_options(input.as_bytes(), options)
        .expect("the test inputs are short")
        .edges()
        .iter()
        .map(|edge| {
            format!(
                "{}-{}:{}:{}:{}",
                edge.from(),
                edge.to(),
                edge.key().text(),
                edge.kind().as_wire(),
                edge.tone()
            )
        })
        .collect()
}

/// `PINYIN_INCOMPLETE | USE_TONE` — the W15 tone profile.
fn tone_options() -> OptionBits {
    OptionBits::PINYIN_INCOMPLETE | OptionBits::USE_TONE
}

fn force_tone_options() -> OptionBits {
    OptionBits::PINYIN_INCOMPLETE | OptionBits::USE_TONE | OptionBits::FORCE_TONE
}

#[test]
fn force_tone_rejects_every_toneless_span() {
    // The pin's force-tone check sits INSIDE the USE_TONE branch
    // (`pinyin_parser2.cpp:185-189`): under `USE_TONE | FORCE_TONE` a
    // span without a trailing 1-5 digit does not parse at all, so
    // `nihao` and `zai6` consume nothing (the measured 0x60 probes).
    for input in ["nihao", "zai6", "ni"] {
        let graph = SegmentGraph::build_with_options(input.as_bytes(), force_tone_options())
            .expect("valid");
        assert_eq!(graph.consumed(), 0, "input {input}");
        assert!(graph.edges().is_empty(), "input {input}");
    }
    // Toned spans still parse.
    let graph = SegmentGraph::build_with_options(b"ni3hao3", force_tone_options()).expect("valid");
    assert!(graph.fully_consumed());
    let graph = SegmentGraph::build_with_options(b"zai4", force_tone_options()).expect("valid");
    assert_eq!(graph.consumed(), 4);
}

#[test]
fn force_tone_is_dead_without_use_tone() {
    // The check is nested inside the tone scan, so a word carrying
    // FORCE_TONE without USE_TONE (the harness's inert `0x1ca` shape)
    // parses exactly like the toneless profile.
    let inert = OptionBits::PINYIN_INCOMPLETE | OptionBits::FORCE_TONE;
    let graph = SegmentGraph::build_with_options(b"nihao", inert).expect("valid");
    assert!(graph.fully_consumed());
}

#[test]
fn use_tone_consumes_a_trailing_digit_with_the_span() {
    // `pinyin_parser2.cpp:176-214`: the digit is inside the consumed
    // span, rides the key as its tone, and outranks the shorter
    // toneless parse at the same position. The shorter parses stay
    // available as segmentation alternatives.
    assert_eq!(
        rendered_with_options("zai4", tone_options()),
        [
            "0-4:zai:exact:4",
            "0-3:zai:segmentation:0",
            "0-2:za:segmentation:0",
            "0-1:z:incomplete:0",
            "1-4:ai:exact:4",
            "1-3:ai:segmentation:0",
            "1-2:a:segmentation:0",
        ]
    );
}

#[test]
fn use_tone_reaches_the_seventh_byte_of_a_span() {
    // `max_full_pinyin_length = 7` "include[s] tone"
    // (`pinyin_parser2.cpp:82`): zhuang4 is a 7-byte span and its
    // toneless 6-byte sibling becomes the segmentation alternative.
    let edges = rendered_with_options("zhuang4", tone_options());
    assert!(edges.contains(&"0-7:zhuang:exact:4".to_owned()));
    assert!(edges.contains(&"0-6:zhuang:segmentation:0".to_owned()));
    assert!(
        SegmentGraph::build_with_options(b"zhuang4", tone_options())
            .expect("valid")
            .fully_consumed()
    );
}

#[test]
fn use_tone_rejects_the_non_tone_digits() {
    // 0 and 6-9 are not tones: they stay in the core, fail the lookup,
    // and the span falls back to the shorter toneless parse with the
    // digit left as junk.
    for input in ["zai6", "zai0", "zai9"] {
        let graph =
            SegmentGraph::build_with_options(input.as_bytes(), tone_options()).expect("valid");
        assert_eq!(graph.consumed(), 3, "input {input}");
        assert!(!graph.fully_consumed(), "input {input}");
        assert!(
            graph
                .edges()
                .iter()
                .all(|edge| edge.tone() == 0 && edge.to() <= 3),
            "input {input}"
        );
    }
}

#[test]
fn use_tone_leaves_junk_after_the_digit_unparsed() {
    let graph = SegmentGraph::build_with_options(b"zai4Q", tone_options()).expect("valid");
    assert_eq!(graph.consumed(), 4);
    assert!(!graph.fully_consumed());
    let path: Vec<(usize, &str, u8)> = graph
        .fewest_keys(true)
        .iter()
        .map(|edge| (edge.to(), edge.key().text(), edge.tone()))
        .collect();
    assert_eq!(path, [(4, "zai", 4)]);
}

#[test]
fn use_tone_carries_a_digit_over_an_apostrophe_boundary() {
    let graph = SegmentGraph::build_with_options(b"zai4'an", tone_options()).expect("valid");
    assert!(graph.fully_consumed());
    let path: Vec<(&str, u8)> = graph
        .fewest_keys(true)
        .iter()
        .map(|edge| (edge.key().text(), edge.tone()))
        .collect();
    assert_eq!(path, [("zai", 4), ("an", 0)]);
}

#[test]
fn use_tone_admits_a_tone_on_an_incomplete_key_only_with_the_bit() {
    // The scan's only precondition is the option-gated index hit, so
    // the initial-only "n" carries a tone like any complete syllable.
    let edges = rendered_with_options("n4", tone_options());
    assert!(edges.contains(&"0-2:n:incomplete:4".to_owned()));

    // Incomplete edges are emitted unconditionally and filtered by the
    // walk (`fewest_keys(false)`, the session's `walk`), so the tone
    // rides an edge the incomplete-off profile drops whole.
    let without_incomplete = OptionBits::USE_TONE;
    let graph = SegmentGraph::build_with_options(b"n4", without_incomplete).expect("valid");
    assert_eq!(
        graph
            .edges()
            .iter()
            .map(|edge| (edge.kind().as_wire(), edge.tone()))
            .collect::<Vec<_>>(),
        [("incomplete", 4), ("incomplete", 0)]
    );
    assert!(graph.fewest_keys(false).is_empty());

    // A lone digit is an empty core and never parses.
    assert!(
        SegmentGraph::build_with_options(b"4", tone_options())
            .expect("valid")
            .edges()
            .is_empty()
    );
}

#[test]
fn the_empty_input_is_one_node_and_no_edges() {
    let graph =
        SegmentGraph::build_with_options(b"", OptionBits::default()).expect("empty is valid");
    assert_eq!(graph.node_count(), 1);
    assert_eq!(graph.edges().len(), 0);
    assert_eq!(graph.consumed(), 0);
    assert!(graph.fully_consumed());
    assert!(graph.outgoing(0).is_empty());
    assert!(graph.outgoing(99).is_empty());
}

#[test]
fn repeated_initials_chain_the_whole_way() {
    // The pin admits initial-only keys repeatedly (`pinyin_parser2.cpp:266-284`).
    let graph =
        SegmentGraph::build_with_options(b"zzzzzzzz", OptionBits::default()).expect("valid");
    assert_eq!(graph.consumed(), 8);
    assert!(graph.fully_consumed());
    assert!(
        graph
            .edges()
            .iter()
            .all(|edge| edge.kind() == EdgeKind::Incomplete)
    );
}

#[test]
fn a_mid_position_initial_is_an_ordinary_edge() {
    // yingchon: the pin selects ying, ch, o, n — two non-final partials.
    let graph =
        SegmentGraph::build_with_options(b"yingchon", OptionBits::default()).expect("valid");
    for (from, key, start) in [(0, "ying", 0), (4, "ch", 4), (6, "o", 6), (7, "n", 7)] {
        let key = SyllableKey::from_text(key).expect("frozen key");
        assert!(
            graph.edges().iter().any(|edge| edge.from() == from
                && edge.key() == key
                && edge.syllable_start() == start),
            "missing edge {from} {}",
            key.text()
        );
    }
    assert!(graph.fully_consumed());
}

#[test]
fn an_input_past_the_pins_gint16_steps_is_parsed_whole() {
    // 80000 bytes: past both the pin's 32767-byte `gint16` steps and the
    // old 65535-byte refusal
    // (`docs/findings/upstream-divergences.md#the-full-pinyin-parses-16-bit-steps`).
    let long = b"ni".repeat(40_000);
    let graph = SegmentGraph::build_with_options(&long, OptionBits::default())
        .expect("no length limit below the position width");
    assert_eq!(graph.consumed(), long.len());
    let path = graph.fewest_keys(false);
    assert_eq!(path.len(), 40_000);
    assert_eq!(path.last().map(Edge::to), Some(long.len()));
}

#[test]
fn a_cached_apostrophe_run_emits_the_edges_of_a_per_node_scan() {
    // The scan `key_start` replaced: every node rescans the rest of its
    // apostrophe run, and a run that reaches the end starts no key.
    fn per_node_key_start(input: &[u8], node: usize) -> Option<usize> {
        let byte = *input.get(node)?;
        if byte == b'\'' {
            let mut cursor = node + 1;
            while *input.get(cursor)? == b'\'' {
                cursor += 1;
            }
            return Some(cursor);
        }
        Some(node)
    }

    let inputs: [(&[u8], usize); 5] = [
        (b"", 0),
        (b"'''", 3),
        (b"'ni", 3),
        (b"ni''hao'''", 10),
        (b"'''xi'an''''zhong'guo'!", b"'''xi'an''''zhong'guo'".len()),
    ];
    for (input, consumed) in inputs {
        let mut per_node = Vec::new();
        let mut starts = Vec::new();
        for node in 0..=input.len() {
            starts.push(per_node.len());
            if let Some(start) = per_node_key_start(input, node) {
                super::emit_edges(input, node, start, OptionBits::default(), &mut per_node);
            }
        }
        starts.push(per_node.len());
        let graph =
            SegmentGraph::build_with_options(input, OptionBits::default()).expect("short input");
        // With the same emission and reachability rules, the only
        // independent input to path selection is the per-node rescan.
        assert_eq!(graph.edges(), per_node);
        let uncached = SegmentGraph {
            edges: per_node,
            starts,
            ..graph.clone()
        };
        assert_eq!(graph.fewest_keys(false), uncached.fewest_keys(false));
        assert_eq!(graph.consumed(), consumed);
    }
}

#[test]
fn edge_distance_marks_alias_spellings_only() {
    let graph = SegmentGraph::build_with_options(b"chonga", OptionBits::PINYIN_CORRECT_ON_ONG)
        .expect("valid");
    let distances: Vec<String> = graph
        .edges()
        .iter()
        .filter(|edge| edge.kind() != EdgeKind::Incomplete)
        .map(|edge| format!("{}-{}:{}", edge.from(), edge.to(), edge.distance()))
        .collect();
    // `chong`, `chon`, then the unreachable-start `hong`, `hon`, `o`,
    // `ong`, then `ga` and `a`: only the `-on` aliases carry 1.
    assert_eq!(
        distances,
        [
            "0-5:0", "0-4:1", "1-5:0", "1-4:1", "2-3:0", "3-5:0", "4-6:0", "5-6:0"
        ]
    );
}

// Every path the pinned oracle selected is a path through our graph.
//
// This is the W4-T1 acceptance check. The pin admits initial-only keys
// at any position, including repeated and non-final keys. The graph
// represents those keys without enumerating every path.
//
// The fixture is the pin's own answer for 1,788 corpus records, recovered
// from historical W2 logs: every record with a partial segment, a consumed
// length short of the input, or nothing consumed (1,744), plus one witness
// per remaining segmentation shape (44). No oracle build is needed to run
// this, which is the point: the check belongs in ordinary portable CI, not
// behind a Linux-only feature.

const PATHS: &str = include_str!("../../../../fixtures/w4/oracle-paths.txt");

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
    let on_ong = OptionBits::PINYIN_CORRECT_ON_ONG;
    assert_eq!(selected("chonga", on_ong), ["chong@0-5", "a@5-6"]);
    assert_eq!(selected("chongan", on_ong), ["chong@0-5", "an@5-7"]);
    assert_eq!(selected("zhonge", on_ong), ["zhong@0-5", "e@5-6"]);
    assert_eq!(
        selected("chonga", OptionBits::default()),
        ["chong@0-5", "a@5-6"]
    );

    // Totals, not counts of corrected keys: `chong'agn` is 1, `chon'gagn` 2.
    let two = OptionBits::PINYIN_CORRECT_ON_ONG | OptionBits::PINYIN_CORRECT_GN_NG;
    assert_eq!(selected("chongagn", two), ["chong@0-5", "ang@5-8"]);

    // The key count still comes first: `liou` is one key at distance 1
    // rather than `li'ou`, two keys at 0.
    let iou_iu = OptionBits::PINYIN_CORRECT_IOU_IU;
    assert_eq!(selected("liou", iou_iu), ["liu@0-4"]);
}

// Field decoding for the `pinyin-capture-v1` text the frozen captures use.

/// Decodes one `pinyin-capture-v1` escaped field back to its bytes.
///
/// A trailing lone backslash or a malformed `\xNN` is preserved literally rather
/// than treated as fatal: a fixture the reader cannot fully decode must still be
/// reportable, not silently dropped.
pub fn unescape(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'\\' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        match bytes.get(index + 1) {
            Some(b'\\') => {
                out.push(b'\\');
                index += 2;
            }
            Some(b't') => {
                out.push(b'\t');
                index += 2;
            }
            Some(b'n') => {
                out.push(b'\n');
                index += 2;
            }
            Some(b'r') => {
                out.push(b'\r');
                index += 2;
            }
            Some(b'x') => {
                let hex = bytes.get(index + 2..index + 4).and_then(|pair| {
                    let text = core::str::from_utf8(pair).ok()?;
                    u8::from_str_radix(text, 16).ok()
                });
                if let Some(value) = hex {
                    out.push(value);
                    index += 4;
                } else {
                    out.push(b'\\');
                    index += 1;
                }
            }
            _ => {
                out.push(b'\\');
                index += 1;
            }
        }
    }
    out
}

#[path = "../../tests/common/parser_rows.rs"]
mod parser_rows;

parser_rows::gated_parser_rows! {
    fn a_gated_parser_row_follows_its_option_bit(
        #[case] input: &str,
        #[case] options: u32,
        #[case] parsed: usize,
        #[case] keys: &[(u16, u16, u16, &str)],
        #[case] _aux: &str,
    ) {
        let graph = SegmentGraph::build_with_options(input.as_bytes(), OptionBits::from_bits_retain(options)).unwrap();
        assert_eq!(graph.consumed(), parsed);
        let found: Vec<_> = graph.fewest_keys(true).iter().map(|edge| {
            let key = edge.key().chewing_key().with_tone(edge.tone());
            (key.to_packed(), u16::try_from(edge.syllable_start()).unwrap(), u16::try_from(edge.to()).unwrap(), key.pinyin_string())
        }).collect();
        let expected: Vec<_> = keys.iter().map(|&(key, begin, end, pinyin)| (key, begin, end, pinyin.to_owned())).collect();
        assert_eq!(found, expected);
    }
}

/// The import parser crosses long separator runs and keeps the two toned keys.
#[test]
fn an_import_reading_parses_past_long_separator_runs() {
    for reading in [
        [&b"ni3"[..], &[b'\''; 40_000], b"hao3"].concat(),
        [&b"ni3'hao3"[..], &[b'\''; 70_000]].concat(),
    ] {
        let graph = SegmentGraph::build_with_options(&reading, OptionBits::USE_TONE).unwrap();
        assert_eq!(graph.consumed(), reading.len());
        let keys: Vec<_> = graph
            .fewest_keys(false)
            .iter()
            .map(|edge| {
                edge.key()
                    .chewing_key()
                    .with_tone(edge.tone())
                    .pinyin_string()
            })
            .collect();
        assert_eq!(keys, ["ni3", "hao3"]);
    }
}
