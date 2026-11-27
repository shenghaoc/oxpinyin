//! The original↔session coordinate mappers over the stored parses.
//!
//! The transformed seams (double pinyin and the zhuyin keyboards) drive
//! the decoder with a `'`-joined full-pinyin spelling while the caller's
//! offsets live in the original input's coordinates — these functions are
//! the map between the two spaces, byte-identical across both C-ABI
//! facades. The Luoma and secondary-zhuyin index parses drive it with the
//! typed input itself, as plain full pinyin does, so they need no map.

use crate::parser::{DoublePinyinParse, ZhuyinParse};

/// Maps a byte offset in the transformed `'`-joined full-pinyin string
/// back to the original zhuyin input offset.
///
/// Candidate consumption — and the session's post-select composition
/// offset — always lands on a key boundary, so the mapping is exact
/// there; an offset inside a transformed key is clamped to that key's
/// original end (the same place a candidate would consume it).
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
/// Ask this before an after-cursor lookup snaps the offset to the key's
/// column. A before-cursor lookup already answers nothing for a span
/// ending mid-key.
#[must_use]
pub fn zhuyin_offset_is_mid_key(parse: &ZhuyinParse, offset: usize) -> bool {
    offset < parse.consumed() && !parse.keys().iter().any(|item| item.start() == offset)
}

/// Whether an original lookup offset falls strictly inside a parsed
/// double-pinyin key — [`zhuyin_offset_is_mid_key`]'s sibling for the
/// pinyin facade. The pin's matrix holds a key only on the column
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

// fill_matrix uses each key rest's m_raw_begin, phonetic_key_matrix.cpp:52–56. The exact
// graph uses the preceding segment end for a later edge: SegmentGraph::build_exact
// (src/graph.rs) sets an edge's from to 0 for the first segment and to the preceding
// segment's end for each later one.
/// Maps an original double-pinyin byte offset to a session matrix column. The first key
/// starts at column zero. A later key starts at its preceding apostrophe. An offset past
/// the keys maps to the last transformed end.
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

/// The shared walk behind the two `*_session_offset` mappers: over
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

/// Maps an original Zhuyin keystroke offset to the corresponding transformed session
/// column. See [`double_session_offset`] for the same mapping on double pinyin.
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

/// Maps a byte offset in the original Zhuyin input to a before- or after-cursor lookup
/// column. At or beyond the consumed prefix, either direction returns `session_len`.
/// Within that prefix, a before-cursor lookup requires an exact key end and otherwise
/// returns `session_len`. An after-cursor lookup uses the current key's column through
/// [`zhuyin_session_offset`].
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
mod tests;
