//! The selection record: what the user has chosen so far.
//!
//! Groups the three fields that always move together — the committed text,
//! the composition offset (the lookup anchor and the store's coordinate
//! origin) and the token history a `pinyin_train` walks — behind one type
//! so a caller cannot advance the cursor without the text and history that
//! must move with it. Upstream keeps no such record (the frontend tracks
//! its own cursor); once forcings exist the constraint store is the source
//! of truth, and [`SelectionRecord::rebuild_from_constraints`] re-derives
//! all three from it.
//!
use oxpinyin_core::{PhraseText, PhraseTextConcat, PhraseToken};

/// The user's selections in this composition.
#[derive(Clone, Debug, Default)]
pub(super) struct SelectionRecord {
    /// The text retained by selections so far, including raw gaps. A
    /// chosen phrase keeps its [`PhraseText`], stored words included, and
    /// each part is cut at its own first zero when joined (the pin converts
    /// every phrase separately, `src/lookup/lookup.cpp:39-68` at 074a2219).
    selected: PhraseText,
    /// Bytes of the raw input the selections consumed: the composition
    /// offset the candidate lookup is anchored at and the store's
    /// coordinate origin. Never past the raw input's length.
    consumed: usize,
    /// The phrase token of every pinned phrase, in selection order — the
    /// record a `pinyin_train` trains.
    history: Vec<PhraseToken>,
}

impl SelectionRecord {
    /// Bytes consumed by selections — the composition offset.
    pub(super) const fn consumed(&self) -> usize {
        self.consumed
    }

    /// The text chosen so far.
    pub(super) const fn selected(&self) -> &PhraseText {
        &self.selected
    }

    /// The pinned tokens in selection order.
    pub(super) fn history(&self) -> &[PhraseToken] {
        &self.history
    }

    /// Empties the record — the all-or-nothing un-select and the full
    /// reset. The constraint store is cleared by the caller alongside
    /// this, or a forcing would outlive its own selection.
    pub(super) fn clear(&mut self) {
        self.selected = PhraseText::default();
        self.consumed = 0;
        self.history.clear();
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
    pub(super) fn append_selected(&mut self, gap: &str, text: &PhraseText) {
        let mut selected = PhraseTextConcat::new();
        selected.push(&self.selected);
        selected.push(&PhraseText::from(gap));
        selected.push(text);
        self.selected = selected.finish();
    }

    /// Replaces the text chosen so far with `text` — an n-best row, whose
    /// text already covers the whole composition, so no gap is prepended.
    pub(super) fn set_selected(&mut self, text: &PhraseText) {
        self.selected = text.clone();
    }

    /// Appends one token to the record.
    pub(super) fn push_token(&mut self, token: PhraseToken) {
        self.history.push(token);
    }

    /// Replaces the history with a chosen n-best row's tokens: the row is
    /// a path over the whole matrix, so a normal selection made before it
    /// leaves no token behind.
    pub(super) fn replace_history(&mut self, tokens: &[PhraseToken]) {
        self.history.clear();
        self.history.extend_from_slice(tokens);
    }

    /// Advances the composition offset — the
    /// fallback n-best row that matched no stored row (its span still
    /// advances the cursor).
    pub(super) const fn set_consumed(&mut self, consumed: usize) {
        self.consumed = consumed;
    }

    /// Rebuilds all three fields from the surviving forcings — the store is
    /// the engine's single source once forcings exist. Gaps between forced
    /// runs are free spans whose text is the current buffer's bytes, so the
    /// rebuilt record never drops raw input the forcings skip over.
    pub(super) fn rebuild_from_constraints(
        &mut self,
        raw: &str,
        runs: &[(usize, usize, PhraseToken, PhraseText)],
    ) {
        if runs.is_empty() {
            self.clear();
            return;
        }
        let mut selected = PhraseTextConcat::new();
        let mut cursor = 0_usize;
        let mut history = Vec::with_capacity(runs.len());
        for (start, end, token, text) in runs {
            if *start > cursor
                && let Some(gap) = raw.get(cursor..*start)
            {
                selected.push(&PhraseText::from(gap));
            }
            selected.push(text);
            history.push(*token);
            cursor = *end;
        }
        self.selected = selected.finish();
        self.consumed = cursor.min(raw.len());
        self.history = history;
    }
}

#[cfg(test)]
mod tests {
    use super::SelectionRecord;
    use oxpinyin_core::{PhraseText, PhraseToken};

    /// Chosen phrases and raw gaps join in order, an n-best row replaces
    /// the text, and a rebuild keeps the gaps its runs skip over.
    #[test]
    fn the_selected_text_keeps_raw_gaps() {
        let mut record = SelectionRecord::default();
        record.append_selected("", &PhraseText::from("你"));
        record.append_selected("ha", &PhraseText::from("好"));
        assert_eq!(record.selected(), &PhraseText::from("你ha好"));

        record.set_selected(&PhraseText::from("你好"));
        assert_eq!(record.selected(), &PhraseText::from("你好"));

        record.rebuild_from_constraints(
            "nihao",
            &[(2, 5, PhraseToken::new(7), PhraseText::from("好"))],
        );
        assert_eq!(record.selected(), &PhraseText::from("ni好"));
        assert_eq!(record.history(), [PhraseToken::new(7)]);

        record.clear();
        assert!(record.selected().is_empty());
    }
}
