//! Selection and constraints: choosing a candidate from the cached list or a re-anchored window, the §3 constraint writes, the selection record, training, commit.
//!
//! One of the `impl Session` slices `session/mod.rs` declares; every
//! method here was moved verbatim from the single 5,800-line
//! `session.rs` (2026-09-08) and keeps its doc comment and upstream
//! citations. Shared state, constants and free functions stay in the
//! parent module.

use super::*;
use crate::constraint::PhraseSpan;

impl<D, L> Session<D, L>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Chooses the candidate at `index`.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::CandidateIndexOutOfRange`] for an index the
    /// current list does not hold — including a stale index left over from an
    /// earlier list — and leaves the session usable.
    pub fn select(&mut self, index: usize) -> Result<Selection, EngineError> {
        // Clone the candidate out of the cached list first: `select_inner`
        // borrows `self` mutably, so it cannot also borrow `self.candidates`.
        let candidate = self.lookup.candidates.get(index).cloned().ok_or(
            EngineError::CandidateIndexOutOfRange {
                index,
                len: self.lookup.candidates.len(),
            },
        )?;
        self.select_inner(self.record.consumed(), &candidate, None)
    }

    /// Chooses the candidate at `index`, recording `promoted_token` in the
    /// sentence history in place of the candidate's own token.
    ///
    /// The addon-promotion path (`pinyin.cpp:2532-2561`,
    /// `docs/findings/addon-choose-promotion.md`): a chosen `ADDON_CANDIDATE`
    /// becomes a `NORMAL_CANDIDATE` at a freshly allocated default-facade
    /// nibble-5 token, and it is that promoted token the constraint — and a
    /// later `pinyin_train` — records, not the addon-facade token.
    ///
    /// # Errors
    ///
    /// Same as [`Session::select`].
    pub fn select_promoted(
        &mut self,
        index: usize,
        promoted_token: PhraseToken,
    ) -> Result<Selection, EngineError> {
        let candidate = self.lookup.candidates.get(index).cloned().ok_or(
            EngineError::CandidateIndexOutOfRange {
                index,
                len: self.lookup.candidates.len(),
            },
        )?;
        self.select_inner(self.record.consumed(), &candidate, Some(promoted_token))
    }

    /// Chooses the candidate at `index` from an explicit candidate window
    /// (a re-anchored `candidates_at` list), not the session's cached list.
    ///
    /// `pinyin_guess_candidates` at an offset other than the composition's
    /// own rebuilds the window the caller sees at that offset; a subsequent
    /// `pinyin_choose_candidate` must resolve its index against that SAME
    /// window, or it would select a different row from the composition-
    /// anchored cached list whenever the two differ. The selection record,
    /// constraint span, and consumed advance are otherwise identical to
    /// [`Session::select`] — they come from the candidate and the session
    /// state, not from which list holds the candidate.
    ///
    /// # Errors
    ///
    /// Same as [`Session::select`]: [`EngineError::CandidateIndexOutOfRange`]
    /// for an index the given window does not hold, and
    /// [`EngineError::LookupOffsetOutOfRange`] for an anchor past the raw
    /// input's end.
    pub fn select_anchored(
        &mut self,
        index: usize,
        window: &CandidateList,
        anchor: usize,
    ) -> Result<Selection, EngineError> {
        if anchor > self.input.len() {
            return Err(EngineError::LookupOffsetOutOfRange {
                offset: anchor,
                len: self.input.len(),
            });
        }
        let candidate = window
            .get(index)
            .ok_or(EngineError::CandidateIndexOutOfRange {
                index,
                len: window.len(),
            })?;
        self.select_inner(anchor, candidate, None)
    }

    /// [`Session::select_anchored`] with an addon-promotion token override,
    /// mirroring [`Session::select_promoted`].
    ///
    /// # Errors
    ///
    /// Same as [`Session::select_anchored`].
    pub fn select_anchored_promoted(
        &mut self,
        index: usize,
        window: &CandidateList,
        anchor: usize,
        promoted_token: PhraseToken,
    ) -> Result<Selection, EngineError> {
        if anchor > self.input.len() {
            return Err(EngineError::LookupOffsetOutOfRange {
                offset: anchor,
                len: self.input.len(),
            });
        }
        let candidate = window
            .get(index)
            .ok_or(EngineError::CandidateIndexOutOfRange {
                index,
                len: window.len(),
            })?;
        self.select_inner(anchor, candidate, Some(promoted_token))
    }

    /// Selects the candidate at `index`, which must not alias `self` —
    /// either an owned clone of a cached candidate (the composition case)
    /// or a reference into an external window.
    pub(super) fn select_inner(
        &mut self,
        anchor: usize,
        candidate: &Candidate,
        token_override: Option<PhraseToken>,
    ) -> Result<Selection, EngineError> {
        let text = candidate.text().to_owned();
        let advance = candidate.consumed_bytes();
        let token = token_override.or_else(|| candidate.token());
        // The chosen span in the store's coordinates — upstream's
        // `add_constraint(m_begin, m_end, token)` (`pinyin.cpp:2582`,
        // `zhuyin.cpp:1652,1659`). For the composition-anchored cached list,
        // `anchor` is the composition offset and the candidate's
        // `consumed_bytes` is measured from it; for an after-cursor window,
        // `anchor` is that window's caller offset and `consumed_bytes` is
        // measured from IT (the pin's `m_begin = start`, `pinyin.cpp:2227`)
        // — both carry `span_start` 0, so the span starts at `anchor`. A
        // before-cursor window is END-anchored at 0 with each row's own
        // absolute `m_begin` in `span_start` and its absolute end in
        // `consumed_bytes`: the span starts at `anchor + span_start`, not at
        // the anchor, so choosing the second key's span constrains that key
        // alone and leaves the leading key to the decode.
        let span_start = anchor.saturating_add(candidate.span_start());
        let constraint_start = span_start;
        let constraint_end = self.next_boundary(anchor.saturating_add(advance));
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
        if span_start < self.record.consumed() && candidate.nbest_row().is_none() {
            return self.select_behind(constraint_start, constraint_end, token, &text);
        }
        // The raw bytes between the composition offset and the span start
        // were typed without being selected. For a re-anchored selection
        // (span start > composition offset) they would otherwise be dropped
        // from the committed/preedit text — the same gap the constraint
        // rebuild preserves (`rebuild_selection_from_constraints`). The
        // composition-anchored path (span start == composition offset) has
        // an empty gap.
        let gap = if span_start > self.record.consumed() {
            self.input
                .as_str()
                .get(self.record.consumed()..span_start)
                .unwrap_or("")
        } else {
            ""
        };
        if candidate.nbest_row().is_some() {
            // An n-best row is a whole-composition hypothesis: its text
            // already covers the full input and its span (consumed_bytes)
            // is the whole composition, so a re-anchored selection must not
            // prepend the typed-but-unselected gap — that would duplicate
            // the raw prefix in the committed text (upstream commits the
            // row's sentence text, pinyin_choose_candidate's NBEST branch
            // returning matrix.size() - 1). The composition-anchored path
            // has an empty gap either way.
            self.record.set_selected(&text);
        } else {
            self.record.append_selected(gap, &text);
        }
        if let Some(token) = token {
            self.record.push_token(token);
        } else if let Some(rank) = candidate.nbest_row() {
            // A prepended n-best row records its whole token path —
            // upstream's `pinyin_choose_candidate` keeps the chosen
            // `MatchResult` on the instance and `pinyin_train` walks it;
            // the engine's record is the token history
            // (`docs/findings/user-store.md` §2.1). The row is looked up
            // by its own tail rank, never by list position: the NBEST-wins
            // dedup can drop an earlier duplicate row, shifting a
            // surviving row off the position its rank would give it, and a
            // positional lookup then trains the wrong path. A fallback
            // sentence candidate carries no rank and no tokens; it records
            // nothing, exactly as before.
            if let Some(row) = self.sentence.rows.get(usize::from(rank)) {
                // The row replaces everything decoded since the lookup ran
                // — the text side of that replace is the assign above. A
                // normal selection made in between must leave no token in
                // the record either, so restore the snapshot the rows were
                // decoded against before extending with this row's path.
                self.record
                    .reset_history_extend(&self.sentence.history, &row.tokens);
            }
        }
        // The §3 constraint writes (`pinyin_choose_candidate`,
        // `pinyin.cpp:2576-2584`): a token-bearing candidate forces its
        // span; an n-best row constrains only the phrases where it
        // differs from the 1-best (`diff_result`) — a row-0 choose
        // constrains nothing, exactly upstream.
        if let Some(rank) = candidate.nbest_row() {
            let best = self.sentence.rows.first().map(|row| row.spans.as_slice());
            let Some(chosen) = self.sentence.rows.get(usize::from(rank)) else {
                self.record.set_consumed(constraint_end);
                self.refresh()?;
                return Ok(self.selection_outcome());
            };
            if let Some(best) = best {
                // Upstream validates the store at choose time
                // (`pinyin.cpp:2576-2580`); the engine sizes it here so a
                // fresh composition's row choose — whose store was never
                // resized, `add` refusing every span past an empty cell
                // count — cannot silently write nothing.
                self.constraints.resize(self.input.len() + 1);
                self.constraints
                    .diff_result(best, &chosen.spans, self.input.len());
            }
        } else if let Some(token) = token {
            self.constraints.resize(self.input.len() + 1);
            self.constraints.add(
                constraint_start,
                constraint_end,
                token,
                compact_str::CompactString::from(text.as_str()),
            );
        }
        self.record.set_consumed(constraint_end);
        self.record
            .set_committed(constraint_end >= self.input.len());
        self.refresh()?;

        Ok(self.selection_outcome())
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
        text: &str,
    ) -> Result<Selection, EngineError> {
        let Some(token) = token else {
            return Err(EngineError::SelectionAnchorBeforeComposition {
                anchor: start,
                composition: self.record.consumed(),
            });
        };
        self.constraints.resize(self.input.len() + 1);
        self.constraints
            .add(start, end, token, compact_str::CompactString::from(text));
        self.rebuild_selection_from_constraints();
        self.refresh()?;
        Ok(self.selection_outcome())
    }

    /// The common tail of [`Session::select_inner`].
    pub(super) const fn selection_outcome(&self) -> Selection {
        if self.record.consumed() >= self.input.len() {
            Selection::Completed
        } else {
            Selection::Continued
        }
    }

    /// Trains the recorded sentence through the user-model seam.
    ///
    /// The §3 constraint-aware walk (`train_result3`,
    /// `phonetic_lookup.h:841-935`): the last sentence lookup's 1-best
    /// result is walked phrase by phrase against the constraint store — a
    /// phrase trains when it is user-forced (`OneStep`) or when
    /// `train_next` is set (the first decoded phrase after each forced
    /// run, where propagation stops), and the bigram predecessor advances
    /// over **every** phrase, trained or not. A user who forces 你 for
    /// "ni" and commits the decoded 好 therefore trains 你→好, not just
    /// `sentence_start→你` (the L3 surface, `docs/findings/live-typing.md`).
    ///
    /// Without a decoded result — the fixture fallback models, or no
    /// lookup since the last reset — the selection history stands in: one
    /// [`UserModel::observe`] per pinned token with the preceding tokens
    /// as context (`docs/findings/user-store.md` §2.1). The C ABI's
    /// `pinyin_train` is this call; per-candidate selection only records
    /// the constraint, and the bigram update is deferred to here (§2.2).
    /// Learning-off callers omit it entirely.
    ///
    /// Re-calling without new selections re-observes the same sentence,
    /// which is the upstream behaviour (a second `pinyin_train` doubles
    /// the counts — there is no guard upstream either).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::UserModel`] when the user model rejects an
    /// observation. Tokens observed before the failure stay observed: the
    /// sentence is trained prefix-wise, like the upstream loop.
    pub fn train<U>(&self, user: &mut U) -> Result<(), EngineError>
    where
        U: UserModel<Token = PhraseToken>,
        U::Error: Display,
    {
        if !self.sentence.last_result.is_empty() {
            return self.train_spans(&self.sentence.last_result, user);
        }
        let history = self.record.history();
        for (index, token) in history.iter().enumerate() {
            user.observe(&history[..index], token)
                .map_err(|error| EngineError::UserModel(error.to_string()))?;
        }
        Ok(())
    }

    /// Train decoded n-best row `index` using the current constraints.
    ///
    /// Returns `Ok(false)` when no decoded results exist, as the pinned
    /// `pinyin_train`/`zhuyin_train` gate requires. Selection history alone
    /// does not satisfy this gate.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::CandidateIndexOutOfRange`] for an index beyond
    /// the nonempty decoded results, before observing any user data; returns
    /// [`EngineError::UserModel`] when an observation fails.
    pub fn train_nbest<U>(&self, index: u8, user: &mut U) -> Result<bool, EngineError>
    where
        U: UserModel<Token = PhraseToken>,
        U::Error: Display,
    {
        if self.sentence.rows.is_empty() {
            return Ok(false);
        }
        let row = self.sentence.rows.get(usize::from(index)).ok_or(
            EngineError::CandidateIndexOutOfRange {
                index: usize::from(index),
                len: self.sentence.rows.len(),
            },
        )?;
        self.train_spans(&row.spans, user)?;
        Ok(true)
    }

    fn train_spans<U>(&self, spans: &[PhraseSpan], user: &mut U) -> Result<(), EngineError>
    where
        U: UserModel<Token = PhraseToken>,
        U::Error: Display,
    {
        // 074a2219 phonetic_lookup.h:854-866,932: only OneStep or
        // train_next observes a phrase; every phrase advances its predecessor.
        // A no-op needs no matrix rebuild: train_next starts false
        // and only a OneStep can start training (pin :854-872).
        if !spans
            .iter()
            .any(|span| self.constraints.is_one_step_at(span.start))
        {
            return Ok(());
        }
        let mut context: Vec<PhraseToken> = Vec::with_capacity(spans.len());
        let mut train_next = false;
        let graph = self.build_graph_at(0, self.input.as_bytes())?;
        let matrix =
            build_scan_matrix(&graph, self.settings.options, self.input.exact().is_empty());
        for (index, span) in spans.iter().enumerate() {
            let forced = self.constraints.is_one_step_at(span.start);
            if train_next || forced {
                train_next = forced;
                // 074a2219 phonetic_lookup.h:911-920 scans to the next
                // non-null result token, not the forced constraint end.
                let next = spans
                    .get(index + 1)
                    .map_or(matrix.len().saturating_sub(1), |next| next.start);
                // :921 clamps the last span to constraints->length()-1;
                // :923-927 trains every matching path of this span.
                let end = next.min(matrix.len().saturating_sub(1));
                let mut readings = training_readings(
                    &matrix,
                    self.input.as_bytes(),
                    span.start,
                    end,
                    span.text.chars().count(),
                );
                user.observe_with_keys(&context, &span.token, &mut readings)
                    .map_err(|error| EngineError::UserModel(error.to_string()))?;
            }
            context.push(span.token);
        }
        Ok(())
    }

    /// Clears the constraint run at `offset` — `pinyin_clear_constraint`
    /// (`pinyin.cpp:2641-2647`). The offset indexes the store's
    /// coordinate space (raw-buffer byte positions, #141's law); a hit
    /// anywhere inside a forced run un-forces the whole run. The
    /// selection record follows the surviving forcings, so the cleared
    /// phrase's text leaves the preedit and its token leaves the record.
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
        // over — the preedit would otherwise lose exactly that gap. The
        // record owns the rebuild (and the commit-branch flag it clears):
        // this seam only hands it the surviving runs and the buffer.
        let runs = self.constraints.runs();
        self.record
            .rebuild_from_constraints(self.input.as_str(), &runs);
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

    /// The current composition's syllable keys, in the engine's selected
    /// parse order.
    ///
    /// This is the fewest-keys segmentation the scan matrix is built from
    /// ([`SegmentGraph::fewest_keys`], `docs/findings/candidate-construction.md`
    /// §8.1) over the whole raw buffer — the standing-in for libpinyin's saved
    /// keys, which `pinyin_remember_user_input` walks to store a phrase with
    /// its pinyin (`docs/findings/user-store.md` §3.1).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Graph`] when the raw buffer cannot be built
    /// into a segment graph (an over-long input; the buffer is capped by
    /// [`MAX_INPUT_BYTES`]).
    pub fn composition_keys(&self) -> Result<Vec<SyllableKey>, EngineError> {
        let graph = self.build_graph_at(0, self.input.as_bytes())?;
        Ok(graph
            .fewest_keys(self.settings.incomplete())
            .into_iter()
            .map(|edge| edge.key())
            .collect())
    }

    /// Finishes the composition and returns its text.
    ///
    /// Never fails on an empty composition: the text is then empty too.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails while the session resets.
    pub fn commit(&mut self) -> Result<String, EngineError> {
        let mut text = self.record.take_selected();
        text.push_str(&self.input.as_str()[self.record.consumed()..]);
        self.reset();
        Ok(text)
    }
}

/// 074a2219 storage/phonetic_key_matrix.cpp:603-670: lazy depth-first
/// traversal keeps only one path, like the pin's cached_keys. Matrix order
/// and multiplicity survive; separator zero keys are folded into scan edges.
fn training_readings<'a>(
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

struct TrainingReadings<'a> {
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
            let Some(key) = oxpinyin_core::ChewingKey::from_pinyin(edge.key.text()) else {
                continue;
            };
            self.keys.push(key.with_tone(edge.tone));
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
