//! Cursor → lookup-offset normalization and word-level cursor moves.
//!
//! The pinned laws, ported from the pin's matrix walks:
//!
//! - `pinyin_get_pinyin_offset` (`pinyin.cpp:3008-3027` at the pin): clamp
//!   the cursor to the parsed length, walk back to the nearest non-empty
//!   matrix column, extend back over the zero-key run before it
//!   (`_compute_zero_start`, `pinyin.cpp:2985-3004`), then validate with
//!   `_check_offset` (`pinyin.cpp:2163-2180`).
//! - `pinyin_get_left_pinyin_offset` (`pinyin.cpp:3029-3059`): validate the
//!   caller offset, walk back to the column holding a key that ENDS at the
//!   offset (the syllable start), zero-start-walk the result, validate it.
//! - `pinyin_get_right_pinyin_offset` (`pinyin.cpp:3061-3094`): validate the
//!   caller offset, skip a leading run of lone zero keys, answer `false`
//!   when no key starts at the position (the pin's one graceful false,
//!   `pinyin.cpp:3085-3086`), otherwise answer the first key's raw end and
//!   validate the result.
//!
//! The pin validates with `_check_offset`, which ABORTS when the position
//! before the examined offset is a lone zero key (`assert(zero_key != key)`,
//! `pinyin.cpp:2175`); the engine answers [`EngineError::ZeroKeyOffsetCheck`]
//! instead and the C surface returns `false` (the no-abort policy —
//! `docs/findings/upstream-divergences.md`).
//!
//! ## The column model
//!
//! The pin's `PhoneticKeyMatrix` holds, per byte position, the keys the
//! parser placed there plus zero-key entries. Measured first-hand on the
//! rebuilt pin (fork-per-probe, every offset in its own child so an abort
//! is a datum):
//!
//! - Real keys sit at their syllable start — the position after any
//!   apostrophe they cross — with their raw end as the span.
//! - Every `'` separator strictly inside the parsed span holds a lone zero
//!   key (raw end one past it), EXCEPT the buffer's leading apostrophe run
//!   when the parse contains real keys — the pin's DP propagates over a
//!   leading run without placing zero keys there (`'ni` has an empty
//!   column 0; an all-apostrophe parse like `'''` places zero keys at
//!   every consumed position instead).
//! - A zero-key tail run fills every position from the parsed length to
//!   the buffer end INCLUSIVE — the unconsumed tail of an early-stopping
//!   parse (`ni2hao` zeros columns 2..=6) and the pin's reserved extra
//!   slot at the buffer end. The tail zero is what makes
//!   `get_right_pinyin_offset(parsed end)` abort on the pin: the zero's
//!   raw end is one past the buffer, and the second `_check_offset` sees
//!   the tail zero column.
//! - An empty input is a truly empty matrix (no parse ever ran): every
//!   column is empty and no zero keys exist.

use oxpinyin_core::OptionBits;
use oxpinyin_core::SyllableKey;
use oxpinyin_core::graph::SegmentGraph;

use crate::error::EngineError;
use crate::session::build_scan_matrix;

/// One matrix column: the raw ends of the real keys placed there, plus the
/// zero-key entry when the position holds one.
#[derive(Default)]
struct Column {
    /// Raw ends of the real keys starting at this position, in placement
    /// order — the pin places parse keys before resplit/divided additions,
    /// and `get_right_pinyin_offset` reads the FIRST one.
    real: Vec<usize>,
    /// The zero key's raw end (position + 1), when this column holds one.
    zero_end: Option<usize>,
}

impl Column {
    /// The pin's `get_column_size`.
    fn size(&self) -> usize {
        self.real.len() + usize::from(self.zero_end.is_some())
    }

    fn is_empty(&self) -> bool {
        self.real.is_empty() && self.zero_end.is_none()
    }

    /// The `_check_offset` shape: exactly one entry, and it is the zero key.
    fn lone_zero(&self) -> bool {
        self.real.is_empty() && self.zero_end.is_some()
    }

    /// The pin's `get_item(column, 0).m_raw_end` — real keys first.
    fn first_end(&self) -> Option<usize> {
        self.real.first().copied().or(self.zero_end)
    }

    /// Whether any key at this position ends at `end` — the
    /// `get_left_pinyin_offset` scan, which visits every item.
    fn ends_with(&self, end: usize) -> bool {
        self.real.contains(&end) || self.zero_end == Some(end)
    }
}

/// Builds the column model for one raw buffer and its parse.
///
/// `spans` are the parse's keys as (syllable start, raw end) pairs;
/// `separators` controls the `'` zero-key fill — plain and index-parsed
/// full pinyin consume `'` as the separator, while double pinyin and the
/// zhuyin keyboards never hold a zero-key column
/// ([`crate::session::Session::validate_lookup_offset`]'s taxonomy).
fn build_columns(
    input: &[u8],
    parsed_len: usize,
    spans: &[(usize, usize)],
    separators: bool,
) -> Vec<Column> {
    let bound = input.len();
    let mut columns: Vec<Column> = (0..=bound).map(|_| Column::default()).collect();
    for &(start, end) in spans {
        if let Some(column) = columns.get_mut(start) {
            column.real.push(end);
        }
    }
    if input.is_empty() {
        return columns;
    }
    let parsed = parsed_len.min(bound);
    if separators {
        let leading_run = input.iter().take_while(|byte| **byte == b'\'').count();
        for position in 0..parsed {
            // The leading apostrophe run holds zero keys only when the
            // parse contains no real keys at all (the all-apostrophe
            // shape); otherwise the pin's DP propagates over it.
            if input[position] == b'\'' && (position >= leading_run || spans.is_empty()) {
                columns[position].zero_end = Some(position + 1);
            }
        }
    }
    // The zero-key tail run: the unconsumed tail plus the reserved slot.
    for (position, column) in columns.iter_mut().enumerate().skip(parsed) {
        column.zero_end = Some(position + 1);
    }
    columns
}

/// The pin's `_check_offset` (`pinyin.cpp:2163-2180`), its abort answered
/// as an error.
fn check(columns: &[Column], offset: usize) -> Result<(), EngineError> {
    if offset > 0 && columns.get(offset - 1).is_some_and(Column::lone_zero) {
        return Err(EngineError::ZeroKeyOffsetCheck { offset });
    }
    Ok(())
}

/// The pin's `_compute_zero_start` (`pinyin.cpp:2985-3004`): walk back
/// over consecutive lone zero-key columns, stopping before column 0.
fn zero_start_walk(columns: &[Column], mut offset: usize) -> usize {
    let mut index = offset.saturating_sub(1);
    while index > 0 {
        if columns[index].lone_zero() {
            offset = index;
            index -= 1;
        } else {
            break;
        }
    }
    offset
}

/// The range half shared with [`crate::check_lookup_offset_range`]: the pin
/// reads its matrix out of bounds past one-past-end, so no pinned behaviour
/// exists there.
fn range_check(input_len: usize, offset: usize) -> Result<(), EngineError> {
    if offset > input_len {
        return Err(EngineError::LookupOffsetOutOfRange {
            offset,
            len: input_len,
        });
    }
    Ok(())
}

/// Normalizes a user cursor position to a lookup offset — the
/// `pinyin_get_pinyin_offset` law over one raw buffer and its parse.
///
/// Clamps the cursor to the parsed length, walks back to the nearest
/// non-empty matrix column, extends back over the zero-key run before it,
/// and validates. The pin never answers false here — it aborts on the
/// validation shape instead, answered as [`EngineError::ZeroKeyOffsetCheck`].
///
/// # Errors
///
/// Returns [`EngineError`] when the cursor cannot be resolved against the input and spans.
pub fn lookup_offset_over_spans(
    input: &[u8],
    parsed_len: usize,
    spans: &[(usize, usize)],
    separators: bool,
    cursor: usize,
) -> Result<usize, EngineError> {
    let columns = build_columns(input, parsed_len, spans, separators);
    let mut offset = cursor.min(parsed_len.min(input.len()));
    while offset > 0 && columns[offset].is_empty() {
        offset -= 1;
    }
    let offset = zero_start_walk(&columns, offset);
    check(&columns, offset)?;
    Ok(offset)
}

/// The word-level left move — the `pinyin_get_left_pinyin_offset` law.
///
/// Finds the start of the syllable ending at `offset` (0 when no key ends
/// there), zero-start-walks the result, and validates both offsets. The pin
/// never answers false here — validation aborts are
/// [`EngineError::ZeroKeyOffsetCheck`].
///
/// # Errors
///
/// Returns [`EngineError`] when the offset cannot be resolved against the input and spans.
pub fn left_word_offset_over_spans(
    input: &[u8],
    parsed_len: usize,
    spans: &[(usize, usize)],
    separators: bool,
    offset: usize,
) -> Result<usize, EngineError> {
    let columns = build_columns(input, parsed_len, spans, separators);
    range_check(input.len(), offset)?;
    check(&columns, offset)?;
    let mut left = offset.saturating_sub(1);
    while left > 0 {
        if columns[left].ends_with(offset) {
            break;
        }
        left -= 1;
    }
    let left = zero_start_walk(&columns, left);
    check(&columns, left)?;
    Ok(left)
}

/// The word-level right move — the `pinyin_get_right_pinyin_offset` law.
///
/// `Ok(None)` is the pin's one graceful false: no key starts at the
/// (zero-run-skipped) position (`pinyin.cpp:3085-3086`). Validation aborts
/// are [`EngineError::ZeroKeyOffsetCheck`].
///
/// # Errors
///
/// Returns [`EngineError`] when the offset cannot be resolved against the input and spans.
pub fn right_word_offset_over_spans(
    input: &[u8],
    parsed_len: usize,
    spans: &[(usize, usize)],
    separators: bool,
    offset: usize,
) -> Result<Option<usize>, EngineError> {
    let columns = build_columns(input, parsed_len, spans, separators);
    range_check(input.len(), offset)?;
    check(&columns, offset)?;
    let mut right = offset;
    // Skip consecutive lone zero-key columns — the pin's loop runs below
    // `matrix.size() - 1`, so the reserved-slot column is never skipped.
    let matrix_size = input.len() + 1;
    let mut index = right;
    while index + 1 < matrix_size {
        let column = &columns[index];
        if column.size() != 1 {
            break;
        }
        if column.lone_zero() {
            right = index + 1;
            index += 1;
        } else {
            break;
        }
    }
    let Some(end) = columns[right].first_end() else {
        return Ok(None);
    };
    check(&columns, end)?;
    Ok(Some(end))
}

/// [`lookup_offset_over_spans`] over the plain full-pinyin scan matrix.
///
/// # Errors
///
/// Returns [`EngineError`] when the cursor cannot be resolved against the input.
pub fn lookup_offset_for_cursor(
    input: &[u8],
    options: OptionBits,
    cursor: usize,
) -> Result<usize, EngineError> {
    let (spans, parsed) = matrix_spans(input, options)?;
    lookup_offset_over_spans(input, parsed, &spans, true, cursor)
}

/// [`left_word_offset_over_spans`] over the plain full-pinyin scan matrix.
///
/// # Errors
///
/// Returns [`EngineError`] when the offset cannot be resolved against the input.
pub fn left_word_offset(
    input: &[u8],
    options: OptionBits,
    offset: usize,
) -> Result<usize, EngineError> {
    let (spans, parsed) = matrix_spans(input, options)?;
    left_word_offset_over_spans(input, parsed, &spans, true, offset)
}

/// [`right_word_offset_over_spans`] over the plain full-pinyin scan matrix.
///
/// # Errors
///
/// Returns [`EngineError`] when the offset cannot be resolved against the input.
pub fn right_word_offset(
    input: &[u8],
    options: OptionBits,
    offset: usize,
) -> Result<Option<usize>, EngineError> {
    let (spans, parsed) = matrix_spans(input, options)?;
    right_word_offset_over_spans(input, parsed, &spans, true, offset)
}

/// The scan matrix's keys as (syllable start, raw end) spans plus the
/// graph's consumed length.
///
/// Columns are indexed by the key's syllable start — the position after any
/// apostrophe a crossing key rides over — matching the pin's placement.
fn matrix_spans(
    input: &[u8],
    options: OptionBits,
) -> Result<(Vec<(usize, usize)>, usize), EngineError> {
    let (keys, parsed) = matrix_keys(input, options)?;
    let spans = keys
        .iter()
        .map(|key| (key.syllable_start(), key.end()))
        .collect();
    Ok((spans, parsed))
}

/// One scan-matrix key with the raw byte span it occupies.
///
/// The C ABI's `ChewingKey` / `ChewingKeyRest` pair, in engine terms:
/// [`MatrixKey::key`] and [`MatrixKey::tone`] are what
/// `pinyin_get_pinyin_string` and its siblings render, and
/// [`MatrixKey::syllable_start`] / [`MatrixKey::end`] are the pin's
/// `ChewingKeyRest::m_raw_begin` / `m_raw_end`.
///
/// The start is the key's own syllable start, NOT the graph node it
/// leaves: an apostrophe separator rides on the edge that follows it, so
/// the node can sit one byte earlier. The pin places a consumed `'` in its
/// own zero-key column and begins the following key's rest after it
/// (`pinyin_parser2.cpp:282` sets `m_raw_begin` over the one-pinyin
/// substring), so the syllable start is the value that matches.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MatrixKey {
    key: SyllableKey,
    tone: u8,
    syllable_start: usize,
    end: usize,
}

impl MatrixKey {
    /// The syllable this key matched.
    #[must_use]
    pub const fn key(self) -> SyllableKey {
        self.key
    }

    /// The tone consumed with this key under `USE_TONE`, or 0.
    #[must_use]
    pub const fn tone(self) -> u8 {
        self.tone
    }

    /// Byte offset where the key's own text begins — the pin's
    /// `ChewingKeyRest::m_raw_begin`.
    #[must_use]
    pub const fn syllable_start(self) -> usize {
        self.syllable_start
    }

    /// Byte offset one past the key's last byte — the pin's
    /// `ChewingKeyRest::m_raw_end`.
    #[must_use]
    pub const fn end(self) -> usize {
        self.end
    }

    /// Raw byte length — the pin's `ChewingKeyRest::length()`.
    #[must_use]
    pub const fn len(self) -> usize {
        self.end - self.syllable_start
    }

    /// Whether the key covers no bytes.
    ///
    /// The scan matrix never holds one; the accessor exists so callers can
    /// check spans they build themselves.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.syllable_start == self.end
    }
}

/// The scan matrix's keys with their spans, plus the graph's consumed
/// length.
///
/// The same walk [`matrix_spans`] projects: keys appear in column order,
/// and within a column in the matrix's own order, so the first key of a
/// column is the pin's `matrix.get_item(column, 0, ...)`.
///
/// # Errors
///
/// [`EngineError::Graph`] when the buffer cannot be built into a segment
/// graph.
pub fn matrix_keys(
    input: &[u8],
    options: OptionBits,
) -> Result<(Vec<MatrixKey>, usize), EngineError> {
    let graph = SegmentGraph::build_with_options(input, options)?;
    let parsed = graph.consumed();
    let matrix = build_scan_matrix(&graph, options, true);
    let mut keys = Vec::new();
    for column in &matrix {
        for key in column {
            keys.push(MatrixKey {
                key: key.key,
                tone: key.tone,
                syllable_start: key.syllable_start,
                end: key.to,
            });
        }
    }
    Ok((keys, parsed))
}

#[cfg(test)]
mod tests;
