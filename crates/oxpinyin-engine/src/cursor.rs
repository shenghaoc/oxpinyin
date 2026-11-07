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
//! parser placed there plus zero-key entries (`fill_matrix`,
//! `phonetic_key_matrix.cpp:30-83` at 074a2219, read from the source):
//!
//! - Real keys sit at their syllable start — the position after any
//!   apostrophe they cross — with their raw end as the span.
//! - The matrix has `parsed_len + 1` columns, the last a reserved slot
//!   holding a lone zero key. A parse that placed no key returns before
//!   `set_size` (`:37-38`), so the matrix stays empty whatever
//!   `m_parsed_len` is: `'''` consumes 3 bytes and has no column at all,
//!   and the pin's first `get_column_size(1)` asserts
//!   (`phonetic_key_matrix.h:103`). The C adapters abort there before this
//!   model is asked (register row 66).
//! - Every `'` between two keys holds a lone zero key (raw end one past it).
//!   The apostrophes before the first key hold none: the fill loop starts
//!   at the first key's end, so `'ni` has an empty column 0.
//! - No column lies past `parsed_len`, so an early-stopping parse
//!   (`ni2hao` parses 2) has the columns `0..=2` and a read of column 3 or
//!   beyond asserts. This model keeps a zero-key tail run up to the buffer
//!   end, which no C entry reaches: the adapters abort first.
//! - An empty input is a truly empty matrix (no parse ever ran): every
//!   column is empty and no zero keys exist.
//!
//! An earlier version of this header attributed the all-apostrophe and
//! tail-run shapes to a first-hand measurement of the pin; the source does
//! not support them.

use std::ops::Range;

use oxpinyin_core::OptionBits;
use oxpinyin_core::SyllableKey;
use oxpinyin_core::graph::SegmentGraph;

use crate::error::EngineError;
use crate::session::build_scan_matrix;

/// A parse's keys as `(syllable start, raw end)` spans, in input order:
/// the starts never decrease, and keys sharing a start keep the order the
/// parse placed them in — the pin's column order, whose first item is
/// `matrix.get_item(column, 0, ...)`.
///
/// The cursor laws read a span list through this trait so a caller can
/// lend its stored keys without first copying them into a span vector.
pub trait KeySpans {
    /// The number of keys.
    fn span_count(&self) -> usize;

    /// Key `index`'s `(syllable start, raw end)`; `index < span_count()`.
    fn span(&self, index: usize) -> (usize, usize);

    /// The first index whose start is not below `position` — the
    /// partition point of the ordered starts.
    fn first_at_or_after(&self, position: usize) -> usize {
        let (mut low, mut high) = (0, self.span_count());
        while low < high {
            let middle = low + (high - low) / 2;
            if self.span(middle).0 < position {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }
}

impl KeySpans for [(usize, usize)] {
    fn span_count(&self) -> usize {
        self.len()
    }

    fn span(&self, index: usize) -> (usize, usize) {
        self[index]
    }
}

impl KeySpans for [MatrixKey] {
    fn span_count(&self) -> usize {
        self.len()
    }

    fn span(&self, index: usize) -> (usize, usize) {
        let key = self[index];
        (key.syllable_start, key.end)
    }
}

/// One matrix column: the range of real keys placed there, plus the
/// zero-key entry when the position holds one.
struct Column<'a, S: KeySpans + ?Sized> {
    spans: &'a S,
    /// Indexes of the real keys starting at this position, in placement
    /// order — the pin places parse keys before resplit/divided additions,
    /// and `get_right_pinyin_offset` reads the FIRST one.
    real: Range<usize>,
    /// The zero key's raw end (position + 1), when this column holds one.
    zero_end: Option<usize>,
}

impl<S: KeySpans + ?Sized> Column<'_, S> {
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
        if self.real.is_empty() {
            self.zero_end
        } else {
            Some(self.spans.span(self.real.start).1)
        }
    }

    /// Whether any key at this position ends at `end` — the
    /// `get_left_pinyin_offset` scan, which visits every item.
    fn ends_with(&self, end: usize) -> bool {
        self.real
            .clone()
            .any(|index| self.spans.span(index).1 == end)
            || self.zero_end == Some(end)
    }
}

/// The column model for one raw buffer and its parse, read column by
/// column: a column is found by binary search over the ordered spans, so
/// one law costs the pin's own walk times a logarithm, not a pass over the
/// whole buffer.
///
/// `spans` are the parse's keys as (syllable start, raw end) pairs in input
/// order ([`KeySpans`]); `separators` controls the `'` zero-key fill —
/// plain and index-parsed full pinyin consume `'` as the separator, while
/// double pinyin and the zhuyin keyboards never hold a zero-key column
/// ([`crate::session::Session::validate_lookup_offset`]'s taxonomy).
struct Columns<'a, S: KeySpans + ?Sized> {
    input: &'a [u8],
    /// The parsed length, at most the buffer length.
    parsed: usize,
    spans: &'a S,
    separators: bool,
    /// The buffer's leading apostrophe run.
    leading_run: usize,
}

impl<'a, S: KeySpans + ?Sized> Columns<'a, S> {
    fn new(input: &'a [u8], parsed_len: usize, spans: &'a S, separators: bool) -> Self {
        let leading_run = if separators {
            input.iter().take_while(|byte| **byte == b'\'').count()
        } else {
            0
        };
        Self {
            input,
            parsed: parsed_len.min(input.len()),
            spans,
            separators,
            leading_run,
        }
    }

    /// The column at `position`, or `None` past the reserved slot at the
    /// buffer end.
    fn get(&self, position: usize) -> Option<Column<'a, S>> {
        if position > self.input.len() {
            return None;
        }
        let low = self.spans.first_at_or_after(position);
        let high = position
            .checked_add(1)
            .map_or(self.spans.span_count(), |next| {
                self.spans.first_at_or_after(next)
            });
        Some(Column {
            spans: self.spans,
            real: low..high,
            zero_end: self.zero_end(position),
        })
    }

    /// The column at `position`; the laws only ask within the buffer, and
    /// past it every column is empty.
    fn at(&self, position: usize) -> Column<'a, S> {
        self.get(position).unwrap_or(Column {
            spans: self.spans,
            real: 0..0,
            zero_end: None,
        })
    }

    /// The zero key's raw end at `position`, if the column holds one.
    fn zero_end(&self, position: usize) -> Option<usize> {
        // An empty input is a truly empty matrix: no parse ever ran.
        if self.input.is_empty() {
            return None;
        }
        // The zero-key tail run: the unconsumed tail plus the reserved slot.
        if position >= self.parsed {
            return Some(position + 1);
        }
        // The leading apostrophe run holds zero keys only when the parse
        // contains no real keys at all (the all-apostrophe shape);
        // otherwise the pin's DP propagates over it.
        (self.separators
            && self.input[position] == b'\''
            && (position >= self.leading_run || self.spans.span_count() == 0))
            .then_some(position + 1)
    }
}

/// The pin's `_check_offset` (`pinyin.cpp:2163-2180`), its abort answered
/// as an error.
fn check<S: KeySpans + ?Sized>(columns: &Columns<'_, S>, offset: usize) -> Result<(), EngineError> {
    if offset > 0
        && columns
            .get(offset - 1)
            .is_some_and(|column| column.lone_zero())
    {
        return Err(EngineError::ZeroKeyOffsetCheck { offset });
    }
    Ok(())
}

/// The pin's `_compute_zero_start` (`pinyin.cpp:2985-3004`): walk back
/// over consecutive lone zero-key columns, stopping before column 0.
fn zero_start_walk<S: KeySpans + ?Sized>(columns: &Columns<'_, S>, mut offset: usize) -> usize {
    let mut index = offset.saturating_sub(1);
    while index > 0 {
        if columns.at(index).lone_zero() {
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
const fn range_check(input_len: usize, offset: usize) -> Result<(), EngineError> {
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
    spans: &(impl KeySpans + ?Sized),
    separators: bool,
    cursor: usize,
) -> Result<usize, EngineError> {
    let columns = Columns::new(input, parsed_len, spans, separators);
    let mut offset = cursor.min(parsed_len.min(input.len()));
    while offset > 0 && columns.at(offset).is_empty() {
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
    spans: &(impl KeySpans + ?Sized),
    separators: bool,
    offset: usize,
) -> Result<usize, EngineError> {
    let columns = Columns::new(input, parsed_len, spans, separators);
    range_check(input.len(), offset)?;
    check(&columns, offset)?;
    let mut left = offset.saturating_sub(1);
    while left > 0 {
        if columns.at(left).ends_with(offset) {
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
    spans: &(impl KeySpans + ?Sized),
    separators: bool,
    offset: usize,
) -> Result<Option<usize>, EngineError> {
    let columns = Columns::new(input, parsed_len, spans, separators);
    range_check(input.len(), offset)?;
    check(&columns, offset)?;
    let mut right = offset;
    // Skip consecutive lone zero-key columns — the pin's loop runs below
    // `matrix.size() - 1`, so the reserved-slot column is never skipped.
    let matrix_size = input.len() + 1;
    let mut index = right;
    while index + 1 < matrix_size {
        let column = columns.at(index);
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
    let Some(end) = columns.at(right).first_end() else {
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
    lookup_offset_over_spans(input, parsed, spans.as_slice(), true, cursor)
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
    left_word_offset_over_spans(input, parsed, spans.as_slice(), true, offset)
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
    right_word_offset_over_spans(input, parsed, spans.as_slice(), true, offset)
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
    key: oxpinyin_core::ChewingKey,
    syllable_start: usize,
    end: usize,
}

impl MatrixKey {
    /// Builds a key from its syllable, tone, and raw span — for callers
    /// (the facade's non-full-pinyin parse modes) whose keys come from a
    /// parse other than the session's own scan matrix.
    #[must_use]
    pub fn new(key: SyllableKey, tone: u8, syllable_start: usize, end: usize) -> Self {
        Self::from_chewing(key.chewing_key().with_tone(tone), syllable_start, end)
    }

    /// Retains the parser key and its original span.
    #[must_use]
    pub const fn from_chewing(
        key: oxpinyin_core::ChewingKey,
        syllable_start: usize,
        end: usize,
    ) -> Self {
        Self {
            key,
            syllable_start,
            end,
        }
    }

    /// The syllable this key matched.
    #[must_use]
    pub const fn key(self) -> oxpinyin_core::ChewingKey {
        self.key
    }

    /// The tone consumed with this key under `USE_TONE`, or 0.
    #[must_use]
    pub const fn tone(self) -> u8 {
        self.key.tone
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
/// The same walk [`matrix_spans`] projects: keys appear in column order
/// (by syllable start, [`in_column_order`]), and within a column in the
/// matrix's own order, so the first key of a column is the pin's
/// `matrix.get_item(column, 0, ...)`.
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
    let keys = matrix
        .iter()
        .flatten()
        .map(|key| MatrixKey::new(key.key, key.tone, key.syllable_start, key.to))
        .collect();
    Ok((in_column_order(keys), parsed))
}

/// Orders scan-matrix keys as the pin's matrix holds them: by syllable
/// start, the column the pin places each key in, keeping the scan's order
/// within a column (a stable sort). The scan matrix files keys by the
/// graph node they leave, and a node's syllable start never decreases
/// with the node, so this only confirms the order [`KeySpans`] relies on.
#[must_use]
pub(crate) fn in_column_order(mut keys: Vec<MatrixKey>) -> Vec<MatrixKey> {
    if !keys.is_sorted_by_key(|key| key.syllable_start) {
        keys.sort_by_key(|key| key.syllable_start);
    }
    keys
}

#[cfg(test)]
mod tests;
