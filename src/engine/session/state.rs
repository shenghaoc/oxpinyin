//! Lifecycle and settings: opening a session, resetting it, replacing the raw buffer, the live option and shape switches, and the read-only accessors (raw input, candidates, composition offset).

use super::*;

impl<D, L> Session<D, L>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    /// Opens a session over the supplied backends.
    ///
    /// Settings and backend handles arrive as data; the session reads no
    /// environment and discovers no path. Construction cannot fail: it
    /// reads no dictionary or model record.
    pub fn new(settings: SessionSettings, dictionary: D, model: L) -> Self {
        Self {
            dictionary,
            model,
            state: SessionState {
                options: settings.options(),
                law: Law::Pinyin,
                defer_list_builds: false,
                input: InputBuffer::default(),
                record: SelectionRecord::default(),
                constraints: crate::engine::constraint::ConstraintStore::default(),
                lookup: Lookup::default(),
                sentence: SentenceState::default(),
                candidate_prefixes: Vec::new(),
                scratch: Scratch::default(),
                scan_unreadable: 0,
                scan_ranked_phrase: false,
            },
        }
    }
}

impl<D, L, S: std::borrow::Borrow<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    /// Whether `text` extends the bytes the selection record was built
    /// over (`raw[..consumed]`) — the continuity retaining the record
    /// requires. The parse seams' own prefix checks run in the caller's
    /// coordinates (the scheme parses' original input), while the record
    /// lives in these canonical bytes: a replacement that passes those
    /// checks but does not extend the covered span — a scheme switch
    /// that decodes the same codes to a different spelling, any
    /// transform divergence — must reconcile, or the record retains a stale
    /// composition offset and token history alongside the new raw input.
    pub(super) fn replacement_extends_selection(&self, text: &str) -> bool {
        text.as_bytes()
            .starts_with(&self.input.as_bytes()[..self.record.consumed()])
    }

    /// The option word applied ([`Self::set_options`]).
    #[must_use]
    pub fn options(&self) -> OptionBits {
        self.options
    }

    /// The last sort-option word applied ([`Self::set_sort_options`]).
    #[must_use]
    pub fn sort_options(&self) -> u32 {
        self.lookup.sort_word
    }

    /// The sort word that orders this session's candidate lists under its
    /// [`Law`].
    ///
    /// The zhuyin comparator has no span key, and the span key would always
    /// tie on a zhuyin list anyway: a phrase length is a key count
    /// (`phrase_index.h:136-150`; zhuyin import refuses a mismatch,
    /// `zhuyin.cpp:525`); a zhuyin column's keys share one end
    /// (`zhuyin.cpp:1030-1038,1069-1076` run no split pass; fuzzy keys keep
    /// the span, `phonetic_key_matrix.cpp:254-258`); and the only longer
    /// copies the pin makes cross an apostrophe run with the same token's
    /// frequency (`zhuyin.cpp:1499-1512`, `:1229-1270`), which this scan does
    /// not produce.
    pub(super) fn rank_word(&self) -> u32 {
        match self.law {
            Law::Pinyin => self.lookup.sort_word,
            Law::Zhuyin => SORT_BY_PHRASE_LENGTH | SORT_BY_FREQUENCY,
        }
    }

    /// The current candidates, in rank order.
    ///
    /// Sentence rows appear at the head of this list only after
    /// [`Session::guess_sentence`] has run for the current composition:
    /// upstream's candidate list prepends its n-best rows exactly when
    /// `m_nbest_results` is non-empty (`pinyin.cpp:2292-2293`), and the
    /// corpus pins were captured without a sentence guess.
    ///
    /// Under `Session::set_defer_list_builds`, call
    /// `Session::refresh_candidates` first.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn candidates(&self) -> &CandidateList {
        &self.lookup.candidates
    }

    /// Whether a sentence lookup has run since the last reset.
    ///
    /// An active lookup can have no n-best rows. The C sentence getters
    /// return `false` in that case (`pinyin.cpp:1470-1471`,
    /// `zhuyin.cpp:988-989`).
    #[must_use]
    pub fn sentence_lookup_active(&self) -> bool {
        self.sentence.active()
    }

    /// The phrases of n-best row `index`, best-first, one part per phrase
    /// as the pin's result vector holds them for `convert_to_utf8`
    /// (`pinyin_get_sentence`'s payload, `pinyin.cpp:1464-1482`,
    /// `lookup/lookup.cpp:27-69`). The caller resolves each
    /// [`SentencePart::Token`] through the live phrase index when it reads
    /// the sentence. `None` when fewer rows exist. A NULL or empty string
    /// still has a row (`lookup/lookup.cpp:27-70`).
    #[must_use]
    pub fn sentence_phrases(&self, index: u8) -> Option<impl Iterator<Item = SentencePart> + '_> {
        let row = self.sentence.rows.get(usize::from(index))?;
        Some(SentenceState::parts(row))
    }

    /// Whether the decoded results hold row 0 and row `rank`: the two
    /// `check_result(results.get_result(...))` asserts of the pin's NBEST
    /// choose (074a2219 `pinyin.cpp:2517-2518`).
    #[must_use]
    pub fn has_nbest_rank(&self, rank: u8) -> bool {
        usize::from(rank) < self.sentence.rows.len()
    }

    /// The decoded text of n-best row `index`, best-first. `None` when the
    /// row is absent or its string is NULL.
    #[cfg(test)]
    #[must_use]
    pub fn sentence_text(&self, index: u8) -> Option<&PhraseText> {
        self.sentence
            .rows
            .get(usize::from(index))
            .and_then(|row| row.text.as_ref())
    }

    /// The raw input typed so far.
    #[must_use]
    pub fn raw_input(&self) -> &str {
        self.input.as_str()
    }

    /// Whether a composition is in progress.
    #[must_use]
    pub fn is_composing(&self) -> bool {
        !self.input.is_empty()
    }

    /// Bytes of the raw input consumed by selections so far — the
    /// composition offset the candidate lookup is anchored at.
    ///
    /// After a successful [`Session::select_target`] this is the chosen
    /// candidate's absolute end position: the previous anchor plus the
    /// candidate's span (separator run included), never past the raw
    /// input. The C ABIs do not answer it: a choose answers from the row's
    /// own `m_begin` and `m_end` as the pin does.
    #[cfg(any(test, feature = "test-hooks"))]
    #[must_use]
    pub fn composition_offset(&self) -> usize {
        self.record.consumed()
    }

    /// Retained suffix tokens in the order the last prefix guess found them.
    #[must_use]
    pub fn candidate_prefixes(&self) -> &[PhraseToken] {
        &self.candidate_prefixes
    }

    /// The dictionary backend.
    #[must_use]
    pub(crate) fn dictionary(&self) -> &D {
        &self.dictionary
    }
}

impl<D, L, S: std::borrow::BorrowMut<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    /// Replaces the retained prefixes after a sentence or prediction guess.
    /// This can change the candidate ranking at offset zero.
    pub fn set_candidate_prefixes(&mut self, prefixes: impl IntoIterator<Item = PhraseToken>) {
        self.candidate_prefixes.clear();
        self.candidate_prefixes.extend(prefixes);
        self.lookup.stale = true;
    }

    // pinyin_reset clears m_constraints and m_nbest_results, pinyin.cpp:2693–2704.
    /// Clears the input, selection record, constraints, retained prefixes and n-best sentence
    /// rows.
    pub fn reset(&mut self) {
        self.discard_composition();
        self.sentence.reset();
        self.candidate_prefixes.clear();
    }

    /// Discards the composition but keeps the last sentence lookup's
    /// n-best rows. Parsing itself never discards constraints; an empty
    /// matrix guess uses this to clear them without replacing the rows.
    ///
    /// The input, the selection record and the constraint store go, as
    /// in [`Session::reset`]; the n-best state does not, because
    /// upstream's parse never touches `m_nbest_results`
    /// (`pinyin_parse_more_full_pinyins`, `pinyin.cpp:1497-1524`): only
    /// `pinyin_reset` (`:2700`) and the next `guess_sentence` replace it.
    /// Until then `pinyin_get_sentence` answers the old rows and
    /// `pinyin_guess_candidates` prepends them.
    pub(crate) fn discard_composition(&mut self) {
        self.reset_composition();
        self.input.clear();
        self.record.clear();
        self.constraints.clear();
    }

    /// The parse-path reset: resets the lookup state (the candidate list).
    /// The retained parse is replaced by `reparse()`; the raw input, the
    /// selection record, the constraint store and the n-best rows stay.
    ///
    /// `pinyin_parse_more_full_pinyins` replaces the input buffer — the
    /// frontend re-sends the whole buffer every keystroke — and never
    /// touches upstream's instance-level `m_constraints` or the chosen
    /// cursor (`pinyin.cpp:1497-1533`); the next `guess_sentence`
    /// re-validates the surviving forcings against the new matrix. This
    /// is that split's engine half: the L2 lifetime rule. The parse leaves
    /// `m_nbest_results` alone too (`pinyin.cpp:1497-1524`; only
    /// `pinyin_reset` clears it, `:2700`), so the sentence rows survive
    /// until the next `guess_sentence` replaces them.
    ///
    /// The raw buffer is NOT cleared here: a cleared raw with a surviving
    /// cursor would leave `consumed > raw.len()` observable, and every
    /// `raw[consumed..]` slice would panic — the constitution forbids that
    /// window. Input replacement is atomic
    /// in [`Session::replace_raw`], which clears, refills, clamps, and
    /// refreshes in one call, so this reset alone always leaves a
    /// consistent session.
    pub(crate) fn reset_composition(&mut self) {
        self.lookup.reset();
    }

    // pinyin_parse_more_full_pinyins, pinyin.cpp:1497–1524. Full-pinyin prefix consumption
    // follows reachable parser positions (pinyin_parser2.cpp:237–328).
    /// Replaces the whole input and reparses it. The cursor stays within the new input.
    /// Constraints survive replacement. A byte change within the selected prefix reconciles
    /// the visible selection. Unrecognized bytes remain in the input.
    ///
    /// # Errors
    ///
    /// A parse, reconciliation or refresh error can leave the new input visible.
    pub fn replace_raw(&mut self, text: &str) -> Result<(), EngineError> {
        self.replace_raw_with_physical_separators(text, true)
    }

    /// Replaces the whole input and records whether its apostrophes were typed. Pass `false`
    /// for separators inserted between canonical syllables. The trellis uses this
    /// distinction.
    ///
    /// # Errors
    ///
    /// Propagates parse, selection-reconciliation and refresh errors after the new input and
    /// separator mode have been stored.
    pub(crate) fn replace_raw_with_physical_separators(
        &mut self,
        text: &str,
        physical_separators: bool,
    ) -> Result<(), EngineError> {
        self.input.clear_exact();
        let continuous = self.replacement_extends_selection(text);
        self.refill_raw(text);
        self.input.set_physical_separators(physical_separators);
        self.reparse()?;
        if !continuous {
            self.reconcile_replaced_selection()?;
        }
        self.refresh_after_parse()
    }

    /// Replaces the raw input with `text` parsed into exactly
    /// `segments` — the scheme-parse seam (zhuyin, double pinyin).
    ///
    /// The scan and the training record use these keys verbatim: the
    /// graph is one [`EdgeKind::Exact`](crate::graph::EdgeKind::Exact) chain, so the pinyin inventory
    /// never re-segments the joined spelling (upstream's decoder receives
    /// the scheme parser's `ChewingKey`s the same way). Segments are
    /// absolute over `text`; spans reaching past its end are dropped,
    /// keeping `end <= raw.len()` an invariant of the stored segments.
    ///
    /// Constraints survive every replacement; a discontinuous one
    /// reconciles only the selection record, as in [`Session::replace_raw`].
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the refresh under the new input hits
    /// a backend failure.
    pub(crate) fn replace_raw_exact(
        &mut self,
        text: &str,
        segments: &[ExactSegment],
    ) -> Result<(), EngineError> {
        let continuous = self.replacement_extends_selection(text);
        self.refill_raw(text);
        self.input.set_exact(segments);
        self.reparse()?;
        if !continuous {
            self.reconcile_replaced_selection()?;
        }
        self.refresh_after_parse()
    }

    /// Replaces input with the typed full pinyin `text` and the keys an
    /// index parse (Luoma, secondary zhuyin) found in it, without reparsing
    /// it as Hanyu. Unlike the double/zhuyin exact seam, full-pinyin
    /// resplit and divided matrix processing remains enabled (pin 074a2219,
    /// `pinyin.cpp:1497-1525`), and each `'` in `text` is a typed separator
    /// with its own zero-key column, as in plain full pinyin
    /// (`phonetic_key_matrix.cpp:52-79`). Segments use byte offsets in
    /// `text`; a segment reaching past its end is dropped.
    ///
    /// # Errors
    /// Returns [`EngineError`] if refreshing the new input fails.
    pub(crate) fn replace_raw_full_pinyin(
        &mut self,
        text: &str,
        segments: &[ExactSegment],
    ) -> Result<(), EngineError> {
        let continuous = self.replacement_extends_selection(text);
        self.refill_raw(text);
        self.input.set_full_pinyin(segments);
        self.reparse()?;
        if !continuous {
            self.reconcile_replaced_selection()?;
        }
        self.refresh_after_parse()
    }

    /// Reconciles the selection record to the new input without changing
    /// the stored constraints. Upstream has no selection record, and its
    /// parse never touches m_constraints (074a2219, pinyin.cpp:1497-1525).
    /// Validate a temporary copy so the record cannot retain stale text;
    /// the real store is resized and validated at the next guess.
    pub(super) fn reconcile_replaced_selection(&mut self) -> Result<(), EngineError> {
        if self.record.consumed() == 0 {
            return Ok(());
        }
        let parse = self.retained_parse();
        let bound = parse.matrix.bound;
        let mut retained = self.constraints.clone();
        if bound > 0 {
            let spelling = crate::engine::nbest::ConstraintSpelling::new(&parse.matrix);
            retained.validate(bound + 1, |start, end, token| {
                spelling.spells(&self.dictionary, start, end, token)
            })?;
        } else {
            // No selection text is valid for this matrix. The real
            // constraints still survive until something validates them.
            retained.clear();
        }
        let state = self.state.borrow_mut();
        state
            .record
            .rebuild_from_constraints(state.input.len(), &retained.runs());
        Ok(())
    }

    /// The shared body of the two replace seams: refill the raw buffer
    /// and clamp the cursor onto the new input. No refresh — the callers
    /// refresh under their own parse mode (the exact seam must set its
    /// segments first).
    pub(super) fn refill_raw(&mut self, text: &str) {
        let restoring = self.input.is_empty() && !text.is_empty() && self.constraints.is_active();
        self.input.refill(text);
        if restoring {
            // An empty parse clamped the cursor but kept the forcings.
            // Restore their selection record when the input returns.
            self.rebuild_selection_from_constraints();
        }
        // A stale consumed from the replaced composition may now sit inside
        // a multi-byte character of the new raw (`a` selected to consumed 1,
        // then `，` replaces it). `refresh`/`scan_window` slice
        // `raw[consumed..]`, so the clamp must land on a char boundary —
        // the composition restarts from the boundary before it.
        let state = self.state.borrow_mut();
        state.record.clamp_consumed(state.input.as_str());
    }

    // pinyin_set_options stores the word, pinyin.cpp:1299–1306. Existing matrix reads are at
    // pinyin.cpp:1373–1386,2184–2190.
    /// Replaces the option word and refreshes an active composition. The current matrix
    /// retains the options used for its parse. New spelling and matrix options take effect at
    /// the next parse.
    ///
    /// # Errors
    ///
    /// A refresh error leaves the new option word in effect.
    pub fn set_options(&mut self, options: OptionBits) -> Result<(), EngineError> {
        if self.options == options {
            return Ok(());
        }
        self.options = options;
        if self.input.is_empty() {
            return Ok(());
        }
        self.refresh()
    }

    /// Apply the candidate-surface sort-option word — `m_sort_option`
    /// (`pinyin.cpp:2203` at the pin), re-stored by every
    /// `pinyin_guess_candidates` and read by `pinyin_choose_candidate`.
    ///
    /// The word's bits: `0x1` `SORT_WITHOUT_SENTENCE_CANDIDATE` (set →
    /// the n-best rows are not prepended, so the phrase-string dedup never
    /// meets them — `pinyin.cpp:2295-2300`), `0x2`
    /// `SORT_WITHOUT_LONGER_CANDIDATE` (clear →
    /// the LONGER row is built and prepended),
    /// `0x4`/`0x8`/`0x10` `SORT_BY_{PHRASE_LENGTH,PINYIN_LENGTH,FREQUENCY}`
    /// (each key ordered descending only while its bit is set; a disabled
    /// key compares equal, so its ties fall to the keys after it and then
    /// to the collection order).
    ///
    /// Refreshes a composing session, exactly `set_options`'s contract:
    /// the cached list's order and LONGER row are functions of the word.
    /// Under `Session::set_defer_list_builds` a change leaves the list
    /// stale instead, for `Session::refresh_candidates`: the pin takes the
    /// word as a `pinyin_guess_candidates` argument and builds once under it
    /// (`pinyin.cpp:2184-2198` at 074a2219), as `Session::candidates_at`
    /// does.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a composing session fails to refresh
    /// under the new word.
    pub fn set_sort_options(&mut self, sort_word: u32) -> Result<(), EngineError> {
        if self.lookup.sort_word == sort_word {
            return Ok(());
        }
        self.lookup.sort_word = sort_word;
        if self.input.is_empty() {
            return Ok(());
        }
        if self.defer_list_builds {
            self.lookup.stale = true;
            return Ok(());
        }
        self.refresh()
    }

    /// Sets the surface's upstream law ([`Law`]). [`Law::Pinyin`] by default.
    ///
    /// Under [`Law::Zhuyin`] the prepended sentence rows collapse onto the
    /// 1-best row — the libzhuyin display law; the pinyin surface keeps one
    /// candidate row per n-best sentence.
    ///
    /// The two upstream surfaces prepend the same `m_nbest_results.size()`
    /// rows but fill their strings differently: pinyin fills row `i` through
    /// `pinyin_get_sentence(instance, m_nbest_index, …)`, keeping one distinct
    /// string per row (`pinyin.cpp:2004-2007`), while zhuyin fills **every**
    /// `BEST_MATCH_CANDIDATE` row through `zhuyin_get_sentence`, which always
    /// reads `get_result(0)` (`zhuyin.cpp:1327-1330`, `:990-995` at the pin
    /// 0c5e80e1). Identical strings then collide in
    /// `_remove_duplicated_items_by_phrase_string`, which physically removes
    /// the duplicates while keeping the `BEST_MATCH` row
    /// (`zhuyin.cpp:1425-1438`), so the observable list carries exactly one
    /// sentence row no matter how many n-best sentences were decoded.
    /// Prepending only the 1-best row is that pipeline's net effect, and it
    /// is what keeps the phrase rows upstream keeps: a phrase whose text
    /// equals a non-first row's own text never collides there (the rows all
    /// display the 1-best string), so it must not be absorbed here either.
    ///
    /// The zhuyin law also picks the facade's parse order: a full-pinyin
    /// parse omits the resplit and divided passes (`zhuyin.cpp:1038-1040`).
    /// Like the option word, that reaches the matrix at the next parse. The
    /// comparator applies at the next list build, and the trellis shape at
    /// the next sentence lookup.
    pub fn set_law(&mut self, law: Law) {
        self.law = law;
    }

    /// Selects whether the parse seams ([`Session::replace_raw`] and its
    /// variants), the sentence guesses ([`Session::guess_sentence`],
    /// [`Session::guess_sentence_with_prefix`]), a sort-word change
    /// ([`Session::set_sort_options`]) and a selection
    /// ([`Session::select_target`]) build the candidate list.
    ///
    /// Off by default: each builds it, as the session API always has. On,
    /// the parse seams and the guesses still build the scan matrix, so its
    /// errors surface where they did, and all four leave the list stale.
    /// A reader of [`Session::candidates`] or [`Session::full_parsed_len`] calls
    /// `Session::refresh_candidates` first. A failure of the deferred build then surfaces from that call
    /// rather than from the parse or the guess. This is the C facades'
    /// mode: upstream builds candidates only in `pinyin_guess_candidates`
    /// (`pinyin.cpp:2184-2308` at 074a2219), which the C ABI answers from
    /// its own re-anchored window ([`Session::candidates_at`]).
    pub(crate) fn set_defer_list_builds(&mut self, defer: bool) {
        self.defer_list_builds = defer;
    }
}
