//! The original↔session coordinate mappers over the stored parses.
//!
//! The transformed seams (double pinyin and the zhuyin keyboards) drive
//! the decoder with a `'`-joined full-pinyin spelling while the caller's
//! offsets live in the original input's coordinates — these functions are
//! the map between the two spaces, byte-identical across both C-ABI
//! facades. The Luoma and secondary-zhuyin index parses drive it with the
//! typed input itself, as plain full pinyin does, so they need no map.

use oxpinyin_core::{DoublePinyinParse, ZhuyinParse};

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
mod tests;
