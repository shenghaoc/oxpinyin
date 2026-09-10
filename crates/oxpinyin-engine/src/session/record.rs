//! The selection record: what the user has chosen so far.
//!
//! Groups the four fields that always move together — the committed text,
//! the composition offset (the lookup anchor and the store's coordinate
//! origin), the token history a `pinyin_train` walks, and whether a
//! selection consumed the whole buffer — behind one type so a caller
//! cannot advance the cursor without the text and history that must move
//! with it. Upstream keeps no such record (the frontend tracks its own
//! cursor); once forcings exist the constraint store is the source of
//! truth, and [`SelectionRecord::rebuild_from_constraints`] re-derives all
//! four from it.
//!
//! The load-bearing invariant is on `committed`: it means a selection
//! consumed the whole buffer *and no rebuild has since changed the
//! record*. Every operation that rewrites the record from the store
//! ([`SelectionRecord::clear`], [`SelectionRecord::rebuild_from_constraints`])
//! clears the flag, because a survivor that still reaches the buffer end
//! would otherwise leave a stale `committed` set — and the next compatible
//! re-parse would start fresh and silently drop it.

use compact_str::CompactString;

use oxpinyin_core::PhraseToken;

/// The user's selections in this composition.
#[derive(Clone, Debug, Default)]
pub(super) struct SelectionRecord {
    /// The text chosen so far — the selected span of the preedit.
    selected: String,
    /// Bytes of the raw input the selections consumed: the composition
    /// offset the candidate lookup is anchored at and the store's
    /// coordinate origin. Never past the raw input's length.
    consumed: usize,
    /// The phrase token of every pinned phrase, in selection order — the
    /// record a `pinyin_train` trains.
    history: Vec<PhraseToken>,
    /// Whether a selection consumed the whole buffer and no rebuild has
    /// since changed the record — the commit-branch shape the parse rule
    /// keeps composing through.
    committed: bool,
}

impl SelectionRecord {
    /// Bytes consumed by selections — the composition offset.
    pub(super) const fn consumed(&self) -> usize {
        self.consumed
    }

    /// The text chosen so far.
    pub(super) fn selected(&self) -> &str {
        &self.selected
    }

    /// Moves the chosen text out, leaving it empty — the commit path, which
    /// resets the whole record immediately after.
    pub(super) fn take_selected(&mut self) -> String {
        core::mem::take(&mut self.selected)
    }

    /// The pinned tokens in selection order.
    pub(super) fn history(&self) -> &[PhraseToken] {
        &self.history
    }

    /// Whether a selection consumed the whole buffer (and no rebuild has
    /// since changed the record).
    pub(super) const fn committed(&self) -> bool {
        self.committed
    }

    /// Empties the record — the all-or-nothing un-select and the full
    /// reset. The constraint store is cleared by the caller alongside
    /// this, or a forcing would outlive its own selection.
    pub(super) fn clear(&mut self) {
        self.selected.clear();
        self.consumed = 0;
        self.history.clear();
        self.committed = false;
    }

    /// Clamps the composition offset onto `raw` — the offset may sit past
    /// the shrunk buffer, or inside a multi-byte character of a replaced
    /// one, so it lands on the character boundary at or before its old
    /// value. Every `raw[consumed..]` slice depends on this.
    pub(super) fn clamp_consumed(&mut self, raw: &str) {
        self.consumed = self.consumed.min(raw.len());
        while !raw.is_char_boundary(self.consumed) {
            self.consumed -= 1;
        }
    }

    /// Extends the text chosen so far with a typed-but-unselected `gap`
    /// then the chosen `text` — the ordinary selection, whose gap is
    /// empty on the composition-anchored path.
    pub(super) fn append_selected(&mut self, gap: &str, text: &str) {
        self.selected.push_str(gap);
        self.selected.push_str(text);
    }

    /// Replaces the text chosen so far with `text` — an n-best row, whose
    /// text already covers the whole composition, so no gap is prepended.
    pub(super) fn set_selected(&mut self, text: &str) {
        self.selected.clear();
        self.selected.push_str(text);
    }

    /// Appends one token to the record.
    pub(super) fn push_token(&mut self, token: PhraseToken) {
        self.history.push(token);
    }

    /// Restores the history to the lookup-time snapshot `base` and extends
    /// it with a chosen n-best row's own tokens `extra` — the row replaces
    /// everything decoded since the lookup, so a normal selection made in
    /// between leaves no token behind.
    pub(super) fn reset_history_extend(&mut self, base: &[PhraseToken], extra: &[PhraseToken]) {
        self.history.clear();
        self.history.extend_from_slice(base);
        self.history.extend_from_slice(extra);
    }

    /// Advances the composition offset without touching `committed` — the
    /// fallback n-best row that matched no stored row (its span still
    /// advances the cursor).
    pub(super) const fn set_consumed(&mut self, consumed: usize) {
        self.consumed = consumed;
    }

    /// Records whether the just-made selection consumed the whole buffer.
    pub(super) const fn set_committed(&mut self, committed: bool) {
        self.committed = committed;
    }

    /// Rebuilds all four fields from the surviving forcings — the store is
    /// the engine's single source once forcings exist. Gaps between forced
    /// runs are free spans whose text is the current buffer's bytes, so the
    /// rebuilt record never drops raw input the forcings skip over. Clears
    /// `committed`: a rebuild means the record changed under the selection,
    /// so the commit-branch shape no longer holds even when the survivors
    /// still reach the buffer end.
    pub(super) fn rebuild_from_constraints(
        &mut self,
        raw: &str,
        runs: &[(usize, usize, PhraseToken, CompactString)],
    ) {
        if runs.is_empty() {
            self.clear();
            return;
        }
        let mut selected = String::new();
        let mut cursor = 0_usize;
        let mut history = Vec::with_capacity(runs.len());
        for (start, end, token, text) in runs {
            if *start > cursor
                && let Some(gap) = raw.get(cursor..*start)
            {
                selected.push_str(gap);
            }
            selected.push_str(text);
            history.push(*token);
            cursor = *end;
        }
        self.selected = selected;
        self.consumed = cursor.min(raw.len());
        self.history = history;
        self.committed = false;
    }
}
