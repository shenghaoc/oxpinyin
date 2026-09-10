//! The sentence-decode state: the n-best rows and what a chosen row needs.
//!
//! Groups the transient state a sentence lookup produces and the two
//! surfaces that read it back — the prepended candidate rows, and the
//! constraint-aware train walk. These four fields live and die together:
//! [`SentenceState::begin`] clears them and marks a lookup active before a
//! decode fills them, and [`SentenceState::reset`] clears them on the
//! parse-path reset. `rows`, `history` and `last_result` stay fields
//! because a lookup rewrites them wholesale and
//! [`crate::session::Session::guess_sentence`] mutates the rows in place
//! (absolutising walk-local span positions) — an access pattern no method
//! seam captures without obscuring it.
//!
//! The surface config (`nbest_shape`, the collapse law) is *not* here: it
//! is set once per surface and survives every reset, a different lifetime
//! from this transient decode output, so it stays on the session.

use oxpinyin_core::PhraseToken;

use crate::constraint::PhraseSpan;
use crate::nbest::NbestRow;

/// The last sentence lookup's decoded output.
#[derive(Clone, Debug, Default)]
pub(super) struct SentenceState {
    /// Decoded n-best sentence rows, best-first — upstream's
    /// `m_nbest_results`. Empty means no sentence has been guessed for the
    /// current composition. Cleared only by [`SentenceState::begin`] /
    /// [`SentenceState::reset`]; survives further typing and selections.
    pub(super) rows: Vec<NbestRow>,
    /// History snapshot taken when the rows were decoded — the seed
    /// context they were decoded against. Selecting an n-best row restores
    /// it before the row's tokens extend the record.
    pub(super) history: Vec<PhraseToken>,
    /// Whether a sentence lookup has run for the current composition — the
    /// half of the `m_nbest_results` gate an empty-but-active lookup still
    /// satisfies (`pinyin_guess_sentence` attempts the search even on an
    /// empty key matrix, so a later `pinyin_get_sentence` answers `false`
    /// rather than the pre-lookup raw form).
    active: bool,
    /// The last lookup's 1-best phrases at their absolute positions —
    /// upstream's `m_nbest_results[0]`, the result `pinyin_train` walks
    /// against the constraint store.
    pub(super) last_result: Vec<PhraseSpan>,
}

impl SentenceState {
    /// Whether a sentence lookup has run since the last reset.
    pub(super) const fn active(&self) -> bool {
        self.active
    }

    /// Clears the decoded state and marks a lookup active — the head of
    /// every `guess_sentence`, so a later `pinyin_get_sentence` answers
    /// decoded-or-nothing even when the lookup produced no rows.
    pub(super) fn begin(&mut self) {
        self.rows.clear();
        self.history.clear();
        self.last_result.clear();
        self.active = true;
    }

    /// Clears the decoded state without marking a lookup active — the
    /// parse-path reset (`reset_composition`), which drops the rows the
    /// `m_nbest_results` gate hangs on.
    pub(super) fn reset(&mut self) {
        self.rows.clear();
        self.history.clear();
        self.last_result.clear();
        self.active = false;
    }
}
