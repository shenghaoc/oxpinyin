//! The sentence-decode state: the n-best rows and what a chosen row needs.
//!
//! Groups the transient state a sentence lookup produces and the two
//! surfaces that read it back — the prepended candidate rows, and the
//! constraint-aware train walk. These four fields live and die together:
//! [`SentenceState::begin`] clears them and marks a lookup active before a
//! decode fills them, and [`SentenceState::reset`] clears them on the full
//! reset only — upstream's parse never touches `m_nbest_results`
//! (`pinyin.cpp:1497-1524`), which `pinyin_reset` alone clears (`:2700`). `rows`, `history` and `last_result` stay fields
//! because a lookup rewrites them wholesale and
//! [`crate::session::Session::guess_sentence`] mutates the rows in place
//! (absolutising walk-local span positions) — an access pattern no method
//! seam captures without obscuring it.
//!
//! The surface config (`nbest_shape`, the collapse law) is *not* here: it
//! is set once per surface and survives every reset, a different lifetime
//! from this transient decode output, so it stays on the session.

use oxpinyin_core::{PhraseText, PhraseToken};

use crate::constraint::PhraseSpan;
use crate::nbest::NbestRow;

/// One phrase of a decoded sentence, as the C sentence getters read it.
///
/// The pin keeps the result tokens and resolves each one through the
/// phrase index when the caller reads the sentence (`convert_to_utf8`,
/// `src/lookup/lookup.cpp:37-48` at 074a2219). A token is therefore kept
/// as a token and resolved at read time. A raw gap of the selection record
/// has no token and keeps its typed text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SentencePart {
    /// A phrase token, resolved through the live phrase index on each read.
    Token(PhraseToken),
    /// Text that has no token: a typed but unselected gap.
    Raw(PhraseText),
}

/// The last sentence lookup's decoded output.
#[derive(Clone, Debug, Default)]
pub(super) struct SentenceState {
    /// Decoded n-best sentence rows, best-first — upstream's
    /// `m_nbest_results`. Empty means no sentence has been guessed for the
    /// current composition. Cleared only by [`SentenceState::begin`] /
    /// [`SentenceState::reset`]; survives further typing, selections and
    /// re-parses, as upstream's does.
    pub(super) rows: Vec<NbestRow>,
    /// History snapshot taken when the rows were decoded — the seed
    /// context they were decoded against. Selecting an n-best row restores
    /// it before the row's tokens extend the record.
    pub(super) history: Vec<PhraseToken>,
    /// Whether a sentence lookup has run since the last full reset — the
    /// half of the `m_nbest_results` gate an empty-but-active lookup still
    /// satisfies (`pinyin_guess_sentence` attempts the search even on an
    /// empty key matrix, so a later `pinyin_get_sentence` answers `false`
    /// rather than the pre-lookup raw form).
    active: bool,
    /// The last lookup's 1-best phrases at their absolute positions —
    /// upstream's `m_nbest_results[0]`, the result `pinyin_train` walks
    /// against the constraint store.
    pub(super) last_result: Vec<PhraseSpan>,
    /// The selection record's pieces the remaining-input walk prepended to
    /// every row's text, empty when it prepended none: the leading parts of
    /// each row ([`SentenceState::parts`]).
    pub(super) prefix: Vec<SentencePart>,
}

impl SentenceState {
    /// Whether a sentence lookup has run since the last reset.
    pub(super) const fn active(&self) -> bool {
        self.active
    }

    /// `row` phrase by phrase, as the pin's result vector holds it for
    /// `convert_to_utf8` (`src/lookup/lookup.cpp:37-48` at 074a2219): one
    /// token per phrase of the decoded path in position order, the trellis
    /// having written a token only at each phrase's start column and
    /// `null_token` elsewhere (`extract_result`,
    /// `lookup/phonetic_lookup.h:370-396`). A row the trellis decoded
    /// carries those phrases as its spans, after any prepended selection
    /// parts. The caller resolves each token when it reads the sentence.
    pub(super) fn parts<'a>(
        &'a self,
        row: &'a NbestRow,
    ) -> impl Iterator<Item = SentencePart> + 'a {
        let whole = row.spans.is_empty();
        let prefix: &[SentencePart] = if whole { &[] } else { &self.prefix };
        prefix
            .iter()
            .cloned()
            .chain(row.spans.iter().map(|span| SentencePart::Token(span.token)))
            .chain(whole.then(|| SentencePart::Raw(row.text.clone())))
    }

    /// Clears the decoded state and marks a lookup active — the head of
    /// every `guess_sentence`, so a later `pinyin_get_sentence` answers
    /// decoded-or-nothing even when the lookup produced no rows.
    pub(super) fn begin(&mut self) {
        self.rows.clear();
        self.history.clear();
        self.last_result.clear();
        self.prefix.clear();
        self.active = true;
    }

    /// Clears the decoded state without marking a lookup active — the
    /// full reset (`Session::reset`, upstream's `pinyin_reset`,
    /// `pinyin.cpp:2700`), which drops the rows the `m_nbest_results` gate
    /// hangs on.
    pub(super) fn reset(&mut self) {
        self.rows.clear();
        self.history.clear();
        self.last_result.clear();
        self.prefix.clear();
        self.active = false;
    }
}
