//! Candidates and checked iteration over them.
//!
//! Every index is checked. `docs/findings/robustness-evidence.md` row F-E-02
//! is an upstream invalid access through a candidate index that outlived the
//! list it came from; here a stale index is an `Option` or an `Err`, and the
//! session stays usable.

use core::slice;

use compact_str::CompactString;
use oxpinyin_core::{Cost, PhraseToken};

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
        Self {
            text: text.into(),
            kind,
            consumed_keys,
            consumed_bytes,
            cost,
            token,
            nbest_index,
        }
    }

    /// The text this candidate would insert.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
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
}

impl CandidateList {
    #[allow(dead_code)] // used by the in-crate tests
    pub(crate) const fn from_vec(items: Vec<Candidate>) -> Self {
        Self { items }
    }

    /// Swap the live list with `items`, returning the previous buffer so
    /// the session can keep its capacity across keystrokes.
    pub(crate) fn swap_items(&mut self, items: &mut Vec<Candidate>) {
        core::mem::swap(&mut self.items, items);
    }

    /// Number of candidates.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no candidates.
    #[must_use]
    pub fn is_empty(&self) -> bool {
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
    use oxpinyin_core::PhraseToken;

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
