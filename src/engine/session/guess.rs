//! Sentence decoding: the n-best lookup over the composition (`pinyin_guess_sentence`, the prefix-seeded variant, the phrase-segment DP).

use super::*;

impl<D, L, S: std::borrow::Borrow<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    /// Segments an arbitrary already-typed sentence string into its
    /// best dictionary phrase path — upstream `pinyin_phrase_segment`
    /// (`pinyin.cpp:1443-1460`), the phrase-lookup span DP over the
    /// sentence's characters, independent of the live composition.
    /// Returns `(matched, tokens)` in `m_phrase_result`'s shape:
    /// character-length, each phrase's token at its span's start
    /// position, `null_token` between phrases — and, on a failed
    /// match, the fully sized all-null array (`PhraseLookup::final_step`
    /// sizes and null-fills before its empty-last-step `false`).
    ///
    /// # Errors
    ///
    /// Propagates the model's step-cost failures.
    pub fn phrase_segment(&self, sentence: &str) -> Result<(bool, Vec<PhraseToken>), EngineError> {
        crate::engine::phrase::phrase_segment(&self.dictionary, &self.model, sentence)
    }

    /// [`Self::phrase_segment`] over raw UCS-4 words, including
    /// surrogate or above-Unicode values. The dictionary compares stored
    /// raw phrase keys; no `char` or `str` is built from a non-scalar.
    ///
    /// # Errors
    ///
    /// Propagates the model's step-cost failures.
    pub(crate) fn phrase_segment_ucs4(
        &self,
        sentence: &[u32],
    ) -> Result<(bool, Vec<PhraseToken>), EngineError> {
        crate::engine::phrase::phrase_segment_ucs4(&self.dictionary, &self.model, sentence)
    }
}

impl<D, L, S: std::borrow::BorrowMut<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    // pinyin_guess_sentence and PhoneticLookup::get_nbest_match, pinyin.cpp:1373–1386.
    /// Replaces the previous n-best results with a sentence lookup over the current matrix
    /// and constraints. The call resets the candidate prefixes before lookup. Empty input
    /// clears the previous results and returns `false`. A completed lookup may return `true`
    /// with no sentence rows. Rows survive further typing until the next guess or
    /// [`Session::reset`].
    ///
    /// # Errors
    ///
    /// Propagates validation, lookup and refresh errors after clearing the previous results.
    pub fn guess_sentence(&mut self) -> Result<bool, EngineError> {
        self.set_candidate_prefixes(core::iter::empty());
        self.sentence.begin();
        if self.input.is_empty() {
            // validate_constraint shrinks the store to the empty matrix
            // before get_nbest_match refuses it (074a2219,
            // phonetic_lookup.cpp:123-138, pinyin.cpp:1379-1380).
            self.constraints.clear();
            self.record.clear();
            return Ok(false);
        }
        let bound = self.retained_parse().matrix.bound;
        if bound == 0 {
            return Ok(false);
        }

        self.validate_guess_constraints(bound, bound + 1)?;

        let retained = self.retained_parse();
        self.sentence.rows = crate::engine::nbest::nbest_sentences_generation(
            crate::engine::nbest::GenerationInput {
                matrix: &retained.scan,
                bound,
                physical_separators: self.input.physical_separators(),
                topology: Some(&retained.matrix),
            },
            &self.dictionary,
            &self.model,
            &[],
            Some(&self.constraints),
            self.nbest_shape,
        )?;
        // The full-matrix rows already carry the chosen prefix: a chosen
        // row's record is its own whole path.
        let state = self.state.borrow_mut();
        state.sentence.last_result = state
            .sentence
            .rows
            .first()
            .map_or_else(Vec::new, |row| row.spans.clone());

        self.refresh_after_parse()?;
        Ok(true)
    }

    // pinyin_guess_sentence_with_prefix, pinyin.cpp:1426–1441; fill_prefixes,
    // phonetic_lookup.h:244–276.
    /// Seeds sentence lookup with the supplied prefix tokens and the sentence-start token. It
    /// replaces the candidate prefixes and clears previous rows before checking the input.
    /// Empty input returns `false` after these changes.
    ///
    /// # Errors
    ///
    /// Propagates validation, lookup and refresh errors. The previous prefixes and rows have
    /// already been replaced.
    pub fn guess_sentence_with_prefix(
        &mut self,
        prefix_tokens: &[PhraseToken],
    ) -> Result<bool, EngineError> {
        self.set_candidate_prefixes(prefix_tokens.iter().copied());
        self.sentence.begin();
        if self.input.is_empty() {
            // validate_constraint shrinks the store to the empty matrix
            // before get_nbest_match refuses it (074a2219,
            // phonetic_lookup.cpp:123-138, pinyin.cpp:1379-1380).
            self.constraints.clear();
            self.record.clear();
            return Ok(false);
        }
        let bound = self.retained_parse().matrix.bound;
        if bound == 0 {
            return Ok(false);
        }

        self.validate_guess_constraints(bound, bound + 1)?;

        // `m_prefixes = [sentence_start] + _compute_prefixes(prefix)`:
        // every entry seeds a zero-cost initial node.
        let mut seeds = Vec::with_capacity(prefix_tokens.len() + 1);
        seeds.push(crate::vocab::SENTENCE_START);
        seeds.extend_from_slice(prefix_tokens);
        let retained = self.retained_parse();
        self.sentence.rows = crate::engine::nbest::nbest_sentences_with_seeds_generation(
            crate::engine::nbest::GenerationInput {
                matrix: &retained.scan,
                bound,
                physical_separators: self.input.physical_separators(),
                topology: Some(&retained.matrix),
            },
            &self.dictionary,
            &self.model,
            &seeds,
            Some(&self.constraints),
            self.nbest_shape,
        )?;
        self.sentence.last_result = self
            .sentence
            .rows
            .first()
            .map_or_else(Vec::new, |row| row.spans.clone());

        self.refresh_after_parse()?;
        Ok(true)
    }

    /// Re-syncs the constraint store to the matrix the last parse filled —
    /// upstream `pinyin_update_constraints`: grow with free cells
    /// (forcings survive typing), shrink by truncation, drop forcings that
    /// overrun or no longer spell. If a forcing dropped (the buffer changed
    /// under it), the selection record follows the surviving forcings.
    /// `validation_size` is the store size the pin validates against.
    fn validate_guess_constraints(
        &mut self,
        _bound: usize,
        validation_size: usize,
    ) -> Result<(), EngineError> {
        let mut store = core::mem::take(&mut self.constraints);
        let retained = self.retained_parse();
        let spelling = crate::engine::nbest::ConstraintSpelling::new(&retained.matrix);
        let dropped = store.validate(validation_size, |start, end, token| {
            spelling.spells(&self.dictionary, start, end, token)
        });
        self.constraints = store;
        if dropped? || (self.constraints.is_active() && self.record.history().is_empty()) {
            // A temporary incompatible parse may have cleared the safe
            // selection record while retaining the actual constraints.
            self.rebuild_selection_from_constraints();
        }
        Ok(())
    }
}
