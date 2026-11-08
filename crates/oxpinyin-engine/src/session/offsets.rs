//! Cursor and offset laws: lookup-offset normalisation, the word-boundary helpers, character offsets, and the parse-continuation rules a re-parse applies.
//!
//! One of the `impl Session` slices `session/mod.rs` declares; every
//! method here was moved verbatim from the single 5,800-line
//! `session.rs` (2026-09-08) and keeps its doc comment and upstream
//! citations. Shared state, constants and free functions stay in the
//! parent module.

use super::*;

impl<D, L, S: std::borrow::Borrow<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Normalizes a caller lookup offset back to the first byte of the
    /// apostrophe separator run before it, then validates it — the
    /// `_compute_zero_start` + `_check_offset` pair `pinyin_guess_candidates`
    /// runs at libpinyin@dbff264 (`pinyin.cpp:2182-2228`).
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
    /// the raw input's one-past-end position (upstream reads its matrix out
    /// of bounds there — no pinned behaviour exists to reproduce), and
    /// [`EngineError::LookupOffsetPastSeparator`] where upstream's
    /// `_check_offset` aborts: the normalized offset still sits one past a
    /// separator, which only a leading apostrophe run can cause (the walk
    /// never crosses byte 0).
    pub fn normalized_lookup_offset(&self, offset: usize) -> Result<usize, EngineError> {
        normalize_lookup_offset(self.input.as_bytes(), offset)
    }

    /// Normalizes a user cursor position to a lookup offset over the
    /// session's own buffer and options — the `pinyin_get_pinyin_offset`
    /// law ([`crate::lookup_offset_for_cursor`]).
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the buffer cannot be represented as a
    /// segment graph, and [`EngineError::ZeroKeyOffsetCheck`] where the
    /// pin's `_check_offset` aborts. A cursor past one-past-end is NOT an
    /// error: like the pin, the cursor is clamped to the parsed length, so
    /// there is no out-of-range shape here.
    pub fn lookup_offset_for_cursor(&self, cursor: usize) -> Result<usize, EngineError> {
        let (keys, parsed) = self.matrix_keys()?;
        crate::cursor::lookup_offset_over_spans(self.input.as_bytes(), parsed, keys, true, cursor)
    }

    /// The word-level left move over the session's own buffer and options
    /// — the `pinyin_get_left_pinyin_offset` law
    /// ([`crate::left_word_offset`]).
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the buffer cannot be represented as a
    /// segment graph; [`EngineError::ZeroKeyOffsetCheck`] where the pin's
    /// `_check_offset` aborts (an input offset one past a lone zero-key
    /// column, or the second check on the computed result); and
    /// [`EngineError::LookupOffsetOutOfRange`] when the offset exceeds the
    /// buffer's one-past-end position (upstream reads its matrix out of
    /// bounds there).
    pub fn left_word_offset(&self, offset: usize) -> Result<usize, EngineError> {
        let (keys, parsed) = self.matrix_keys()?;
        crate::cursor::left_word_offset_over_spans(
            self.input.as_bytes(),
            parsed,
            keys,
            true,
            offset,
        )
    }

    /// The word-level right move over the session's own buffer and options
    /// — the `pinyin_get_right_pinyin_offset` law
    /// ([`crate::right_word_offset`]). `Ok(None)` is the pin's graceful
    /// false: no key starts at the position.
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the buffer cannot be represented as a
    /// segment graph; [`EngineError::ZeroKeyOffsetCheck`] where the pin's
    /// `_check_offset` aborts (an input offset one past a lone zero-key
    /// column, or the second check on the computed result); and
    /// [`EngineError::LookupOffsetOutOfRange`] when the offset exceeds the
    /// buffer's one-past-end position (upstream reads its matrix out of
    /// bounds there).
    pub fn right_word_offset(&self, offset: usize) -> Result<Option<usize>, EngineError> {
        let (keys, parsed) = self.matrix_keys()?;
        crate::cursor::right_word_offset_over_spans(
            self.input.as_bytes(),
            parsed,
            keys,
            true,
            offset,
        )
    }

    /// The composition's scan-matrix keys with their raw byte spans.
    ///
    /// The same walk the cursor laws above run — `matrix_spans` is a
    /// projection of this — so a key answered here and an offset answered
    /// by [`Session::right_word_offset`] agree by construction. The C ABI's
    /// `pinyin_get_pinyin_key` family reads this.
    ///
    /// The keys are in the pin's column order ([`crate::cursor::KeySpans`])
    /// and are built once per input: every input mutation and option change
    /// drops them with the parser graph, so each getter after a parse costs
    /// a lookup rather than a rebuild.
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the raw buffer cannot be built into a
    /// segment graph.
    pub fn matrix_keys(&self) -> Result<(&[crate::cursor::MatrixKey], usize), EngineError> {
        let (keys, parsed) = match self.input.keys.get() {
            Some(cached) => cached,
            None => {
                let built = self.build_matrix_keys()?;
                self.input.keys.get_or_init(|| built)
            }
        };
        Ok((keys, *parsed))
    }

    /// [`Session::matrix_keys`] without the cache.
    fn build_matrix_keys(&self) -> Result<(Vec<crate::cursor::MatrixKey>, usize), EngineError> {
        if self.input.full_pinyin() {
            // pinyin.cpp:1497-1525 fills the matrix from the indexed parser's
            // keys, including initial-only keys accepted without INCOMPLETE;
            // zhuyin.cpp:1886-1927 reads the one matrix its own parse
            // filled. Both facades project their keys from the retained
            // scan matrix, whichever transforms it carries.
            let retained = self.retained_parse()?;
            let keys = retained
                .scan
                .iter()
                .flatten()
                .map(|key| {
                    crate::cursor::MatrixKey::new(key.key, key.tone, key.syllable_start, key.to)
                })
                .collect();
            return Ok((crate::cursor::in_column_order(keys), retained.matrix.bound));
        }
        crate::cursor::matrix_keys(self.input.as_bytes(), self.settings.options)
    }

    /// Lookup byte offset → character count within `phrase` over the
    /// session's own scan matrix — the `pinyin_get_character_offset` law
    /// ([`crate::character_offset_over_keys`]): `Ok(Some(n))` when a key
    /// path pronouncing `phrase`'s first `n` characters reaches `offset`,
    /// `Ok(None)` for the pin's graceful `false` (empty matrix, empty
    /// phrase, a character with no dictionary token, no satisfying path).
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the raw buffer cannot be built into a
    /// segment graph; [`EngineError::LookupOffsetOutOfRange`],
    /// [`EngineError::ZeroKeyOffsetCheck`] and
    /// [`EngineError::MatrixColumnAssert`] where the pin asserts; the
    /// dictionary's backend failure.
    pub fn character_offset(
        &self,
        phrase: &oxpinyin_core::PhraseText,
        offset: usize,
    ) -> Result<Option<usize>, EngineError> {
        self.character_offset_with_walk(phrase, offset)
            .map(|(count, _)| count)
    }

    /// The offset and whether its recursive walk ran, so a facade need
    /// not repeat phrase-table reads to decide whether to write its output.
    ///
    /// # Errors
    /// As [`Self::character_offset`].
    pub fn character_offset_with_walk(
        &self,
        phrase: &oxpinyin_core::PhraseText,
        offset: usize,
    ) -> Result<(Option<usize>, bool), EngineError> {
        // The retained working matrix, not the parsed-only one `matrix_keys`
        // builds for scheme modes: pre-parsed scheme segments keep their
        // boundaries and gain no divided/resplit alternates, as every other
        // law over the session's matrix has it.
        let retained = self.retained_parse()?;
        let parsed = retained.matrix.bound;
        let keys: Vec<crate::cursor::MatrixKey> = retained
            .scan
            .iter()
            .flatten()
            .map(|key| crate::cursor::MatrixKey::new(key.key, key.tone, key.syllable_start, key.to))
            .collect();
        crate::character_offset_with_walk_over_keys(
            self.input.as_bytes(),
            parsed,
            &keys,
            true,
            &self.dictionary,
            phrase,
            offset,
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
    pub fn full_parsed_len(&self) -> usize {
        if self.input.is_empty() {
            return 0;
        }
        // The exact chain drives the length when a scheme parse owns the
        // buffer: re-segmenting the joined text through the pinyin
        // inventory would under-report zhuyin-only spellings ("den" → 2).
        self.selected_path()
            .map_or(0, |selected| selected.parsed_len)
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
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
}
