//! Cursor and offset laws: lookup-offset normalisation, the word-boundary helpers, character offsets, and the parse-continuation rules a re-parse applies.

use super::*;

impl<D, L, S: std::borrow::Borrow<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: Display,
    L: LanguageModel,
    L::Error: Display,
{
    /// Normalizes a caller lookup offset back to the first byte of the
    /// physical apostrophe separator run before it, as in the
    /// `_compute_zero_start` walk of `pinyin_guess_candidates` at
    /// libpinyin@dbff264 (`pinyin.cpp:2182-2228`).
    ///
    /// ibus-libpinyin ≥ 1.16.1 passes the raw begin of the next key rest,
    /// which can sit one position past the zero-`ChewingKey` `'` run
    /// (ibus-libpinyin issue #570). The pin walks the matrix from
    /// `offset - 1` downward while the index stays positive and the column
    /// is a lone zero key; in the raw buffer those columns are exactly the
    /// `'` bytes, so the byte walk is the same law.
    ///
    /// The candidate lookup stays anchored at the composition offset the
    /// session owns, so a choose at the caller offset keeps round-tripping.
    ///
    /// Previous-token context: upstream resolves the bigram predecessor by
    /// indexing per-position match results at the lookup offset, and the
    /// raw one-past-separator offset hits a null slot — the system+user
    /// bigram merge is silently skipped and ranking quietly degrades
    /// (C++ libpinyin 2.11.92 still does; libpinyin@412f88e3 feeds the
    /// normalized offset instead). oxpinyin's counterpart is the selection
    /// history — [`Session::selected_tokens`]' tail seeds `rank_phrases`
    /// and the n-best trellis — which no lookup offset ever indexes, so
    /// that degradation cannot occur here; any future offset-indexed
    /// context lookup must take this method's normalized offset (#99
    /// folds the ranking bigram term to zero today,
    /// `dynamic_adjust_bigram_term`).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::LookupOffsetOutOfRange`] for an offset beyond
    /// the raw input's one-past-end position.
    pub(crate) fn normalized_lookup_offset(&self, offset: usize) -> Result<usize, EngineError> {
        normalize_lookup_offset(self.input.as_bytes(), offset)
    }

    /// Normalizes a user cursor position to a lookup offset over the
    /// session's own buffer and options — the `pinyin_get_pinyin_offset`
    /// law ([`crate::engine::lookup_offset_over_spans`]).
    ///
    /// # Errors
    ///
    /// [`EngineError::ZeroKeyOffsetCheck`] where the pin's `_check_offset`
    /// aborts. A cursor past one-past-end is NOT an
    /// error: like the pin, the cursor is clamped to the parsed length, so
    /// there is no out-of-range shape here.
    pub(crate) fn lookup_offset_for_cursor(&self, cursor: usize) -> Result<usize, EngineError> {
        let keys = self.matrix_keys();
        let parsed = self.retained_parse().matrix.bound;
        crate::engine::cursor::lookup_offset_over_spans(
            self.input.as_bytes(),
            parsed,
            &keys,
            true,
            cursor,
        )
    }

    /// The word-level left move over the session's own buffer and options
    /// — the `pinyin_get_left_pinyin_offset` law
    /// ([`crate::engine::left_word_offset_over_spans`]).
    ///
    /// # Errors
    ///
    /// [`EngineError::ZeroKeyOffsetCheck`] where the pin's
    /// `_check_offset` aborts (an input offset one past a lone zero-key
    /// column, or the second check on the computed result); and
    /// [`EngineError::LookupOffsetOutOfRange`] when the offset exceeds the
    /// buffer's one-past-end position (upstream reads its matrix out of
    /// bounds there).
    pub(crate) fn left_word_offset(&self, offset: usize) -> Result<usize, EngineError> {
        let keys = self.matrix_keys();
        let parsed = self.retained_parse().matrix.bound;
        crate::engine::cursor::left_word_offset_over_spans(
            self.input.as_bytes(),
            parsed,
            &keys,
            true,
            offset,
        )
    }

    /// The word-level right move over the session's own buffer and options
    /// — the `pinyin_get_right_pinyin_offset` law
    /// ([`crate::engine::right_word_offset_over_spans`]). `Ok(None)` is the pin's graceful
    /// false: no key starts at the position.
    ///
    /// # Errors
    ///
    /// [`EngineError::ZeroKeyOffsetCheck`] where the pin's
    /// `_check_offset` aborts (an input offset one past a lone zero-key
    /// column, or the second check on the computed result); and
    /// [`EngineError::LookupOffsetOutOfRange`] when the offset exceeds the
    /// buffer's one-past-end position (upstream reads its matrix out of
    /// bounds there).
    pub(crate) fn right_word_offset(&self, offset: usize) -> Result<Option<usize>, EngineError> {
        let keys = self.matrix_keys();
        let parsed = self.retained_parse().matrix.bound;
        crate::engine::cursor::right_word_offset_over_spans(
            self.input.as_bytes(),
            parsed,
            &keys,
            true,
            offset,
        )
    }

    // pinyin_get_pinyin_key and the corresponding getters read m_matrix in place
    // (pinyin.cpp:2924,3013–3066; zhuyin.cpp:1886–1927).
    /// Borrows the last parse’s matrix keys and their raw byte spans in column order. Cursor
    /// operations read these same spans through [`crate::engine::KeySpans`].
    #[must_use]
    pub fn matrix_keys(&self) -> crate::engine::cursor::MatrixKeys<'_> {
        crate::engine::cursor::MatrixKeys::new(self.retained_parse().scan.keys())
    }

    /// Lookup byte offset → character count within `phrase` over the
    /// session's own scan matrix under the calling facade's zero-key rule
    /// ([`crate::engine::character_offset_with_walk_over_keys`]): `Ok(Some(n))` when a key
    /// path pronouncing `phrase`'s first `n` characters reaches `offset`,
    /// `Ok(None)` for the pin's graceful `false` (empty matrix, empty
    /// phrase, a character with no dictionary token, no satisfying path).
    /// Paired with whether its recursive walk ran, so a facade need not
    /// repeat phrase-table reads to decide whether to write its output.
    ///
    /// # Errors
    ///
    /// [`EngineError::LookupOffsetOutOfRange`],
    /// [`EngineError::ZeroKeyOffsetCheck`] and
    /// [`EngineError::MatrixColumnAssert`] where the pin asserts; the
    /// dictionary's backend failure.
    pub(crate) fn character_offset_with_walk(
        &self,
        phrase: &crate::PhraseText,
        offset: usize,
        zhuyin: bool,
    ) -> Result<(Option<usize>, bool), EngineError> {
        // The retained working matrix, not the parsed-only one `matrix_keys`
        // builds for scheme modes: pre-parsed scheme segments keep their
        // boundaries and gain no divided/resplit alternates, as every other
        // law over the session's matrix has it.
        crate::engine::character_offset_with_walk_over_keys(
            (self.input.as_bytes(), true),
            self.retained_parse().matrix.bound,
            self.matrix_keys().iter(),
            &self.dictionary,
            phrase,
            offset,
            zhuyin,
        )
    }

    /// The filtered fewest-keys parse length of the WHOLE raw buffer —
    /// the `pinyin_parse_more_*` return and `pinyin_get_parsed_input_length`
    /// value, which are defined over the passed input, never the
    /// remaining slice a mid-composition re-parse decodes from.
    ///
    /// Extends the filtered key path over any trailing apostrophe run:
    /// the pin's DP propagates `'` byte-for-byte from any reachable
    /// position (`pinyin_parser2.cpp:237-251`) and `final_step` answers
    /// the consistent-chain length, so a trailing or standalone run is
    /// consumed even though no key covers it.
    #[must_use]
    pub(crate) fn full_parsed_len(&self) -> usize {
        if self.input.is_empty() {
            return 0;
        }
        // The exact chain drives the length when a scheme parse owns the
        // buffer: re-segmenting the joined text through the pinyin
        // inventory would under-report zhuyin-only spellings ("den" → 2).
        self.selected_path().parsed_len
    }

    /// Rounds `offset` up to the next character boundary of the raw input.
    ///
    /// The raw buffer only ever holds ASCII, so this is the identity in
    /// practice; it exists so a future input character class cannot turn a
    /// byte count into a slicing panic.
    pub(super) fn next_boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.input.len());
        while !self.input.is_char_boundary(offset) {
            offset += 1;
        }
        offset
    }
}

impl<D, L, S: std::borrow::BorrowMut<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: Display,
    L: LanguageModel,
    L::Error: Display,
{
}
