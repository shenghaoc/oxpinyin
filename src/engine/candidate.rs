//! Candidates and checked iteration over them.
//!
//! A list hands out its rows by iteration only; the session selects a row
//! it is given, never an index, so a candidate that outlived its list is a
//! C ABI case: a stale `lookup_candidate_t` pointer, which the snapshot
//! resolves by address.

use core::slice;

use crate::vocab::{PhraseText, PhraseToken};

// The comparator subtracts the stored frequency fields, pinyin.cpp:1678–1709. The
// compatibility policy uses stable descending numeric frequency for a cyclic group.
/// Stably ranks candidates by descending phrase length and pinyin span, then by frequency
/// within each equal-length, equal-span group. `key` must be pure. Equal keys retain their
/// original order. For the wrapping frequency comparison to define an order, each such
/// group must have consistent pairwise comparisons. Sorting takes O(n log n) time and
/// O(n) auxiliary space.
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Candidate {
    text: PhraseText,
    token: Option<PhraseToken>,
    consumed_bytes: u32,
    /// Tail rank of the n-best sentence row this candidate represents, or
    /// `None` for a non-row candidate. The rank is the *original* tail
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
    span_start: u32,
    kind: CandidateKind,
}

// Keep the row at 48 bytes: an instance retains a window of a hundred-odd
// rows, so each byte here is a hundred-odd bytes of instance heap. The byte
// count and the span start are `u32` (`MAX_GRAPH_POSITION`), and the n-best
// rank is the pin's `gint8 m_nbest_index` width (`pinyin.cpp:102` at
// 074a2219) as an `Option<u8>`. A field that grows the row fails here.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<Candidate>() == 48);

/// Narrows a byte or key count to the row's width, saturating an oversized raw suffix.
const fn narrow(count: usize) -> u32 {
    if count > u32::MAX as usize {
        u32::MAX
    } else {
        count as u32
    }
}

impl Candidate {
    pub(crate) fn new(
        text: impl Into<PhraseText>,
        kind: CandidateKind,
        consumed_bytes: usize,
        token: Option<PhraseToken>,
        nbest_index: Option<u8>,
    ) -> Self {
        Self {
            text: text.into(),
            token,
            consumed_bytes: narrow(consumed_bytes),
            nbest_index,
            span_start: 0,
            kind,
        }
    }

    /// Records where this candidate's span starts (see [`Self::span_start`]).
    pub(crate) const fn set_span_start(&mut self, start: usize) {
        self.span_start = narrow(start);
    }

    /// Where this candidate's span starts, in the coordinates of the
    /// window it came from: 0 for a window anchored at a lookup offset
    /// (its rows are measured from that offset) and the span's absolute
    /// start for a before-cursor window, whose rows all END at the lookup
    /// offset but begin wherever each
    /// matching key path starts. Upstream's `m_begin`: the choose writes
    /// its constraint on `[m_begin, m_end)` and, for a before-cursor row,
    /// answers `m_begin` as the new cursor (`zhuyin.cpp:1656-1660` at the
    /// pin).
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
    #[cfg(test)]
    #[must_use]
    pub(crate) fn text_str(&self) -> Option<&str> {
        self.text.as_str()
    }

    /// The item's stored character count, which ranking uses
    /// (`_compute_phrase_length` at the pin): a zero or a non-scalar word
    /// leaves it unchanged.
    #[must_use]
    pub(crate) fn phrase_length(&self) -> usize {
        self.text.phrase_length()
    }

    /// Where the candidate came from.
    #[must_use]
    pub const fn kind(&self) -> CandidateKind {
        self.kind
    }

    /// How many bytes of the raw input the candidate absorbs.
    #[must_use]
    pub const fn consumed_bytes(&self) -> usize {
        self.consumed_bytes as usize
    }

    /// The scoring token behind this candidate, when it is one phrase.
    ///
    /// The session uses it as the bigram history for the next input. A
    /// sentence built from several phrases has no single token.
    #[must_use]
    pub const fn token(&self) -> Option<PhraseToken> {
        self.token
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
    pub(crate) const fn nbest_row(&self) -> Option<u8> {
        self.nbest_index
    }
}

/// An ordered, deterministic candidate list.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CandidateList {
    items: Vec<Candidate>,
    /// The unreadable items the building scan met, for the tests.
    #[cfg(test)]
    unreadable_count: usize,
}

impl CandidateList {
    pub(crate) const fn from_vec(items: Vec<Candidate>) -> Self {
        Self {
            items,
            #[cfg(test)]
            unreadable_count: 0,
        }
    }

    #[cfg(test)]
    pub(crate) const fn with_unreadable_count(mut self, unreadable_count: usize) -> Self {
        self.unreadable_count = unreadable_count;
        self
    }

    /// Whether the lookups that built this list met a token of a loaded
    /// library whose item cannot be read. The readable candidates remain
    /// available separately from that search metadata.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn had_unreadable_item(&self) -> bool {
        self.unreadable_count != 0
    }

    /// The rows, for their storage to be reused.
    pub(crate) fn into_items(self) -> Vec<Candidate> {
        self.items
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
mod tests;
