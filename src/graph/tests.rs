use super::{Edge, EdgeKind, ExactSegment, SegmentGraph};
use crate::OptionBits;
use crate::parser::Completeness;
use crate::vocab::SyllableKey;

fn edge_kind_wire(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Exact => "exact",
        EdgeKind::Segmentation => "segmentation",
        EdgeKind::Incomplete => "incomplete",
    }
}

fn rendered(graph: &SegmentGraph) -> Vec<String> {
    graph
        .edges()
        .iter()
        .map(|edge| {
            format!(
                "{}-{}:{}:{}:{}",
                edge.from(),
                edge.to(),
                edge.key().text(),
                edge_kind_wire(edge.kind()),
                edge.tone()
            )
        })
        .collect()
}

fn tone_options() -> OptionBits {
    OptionBits::PINYIN_INCOMPLETE | OptionBits::USE_TONE
}

#[rstest::rstest]
#[case::forced_without_tone("ni", tone_options() | OptionBits::FORCE_TONE, 0, &[])]
#[case::forced_with_tone("ni3hao3", tone_options() | OptionBits::FORCE_TONE, 7, &[(3, "ni", 3), (7, "hao", 3)])]
#[case::inert_force("nihao", OptionBits::PINYIN_INCOMPLETE | OptionBits::FORCE_TONE, 5, &[(2, "ni", 0), (5, "hao", 0)])]
#[case::tone("zai4", tone_options(), 4, &[(4, "zai", 4)])]
#[case::span_limit("zhuang4", tone_options(), 7, &[(7, "zhuang", 4)])]
#[case::below_tones("zai0", tone_options(), 3, &[(3, "zai", 0)])]
#[case::above_tones("zai6", tone_options(), 3, &[(3, "zai", 0)])]
#[case::trailing_junk("zai4Q", tone_options(), 4, &[(4, "zai", 4)])]
#[case::separator("zai4'an", tone_options(), 7, &[(4, "zai", 4), (7, "an", 0)])]
#[case::incomplete("n4", OptionBits::USE_TONE, 2, &[(2, "n", 4)])]
#[case::empty_core("4", tone_options(), 0, &[])]
fn use_tone_consumes_a_trailing_digit_with_the_span(
    #[case] input: &str,
    #[case] options: OptionBits,
    #[case] consumed: usize,
    #[case] path: &[(usize, &str, u8)],
) {
    // Pin 074a2219: pinyin_parser2.cpp:82,176–213,268–281; graph keeps incomplete edges until path selection.
    let graph = SegmentGraph::build_with_options(input.as_bytes(), options).unwrap();
    assert_eq!(graph.consumed(), consumed);
    assert_eq!(
        graph
            .fewest_keys(true)
            .iter()
            .map(|edge| (edge.to(), edge.key().text(), edge.tone()))
            .collect::<Vec<_>>(),
        path
    );
    let edges = rendered(&graph);
    match input {
        "ni" | "4" => assert!(edges.is_empty()),
        "zai4" => assert_eq!(
            edges,
            [
                "0-4:zai:exact:4",
                "0-3:zai:segmentation:0",
                "0-2:za:segmentation:0",
                "0-1:z:incomplete:0",
                "1-4:ai:exact:4",
                "1-3:ai:segmentation:0",
                "1-2:a:segmentation:0",
            ]
        ),
        "zhuang4" => {
            for expected in ["0-7:zhuang:exact:4", "0-6:zhuang:segmentation:0"] {
                assert!(edges.iter().any(|edge| edge == expected));
            }
        }
        "zai0" | "zai6" => assert!(
            graph
                .edges()
                .iter()
                .all(|edge| edge.tone() == 0 && edge.to() <= 3)
        ),
        "n4" => {
            assert_eq!(edges, ["0-2:n:incomplete:4", "0-1:n:incomplete:0"]);
            assert!(graph.fewest_keys(false).is_empty());
        }
        _ => {}
    }
}

#[test]
fn the_empty_input_is_one_node_and_no_edges() {
    // Rust graph contract: the empty input has one node, consumes nothing and has no outgoing edges or path.
    let graph = SegmentGraph::build_with_options(b"", OptionBits::default()).unwrap();
    assert_eq!(graph.node_count(), 1);
    assert!(graph.edges().is_empty());
    assert_eq!(graph.consumed(), 0);
    assert!(graph.outgoing(0).is_empty());
    assert!(graph.outgoing(99).is_empty());
    assert!(graph.fewest_keys(false).is_empty());
}

#[test]
fn an_input_past_the_pins_gint16_steps_is_parsed_whole() {
    // Register: the full-pinyin parse's 16-bit steps; pin 074a2219 pinyin_parser2.cpp:237–291.
    let long = b"ni".repeat(40_000);
    let graph = SegmentGraph::build_with_options(&long, OptionBits::default()).unwrap();
    assert_eq!(graph.consumed(), long.len());
    let path = graph.fewest_keys(false);
    assert_eq!(path.len(), 40_000);
    assert_eq!(path.last().map(Edge::to), Some(long.len()));
}
const PATHS: &str = include_str!("../../fixtures/w4/oracle-paths.txt");

const EXPECTED_PIN_REF: &str = concat!(
    "libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99",
    "+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155",
    "+dbm-tkrzw"
);

const OPEN_APOSTROPHE_CASES: [&str; 0] = [];

struct Record {
    input: Vec<u8>,
    consumed: usize,
    segments: Vec<(String, usize, usize, Completeness)>,
}

fn records() -> Vec<Record> {
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

#[test]
fn captured_oracle_paths_cover_graph_partitions() {
    // Frozen 074a2219 observations: every one of the 1,788 paths in fixtures/w4/oracle-paths.txt.
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

#[rstest::rstest]
#[case::no_resplit(b"bie", &[(0, 3, "bie")], &["0-3:bie:exact:0"])]
#[case::separator_gap(b"ni''hao", &[(0, 2, "ni"), (4, 7, "hao")], &["0-2:ni:exact:0", "2-7:hao:exact:0"])]
fn exact_graph_has_one_edge_per_segment_and_no_resplits(
    #[case] input: &[u8],
    #[case] spans: &[(usize, usize, &str)],
    #[case] edges: &[&str],
) {
    // Rust build_exact contract: one exact edge per parser span, with intervening apostrophes on the next edge.
    let segments: Vec<_> = spans
        .iter()
        .map(|&(start, end, text)| {
            ExactSegment::new(
                start,
                end,
                SyllableKey::from_canonical_text(text).unwrap(),
                0,
            )
        })
        .collect();
    let graph = SegmentGraph::build_exact(input, &segments).unwrap();
    assert_eq!(rendered(&graph), edges);
    assert_eq!(graph.consumed(), input.len());
    assert_eq!(
        graph
            .fewest_keys(false)
            .iter()
            .map(|edge| edge.key().text())
            .collect::<Vec<_>>(),
        spans.iter().map(|&(_, _, key)| key).collect::<Vec<_>>()
    );
}

#[rstest::rstest]
#[case::leading("''ni", &["ni"])]
#[case::internal("ni''hao", &["ni", "hao"])]
#[case::trailing("ni'", &["ni"])]
#[case::standalone("'''", &[])]
fn a_leading_apostrophe_run_is_consumed_like_a_separator(
    #[case] input: &str,
    #[case] path: &[&str],
) {
    // Pin 074a2219: pinyin_parser2.cpp:237–251.
    let graph = SegmentGraph::build_with_options(input.as_bytes(), OptionBits::default()).unwrap();
    assert_eq!(graph.consumed(), input.len());
    assert_eq!(
        graph
            .fewest_keys(false)
            .iter()
            .map(|edge| edge.key().text())
            .collect::<Vec<_>>(),
        path
    );
}

#[rstest::rstest]
#[case::reachable_prefix("nihaoXYZ", false, &["ni", "hao"])]
#[case::incomplete_only_refused("n", false, &[])]
#[case::incomplete_only_admitted("n", true, &["n"])]
#[case::incomplete_suffix_refused("nih", false, &["ni"])]
#[case::incomplete_suffix_admitted("nih", true, &["ni", "h"])]
#[case::fewest_keys("xian", false, &["xian"])]
fn fewest_keys_picks_the_shortest_complete_path(
    #[case] input: &str,
    #[case] allow_incomplete: bool,
    #[case] expected: &[&str],
) {
    // Pin 074a2219: pinyin_parser2.cpp:296–310,346–380; Rust path selection filters incomplete edges.
    let graph = SegmentGraph::build_with_options(input.as_bytes(), OptionBits::default()).unwrap();
    assert_eq!(
        graph
            .fewest_keys(allow_incomplete)
            .iter()
            .map(|edge| edge.key().text())
            .collect::<Vec<_>>(),
        expected
    );
}

#[rstest::rstest]
#[case::distance("chonga", OptionBits::PINYIN_CORRECT_ON_ONG, &["chong@0-5", "a@5-6"])]
#[case::summed_distance("chongagn", OptionBits::PINYIN_CORRECT_ON_ONG | OptionBits::PINYIN_CORRECT_GN_NG, &["chong@0-5", "ang@5-8"])]
#[case::key_count_first("liou", OptionBits::PINYIN_CORRECT_IOU_IU, &["liu@0-4"])]
fn fewest_keys_breaks_equal_key_counts_by_correction_distance(
    #[case] input: &str,
    #[case] options: OptionBits,
    #[case] expected: &[&str],
) {
    // Pin 074a2219: pinyin_parser2.cpp:296–310.
    let graph = SegmentGraph::build_with_options(input.as_bytes(), options).unwrap();
    assert_eq!(
        graph
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
            .collect::<Vec<_>>(),
        expected
    );
}

const OPTION_BASE: u32 = 0x188;
const OPTION_GATES: u32 = 0x1FE0_1800;

#[rstest::rstest]
#[case::gn_ng_target_outside_the_inventory("egn", OPTION_BASE | 1 << 21, &[(1280, 0, 3, "eng")])]
#[case::s_sh_on("sua", OPTION_BASE | 1 << 11, &[(209, 0, 3, "shua")])]
#[case::s_sh_off("sua", (OPTION_BASE | OPTION_GATES) & !(1 << 11), &[(80, 0, 2, "su"), (128, 2, 3, "a")])]
#[case::z_zh_on("zua", OPTION_BASE | 1 << 12, &[(215, 0, 3, "zhua")])]
#[case::z_zh_off("zua", (OPTION_BASE | OPTION_GATES) & !(1 << 12), &[(86, 0, 2, "zu"), (128, 2, 3, "a")])]
fn a_gated_parser_row_follows_its_option_bit(
    #[case] input: &str,
    #[case] options: u32,
    #[case] keys: &[(u16, u16, u16, &str)],
) {
    // Frozen option-row observations at 074a2219; these target/ambiguity cases lack a syllables-local witness.
    let graph =
        SegmentGraph::build_with_options(input.as_bytes(), OptionBits::from_bits_retain(options))
            .unwrap();
    assert_eq!(graph.consumed(), 3);
    let found: Vec<_> = graph
        .fewest_keys(true)
        .iter()
        .map(|edge| {
            let key = edge.key().chewing_key().with_tone(edge.tone());
            (
                key.to_packed(),
                u16::try_from(edge.syllable_start()).unwrap(),
                u16::try_from(edge.to()).unwrap(),
                key.pinyin_string(),
            )
        })
        .collect();
    let expected: Vec<_> = keys
        .iter()
        .map(|&(key, begin, end, pinyin)| (key, begin, end, pinyin.to_owned()))
        .collect();
    assert_eq!(found, expected);
}

#[test]
fn an_import_reading_parses_past_long_separator_runs() {
    // Register: the full-pinyin parse's 16-bit steps; pin 074a2219 pinyin_parser2.cpp:237–291.
    let reading = [&b"ni3"[..], &[b'\''; 40_000], b"hao3"].concat();
    let graph = SegmentGraph::build_with_options(&reading, OptionBits::USE_TONE).unwrap();
    assert_eq!(graph.consumed(), reading.len());
    assert_eq!(
        graph
            .fewest_keys(false)
            .iter()
            .map(|edge| {
                edge.key()
                    .chewing_key()
                    .with_tone(edge.tone())
                    .pinyin_string()
            })
            .collect::<Vec<_>>(),
        ["ni3", "hao3"]
    );
}
fn unescape(text: &str) -> Vec<u8> {
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
