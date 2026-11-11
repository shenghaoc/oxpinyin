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
//!    and [`EngineError::ZeroKeyOffsetCheck`], which the C adapters abort on
//!    (register row 19).
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
//! Two upstream shapes are reported as errors here (register row 19): an
//! empty column at a reached
//! position (`assert(size > 0)`) or a zero key sharing a column with real
//! keys (`assert(1 == size)`) answer [`EngineError::MatrixColumnAssert`],
//! which the C adapters abort on; a walk that runs past the cached tokens
//! (the pin reads `g_array_index` past the array — a phrase shorter than
//! the key path it is measured against) treats the missing token as
//! pronouncing nothing, so that branch fails like a mismatched key — the
//! memory-safety class.
//!
//! [`remember_walk`] is `_remember_phrase_recur` (`pinyin.cpp:3578-3666`)
//! over the same columns, for `pinyin_remember_user_input`: it differs in
//! refusing, not asserting, on an empty column or a zero key that shares its
//! column, and in calling the caller's `add` at every complete path.

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
fn pronounces<D>(dictionary: &D, key: MatrixKey, token: PhraseToken) -> Result<bool, EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: core::fmt::Display,
{
    let Some(syllable) = SyllableKey::from_chewing_key(key.key()) else {
        return Ok(false);
    };
    match dictionary
        .phrase_item_pronunciations(token)
        .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?
    {
        oxpinyin_core::PhraseItemRead::Item(rows) => {
            let query = [key.key()];
            let total = rows
                .iter()
                .fold(0_u32, |sum, (_, freq)| sum.wrapping_add(*freq));
            let matched = rows
                .iter()
                .filter(|(stored, _)| crate::nbest::reading_matches(&query, stored))
                .fold(0_u32, |sum, (_, freq)| sum.wrapping_add(*freq));
            // phrase_index.h:141-162: wrapping guint32 sums, then gfloat
            // division; the caller compares against FLT_EPSILON.
            return Ok(total != 0 && matched as f32 / total as f32 >= f32::EPSILON);
        }
        oxpinyin_core::PhraseItemRead::NoItem => return Ok(false),
        oxpinyin_core::PhraseItemRead::Unsupported => {}
    }
    let key = syllable;
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

/// Whether `token`'s item pronounces `key` with a kept possibility, the
/// pin's `item.get_pronunciation_possibility(&key) >= FLT_EPSILON`: the
/// test both `_get_char_offset_recur` (`pinyin.cpp:3170-3174`) and
/// `_remember_phrase_recur` (`:3640-3645`) apply before a key consumes the
/// next cached token.
///
/// # Errors
///
/// The dictionary's backend failure as [`EngineError::Scoring`].
pub fn key_pronounced_by<D>(
    dictionary: &D,
    key: MatrixKey,
    token: PhraseToken,
) -> Result<bool, EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: core::fmt::Display,
{
    pronounces(dictionary, key, token)
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
/// [`character_offset_with_walk_over_keys`].
#[must_use]
pub fn matrix_columns(
    input: &[u8],
    parsed_len: usize,
    keys: &[MatrixKey],
    separators: bool,
) -> Option<MatrixColumns> {
    if input.is_empty() || keys.is_empty() {
        return None;
    }
    let columns = build_columns(input, parsed_len, keys, separators);
    Some(MatrixColumns {
        columns,
        end: parsed_len.min(input.len()),
    })
}

/// `_remember_phrase_recur` (`pinyin.cpp:3578-3666`) over `columns`, from
/// column 0 with no key chosen.
///
/// A real key is kept only when `pronounces(key, index)` accepts it, where
/// `index` is the number of keys chosen before it, the cached token the key
/// would consume. A column holding a zero key steps on only when that is its
/// sole entry; an empty column refuses. At the parsed length the walk calls
/// `add` once for every complete path of exactly `phrase_length` keys, in the
/// pin's depth-first order and before it explores the next branch, and ORs
/// the answers. `pronounces` is read at the moment of each test, so a write
/// `add` made earlier is seen by the later tests, as in the pin.
///
/// # Errors
///
/// Whatever `pronounces` or `add` return.
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
    // `size <= 0`: "assume pinyin parsers will filter invalid keys."
    if column.real.is_empty() && column.zero_end.is_none() {
        return Ok(false);
    }
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
    Ok(result)
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
    D: Dictionary<Syllable = SyllableKey>,
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
        if !pronounces(dictionary, *key, token)? {
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
/// separator in that mode. The first value is the character count on
/// success, or `None` on graceful refusal (empty matrix, empty or invalid
/// phrase, a character with no dictionary token, or no satisfying path).
/// The second value reports whether the walk ran so the facade can leave
/// the caller's output alone on an early refusal without rereading the DBM.
///
/// # Errors
///
/// [`EngineError::LookupOffsetOutOfRange`] past the reserved slot and
/// [`EngineError::ZeroKeyOffsetCheck`] one past a lone zero-key column
/// (the pin's asserts); [`EngineError::MatrixColumnAssert`] for the two
/// recursion asserts; the dictionary's backend failure as
/// [`EngineError::Scoring`].
pub fn character_offset_with_walk_over_keys<D>(
    input: &[u8],
    parsed_len: usize,
    keys: &[MatrixKey],
    separators: bool,
    dictionary: &D,
    phrase: &oxpinyin_core::PhraseText,
    offset: usize,
) -> Result<(Option<usize>, bool), EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: core::fmt::Display,
{
    // `0 == matrix.size()`: no parse ran, or the parse placed no key —
    // `fill_matrix` clears the matrix and returns before sizing it when
    // the key vector is empty (`phonetic_key_matrix.cpp:34-38`).
    if input.is_empty() || keys.is_empty() {
        return Ok((None, false));
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
        return Ok((None, false));
    }

    // `_pre_compute_tokens`: the first token of each character's
    // one-character phrase-table search, in order (`pinyin.cpp:3109-3123`).
    // `get_first_token`'s validating `reduce_tokens` asserts on more than
    // four tokens before the empty search's `false` is tested.
    let mut tokens = Vec::with_capacity(phrase.phrase_length());
    for word in phrase.outward_words() {
        let found = dictionary.tokens_for_ucs4(&[word]);
        if found.len() > 4 {
            return Err(EngineError::ReduceTokensCount { count: found.len() });
        }
        let Some(&token) = found.first() else {
            return Ok((None, false));
        };
        tokens.push(token);
    }

    walk(dictionary, &columns, &tokens, 0, offset, 0).map(|count| (count, true))
}
