//! Sentence decoding: the n-best lookup over the composition (`pinyin_guess_sentence`, the prefix-seeded variant, the phrase-segment DP).
//!
//! One of the `impl Session` slices `session/mod.rs` declares; every
//! method here was moved verbatim from the single 5,800-line
//! `session.rs` (2026-09-08) and keeps its doc comment and upstream
//! citations. Shared state, constants and free functions stay in the
//! parent module.

use super::*;

impl<D, L, S: std::borrow::Borrow<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
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
        crate::phrase::phrase_segment(&self.dictionary, &self.model, sentence)
    }

    /// [`Self::phrase_segment`] over raw UCS-4 words, including
    /// surrogate or above-Unicode values. The dictionary compares stored
    /// raw phrase keys; no `char` or `str` is built from a non-scalar.
    ///
    /// # Errors
    ///
    /// Propagates the model's step-cost failures.
    pub fn phrase_segment_ucs4(
        &self,
        sentence: &[u32],
    ) -> Result<(bool, Vec<PhraseToken>), EngineError> {
        crate::phrase::phrase_segment_ucs4(&self.dictionary, &self.model, sentence)
    }
}

impl<D, L, S: std::borrow::BorrowMut<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Runs the n-best sentence lookup and stores its rows
    /// (`pinyin_guess_sentence`, `pinyin.cpp:1373-1385`).
    ///
    /// This is the trellis port of upstream's `PhoneticLookup<2, 3>`
    /// (`crate::nbest`). Rows survive further typing and selections
    /// until the next [`Session::guess_sentence`] or [`Session::reset`] —
    /// upstream's `m_nbest_results` is cleared nowhere else — and
    /// [`Session::candidates`] prepends them while they live.
    ///
    /// The walk always covers the whole matrix the last parse filled, with
    /// the constraint store forcing the chosen spans, as the pin's
    /// `get_nbest_match(m_prefixes, &m_matrix, m_constraints, …)` does
    /// (`pinyin.cpp:1373-1385`). Selections never re-parse the input that
    /// remains after them.
    ///
    /// Returns whether a lookup ran at all (upstream returns the lookup's
    /// `false` only for an empty key matrix; zero rows is still `true`).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails during the lookup.
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
        self.sentence.rows = crate::nbest::nbest_sentences_generation(
            crate::nbest::GenerationInput {
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

    /// Guesses a sentence seeded with prefix tokens — upstream
    /// `pinyin_guess_sentence_with_prefix` (`pinyin.cpp:1426-1441`):
    /// the prefix tokens join the virtual start as zero-cost initial
    /// trellis nodes (`fill_prefixes`, `phonetic_lookup.h:244-276`),
    /// the constraint store validates against the matrix, and the
    /// ordinary full-matrix decode runs.
    /// The caller supplies the tail-substring tokens (`_compute_prefixes`
    /// over the prefix text).
    ///
    /// # Errors
    ///
    /// Propagates engine failures from the decode.
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
        seeds.push(PhraseToken::new(crate::nbest::SENTENCE_START));
        seeds.extend_from_slice(prefix_tokens);
        let retained = self.retained_parse();
        self.sentence.rows = crate::nbest::nbest_sentences_with_seeds_generation(
            crate::nbest::GenerationInput {
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
        bound: usize,
        validation_size: usize,
    ) -> Result<(), EngineError> {
        let mut store = core::mem::take(&mut self.constraints);
        let retained = self.retained_parse();
        let spelling = crate::nbest::ConstraintSpelling::new(
            &retained.scan,
            bound,
            self.input.physical_separators(),
            self.input.as_bytes(),
        );
        let dropped = store.validate(validation_size, |start, end, token| {
            spelling.spells(&self.dictionary, start, end, token)
        });
        drop(spelling);
        self.constraints = store;
        if dropped? || (self.constraints.has_runs() && self.record.history().is_empty()) {
            // A temporary incompatible parse may have cleared the safe
            // selection record while retaining the actual constraints.
            self.rebuild_selection_from_constraints();
        }
        Ok(())
    }
}
