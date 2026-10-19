//! Candidates and checked iteration over them.
//!
//! Every index is checked. `docs/findings/robustness-evidence.md` row F-E-02
//! is an upstream invalid access through a candidate index that outlived the
//! list it came from; here a stale index is an `Option` or an `Err`, and the
//! session stays usable.

use core::slice;

use oxpinyin_core::{Cost, PhraseEntry, PhraseString, PhraseToken};

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
    /// The string the pin hands out for the candidate: for a phrase it ends at
    /// the phrase's first zero word, it is bytes that are not UTF-8 for a word
    /// that is no scalar value, and it is NULL for a word of 0x80000000 or
    /// more (`g_ucs4_to_utf8`, `glib/gutf8.c:1023-1070`).
    string: PhraseString,
    /// The phrase's length in words: the item header's `get_phrase_length()`,
    /// which the pin reads from the header and not from the string. For a
    /// sentence or the raw input it is the length of the text in characters.
    len: u32,
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
}

impl Candidate {
    /// A candidate that is not a dictionary hit: a sentence, the raw input,
    /// or a text the caller made. Its length is the text's characters.
    pub(crate) fn new(
        string: impl Into<PhraseString>,
        kind: CandidateKind,
        consumed_keys: usize,
        consumed_bytes: usize,
        cost: Cost,
        token: Option<PhraseToken>,
        nbest_index: Option<u8>,
    ) -> Self {
        let string = string.into();
        let len = string.as_str().map_or(0, |text| {
            text.chars().count().try_into().unwrap_or(u32::MAX)
        });
        Self {
            string,
            len,
            kind,
            consumed_keys,
            consumed_bytes,
            cost,
            token,
            nbest_index,
            span_start: 0,
        }
    }

    /// A dictionary hit as a candidate: the string and the length the
    /// dictionary decoder made of the item.
    pub(crate) fn from_entry(
        entry: PhraseEntry,
        kind: CandidateKind,
        consumed_keys: usize,
        consumed_bytes: usize,
    ) -> Self {
        let token = entry.token();
        let len = entry.len().try_into().unwrap_or(u32::MAX);
        Self {
            string: entry.into_string(),
            len,
            kind,
            consumed_keys,
            consumed_bytes,
            cost: 0,
            token: Some(token),
            nbest_index: None,
            span_start: 0,
        }
    }

    /// This candidate with the decoder cost that ranked it.
    pub(crate) const fn with_cost(mut self, cost: Cost) -> Self {
        self.cost = cost;
        self
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

    /// The string this candidate would insert: the one the pin hands out for
    /// it ([`PhraseString`]). It is NULL where the pin's conversion answers
    /// NULL, and then the pin hands out the NULL as the candidate's string.
    #[must_use]
    pub const fn string(&self) -> &PhraseString {
        &self.string
    }

    /// [`Self::string`] as text, for a test that reads a candidate made of
    /// text.
    #[cfg(test)]
    pub(crate) fn text(&self) -> &str {
        self.string.as_str().expect("a candidate made of text")
    }

    /// The phrase's length in words: the sort key's `m_phrase_length`
    /// (`pinyin.cpp:1953-1993`), which is the item's `get_phrase_length()`
    /// and not the length of the string.
    pub(crate) const fn phrase_length(&self) -> usize {
        self.len as usize
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

    /// Whether the pin's string comparison dies on this list: it sorts and
    /// compares the candidates' strings with `strcmp` (`pinyin.cpp:2053`,
    /// `:2082`), which reads through a NULL string once there is a second
    /// candidate to compare it with. The C ABIs answer such a list as the
    /// pin's stop (class (b)): a failed call, with a warning.
    #[must_use]
    pub fn strcmp_would_die(&self) -> bool {
        self.items.len() >= 2
            && self
                .items
                .iter()
                .any(|candidate| candidate.string().is_null())
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
    use oxpinyin_core::{PhraseEntry, PhraseString, PhraseToken, PhraseWords};

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
    fn a_hit_carries_the_pins_string_and_the_headers_length() {
        // The pin's candidate string is `g_ucs4_to_utf8` of the phrase; the
        // sort key reads the length from the item header
        // (`pinyin.cpp:1953-1993`).
        let cases: [(&[u32], Option<&[u8]>); 8] = [
            (&[0x59d4, 0, 0x4f1a], Some("\u{59d4}".as_bytes())),
            (&[0, 0x52a1, 0x9662], Some(b"")),
            (&[0x529e, 0x516c, 0], Some("\u{529e}\u{516c}".as_bytes())),
            (&[0x4f60, 0x597d], Some("\u{4f60}\u{597d}".as_bytes())),
            (&[], Some(b"")),
            // A surrogate is written as the three bytes `g_unichar_to_utf8`
            // gives it, a value above U+10FFFF as four to six.
            (
                &[0x51fa, 0xd800, 0x793e],
                Some(b"\xe5\x87\xba\xed\xa0\x80\xe7\xa4\xbe"),
            ),
            (&[0x535a, 0x11_0000], Some(b"\xe5\x8d\x9a\xf4\x90\x80\x80")),
            // A word of 0x80000000 or more before a zero makes the string NULL.
            (&[0x8ba1, 0x8000_0000, 0x673a], None),
        ];
        for (words, string) in cases {
            // The length is the header's, so it counts the zero and what
            // follows it.
            let length = words.len();
            let phrase = PhraseWords::from_words(words.iter().copied());
            let hit = Candidate::from_entry(
                PhraseEntry::from_words(PhraseToken::new(1), &phrase),
                CandidateKind::Phrase,
                length,
                length,
            );
            assert_eq!(hit.string().as_bytes(), string, "{words:x?}");
            assert_eq!(hit.phrase_length(), length, "{words:x?}");
        }
    }

    #[test]
    fn a_candidate_made_from_text_counts_its_characters() {
        let candidate = Candidate::new(
            "\u{4f60}\u{597d}",
            CandidateKind::Sentence,
            2,
            6,
            0,
            None,
            None,
        );
        assert_eq!(candidate.phrase_length(), 2);
        assert_eq!(*candidate.string(), "\u{4f60}\u{597d}");
        // A string that is not text has no characters to count.
        let null = Candidate::new(
            PhraseString::NULL,
            CandidateKind::Sentence,
            0,
            0,
            0,
            None,
            None,
        );
        assert!(null.string().is_null());
        assert_eq!(null.phrase_length(), 0);
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
        let texts: Vec<&str> = candidates
            .iter()
            .map(|candidate| candidate.string().as_str().unwrap_or_default())
            .collect();
        assert_eq!(texts, ["\u{4f60}\u{597d}", "\u{4f60}"]);

        let by_ref: Vec<&str> = (&candidates)
            .into_iter()
            .map(|candidate| candidate.string().as_str().unwrap_or_default())
            .collect();
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
