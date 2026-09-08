//! Sentence decoding: the n-best lookup over the composition (`pinyin_guess_sentence`, the prefix-seeded variant, the phrase-segment DP).
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
    /// Runs the n-best sentence lookup and stores its rows
    /// (`pinyin_guess_sentence`, `pinyin.cpp:1373-1385`).
    ///
    /// With real unigrams this is the trellis port of upstream's
    /// `PhoneticLookup<2, 3>` (`crate::nbest`); without them the
    /// pre-frequency per-path DP supplies up to three rows so the surface
    /// exists for every model. Rows survive further typing and selections
    /// until the next [`Session::guess_sentence`] or [`Session::reset`] —
    /// upstream's `m_nbest_results` is cleared nowhere else — and
    /// [`Session::candidates`] prepends them while they live.
    ///
    /// Which matrix the walk covers: an unconstrained decode with input
    /// remaining is today's remaining-input walk (the W6 re-seed,
    /// bit-identical under the frozen pins — the store is empty there).
    /// Anything else with a non-empty raw buffer walks the **full**
    /// matrix: a constrained composition (the §3 gates, the chosen
    /// prefix forced) or a fully-consumed one (upstream's walk still
    /// answers a terminal choose — the L1 surface,
    /// `docs/findings/live-typing.md`), which the remaining-input model
    /// structurally cannot.
    ///
    /// Returns whether a lookup ran at all (upstream returns the lookup's
    /// `false` only for an empty key matrix; zero rows is still `true`).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails during the lookup.
    pub fn guess_sentence(&mut self) -> Result<bool, EngineError> {
        self.nbest_rows.clear();
        self.nbest_history.clear();
        self.last_result.clear();
        self.sentence_lookup_active = true;
        if self.raw.is_empty() {
            return Ok(false);
        }
        let remaining_empty = self.consumed >= self.raw.len();
        if !remaining_empty && (!self.constraints.is_active() || !self.model.has_real_unigrams()) {
            return self.guess_over_remaining();
        }
        if !self.model.has_real_unigrams() {
            // The pre-frequency fallback has no constrained form; with the
            // input consumed there is nothing to fall back to either.
            return Ok(false);
        }

        let graph = self.build_graph_at(0, self.raw.as_bytes())?;
        let bound = graph.consumed();
        if bound == 0 {
            return Ok(false);
        }
        let matrix = build_scan_matrix(
            &graph,
            self.settings.options,
            self.exact_segments.is_empty(),
        );

        // `pinyin_update_constraints`: re-sync the store to the matrix —
        // grow with free cells (forcings survive typing), shrink by
        // truncation, drop forcings that overrun or no longer spell. If a
        // forcing dropped (the buffer changed under it), the selection
        // record follows the surviving forcings.
        let mut store = core::mem::take(&mut self.constraints);
        let dropped = store.validate(bound + 1, |start, end, token| {
            crate::nbest::span_finds_token(&matrix, start, end, token, &self.dictionary)
        });
        self.constraints = store;
        if dropped? {
            self.rebuild_selection_from_constraints();
        }

        self.nbest_rows = crate::nbest::nbest_sentences(
            &matrix,
            bound,
            &self.dictionary,
            &self.model,
            &[],
            Some(&self.constraints),
            self.nbest_shape,
        )?;
        // The full-matrix rows already carry the chosen prefix: a chosen
        // row's record is its own whole path, so no lookup-time history
        // snapshot stands behind it (the remaining-input walk's §10
        // snapshot-restore pair does not apply).
        self.nbest_history.clear();
        self.last_result = self
            .nbest_rows
            .first()
            .map_or_else(Vec::new, |row| row.spans.clone());

        self.refresh()?;
        Ok(true)
    }

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

    /// Guesses a sentence seeded with prefix tokens — upstream
    /// `pinyin_guess_sentence_with_prefix` (`pinyin.cpp:1426-1441`):
    /// the prefix tokens join the virtual start as zero-cost initial
    /// trellis nodes (`fill_prefixes`, `phonetic_lookup.h:244-276`),
    /// the constraint store validates against the matrix, and the
    /// ordinary full-matrix decode runs — no remaining-input shortcut.
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
        self.nbest_rows.clear();
        self.nbest_history.clear();
        self.last_result.clear();
        self.sentence_lookup_active = true;
        if self.raw.is_empty() {
            return Ok(false);
        }
        let graph = self.build_graph_at(0, self.raw.as_bytes())?;
        let bound = graph.consumed();
        if bound == 0 {
            return Ok(false);
        }
        let matrix = build_scan_matrix(
            &graph,
            self.settings.options,
            self.exact_segments.is_empty(),
        );

        let mut store = core::mem::take(&mut self.constraints);
        let dropped = store.validate(bound + 1, |start, end, token| {
            crate::nbest::span_finds_token(&matrix, start, end, token, &self.dictionary)
        });
        self.constraints = store;
        if dropped? {
            self.rebuild_selection_from_constraints();
        }

        // `m_prefixes = [sentence_start] + _compute_prefixes(prefix)`:
        // every entry seeds a zero-cost initial node.
        let mut seeds = Vec::with_capacity(prefix_tokens.len() + 1);
        seeds.push(PhraseToken::new(crate::nbest::SENTENCE_START));
        seeds.extend_from_slice(prefix_tokens);
        self.nbest_rows = crate::nbest::nbest_sentences_with_seeds(
            &matrix,
            bound,
            &self.dictionary,
            &self.model,
            &seeds,
            Some(&self.constraints),
            self.nbest_shape,
        )?;
        self.last_result = self
            .nbest_rows
            .first()
            .map_or_else(Vec::new, |row| row.spans.clone());

        self.refresh()?;
        Ok(true)
    }

    /// Today's remaining-input walk — the W6 re-seed surface, verbatim:
    /// the trellis over `raw[consumed..]` seeded from the selection
    /// history, the §10 text prefix, and the lookup-time history
    /// snapshot a later row choice restores.
    pub(super) fn guess_over_remaining(&mut self) -> Result<bool, EngineError> {
        let remaining = &self.raw[self.consumed..];
        if remaining.is_empty() {
            return Ok(false);
        }

        let graph = self.build_graph_at(self.consumed, remaining.as_bytes())?;
        let bound = graph.consumed();
        if bound == 0 {
            return Ok(false);
        }

        let offset = self.consumed;
        self.nbest_rows = if self.model.has_real_unigrams() {
            let matrix = build_scan_matrix(
                &graph,
                self.settings.options,
                self.exact_segments.is_empty(),
            );
            crate::nbest::nbest_sentences(
                &matrix,
                bound,
                &self.dictionary,
                &self.model,
                &self.history,
                None,
                self.nbest_shape,
            )?
        } else {
            let scorer = Scorer::with_key_costs(
                self.scoring,
                &self.dictionary,
                &self.model,
                self.key_costs.clone(),
            );
            let paths = k_best(&graph, &scorer, SEGMENTATION_K)?;
            let mut sentences: Vec<(Candidate, Vec<PhraseToken>)> = Vec::new();
            for path in &paths {
                sentences.extend(self.collect_sentences_with_tokens(&graph, &scorer, path)?);
            }
            sentences.sort_by_key(|(candidate, _)| candidate.cost());
            let mut seen: HashSet<compact_str::CompactString> = HashSet::new();
            sentences
                .into_iter()
                .filter(|(candidate, _)| seen.insert(candidate.text().into()))
                .take(self.nbest_shape.nbest())
                .map(|(candidate, tokens)| crate::nbest::NbestRow {
                    text: candidate.text().into(),
                    tokens,
                    spans: Vec::new(),
                    keys: candidate.consumed_keys(),
                    span: candidate.consumed_bytes(),
                    cost: candidate.cost(),
                })
                .collect()
        };
        // The rows were seeded with the history as it stands right here;
        // a later row selection restores this snapshot before extending
        // the record with the row's own tokens.
        self.nbest_history.clone_from(&self.history);
        // The walk's positions are remaining-relative; the store and the
        // train result are absolute.
        for row in &mut self.nbest_rows {
            for span in &mut row.spans {
                span.start += offset;
            }
        }
        self.last_result = self
            .nbest_rows
            .first()
            .map_or_else(Vec::new, |row| row.spans.clone());

        if !self.selected.is_empty() {
            for row in &mut self.nbest_rows {
                let mut full = compact_str::CompactString::from(&self.selected);
                full.push_str(&row.text);
                row.text = full;
            }
        }

        self.refresh()?;
        Ok(true)
    }
}
