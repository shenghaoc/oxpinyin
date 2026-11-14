//! Selection and constraints: choosing a candidate from the cached list or a re-anchored window, the §3 constraint writes, the selection record and training.
//!
//! One of the `impl Session` slices `session/mod.rs` declares; every
//! method here was moved verbatim from the single 5,800-line
//! `session.rs` (2026-09-08) and keeps its doc comment and upstream
//! citations. Shared state, constants and free functions stay in the
//! parent module.

use super::*;
use crate::constraint::PhraseSpan;
use crate::error::TrainingSpanFault;

/// What a choose reads off the row it selects: the pin's `m_begin`,
/// `m_end` and `m_token`, and for an n-best row `m_nbest_index`
/// (`lookup_candidate_t`, 074a2219 `pinyin.cpp:97-105`; read at
/// `:2515-2520` and `:2577-2590`, `zhuyin.cpp:1646-1666`), plus the text
/// the selection record keeps. Both spans are absolute, in the store's
/// coordinates.
#[derive(Clone, Copy, Debug)]
pub struct SelectionTarget<'a> {
    /// Where the chosen span starts (`m_begin`).
    pub span_start: usize,
    /// Where the chosen span ends, before the separator walk
    /// (`m_end`).
    pub span_end: usize,
    /// The token the choose forces (`m_token`); `None` for a row without
    /// one.
    pub token: Option<PhraseToken>,
    /// The n-best tail rank, for a sentence row only.
    pub nbest_row: Option<u8>,
    /// The chosen phrase's text, stored words included.
    pub text: &'a PhraseText,
}

impl<'a> SelectionTarget<'a> {
    /// The target of `candidate` read in a window anchored at `anchor`,
    /// forcing `token_override` in place of its own token when given (the
    /// addon promotion, `pinyin.cpp:2532-2562`).
    ///
    /// For the composition-anchored cached list, `anchor` is the
    /// composition offset and the candidate's `consumed_bytes` is measured
    /// from it; for an after-cursor window, `anchor` is that window's
    /// caller offset and `consumed_bytes` is measured from IT (the pin's
    /// `m_begin = start`, `pinyin.cpp:2246`) — both carry `span_start` 0,
    /// so the span starts at `anchor`. A before-cursor window is
    /// END-anchored at 0 with each row's own absolute `m_begin` in
    /// `span_start` and its absolute end in `consumed_bytes`: the span
    /// starts at `anchor + span_start`, not at the anchor, so choosing the
    /// second key's span constrains that key alone and leaves the leading
    /// key to the decode.
    #[must_use]
    pub fn of_candidate(
        candidate: &'a Candidate,
        anchor: usize,
        token_override: Option<PhraseToken>,
    ) -> Self {
        Self {
            span_start: anchor.saturating_add(candidate.span_start()),
            span_end: anchor.saturating_add(candidate.consumed_bytes()),
            token: token_override.or_else(|| candidate.token()),
            nbest_row: candidate.nbest_row(),
            text: candidate.text(),
        }
    }
}

/// A checked training walk. It borrows composition state only, so all
/// dictionary/model readers can end before the user store is mutated.
pub struct TrainingPlan<'a> {
    state: &'a SessionState,
    index: u8,
    spans: Option<SpanPlan<'a>>,
}

impl TrainingPlan<'_> {
    /// Observe the already checked steps in their original order.
    ///
    /// # Errors
    /// User-model errors retain the observations before the failure.
    pub fn observe<U: UserModel<Token = PhraseToken>>(
        self,
        user: &mut U,
    ) -> Result<bool, EngineError>
    where
        U::Error: Display,
    {
        let row = &self.state.sentence.rows[usize::from(self.index)];
        if row.spans.is_empty() {
            let history = self.state.record.history();
            for (index, token) in history.iter().enumerate() {
                user.observe(&history[..index], token)
                    .map_err(|error| EngineError::UserModel(error.to_string()))?;
            }
        } else if let Some((parsed, ends)) = self.spans {
            let mut context = Vec::with_capacity(row.spans.len());
            for (index, span) in row.spans.iter().enumerate() {
                if let Some(end) = ends[index] {
                    let readings = training_readings(
                        &parsed.columns,
                        self.state.input.as_bytes(),
                        span.start,
                        end,
                        span.text.phrase_length(),
                    );
                    user.observe_with_keys(&context, &span.token, readings)
                        .map_err(|error| EngineError::UserModel(error.to_string()))?;
                }
                context.push(span.token);
            }
        }
        Ok(true)
    }
}

impl<'s, D, L> Session<D, L, &'s SessionState>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// The retained matrix, borrowed for the composition state's own
    /// lifetime so a plan can keep it. The checks that precede this call
    /// have built it; the build is repeated only if they did not.
    fn plan_matrix(&self) -> Result<&'s super::matrix::ParsedMatrix, EngineError> {
        let state: &'s SessionState = self.state;
        if let Some(parse) = state.input.parse.get() {
            return Ok(&parse.matrix);
        }
        let built = self.build_retained_parse()?;
        Ok(&state.input.parse.get_or_init(|| built).matrix)
    }

    /// Check one n-best training operation without retaining a storage reader.
    ///
    /// # Errors
    /// The ordered index, constraint, store and span checks of
    /// [`Self::train_nbest_preflight`], plus the user model's per-step store checks.
    pub fn prepare_training(
        &self,
        index: u8,
        check: &mut impl FnMut(&[PhraseToken], &PhraseToken) -> Result<(), EngineError>,
    ) -> Result<Option<TrainingPlan<'s>>, EngineError> {
        let Some(row) = self.nbest_training_row(index)? else {
            return Ok(None);
        };
        let spans = if row.spans.is_empty() {
            let history = self.record.history();
            for (index, token) in history.iter().enumerate() {
                check(&history[..index], token)?;
            }
            None
        } else {
            match self.plan_spans(&row.spans, check)? {
                Some(ends) => Some((self.plan_matrix()?, ends)),
                None => None,
            }
        };
        Ok(Some(TrainingPlan {
            state: self.state,
            index,
            spans,
        }))
    }
}

impl<D, L, S: std::borrow::Borrow<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// The common tail of [`Session::select_target`].
    pub(super) fn selection_outcome(&self) -> Selection {
        if self.record.consumed() >= self.input.len() {
            Selection::Completed
        } else {
            Selection::Continued
        }
    }

    /// The pin's own aborts of training row `index`, without the user
    /// model: the index past nonempty results (`pinyin.cpp:2684`), a stale
    /// forced phrase (`phonetic_lookup.h:868`) and a span outside the
    /// current matrix (`storage/phonetic_key_matrix.cpp:661`, `:663`,
    /// `:664`), in the order [`Self::prepare_training`] meets them. It runs
    /// the same row lookup and span walk as `prepare_training`, with the
    /// user model's per-step store checks left out, and observes nothing.
    /// `Ok(())` where `prepare_training` would reach none of them (it may
    /// still meet a store fault).
    ///
    /// # Errors
    ///
    /// [`EngineError::CandidateIndexOutOfRange`],
    /// [`EngineError::StaleTrainingConstraint`] or
    /// [`EngineError::StaleTrainingSpan`]; [`EngineError::Graph`] when the
    /// raw buffer cannot be built into a segment graph.
    pub fn train_nbest_preflight(&self, index: u8) -> Result<(), EngineError> {
        if let Some(row) = self.nbest_training_row(index)?
            && !row.spans.is_empty()
        {
            self.plan_spans(&row.spans, &mut |_, _| Ok(()))?;
        }
        Ok(())
    }

    /// The decoded row [`Session::prepare_training`] trains: `None` for no
    /// results (the pin's quiet `false`), the bounds error past nonempty
    /// results.
    fn nbest_training_row(
        &self,
        index: u8,
    ) -> Result<Option<&crate::nbest::NbestRow>, EngineError> {
        if self.sentence.rows.is_empty() {
            return Ok(None);
        }
        self.sentence.rows.get(usize::from(index)).map(Some).ok_or(
            EngineError::CandidateIndexOutOfRange {
                index: usize::from(index),
                len: self.sentence.rows.len(),
            },
        )
    }

    /// The training walk's checks over `spans`, in the pin's step order,
    /// before anything is observed: each step's forced-phrase assert, then
    /// `check` (the user model's store checks for the step), then the span
    /// asserts. Answers each observed span's end, or `None`
    /// when no span is forced (nothing trains).
    fn plan_spans<F>(
        &self,
        spans: &[PhraseSpan],
        check: &mut F,
    ) -> Result<Option<Vec<Option<usize>>>, EngineError>
    where
        F: FnMut(&[PhraseToken], &PhraseToken) -> Result<(), EngineError>,
    {
        // 074a2219 phonetic_lookup.h:854-866,932: only OneStep or
        // train_next observes a phrase; every phrase advances its predecessor.
        // A no-op needs no matrix rebuild: train_next starts false
        // and only a OneStep can start training (pin :854-872).
        if !spans
            .iter()
            .any(|span| self.constraints.is_one_step_at(span.start))
        {
            return Ok(None);
        }
        // `assert(token == constraint->m_token)` at a forced position
        // (`phonetic_lookup.h:868`): checked in the walk below, in span
        // order, since the pin's abort leaves nothing behind.
        // The walk indexes raw-buffer positions, separators included. The
        // pin's `fill_matrix` leaves a leading separator gap empty
        // (`phonetic_key_matrix.cpp:52-78`), but the raw scan has already
        // re-anchored a key that crosses a separator to its buffer start.
        // The retained topology re-applies the physical separators
        // (`ParsedMatrix::from_scan` — the matrix the lookup indexes), or a
        // span over the gap would silently read a filled column `:663`
        // refuses.
        let parsed = &self.retained_parse()?.matrix;
        let matrix = &parsed.columns;
        let zeros = &parsed.zeros;
        // 074a2219 phonetic_lookup.h:921 tails the last span at
        // `constraints->length() - 1`, NOT the CURRENT matrix's last
        // column: the store survives a parse (`pinyin.cpp:1497-1525`,
        // never validating), so a re-parse that shortened the input
        // leaves it longer than the matrix the walk now indexes. The
        // pin's `increase_pronunciation_possibility` then asserts the
        // walk stays inside and starts on a filled column
        // (`storage/phonetic_key_matrix.cpp:661`, `:663`), which the
        // engine answers as an error before observing anything; the C
        // facades abort there. The endpoints are pre-computed in one pass
        // so the error precedes every observation, as the pin's abort
        // leaves the process dead.
        let constraint_len = self.constraints.cell_count();
        let tail = constraint_len.saturating_sub(1);
        let mut train_next = false;
        let mut ends: Vec<Option<usize>> = vec![None; spans.len()];
        let tokens: Vec<PhraseToken> = spans.iter().map(|span| span.token).collect();
        for (index, span) in spans.iter().enumerate() {
            // 074a2219 phonetic_lookup.h:858 walks the result positions
            // below `constraints->length()`, reloaded each step (the
            // pin-built `pinyin_train` compares `i` with the `GArray`'s
            // `len`). A choose after a shortening re-parse shrinks the store
            // (`pinyin.cpp:2579`) under a row decoded for the longer input:
            // its later spans are never reached, trained or checked.
            if span.start >= constraint_len {
                break;
            }
            let forced = self.constraints.is_one_step_at(span.start);
            if !(train_next || forced) {
                continue;
            }
            // 074a2219 train_result3 walks the spans in order: the forced
            // position's `assert(token == constraint->m_token)`
            // (`phonetic_lookup.h:868`) precedes
            // `increase_pronunciation_possibility`'s three asserts
            // (`storage/phonetic_key_matrix.cpp:661`, `:663`, `:664`) for
            // the same span, so an earlier span's bounds fault is reached
            // before a later span's token fault. Both are preflight.
            if forced
                && self
                    .constraints
                    .one_step_token_at(span.start)
                    .is_some_and(|forced_token| forced_token != span.token)
            {
                return Err(EngineError::StaleTrainingConstraint {
                    position: span.start,
                });
            }
            // The step's bigram half comes next (`phonetic_lookup.h:876-909`:
            // `get_total_freq` :884, the `get_freq` walk :888, `set_freq`
            // :903), before its pronunciation update's span asserts (:925).
            check(&tokens[..index], &span.token)?;
            train_next = forced;
            // 074a2219 phonetic_lookup.h:911-920 scans to the next
            // non-null result token, not the forced constraint end.
            let next = spans.get(index + 1).map_or(tail, |next| next.start);
            // :921 clamps the last span to constraints->length()-1.
            let end = next.min(tail);
            if end >= matrix.len() {
                return Err(EngineError::StaleTrainingSpan {
                    position: span.start,
                    fault: TrainingSpanFault::PastMatrixEnd,
                });
            }
            if column_is_empty(matrix, zeros, span.start) {
                return Err(EngineError::StaleTrainingSpan {
                    position: span.start,
                    fault: TrainingSpanFault::EmptyStartColumn,
                });
            }
            if column_is_empty(matrix, zeros, end) {
                return Err(EngineError::StaleTrainingSpan {
                    position: span.start,
                    fault: TrainingSpanFault::EndEmptyColumn,
                });
            }
            ends[index] = Some(end);
        }
        Ok(Some(ends))
    }

    /// The sentence recorded so far: the token of every phrase the user
    /// pinned in this composition, in selection order.
    ///
    /// Sentence-level candidates carry no token and are not part of the
    /// record — exactly the phrases a `pinyin_train` call would train
    /// (`docs/findings/user-store.md` §2.1). The C ABI uses the tail of this
    /// slice as the predecessor for predicted-candidate training (§2.3).
    #[must_use]
    pub fn selected_tokens(&self) -> &[PhraseToken] {
        self.record.history()
    }

    /// Text retained by selections, including raw gaps before anchored choices.
    /// Unselected trailing input remains in [`Session::raw_input`] after
    /// [`Session::composition_offset`].
    #[must_use]
    pub fn selected_text(&self) -> &PhraseText {
        self.record.selected()
    }
}

impl<D, L, S: std::borrow::BorrowMut<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Chooses the row `target` describes: the span, token and n-best rank
    /// a published row carries, as the pin's choose reads them off the
    /// `lookup_candidate_t` alone (`m_begin`, `m_end`, `m_token`,
    /// `m_nbest_index`; 074a2219 `pinyin.cpp:2515-2520`, `:2577-2590`,
    /// `zhuyin.cpp:1646-1666`); [`SelectionTarget::of_candidate`] reads
    /// one off a candidate list's row.
    ///
    /// # Errors
    ///
    /// [`EngineError`] from the constraint validation and the refresh that
    /// follow the selection; a choose behind the composition with no token
    /// is [`EngineError::SelectionAnchorBeforeComposition`]. A normal span
    /// beyond the current matrix returns [`Selection::Refused`].
    pub fn select_target(
        &mut self,
        target: &SelectionTarget<'_>,
    ) -> Result<Selection, EngineError> {
        let span_end = target.span_end;
        if target.nbest_row.is_none()
            && target.token.is_some()
            && span_end > self.selection_constraint_size()?
        {
            // Both choose functions validate before and after add_constraint,
            // even when it returns zero (074a2219 pinyin.cpp:2578-2587;
            // zhuyin.cpp:1646-1666). Do not clip the retained span or add its
            // token/text to the record. The ABI owns the refusal cursor.
            self.validate_selection_constraints()?;
            self.validate_selection_constraints()?;
            return Ok(Selection::Refused);
        }
        // The chosen phrase's own text, stored words included: the pin's
        // choose reads no text (`add_constraint(m_begin, m_end, token)`,
        // `pinyin.cpp:2577-2587`, `zhuyin.cpp:1646-1666` at 074a2219), and
        // the C sentence read converts each phrase from its stored words
        // (`src/lookup/lookup.cpp:39-68`), so the record keeps them too.
        let text = target.text.clone();
        let token = target.token;
        // The chosen span in the store's coordinates — upstream's
        // `add_constraint(m_begin, m_end, token)` (`pinyin.cpp:2582`,
        // `zhuyin.cpp:1652,1659`); [`SelectionTarget::of_candidate`] says
        // how a window row's span is placed there.
        let span_start = target.span_start;
        let constraint_start = span_start;
        let constraint_end = self.next_boundary(span_end);
        // A span that starts before the composition offset — a choose from
        // a window the caller looked up behind an earlier selection (ibus's
        // `moveCursorLeft`, `PYPPhoneticEditor.cc:595-604`, sets the lookup
        // cursor to 0). This was refused as a backward selection no
        // frontend drives; amended 2026-09-27 (maintainer ruling, register row
        // 37): the pin keeps no composition offset — its window is built
        // from `start = offset` (`pinyin.cpp:2229`, `m_begin = start` at
        // `:2246`) and the choose writes `add_constraint(m_begin, m_end,
        // token)` over whatever forcings the span overlaps (`:2578-2590`,
        // `phonetic_lookup.cpp:61-86`) — so the choose forces the span and
        // the record follows the store, moving back to it. An n-best row
        // is a whole-composition hypothesis and takes the row path below.
        if span_start < self.record.consumed() && target.nbest_row.is_none() {
            return self.select_behind(constraint_start, constraint_end, token, text);
        }
        // The raw bytes between the composition offset and the span start
        // were typed without being selected. For a re-anchored selection
        // (span start > composition offset) they would otherwise be dropped
        // from the retained selection text — the same gap the constraint
        // rebuild preserves (`rebuild_selection_from_constraints`). The
        // composition-anchored path (span start == composition offset) has
        // an empty gap.
        let state = self.state.borrow_mut();
        let gap = if span_start > state.record.consumed() {
            state
                .input
                .as_str()
                .get(state.record.consumed()..span_start)
                .unwrap_or("")
        } else {
            ""
        };
        if target.nbest_row.is_some() {
            // An n-best row is a whole-composition hypothesis: its text
            // already covers the full input and its span (consumed_bytes)
            // is the whole composition, so a re-anchored selection must not
            // prepend the typed-but-unselected gap — that would duplicate
            // the raw prefix in the committed text (upstream commits the
            // row's sentence text, pinyin_choose_candidate's NBEST branch
            // returning matrix.size() - 1). The composition-anchored path
            // has an empty gap either way.
            let parts = target
                .nbest_row
                .and_then(|rank| state.sentence.rows.get(usize::from(rank)))
                .map_or_else(
                    || vec![SentencePart::Raw(text.clone())],
                    |row| state.sentence.parts(row).collect(),
                );
            state.record.set_selected(&text, parts);
        } else {
            state.record.append_selected(gap, &text, token);
        }
        if let Some(token) = token {
            state.record.push_token(token);
        } else if let Some(rank) = target.nbest_row {
            // A prepended n-best row records its whole token path —
            // upstream's `pinyin_choose_candidate` keeps the chosen
            // `MatchResult` on the instance and `pinyin_train` walks it;
            // the engine's record is the token history
            // (`docs/findings/user-store.md` §2.1). The row is looked up
            // by its own tail rank, never by list position: the NBEST-wins
            // dedup can drop an earlier duplicate row, shifting a
            // surviving row off the position its rank would give it, and a
            // positional lookup then trains the wrong path.
            if let Some(row) = state.sentence.rows.get(usize::from(rank)) {
                // The row replaces everything decoded since the lookup ran
                // — the text side of that replace is the assign above. A
                // normal selection made in between must leave no token in
                // the record either, so restore the snapshot the rows were
                // decoded against before extending with this row's path.
                state
                    .record
                    .reset_history_extend(&state.sentence.history, &row.tokens);
            }
        }
        // The §3 constraint writes (`pinyin_choose_candidate`,
        // `pinyin.cpp:2576-2584`): a token-bearing candidate forces its
        // span; an n-best row constrains only the phrases where it
        // differs from the 1-best (`diff_result`) — a row-0 choose
        // constrains nothing, exactly upstream.
        if let Some(rank) = target.nbest_row {
            let best = state.sentence.rows.first().map(|row| row.spans.as_slice());
            let Some(chosen) = state.sentence.rows.get(usize::from(rank)) else {
                state.record.set_consumed(constraint_end);
                self.refresh()?;
                return Ok(self.selection_outcome());
            };
            if let Some(best) = best {
                // Upstream validates the store at choose time
                // (`pinyin.cpp:2576-2580`); the engine sizes it here so a
                // fresh composition's row choose — whose store was never
                // resized, `add` refusing every span past an empty cell
                // count — cannot silently write nothing.
                state.constraints.resize(state.input.len() + 1);
                state
                    .constraints
                    .diff_result(best, &chosen.spans, state.input.len());
            }
        } else if let Some(token) = token {
            state.constraints.resize(state.input.len() + 1);
            state
                .constraints
                .add(constraint_start, constraint_end, token, text);
        }
        state.record.set_consumed(constraint_end);
        self.refresh()?;

        Ok(self.selection_outcome())
    }

    /// `pinyin_choose_candidate`'s normal branch for a row with no token and
    /// `m_begin = m_end = 0`, which every predicted row carries
    /// (`pinyin.cpp:114`): validate the store, `add_constraint(0, 0,
    /// null_token)`, validate again (`:2577-2590`). The write is a
    /// zero-length null-token cell at 0, which neither validation removes.
    /// While it stays, no sentence decode passes position 0 (the n-best
    /// walk's `ONESTEP` branch searches `[0, 0)`, which is no span, as the
    /// pin's `search_matrix` answers `SEARCH_CONTINUED` there,
    /// `phonetic_lookup.h:776-790`). A reset or a shrink of the store to
    /// zero clears the cell, and a later non-empty forcing that starts at 0
    /// overwrites it: `add_constraint` finds nothing to clear in the empty
    /// run and writes its `ONESTEP` over the cell
    /// (`phonetic_lookup.cpp:61-86`). The selection record, which the pin
    /// does not keep, is left alone.
    ///
    /// # Errors
    ///
    /// [`EngineError`] from either validation's spelling probe.
    pub fn choose_null_step(&mut self) -> Result<(), EngineError> {
        self.validate_selection_constraints()?;
        self.constraints.add_null_step();
        self.validate_selection_constraints()
    }

    /// Revalidate existing forcings on a refused choose, preserving the pin's
    /// resize and spelling checks without attempting sentence decoding.
    fn validate_selection_constraints(&mut self) -> Result<(), EngineError> {
        self.ensure_matrix()?;
        let size = self.selection_constraint_size()?;
        let mut store = core::mem::take(&mut self.constraints);
        let result = (|| {
            let retained = self.retained_parse()?;
            let spelling = crate::nbest::ConstraintSpelling::new(&retained.matrix);
            store.validate(size, |start, end, token, text_len| {
                spelling.spells(&self.dictionary, start, end, token, text_len)
            })
        })();
        self.constraints = store;
        // Only a run can restore a record that lost its tokens. A null step
        // alone leaves the store active with none, and a rebuild from no
        // runs would clear a token-free selection.
        if result? || (self.constraints.has_runs() && self.record.history().is_empty()) {
            self.rebuild_selection_from_constraints();
        }
        Ok(())
    }

    /// `fill_matrix` clears the matrix and returns early for zero keys;
    /// otherwise its size is parsed_len + 1, not the raw input length
    /// (074a2219 storage/phonetic_key_matrix.cpp:30-48). Read the retained
    /// key path and matrix rather than keeping another parse-length field.
    fn selection_constraint_size(&self) -> Result<usize, EngineError> {
        if self.selected_path()?.edges.is_empty() {
            Ok(0)
        } else {
            Ok(self.retained_parse()?.matrix.bound + 1)
        }
    }

    /// A choose whose span starts before the composition offset (register
    /// row 37, amended 2026-09-27): the pin's `add_constraint(m_begin, m_end,
    /// token)` (`pinyin.cpp:2582-2584`, `phonetic_lookup.cpp:61-86`) —
    /// every forcing the span overlaps is cleared, the span is forced, and
    /// forcings outside it survive — after which the selection record is
    /// re-derived from the store
    /// ([`Session::rebuild_selection_from_constraints`]), so the
    /// composition offset moves back to the end of the forced prefix. A
    /// row with no token has nothing to force; it keeps the old refusal
    /// ([`EngineError::SelectionAnchorBeforeComposition`]).
    fn select_behind(
        &mut self,
        start: usize,
        end: usize,
        token: Option<PhraseToken>,
        text: PhraseText,
    ) -> Result<Selection, EngineError> {
        let Some(token) = token else {
            return Err(EngineError::SelectionAnchorBeforeComposition {
                anchor: start,
                composition: self.record.consumed(),
            });
        };
        let state = self.state.borrow_mut();
        state.constraints.resize(state.input.len() + 1);
        state.constraints.add(start, end, token, text);
        self.rebuild_selection_from_constraints();
        self.refresh()?;
        Ok(self.selection_outcome())
    }

    /// Clears the constraint run at `offset` — `pinyin_clear_constraint`
    /// (`pinyin.cpp:2641-2647`). The offset indexes the store's
    /// coordinate space (raw-buffer byte positions, #141's law); a hit
    /// anywhere inside a forced run un-forces the whole run. The
    /// selection record follows the surviving forcings, so the cleared
    /// phrase's text and token leave the record.
    ///
    /// Returns `false` for a free cell or an out-of-range offset —
    /// upstream's own defined return, never an abort.
    #[must_use]
    pub fn clear_constraint(&mut self, offset: usize) -> bool {
        if !self.constraints.clear_by_offset(offset) {
            return false;
        }
        self.rebuild_selection_from_constraints();
        true
    }

    /// Rebuilds the selection record (`selected`, `consumed`, `history`)
    /// from the surviving forcings. Upstream keeps no such record — the
    /// frontend tracks its own cursor — so the store is the engine's
    /// single source once forcings exist.
    pub(super) fn rebuild_selection_from_constraints(&mut self) {
        // Gaps between forced runs are free spans (diff_result forces only
        // the differing phrases); their text is the current buffer's bytes,
        // so the rebuilt record never drops raw input the forcings skip
        // over — the selected text would otherwise lose exactly that gap. The
        // record owns the rebuild (and the commit-branch flag it clears):
        // this seam only hands it the surviving runs and the buffer.
        let runs = self.constraints.runs();
        let state = self.state.borrow_mut();
        state
            .record
            .rebuild_from_constraints(state.input.as_str(), &runs);
    }
}

/// 074a2219 `get_column_size(offset) > 0` (`storage/phonetic_key_matrix.cpp`):
/// a column is occupied by either a real key or a zero key. `from_scan`
/// records real keys in `matrix` and internal separator zero keys (and the
/// reserved end slot) in `zeros`.
/// A training walk's plan ([`Session::plan_spans`]): the retained matrix it
/// indexes, borrowed from the composition state the plan itself borrows, and
/// the end of each span it observes (`None` for one it only passes).
type SpanPlan<'a> = (&'a super::matrix::ParsedMatrix, Vec<Option<usize>>);

pub(crate) fn column_is_empty(matrix: &[Vec<ScanKey>], zeros: &[bool], offset: usize) -> bool {
    matrix.get(offset).is_none_or(Vec::is_empty) && !zeros.get(offset).copied().unwrap_or(false)
}

/// 074a2219 storage/phonetic_key_matrix.cpp:603-670: lazy depth-first
/// traversal keeps only one path, like the pin's cached_keys. Matrix order
/// and multiplicity survive; separator zero keys are folded into scan edges.
/// `compute_pronunciation_possibility_recur` (`:534-585`), which revalidates
/// a forcing, walks the columns the same way, so
/// [`crate::nbest::ConstraintSpelling`] reads its paths from here too.
pub(crate) fn training_readings<'a>(
    matrix: &'a [Vec<ScanKey>],
    input: &'a [u8],
    start: usize,
    end: usize,
    length: usize,
) -> TrainingReadings<'a> {
    TrainingReadings {
        matrix,
        input,
        end,
        length,
        frames: vec![(start, 0)],
        keys: Vec::new(),
        yielded: false,
    }
}

pub(crate) struct TrainingReadings<'a> {
    matrix: &'a [Vec<ScanKey>],
    input: &'a [u8],
    end: usize,
    length: usize,
    frames: Vec<(usize, usize)>,
    keys: Vec<oxpinyin_core::ChewingKey>,
    yielded: bool,
}

impl TrainingReadings<'_> {
    fn backtrack(&mut self) {
        self.frames.pop();
        if !self.frames.is_empty() {
            self.keys.pop();
        }
    }
}

impl Iterator for TrainingReadings<'_> {
    type Item = Vec<oxpinyin_core::ChewingKey>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.yielded {
            self.yielded = false;
            self.backtrack();
        }
        while let Some(&(position, index)) = self.frames.last() {
            // :607-621: exact span and phrase length only.
            if position == self.end && self.keys.len() == self.length {
                self.yielded = true;
                return Some(self.keys.clone());
            }
            // 074a2219 phonetic_key_matrix.cpp:635-642: a zero key
            // advances without adding a syllable, including a trailing
            // separator after the last key. Other separator hops are
            // already folded into the scan edge that follows them.
            if position < self.end
                && self.input.get(position) == Some(&b'\'')
                && self.matrix.get(position).is_some_and(Vec::is_empty)
            {
                if let Some(frame) = self.frames.last_mut() {
                    frame.0 += 1;
                }
                continue;
            }
            if position >= self.end || self.keys.len() >= self.length {
                self.backtrack();
                continue;
            }
            let Some(edge) = self
                .matrix
                .get(position)
                .and_then(|column| column.get(index))
            else {
                self.backtrack();
                continue;
            };
            if let Some(frame) = self.frames.last_mut() {
                frame.1 += 1;
            }
            // :629-651: visit each matrix key in order, without deduping.
            if edge.to <= position {
                continue;
            }
            self.keys.push(edge.key.chewing_key().with_tone(edge.tone));
            self.frames.push((edge.to, 0));
        }
        None
    }
}

#[cfg(test)]
mod training_span_tests {
    use super::*;

    #[test]
    fn readings_preserve_span_tones_order_and_multiplicity() {
        let Some(key) = SyllableKey::from_text("jin") else {
            panic!("inventory");
        };
        let edge = |from, to, tone| ScanKey {
            key,
            from,
            to,
            syllable_start: from,
            crosses_separator: false,
            tone,
        };
        let matrix = vec![
            vec![edge(0, 1, 2), edge(0, 1, 3), edge(0, 2, 4)],
            vec![edge(1, 2, 1)],
            vec![],
        ];
        let readings: Vec<_> = training_readings(&matrix, b"xx", 0, 1, 1).collect();
        assert_eq!(
            readings.iter().map(|r| r[0].tone).collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(training_readings(&matrix, b"xx", 0, 2, 2).count(), 2);
        assert_eq!(training_readings(&matrix, b"xx", 0, 2, 1).count(), 1);
        assert_eq!(training_readings(&matrix, b"xx", 2, 1, 1).count(), 0);
        let trailing = vec![vec![edge(0, 1, 2)], vec![], vec![], vec![]];
        assert_eq!(training_readings(&trailing, b"x''", 0, 3, 1).count(), 1);
        assert_eq!(training_readings(&trailing, b"xxx", 0, 3, 1).count(), 0);
    }
}
