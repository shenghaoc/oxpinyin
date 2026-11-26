//! Selection and constraints: choosing a candidate from the cached list or a re-anchored window, the §3 constraint writes, the selection record and training.

use super::*;
use crate::engine::constraint::PhraseSpan;
use crate::engine::error::{PinAbort, TrainingStop};
use crate::vocab::MAX_PHRASE_LENGTH;

/// The anchor of a **before-cursor** window — the buffer start, not the
/// lookup offset — that a caller passes to [`SelectionTarget::of_candidate`]
/// for a row of that window.
///
/// The two window builders measure a candidate's `consumed_bytes` in
/// different coordinates, and [`SelectionTarget::of_candidate`] reads the
/// chosen span as `[anchor, anchor + consumed_bytes)`:
///
/// - **After-cursor** (`Session::candidates_at(offset)`) rebases its graph
///   onto `raw[offset..]`, so `consumed_bytes` is a LENGTH measured from
///   the lookup offset. Its anchor is that offset.
/// - **Before-cursor** (`Session::candidates_ending_at(offset)`) reads
///   the retained matrix, whose coordinates are absolute from
///   the buffer start, so every row's `consumed_bytes` is the span's
///   ABSOLUTE END — the lookup offset itself, shared by every row in the
///   window, whatever byte each span starts on. The anchor that reproduces
///   that end (and so the consumed advance) is therefore the coordinate
///   those ends are measured from: 0.
///
/// Reusing the after-cursor anchor here would read the span as
/// `[offset, 2 * offset)` and walk the composition off the end of the
/// buffer; resolving the row's index against the composition-anchored
/// cached list instead reads a different list — the caller would commit a
/// row it never displayed.
pub const BEFORE_CURSOR_ANCHOR: usize = 0;

/// What a choose reads off the row it selects: the pin's `m_begin`,
/// `m_end` and `m_token`, and for an n-best row `m_nbest_index`
/// (`lookup_candidate_t`, 074a2219 `pinyin.cpp:97-105`; read at
/// `:2515-2520` and `:2577-2590`, `zhuyin.cpp:1646-1666`). Both spans
/// are absolute, in the store's coordinates.
#[derive(Clone, Copy, Debug)]
pub struct SelectionTarget {
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
}

/// Whether `Session::select_row` checks the chosen item before selecting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowText {
    /// Resolve the token's item; an unreadable item takes only the constraint write.
    Resolve,
    /// Skip item resolution: an addon row whose item the receiving context lacks
    /// (`pinyin.cpp:2534-2563`).
    Empty,
}

impl SelectionTarget {
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
        candidate: &Candidate,
        anchor: usize,
        token_override: Option<PhraseToken>,
    ) -> Self {
        Self {
            span_start: anchor.saturating_add(candidate.span_start()),
            span_end: anchor.saturating_add(candidate.consumed_bytes()),
            token: token_override.or_else(|| candidate.token()),
            nbest_row: candidate.nbest_row(),
        }
    }
}

impl SessionState {
    /// `pinyin_train(index)` (074a2219 `pinyin.cpp:2670-2690`) and the
    /// `train_result3` loop it runs (`phonetic_lookup.h:844-932`): `false`
    /// for no results, then each step is checked and trained in turn. A
    /// step checks the pin's asserts first (the forced token, then the
    /// span), then the user model loads, trains and stores the step's gram
    /// and updates the item. The walk reads composition state only, so the
    /// dictionary's borrow of the store has ended when it runs.
    ///
    /// # Errors
    ///
    /// [`TrainingStop::PinAbort`] at the pin's own aborts, which production
    /// terminates on, and [`TrainingStop::User`] where the user model
    /// refuses a step. The steps before either stay recorded.
    pub(crate) fn train<U: UserModel>(
        &self,
        index: u8,
        user: &mut U,
    ) -> Result<bool, TrainingStop<U::Error>> {
        let Some(row) = self.training_row(index)? else {
            return Ok(false);
        };
        if row.spans.is_empty() {
            let history = self.record.history();
            for (index, token) in history.iter().enumerate() {
                user.observe(&history[..index], token)
                    .map_err(TrainingStop::User)?;
            }
            return Ok(true);
        }
        // Only a OneStep span, or the span after one, trains
        // (`phonetic_lookup.h:854-872`): with none, the walk trains nothing.
        if !row
            .spans
            .iter()
            .any(|span| self.constraints.is_one_step_at(span.start))
        {
            return Ok(true);
        }
        let matrix = &self.input.parsed.retained.matrix;
        // Made at the first step that trains.
        let mut stacks: Option<ReadingStacks> = None;
        for step in self.training_steps(&row.spans) {
            let step = step?;
            let mut readings = training_readings(
                matrix,
                self.input.as_bytes(),
                step.span.start,
                step.end,
                step.span.phrase_length,
                stacks.get_or_insert_with(ReadingStacks::default),
            );
            user.observe_with_keys(step.last, &step.span.token, &mut readings)
                .map_err(TrainingStop::User)?;
        }
        Ok(true)
    }

    /// Whether `pinyin_train(index)` meets one of the pin's own aborts
    /// ([`PinAbort`]): the checks [`Self::train`] makes, in its order,
    /// without the user model. The prediction does not model the store's
    /// refusals: where a partial-tail refusal comes before a stale span, it
    /// predicts an abort that [`Self::train`] does not reach. That cannot
    /// occur where the probe is used: a partial tail needs a planted user
    /// bigram, the store's own aborts need one or a failed container, and
    /// no C API call makes either. Reads only.
    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn training_aborts(&self, index: u8) -> bool {
        match self.training_row(index) {
            Err(PinAbort) => true,
            Ok(None) => false,
            Ok(Some(row)) => self.training_steps(&row.spans).any(|step| step.is_err()),
        }
    }

    /// The decoded row [`Self::train`] trains: `None` for no results (the
    /// pin's quiet `false`), the pin's index assert past nonempty results
    /// (`pinyin.cpp:2684` at 074a2219; the Zhuyin caller always trains row
    /// 0, `zhuyin.cpp:1696-1715`).
    fn training_row(&self, index: u8) -> Result<Option<&crate::engine::nbest::NbestRow>, PinAbort> {
        if self.sentence.rows.is_empty() {
            return Ok(None);
        }
        self.sentence
            .rows
            .get(usize::from(index))
            .map(Some)
            .ok_or(PinAbort)
    }

    /// The steps `train_result3` trains over `spans`, each after the pin's
    /// asserts for it.
    fn training_steps<'a>(&'a self, spans: &'a [PhraseSpan]) -> TrainingSteps<'a> {
        // 074a2219 phonetic_lookup.h:921 tails the last span at
        // `constraints->length() - 1`, NOT the CURRENT matrix's last
        // column: the store survives a parse (`pinyin.cpp:1497-1525`,
        // never validating), so a re-parse that shortened the input
        // leaves it longer than the matrix the walk now indexes, and the
        // span asserts fire.
        let constraint_len = self.constraints.cell_count();
        TrainingSteps {
            spans,
            next: 0,
            constraints: &self.constraints,
            matrix: &self.input.parsed.retained.matrix,
            constraint_len,
            tail: constraint_len.saturating_sub(1),
            train_next: false,
            last: None,
        }
    }
}

/// One step `train_result3` trains: the span, its predecessor (`None` at
/// the sentence start) and the end its reading walk stops at.
struct TrainingStep<'a> {
    last: Option<PhraseToken>,
    span: &'a PhraseSpan,
    end: usize,
}

/// `train_result3`'s walk over a decoded row's spans (074a2219
/// `phonetic_lookup.h:854-932`): only a OneStep span or the span after one
/// trains, every span advances the predecessor, and each trained span is
/// checked first. The walk ends after an abort.
struct TrainingSteps<'a> {
    spans: &'a [PhraseSpan],
    next: usize,
    constraints: &'a crate::engine::constraint::ConstraintStore,
    /// The retained topology re-applies the physical separators
    /// (`ParsedMatrix::from_scan`, the matrix the lookup indexes): the
    /// pin's `fill_matrix` leaves a leading separator gap empty
    /// (`phonetic_key_matrix.cpp:52-78`), which `:663` refuses.
    matrix: &'a super::matrix::ParsedMatrix,
    constraint_len: usize,
    tail: usize,
    train_next: bool,
    last: Option<PhraseToken>,
}

impl<'a> Iterator for TrainingSteps<'a> {
    type Item = Result<TrainingStep<'a>, PinAbort>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(span) = self.spans.get(self.next) {
            let index = self.next;
            self.next += 1;
            // 074a2219 phonetic_lookup.h:858 walks the result positions
            // below `constraints->length()`. A choose after a shortening
            // re-parse shrinks the store (`pinyin.cpp:2579`) under a row
            // decoded for the longer input: its later spans are never
            // reached.
            if span.start >= self.constraint_len {
                break;
            }
            let last = self.last.replace(span.token);
            let forced = self.constraints.is_one_step_at(span.start);
            if !(self.train_next || forced) {
                continue;
            }
            // `assert(token == constraint->m_token)` (`phonetic_lookup.h:868`).
            if forced
                && self
                    .constraints
                    .one_step_token_at(span.start)
                    .is_some_and(|forced_token| forced_token != span.token)
            {
                return Some(self.abort());
            }
            self.train_next = forced;
            // :911-920 scans to the next non-null result token, not the
            // forced constraint end; :921 clamps the last span to
            // `constraints->length() - 1`.
            let next = self
                .spans
                .get(index + 1)
                .map_or(self.tail, |next| next.start);
            let end = next.min(self.tail);
            // `increase_pronunciation_possibility`'s asserts
            // (`storage/phonetic_key_matrix.cpp:661`, `:663`, `:664`):
            // `end < matrix->size()`, then a filled start column (an
            // internal separator holds a zero key and is accepted), then a
            // filled end column. The pin reaches them after the step's
            // gram is stored; an abort leaves nothing it wrote observable,
            // so they are checked before the step's writes.
            let zeros = &self.matrix.zeros;
            if end >= self.matrix.column_count()
                || column_is_empty(self.matrix, zeros, span.start)
                || column_is_empty(self.matrix, zeros, end)
            {
                return Some(self.abort());
            }
            return Some(Ok(TrainingStep { last, span, end }));
        }
        None
    }
}

impl TrainingSteps<'_> {
    /// Ends the walk at an abort.
    fn abort<T>(&mut self) -> Result<T, PinAbort> {
        self.next = self.spans.len();
        Err(PinAbort)
    }
}

impl<D, L, S: std::borrow::Borrow<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    /// The common tail of [`Session::select_target`].
    pub(super) fn selection_outcome(&self) -> Selection {
        if self.record.consumed() >= self.input.len() {
            Selection::Completed
        } else {
            Selection::Continued
        }
    }

    /// Whether the pin's NBEST choose of row `rank` asserts (074a2219
    /// `pinyin.cpp:2515-2520`): the results do not hold row 0 and row
    /// `rank` (`:2517-2518`), or `diff_result` meets a write that answers
    /// zero (`phonetic_lookup.cpp:203`). The store and the rows are read as
    /// [`Session::select_nbest_row`] reads them, without validation. Reads
    /// only.
    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn nbest_choose_aborts(&self, rank: u8) -> bool {
        let state = self.state.borrow();
        let (Some(best), Some(chosen)) = (
            state.sentence.rows.first(),
            state.sentence.rows.get(usize::from(rank)),
        ) else {
            return true;
        };
        state
            .constraints
            .diff_result_refuses(&best.spans, &chosen.spans, chosen.span)
    }

    /// The sentence recorded so far: the token of every phrase the user
    /// pinned in this composition, in selection order.
    ///
    /// Sentence-level candidates carry no token and are not part of the
    /// record — exactly the phrases a `pinyin_train` call would train
    /// (`docs/findings/user-store.md` §2.1). The C ABI uses the tail of this
    /// slice as the predecessor for predicted-candidate training (§2.3).
    #[cfg(any(test, feature = "test-hooks"))]
    #[must_use]
    pub fn selected_tokens(&self) -> &[PhraseToken] {
        self.record.history()
    }
}

impl<D, L, S: std::borrow::BorrowMut<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    // pinyin_choose_candidate dispatches on m_begin, m_end, m_token and m_nbest_index
    // (pinyin.cpp:2515–2520,2577–2590; zhuyin.cpp:1646–1666). diff_result requires each
    // differing constraint write to succeed (lookup/phonetic_lookup.cpp:189–203).
    /// Selects the token span or n-best sentence rank in a published [`SelectionTarget`]. An
    /// ordinary token span past the current matrix returns [`Selection::Refused`].
    ///
    /// # Errors
    ///
    /// Propagates constraint validation and refresh errors. A tokenless choice behind the
    /// composition yields [`EngineError::Failure`].
    ///
    /// # Aborts
    ///
    /// Selecting an n-best row aborts if a differing phrase in that row cannot be written
    /// into the current constraint store. This can occur when the selected row predates a
    /// change of composition.
    pub fn select_target(&mut self, target: &SelectionTarget) -> Result<Selection, EngineError> {
        let span_end = target.span_end;
        if target.nbest_row.is_none()
            && target.token.is_some()
            && span_end > self.selection_constraint_size()
        {
            // Both choose functions validate before and after add_constraint,
            // even when it returns zero (074a2219 pinyin.cpp:2578-2587;
            // zhuyin.cpp:1646-1666). Do not clip the retained span or add its
            // token/text to the record. The ABI owns the refusal cursor.
            self.validate_selection_constraints()?;
            self.validate_selection_constraints()?;
            return Ok(Selection::Refused);
        }
        // Only NORMAL choose validates before its write; NBEST dispatches
        // directly to diff_result (pinyin.cpp:2515-2520,2577-2587).
        if target.nbest_row.is_none() {
            self.validate_selection_constraints()?;
        }
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
        // cursor to 0). The pin's window is built from `start = offset`
        // (`pinyin.cpp:2229`, `m_begin = start` at
        // `:2246`) and the choose writes `add_constraint(m_begin, m_end,
        // token)` over whatever forcings the span overlaps (`:2578-2590`,
        // `phonetic_lookup.cpp:61-86`) — so the choose forces the span and
        // the record follows the store, moving back to it. An n-best row
        // is a whole-composition hypothesis and takes the row path below.
        if span_start < self.record.consumed() && target.nbest_row.is_none() {
            return self.select_behind(constraint_start, constraint_end, token);
        }
        // The normal choose of a token: `add_constraint(m_begin, m_end,
        // token)`, then `validate_constraint` again (074a2219
        // `pinyin.cpp:2582-2585`, `zhuyin.cpp:1652-1662`;
        // `phonetic_lookup.cpp:120-168`), which drops a forcing whose token
        // does not spell its span. The record follows the forcings that
        // survive.
        if target.nbest_row.is_none()
            && let Some(token) = token
        {
            self.constraints
                .add(constraint_start, constraint_end, token);
            return self.settle_written_forcing();
        }
        self.record_row_choice(target, constraint_end)
    }

    /// The row path of [`Self::select_target`], for an n-best row or a
    /// choose without a token: record the token history, constrain the
    /// phrases where an n-best row differs from the best one, move the
    /// composition offset to `constraint_end` and
    /// refresh.
    fn record_row_choice(
        &mut self,
        target: &SelectionTarget,
        constraint_end: usize,
    ) -> Result<Selection, EngineError> {
        let state = self.state.borrow_mut();
        if let Some(token) = target.token {
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
                // A row replaces the whole path, including any earlier selection.
                state
                    .record
                    .replace_history(row.spans.iter().map(|span| span.token));
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
                self.refresh_after_parse()?;
                return Ok(self.selection_outcome());
            };
            if let Some(best) = best {
                // NBEST choose does not validate or resize the store
                // (pinyin.cpp:2515-2520). diff_result takes its tail from
                // the decoded result, not the current parse (:192).
                state
                    .constraints
                    .diff_result(best, &chosen.spans, chosen.span);
            }
        }
        state.record.set_consumed(constraint_end);
        self.refresh_after_parse()?;

        Ok(self.selection_outcome())
    }

    /// Validates a tokenless choice at the start of the composition and
    /// records any orphaned span for a later clear or forcing write.
    ///
    /// # Errors
    /// Returns a lookup error from either constraint validation.
    pub fn choose_tokenless(&mut self) -> Result<(), EngineError> {
        self.validate_selection_constraints()?;
        if self.constraints.choose_tokenless() {
            self.rebuild_selection_from_constraints();
        }
        self.validate_selection_constraints()
    }

    /// Choose a previously published row whose item is currently unreadable.
    /// Apply its constraint and validate it without decoding a sentence.
    fn choose_unreadable_step(
        &mut self,
        start: usize,
        end: usize,
        token: PhraseToken,
    ) -> Result<usize, EngineError> {
        self.validate_selection_constraints()?;
        let len = self.constraints.add(start, end, token);
        self.validate_selection_constraints()?;
        Ok(len)
    }

    /// The matrix size a choose validates the store to, the pin's
    /// `m_constraints->len` after `validate_constraint` (074a2219
    /// `phonetic_lookup.cpp:64-65`): 0 for a parse without keys.
    #[must_use]
    pub(crate) fn selection_capacity(&self) -> usize {
        self.selection_constraint_size()
    }

    /// The pin's choose of an NBEST row: only the rank is read off the row;
    /// the results, their spans and their text are this session's own
    /// (074a2219 `pinyin.cpp:2515-2520`). The caller checks
    /// [`Self::has_nbest_rank`] first, where the pin asserts.
    ///
    /// # Errors
    ///
    /// [`EngineError`] from the refresh that follows the selection.
    pub(crate) fn select_nbest_row(&mut self, rank: u8) -> Result<Selection, EngineError> {
        let Some(row) = self.sentence.rows.get(usize::from(rank)) else {
            return Ok(Selection::Refused);
        };
        let span_end = row.span;
        self.select_target(&SelectionTarget {
            span_start: 0,
            span_end,
            token: None,
            nbest_row: Some(rank),
        })
    }

    /// The pin's normal choose of `token` over `[start, end)` in store
    /// coordinates (074a2219 `pinyin.cpp:2577-2590`, `zhuyin.cpp:1646-1666`).
    /// The item lookup still chooses the unreadable-item path, which writes
    /// and validates the constraint without refreshing the candidate list.
    ///
    /// # Errors
    ///
    /// [`EngineError`] from the validations and the refresh.
    pub(crate) fn select_row(
        &mut self,
        start: usize,
        end: usize,
        token: PhraseToken,
        text: RowText,
    ) -> Result<Selection, EngineError> {
        if text == RowText::Resolve && !self.dictionary.has_phrase_text_for_token(token) {
            let len = self.choose_unreadable_step(start, end, token)?;
            return Ok(if len == 0 {
                Selection::Refused
            } else {
                self.selection_outcome()
            });
        }
        self.select_target(&SelectionTarget {
            span_start: start,
            span_end: end,
            token: Some(token),
            nbest_row: None,
        })
    }

    /// A choose whose span does not start or end on a key of the current
    /// parse. The pin writes the forcing at a matrix column no key fills;
    /// its second `validate_constraint` removes that forcing, and the
    /// forcings `add_constraint` cleared stay cleared (074a2219
    /// `pinyin.cpp:2577-2590`, `phonetic_lookup.cpp:61-86`). `[start, end)`
    /// covers, in store coordinates, the keys the span overlaps.
    ///
    /// # Errors
    ///
    /// [`EngineError`] from either validation.
    pub(crate) fn choose_unspellable_span(
        &mut self,
        start: usize,
        end: usize,
    ) -> Result<Selection, EngineError> {
        self.validate_selection_constraints()?;
        let mut cleared = false;
        let end = end.min(self.constraints.cell_count());
        for index in start..end {
            cleared |= self.constraints.clear_by_offset(index);
        }
        self.constraints.note_write(start..end);
        if cleared {
            self.rebuild_selection_from_constraints();
        }
        self.validate_selection_constraints()?;
        Ok(self.selection_outcome())
    }

    /// A choose `add_constraint` refuses (`end > m_constraints->len`): both
    /// validations still run, and nothing else changes (074a2219
    /// `pinyin.cpp:2578-2587`, `zhuyin.cpp:1646-1666`).
    ///
    /// # Errors
    ///
    /// [`EngineError`] from either validation.
    pub fn refuse_selection(&mut self) -> Result<Selection, EngineError> {
        self.validate_selection_constraints()?;
        self.validate_selection_constraints()?;
        Ok(Selection::Refused)
    }

    /// Revalidate existing forcings on a refused choose, preserving the pin's
    /// resize and spelling checks without attempting sentence decoding.
    fn validate_selection_constraints(&mut self) -> Result<(), EngineError> {
        self.revalidate_selection_constraints().map(|_| ())
    }

    /// [`Self::validate_selection_constraints`], answering whether it rebuilt
    /// the selection record from the forcings.
    fn revalidate_selection_constraints(&mut self) -> Result<bool, EngineError> {
        let size = self.selection_constraint_size();
        let mut store = core::mem::take(&mut self.constraints);
        let result = {
            let retained = self.retained_parse();
            let spelling = crate::engine::nbest::ConstraintSpelling::new(&retained.matrix);
            store.validate(size, |start, end, token| {
                spelling.spells(&self.dictionary, start, end, token)
            })
        };
        self.constraints = store;
        let rebuild = result? || (self.constraints.is_active() && self.record.history().is_empty());
        if rebuild {
            self.rebuild_selection_from_constraints();
        }
        Ok(rebuild)
    }

    /// `fill_matrix` clears the matrix and returns early for zero keys;
    /// otherwise its size is parsed_len + 1, not the raw input length
    /// (074a2219 storage/phonetic_key_matrix.cpp:30-48). Read the retained
    /// key path and matrix rather than keeping another parse-length field.
    fn selection_constraint_size(&self) -> usize {
        if self.selected_path().edges.is_empty() {
            0
        } else {
            self.retained_parse().matrix.bound + 1
        }
    }

    /// A choose whose span starts before the composition offset:
    /// the pin's `add_constraint(m_begin, m_end,
    /// token)` (`pinyin.cpp:2582-2584`, `phonetic_lookup.cpp:61-86`) —
    /// every forcing the span overlaps is cleared, the span is forced, and
    /// forcings outside it survive — after which the selection record is
    /// re-derived from the store
    /// ([`Session::rebuild_selection_from_constraints`]), so the
    /// composition offset moves back to the end of the forced prefix. A
    /// row with no token has nothing to force; it keeps the old refusal
    /// ([`EngineError::Failure`]).
    fn select_behind(
        &mut self,
        start: usize,
        end: usize,
        token: Option<PhraseToken>,
    ) -> Result<Selection, EngineError> {
        let Some(token) = token else {
            return Err(EngineError::Failure);
        };
        let state = self.state.borrow_mut();
        state.constraints.resize(state.input.len() + 1);
        state.constraints.add(start, end, token);
        self.settle_written_forcing()
    }

    /// After a normal choose's `add_constraint`: the pin's second
    /// `validate_constraint` (074a2219 `pinyin.cpp:2585`, `zhuyin.cpp:1662`;
    /// `phonetic_lookup.cpp:120-168`) drops a forcing whose token does not
    /// spell its span, and the selection record is rebuilt from the
    /// forcings that survive. The pin's choose writes no candidate list
    /// (`pinyin.cpp:2578-2590`; its getters read `m_candidates`, `:2845-2864`),
    /// so a deferring session leaves its list stale here too.
    fn settle_written_forcing(&mut self) -> Result<Selection, EngineError> {
        // The record is a function of the forcings, so a validation that
        // rebuilt it already left the record the rebuild below would make.
        if !self.revalidate_selection_constraints()? {
            self.rebuild_selection_from_constraints();
        }
        self.refresh_after_parse()?;
        Ok(self.selection_outcome())
    }

    /// Moves the forcings into new store coordinates before a validation
    /// (`ConstraintStore::remap`), then rebuilds
    /// the selection record from the forcings that remain.
    pub(crate) fn remap_constraints(
        &mut self,
        forcing: impl Fn(usize, usize) -> Option<(usize, usize)>,
        orphaned: impl Fn(&std::ops::Range<usize>) -> std::ops::Range<usize>,
    ) {
        if !self.constraints.has_spans_to_remap() {
            return;
        }
        self.constraints.remap(forcing, orphaned);
        self.rebuild_selection_from_constraints();
    }

    /// Clears the constraint run at `offset` — `pinyin_clear_constraint`
    /// (`pinyin.cpp:2641-2647`). The offset indexes the store's
    /// coordinate space (raw-buffer byte positions, #141's law); a hit
    /// anywhere inside a forced run un-forces the whole run. The
    /// selection record follows the surviving forcings, so the cleared
    /// phrase's token leaves the record.
    ///
    /// Returns `false` for a free cell or an out-of-range offset.
    #[must_use]
    pub fn clear_constraint(&mut self, offset: usize) -> bool {
        if !self.constraints.clear_by_offset(offset) {
            return false;
        }
        self.rebuild_selection_from_constraints();
        true
    }

    /// Rebuilds the selection record (`consumed`, `history`)
    /// from the surviving forcings. Upstream keeps no such record — the
    /// frontend tracks its own cursor — so the store is the engine's
    /// single source once forcings exist.
    pub(super) fn rebuild_selection_from_constraints(&mut self) {
        let runs = self.constraints.runs();
        let state = self.state.borrow_mut();
        state
            .record
            .rebuild_from_constraints(state.input.len(), &runs);
    }
}

/// 074a2219 `get_column_size(offset) > 0` (`storage/phonetic_key_matrix.cpp`):
/// a column is occupied by either a real key or a zero key. `from_scan`
/// records real keys in `matrix` and internal separator zero keys (and the
/// reserved end slot) in `zeros`.
pub(crate) fn column_is_empty(
    matrix: &super::matrix::ParsedMatrix,
    zeros: &[bool],
    offset: usize,
) -> bool {
    matrix.get(offset).is_none_or(<[ScanKey]>::is_empty)
        && !zeros.get(offset).copied().unwrap_or(false)
}

/// 074a2219 storage/phonetic_key_matrix.cpp:603-670: lazy depth-first
/// traversal keeps only one path, like the pin's cached_keys. Matrix order
/// and multiplicity survive; separator zero keys are folded into scan edges.
/// `compute_pronunciation_possibility_recur` (`:534-585`), which revalidates
/// a forcing, walks the columns the same way, so
/// [`crate::engine::nbest::ConstraintSpelling`] reads its paths from here too.
pub(crate) fn training_readings<'a>(
    matrix: &'a super::matrix::ParsedMatrix,
    input: &'a [u8],
    start: usize,
    end: usize,
    length: usize,
    stacks: &'a mut ReadingStacks,
) -> TrainingReadings<'a> {
    stacks.frames.clear();
    stacks.keys.clear();
    stacks.frames.push((start, 0));
    TrainingReadings {
        matrix,
        input,
        end,
        length,
        stacks,
        yielded: false,
    }
}

/// The reading walk's stacks, which the caller owns and lends to each walk,
/// as the pin reuses one `m_cached_keys` buffer (`phonetic_lookup.h:924-926`).
/// The walk pushes one frame per key and stops at `length` keys, so it never
/// holds more than `length + 1` frames and `length` keys. Up to
/// `MAX_PHRASE_LENGTH` keys they stay inline. `length` is the item's stored
/// header count (`nbest_rows`), which a crafted item can set past that
/// bound; such a walk spills to the heap.
#[derive(Default)]
pub(crate) struct ReadingStacks {
    frames: smallvec::SmallVec<[(usize, usize); MAX_PHRASE_LENGTH + 1]>,
    keys: smallvec::SmallVec<[crate::parser::ChewingKey; MAX_PHRASE_LENGTH]>,
}

pub(crate) struct TrainingReadings<'a> {
    matrix: &'a super::matrix::ParsedMatrix,
    input: &'a [u8],
    end: usize,
    length: usize,
    stacks: &'a mut ReadingStacks,
    yielded: bool,
}

impl TrainingReadings<'_> {
    fn backtrack(&mut self) {
        self.stacks.frames.pop();
        if !self.stacks.frames.is_empty() {
            self.stacks.keys.pop();
        }
    }

    /// Every reading of the walk, copied, for a test to compare.
    #[cfg(test)]
    pub(crate) fn collect_all(mut self) -> Vec<Vec<crate::parser::ChewingKey>> {
        use crate::model::ReadingWalk;
        let mut readings = Vec::new();
        while let Some(reading) = self.next_reading() {
            readings.push(reading.to_vec());
        }
        readings
    }

    /// How many readings the walk lends.
    #[cfg(test)]
    pub(crate) fn count_all(mut self) -> usize {
        use crate::model::ReadingWalk;
        let mut count = 0;
        while self.next_reading().is_some() {
            count += 1;
        }
        count
    }
}

/// Each reading is the walk's key stack itself, lent until the next call,
/// as the pin hands `cached_keys` to the item (`phonetic_key_matrix.cpp:614-621`).
impl crate::model::ReadingWalk for TrainingReadings<'_> {
    fn next_reading(&mut self) -> Option<&[crate::parser::ChewingKey]> {
        if self.yielded {
            self.yielded = false;
            self.backtrack();
        }
        while let Some(&(position, index)) = self.stacks.frames.last() {
            // :607-621: exact span and phrase length only.
            if position == self.end && self.stacks.keys.len() == self.length {
                self.yielded = true;
                return Some(&self.stacks.keys);
            }
            // 074a2219 phonetic_key_matrix.cpp:635-642: a zero key
            // advances without adding a syllable, including a trailing
            // separator after the last key. Other separator hops are
            // already folded into the scan edge that follows them.
            if position < self.end
                && self.input.get(position) == Some(&b'\'')
                && self.matrix.get(position).is_some_and(<[ScanKey]>::is_empty)
            {
                if let Some(frame) = self.stacks.frames.last_mut() {
                    frame.0 += 1;
                }
                continue;
            }
            if position >= self.end || self.stacks.keys.len() >= self.length {
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
            if let Some(frame) = self.stacks.frames.last_mut() {
                frame.1 += 1;
            }
            // :629-651: visit each matrix key in order, without deduping.
            if edge.to <= position {
                continue;
            }
            self.stacks
                .keys
                .push(edge.key.chewing_key().with_tone(edge.tone));
            self.stacks.frames.push((edge.to, 0));
        }
        None
    }
}

#[cfg(test)]
mod tests;
