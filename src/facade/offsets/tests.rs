use crate::OptionBits;
use crate::parser::{DoublePinyinParser, DoublePinyinScheme, ZhuyinParser, ZhuyinScheme};

use super::*;

/// Maps a byte offset in the transformed `'`-joined full-pinyin string
/// back to the original double-pinyin input offset.
///
/// Candidate consumption — and the session's post-select composition
/// offset — always lands on a key boundary, so the mapping is exact
/// there; an offset inside a transformed key is clamped to that key's
/// original end (the same place a candidate would consume it).
#[must_use]
fn double_original_offset(parse: &DoublePinyinParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        let key_len = item.key().text().len();
        let boundary = transformed + key_len;
        if offset <= boundary {
            return item.end();
        }
        transformed = boundary + 1; // apostrophe between keys
    }
    parse.consumed()
}

/// `su3cl3` on the standard keyboard is 你好: two keys, `ni` over
/// original `[0, 3)` and `hao` over `[3, 6)`; the joined session
/// buffer is `ni'hao` (the syllable texts, no tones), so the
/// transformed key boundaries are 2 and 6 with the apostrophe at 2.
fn nihao() -> ZhuyinParse {
    let parse = ZhuyinParser::with_scheme(ZhuyinScheme::Standard)
        .parse_with_options(b"su3cl3", OptionBits::USE_TONE);
    assert_eq!(parse.consumed(), 6);
    assert_eq!(parse.keys().len(), 2);
    assert_eq!(
        parse
            .keys()
            .iter()
            .map(|k| (k.start(), k.end()))
            .collect::<Vec<_>>(),
        [(0, 3), (3, 6)]
    );
    parse
}

#[test]
fn zhuyin_original_offset_answers_the_end_of_the_key_it_falls_in() {
    let parse = nihao();
    // Inside or at the end of `ni` (transformed 0..=2): its original end.
    assert_eq!(zhuyin_original_offset(&parse, 0), 3);
    assert_eq!(zhuyin_original_offset(&parse, 1), 3);
    assert_eq!(zhuyin_original_offset(&parse, 2), 3);
    // Past the apostrophe, inside `hao` (transformed 3..=6): its end.
    assert_eq!(zhuyin_original_offset(&parse, 3), 6);
    assert_eq!(zhuyin_original_offset(&parse, 6), 6);
    // Beyond the joined buffer: the parse's consumed length.
    assert_eq!(zhuyin_original_offset(&parse, 40), 6);
}

#[test]
fn zhuyin_mid_key_offsets_are_the_ones_no_key_starts_on() {
    let parse = nihao();
    // Key starts and the terminal offset are not mid-key.
    assert!(!zhuyin_offset_is_mid_key(&parse, 0));
    assert!(!zhuyin_offset_is_mid_key(&parse, 3));
    assert!(!zhuyin_offset_is_mid_key(&parse, 6));
    assert!(!zhuyin_offset_is_mid_key(&parse, 40));
    // Inside `su3` and inside `cl3`.
    for offset in [1, 2, 4, 5] {
        assert!(zhuyin_offset_is_mid_key(&parse, offset), "offset {offset}");
    }
}

/// Issue #602: a key boundary maps to the right key's MATRIX COLUMN —
/// the apostrophe byte in front of it (the exact graph's edge `from`),
/// the session image of the pin's `m_raw_begin` column — not to its
/// text start, where the whole-buffer matrix holds nothing.
#[test]
fn zhuyin_session_offset_is_the_next_key_column() {
    let parse = nihao();
    assert_eq!(zhuyin_session_offset(&parse, 0), 0);
    assert_eq!(zhuyin_session_offset(&parse, 2), 0);
    // The key boundary maps to the apostrophe in front of `hao`.
    assert_eq!(zhuyin_session_offset(&parse, 3), 2);
    assert_eq!(zhuyin_session_offset(&parse, 5), 2);
    // Past every key: the buffer's one-past-end.
    assert_eq!(zhuyin_session_offset(&parse, 6), "ni'hao".len());
}

/// The column a mapped offset names is the one the exact-key graph
/// starts that key's edge at — the invariant #602 broke.
#[test]
fn zhuyin_session_offset_names_a_graph_column() {
    use crate::graph::SegmentGraph;
    let parse = nihao();
    let segments = [
        crate::graph::ExactSegment::new(0, 2, parse.keys()[0].key(), 0),
        crate::graph::ExactSegment::new(3, 6, parse.keys()[1].key(), 0),
    ];
    let graph = SegmentGraph::build_exact(b"ni'hao", &segments).expect("valid segments");
    let froms: Vec<usize> = graph.edges().iter().map(|edge| edge.from()).collect();
    for key in parse.keys() {
        assert!(
            froms.contains(&zhuyin_session_offset(&parse, key.start())),
            "key at original {} maps to a column the graph holds ({froms:?})",
            key.start()
        );
    }
}

#[test]
fn zhuyin_lookup_offset_is_direction_dependent_at_a_boundary() {
    let parse = nihao();
    let session_len = "ni'hao".len();
    // The boundary between the two keys: after-cursor takes the right
    // key's column (the apostrophe, 2 — #602), before-cursor the left
    // key's end (also 2).
    assert_eq!(
        zhuyin_lookup_session_offset(&parse, session_len, 3, false),
        2
    );
    assert_eq!(
        zhuyin_lookup_session_offset(&parse, session_len, 3, true),
        2
    );
    // The terminal offset maps to the buffer's one-past-end either way.
    assert_eq!(
        zhuyin_lookup_session_offset(&parse, session_len, 6, false),
        session_len
    );
    assert_eq!(
        zhuyin_lookup_session_offset(&parse, session_len, 6, true),
        session_len
    );
    // A before-cursor offset on no key boundary falls through to the end.
    assert_eq!(
        zhuyin_lookup_session_offset(&parse, session_len, 1, true),
        session_len
    );
}

#[test]
fn double_pinyin_mappers_round_trip_every_key_boundary() {
    // The scheme's own reading of the input is the fixture: whatever
    // keys ZRM produces, the two mappers must agree on their boundaries.
    let parse = DoublePinyinParser::with_scheme(DoublePinyinScheme::Zrm)
        .parse_with_options(b"nihk", OptionBits::empty());
    assert!(!parse.keys().is_empty(), "ZRM parses `nihk`");
    let mut transformed = 0;
    for key in parse.keys() {
        let start = transformed;
        let end = transformed + key.key().text().len();
        // Transformed positions inside the key map to its original end.
        assert_eq!(double_original_offset(&parse, start), key.end());
        assert_eq!(double_original_offset(&parse, end), key.end());
        // The original start maps back to the key's matrix column:
        // 0 for the first key, the apostrophe before it otherwise
        // (#602).
        assert_eq!(
            double_session_offset(&parse, key.start()),
            start.saturating_sub(1)
        );
        transformed = end + 1;
    }
    assert_eq!(
        double_original_offset(&parse, transformed + 5),
        parse.consumed()
    );
}
