//! The selection record: the composition offset and chosen token history.
//!
//! The offset anchors lookup and the history feeds training. Once forcings
//! exist, the constraint store is their source of truth.

use crate::vocab::PhraseToken;

/// The user's selections in this composition.
#[derive(Clone, Debug, Default)]
pub(super) struct SelectionRecord {
    /// Bytes of raw input consumed by selections. Never past its length.
    consumed: usize,
    /// The pinned phrase tokens in selection order.
    history: Vec<PhraseToken>,
}

impl SelectionRecord {
    /// Bytes consumed by selections — the composition offset.
    pub(super) const fn consumed(&self) -> usize {
        self.consumed
    }

    /// The pinned tokens in selection order.
    pub(super) fn history(&self) -> &[PhraseToken] {
        &self.history
    }

    /// Empties the record. The caller clears the constraint store alongside it.
    pub(super) fn clear(&mut self) {
        self.consumed = 0;
        self.history.clear();
    }

    /// Clamps the composition offset to the character boundary at or before
    /// its old value. Every `raw[consumed..]` slice depends on this.
    pub(super) fn clamp_consumed(&mut self, raw: &str) {
        self.consumed = raw.floor_char_boundary(self.consumed);
    }

    /// Appends one token to the record.
    pub(super) fn push_token(&mut self, token: PhraseToken) {
        self.history.push(token);
    }

    /// Replaces the history with a chosen n-best row's whole token path.
    pub(super) fn replace_history(&mut self, tokens: impl Iterator<Item = PhraseToken>) {
        self.history.clear();
        self.history.extend(tokens);
    }

    /// Advances the composition offset, including a fallback n-best row
    /// that matched no stored row.
    pub(super) const fn set_consumed(&mut self, consumed: usize) {
        self.consumed = consumed;
    }

    /// Rebuilds the offset and history from the surviving forcings, in
    /// matrix order. Free gaps contribute no token to the history.
    pub(super) fn rebuild_from_constraints(
        &mut self,
        raw_len: usize,
        runs: &[(usize, usize, PhraseToken)],
    ) {
        self.consumed = runs.last().map_or(0, |(_, end, _)| *end).min(raw_len);
        self.replace_history(runs.iter().map(|(_, _, token)| *token));
    }
}
