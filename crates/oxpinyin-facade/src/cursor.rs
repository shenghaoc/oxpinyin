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

use oxpinyin_engine::EngineError;

use crate::instance::InstanceCore;

/// The active parse mode's span source: the coordinate input bytes, its
/// parsed length, the key spans `(start, end)`, and whether `'` is a
/// zero-key separator in that mode. `None` for plain full pinyin, whose
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

    /// The key the pin's `get_pinyin_key`/`get_zhuyin_key` family answers
    /// at `offset`.
    ///
    /// The pin's three steps: refuse `offset >= matrix.size() - 1` (the
    /// reserved slot), refuse an empty column, then skip forward over
    /// columns holding one lone zero key — a consumed `'` separator —
    /// and the answer is that column's first item.
    #[must_use]
    pub fn key_at(&self, offset: usize) -> Option<KeyAt> {
        let (keys, input, separators) = self.mode_keys().ok()?;
        // matrix.size() is input.len() + 1; the last column is the
        // reserved slot. `input` and the key spans share one coordinate
        // space — the active mode's own buffer — so the separator walk
        // reads it, not the session's `'`-joined canonical spelling.
        if offset >= input.len() {
            return None;
        }
        let mut at = offset;
        loop {
            if let Some(found) = keys.iter().find(|k| k.begin == at) {
                return Some(KeyAt {
                    text: found.text,
                    tone: found.tone,
                    begin: found.begin,
                    end: found.end,
                });
            }
            // A lone zero-key column is a consumed separator; the pin
            // walks past the run. Only the separator modes hold one —
            // zhuyin and double pinyin carry `'` as content or not at
            // all, so their empty columns end the walk. Anything else is
            // an empty mid-syllable column.
            if separators && input.get(at).copied() == Some(b'\'') && at + 1 < input.len() {
                at += 1;
                continue;
            }
            return None;
        }
    }
}
