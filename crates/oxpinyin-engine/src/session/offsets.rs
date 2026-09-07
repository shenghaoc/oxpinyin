//! Cursor and offset laws: lookup-offset normalisation, the word-boundary helpers, character offsets, and the parse-continuation rules a re-parse applies.
//!
//! One of the `impl Session` slices `session/mod.rs` declares; every
//! method here was moved verbatim from the single 5,800-line
//! `session.rs` (2026-09-08) and keeps its doc comment and upstream
//! citations. Shared state, constants and free functions stay in the
//! parent module.

use super::*;

impl<D, L> Session<D, L>
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
        normalize_lookup_offset(self.raw.as_bytes(), offset)
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
        crate::cursor::lookup_offset_for_cursor(self.raw.as_bytes(), self.settings.options, cursor)
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
        crate::cursor::left_word_offset(self.raw.as_bytes(), self.settings.options, offset)
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
        crate::cursor::right_word_offset(self.raw.as_bytes(), self.settings.options, offset)
    }

    /// The composition's scan-matrix keys with their raw byte spans.
    ///
    /// The same walk the cursor laws above run — `matrix_spans` is a
    /// projection of this — so a key answered here and an offset answered
    /// by [`Session::right_word_offset`] agree by construction. The C ABI's
    /// `pinyin_get_pinyin_key` family reads this.
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the raw buffer cannot be built into a
    /// segment graph.
    pub fn matrix_keys(&self) -> Result<(Vec<crate::cursor::MatrixKey>, usize), EngineError> {
        crate::cursor::matrix_keys(self.raw.as_bytes(), self.settings.options)
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
        phrase: &str,
        offset: usize,
    ) -> Result<Option<usize>, EngineError> {
        // The working graph, not the parsed-only one `matrix_keys`
        // builds: pre-parsed scheme segments keep their boundaries and
        // gain no divided/resplit alternates, as every other law over
        // the session's matrix has it.
        let graph = self.build_graph_at(0, self.raw.as_bytes())?;
        let parsed = graph.consumed();
        let matrix = build_scan_matrix(
            &graph,
            self.settings.options,
            self.exact_segments.is_empty(),
        );
        let keys: Vec<crate::cursor::MatrixKey> = matrix
            .iter()
            .flatten()
            .map(|key| crate::cursor::MatrixKey::new(key.key, key.tone, key.syllable_start, key.to))
            .collect();
        crate::character_offset_over_keys(
            self.raw.as_bytes(),
            parsed,
            &keys,
            true,
            &self.dictionary,
            phrase,
            offset,
        )
    }

    /// Whether a selection consumed the whole buffer and no rebuild has
    /// since changed the record — the commit-branch shape the R5 revert
    /// keeps composing through ([`Session::committed_parse_continues`]).
    /// A pure query over valid state.
    #[must_use]
    pub const fn selection_committed(&self) -> bool {
        self.selection_committed
    }

    /// Whether a re-parse of `original` continues the current composition
    /// (`CapiInstance::begin_parse`'s rule): the composition is open —
    /// not completed by a selection — and the buffer evolved from
    /// itself (one input is a prefix of the other: forward typing or
    /// backspace). Upstream's constraints survive every re-parse with
    /// `validate_constraint` dropping whatever stops spelling at the
    /// next guess, so an open composition's extension, shrink, or
    /// re-send continues it — the cursor may sit mid-buffer, or the
    /// buffer may have shrunk TO the cursor (a backspace that ate the
    /// tail — still open). A selection-consumed composition continues
    /// through [`Session::committed_parse_continues`] (the R5 revert,
    /// register #8); only a divergent buffer starts fresh — a different
    /// string is a different composition, and a stale selection-derived
    /// cursor must not mis-anchor its window before validate could drop
    /// the mismatched forcings.
    ///
    /// A pure query, not a fallible operation — it reads already-valid
    /// state and cannot fail, so the constitution's `Result` rule for
    /// fallible public APIs does not reach it. The state-changing halves
    /// of the parse pipeline are the fallible [`Session::replace_raw`]
    /// and the infallible [`Session::reset_composition`]/[`reset`].
    #[must_use]
    pub fn parse_continues(&self, stored: &[u8], original: &[u8]) -> bool {
        !self.selection_committed
            && !stored.is_empty()
            && (original.starts_with(stored) || stored.starts_with(original))
    }

    /// The R5 half of the parse rule (register #8): a SELECTION-committed
    /// composition whose buffer evolved from the stored one still
    /// continues — the constraint store and the selection record survive
    /// into the next guess, where validate drops whatever stops spelling.
    /// Upstream's parse path never touches `m_constraints`
    /// (`pinyin.cpp:1497-1517`) and only `pinyin_reset` clears the store
    /// (`pinyin.cpp:2693-2704`), so a commit no longer ends the
    /// composition engine-side: the pre-revert rule re-parsed this shape
    /// fresh — an emulation of the frontend's reset-on-commit contract
    /// the #141 cursor flows pinned — which dropped forcings upstream
    /// keeps. The divergence boundary that stays: a DIVERGENT buffer
    /// answers `false` here and in [`Session::parse_continues`], so it
    /// alone re-parses fresh.
    ///
    /// A pure query, not a fallible operation — it reads already-valid
    /// state and cannot fail, so the constitution's `Result` rule for
    /// fallible public APIs does not reach it. The state-changing halves
    /// of the parse pipeline are the fallible [`Session::replace_raw`]
    /// and the infallible [`Session::reset_composition`]/[`reset`].
    #[must_use]
    pub fn committed_parse_continues(&self, stored: &[u8], original: &[u8]) -> bool {
        self.selection_committed
            && !stored.is_empty()
            && (original.starts_with(stored) || stored.starts_with(original))
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
        if self.raw.is_empty() {
            return 0;
        }
        // The exact chain drives the length when a scheme parse owns the
        // buffer: re-segmenting the joined text through the pinyin
        // inventory would under-report zhuyin-only spellings ("den" → 2).
        self.build_graph_at(0, self.raw.as_bytes())
            .map_or(0, |graph| {
                apostrophe_extended(
                    self.raw.as_bytes(),
                    graph
                        .fewest_keys(self.settings.incomplete())
                        .last()
                        .map_or(0, Edge::to),
                )
            })
    }

    /// Rounds `offset` up to the next character boundary of the raw input.
    ///
    /// The raw buffer only ever holds ASCII, so this is the identity in
    /// practice; it exists so a future input character class cannot turn a
    /// byte count into a slicing panic.
    pub(super) fn next_boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.raw.len());
        while !self.raw.is_char_boundary(offset) {
            offset += 1;
        }
        offset
    }
}
