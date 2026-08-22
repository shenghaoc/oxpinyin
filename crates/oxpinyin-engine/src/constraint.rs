//! The §3 constraint store — the port of upstream's
//! `ForwardPhoneticConstraints` (`lookup/phonetic_lookup.cpp:61-205`).
//!
//! One cell per matrix column: the session's raw-buffer byte positions,
//! the same coordinate space `build_scan_matrix` keys its columns by and
//! `pinyin_choose_candidate` reports its cursor in (#141's offset law —
//! one coordinate system end to end, no byte-space conversion).
//!
//! A `OneStep` cell at `start` forces `token` over the span
//! `[start, end)`; the interior cells carry `NoSearch` back-pointers so a
//! clear at any offset inside a forced run finds the run's start. The
//! store survives `parse_more` (the frontend re-sends the whole buffer
//! every keystroke, and only `pinyin_reset` clears upstream's
//! `m_constraints`) — see `Session::reset_composition` for the split.
//!
//! `OneStep` carries the chosen phrase's display text: upstream
//! re-fetches it from the phrase index by token, the engine carries it
//! from the candidate so the selection record can be rebuilt from the
//! store alone (promoted addon phrases included) without a by-token
//! dictionary lookup.

use compact_str::CompactString;
use oxpinyin_core::PhraseToken;

use crate::error::EngineError;

/// One constraint cell (`trellis_constraint_t`,
/// `phonetic_lookup.h:121-137`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Cell {
    /// `NO_CONSTRAINT` — a free step.
    None,
    /// `CONSTRAINT_ONESTEP` — `token` is forced over `[start, end)`;
    /// `end` is the cell's own index plus the span, `text` the chosen
    /// phrase's display form.
    OneStep {
        token: PhraseToken,
        end: usize,
        text: CompactString,
    },
    /// `CONSTRAINT_NOSEARCH` — interior of a forced run; `start` points
    /// back at the run's `OneStep` cell.
    NoSearch { start: usize },
}

/// One phrase of a decoded sentence, at its absolute matrix position —
/// the `MatchResult` shape `diff_result` walks (a phrase token at its
/// start position; the interior positions it covers are null upstream).
#[derive(Clone, Debug)]
pub(crate) struct PhraseSpan {
    pub(crate) start: usize,
    pub(crate) token: PhraseToken,
    pub(crate) text: CompactString,
}

/// The constraint array (`ForwardPhoneticConstraints`).
#[derive(Clone, Debug, Default)]
pub(crate) struct ConstraintStore {
    cells: Vec<Cell>,
}

impl ConstraintStore {
    /// Whether any forcing exists — the empty store is what the frozen
    /// pins run under, and the walk must degenerate with it.
    pub(crate) fn is_active(&self) -> bool {
        self.cells.iter().any(|cell| !matches!(cell, Cell::None))
    }

    /// `clear()` — `pinyin_reset`'s rule.
    pub(crate) fn clear(&mut self) {
        self.cells.clear();
    }

    /// Bounds-checked cell read (`get_constraint`).
    pub(crate) fn cell(&self, index: usize) -> Option<&Cell> {
        self.cells.get(index)
    }

    /// Whether the cell at `index` forces a token (`CONSTRAINT_ONESTEP`).
    pub(crate) fn is_one_step_at(&self, index: usize) -> bool {
        matches!(self.cell(index), Some(Cell::OneStep { .. }))
    }

    /// Grow with free cells / shrink by truncation (`validate_constraint`'s
    /// resize, `phonetic_lookup.cpp:120-140`): typing more keys extends the
    /// array, it never drops existing forcings.
    pub(crate) fn resize(&mut self, len: usize) {
        self.cells.resize(len, Cell::None);
    }

    /// `add_constraint` (`phonetic_lookup.cpp:61-86`): clear anything the
    /// span covers, write the `OneStep` at `start` and `NoSearch`
    /// back-pointers on the interior. Returns the span length, or 0 when
    /// the span overruns the array (upstream's refusal, not an abort).
    pub(crate) fn add(
        &mut self,
        start: usize,
        end: usize,
        token: PhraseToken,
        text: CompactString,
    ) -> usize {
        if end > self.cells.len() || start >= end {
            return 0;
        }
        for cell in &mut self.cells[start..end] {
            *cell = Cell::None;
        }
        self.cells[start] = Cell::OneStep { token, end, text };
        for index in start + 1..end {
            self.cells[index] = Cell::NoSearch { start };
        }
        end - start
    }

    /// `clear_constraint` (`phonetic_lookup.cpp:88-105`): a free cell
    /// answers `false`; a `NoSearch` cell jumps back to its run's start;
    /// the whole run `[start, end)` returns to free. Out of range answers
    /// `false` — upstream's own defined return, not an abort.
    pub(crate) fn clear_by_offset(&mut self, index: usize) -> bool {
        let run_start = match self.cells.get(index) {
            Some(Cell::NoSearch { start }) => *start,
            Some(Cell::OneStep { .. }) => index,
            _ => return false,
        };
        let Some(Cell::OneStep { end, .. }) = self.cells.get(run_start) else {
            return false;
        };
        let end = (*end).min(self.cells.len());
        for cell in &mut self.cells[run_start..end] {
            *cell = Cell::None;
        }
        true
    }

    /// `validate_constraint`'s per-cell pass (`phonetic_lookup.cpp:142-168`):
    /// after resizing to `len`, drop every `OneStep` whose span overruns
    /// the array or whose forced token no longer spells over its span
    /// under the current matrix — upstream drops at pronunciation
    /// possibility `< FLT_EPSILON`; the engine's ported possibility shape
    /// (`sentence-surface.md` §3) makes "the span search no longer yields
    /// the token" the equivalent test. Returns whether anything dropped.
    ///
    /// # Errors
    ///
    /// Propagates the spelling probe's backend failure.
    pub(crate) fn validate<F>(&mut self, len: usize, still_spells: F) -> Result<bool, EngineError>
    where
        F: Fn(usize, usize, PhraseToken) -> Result<bool, EngineError>,
    {
        self.cells.resize(len, Cell::None);
        // The forcings to inspect, gathered before any mutation clears
        // cells the iteration would otherwise alias.
        let forcings: Vec<(usize, PhraseToken, usize)> = self
            .cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| match cell {
                Cell::OneStep { token, end, .. } => Some((index, *token, *end)),
                _ => None,
            })
            .collect();
        let mut dropped = false;
        for (index, token, end) in forcings {
            if end >= self.cells.len() || !still_spells(index, end, token)? {
                // The run's interior cells go with it (`clear_constraint`).
                let end = end.min(self.cells.len());
                for cell in &mut self.cells[index..end] {
                    *cell = Cell::None;
                }
                dropped = true;
            }
        }
        Ok(dropped)
    }

    /// `diff_result` (`phonetic_lookup.cpp:172-205`): constrain every
    /// phrase where the chosen n-best row (`other`) differs from the
    /// 1-best (`best`) at the same position. A differing phrase is forced
    /// over `[its start, the next phrase's start)` — the tail node span
    /// when no phrase follows. Returns whether anything was constrained.
    pub(crate) fn diff_result(
        &mut self,
        best: &[PhraseSpan],
        other: &[PhraseSpan],
        tail: usize,
    ) -> bool {
        let mut changed = false;
        for (index, span) in other.iter().enumerate() {
            let best_token = best
                .iter()
                .find(|candidate| candidate.start == span.start)
                .map(|candidate| candidate.token);
            if best_token == Some(span.token) {
                continue;
            }
            let next_pos = other.get(index + 1).map_or(tail, |next| next.start);
            self.add(span.start, next_pos, span.token, span.text.clone());
            changed = true;
        }
        changed
    }

    /// The selection record the surviving forcings imply: chosen text,
    /// composition offset, and token history, left to right. `None` when
    /// nothing is forced.
    pub(crate) fn selection(&self) -> Option<(CompactString, usize, Vec<PhraseToken>)> {
        let mut selected = CompactString::const_new("");
        let mut consumed = 0;
        let mut history = Vec::new();
        let mut forced = false;
        for cell in &self.cells {
            if let Cell::OneStep { token, end, text } = cell {
                selected.push_str(text);
                consumed = *end;
                history.push(*token);
                forced = true;
            }
        }
        forced.then_some((selected, consumed, history))
    }
}

#[cfg(test)]
mod tests {
    use super::{Cell, ConstraintStore, PhraseSpan};

    fn token(value: u32) -> oxpinyin_core::PhraseToken {
        oxpinyin_core::PhraseToken::new(value)
    }

    fn span(start: usize, value: u32, text: &str) -> PhraseSpan {
        PhraseSpan {
            start,
            token: token(value),
            text: text.into(),
        }
    }

    #[test]
    fn add_writes_the_run_and_clear_walks_it_back() {
        let mut store = ConstraintStore::default();
        store.resize(6);
        assert_eq!(store.add(0, 2, token(7), "你".into()), 2);
        assert_eq!(
            store.cell(0),
            Some(&Cell::OneStep {
                token: token(7),
                end: 2,
                text: "你".into()
            })
        );
        assert_eq!(store.cell(1), Some(&Cell::NoSearch { start: 0 }));

        // A hit inside the run clears the whole run.
        assert!(store.clear_by_offset(1));
        assert_eq!(store.cell(0), Some(&Cell::None));
        assert_eq!(store.cell(1), Some(&Cell::None));
        // Now free: false, exactly upstream's defined return.
        assert!(!store.clear_by_offset(1));
    }

    #[test]
    fn add_refuses_an_overrunning_span() {
        let mut store = ConstraintStore::default();
        store.resize(2);
        assert_eq!(store.add(0, 3, token(7), "你".into()), 0);
        assert!(!store.is_active());
    }

    #[test]
    fn clear_out_of_range_answers_false() {
        let mut store = ConstraintStore::default();
        store.resize(2);
        assert!(!store.clear_by_offset(9));
    }

    #[test]
    fn validate_drops_a_run_that_overruns_or_stops_spelling() {
        let mut store = ConstraintStore::default();
        store.resize(6);
        store.add(0, 2, token(7), "你".into());
        store.add(3, 5, token(8), "好".into());

        // Shrink below the second run's end: the second run drops, the
        // first survives (growing never drops).
        let dropped = store
            .validate(4, |_, _, _| Ok(true))
            .expect("probe cannot fail");
        assert!(dropped);
        assert!(store.is_one_step_at(0));
        assert!(!store.is_one_step_at(3));

        // A token that no longer spells over its span drops.
        let dropped = store
            .validate(6, |start, _, value| Ok(value.value() != 7 || start != 0))
            .expect("probe cannot fail");
        assert!(dropped);
        assert!(!store.is_active());
    }

    #[test]
    fn validate_grows_with_free_cells() {
        let mut store = ConstraintStore::default();
        store.resize(3);
        store.add(0, 2, token(7), "你".into());
        let dropped = store
            .validate(8, |_, _, _| Ok(true))
            .expect("probe cannot fail");
        assert!(!dropped);
        assert!(store.is_one_step_at(0));
        assert_eq!(store.cell(7), Some(&Cell::None));
    }

    #[test]
    fn diff_result_constrains_only_the_differing_phrases() {
        let mut store = ConstraintStore::default();
        store.resize(6);
        // best: 你@[0] 好@[2]; chosen: 你@[0] 浩@[2]
        let best = [span(0, 7, "你"), span(2, 8, "好")];
        let other = [span(0, 7, "你"), span(2, 9, "浩")];
        assert!(store.diff_result(&best, &other, 5));
        assert!(!store.is_one_step_at(0));
        assert!(store.is_one_step_at(2));
        assert_eq!(store.cell(3), Some(&Cell::NoSearch { start: 2 }));
        assert_eq!(store.cell(4), Some(&Cell::NoSearch { start: 2 }));
    }

    #[test]
    fn diff_result_tail_phrase_spans_to_the_tail() {
        let mut store = ConstraintStore::default();
        store.resize(6);
        let best = [span(0, 10, "你好")];
        let other = [span(0, 11, "你浩")];
        assert!(store.diff_result(&best, &other, 5));
        match store.cell(0) {
            Some(Cell::OneStep { end, .. }) => assert_eq!(*end, 5),
            other => panic!("expected OneStep at 0, got {other:?}"),
        }
    }

    #[test]
    fn selection_rebuilds_from_the_surviving_runs() {
        let mut store = ConstraintStore::default();
        store.resize(6);
        store.add(0, 2, token(7), "你".into());
        store.add(2, 5, token(9), "浩".into());
        let (selected, consumed, history) = store.selection().expect("two runs are forced");
        assert_eq!(selected.as_str(), "你浩");
        assert_eq!(consumed, 5);
        assert_eq!(history, vec![token(7), token(9)]);

        store.clear_by_offset(2);
        let (selected, consumed, history) = store.selection().expect("one run survives");
        assert_eq!(selected.as_str(), "你");
        assert_eq!(consumed, 2);
        assert_eq!(history, vec![token(7)]);

        store.clear_by_offset(0);
        assert!(store.selection().is_none());
    }
}
