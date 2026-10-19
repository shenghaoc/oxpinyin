//! Candidates and checked iteration over them.
//!
//! Every index is checked. `docs/findings/robustness-evidence.md` row F-E-02
//! is an upstream invalid access through a candidate index that outlived the
//! list it came from; here a stale index is an `Option` or an `Err`, and the
//! session stays usable.

use core::slice;

use compact_str::CompactString;
use oxpinyin_core::{Cost, PhraseEntry, PhraseToken, holds_zero, phrase_c_string};

/// Where a candidate came from.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CandidateKind {
    /// A dictionary phrase covering part of the composition.
    Phrase,
    /// An addon-facade phrase (`ADDON_CANDIDATE`).
    Addon,
    /// A decoded sentence covering the whole composition.
    Sentence,
    /// Raw input offered back when no phrase was found.
    Fallback,
}

/// One offered conversion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Candidate {
    text: CompactString,
    kind: CandidateKind,
    consumed_keys: usize,
    consumed_bytes: usize,
    cost: Cost,
    token: Option<PhraseToken>,
    /// Tail rank of the n-best sentence row this candidate is (W14);
    /// `None` for every non-row candidate — including fallback sentence
    /// candidates, which share the row `CandidateKind::Sentence` but are
    /// not rows and carry no token path. The rank is the *original* tail
    /// order, so a row that survives the NBEST-wins dedup still knows
    /// which `NbestRow` is its own even when its list position shifted.
    nbest_index: Option<u8>,
    /// Where the candidate's span starts, in the coordinates of the window
    /// it was built in — upstream's `lookup_candidate_t::m_begin`
    /// (`pinyin.h`/`zhuyin.h`; set beside `m_end` at `zhuyin.cpp:1509,1595`
    /// at the pin). A window anchored at a lookup offset measures every
    /// span from that offset, so its rows carry 0; the before-cursor window
    /// (`Session::candidates_ending_at`) runs on the absolute prefix graph,
    /// so its rows carry each span's absolute start. Sentence rows and
    /// fallbacks carry 0.
    span_start: usize,
    /// Whether `text` holds a U+0000, where the pin's string for the phrase
    /// ends ([`Self::text`]). Settled once, where the text is made, so the
    /// readers of the string (a dictionary hit is read at least once per
    /// refresh) do not scan for it.
    zero: bool,
}

impl Candidate {
    pub(crate) fn new(
        text: impl Into<CompactString>,
        kind: CandidateKind,
        consumed_keys: usize,
        consumed_bytes: usize,
        cost: Cost,
        token: Option<PhraseToken>,
        nbest_index: Option<u8>,
    ) -> Self {
        let text = text.into();
        let zero = holds_zero(&text);
        Self {
            text,
            kind,
            consumed_keys,
            consumed_bytes,
            cost,
            token,
            nbest_index,
            span_start: 0,
            zero,
        }
    }

    /// A dictionary hit as a candidate, with the entry's own answer to
    /// whether its text holds a U+0000 (the dictionary decoder learns it
    /// while it reads the stored words).
    pub(crate) fn from_entry(
        entry: PhraseEntry,
        kind: CandidateKind,
        consumed_keys: usize,
        consumed_bytes: usize,
    ) -> Self {
        let token = entry.token();
        let zero = entry.holds_zero();
        Self {
            text: entry.into_text(),
            kind,
            consumed_keys,
            consumed_bytes,
            cost: 0,
            token: Some(token),
            nbest_index: None,
            span_start: 0,
            zero,
        }
    }

    /// Records where this candidate's span starts (see [`Self::span_start`]).
    pub(crate) const fn set_span_start(&mut self, start: usize) {
        self.span_start = start;
    }

    /// Where this candidate's span starts, in the coordinates of the
    /// window it came from: 0 for a window anchored at a lookup offset
    /// (its rows are measured from that offset) and the span's absolute
    /// start for a before-cursor window, whose rows all END at the lookup
    /// offset ([`Session::candidates_ending_at`]) but begin wherever each
    /// matching key path starts. Upstream's `m_begin`: the choose writes
    /// its constraint on `[m_begin, m_end)` and, for a before-cursor row,
    /// answers `m_begin` as the new cursor (`zhuyin.cpp:1656-1660` at the
    /// pin).
    ///
    /// [`Session::candidates_ending_at`]: crate::Session::candidates_ending_at
    #[must_use]
    pub const fn span_start(&self) -> usize {
        self.span_start
    }

    /// The text this candidate would insert: the string the pin hands out for
    /// it, which ends at the phrase's first U+0000
    /// ([`oxpinyin_core::phrase_c_string`]).
    #[must_use]
    pub fn text(&self) -> &str {
        if self.zero {
            phrase_c_string(&self.text)
        } else {
            &self.text
        }
    }

    /// The phrase as stored, one character per `ucs4_t` — a U+0000 and what
    /// follows it included. Its character count is the item's
    /// `get_phrase_length()`, which the pin reads from the header and not
    /// from the string ([`Self::text`]).
    pub(crate) fn phrase_text(&self) -> &str {
        &self.text
    }

    /// The phrase's stored length in characters: the sort key's `m_phrase_length`
    /// (`pinyin.cpp:1953-1993`).
    pub(crate) fn phrase_length(&self) -> usize {
        self.text.chars().count()
    }

    /// Where the candidate came from.
    #[must_use]
    pub const fn kind(&self) -> CandidateKind {
        self.kind
    }

    /// How many pinyin keys the candidate absorbs.
    #[must_use]
    pub const fn consumed_keys(&self) -> usize {
        self.consumed_keys
    }

    /// How many bytes of the raw input the candidate absorbs.
    #[must_use]
    pub const fn consumed_bytes(&self) -> usize {
        self.consumed_bytes
    }

    /// The scoring token behind this candidate, when it is one phrase.
    ///
    /// A shell never needs it; the session uses it as the bigram history for
    /// whatever the user types next. A sentence built from several phrases
    /// has no single token.
    #[must_use]
    pub const fn token(&self) -> Option<PhraseToken> {
        self.token
    }

    /// The decoder cost that ranked this candidate.
    ///
    /// Lower is better. The scale belongs to
    /// `docs/findings/scoring-spec.md`; a shell should treat it as opaque and
    /// use the list order.
    #[must_use]
    pub const fn cost(&self) -> Cost {
        self.cost
    }

    /// The n-best tail rank when this candidate is a sentence row
    /// (`pinyin_get_candidate_nbest_index`); `0` otherwise.
    ///
    /// Row ranks are the original tail order: a row whose string repeats an
    /// earlier row's keeps its own rank, because upstream zombies the
    /// duplicate without renumbering.
    #[must_use]
    pub const fn nbest_index(&self) -> u8 {
        match self.nbest_index {
            Some(rank) => rank,
            None => 0,
        }
    }

    /// The tail rank of the n-best row this candidate is, or `None` when it
    /// is not a row.
    ///
    /// The selection record indexes `Session`'s n-best rows by this rank —
    /// never by list position, which the NBEST-wins dedup can shift.
    #[must_use]
    pub const fn nbest_row(&self) -> Option<u8> {
        self.nbest_index
    }
}

/// An ordered, deterministic candidate list.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CandidateList {
    items: Vec<Candidate>,
    unreadable_item: bool,
}

impl CandidateList {
    pub(crate) const fn from_vec(items: Vec<Candidate>) -> Self {
        Self {
            items,
            unreadable_item: false,
        }
    }

    pub(crate) const fn with_unreadable_item(mut self, unreadable_item: bool) -> Self {
        self.unreadable_item = unreadable_item;
        self
    }

    /// Whether the lookups that built this list met a token of a loaded
    /// library whose item cannot be read. The pin's candidate listing dies
    /// there (`pinyin.cpp:1635-1637`, `:2053`); the entry is left out of the
    /// list and the C ABIs fail the call.
    #[must_use]
    pub const fn had_unreadable_item(&self) -> bool {
        self.unreadable_item
    }

    /// Swap the live list with `items`, returning the previous buffer so
    /// the session can keep its capacity across keystrokes.
    pub(crate) const fn swap_items(&mut self, items: &mut Vec<Candidate>) {
        core::mem::swap(&mut self.items, items);
    }

    /// Number of candidates.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no candidates.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The candidate at `index`, or `None` when out of range.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Candidate> {
        self.items.get(index)
    }

    /// Iterates the candidates in rank order.
    pub fn iter(&self) -> slice::Iter<'_, Candidate> {
        self.items.iter()
    }
}

impl<'a> IntoIterator for &'a CandidateList {
    type IntoIter = slice::Iter<'a, Candidate>;
    type Item = &'a Candidate;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

#[cfg(test)]
mod tests {
    use oxpinyin_core::{PhraseEntry, PhraseToken};

    use super::{Candidate, CandidateKind, CandidateList};

    fn list() -> CandidateList {
        CandidateList::from_vec(vec![
            Candidate::new(
                "\u{4f60}\u{597d}".to_owned(),
                CandidateKind::Sentence,
                2,
                5,
                -12,
                None,
                None,
            ),
            Candidate::new(
                "\u{4f60}".to_owned(),
                CandidateKind::Phrase,
                1,
                2,
                -7,
                Some(PhraseToken::new(3)),
                None,
            ),
        ])
    }

    #[test]
    fn a_zero_ends_the_string_and_not_the_stored_length() {
        // The pin's candidate string is `g_ucs4_to_utf8` of the phrase, which
        // stops at the first zero code point; the sort key reads the length
        // from the item header (`pinyin.cpp:1953-1993`).
        for (stored, string, length) in [
            ("\u{59d4}\0\u{4f1a}", "\u{59d4}", 3),
            ("\0\u{52a1}\u{9662}", "", 3),
            ("\u{529e}\u{516c}\0", "\u{529e}\u{516c}", 3),
            ("\u{4f60}\u{597d}", "\u{4f60}\u{597d}", 2),
            ("", "", 0),
        ] {
            let candidate = Candidate::new(
                stored.to_owned(),
                CandidateKind::Phrase,
                length,
                length,
                0,
                Some(PhraseToken::new(1)),
                None,
            );
            assert_eq!(candidate.text(), string, "{stored:?}");
            assert_eq!(candidate.phrase_text(), stored, "{stored:?}");
            assert_eq!(candidate.phrase_length(), length, "{stored:?}");
            // A dictionary hit takes the decoder's answer instead of
            // looking for a zero, and reads the same.
            let zero = stored.contains('\0');
            let hit = Candidate::from_entry(
                PhraseEntry::decoded(PhraseToken::new(1), stored, zero),
                CandidateKind::Phrase,
                length,
                length,
            );
            assert_eq!(hit, candidate, "{stored:?}");
            assert_eq!(hit.text(), string, "{stored:?}");
        }
    }

    #[test]
    fn indexing_past_the_end_is_none_not_a_panic() {
        let candidates = list();
        assert_eq!(candidates.len(), 2);
        assert!(!candidates.is_empty());
        assert!(candidates.get(0).is_some());
        assert!(candidates.get(2).is_none());
        assert!(candidates.get(usize::MAX).is_none());
    }

    #[test]
    fn iteration_preserves_rank_order() {
        let candidates = list();
        let texts: Vec<&str> = candidates.iter().map(Candidate::text).collect();
        assert_eq!(texts, ["\u{4f60}\u{597d}", "\u{4f60}"]);

        let by_ref: Vec<&str> = (&candidates).into_iter().map(Candidate::text).collect();
        assert_eq!(by_ref, texts);
    }

    #[test]
    fn candidates_report_what_they_absorb() {
        let candidates = list();
        let first = candidates.get(0).expect("first candidate");
        assert_eq!(first.kind(), CandidateKind::Sentence);
        assert_eq!(first.consumed_keys(), 2);
        assert_eq!(first.consumed_bytes(), 5);
        assert_eq!(first.cost(), -12);
        assert_eq!(first.token(), None, "a sentence has no single token");
        assert_eq!(
            candidates.get(1).expect("second").token(),
            Some(PhraseToken::new(3))
        );
        assert!(CandidateList::default().is_empty());
    }
}
