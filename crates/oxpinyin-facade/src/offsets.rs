//! The original↔session coordinate mappers over the stored parses.
//!
//! The transformed seams (double pinyin, the zhuyin keyboards, and the
//! LUOMA / `SECONDARY_ZHUYIN` full-pinyin index) drive the decoder with a
//! `'`-joined full-pinyin spelling while the caller's offsets live in the
//! original input's coordinates — these six functions are the map between
//! the two spaces, byte-identical across both C-ABI facades.

use oxpinyin_core::{DoublePinyinParse, FullPinyinIndexParse, ZhuyinParse};

/// Maps a byte offset in the transformed `'`-joined full-pinyin string
/// back to the original double-pinyin input offset.
///
/// Candidate consumption — and the session's post-select composition
/// offset — always lands on a key boundary, so the mapping is exact
/// there; an offset inside a transformed key is clamped to that key's
/// original end (the same place a candidate would consume it).
#[must_use]
pub fn double_original_offset(parse: &DoublePinyinParse, offset: usize) -> usize {
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

/// [`double_original_offset`]'s zhuyin sibling.
#[must_use]
pub fn zhuyin_original_offset(parse: &ZhuyinParse, offset: usize) -> usize {
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

/// Whether an original zhuyin lookup offset falls strictly inside a
/// parsed key — not at any key's start and not at the terminal offset.
///
/// The pin's matrix holds keys only at the columns their key rests
/// begin on (`fill_matrix` appends each key at `m_raw_begin`,
/// `phonetic_key_matrix.cpp:52-56` at 074a2219; `fuzzy_syllable_step`
/// adds alternatives on those same columns), so an after-cursor lookup
/// at a mid-key column searches an empty column: every `search_matrix`
/// answers `SEARCH_NONE`, no item is appended, and the list is the
/// prepended sentence rows alone (`zhuyin.cpp:1498-1512`, `:1624-1626`).
/// The after-cursor facade path asks this before snapping the offset to
/// the key's column (issue #577); the before-cursor builder already
/// answers nothing for a span ending mid-key.
#[must_use]
pub fn zhuyin_offset_is_mid_key(parse: &ZhuyinParse, offset: usize) -> bool {
    offset < parse.consumed() && !parse.keys().iter().any(|item| item.start() == offset)
}

/// Whether an original lookup offset falls strictly inside a parsed
/// double-pinyin key — [`zhuyin_offset_is_mid_key`]'s sibling for the
/// pinyin facade (#625). The pin's matrix holds a key only on the column
/// its key rest begins on (`fill_matrix`, `phonetic_key_matrix.cpp:52-56`
/// at 074a2219), so `pinyin_guess_candidates` at a mid-key column searches
/// nothing and answers the prepended sentence rows alone
/// (`pinyin.cpp:2224-2262`, `:2295-2296`).
#[must_use]
pub fn double_offset_is_mid_key(parse: &DoublePinyinParse, offset: usize) -> bool {
    parse
        .keys()
        .iter()
        .any(|item| item.start() < offset && offset < item.end())
}

/// [`double_offset_is_mid_key`]'s Luoma/secondary-zhuyin sibling.
#[must_use]
pub fn full_offset_is_mid_key(parse: &FullPinyinIndexParse, offset: usize) -> bool {
    parse
        .keys()
        .iter()
        .any(|item| item.start() < offset && offset < item.end())
}

/// Maps a session-coordinate span START back to original zhuyin input
/// coordinates — [`zhuyin_original_offset`]'s sibling for `m_begin`.
///
/// The end mapper answers the END of the key an offset falls in, so
/// feeding it a key's start (or 0) would answer that key's end, one key
/// too far. A span start sits on a key boundary of the `'`-joined buffer:
/// 0 for the first key, otherwise one past the apostrophe that follows
/// the previous key, so the previous key's original end is exactly this
/// key's original start (zhuyin keys are contiguous in the original).
/// Mapping `start - 1` (the apostrophe byte, still inside the previous
/// key's boundary) through the end mapper answers that; 0 stays 0.
#[must_use]
pub fn zhuyin_original_begin(parse: &ZhuyinParse, start: usize) -> usize {
    start
        .checked_sub(1)
        .map_or(0, |before| zhuyin_original_offset(parse, before))
}

/// The Luoma/secondary-zhuyin sibling of [`double_original_offset`]: the
/// transformed string is the `'`-joined canonical spellings, and each key
/// remembers its original byte span (tone digit included).
#[must_use]
pub fn full_original_offset(parse: &FullPinyinIndexParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        let key_len = item.canonical().len();
        let boundary = transformed + key_len;
        if offset <= boundary {
            return item.end();
        }
        transformed = boundary + 1; // apostrophe between keys
    }
    parse.consumed()
}

/// Maps an original-input offset to the transformed session offset — the
/// inverse of [`double_original_offset`]: the matrix column of the first
/// key whose original span ends past `offset`.
///
/// That column is the key's graph edge `from`: 0 for the first key, and
/// for every later key the byte of the apostrophe in front of it — the
/// previous key's transformed end — because the exact-key graph starts
/// each later edge at the previous segment's end
/// (`oxpinyin_core::SegmentGraph::build_exact`) and the scan matrix keys
/// its columns by that start. It is the session-coordinate image of the
/// pin's column: `fill_matrix` appends each key at its key rest's
/// `m_raw_begin` (`phonetic_key_matrix.cpp:52-56` at pin 074a2219), which
/// for these separator-free inputs is the previous key's `m_raw_end`. A
/// key-boundary offset therefore maps to the column a forced run at the
/// next key sits at, and that run spells under the whole-buffer matrix.
///
/// Amended 2026-09-27 (issue #602): this answered the key's text start, one
/// past the apostrophe — a column the matrix does not hold — so a forcing
/// written there was dropped by the next `guess_sentence`'s validation.
#[must_use]
pub fn double_session_offset(parse: &DoublePinyinParse, offset: usize) -> usize {
    session_column(
        parse
            .keys()
            .iter()
            .map(|item| (item.end(), item.key().text().len())),
        offset,
    )
}

/// The shared walk behind the three `*_session_offset` mappers: over
/// `(original end, transformed text length)` per key, the column of the
/// first key whose original span ends past `offset` — 0, or the previous
/// key's transformed end (its trailing apostrophe) — and past every key
/// the last key's transformed end, the buffer's one-past-end.
fn session_column(keys: impl Iterator<Item = (usize, usize)>, offset: usize) -> usize {
    let mut column = 0;
    let mut text_start = 0;
    for (end, len) in keys {
        if offset < end {
            return column;
        }
        column = text_start + len;
        text_start = column + 1; // the apostrophe between keys
    }
    column
}

/// [`double_session_offset`]'s zhuyin sibling — the mapping the zhuyin
/// facade's after-cursor lookup and `zhuyin_choose_candidate` forcing
/// ride (issue #602).
#[must_use]
pub fn zhuyin_session_offset(parse: &ZhuyinParse, offset: usize) -> usize {
    session_column(
        parse
            .keys()
            .iter()
            .map(|item| (item.end(), item.key().text().len())),
        offset,
    )
}

/// [`double_session_offset`]'s Luoma/secondary-zhuyin sibling.
#[must_use]
pub fn full_session_offset(parse: &FullPinyinIndexParse, offset: usize) -> usize {
    session_column(
        parse
            .keys()
            .iter()
            .map(|item| (item.end(), item.canonical().len())),
        offset,
    )
}

///
///
/// The terminal offset (`offset == consumed`) maps to the session
///
/// The mapping is direction-dependent because a key boundary between two
/// syllables is two session positions at once: the end of the left key
/// (`'a'`-joined bytes up to the apostrophe) and the start of the right
/// key (past the apostrophe). The after-cursor family searches spans
/// STARTING at the offset and takes the right key's matrix column
/// ([`zhuyin_session_offset`]); the before-cursor family searches spans
/// ENDING at it and takes the left-key end — upstream's `search_matrix`
/// walk answers the left syllable's candidates there. Amended 2026-09-27 (issue
/// #602): the right key's column is the apostrophe byte in front of it,
/// not its text start, so at a key boundary the two directions now answer
/// the same position; they still differ inside a key and past the last.
#[must_use]
pub fn zhuyin_lookup_session_offset(
    parse: &ZhuyinParse,
    session_len: usize,
    offset: usize,
    before_cursor: bool,
) -> usize {
    if offset >= parse.consumed() {
        return session_len;
    }
    if before_cursor {
        let mut transformed = 0;
        for item in parse.keys() {
            let key_len = item.key().text().len();
            if offset == item.end() {
                return transformed + key_len;
            }
            transformed += key_len + 1; // apostrophe between keys
        }
        return session_len;
    }
    zhuyin_session_offset(parse, offset)
}

#[cfg(test)]
mod tests {
    use oxpinyin_core::{DoublePinyinParser, DoublePinyinScheme, ZhuyinParser, ZhuyinScheme};

    use super::*;

    /// `su3cl3` on the standard keyboard is 你好: two keys, `ni` over
    /// original `[0, 3)` and `hao` over `[3, 6)`; the joined session
    /// buffer is `ni'hao` (the syllable texts, no tones), so the
    /// transformed key boundaries are 2 and 6 with the apostrophe at 2.
    fn nihao() -> ZhuyinParse {
        let parse = ZhuyinParser::with_scheme(ZhuyinScheme::Standard).parse(b"su3cl3", true, false);
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

    #[test]
    fn zhuyin_original_begin_answers_the_key_start_and_keeps_zero() {
        let parse = nihao();
        // The end mapper would turn a first-key start into that key's end.
        assert_eq!(zhuyin_original_begin(&parse, 0), 0);
        // The second key starts one past the apostrophe (transformed 3):
        // the previous key's original end, 3.
        assert_eq!(zhuyin_original_begin(&parse, 3), 3);
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
        use oxpinyin_core::graph::SegmentGraph;
        let parse = nihao();
        let segments = [
            oxpinyin_core::graph::ExactSegment::new(0, 2, parse.keys()[0].key(), 0),
            oxpinyin_core::graph::ExactSegment::new(3, 6, parse.keys()[1].key(), 0),
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
        let parse = DoublePinyinParser::with_scheme(DoublePinyinScheme::Zrm).parse(b"nihk", false);
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
}
