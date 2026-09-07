//! Lifecycle and settings: opening a session, resetting it, replacing the raw buffer, the live option and shape switches, and the read-only accessors (preedit, candidates, composition offset).
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
    /// Opens a session over the supplied backends.
    ///
    /// Configuration and storage locations arrive as data; the session reads
    /// no environment and discovers no path.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend rejects the settings it is
    /// opened with, or — on the pre-frequency fallback only — when a
    /// backend fails while the key-cost table is walked. A model carrying
    /// real unigram frequencies never walks, so no such rejection exists
    /// for it and construction currently always succeeds.
    pub fn new(
        config: &dyn ConfigSource,
        paths: StoragePaths,
        dictionary: D,
        model: L,
    ) -> Result<Self, EngineError> {
        // Only the fallback scorer reads the table (see `key_costs`), and
        // it is unreachable under real frequencies. The fallback keeps the
        // SPEC's construction-time walk verbatim: costs complete before any
        // sweep, so `EdgeCost` still cannot fail, and a backend failure
        // still surfaces here rather than inside the search.
        let key_costs = if model.has_real_unigrams() {
            Vec::new()
        } else {
            key_cost_table(&dictionary, &model)?
        };
        Self::init(config, paths, dictionary, model, key_costs)
    }

    /// Builds a session over an already-computed key-cost table.
    ///
    /// [`Runtime::new_session`](oxpinyin_runtime) uses this to skip the
    /// per-session [`key_cost_table`] recomputation: the table is computed
    /// once at [`Runtime::open`](oxpinyin_runtime) time and reused.
    pub fn new_with_key_costs(
        config: &dyn ConfigSource,
        paths: StoragePaths,
        dictionary: D,
        model: L,
        key_costs: Vec<Cost>,
    ) -> Result<Self, EngineError> {
        Self::init(config, paths, dictionary, model, key_costs)
    }

    pub(super) fn init(
        config: &dyn ConfigSource,
        paths: StoragePaths,
        dictionary: D,
        model: L,
        key_costs: Vec<Cost>,
    ) -> Result<Self, EngineError> {
        Ok(Self {
            dictionary,
            model,
            paths,
            settings: Settings::read(config),
            raw: String::new(),
            selected: String::new(),
            consumed: 0,
            parsed_prefix: 0,
            exact_segments: Vec::new(),
            candidates: CandidateList::default(),
            history: Vec::new(),
            scoring: ScoringConfig::default(),
            key_costs,
            nbest_rows: Vec::new(),
            nbest_history: Vec::new(),
            sentence_lookup_active: false,
            collapse_sentence_rows_to_best: false,
            nbest_shape: crate::nbest::NbestShape::default(),
            selection_committed: false,
            constraints: crate::constraint::ConstraintStore::default(),
            last_result: Vec::new(),
            scratch_collected: Vec::new(),
            scratch_ranked: Vec::new(),
            scratch_entries: Vec::new(),
            scratch_path: SmallVec::new(),
            scratch_window_phrase: Vec::new(),
            scratch_window_addon: Vec::new(),
        })
    }

    /// Replaces the scoring weights used by subsequent refreshes.
    ///
    /// Does not recompute the per-key cost table (that depends only on the
    /// dictionary and language model). Intended for constant sweeps and
    /// measurements; interactive shells normally keep [`ScoringConfig::default`].
    pub const fn set_scoring_config(&mut self, config: ScoringConfig) {
        self.scoring = config;
    }

    /// The scoring weights currently in force.
    #[must_use]
    pub const fn scoring_config(&self) -> &ScoringConfig {
        &self.scoring
    }

    /// Discards the composition.
    ///
    /// The full reset — upstream's `pinyin_reset`: the input, the
    /// selection record, the n-best rows, and the constraint store all
    /// go (`pinyin.cpp:2697` clears `m_constraints`).
    pub fn reset(&mut self) {
        self.reset_composition();
        self.exact_segments.clear();
        self.raw.clear();
        self.selected.clear();
        self.consumed = 0;
        self.selection_committed = false;
        self.history.clear();
        self.constraints.clear();
    }

    /// The parse-path reset: the composition's PARSE state goes; the
    /// raw input, the selection record, and the constraint store stay.
    ///
    /// `pinyin_parse_more_full_pinyins` replaces the input buffer — the
    /// frontend re-sends the whole buffer every keystroke — and never
    /// touches upstream's instance-level `m_constraints` or the chosen
    /// cursor (`pinyin.cpp:1497-1533`); the next `guess_sentence`
    /// re-validates the surviving forcings against the new matrix. This
    /// is that split's engine half: the L2 lifetime rule
    /// (`docs/findings/live-typing.md`).
    ///
    /// The raw buffer is NOT cleared here: a cleared raw with a surviving
    /// cursor would leave `consumed > raw.len()` observable, and every
    /// `raw[consumed..]` slice (`preedit`, `commit`) would panic — the
    /// constitution forbids that window. The input replacement is atomic
    /// in [`Session::replace_raw`], which clears, refills, clamps, and
    /// refreshes in one call, so this reset alone always leaves a
    /// consistent session.
    pub fn reset_composition(&mut self) {
        self.parsed_prefix = 0;
        self.candidates = CandidateList::default();
        self.nbest_rows.clear();
        self.nbest_history.clear();
        self.last_result.clear();
        self.sentence_lookup_active = false;
    }

    /// Replaces the raw input with `text` in one step — the capi parse
    /// path's `parse_more` contract (the frontend re-sends the whole
    /// buffer every keystroke). The selection record and the constraint
    /// store survive (whether they should is the caller's
    /// [`Session::parse_continues`] decision); the cursor is clamped into
    /// the new buffer and the candidates refresh, so the session is never
    /// observable with a cursor past its input. A replacement that does
    /// not extend the covered selection span — a clamp below it, or a
    /// byte divergence inside it — reconciles the store and record
    /// ([`Session::reconcile_replaced_selection`]), so
    /// [`Session::commit`] answers only text valid for the current
    /// input.
    ///
    /// Keeps every character: the pin's parser accepts any input string
    /// and simply stops consuming at the first byte no key matches
    /// (`pinyin_parser2.cpp:237-328` — there is no explicit stop, the
    /// termination is the DP's reachability), so space, control, and
    /// non-ASCII bytes must REACH the decoder for it to stop there
    /// (class B2 of `uncovered-surface-differentials.md`). The decoder
    /// hard-stops on them; this seam must not pre-filter them away.
    ///
    /// The batch [`Session::type_pinyin`] keeps its printable-ASCII
    /// accept set (the frozen F1 design, `f1-junk-aware-parse.md`): the
    /// two seams are deliberately different. The corpus and sentence pins
    /// feed through `type_pinyin` only — no path reaches this seam — so
    /// the loosened filter here cannot move them.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the refresh under the new input hits
    /// a backend failure.
    pub fn replace_raw(&mut self, text: &str) -> Result<(), EngineError> {
        self.exact_segments.clear();
        let continuous = self.replacement_extends_selection(text);
        self.refill_raw(text);
        if !continuous {
            self.reconcile_replaced_selection()?;
        }
        self.refresh()
    }

    /// Replaces the raw input with `text` parsed into exactly
    /// `segments` — the scheme-parse seam (zhuyin, double pinyin).
    ///
    /// The scan and the training record use these keys verbatim: the
    /// graph is one [`EdgeKind::Exact`] chain, so the pinyin inventory
    /// never re-segments the joined spelling (upstream's decoder receives
    /// the scheme parser's `ChewingKey`s the same way). Segments are
    /// absolute over `text`; spans outside the accepted prefix of
    /// `text` (the [`MAX_INPUT_BYTES`] clamp) are dropped, keeping
    /// `end <= raw.len()` an invariant of the stored segments.
    ///
    /// The selection record and the constraint store survive while the
    /// replacement extends the covered span, and a discontinuous one
    /// reconciles, exactly as in [`Session::replace_raw`].
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the refresh under the new input hits
    /// a backend failure.
    pub fn replace_raw_exact(
        &mut self,
        text: &str,
        segments: &[ExactSegment],
    ) -> Result<(), EngineError> {
        let continuous = self.replacement_extends_selection(text);
        self.refill_raw(text);
        let raw_len = self.raw.len();
        self.exact_segments = segments
            .iter()
            .copied()
            .filter(|segment| segment.end() <= raw_len)
            .collect();
        if !continuous {
            self.reconcile_replaced_selection()?;
        }
        self.refresh()
    }

    /// Whether `text` extends the bytes the selection record was built
    /// over (`raw[..consumed]`) — the continuity retaining the record
    /// requires. The parse seams' own prefix checks run in the caller's
    /// coordinates (the scheme parses' original input), while the record
    /// lives in these canonical bytes: a replacement that passes those
    /// checks but does not extend the covered span — a scheme switch
    /// that decodes the same codes to a different spelling, any
    /// transform divergence — must reconcile, or [`Session::commit`]
    /// combines the stale selection with the new raw suffix.
    pub(super) fn replacement_extends_selection(&self, text: &str) -> bool {
        text.as_bytes()
            .starts_with(&self.raw.as_bytes()[..self.consumed])
    }

    /// Reconciles the selection to a replacement it does not extend:
    /// the full validate the next guess would run — bounds and spelling
    /// over the new input's matrix, so a forcing that no longer spells
    /// under the divergent replacement drops here instead of surviving
    /// under a stale record — then the record re-derived from the
    /// surviving runs, whatever the validate dropped. A backward clamp
    /// is the runs-empty extreme: the whole record goes and the
    /// composition re-opens at 0. The empty-record parse path pays only
    /// the continuity check.
    pub(super) fn reconcile_replaced_selection(&mut self) -> Result<(), EngineError> {
        if self.consumed == 0 {
            return Ok(());
        }
        let graph = self.build_graph_at(0, self.raw.as_bytes())?;
        let bound = graph.consumed();
        if bound > 0 {
            let matrix = build_scan_matrix(
                &graph,
                self.settings.options,
                self.exact_segments.is_empty(),
            );
            self.constraints.validate(bound + 1, |start, end, token| {
                crate::nbest::span_finds_token(&matrix, start, end, token, &self.dictionary)
            })?;
        } else {
            // Nothing spells over the replaced buffer: every forcing is
            // dead and the record with it.
            self.constraints.clear();
        }
        self.rebuild_selection_from_constraints();
        Ok(())
    }

    /// The shared body of the two replace seams: refill the raw buffer
    /// and clamp the cursor onto the new input. No refresh — the callers
    /// refresh under their own parse mode (the exact seam must set its
    /// segments first).
    pub(super) fn refill_raw(&mut self, text: &str) {
        self.raw.clear();
        for character in text.chars() {
            if self.raw.len() + character.len_utf8() > MAX_INPUT_BYTES {
                break;
            }
            self.raw.push(character);
        }
        // A stale consumed from the replaced composition may now sit inside
        // a multi-byte character of the new raw (`a` selected to consumed 1,
        // then `，` replaces it). `refresh`/`scan_window` slice
        // `raw[consumed..]`, so the clamp must land on a char boundary —
        // the composition restarts from the boundary before it.
        self.consumed = self.consumed.min(self.raw.len());
        while !self.raw.is_char_boundary(self.consumed) {
            self.consumed -= 1;
        }
    }

    /// Filtered parse length of the remaining input after the last refresh.
    ///
    /// This is the last byte of [`SegmentGraph::fewest_keys`] under the
    /// session's `incomplete-pinyin` setting — not the unfiltered
    /// [`SegmentGraph::consumed`].
    #[must_use]
    pub const fn parsed_prefix_len(&self) -> usize {
        self.parsed_prefix
    }

    /// Apply a live `incomplete-pinyin` change and refresh if composing.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a composing session fails to refresh
    /// under the new setting.
    pub fn set_incomplete_pinyin(&mut self, enabled: bool) -> Result<(), EngineError> {
        let options = self
            .settings
            .options
            .with(oxpinyin_core::PINYIN_INCOMPLETE, enabled);
        self.set_options(options)
    }

    /// Apply a live option-word change and refresh if composing.
    ///
    /// This is the engine half of `pinyin_set_options`: correction and
    /// ambiguity bits remask already-allocated sessions on the next parse or
    /// guess. The C ABI stores the raw word and calls this before parse/guess.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a composing session fails to refresh
    /// under the new options.
    pub fn set_options(&mut self, options: OptionBits) -> Result<(), EngineError> {
        if self.settings.options == options {
            return Ok(());
        }
        self.settings.options = options;
        if self.raw.is_empty() {
            return Ok(());
        }
        self.refresh()
    }

    /// Collapses the prepended sentence rows onto the 1-best row — the
    /// libzhuyin display law. Off by default: the pinyin surface keeps one
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
    /// the duplicates while keeping the BEST_MATCH row
    /// (`zhuyin.cpp:1425-1438`), so the observable list carries exactly one
    /// sentence row no matter how many n-best sentences were decoded.
    /// Prepending only the 1-best row is that pipeline's net effect, and it
    /// is what keeps the phrase rows upstream keeps: a phrase whose text
    /// equals a non-first row's own text never collides there (the rows all
    /// display the 1-best string), so it must not be absorbed here either.
    pub fn set_collapse_sentence_rows_to_best(&mut self, collapse: bool) {
        self.collapse_sentence_rows_to_best = collapse;
    }

    /// Selects the n-best trellis's `<nstore, nbest>` for this surface —
    /// upstream instantiates `PhoneticLookup<2, 3>` for libpinyin
    /// (`pinyin.cpp:55`) and `PhoneticLookup<1, 1>` for libzhuyin
    /// (`zhuyin.cpp:50`), so the two facades prune the trellis to different
    /// depths and extract a different number of sentence tails. The pinyin
    /// shape is the default; a zhuyin facade sets
    /// [`NbestShape::ZHUYIN`](crate::NbestShape::ZHUYIN) at instance
    /// allocation, beside the sentence-row display law. Takes effect at the
    /// next sentence lookup.
    pub const fn set_nbest_shape(&mut self, shape: crate::nbest::NbestShape) {
        self.nbest_shape = shape;
    }

    /// What the shell should display.
    #[must_use]
    pub fn preedit(&self) -> Preedit {
        let remaining = &self.raw[self.consumed..];
        if self.selected.is_empty() && remaining.is_empty() {
            return Preedit::default();
        }

        let mut text = self.selected.clone();
        text.push_str(remaining);

        let mut spans = Vec::with_capacity(2);
        if !self.selected.is_empty() {
            spans.push(PreeditSpan::new(
                0,
                self.selected.len(),
                SpanStyle::Selected,
            ));
        }
        if !remaining.is_empty() {
            spans.push(PreeditSpan::new(
                self.selected.len(),
                text.len(),
                SpanStyle::Raw,
            ));
        }

        let cursor = text.len();
        Preedit::new(text, spans, cursor)
    }

    /// The current candidates, in rank order.
    ///
    /// Sentence rows appear at the head of this list only after
    /// [`Session::guess_sentence`] has run for the current composition:
    /// upstream's candidate list prepends its n-best rows exactly when
    /// `m_nbest_results` is non-empty (`pinyin.cpp:2292-2293`), and the
    /// corpus pins were captured without a sentence guess.
    #[must_use]
    pub const fn candidates(&self) -> &CandidateList {
        &self.candidates
    }

    /// Whether a sentence lookup has run since the last reset.
    ///
    /// The lookup-active half of the gate: while this is true,
    /// `pinyin_get_sentence` answers decoded-or-nothing (upstream's
    /// `0 == results.size()` false) and never the pre-lookup raw form,
    /// even when the lookup produced no rows.
    #[must_use]
    pub const fn sentence_lookup_active(&self) -> bool {
        self.sentence_lookup_active
    }

    /// The decoded text of n-best row `index`, best-first
    /// (`pinyin_get_sentence`'s payload). `None` when fewer rows exist.
    #[must_use]
    pub fn sentence_text(&self, index: u8) -> Option<&str> {
        self.nbest_rows
            .get(usize::from(index))
            .map(|row| row.text.as_str())
            .filter(|text| !text.is_empty())
    }

    /// The raw input typed so far.
    #[must_use]
    pub fn raw_input(&self) -> &str {
        &self.raw
    }

    /// Whether a composition is in progress.
    #[must_use]
    pub const fn is_composing(&self) -> bool {
        !self.raw.is_empty()
    }

    /// Bytes of the raw input consumed by selections so far — the
    /// composition offset the candidate lookup is anchored at.
    ///
    /// After a successful [`Session::select`] this is the chosen
    /// candidate's absolute end position: the previous anchor plus the
    /// candidate's span (separator run included), never past the raw
    /// input. The C ABI answers it as the new lookup cursor — the caller
    /// offset may sit past a separator run the span also covers, so
    /// caller-offset-plus-span would count that run twice
    /// (libpinyin@412f88e3 instead anchors `m_begin` at the caller offset,
    /// reaching the same end).
    #[must_use]
    pub const fn composition_offset(&self) -> usize {
        self.consumed
    }

    /// Candidates per page, from the configuration the session was opened
    /// with.
    #[must_use]
    pub const fn page_size(&self) -> usize {
        self.settings.page_size
    }

    /// The storage locations the session was opened with.
    #[must_use]
    pub const fn paths(&self) -> &StoragePaths {
        &self.paths
    }

    /// The dictionary backend.
    #[must_use]
    pub const fn dictionary(&self) -> &D {
        &self.dictionary
    }

    /// The language model backend.
    #[must_use]
    pub const fn language_model(&self) -> &L {
        &self.model
    }
}
