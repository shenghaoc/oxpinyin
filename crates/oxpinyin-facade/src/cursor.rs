//! The cursor/offset navigation laws over the active parse mode's own
//! coordinates.
//!
//! The cursor → lookup-offset normalization and the word-level left/right
//! moves port the pin's matrix laws over the engine's positional data —
//! `oxpinyin_engine::lookup_offset_over_spans` and the `*_word_offset`
//! pair. Where the pin's `_check_offset` aborts, these answer an error
//! the C layers turn into `false` (the no-abort policy).
//!
//! Parse-mode dispatch mirrors [`InstanceCore::validate_lookup_offset`]:
//! plain full pinyin runs the law over the session's own buffer; LUOMA /
//! `SECONDARY_ZHUYIN` run it over the stored original input with the
//! index parse's key spans (the pinned index parse consumes `'` as the
//! same separator); double pinyin and the zhuyin keyboards hold no
//! zero-key columns, so the law steps their parse's key spans only.

use oxpinyin_core::SyllableKey;
use oxpinyin_engine::{EngineError, MatrixKey};

use crate::instance::InstanceCore;

type AbiColumns = Vec<Option<(u16, usize, usize)>>;

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
    /// The keys' original-coordinate spans.
    pub spans: Vec<(usize, usize)>,
    /// Whether `'` is a zero-key separator column in this mode.
    pub separators: bool,
}

/// One matrix key at an offset: its canonical pinyin spelling, its tone,
/// and its raw span.
///
/// The spelling rather than a `SyllableKey` because all renderers want
/// text, and because the LUOMA / `SECONDARY_ZHUYIN` index parse carries a
/// canonical spelling rather than a vocabulary key.
pub struct KeyAt {
    /// The key's canonical full-pinyin spelling.
    pub text: &'static str,
    /// The tone consumed with the key, `0` when toneless.
    pub tone: u8,
    /// Inclusive byte offset of the key's first byte.
    pub begin: usize,
    /// Exclusive byte offset one past the key's last byte.
    pub end: usize,
}

impl InstanceCore {
    /// The mode dispatch shared by the three offset laws: zhuyin, then
    /// double pinyin, then the LUOMA / `SECONDARY_ZHUYIN` full-pinyin
    /// index — the same precedence as
    /// [`InstanceCore::validate_lookup_offset`], and the union of the two
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
                spans: parse
                    .keys()
                    .iter()
                    .map(|key| (key.start(), key.end()))
                    .collect(),
                separators: false,
            });
        }
        if let Some(parse) = self.double_parse.as_ref() {
            return Some(SpanSource {
                input: self.double_input.as_bytes(),
                parsed: parse.consumed(),
                spans: parse
                    .keys()
                    .iter()
                    .map(|key| (key.start(), key.end()))
                    .collect(),
                separators: false,
            });
        }
        self.full_parse.as_ref().map(|parse| SpanSource {
            input: self.full_input.as_bytes(),
            parsed: parse.consumed(),
            spans: parse
                .keys()
                .iter()
                .map(|key| (key.start(), key.end()))
                .collect(),
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
    /// Forwards [`EngineError`] where the pin aborts (the no-abort
    /// policy's refusal).
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

    /// The active parse mode's keys as `(text, tone, syllable start, raw
    /// end)`, the mode's own input buffer, and whether `'` is a zero-key
    /// separator in that mode — the same `(input, separators)` dispatch
    /// [`Self::span_source`] and
    /// [`InstanceCore::validate_lookup_offset`] make. The key spans are
    /// in the active input's coordinates, so [`Self::key_at`] must walk
    /// that same buffer, not the session's `'`-joined canonical spelling.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] from the session's matrix read.
    pub fn mode_keys(&self) -> Result<(Vec<KeyAt>, &[u8], bool), EngineError> {
        if let Some(parse) = self.zhuyin_parse.as_ref() {
            return Ok((
                parse
                    .keys()
                    .iter()
                    .map(|k| KeyAt {
                        text: k.key().text(),
                        tone: k.tone(),
                        begin: k.start(),
                        end: k.end(),
                    })
                    .collect(),
                self.zhuyin_input.as_bytes(),
                false,
            ));
        }
        if let Some(parse) = self.double_parse.as_ref() {
            return Ok((
                parse
                    .keys()
                    .iter()
                    .map(|k| KeyAt {
                        text: k.key().text(),
                        tone: 0,
                        begin: k.start(),
                        end: k.end(),
                    })
                    .collect(),
                self.double_input.as_bytes(),
                false,
            ));
        }
        if let Some(parse) = self.full_parse.as_ref() {
            return Ok((
                parse
                    .keys()
                    .iter()
                    .map(|k| KeyAt {
                        text: k.canonical(),
                        tone: k.tone(),
                        begin: k.start(),
                        end: k.end(),
                    })
                    .collect(),
                self.full_input.as_bytes(),
                true,
            ));
        }
        let (keys, _) = self.session.matrix_keys()?;
        Ok((
            keys.iter()
                .map(|k| KeyAt {
                    text: k.key().text(),
                    tone: k.tone(),
                    begin: k.syllable_start(),
                    end: k.end(),
                })
                .collect(),
            self.session.raw_input().as_bytes(),
            true,
        ))
    }

    /// Lookup byte offset → character count within `phrase` in the
    /// instance's active parse mode — the `pinyin_get_character_offset` /
    /// `zhuyin_get_character_offset` law
    /// ([`oxpinyin_engine::character_offset_over_keys`]) over
    /// [`Self::mode_keys`]'s dispatch: plain full pinyin walks the
    /// session's own scan matrix; the other modes walk their parse's keys
    /// over the stored original input. `Ok(Some(n))` is the pin's `true`
    /// with the character count, `Ok(None)` its graceful `false`.
    ///
    /// # Errors
    ///
    /// Forwards [`EngineError`] where the pin asserts (the no-abort
    /// policy's refusal) and the dictionary's backend failure.
    pub fn character_offset(
        &self,
        phrase: &str,
        offset: usize,
    ) -> Result<Option<usize>, EngineError> {
        let Some(source) = self.span_source() else {
            return self.session.character_offset(phrase, offset);
        };
        let (keys, input, separators) = self.mode_keys()?;
        // A key whose canonical spelling is not a vocabulary syllable
        // cannot be looked up; the parsers never place one.
        let keys: Vec<MatrixKey> = keys
            .iter()
            .filter_map(|k| {
                Some(MatrixKey::new(
                    SyllableKey::from_text(k.text)?,
                    k.tone,
                    k.begin,
                    k.end,
                ))
            })
            .collect();
        oxpinyin_engine::character_offset_over_keys(
            input,
            source.parsed,
            &keys,
            separators,
            self.session.dictionary(),
            phrase,
            offset,
        )
    }

    /// Original-coordinate columns: a real first item, a zero key, or empty.
    fn abi_columns(&self) -> Result<AbiColumns, EngineError> {
        let (keys, input, separators) = self.mode_keys()?;
        let mut columns = vec![None; self.parsed_len + 1];
        for key in &keys {
            if let Some(column) = columns.get_mut(key.begin)
                && column.is_none()
            {
                let packed = oxpinyin_core::ChewingKey::from_pinyin(key.text)
                    .map_or(0, |k| k.with_tone(key.tone).to_packed());
                *column = Some((packed, key.begin, key.end));
            }
        }
        if keys.is_empty() {
            return Ok(columns);
        }
        let first = keys.iter().map(|key| key.begin).min();
        if separators {
            for (position, column) in columns.iter_mut().enumerate().take(self.parsed_len) {
                if column.is_none()
                    && input.get(position) == Some(&b'\'')
                    && first.is_none_or(|begin| position >= begin)
                {
                    *column = Some((0, position, position + 1));
                }
            }
        }
        for (position, column) in columns.iter_mut().enumerate().skip(self.parsed_len) {
            if column.is_none() {
                *column = Some((0, position, position + 1));
            }
        }
        Ok(columns)
    }

    /// Packed key and raw rest for the two ABI accessor laws.
    ///
    /// # Errors
    /// Returns the pin's zhuyin zero-column assertion as an error.
    pub fn abi_key_at(
        &self,
        offset: usize,
        zhuyin: bool,
    ) -> Result<Option<(u16, usize, usize)>, EngineError> {
        let columns = self.abi_columns()?;
        if offset >= columns.len().saturating_sub(1) {
            return Ok(None);
        }
        if zhuyin
            && offset > 0
            && columns
                .get(offset - 1)
                .is_some_and(|c| matches!(c, Some((0, _, _))))
        {
            return Err(EngineError::ZeroKeyOffsetCheck { offset });
        }
        let Some(mut item) = columns[offset] else {
            return Ok(None);
        };
        if !zhuyin {
            let mut at = offset;
            while item.0 == 0 && at + 1 < columns.len() {
                at += 1;
                let Some(next) = columns[at] else {
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
        let columns = self.abi_columns()?;
        oxpinyin_engine::check_lookup_offset_range(self.parsed_len, offset)?;
        if zhuyin {
            if offset > 0
                && columns
                    .get(offset - 1)
                    .is_some_and(|c| matches!(c, Some((0, _, _))))
            {
                return Err(EngineError::ZeroKeyOffsetCheck { offset });
            }
            return Ok(offset);
        }
        let (_, input, _) = self.mode_keys()?;
        if input.first() == Some(&b'\'') && columns.first() == Some(&None) {
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
        if offset > 0
            && columns
                .get(offset - 1)
                .is_some_and(|c| matches!(c, Some((0, _, _))))
        {
            return Err(EngineError::ZeroKeyOffsetCheck { offset });
        }
        let mut left = offset.saturating_sub(1);
        while left > 0 && columns[left].is_none_or(|item| item.2 != offset) {
            left -= 1;
        }
        // The pin normalizes offset, not left. Validate the unnormalized left.
        if left > 0
            && columns
                .get(left - 1)
                .is_some_and(|c| matches!(c, Some((0, _, _))))
        {
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
        let (keys, _, _) = self.mode_keys().ok()?;
        let found = keys
            .iter()
            .find(|key| key.begin == begin && key.end == end)?;
        Some(KeyAt {
            text: found.text,
            tone: found.tone,
            begin,
            end,
        })
    }
}
