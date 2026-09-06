//! Lookup byte offset → character offset within a committed phrase — the
//! port of `pinyin_get_character_offset` (`pinyin.cpp:3193-3241` at the
//! pin) and its `_pre_compute_tokens` / `_get_char_offset_recur` helpers
//! (`pinyin.cpp:3098-3191`). `zhuyin_get_character_offset`
//! (`zhuyin.cpp:2148-2196`) is the same body over the zhuyin instance.
//!
//! The pin's law, in order:
//!
//! 1. An empty matrix (no parse ran, or a parse that placed no key —
//!    `fill_matrix` leaves it cleared) answers `false`.
//! 2. `assert(offset < matrix.size())` and `_check_offset(matrix, offset)`
//!    — abort shapes, answered as [`EngineError::LookupOffsetOutOfRange`]
//!    and [`EngineError::ZeroKeyOffsetCheck`] (the no-abort policy,
//!    `docs/findings/upstream-divergences.md`).
//! 3. A NULL or empty phrase answers `false` (`g_utf8_to_ucs4` also
//!    answers NULL — length 0 — on invalid UTF-8).
//! 4. `_pre_compute_tokens`: every character of the phrase is searched in
//!    the phrase table as a one-character phrase and its FIRST token is
//!    cached; a character with no token — an emoji, a Latin letter, the
//!    pinyin string itself — answers `false`. This is the row issue #356
//!    pinned: the port used to skip the search and answer `true`.
//! 5. `_get_char_offset_recur`: walk the matrix from column 0. A lone
//!    zero-key column (a consumed `'`, or the zero tail from the parsed
//!    length to the reserved slot) is stepped over without consuming a
//!    character; a real key at the column consumes the next cached token
//!    when the token's item pronounces that key (a below-ε possibility
//!    skips it); the walk answers `true` with the characters consumed so
//!    far as soon as the next key's raw end lies past `offset`, and
//!    backtracks over the column's other keys when a branch fails.
//!
//! Two upstream shapes are answered conservatively here (register entry
//! `pinyin_get_character_offset`'s recursion asserts answer `false` in
//! `docs/findings/upstream-divergences.md`): an empty column at a reached
//! position (`assert(size > 0)`) or a zero key sharing a column with real
//! keys (`assert(1 == size)`) answer [`EngineError::MatrixColumnAssert`]
//! — the availability class; a walk that runs past the cached tokens
//! (the pin reads `g_array_index` past the array — a phrase shorter than
//! the key path it is measured against) treats the missing token as
//! pronouncing nothing, so that branch fails like a mismatched key — the
//! memory-safety class.

use oxpinyin_core::scoring::{ScoringError, expand_keys};
use oxpinyin_core::{Completeness, Dictionary, PhraseEntry, PhraseToken, SyllableKey};

use crate::cursor::MatrixKey;
use crate::error::EngineError;
use crate::session::SCAN_EXPANSION_LIMIT;

/// One matrix column: the real keys placed at the position, in placement
/// order, plus the zero-key entry when the position holds one.
#[derive(Default)]
struct Column {
    real: Vec<MatrixKey>,
    zero_end: Option<usize>,
}

impl Column {
    /// The `_check_offset` shape: exactly one entry, and it is the zero key.
    const fn lone_zero(&self) -> bool {
        self.real.is_empty() && self.zero_end.is_some()
    }
}

/// Builds the column model for one raw buffer and its keys — the same
/// zero-key law [`crate::cursor`] runs (`'` separators inside the parsed
/// span except a leading run, and the zero tail from the parsed length
/// through the reserved slot), with the keys themselves kept.
fn build_columns(
    input: &[u8],
    parsed_len: usize,
    keys: &[MatrixKey],
    separators: bool,
) -> Vec<Column> {
    let bound = input.len();
    let mut columns: Vec<Column> = (0..=bound).map(|_| Column::default()).collect();
    for key in keys {
        if let Some(column) = columns.get_mut(key.syllable_start()) {
            column.real.push(*key);
        }
    }
    let parsed = parsed_len.min(bound);
    if separators {
        let leading_run = input.iter().take_while(|byte| **byte == b'\'').count();
        for position in 0..parsed {
            if input[position] == b'\'' && (position >= leading_run || keys.is_empty()) {
                columns[position].zero_end = Some(position + 1);
            }
        }
    }
    for (position, column) in columns.iter_mut().enumerate().skip(parsed) {
        column.zero_end = Some(position + 1);
    }
    columns
}

/// Whether `token`'s item pronounces `key` with a kept possibility —
/// `item.get_pronunciation_possibility(&key) >= FLT_EPSILON`. An entry
/// spelling the token keeps it when its possibility is `None` (no counts,
/// read as 1) or `Some` with a nonzero matched count; an incomplete key
/// is expanded over its completions like the n-best span scan, since the
/// pin's `pinyin_compare_with_tones` accepts every final for one.
fn pronounces<D>(dictionary: &D, key: SyllableKey, token: PhraseToken) -> Result<bool, EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
{
    let keeps = |entries: Vec<PhraseEntry>| {
        entries.iter().any(|entry| {
            entry.token() == token && !matches!(entry.pronunciation_possibility(), Some((0, _)))
        })
    };
    let lookup = |keys: &[SyllableKey]| {
        dictionary
            .lookup(keys)
            .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))
    };
    if key.completeness() == Completeness::Partial {
        for sequence in expand_keys(&[key], SCAN_EXPANSION_LIMIT) {
            if keeps(lookup(sequence.as_slice())?) {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    Ok(keeps(lookup(&[key])?))
}

/// `_get_char_offset_recur` (`pinyin.cpp:3138-3191`): `Ok(Some(length))`
/// is the pin's `true` with `*plength`, `Ok(None)` its `false`.
fn walk<D>(
    dictionary: &D,
    columns: &[Column],
    tokens: &[PhraseToken],
    start: usize,
    offset: usize,
    length: usize,
) -> Result<Option<usize>, EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
{
    if start > offset {
        return Ok(Some(length));
    }
    let Some(column) = columns.get(start) else {
        // Unreachable by construction: every raw end is at most the
        // reserved slot, and the reserved slot's zero key ends past
        // `offset`. Answered as the empty-column assert shape.
        return Err(EngineError::MatrixColumnAssert { offset: start });
    };
    if column.real.is_empty() && column.zero_end.is_none() {
        return Err(EngineError::MatrixColumnAssert { offset: start });
    }
    if let Some(zero_end) = column.zero_end {
        // "assume only one key here for "'" or the last key."
        if !column.real.is_empty() {
            return Err(EngineError::MatrixColumnAssert { offset: start });
        }
        return walk(dictionary, columns, tokens, zero_end, offset, length);
    }
    for key in &column.real {
        let newstart = key.end();
        // Past the cached tokens the pin reads off its array; the branch
        // is treated as a pronunciation miss instead.
        let Some(&token) = tokens.get(length) else {
            continue;
        };
        if !pronounces(dictionary, key.key(), token)? {
            continue;
        }
        if newstart > offset {
            return Ok(Some(length));
        }
        if let Some(found) = walk(dictionary, columns, tokens, newstart, offset, length + 1)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// The `pinyin_get_character_offset` law over one raw buffer, its parse,
/// and its keys.
///
/// `input` is the active mode's own buffer, `parsed_len` the parse's
/// consumed byte count, `keys` the matrix keys in column order (each at
/// its syllable start), and `separators` whether `'` is a zero-key
/// separator in that mode. `Ok(Some(characters))` is the pin's `true`
/// with the character count; `Ok(None)` its graceful `false` (empty
/// matrix, empty or invalid phrase, a phrase character with no
/// dictionary token, or a walk no key path satisfies).
///
/// # Errors
///
/// [`EngineError::LookupOffsetOutOfRange`] past the reserved slot and
/// [`EngineError::ZeroKeyOffsetCheck`] one past a lone zero-key column
/// (the pin's asserts); [`EngineError::MatrixColumnAssert`] for the two
/// recursion asserts; the dictionary's backend failure as
/// [`EngineError::Scoring`].
pub fn character_offset_over_keys<D>(
    input: &[u8],
    parsed_len: usize,
    keys: &[MatrixKey],
    separators: bool,
    dictionary: &D,
    phrase: &str,
    offset: usize,
) -> Result<Option<usize>, EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
{
    // `0 == matrix.size()`: no parse ran, or the parse placed no key —
    // `fill_matrix` clears the matrix and returns before sizing it when
    // the key vector is empty (`phonetic_key_matrix.cpp:34-38`).
    if input.is_empty() || keys.is_empty() {
        return Ok(None);
    }
    if offset > input.len() {
        return Err(EngineError::LookupOffsetOutOfRange {
            offset,
            len: input.len(),
        });
    }
    let columns = build_columns(input, parsed_len, keys, separators);
    if offset > 0 && columns.get(offset - 1).is_some_and(Column::lone_zero) {
        return Err(EngineError::ZeroKeyOffsetCheck { offset });
    }
    if phrase.is_empty() {
        return Ok(None);
    }

    // `_pre_compute_tokens`: the first token of each character's
    // one-character phrase-table search; none → `false`.
    let mut tokens = Vec::with_capacity(phrase.chars().count());
    let mut buffer = [0_u8; 4];
    for character in phrase.chars() {
        let text: &str = character.encode_utf8(&mut buffer);
        let Some(token) = dictionary.tokens_for_text(text).into_iter().next() else {
            return Ok(None);
        };
        tokens.push(token);
    }

    walk(dictionary, &columns, &tokens, 0, offset, 0)
}

#[cfg(test)]
mod tests {
    use super::character_offset_over_keys;
    use crate::cursor::MatrixKey;
    use crate::error::EngineError;
    use oxpinyin_core::{Dictionary, PhraseEntry, PhraseToken, SyllableKey};

    /// A dictionary of one-character items: text → token, plus the
    /// syllables each token pronounces (possibility 1 when spelled,
    /// absent otherwise).
    struct Dict {
        items: Vec<(&'static str, u32, &'static [&'static str])>,
    }

    impl Dictionary for Dict {
        type Syllable = SyllableKey;
        type Entry = PhraseEntry;
        type Error = core::convert::Infallible;

        fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, Self::Error> {
            let [key] = syllables else {
                return Ok(Vec::new());
            };
            Ok(self
                .items
                .iter()
                .filter(|(_, _, spellings)| spellings.contains(&key.text()))
                .map(|(text, token, _)| {
                    PhraseEntry::new(PhraseToken::new(*token), (*text).to_owned())
                })
                .collect())
        }

        fn tokens_for_text(&self, text: &str) -> Vec<PhraseToken> {
            self.items
                .iter()
                .filter(|(stored, _, _)| *stored == text)
                .map(|(_, token, _)| PhraseToken::new(*token))
                .collect()
        }
    }

    fn dict() -> Dict {
        Dict {
            items: vec![("你", 1, &["ni"]), ("好", 2, &["hao"]), ("泥", 3, &["ni"])],
        }
    }

    fn key(text: &str, start: usize, end: usize) -> MatrixKey {
        MatrixKey::new(
            SyllableKey::from_text(text).expect("syllable"),
            0,
            start,
            end,
        )
    }

    fn nihao() -> Vec<MatrixKey> {
        vec![key("ni", 0, 2), key("hao", 2, 5)]
    }

    #[test]
    fn counts_characters_up_to_the_offset() {
        let keys = nihao();
        for (offset, expected) in [(0, 0), (2, 1), (5, 2)] {
            let got = character_offset_over_keys(b"nihao", 5, &keys, true, &dict(), "你好", offset)
                .expect("no abort shape");
            assert_eq!(got, Some(expected), "offset {offset}");
        }
    }

    #[test]
    fn a_phrase_character_without_a_token_answers_false() {
        // Issue #356: the pinyin string itself as the phrase. Both pinned
        // oracles answer false at every valid offset; the port answered
        // true with a byte-derived count.
        let keys = vec![key("ni", 0, 2)];
        for offset in 0..=2 {
            let got = character_offset_over_keys(b"ni'", 3, &keys, true, &dict(), "ni'", offset)
                .expect("no abort shape");
            assert_eq!(got, None, "offset {offset}");
        }
    }

    #[test]
    fn the_separator_and_tail_zero_keys_are_stepped_over() {
        let keys = vec![key("ni", 0, 2), key("hao", 3, 6)];
        let got = character_offset_over_keys(b"ni'hao", 6, &keys, true, &dict(), "你好", 6)
            .expect("no abort shape");
        assert_eq!(got, Some(2));
        // Offset 3 sits one past the separator's lone zero key: the pin's
        // `_check_offset` abort.
        let got = character_offset_over_keys(b"ni'hao", 6, &keys, true, &dict(), "你好", 3);
        assert!(matches!(
            got,
            Err(EngineError::ZeroKeyOffsetCheck { offset: 3 })
        ));
        // Past the reserved slot: the range assert.
        let got = character_offset_over_keys(b"ni'hao", 6, &keys, true, &dict(), "你好", 7);
        assert!(matches!(
            got,
            Err(EngineError::LookupOffsetOutOfRange { offset: 7, len: 6 })
        ));
    }

    #[test]
    fn a_key_the_phrase_does_not_pronounce_fails_the_walk() {
        // 好 does not pronounce `ni`, so no path reaches the offset.
        let keys = nihao();
        let got = character_offset_over_keys(b"nihao", 5, &keys, true, &dict(), "好你", 2)
            .expect("no abort shape");
        assert_eq!(got, None);
        // A shorter phrase than the matrix: the walk beyond its tokens is
        // a miss, not a read off the array.
        let got = character_offset_over_keys(b"nihao", 5, &keys, true, &dict(), "你", 5)
            .expect("no abort shape");
        assert_eq!(got, None);
    }

    #[test]
    fn empty_input_and_empty_phrase_answer_false() {
        assert_eq!(
            character_offset_over_keys(b"", 0, &[], true, &dict(), "你", 0).expect("no abort"),
            None
        );
        // A non-empty buffer whose parse placed no key: `fill_matrix`
        // leaves the matrix cleared, so the pin's empty-matrix `false`
        // applies before any offset check.
        assert_eq!(
            character_offset_over_keys(b"xyz", 0, &[], true, &dict(), "你", 0).expect("no abort"),
            None
        );
        assert_eq!(
            character_offset_over_keys(b"nihao", 5, &nihao(), true, &dict(), "", 0)
                .expect("no abort"),
            None
        );
    }
}
