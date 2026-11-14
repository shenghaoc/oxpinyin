//! The cursor/offset navigation laws over the active parse mode's own
//! coordinates.
//!
//! The cursor → lookup-offset normalization and the word-level left/right
//! moves port the pin's matrix laws over the engine's positional data —
//! `oxpinyin_engine::lookup_offset_over_spans` and the `*_word_offset`
//! pair. Where the pin's `_check_offset` aborts, these answer an error
//! for the C layers to act on.
//!
//! Parse-mode dispatch mirrors [`InstanceView::validate_lookup_offset`]:
//! plain full pinyin runs the law over the session's own buffer; LUOMA /
//! `SECONDARY_ZHUYIN` run it over the stored original input with the
//! index parse's key spans (the pinned index parse consumes `'` as the
//! same separator); double pinyin and the zhuyin keyboards hold no
//! zero-key columns, so the law steps their parse's key spans only.

use oxpinyin_core::{DoublePinyinKey, FullPinyinIndexKey, ZhuyinKey};
use oxpinyin_engine::{EngineError, KeySpans, MatrixKey};

use crate::instance::{InstanceState, InstanceView};

/// One original-coordinate ABI column's first item: the packed key and its
/// raw begin and end. A packed `0` is a zero key (or a key with no
/// `ChewingKey` spelling, which the pin's tests treat alike).
type AbiItem = (u16, usize, usize);

/// The active parse mode's span source: the coordinate input bytes, its
/// parsed length, the key spans `(start, end)`, and whether `'` is a
/// zero-key separator in that mode.
///
/// `None` for plain full pinyin, whose
/// law runs over the session's own buffer.
pub struct SpanSource<'a> {
    /// The mode's own input buffer.
    pub input: &'a [u8],
    /// The parse's consumed byte count.
    pub parsed: usize,
    /// The parse's keys, read in place in input order; their
    /// [`KeySpans`] are the original-coordinate spans.
    pub spans: ModeKeys<'a>,
    /// Whether `'` is a zero-key separator column in this mode.
    pub separators: bool,
}

/// The active parse mode's keys, borrowed from the parse that holds them
/// and read by index, in input order (ordered by begin, the pin's column
/// order). Every getter reads them in place, so a getter costs a binary
/// search instead of a copy of the key list.
#[derive(Clone, Copy, Debug)]
pub enum ModeKeys<'a> {
    /// A zhuyin keyboard parse.
    Zhuyin(&'a [ZhuyinKey]),
    /// A double-pinyin parse.
    Double(&'a [DoublePinyinKey]),
    /// A LUOMA / `SECONDARY_ZHUYIN` full-pinyin index parse.
    Full(&'a [FullPinyinIndexKey]),
    /// The parse-time matrix a scheme parse keeps (`parse_matrix`).
    Matrix(&'a [MatrixKey]),
    /// The matrix the plain full-pinyin session's last parse filled.
    Session(oxpinyin_engine::MatrixKeys<'a>),
}

impl ModeKeys<'_> {
    /// The number of keys.
    #[must_use]
    pub const fn len(&self) -> usize {
        match self {
            Self::Zhuyin(keys) => keys.len(),
            Self::Double(keys) => keys.len(),
            Self::Full(keys) => keys.len(),
            Self::Matrix(keys) => keys.len(),
            Self::Session(keys) => keys.len(),
        }
    }

    /// Whether the parse holds no key.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Key `index` (`index < len()`) as the getters render it.
    #[must_use]
    pub fn get(&self, index: usize) -> KeyAt {
        match self {
            Self::Zhuyin(keys) => {
                let key = &keys[index];
                KeyAt {
                    text: key.key().text(),
                    chewing: key.key().chewing_key().with_tone(key.tone()),
                    key: Some(key.key()),
                    tone: key.tone(),
                    begin: key.start(),
                    end: key.end(),
                }
            }
            Self::Double(keys) => {
                let key = keys[index];
                KeyAt {
                    text: key.key().text(),
                    chewing: key.key().chewing_key().with_tone(key.tone()),
                    key: Some(key.key()),
                    tone: key.tone(),
                    begin: key.start(),
                    end: key.end(),
                }
            }
            Self::Full(keys) => {
                let key = &keys[index];
                KeyAt {
                    text: key.canonical(),
                    chewing: oxpinyin_core::ChewingKey::from_pinyin(key.canonical())
                        .unwrap_or_default()
                        .with_tone(key.tone()),
                    key: None,
                    tone: key.tone(),
                    begin: key.start(),
                    end: key.end(),
                }
            }
            Self::Matrix(keys) => matrix_key_at(keys[index]),
            Self::Session(keys) => matrix_key_at(keys.get(index)),
        }
    }

    /// Every key, in input order.
    pub fn iter(&self) -> impl Iterator<Item = KeyAt> + '_ {
        (0..self.len()).map(|index| self.get(index))
    }
}

impl KeySpans for ModeKeys<'_> {
    fn span_count(&self) -> usize {
        self.len()
    }

    fn span(&self, index: usize) -> (usize, usize) {
        // The binary searches read spans only; skip the spelling lookup.
        match self {
            Self::Zhuyin(keys) => (keys[index].start(), keys[index].end()),
            Self::Double(keys) => (keys[index].start(), keys[index].end()),
            Self::Full(keys) => (keys[index].start(), keys[index].end()),
            Self::Matrix(keys) => (keys[index].syllable_start(), keys[index].end()),
            Self::Session(keys) => keys.span(index),
        }
    }
}

/// The ABI's original-coordinate column table, read column by column over
/// the mode's ordered keys rather than built: a real first item, a zero
/// key, or empty. Columns run from 0 to the parsed length inclusive.
struct AbiColumns<'a> {
    keys: ModeKeys<'a>,
    input: &'a [u8],
    separators: bool,
    parsed_len: usize,
}

impl AbiColumns<'_> {
    /// The table's length: the parsed length plus the reserved slot.
    const fn len(&self) -> usize {
        self.parsed_len + 1
    }

    /// The column at `position`; `None` for an empty column or one past
    /// the table.
    fn get(&self, position: usize) -> Option<AbiItem> {
        if position > self.parsed_len {
            return None;
        }
        // The first key placed at its begin owns the column.
        let index = self.keys.first_at_or_after(position);
        if index < self.keys.len() {
            let key = self.keys.get(index);
            if key.begin == position {
                // The Full index variant has only canonical text: its key
                // converts by spelling, as before.
                let packed = key.chewing.to_packed();
                return Some((packed, key.begin, key.end));
            }
        }
        if self.keys.is_empty() {
            return None;
        }
        // Zero columns fill from the parsed length on, and at separators
        // at or after the first key when `'` is a separator.
        let zero = position == self.parsed_len
            || (self.separators
                && self.input.get(position) == Some(&b'\'')
                && position >= self.keys.span(0).0);
        zero.then_some((0, position, position + 1))
    }

    /// Whether the column at `position` exists and its first item packs
    /// to `0` — the pin's zero-column test.
    fn packs_zero(&self, position: usize) -> bool {
        self.get(position).is_some_and(|item| item.0 == 0)
    }
}

/// One matrix key at an offset: its canonical pinyin spelling, its tone,
/// and its raw span.
///
/// The spelling because all renderers want text, and because the LUOMA /
/// `SECONDARY_ZHUYIN` index parse carries a canonical spelling rather than
/// a vocabulary key; the vocabulary key rides beside it where the parse has
/// one.
#[derive(Clone, Copy, Debug)]
pub struct KeyAt {
    /// The exact parser key.
    pub chewing: oxpinyin_core::ChewingKey,
    /// The key's canonical full-pinyin spelling.
    pub text: &'static str,
    /// The key's vocabulary key; `None` for the index parse, which carries
    /// only canonical text.
    pub key: Option<oxpinyin_core::SyllableKey>,
    /// The tone consumed with the key, `0` when toneless.
    pub tone: u8,
    /// Inclusive byte offset of the key's first byte.
    pub begin: usize,
    /// Exclusive byte offset one past the key's last byte.
    pub end: usize,
}

impl<S: std::borrow::Borrow<InstanceState>, E: std::borrow::Borrow<oxpinyin_engine::SessionState>>
    InstanceView<'_, S, E>
{
    /// The mode dispatch shared by the three offset laws: zhuyin, then
    /// double pinyin, then the LUOMA / `SECONDARY_ZHUYIN` full-pinyin
    /// index — the same precedence as
    /// [`InstanceView::validate_lookup_offset`], and the union of the two
    /// facades' chains (a facade that never populates a parse state never
    /// takes its branch). Zhuyin and double pinyin hold no zero-key
    /// columns (`separators` false); the index parse consumes `'` as a
    /// separator (`separators` true). Plain full pinyin answers `None`.
    #[must_use]
    pub fn span_source(&self) -> Option<SpanSource<'_>> {
        if let Some(parse) = self.zhuyin_parse.as_ref() {
            return Some(SpanSource {
                input: self.zhuyin_input.as_bytes(),
                parsed: parse.consumed(),
                spans: ModeKeys::Matrix(&self.parse_matrix),
                separators: false,
            });
        }
        if let Some(parse) = self.double_parse.as_ref() {
            return Some(SpanSource {
                input: self.double_input.as_bytes(),
                parsed: parse.consumed(),
                spans: ModeKeys::Matrix(&self.parse_matrix),
                separators: false,
            });
        }
        self.full_parse.as_ref().map(|parse| SpanSource {
            input: self.full_input.as_bytes(),
            parsed: parse.consumed(),
            spans: ModeKeys::Matrix(&self.parse_matrix),
            separators: true,
        })
    }

    /// The cursor → lookup-offset law in the instance's active parse
    /// mode.
    ///
    /// Plain full pinyin walks the session's own scan matrix; the
    /// index-parsed schemes walk the index parse's key spans over the
    /// stored original input; double pinyin and zhuyin hold no zero-key
    /// columns and step the parse's key spans in original coordinates.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] where the pin aborts.
    pub fn lookup_offset(&self, cursor: usize) -> Result<usize, EngineError> {
        match self.span_source() {
            Some(source) => oxpinyin_engine::lookup_offset_over_spans(
                source.input,
                source.parsed,
                &source.spans,
                source.separators,
                cursor,
            ),
            None => self.session.lookup_offset_for_cursor(cursor),
        }
    }

    /// The word-level left-move law in the instance's active parse mode —
    /// [`Self::lookup_offset`]'s mode dispatch applied to the engine's
    /// `left_word_offset` law.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] where the pin aborts.
    pub fn left_offset(&self, offset: usize) -> Result<usize, EngineError> {
        self.abi_left_offset(offset, false)
    }

    fn left_offset_impl(&self, offset: usize) -> Result<usize, EngineError> {
        match self.span_source() {
            Some(source) => oxpinyin_engine::left_word_offset_over_spans(
                source.input,
                source.parsed,
                &source.spans,
                source.separators,
                offset,
            ),
            None => self.session.left_word_offset(offset),
        }
    }

    /// The word-level right-move law in the instance's active parse mode.
    /// `Ok(None)` is the pin's one graceful false: no key starts at the
    /// (zero-run-skipped) position.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] where the pin aborts.
    pub fn right_offset(&self, offset: usize) -> Result<Option<usize>, EngineError> {
        match self.span_source() {
            Some(source) => oxpinyin_engine::right_word_offset_over_spans(
                source.input,
                source.parsed,
                &source.spans,
                source.separators,
                offset,
            ),
            None => self.session.right_word_offset(offset),
        }
    }

    /// The active parse mode's input buffer and whether `'` is a zero-key
    /// separator in that mode — the same dispatch [`Self::span_source`]
    /// and [`InstanceView::validate_lookup_offset`] make. It reads no
    /// matrix, so a caller holding the key list from an earlier
    /// [`Self::mode_key_list`] can take the buffer again after mutating
    /// the session.
    #[must_use]
    pub fn mode_input(&self) -> (&[u8], bool) {
        if self.zhuyin_parse.is_some() {
            (self.zhuyin_input.as_bytes(), false)
        } else if self.double_parse.is_some() {
            (self.double_input.as_bytes(), false)
        } else if self.full_parse.is_some() {
            (self.full_input.as_bytes(), true)
        } else {
            (self.session.raw_input().as_bytes(), true)
        }
    }

    /// The active parse mode's keys, copied out of
    /// [`Self::mode_key_source`].
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from the session's matrix read.
    pub fn mode_key_list(&self) -> Result<Vec<KeyAt>, EngineError> {
        Ok(self.mode_key_source()?.iter().collect())
    }

    /// Whether the active parse placed a key in a non-empty input — the
    /// pin's non-empty matrix (`fill_matrix`, `phonetic_key_matrix.cpp:34-38`),
    /// answered without copying the keys.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from the session's matrix read.
    pub fn mode_has_keys(&self) -> Result<bool, EngineError> {
        let keys = self.mode_key_source()?;
        Ok(!(self.mode_input().0.is_empty() || keys.is_empty()))
    }

    /// The bytes the active parse consumed: the pin's `m_parsed_len`
    /// (`pinyin.cpp:1511`, `zhuyin.cpp:1034`), which every parse stores and
    /// `pinyin_reset` zeroes. Apostrophes count, keys or not, so `'''`
    /// consumes 3 with an empty matrix.
    #[must_use]
    pub fn pin_parsed_len(&self) -> usize {
        self.parsed_len
    }

    /// The pin matrix's column count: `parsed_len + 1` once the parse placed
    /// a key (`fill_matrix` sizes it so, with one reserved slot), and 0
    /// while it is empty (`phonetic_key_matrix.cpp:34-38`). The pin's
    /// `PhoneticTable::get_column_size` asserts `index < columns`
    /// (`phonetic_key_matrix.h:103`), so a read at or past this count aborts.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from the session's matrix read.
    pub fn pin_matrix_columns(&self) -> Result<usize, EngineError> {
        if self.mode_has_keys()? {
            Ok(self.pin_parsed_len() + 1)
        } else {
            Ok(0)
        }
    }

    /// The keys the active parser itself placed, before the pin's
    /// post-fill steps: the input of `parse_matrix`'s construction.
    /// Empty for plain full pinyin, which keeps no parser keys.
    pub(crate) fn parser_key_list(&self) -> Vec<KeyAt> {
        if let Some(parse) = self.zhuyin_parse.as_ref() {
            ModeKeys::Zhuyin(parse.keys()).iter().collect()
        } else if let Some(parse) = self.double_parse.as_ref() {
            ModeKeys::Double(parse.keys()).iter().collect()
        } else if let Some(parse) = self.full_parse.as_ref() {
            ModeKeys::Full(parse.keys()).iter().collect()
        } else {
            Vec::new()
        }
    }

    /// The active parse mode's matrix keys in place, in input order. Plain
    /// full pinyin reads the session's scan-matrix keys, which the session
    /// builds once per input and option word; the index-parsed full pinyin,
    /// double pinyin and chewing modes read the matrix kept with the parse
    /// (`parse_matrix`), with the pin's fuzzy, resplit and divided additions.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from the session's matrix read.
    pub fn mode_key_source(&self) -> Result<ModeKeys<'_>, EngineError> {
        // The three parsed modes keep the matrix the pin built at the parse,
        // with its fuzzy, resplit and divided additions, as plain full
        // pinyin's session does.
        if self.zhuyin_parse.is_some() || self.double_parse.is_some() || self.full_parse.is_some() {
            return Ok(ModeKeys::Matrix(&self.parse_matrix));
        }
        Ok(ModeKeys::Session(self.session.matrix_keys()))
    }

    /// Lookup byte offset → character count within `phrase` in the
    /// instance's active parse mode — the `pinyin_get_character_offset` /
    /// `zhuyin_get_character_offset` law
    /// ([`oxpinyin_engine::character_offset_with_walk_over_keys`]) over
    /// [`Self::mode_input`]'s dispatch: plain full pinyin walks the
    /// session's own scan matrix; the other modes walk their parse's keys
    /// over the stored original input. The first result is the character
    /// count on success, or `None` on refusal. The second says whether
    /// the walk ran and the C facade must write its output parameter.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] where the pin asserts and the dictionary's
    /// backend failure.
    pub fn character_offset(
        &self,
        phrase: &oxpinyin_core::PhraseText,
        offset: usize,
    ) -> Result<(Option<usize>, bool), EngineError> {
        let Some(source) = self.span_source() else {
            return self.session.character_offset_with_walk(phrase, offset);
        };
        let (input, separators) = self.mode_input();
        oxpinyin_engine::character_offset_with_walk_over_keys(
            input,
            source.parsed,
            self.parse_matrix.iter().copied(),
            separators,
            self.session.dictionary(),
            phrase,
            offset,
        )
    }

    /// The pin's matrix columns for the active parse mode, for
    /// `_remember_phrase_recur`'s walk; `None` is the empty matrix (a fresh
    /// instance, or a parse that placed no key).
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from the session's matrix read.
    pub fn matrix_columns(&self) -> Result<Option<oxpinyin_engine::MatrixColumns>, EngineError> {
        let (input, separators) = self.mode_input();
        let parsed = self.pin_parsed_len();
        // The index-parsed full pinyin, double pinyin and chewing modes keep
        // the matrix the pin built at the parse, with its fuzzy alternatives;
        // plain full pinyin reads the session's scan matrix, which has them.
        Ok(if self.span_source().is_some() {
            oxpinyin_engine::matrix_columns(
                input,
                parsed,
                self.parse_matrix.iter().copied(),
                separators,
            )
        } else {
            oxpinyin_engine::matrix_columns(
                input,
                parsed,
                self.session.matrix_keys().iter(),
                separators,
            )
        })
    }

    /// The original-coordinate column table over the mode's keys.
    fn abi_columns(&self) -> Result<AbiColumns<'_>, EngineError> {
        let keys = self.mode_key_source()?;
        Ok(self.abi_columns_over(keys))
    }

    /// [`Self::abi_columns`] over keys already taken from
    /// [`Self::mode_key_source`] or [`Self::mode_key_list`].
    fn abi_columns_over<'a>(&'a self, keys: ModeKeys<'a>) -> AbiColumns<'a> {
        let (input, separators) = self.mode_input();
        AbiColumns {
            keys,
            input,
            separators,
            parsed_len: self.parsed_len,
        }
    }

    /// Packed key and raw rest for the two ABI accessor laws.
    ///
    /// # Errors
    /// Returns the pin's zhuyin zero-column assertion as an error.
    pub fn abi_key_at(&self, offset: usize, zhuyin: bool) -> Result<Option<AbiItem>, EngineError> {
        let columns = self.abi_columns()?;
        if offset >= columns.len().saturating_sub(1) {
            return Ok(None);
        }
        if zhuyin && offset > 0 && columns.packs_zero(offset - 1) {
            return Err(EngineError::ZeroKeyOffsetCheck { offset });
        }
        let Some(mut item) = columns.get(offset) else {
            return Ok(None);
        };
        if !zhuyin {
            let mut at = offset;
            while item.0 == 0 && at + 1 < columns.len() {
                at += 1;
                let Some(next) = columns.get(at) else {
                    return Ok(None);
                };
                item = next;
            }
        }
        Ok(Some(item))
    }

    /// Validate the caller's original column before any facade mapping.
    ///
    /// # Errors
    /// Returns out-of-range or the pin's zhuyin zero-column assertion.
    pub fn validate_abi_lookup_offset(
        &self,
        offset: usize,
        zhuyin: bool,
    ) -> Result<usize, EngineError> {
        let keys = self.mode_key_source()?;
        self.validate_abi_lookup_offset_in(keys, offset, zhuyin)
    }

    fn validate_abi_lookup_offset_in(
        &self,
        keys: ModeKeys<'_>,
        offset: usize,
        zhuyin: bool,
    ) -> Result<usize, EngineError> {
        let columns = self.abi_columns_over(keys);
        oxpinyin_engine::check_lookup_offset_range(self.parsed_len, offset)?;
        if zhuyin {
            if offset > 0 && columns.packs_zero(offset - 1) {
                return Err(EngineError::ZeroKeyOffsetCheck { offset });
            }
            return Ok(offset);
        }
        if columns.input.first() == Some(&b'\'') && columns.get(0).is_none() {
            // pinyin.cpp:2226 ignores _check_offset's Boolean result and
            // searches this original column, including leading empties.
            return Ok(offset);
        }
        self.validate_lookup_offset_impl(offset)
    }

    /// Facade-specific left movement, retaining zhuyin.cpp:2019's typo.
    ///
    /// # Errors
    /// Returns the pin's zero-column assertion as an error.
    pub fn abi_left_offset(&self, offset: usize, zhuyin: bool) -> Result<usize, EngineError> {
        if !zhuyin {
            return self.left_offset_impl(offset);
        }
        let columns = self.abi_columns()?;
        oxpinyin_engine::check_lookup_offset_range(self.parsed_len, offset)?;
        if offset > 0 && columns.packs_zero(offset - 1) {
            return Err(EngineError::ZeroKeyOffsetCheck { offset });
        }
        let mut left = offset.saturating_sub(1);
        while left > 0 && columns.get(left).is_none_or(|item| item.2 != offset) {
            left -= 1;
        }
        // The pin normalizes offset, not left. Validate the unnormalized left.
        if left > 0 && columns.packs_zero(left - 1) {
            return Err(EngineError::ZeroKeyOffsetCheck { offset: left });
        }
        Ok(left)
    }

    /// The key the pin's `get_pinyin_key`/`get_zhuyin_key` family answers
    /// at `offset`.
    ///
    /// The pin's three steps: refuse `offset >= matrix.size() - 1` (the
    /// reserved slot), refuse an empty column, then skip forward over
    /// columns holding one lone zero key — a consumed `'` separator —
    /// and the answer is that column's first item.
    #[must_use]
    pub fn key_at(&self, offset: usize) -> Option<KeyAt> {
        let (packed, begin, end) = self.abi_key_at(offset, false).ok()??;
        if packed == 0 {
            return None;
        }
        let keys = self.mode_key_source().ok()?;
        let found = (keys.first_at_or_after(begin)..keys.len())
            .map(|index| keys.get(index))
            .take_while(|key| key.begin == begin)
            .find(|key| key.end == end)?;
        Some(KeyAt {
            text: found.text,
            chewing: found.chewing,
            key: found.key,
            tone: found.tone,
            begin,
            end,
        })
    }
}

impl<
    S: std::borrow::BorrowMut<InstanceState>,
    E: std::borrow::BorrowMut<oxpinyin_engine::SessionState>,
> InstanceView<'_, S, E>
{
}

/// A matrix key as the getters render it.
fn matrix_key_at(key: MatrixKey) -> KeyAt {
    KeyAt {
        text: key.key().pinyin_spelling(),
        chewing: key.key(),
        key: None,
        tone: key.tone(),
        begin: key.syllable_start(),
        end: key.end(),
    }
}

/// Retain every parser key, including keys without a pinyin vocabulary entry.
pub(crate) fn matrix_keys_of(keys: &[KeyAt]) -> Vec<MatrixKey> {
    keys.iter()
        .map(|k| MatrixKey::from_chewing(k.chewing, k.begin, k.end))
        .collect()
}
