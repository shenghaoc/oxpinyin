//! Lookup byte offset → character offset within a committed phrase — the
//! port of `pinyin_get_character_offset` (`pinyin.cpp:3193-3241` at the
//! pin) and its `_pre_compute_tokens` / `_get_char_offset_recur` helpers
//! (`pinyin.cpp:3098-3191`). The Zhuyin walk tests zero keys for a
//! matching pronunciation (`zhuyin.cpp:2095-2145`, `:2148-2194`).
//!
//! The pin's law, in order:
//!
//! 1. An empty matrix (no parse ran, or a parse that placed no key —
//!    `fill_matrix` leaves it cleared) answers `false`.
//! 2. `assert(offset < matrix.size())` and `_check_offset(matrix, offset)`
//!    — abort shapes, answered as [`EngineError::LookupOffsetOutOfRange`]
//!    and [`EngineError::ZeroKeyOffsetCheck`], which the C adapters abort on.
//! 3. A NULL or empty phrase answers `false` (`g_utf8_to_ucs4` also
//!    answers NULL — length 0 — on invalid UTF-8).
//! 4. `_pre_compute_tokens`: every character of the phrase is searched in
//!    the phrase table as a one-character phrase and its FIRST token is
//!    cached; a character with no token — an emoji, a Latin letter, the
//!    pinyin string itself — answers `false`.
//! 5. `_get_char_offset_recur` walks the matrix from column 0. Pinyin skips
//!    a lone zero key without consuming a token (`pinyin.cpp:3158-3164`).
//!    For all other keys, including Zhuyin separator and terminal zero keys,
//!    the next cached token must pronounce the key. If that key ends past
//!    `offset`, the walk returns the count so far. Otherwise it consumes the
//!    token and recurses. A failed branch backtracks to the next key.
//!
//! Both walks assert on a reached empty column (`assert(size > 0)`).
//! Pinyin also asserts when a zero key shares a column with real keys
//! (`assert(1 == size)`). The C adapters abort on the corresponding
//! [`EngineError::PinAssert`]. A missing
//! cached token fails the pronunciation test.
//!
//! [`remember_walk`] is `_remember_phrase_recur` (`pinyin.cpp:3578-3666`)
//! over the same columns, for `pinyin_remember_user_input`: it differs in
//! refusing, not asserting, on an empty column or a zero key that shares its
//! column, and in calling the caller's `add` at every complete path.

use crate::model::Dictionary;
use crate::vocab::{PhraseToken, SyllableKey};

use crate::engine::cursor::MatrixKey;
use crate::engine::error::EngineError;

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
/// zero-key law [`crate::engine::cursor`] runs (`'` separators inside the parsed
/// span except a leading run, and the zero tail from the parsed length
/// through the reserved slot), with the keys themselves kept.
fn build_columns(
    input: &[u8],
    parsed_len: usize,
    keys: impl IntoIterator<Item = MatrixKey>,
    separators: bool,
) -> Vec<Column> {
    let bound = input.len();
    let mut columns: Vec<Column> = (0..=bound).map(|_| Column::default()).collect();
    let mut any_key = false;
    for key in keys {
        any_key = true;
        if let Some(column) = columns.get_mut(key.syllable_start()) {
            column.real.push(key);
        }
    }
    let parsed = parsed_len.min(bound);
    if separators {
        let leading_run = input.iter().take_while(|byte| **byte == b'\'').count();
        for position in 0..parsed {
            if input[position] == b'\'' && (position >= leading_run || !any_key) {
                columns[position].zero_end = Some(position + 1);
            }
        }
    }
    for (position, column) in columns.iter_mut().enumerate().skip(parsed) {
        column.zero_end = Some(position + 1);
    }
    columns
}

/// Whether `token`'s item pronounces `key` with a kept possibility, the
/// pin's `item.get_pronunciation_possibility(&key) >= FLT_EPSILON`: the
/// test both `_get_char_offset_recur` (`pinyin.cpp:3170-3174`) and
/// `_remember_phrase_recur` (`:3640-3645`) apply before a key consumes the
/// next cached token: `item.get_pronunciation_possibility(&key) >=
/// FLT_EPSILON` over the item's stored pronunciations
/// (`phrase_index.h:136-163`). A token with no readable item keeps nothing.
///
/// # Errors
///
/// The dictionary's backend failure as [`EngineError::Failure`].
pub fn key_pronounced_by<D>(
    dictionary: &D,
    key: MatrixKey,
    token: PhraseToken,
) -> Result<bool, EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
{
    let query = [key.key()];
    Ok(dictionary
        .read_phrase_item(token, |item| {
            let total = item
                .pronunciations()
                .fold(0_u32, |sum, reading| sum.wrapping_add(reading.freq));
            let matched = item
                .pronunciations()
                .filter(|reading| crate::engine::nbest::reading_matches(&query, reading.keys))
                .fold(0_u32, |sum, reading| sum.wrapping_add(reading.freq));
            // phrase_index.h:141-162: wrapping guint32 sums, then gfloat
            // division; the caller compares against FLT_EPSILON.
            total != 0 && matched as f32 / total as f32 >= f32::EPSILON
        })
        .map_err(|_| EngineError::Failure)?
        .unwrap_or(false))
}

/// The pin's matrix columns for one parse, owned so a walk can alternate
/// dictionary reads with writes to the store behind them.
pub struct MatrixColumns {
    columns: Vec<Column>,
    /// `matrix.size() - 1`: the parsed length, where a complete walk ends.
    end: usize,
}

/// The columns of one parse, or `None` for the empty matrix (`fill_matrix`
/// returns before sizing it when the parse placed no key,
/// `phonetic_key_matrix.cpp:34-38`). Arguments as
/// [`character_offset_walk`].
#[must_use]
pub fn matrix_columns(
    input: &[u8],
    parsed_len: usize,
    keys: impl IntoIterator<Item = MatrixKey>,
    separators: bool,
) -> Option<MatrixColumns> {
    let mut keys = keys.into_iter().peekable();
    if input.is_empty() || keys.peek().is_none() {
        return None;
    }
    let columns = build_columns(input, parsed_len, keys, separators);
    Some(MatrixColumns {
        columns,
        end: parsed_len.min(input.len()),
    })
}

// _remember_phrase_recur, pinyin.cpp:3578–3666.
/// Calls `add` in depth-first order for each complete, nonempty path of `phrase_length`
/// accepted keys. `pronounces` receives the next key and its position in the phrase. Each
/// later probe sees effects of earlier `add` calls. The result is normally `true` if any
/// call to `add` returned `true`. A column that holds both a real key and a separator zero
/// key returns `false`, even if an earlier branch succeeded. The effects of the earlier
/// callbacks remain.
///
/// # Errors
///
/// Propagates the first error from either callback. Earlier callback effects are
/// retained.
pub fn remember_walk<E>(
    columns: &MatrixColumns,
    phrase_length: usize,
    pronounces: &mut dyn FnMut(MatrixKey, usize) -> Result<bool, E>,
    add: &mut dyn FnMut(&[MatrixKey]) -> Result<bool, E>,
) -> Result<bool, E> {
    remember_recur(columns, phrase_length, 0, &mut Vec::new(), pronounces, add)
}

fn remember_recur<E>(
    columns: &MatrixColumns,
    phrase_length: usize,
    start: usize,
    path: &mut Vec<MatrixKey>,
    pronounces: &mut dyn FnMut(MatrixKey, usize) -> Result<bool, E>,
    add: &mut dyn FnMut(&[MatrixKey]) -> Result<bool, E>,
) -> Result<bool, E> {
    if start > columns.end {
        return Ok(false);
    }
    if start == columns.end {
        // `cached_keys->len != phrase_length`, then the empty and
        // `MAX_PHRASE_LENGTH` bounds (`:3600-3609`); the caller's `add`
        // holds the upper bound.
        if path.len() != phrase_length || path.is_empty() {
            return Ok(false);
        }
        return add(path);
    }
    let Some(column) = columns.columns.get(start) else {
        return Ok(false);
    };
    let mut result = false;
    for key in &column.real {
        // `cached_keys->len >= phrase_length`.
        if path.len() >= phrase_length {
            return Ok(false);
        }
        if !pronounces(*key, path.len())? {
            continue;
        }
        path.push(*key);
        let found = remember_recur(columns, phrase_length, key.end(), path, pronounces, add);
        path.pop();
        result = found? || result;
    }
    if let Some(zero_end) = column.zero_end {
        // "assume only one key here for "'" or the last key": the zero key
        // follows the real keys in the column, and `1 != size` drops the
        // result gathered so far.
        if !column.real.is_empty() {
            return Ok(false);
        }
        return remember_recur(columns, phrase_length, zero_end, path, pronounces, add);
    }
    // An empty column also ends here, with `false` and no callback (`size <= 0`:
    // "assume pinyin parsers will filter invalid keys.").
    Ok(result)
}

/// `_get_char_offset_recur` (`pinyin.cpp:3138-3191`, `zhuyin.cpp:2095-2145`): `Ok(Some(length))`
/// is the pin's `true` with `*plength`, `Ok(None)` its `false`.
fn walk<D>(
    dictionary: &D,
    columns: &[Column],
    tokens: &[PhraseToken],
    start: usize,
    offset: usize,
    length: usize,
    zhuyin: bool,
) -> Result<Option<usize>, EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
{
    if start > offset {
        return Ok(Some(length));
    }
    let Some(column) = columns.get(start) else {
        // Unreachable by construction: every raw end is at most the
        // reserved slot, and the reserved slot's zero key ends past
        // `offset`. Answered as the empty-column assert shape.
        return Err(EngineError::PinAssert);
    };
    if column.real.is_empty() && column.zero_end.is_none() {
        return Err(EngineError::PinAssert);
    }
    if !zhuyin && let Some(zero_end) = column.zero_end {
        // "assume only one key here for "'" or the last key."
        if !column.real.is_empty() {
            return Err(EngineError::PinAssert);
        }
        return walk(
            dictionary, columns, tokens, zero_end, offset, length, zhuyin,
        );
    }
    let zero_key = column
        .zero_end
        .map(|end| MatrixKey::from_chewing(crate::parser::ChewingKey::default(), start, end));
    for key in column.real.iter().copied().chain(zero_key) {
        if let Some(found) =
            walk_pronounced_key(dictionary, columns, tokens, key, offset, length, zhuyin)?
        {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn walk_pronounced_key<D>(
    dictionary: &D,
    columns: &[Column],
    tokens: &[PhraseToken],
    key: MatrixKey,
    offset: usize,
    length: usize,
    zhuyin: bool,
) -> Result<Option<usize>, EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
{
    // A missing cached token fails the pronunciation test.
    let Some(&token) = tokens.get(length) else {
        return Ok(None);
    };
    if !key_pronounced_by(dictionary, key, token)? {
        return Ok(None);
    }
    if key.end() > offset {
        return Ok(Some(length));
    }
    walk(
        dictionary,
        columns,
        tokens,
        key.end(),
        offset,
        length + 1,
        zhuyin,
    )
}

/// The character-offset walk over one parse's columns, `None` for the
/// empty matrix ([`matrix_columns`]). `zhuyin` selects the walk that tests
/// zero keys. The answer tells an early refusal, which leaves the caller's
/// output alone, from a walk that found no path, without rereading the
/// DBM.
///
/// # Errors
///
/// [`EngineError::LookupOffsetOutOfRange`] past the reserved slot and
/// [`EngineError::ZeroKeyOffsetCheck`] one past a lone zero-key column
/// (the pin's asserts); [`EngineError::PinAssert`] for the two
/// recursion asserts; the dictionary's backend failure as
/// [`EngineError::Failure`].
pub fn character_offset_walk<D>(
    columns: Option<&MatrixColumns>,
    dictionary: &D,
    phrase: &crate::vocab::PhraseText,
    offset: usize,
    zhuyin: bool,
) -> Result<CharacterOffset, EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
{
    // `0 == matrix.size()`: no parse ran, or the parse placed no key —
    // `fill_matrix` clears the matrix and returns before sizing it when
    // the key vector is empty (`phonetic_key_matrix.cpp:34-38`).
    let Some(MatrixColumns { columns, .. }) = columns else {
        return Ok(CharacterOffset::Refused);
    };
    // The columns run from 0 to the input length inclusive.
    let len = columns.len() - 1;
    if offset > len {
        return Err(EngineError::LookupOffsetOutOfRange { offset, len });
    }
    if offset > 0 && columns.get(offset - 1).is_some_and(Column::lone_zero) {
        return Err(EngineError::ZeroKeyOffsetCheck);
    }
    if phrase.is_empty() {
        return Ok(CharacterOffset::Refused);
    }

    // `_pre_compute_tokens`: the first token of each character's
    // one-character phrase-table search, in order (`pinyin.cpp:3109-3123`).
    // `get_first_token`'s validating `reduce_tokens` asserts on more than
    // four tokens before the empty search's `false` is tested.
    let mut tokens = Vec::with_capacity(phrase.phrase_length());
    for word in phrase.outward_words() {
        let found = dictionary.tokens_for_ucs4(&[word]);
        if found.len() > 4 {
            return Err(EngineError::PinAssert);
        }
        let Some(&token) = found.first() else {
            return Ok(CharacterOffset::Refused);
        };
        tokens.push(token);
    }

    Ok(walk(dictionary, columns, &tokens, 0, offset, 0, zhuyin)?
        .map_or(CharacterOffset::NoPath, CharacterOffset::Found))
}

/// What [`character_offset_walk`] answered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CharacterOffset {
    /// The pin's early `false`, before its walk ran: the empty matrix, an
    /// empty phrase, or a character with no dictionary token. The caller's
    /// output stays untouched.
    Refused,
    /// The walk ran and found no path: the pin's `false` after it stored
    /// the length it reached (`*plength = length`, `pinyin.cpp:3238`).
    NoPath,
    /// A key path pronouncing the phrase's first `n` characters reaches the
    /// offset.
    Found(usize),
}

#[cfg(test)]
mod tests;
