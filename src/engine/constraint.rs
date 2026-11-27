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
use std::ops::Range;

use crate::engine::error::EngineError;
use crate::vocab::PhraseToken;

/// One constraint cell (`trellis_constraint_t`,
/// `phonetic_lookup.h:121-137`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Cell {
    /// `NO_CONSTRAINT` — a free step.
    None,
    /// `CONSTRAINT_ONESTEP` — `token` is forced over `[start, end)`;
    /// `end` is the cell's own index plus the span.
    OneStep { token: PhraseToken, end: usize },
    /// `CONSTRAINT_NOSEARCH` — interior of a forced run; `start` points
    /// back at the run's `OneStep` cell.
    NoSearch { start: usize },
}

/// One phrase of a decoded sentence, at its absolute matrix position —
/// the `MatchResult` shape `diff_result` walks (a phrase token at its
/// start position; the interior positions it covers are null upstream).
#[derive(Clone, Debug)]
pub struct PhraseSpan {
    pub start: usize,
    pub token: PhraseToken,
    /// The item's stored phrase length, the `get_phrase_length` training
    /// walks the span's readings for (074a2219 `phonetic_lookup.h:922-927`).
    pub(crate) phrase_length: usize,
}

/// The constraint array (`ForwardPhoneticConstraints`).
#[derive(Clone, Debug, Default)]
pub struct ConstraintStore {
    cells: Vec<Cell>,
    orphaned: Option<OrphanedConstraint>,
}

/// The clear assertion left by a tokenless choice at column zero. A later
/// write can replace part of a span, including its middle. Keep the remaining
/// span bounds and whether a real forcing has replaced the tokenless head.
#[derive(Clone, Debug)]
struct OrphanedConstraint {
    spans: Vec<Range<usize>>,
    tokenless_head: bool,
}

impl OrphanedConstraint {
    fn overwrite(&mut self, written: Range<usize>) {
        let mut right = None;
        self.spans.retain_mut(|span| {
            if written.end <= span.start || span.end <= written.start {
                return true;
            }
            if span.start < written.start {
                if written.end < span.end {
                    right = Some(written.end..span.end);
                }
                span.end = written.start;
            } else {
                span.start = written.end;
            }
            span.start < span.end
        });
        self.spans.extend(right);
        self.tokenless_head &= written.start != 0;
    }
}

impl ConstraintStore {
    /// Whether any forcing exists — the empty store is what the frozen
    /// pins run under, and the walk must degenerate with it.
    pub(crate) fn is_active(&self) -> bool {
        self.cells.iter().any(|cell| !matches!(cell, Cell::None))
    }

    pub(crate) fn has_spans_to_remap(&self) -> bool {
        self.orphaned.is_some() || self.is_active()
    }

    /// `clear()` — `pinyin_reset`'s rule.
    pub(crate) fn clear(&mut self) {
        self.cells.clear();
        self.orphaned = None;
    }

    /// Bounds-checked cell read (`get_constraint`).
    pub(crate) fn cell(&self, index: usize) -> Option<&Cell> {
        self.cells.get(index)
    }

    /// The cell count — upstream's `m_constraints->len`, the length
    /// `train_result3` walks and the tail `:921` clamps to. It survives a
    /// re-parse (upstream's parse never validates, `pinyin.cpp:1497-1525`),
    /// so it can exceed the current matrix's `size()`.
    pub(crate) fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// The token the cell at `index` forces, `None` for any other cell.
    pub(crate) fn one_step_token_at(&self, index: usize) -> Option<PhraseToken> {
        if let Some(Cell::OneStep { token, .. }) = self.cell(index) {
            Some(*token)
        } else {
            None
        }
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
        self.map_orphaned(|span| span.start..span.end.min(len));
    }

    /// Record the assertion condition without a zero-length forcing.
    /// `add_constraint(0, 0, null_token)` replaces the head without clearing
    /// its interior (`phonetic_lookup.cpp:61-86`).
    pub(crate) fn choose_tokenless(&mut self) -> bool {
        let end = match self.cell(0) {
            Some(Cell::OneStep { end, .. }) => *end,
            _ => 0,
        };
        if end > 0 {
            self.clear_by_offset(0);
        }
        if end > 1 {
            self.orphaned
                .get_or_insert_with(|| OrphanedConstraint {
                    spans: Vec::new(),
                    tokenless_head: true,
                })
                .spans
                .push(1..end);
        }
        if let Some(orphaned) = &mut self.orphaned {
            orphaned.tokenless_head = true;
        }
        end > 0
    }

    pub(crate) fn note_write(&mut self, span: Range<usize>) {
        if span.is_empty() {
            return;
        }
        if let Some(orphaned) = &mut self.orphaned {
            orphaned.overwrite(span);
            if orphaned.spans.is_empty() {
                self.orphaned = None;
            }
        }
    }

    fn map_orphaned(&mut self, map: impl Fn(&Range<usize>) -> Range<usize>) {
        if let Some(orphaned) = &mut self.orphaned {
            orphaned.spans.retain_mut(|span| {
                *span = map(span);
                span.start < span.end
            });
            if orphaned.spans.is_empty() {
                self.orphaned = None;
            }
        }
    }

    /// `add_constraint` (`phonetic_lookup.cpp:61-86`): clear every forcing
    /// the span overlaps — `clear_constraint` per covered cell (`:66-68`),
    /// which clears each overlapped run whole, the part outside the span
    /// included — then write the `OneStep` at `start` and `NoSearch`
    /// back-pointers on the interior. Returns the span length, or 0 when
    /// the span overruns the array (upstream's refusal, not an abort).
    /// The clear of an orphaned continuation aborts if its head is gone.
    pub(crate) fn add(&mut self, start: usize, end: usize, token: PhraseToken) -> usize {
        if self.add_refuses(start, end) {
            return 0;
        }
        for index in start..end {
            self.clear_by_offset(index);
        }
        self.note_write(start..end);
        self.cells[start] = Cell::OneStep { token, end };
        for cell in &mut self.cells[start + 1..end] {
            *cell = Cell::NoSearch { start };
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
            Some(Cell::None) => return self.clear_orphaned(index),
            None => return false,
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

    fn clear_orphaned(&mut self, index: usize) -> bool {
        let Some(orphaned) = &self.orphaned else {
            return false;
        };
        if index != 0 && !orphaned.spans.iter().any(|span| span.contains(&index)) {
            return false;
        }
        if orphaned.tokenless_head {
            return true;
        }
        if index == 0 {
            return false;
        }
        if !self.clear_by_offset(0) {
            // A continuation reached a head that no longer exists.
            // This is also the clear inside add_constraint (:67-68).
            std::process::abort();
        }
        true
    }

    /// Moves the store into new coordinates, the pin's cells keeping their
    /// matrix columns (`phonetic_lookup.cpp:120-163`). A caller whose store
    /// coordinates moved under a reparse runs this before validation.
    ///
    /// `forcing` maps each live forcing. `orphaned` maps the bounds of the
    /// recorded assertion condition.
    pub(crate) fn remap(
        &mut self,
        forcing: impl Fn(usize, usize) -> Option<(usize, usize)>,
        orphaned: impl Fn(&Range<usize>) -> Range<usize>,
    ) {
        let forcings: Vec<(usize, usize, PhraseToken)> = self
            .cells
            .iter()
            .enumerate()
            .filter_map(|(start, cell)| match cell {
                Cell::OneStep { token, end } => Some((start, *end, *token)),
                Cell::None | Cell::NoSearch { .. } => None,
            })
            .collect();
        let pending = self.orphaned.take();
        self.cells.fill(Cell::None);
        for (start, end, token) in forcings {
            let Some((start, end)) = forcing(start, end).filter(|(start, end)| start < end) else {
                continue;
            };
            if self.cells.len() <= end {
                self.cells.resize(end + 1, Cell::None);
            }
            self.add(start, end, token);
        }
        self.orphaned = pending;
        self.map_orphaned(orphaned);
    }

    /// `validate_constraint`'s per-cell pass (`phonetic_lookup.cpp:142-168`):
    /// after resizing to `len`, drop every `OneStep` whose span overruns
    /// the array or whose forced token no longer spells over its span
    /// under the current matrix — upstream drops at pronunciation
    /// possibility `< FLT_EPSILON`, which `still_spells` decides from the
    /// token's stored pronunciations against the matrix
    /// ([`crate::engine::nbest::ConstraintSpelling::spells`]); it is handed the
    /// run's start, end, token and the forced phrase's stored length, the
    /// `get_phrase_length` the pin's check reads
    /// (`storage/phonetic_key_matrix.cpp:541-551`).
    /// Returns whether anything dropped.
    ///
    /// The overrun boundary is `end >= len`, mirroring upstream verbatim,
    /// and it is not an off-by-one against `add`'s `end > len` refusal:
    /// `len` is the matrix size — one column per input byte PLUS the
    /// reserved tail slot — so `end == len - 1` (a run ending at the last
    /// real byte, the exact-buffer-end forcing the backspace ladder's
    /// floor pins) survives, while `end == len` would end the run ON the
    /// tail slot: the walk inserts at `end`, and an insert there targets
    /// the reserved slot instead of a real column (upstream's trellis
    /// prepares exactly `size()` steps). `add` accepting `end == len` is
    /// upstream's own asymmetry — no caller produces such a span, since
    /// candidate spans end at or before the parsed bound.
    ///
    /// # Errors
    ///
    /// Propagates the spelling probe's backend failure.
    pub(crate) fn validate<F>(&mut self, len: usize, still_spells: F) -> Result<bool, EngineError>
    where
        F: Fn(usize, usize, PhraseToken) -> Result<bool, EngineError>,
    {
        self.resize(len);
        // The forcings to inspect, gathered before any mutation clears
        // cells the iteration would otherwise alias.
        let forcings: Vec<(usize, PhraseToken, usize)> = self
            .cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| {
                if let Cell::OneStep { token, end, .. } = cell {
                    Some((index, *token, *end))
                } else {
                    None
                }
            })
            .collect();
        let mut dropped = false;
        for (index, token, end) in forcings {
            // Resize and check the spelling of each forcing in matrix order.
            let spells = still_spells(index, end, token);
            let overruns = end >= self.cells.len();
            if overruns || !spells? {
                self.clear_by_offset(index);
                dropped = true;
            }
        }
        Ok(dropped)
    }

    /// `diff_result` (`phonetic_lookup.cpp:172-205`): constrain every
    /// phrase where the chosen n-best row (`other`) differs from the
    /// 1-best (`best`) at the same position. A differing phrase is forced
    /// over `[its start, the next phrase's start)` — the tail node span
    /// when no phrase follows. Returns whether a phrase differs. A refused
    /// write aborts, as check_result does (`:203`).
    pub(crate) fn diff_result(
        &mut self,
        best: &[PhraseSpan],
        other: &[PhraseSpan],
        tail: usize,
    ) -> bool {
        let mut changed = false;
        for (span, next_pos) in differing_spans(best, other, tail) {
            changed = true;
            // check_result is an assert at the n-best choose's write
            // (phonetic_lookup.cpp:189-203 at 074a2219).
            if self.add(span.start, next_pos, span.token) == 0 {
                std::process::abort();
            }
        }
        changed
    }

    /// Whether [`Self::diff_result`] with the same arguments reaches its
    /// abort: a write of a differing phrase answers zero. The store's length
    /// decides each answer, and no write changes the length, so the store
    /// before the first write decides them all. Reads only.
    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn diff_result_refuses(
        &self,
        best: &[PhraseSpan],
        other: &[PhraseSpan],
        tail: usize,
    ) -> bool {
        differing_spans(best, other, tail)
            .any(|(span, next_pos)| self.add_refuses(span.start, next_pos))
    }

    /// Whether [`Self::add`] answers zero for `[start, end)`.
    fn add_refuses(&self, start: usize, end: usize) -> bool {
        end > self.cells.len() || start >= end
    }

    /// The surviving forcings as `(start, end, token)` runs, left to right.
    pub(crate) fn runs(&self) -> Vec<(usize, usize, PhraseToken)> {
        self.cells
            .iter()
            .enumerate()
            .filter_map(|(start, cell)| {
                if let Cell::OneStep { token, end } = cell {
                    Some((start, *end, *token))
                } else {
                    None
                }
            })
            .collect()
    }
}

/// The walk of `diff_result` (`phonetic_lookup.cpp:172-205`): each phrase of
/// `other` whose token differs from the token `best` holds at the same
/// position, with the end of its write, which is the next phrase's start, or
/// `tail` when no phrase follows.
fn differing_spans<'a>(
    best: &'a [PhraseSpan],
    other: &'a [PhraseSpan],
    tail: usize,
) -> impl Iterator<Item = (&'a PhraseSpan, usize)> {
    // Both rows come from extract's reversed backward walk: starts ascend.
    let mut best = best.iter().peekable();
    other.iter().enumerate().filter_map(move |(index, span)| {
        while best
            .peek()
            .is_some_and(|candidate| candidate.start < span.start)
        {
            best.next();
        }
        let best_token = best
            .peek()
            .filter(|candidate| candidate.start == span.start)
            .map(|candidate| candidate.token);
        if best_token == Some(span.token) {
            return None;
        }
        let next_pos = other.get(index + 1).map_or(tail, |next| next.start);
        Some((span, next_pos))
    })
}

#[cfg(test)]
mod tests;
