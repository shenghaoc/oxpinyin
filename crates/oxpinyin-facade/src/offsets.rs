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
/// inverse of [`double_original_offset`]: the transformed start of the
/// first key whose original span ends past `offset`. A key-boundary
/// offset therefore maps to the next key's start, the position a forced
/// run at that key would sit at.
#[must_use]
pub fn double_session_offset(parse: &DoublePinyinParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        if offset < item.end() {
            return transformed;
        }
        transformed += item.key().text().len() + 1; // key + apostrophe
    }
    transformed
}

/// [`double_session_offset`]'s zhuyin sibling.
#[must_use]
pub fn zhuyin_session_offset(parse: &ZhuyinParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        if offset < item.end() {
            return transformed;
        }
        transformed += item.key().text().len() + 1; // key + apostrophe
    }
    transformed
}

/// [`double_session_offset`]'s Luoma/secondary-zhuyin sibling.
#[must_use]
pub fn full_session_offset(parse: &FullPinyinIndexParse, offset: usize) -> usize {
    let mut transformed = 0;
    for item in parse.keys() {
        if offset < item.end() {
            return transformed;
        }
        transformed += item.canonical().len() + 1; // key + apostrophe
    }
    transformed
}

/// Maps an original zhuyin-input lookup offset to the session raw-buffer
/// offset for the candidate-guess family — the libzhuyin facade's law.
/// The terminal offset (`offset == consumed`) maps to the session
/// buffer's one-past-end — upstream's matrix reserved slot, where the
/// span walk yields nothing and only the prepended sentence rows answer
/// — which the per-key walk in [`zhuyin_session_offset`] overruns by one
/// trailing apostrophe.
///
/// The mapping is direction-dependent because a key boundary between two
/// syllables is two session positions at once: the end of the left key
/// (`'a'`-joined bytes up to the apostrophe) and the start of the right
/// key (past the apostrophe). The after-cursor family searches spans
/// STARTING at the offset and takes the right-key start
/// ([`zhuyin_session_offset`]); the before-cursor family searches spans
/// ENDING at it and takes the left-key end — upstream's `search_matrix`
/// walk answers the left syllable's candidates there.
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
