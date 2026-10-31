//! Candidates and checked iteration over them.
//!
//! Every index is checked. `docs/findings/robustness-evidence.md` row F-E-02
//! is an upstream invalid access through a candidate index that outlived the
//! list it came from; here a stale index is an `Option` or an `Err`, and the
//! session stays usable.

use core::slice;

use oxpinyin_core::{Cost, PhraseText, PhraseToken};

/// Stably rank rows by descending phrase length and pinyin span, then by
/// libpinyin's wrapping frequency comparison (`pinyin.cpp:1678-1709` at
/// 074a2219). `key` returns those three stored fields and must be pure.
///
/// Frequency is compared only inside equal length/span groups. Each group
/// is guarded before sorting: a consistent group uses the pin's defined
/// `rhs.wrapping_sub(lhs) as i32`; a cyclic group falls back to stable
/// numeric descending frequency. No inconsistent comparator reaches std.
/// The two stable sorts retain original order among equal complete keys.
/// Worst-case time is O(n log n), auxiliary space O(n), and guarding O(n).
pub fn rank_by_frequency<T>(items: &mut [T], key: impl Fn(&T) -> (usize, u8, u32)) {
    items.sort_by(|left, right| {
        let (left_length, left_span, _) = key(left);
        let (right_length, right_span, _) = key(right);
        (right_length, right_span).cmp(&(left_length, left_span))
    });
    for group in items.chunk_by_mut(|left, right| {
        let (left_length, left_span, _) = key(left);
        let (right_length, right_span, _) = key(right);
        (left_length, left_span) == (right_length, right_span)
    }) {
        let consistent = frequency_order_is_consistent(group.iter().map(|row| key(row).2));
        group.sort_by(|left, right| {
            let (left, right) = (key(left).2, key(right).2);
            if consistent {
                frequency_compare(left, right)
            } else {
                right.cmp(&left)
            }
        });
    }
}

/// This is libpinyin's product comparator, not a sort algorithm. Antipodal
/// values compare Less in both directions; callers must guard the list.
pub(crate) fn frequency_compare(left: u32, right: u32) -> std::cmp::Ordering {
    (right.wrapping_sub(left) as i32).cmp(&0)
}

/// A circular interval of width strictly below 2^31 is exactly the domain
/// where the wrapping comparison is consistent. In a consistent list the
/// tournament scan finds its maximum; checking that every value is less
/// than half a circle behind it proves the interval exists. An antipodal
/// pair fails the strict bound, as does any longer circular cycle.
/// Empty lists and ties pass. No allocation, unchecked access or arithmetic
/// overflow is possible; both scans are linear and use defined wrapping.
fn frequency_order_is_consistent(mut frequencies: impl Iterator<Item = u32> + Clone) -> bool {
    let Some(first) = frequencies.next() else {
        return true;
    };
    let mut maximum = first;
    for frequency in frequencies.clone() {
        if frequency_compare(frequency, maximum).is_lt() {
            maximum = frequency;
        }
    }
    std::iter::once(first)
        .chain(frequencies)
        .all(|frequency| maximum.wrapping_sub(frequency) < (1 << 31))
}

/// Where a candidate came from.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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
///
/// Sized to need (48 bytes): the three span counts are `u16` because an
/// input is at most [`MAX_INPUT_BYTES`](crate::MAX_INPUT_BYTES) bytes
/// long and a key is at least one byte, and the n-best rank is a byte
/// with `NO_ROW` for "not a row", the pin's own `gint8 m_nbest_index`
/// width (`pinyin.cpp:102` at 074a2219). A window of a hundred-odd rows
/// is retained per instance, so each byte here is a hundred-odd bytes of
/// instance heap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Candidate {
    text: PhraseText,
    cost: Cost,
    token: Option<PhraseToken>,
    consumed_keys: u16,
    consumed_bytes: u16,
    /// Tail rank of the n-best sentence row this candidate is (W14);
    /// `None` for every non-row candidate — including fallback sentence
    /// candidates, which share the row `CandidateKind::Sentence` but are
    /// not rows and carry no token path. The rank is the *original* tail
    /// order, so a row that survives the NBEST-wins dedup still knows
    /// which `NbestRow` is its own even when its list position shifted.
    /// [`NO_ROW`] stands for `None`.
    nbest_index: u8,
    /// Where the candidate's span starts, in the coordinates of the window
    /// it was built in — upstream's `lookup_candidate_t::m_begin`
    /// (`pinyin.h`/`zhuyin.h`; set beside `m_end` at `zhuyin.cpp:1509,1595`
    /// at the pin). A window anchored at a lookup offset measures every
    /// span from that offset, so its rows carry 0; the before-cursor window
    /// (`Session::candidates_ending_at`) runs on the absolute prefix graph,
    /// so its rows carry each span's absolute start. Sentence rows and
    /// fallbacks carry 0.
    span_start: u16,
    kind: CandidateKind,
}

/// The `nbest_index` value of a candidate that is not an n-best row.
const NO_ROW: u8 = u8::MAX;

// The row width the doc above claims; a field that grows it shows up here.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<Candidate>() == 48);

/// Narrows a byte or key count to the row's width. Every count the
/// session produces is below [`MAX_INPUT_BYTES`](crate::MAX_INPUT_BYTES)
/// (32,767), so the saturation never fires; it stands in for a panic.
const fn narrow(count: usize) -> u16 {
    if count > u16::MAX as usize {
        u16::MAX
    } else {
        count as u16
    }
}

impl Candidate {
    pub(crate) fn new(
        text: impl Into<PhraseText>,
        kind: CandidateKind,
        consumed_keys: usize,
        consumed_bytes: usize,
        cost: Cost,
        token: Option<PhraseToken>,
        nbest_index: Option<u8>,
    ) -> Self {
        debug_assert!(consumed_keys <= u16::MAX as usize && consumed_bytes <= u16::MAX as usize);
        debug_assert!(nbest_index.is_none_or(|rank| rank != NO_ROW));
        Self {
            text: text.into(),
            cost,
            token,
            consumed_keys: narrow(consumed_keys),
            consumed_bytes: narrow(consumed_bytes),
            nbest_index: match nbest_index {
                Some(rank) if rank != NO_ROW => rank,
                _ => NO_ROW,
            },
            span_start: 0,
            kind,
        }
    }

    /// Records where this candidate's span starts (see [`Self::span_start`]).
    pub(crate) const fn set_span_start(&mut self, start: usize) {
        debug_assert!(start <= u16::MAX as usize);
        self.span_start = narrow(start);
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
        self.span_start as usize
    }

    /// The text this candidate would insert.
    #[must_use]
    pub fn text(&self) -> &PhraseText {
        &self.text
    }

    /// The text as `str`, when every stored word was a scalar value.
    #[must_use]
    pub fn text_str(&self) -> Option<&str> {
        self.text.as_str()
    }

    /// The item's stored character count, which ranking uses
    /// (`_compute_phrase_length` at the pin): a zero or a non-scalar word
    /// leaves it unchanged.
    #[must_use]
    pub fn phrase_length(&self) -> usize {
        self.text.phrase_length()
    }

    /// Where the candidate came from.
    #[must_use]
    pub const fn kind(&self) -> CandidateKind {
        self.kind
    }

    /// How many pinyin keys the candidate absorbs.
    #[must_use]
    pub const fn consumed_keys(&self) -> usize {
        self.consumed_keys as usize
    }

    /// How many bytes of the raw input the candidate absorbs.
    #[must_use]
    pub const fn consumed_bytes(&self) -> usize {
        self.consumed_bytes as usize
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

    /// The pre-frequency fallback cost; real-table sentence rows carry zero.
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
        if self.nbest_index == NO_ROW {
            0
        } else {
            self.nbest_index
        }
    }

    /// The tail rank of the n-best row this candidate is, or `None` when it
    /// is not a row.
    ///
    /// The selection record indexes `Session`'s n-best rows by this rank —
    /// never by list position, which the NBEST-wins dedup can shift.
    #[must_use]
    pub const fn nbest_row(&self) -> Option<u8> {
        if self.nbest_index == NO_ROW {
            None
        } else {
            Some(self.nbest_index)
        }
    }
}

/// An ordered, deterministic candidate list.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CandidateList {
    items: Vec<Candidate>,
    unreadable_item: bool,
    ranked_phrase_row: bool,
}

impl CandidateList {
    pub(crate) const fn from_vec(items: Vec<Candidate>) -> Self {
        Self {
            items,
            unreadable_item: false,
            ranked_phrase_row: false,
        }
    }

    pub(crate) const fn with_unreadable_item(mut self, unreadable_item: bool) -> Self {
        self.unreadable_item = unreadable_item;
        self
    }

    pub(crate) const fn with_ranked_phrase_row(mut self, ranked_phrase_row: bool) -> Self {
        self.ranked_phrase_row = ranked_phrase_row;
        self
    }

    /// Whether the scan that built this list searched a row the pin scores
    /// against the facade's unigram total, before its prepends and its
    /// phrase-string dedup: the rows `_compute_frequency_of_items` runs on
    /// (`pinyin.cpp:2283`, `zhuyin.cpp:1526`, `:1607`), less the addon rows
    /// it scores against the addon total (`pinyin.cpp:1828-1840`). With that
    /// total at zero, the pin's `assert (0 < total_freq)` dies on such a row
    /// (`pinyin.cpp:1859`, `zhuyin.cpp:1261`, register row 61), even when the
    /// dedup later removes it from the list. A window of prepended or
    /// fallback rows alone has none.
    #[must_use]
    pub const fn ranked_phrase_row(&self) -> bool {
        self.ranked_phrase_row
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
    use oxpinyin_core::PhraseToken;

    use super::{Candidate, CandidateKind, CandidateList};

    #[test]
    fn frequency_guard_agrees_with_the_order_laws_at_wrap_boundaries() {
        use super::{frequency_compare, frequency_order_is_consistent};
        let values = [
            0,
            1,
            0x6000_0000,
            0x7fff_ffff,
            0x8000_0000,
            0x8000_0001,
            0xc000_0000,
            u32::MAX,
        ];
        for a in values {
            for b in values {
                for c in values {
                    let rows = [a, b, c];
                    // Independent definition of a consistent comparison:
                    // antisymmetry and transitivity on this complete list.
                    let consistent = rows.iter().all(|&x| {
                        rows.iter().all(|&y| {
                            frequency_compare(x, y) == frequency_compare(y, x).reverse()
                                && rows.iter().all(|&z| {
                                    !(frequency_compare(x, y).is_le()
                                        && frequency_compare(y, z).is_le())
                                        || frequency_compare(x, z).is_le()
                                })
                        })
                    });
                    assert_eq!(
                        frequency_order_is_consistent(rows.into_iter()),
                        consistent,
                        "{rows:?}"
                    );
                }
            }
        }
        assert!(frequency_order_is_consistent([].into_iter()));
        assert!(frequency_order_is_consistent([7].into_iter()));
        assert!(frequency_order_is_consistent([7, 7, 7].into_iter()));
        assert!(!frequency_order_is_consistent([0, 1 << 31].into_iter()));
        assert!(!frequency_order_is_consistent(
            [0, 0x6000_0000, 0xc000_0000].into_iter()
        ));
        assert!(frequency_order_is_consistent([u32::MAX, 0, 1].into_iter()));
        assert!(frequency_order_is_consistent([0, 0x7fff_ffff].into_iter()));
    }

    #[test]
    fn frequency_sort_guards_each_leading_key_group_and_preserves_ties() {
        // The first group cycles and falls back numerically. The second
        // crosses zero consistently and must keep wrapping order, despite
        // the cycle elsewhere. Different leading fields never compare freq.
        let mut rows = vec![
            (2, 1, 0, 'a'),
            (1, 1, u32::MAX, 'b'),
            (2, 1, 0x6000_0000, 'c'),
            (1, 1, 1, 'd'),
            (2, 1, 0xc000_0000, 'e'),
            (1, 1, 1, 'f'),
            (1, 2, 1 << 31, 'g'),
            (2, 0, 0, 'h'),
        ];
        super::rank_by_frequency(&mut rows, |&(length, span, freq, _)| (length, span, freq));
        assert_eq!(
            rows.iter().map(|row| row.3).collect::<Vec<_>>(),
            ['e', 'c', 'a', 'h', 'g', 'd', 'f', 'b']
        );
    }

    #[test]
    fn frequency_sort_has_a_defined_antipodal_fallback() {
        let mut rows = [(1, 0), (0, 1 << 31), (2, 0), (3, 1 << 31)];
        super::rank_by_frequency(&mut rows, |&(_, freq)| (0, 0, freq));
        assert_eq!(rows, [(0, 1 << 31), (3, 1 << 31), (1, 0), (2, 0)]);
        super::rank_by_frequency::<(usize, u32)>(&mut [], |&(_, freq)| (0, 0, freq));
    }

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
        let texts: Vec<&str> = candidates.iter().filter_map(Candidate::text_str).collect();
        assert_eq!(texts, ["\u{4f60}\u{597d}", "\u{4f60}"]);

        let by_ref: Vec<&str> = (&candidates)
            .into_iter()
            .filter_map(Candidate::text_str)
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
