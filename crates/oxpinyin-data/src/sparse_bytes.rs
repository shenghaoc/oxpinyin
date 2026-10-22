//! A sparse map from byte positions to byte values, kept by 4 KiB page: one
//! bit per position and the values of the set bits in position order.
//!
//! A sealed in-place journal replay keeps here the original value of each
//! byte it changed in a mapped chunk
//! ([`crate::phrase_library::PhraseLibrary`]), so the library still reads as
//! loaded; the seal records them in one ascending pass ([`Ascending`]). A
//! replay over a library another handle shares keeps here the bytes it
//! writes ([`SparseBytes::set`]). Only changed bytes are kept: a journal
//! rewrites whole items, but a frequency update changes two or three of
//! their bytes.
//!
//! A read finds its page record once ([`SparseBytes::span`]) and answers
//! from it.

/// Positions per page record.
const PAGE: usize = 4096;
/// Bit words per page record.
const WORDS: usize = PAGE / 64;

/// Byte values by position (see the module docs). A lookup takes at most
/// one population count: the shipping targets' baseline has no instruction
/// for it, so each is a dozen instructions.
#[derive(Clone, Debug, Default)]
pub struct SparseBytes {
    /// By page number: one more than the index of the page's record in
    /// [`Self::pages`], or zero for a page with none.
    slots: Vec<u32>,
    /// The pages holding a recorded byte, by page number.
    pages: Vec<Page>,
}

/// One page's recorded bytes.
#[derive(Clone, Debug)]
struct Page {
    /// The page number: position / [`PAGE`].
    number: usize,
    /// Bit `n` is set when page offset `n` holds a recorded value.
    bits: [u64; WORDS],
    /// The set bits before each word of [`Self::bits`].
    ranks: [u16; WORDS],
    /// One past the highest recorded offset: a record there or later appends.
    high: usize,
    /// The recorded values, in offset order.
    values: Vec<u8>,
}

/// The bits below `count` of a word, `count <= 64`.
const fn low_bits(count: usize) -> u64 {
    if count >= 64 {
        u64::MAX
    } else {
        (1_u64 << count) - 1
    }
}

/// The bits `low..high` of a word, `low < high <= 64`.
const fn span_mask(low: usize, high: usize) -> u64 {
    let below_high = if high >= 64 {
        u64::MAX
    } else {
        (1_u64 << high) - 1
    };
    below_high & !((1_u64 << low) - 1)
}

impl Page {
    const fn new(number: usize) -> Self {
        Self {
            number,
            bits: [0; WORDS],
            ranks: [0; WORDS],
            high: 0,
            values: Vec::new(),
        }
    }

    fn holds(&self, offset: usize) -> bool {
        self.bits[offset / 64] >> (offset % 64) & 1 == 1
    }

    /// The set bits before page offset `offset`.
    #[inline(always)]
    fn rank(&self, offset: usize) -> usize {
        let word = offset / 64;
        let below = self.bits[word] & ((1_u64 << (offset % 64)) - 1);
        usize::from(self.ranks[word]) + below.count_ones() as usize
    }

    fn get(&self, offset: usize) -> Option<u8> {
        if !self.holds(offset) {
            return None;
        }
        self.values.get(self.rank(offset)).copied()
    }

    /// The bits of page offsets `offset..offset + count`, `count <= 64`,
    /// from bit zero.
    #[inline(always)]
    fn bits_at(&self, offset: usize, count: usize) -> u64 {
        let (word, shift) = (offset / 64, offset % 64);
        let low = self.bits.get(word).map_or(0, |bits| bits >> shift);
        let high = match shift {
            0 => 0,
            _ => self
                .bits
                .get(word + 1)
                .map_or(0, |bits| bits << (64 - shift)),
        };
        (low | high) & low_bits(count)
    }

    /// Whether a bit in page offsets `start..end` is set.
    #[inline(always)]
    fn any(&self, start: usize, end: usize) -> bool {
        let (first, last) = (start / 64, (end - 1) / 64);
        if first == last {
            return self.bits[first] & span_mask(start % 64, (end - 1) % 64 + 1) != 0;
        }
        (first..=last).any(|word| {
            let low = (word * 64).max(start) - word * 64;
            let high = ((word + 1) * 64).min(end) - word * 64;
            self.bits[word] & span_mask(low, high) != 0
        })
    }

    /// Writes this page's values over `bytes`, which hold page offsets
    /// `start..start + bytes.len()`.
    #[inline(always)]
    fn overlay(&self, start: usize, bytes: &mut [u8]) {
        if bytes.len() <= 64 {
            // A field's bytes: one bit extraction and one rank.
            let mut bits = self.bits_at(start, bytes.len());
            if bits == 0 {
                return;
            }
            let mut index = self.rank(start + bits.trailing_zeros() as usize);
            while bits != 0 {
                let at = bits.trailing_zeros() as usize;
                if let (Some(byte), Some(&value)) = (bytes.get_mut(at), self.values.get(index)) {
                    *byte = value;
                }
                index += 1;
                bits &= bits - 1;
            }
            return;
        }
        let end = start + bytes.len();
        let mut rank = None;
        for word in start / 64..end.div_ceil(64) {
            let low = (word * 64).max(start) - word * 64;
            let high = ((word + 1) * 64).min(end) - word * 64;
            let mut bits = self.bits[word] & span_mask(low, high);
            while bits != 0 {
                let offset = word * 64 + bits.trailing_zeros() as usize;
                let index = rank.unwrap_or_else(|| self.rank(offset));
                if let (Some(byte), Some(&value)) =
                    (bytes.get_mut(offset - start), self.values.get(index))
                {
                    *byte = value;
                }
                rank = Some(index + 1);
                bits &= bits - 1;
            }
        }
    }

    fn insert(&mut self, offset: usize, value: u8) {
        let word = offset / 64;
        if offset >= self.high {
            // Past every record: append.
            self.values.push(value);
            self.high = offset + 1;
        } else {
            let rank = self.rank(offset);
            if self.holds(offset) {
                if let Some(slot) = self.values.get_mut(rank) {
                    *slot = value;
                }
                return;
            }
            self.values.insert(rank, value);
        }
        self.bits[word] |= 1 << (offset % 64);
        if let Some(later) = self.ranks.get_mut(word + 1..) {
            for rank in later {
                *rank += 1;
            }
        }
    }

    /// Counts [`Self::ranks`] from the bits.
    fn count_ranks(&mut self) {
        let mut before = 0_u16;
        for (bits, rank) in self.bits.iter().zip(&mut self.ranks) {
            *rank = before;
            before += bits.count_ones() as u16;
        }
    }
}

/// The recorded pages overlapping one span of positions: one lookup that
/// the span's reads share.
#[derive(Clone, Copy, Debug, Default)]
pub struct Span<'a> {
    pages: &'a [Page],
    /// The positions whose pages the lookup covered.
    covered: (usize, usize),
}

impl Span<'_> {
    /// Whether no page of the span has a record.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Whether positions `start..end` lie in the pages this span covers, so
    /// that it answers for them as a lookup of their own would.
    #[must_use]
    #[inline]
    pub const fn covers(&self, start: usize, end: usize) -> bool {
        self.covered.0 <= start && end <= self.covered.1
    }

    /// Whether a position in `start..end`, inside the span, has a record.
    #[must_use]
    #[inline(always)]
    pub fn touches(&self, start: usize, end: usize) -> bool {
        self.pages.iter().any(|page| {
            let base = page.number * PAGE;
            let (low, high) = (start.max(base), end.min(base + PAGE));
            low < high && page.any(low - base, high - base)
        })
    }

    /// Which of positions `start..start + count`, `count <= 64`, have a
    /// record: bit `n` for position `start + n`.
    #[must_use]
    #[inline(always)]
    pub fn bits(&self, start: usize, count: usize) -> u64 {
        let end = start.saturating_add(count.min(64));
        let mut bits = 0;
        for page in self.pages {
            let base = page.number * PAGE;
            let (low, high) = (start.max(base), end.min(base + PAGE));
            if low < high {
                bits |= page.bits_at(low - base, high - low) << (low - start);
            }
        }
        bits
    }

    /// The value recorded at `position`, inside the span.
    #[must_use]
    #[inline]
    pub fn get(&self, position: usize) -> Option<u8> {
        let number = position / PAGE;
        self.pages
            .iter()
            .find(|page| page.number == number)?
            .get(position % PAGE)
    }

    /// Writes the recorded values over `bytes`, which hold positions
    /// `start..start + bytes.len()` inside the span.
    #[inline(always)]
    pub fn overlay(&self, start: usize, bytes: &mut [u8]) {
        let end = start + bytes.len();
        for page in self.pages {
            let base = page.number * PAGE;
            let (low, high) = (start.max(base), end.min(base + PAGE));
            if low < high && page.any(low - base, high - base) {
                page.overlay(low - base, &mut bytes[low - start..high - start]);
            }
        }
    }
}

impl SparseBytes {
    /// No recorded bytes.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            pages: Vec::new(),
        }
    }

    /// Whether nothing is recorded.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// The index of page `number`'s record.
    #[inline(always)]
    fn index(&self, number: usize) -> Option<usize> {
        let slot = *self.slots.get(number)?;
        slot.checked_sub(1).map(|index| index as usize)
    }

    /// The recorded pages overlapping positions `start..end`.
    #[must_use]
    #[inline(always)]
    pub fn span(&self, start: usize, end: usize) -> Span<'_> {
        if start >= end {
            return Span::default();
        }
        let (first, last) = (start / PAGE, (end - 1) / PAGE);
        let covered = (first * PAGE, (last + 1).saturating_mul(PAGE));
        if self.pages.is_empty() {
            return Span {
                pages: &[],
                covered,
            };
        }
        let pages = if last - first <= 1 {
            // Two adjacent page numbers that both have records sit next to
            // each other in the record list.
            match (self.index(first), self.index(last)) {
                (Some(from), Some(to)) => self.pages.get(from..=to),
                (Some(at), None) | (None, Some(at)) => self.pages.get(at..=at),
                (None, None) => None,
            }
        } else {
            let from = self.pages.partition_point(|page| page.number < first);
            let to = self.pages.partition_point(|page| page.number <= last);
            self.pages.get(from..to)
        };
        Span {
            pages: pages.unwrap_or_default(),
            covered,
        }
    }

    /// Whether a page holding a position in `start..end` has a record: a
    /// lookup per page, without reading the records.
    #[must_use]
    #[inline(always)]
    pub fn pages_touch(&self, start: usize, end: usize) -> bool {
        start < end && (start / PAGE..=(end - 1) / PAGE).any(|number| self.index(number).is_some())
    }

    /// Whether a position in `start..end` has a recorded value.
    #[must_use]
    pub fn touches(&self, start: usize, end: usize) -> bool {
        self.span(start, end).touches(start, end)
    }

    /// The value recorded at `position`.
    #[must_use]
    pub fn get(&self, position: usize) -> Option<u8> {
        self.span(position, position.checked_add(1)?).get(position)
    }

    /// Writes the recorded values over `bytes`, which hold positions
    /// `start..start + bytes.len()`.
    pub fn overlay(&self, start: usize, bytes: &mut [u8]) {
        self.span(start, start + bytes.len()).overlay(start, bytes);
    }

    /// Records `value` at `position`, replacing a value recorded there.
    pub fn set(&mut self, position: usize, value: u8) {
        let number = position / PAGE;
        // A replay writes in token order, which is position order in every
        // chunk the pin writes, so the last page is the usual one.
        if let Some(last) = self.pages.last_mut()
            && last.number == number
        {
            last.insert(position % PAGE, value);
            return;
        }
        let index = match self.index(number) {
            Some(index) => index,
            None => self.add_page(number),
        };
        if let Some(page) = self.pages.get_mut(index) {
            page.insert(position % PAGE, value);
        }
    }

    /// Points page `number`'s slot at record `index`.
    fn point(&mut self, number: usize, index: usize) {
        if self.slots.len() <= number {
            self.slots.resize(number + 1, 0);
        }
        self.slots[number] = u32::try_from(index + 1).unwrap_or(0);
    }

    fn add_page(&mut self, number: usize) -> usize {
        let index = self.pages.partition_point(|page| page.number < number);
        self.pages.insert(index, Page::new(number));
        // The records after it moved up by one.
        for at in index..self.pages.len() {
            let moved = self.pages[at].number;
            self.point(moved, at);
        }
        index
    }

    /// Releases the spare capacity recording left behind.
    pub fn shrink_to_fit(&mut self) {
        self.slots.shrink_to_fit();
        self.pages.shrink_to_fit();
        for page in &mut self.pages {
            page.values.shrink_to_fit();
        }
    }

    /// The heap bytes this map holds, by capacity.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.slots.capacity() * size_of::<u32>()
            + self.pages.capacity() * size_of::<Page>()
            + self
                .pages
                .iter()
                .map(|page| page.values.capacity())
                .sum::<usize>()
    }
}

/// One bit per byte lane of `lanes`, set where the lane is non-zero.
const fn lane_mask(lanes: u64) -> u8 {
    // Fold each lane into its low bit, then gather the eight low bits into
    // the top byte: the multiplier's terms put lane `k` at bit `56 + k` and
    // no two terms share a bit, so nothing carries.
    let mut folded = lanes;
    folded |= folded >> 4;
    folded |= folded >> 2;
    folded |= folded >> 1;
    ((folded & 0x0101_0101_0101_0101).wrapping_mul(0x0102_0408_1020_4080) >> 56) as u8
}

/// Builds a [`SparseBytes`] from records in ascending position order, each
/// appended: the map [`SparseBytes::set`] would build from them, its ranks
/// counted once at the end.
#[derive(Debug, Default)]
pub struct Ascending {
    map: SparseBytes,
    /// The last recorded position.
    last: Option<usize>,
}

impl Ascending {
    /// A builder with room for `pages` page records below page number
    /// `end`: a caller that knows where its records lie builds without
    /// spare capacity.
    #[must_use]
    pub fn with_pages(pages: usize, end: usize) -> Self {
        Self {
            map: SparseBytes {
                slots: Vec::with_capacity(end),
                pages: Vec::with_capacity(pages),
            },
            last: None,
        }
    }

    /// Records `value` at `position`. A position not past the last recorded
    /// one is left out: the records come in ascending order.
    pub fn push(&mut self, position: usize, value: u8) {
        if self.last.is_some_and(|last| position <= last) {
            return;
        }
        self.last = Some(position);
        let number = position / PAGE;
        if self
            .map
            .pages
            .last()
            .is_none_or(|page| page.number != number)
        {
            self.map.point(number, self.map.pages.len());
            self.map.pages.push(Page::new(number));
        }
        if let Some(page) = self.map.pages.last_mut() {
            let offset = position % PAGE;
            page.bits[offset / 64] |= 1 << (offset % 64);
            page.values.push(value);
            page.high = offset + 1;
        }
    }

    /// Records the value in `loaded` at each position from `position` where
    /// `current` differs: what [`Self::push`] records from them one at a
    /// time, eight positions per step and a whole page record at once where
    /// the run covers one page from its start.
    #[inline(never)]
    pub fn push_changes(&mut self, position: usize, current: &[u8], loaded: &[u8]) {
        let len = current.len().min(loaded.len());
        let fresh = self.last.is_none_or(|last| last < position);
        if !fresh || !position.is_multiple_of(PAGE) || len > PAGE {
            for (index, (&now, &then)) in current.iter().zip(loaded).enumerate() {
                if now != then {
                    self.push(position.saturating_add(index), then);
                }
            }
            return;
        }
        // The changed offsets first, then their values in offset order.
        let mut page = Page::new(position / PAGE);
        let words = current[..len]
            .chunks_exact(8)
            .zip(loaded[..len].chunks_exact(8));
        for (index, (now, then)) in words.enumerate() {
            let lanes = u64::from_le_bytes(now.try_into().unwrap_or_default())
                ^ u64::from_le_bytes(then.try_into().unwrap_or_default());
            if lanes != 0 {
                page.bits[index / 8] |= u64::from(lane_mask(lanes)) << (index % 8 * 8);
            }
        }
        let tail = len / 8 * 8;
        for (offset, (&now, &then)) in current[tail..len]
            .iter()
            .zip(&loaded[tail..len])
            .enumerate()
        {
            if now != then {
                let offset = tail + offset;
                page.bits[offset / 64] |= 1 << (offset % 64);
            }
        }
        let count = page
            .bits
            .iter()
            .map(|bits| bits.count_ones() as usize)
            .sum();
        if count == 0 {
            return;
        }
        let mut values = Vec::with_capacity(count);
        for (word, &bits) in page.bits.iter().enumerate() {
            if bits == 0 {
                continue;
            }
            let base = word * 64;
            let mut rest = bits;
            while rest != 0 {
                let offset = base + rest.trailing_zeros() as usize;
                values.push(loaded.get(offset).copied().unwrap_or_default());
                rest &= rest - 1;
            }
            page.high = base + 64 - bits.leading_zeros() as usize;
        }
        page.values = values;
        self.last = Some(position + page.high - 1);
        self.map.point(page.number, self.map.pages.len());
        self.map.pages.push(page);
    }

    /// The map, its ranks counted and its spare capacity released.
    #[must_use]
    pub fn finish(mut self) -> SparseBytes {
        for page in &mut self.map.pages {
            page.count_ranks();
        }
        self.map.shrink_to_fit();
        self.map
    }
}

#[cfg(test)]
mod tests {
    use super::{Ascending, PAGE, SparseBytes};

    /// A plain model: every recorded position and its last value.
    fn model(records: &[(usize, u8)]) -> std::collections::BTreeMap<usize, u8> {
        records.iter().copied().collect()
    }

    fn build(records: &[(usize, u8)]) -> SparseBytes {
        let mut sparse = SparseBytes::new();
        for &(position, value) in records {
            sparse.set(position, value);
        }
        sparse
    }

    fn check(sparse: &SparseBytes, records: &[(usize, u8)]) {
        let model = model(records);
        for position in 0..70 * PAGE {
            assert_eq!(
                sparse.get(position),
                model.get(&position).copied(),
                "{position}"
            );
        }
        for (start, end) in [
            (0, 1),
            (60, 70),
            (PAGE - 3, PAGE + 3),
            (0, 6 * PAGE),
            (100, 100),
            (64 * PAGE - 5, 66 * PAGE + 9),
        ] {
            let expected = model.range(start..end.max(start)).next().is_some();
            assert_eq!(sparse.touches(start, end), expected, "{start}..{end}");
            let mut bytes: Vec<u8> = (start..end).map(|p| (p % 7) as u8).collect();
            sparse.overlay(start, &mut bytes);
            for (index, byte) in bytes.iter().enumerate() {
                let position = start + index;
                let want = model
                    .get(&position)
                    .copied()
                    .unwrap_or((position % 7) as u8);
                assert_eq!(*byte, want, "{position}");
            }
        }
    }

    /// Out-of-order positions, page and word boundaries, repeats, and a page
    /// past the first word of page bits.
    fn scattered() -> Vec<(usize, u8)> {
        let mut records = Vec::new();
        let mut state = 0x2545_f491_u32;
        for step in 0..3000_u32 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let position = (state >> 8) as usize % (5 * PAGE);
            records.push((position, (step % 251) as u8));
        }
        for position in [
            0,
            63,
            64,
            PAGE - 1,
            PAGE,
            2 * PAGE + 511,
            2 * PAGE + 512,
            65 * PAGE + 3,
        ] {
            records.push((position, 0xA5));
        }
        records
    }

    #[test]
    fn records_read_back_by_point_range_and_overlay() {
        let records = scattered();
        check(&build(&records), &records);
    }

    #[test]
    fn ascending_records_append() {
        // The replay's order: ascending runs of two bytes.
        let records: Vec<_> = (0..4000_usize)
            .flat_map(|item| {
                let at = 12 + item * 17;
                [(at, 1), (at + 1, 2)]
            })
            .collect();
        check(&build(&records), &records);
    }

    /// The ascending builder makes the map `set` makes from the same records,
    /// and leaves out a record that is not past the one before it.
    #[test]
    fn the_ascending_builder_makes_the_same_map() {
        let mut records = scattered();
        records.sort_unstable_by_key(|&(position, _)| position);
        records.dedup_by_key(|&mut (position, _)| position);
        let mut ascending = Ascending::default();
        for &(position, value) in &records {
            ascending.push(position, value);
            ascending.push(position, value.wrapping_add(1));
            ascending.push(position / 2, 0xEE);
        }
        let built = ascending.finish();
        check(&built, &records);
        let mut both = build(&records);
        both.shrink_to_fit();
        assert_eq!(built.heap_bytes(), both.heap_bytes());
        assert!(Ascending::default().finish().is_empty());
    }

    /// A span's record bits match its point reads, across a page boundary and
    /// at every alignment.
    #[test]
    fn span_bits_match_the_records() {
        let records = scattered();
        let sparse = build(&records);
        for start in (0..3 * PAGE).step_by(29).chain(PAGE - 70..PAGE + 10) {
            for count in [0, 1, 2, 31, 63, 64] {
                let span = sparse.span(start, start + count.max(1));
                let want = (0..count).fold(0_u64, |bits, index| {
                    bits | u64::from(sparse.get(start + index).is_some()) << index
                });
                assert_eq!(span.bits(start, count), want, "{start} {count}");
            }
        }
    }

    /// The lane mask has a bit for each non-zero byte lane, and only those.
    #[test]
    fn the_lane_mask_finds_every_changed_lane() {
        for lanes in [
            0_u64,
            1,
            0xff,
            0x8000_0000_0000_0000,
            0x0100_0000_0001_0000,
            u64::MAX,
        ] {
            let mask = super::lane_mask(lanes);
            for lane in 0..8 {
                assert_eq!(
                    mask >> lane & 1 == 1,
                    lanes >> (lane * 8) & 0xff != 0,
                    "{lanes:#x}"
                );
            }
        }
    }

    /// A run of changes records what its changed bytes one at a time would,
    /// whole pages or not.
    #[test]
    fn runs_record_their_changed_bytes() {
        let current: Vec<u8> = (0..3 * PAGE + 13).map(|at| (at % 251) as u8).collect();
        let mut loaded = current.clone();
        for at in (5..loaded.len()).step_by(37) {
            loaded[at] ^= 0x5A;
        }
        loaded[PAGE - 1] ^= 1;
        loaded[3 * PAGE + 12] ^= 1;
        let runs = [
            (0, PAGE),
            (PAGE, 2 * PAGE),
            (2 * PAGE + 9, 3 * PAGE),
            (3 * PAGE, 3 * PAGE + 13),
        ];
        let (mut changes, mut bytes) = (Ascending::default(), Ascending::default());
        for (start, end) in runs {
            changes.push_changes(start, &current[start..end], &loaded[start..end]);
            for at in start..end {
                if current[at] != loaded[at] {
                    bytes.push(at, loaded[at]);
                }
            }
        }
        // A run not past the last record keeps nothing new.
        changes.push_changes(0, &current[..PAGE], &loaded[..PAGE]);
        let (changes, bytes) = (changes.finish(), bytes.finish());
        for at in 0..current.len() {
            assert_eq!(changes.get(at), bytes.get(at), "{at}");
        }
        assert_eq!(changes.heap_bytes(), bytes.heap_bytes());
        let mut none = Ascending::default();
        none.push_changes(0, &current[..PAGE], &current[..PAGE]);
        assert!(none.finish().is_empty());
    }

    #[test]
    fn set_holds_the_last_value() {
        let mut sparse = SparseBytes::new();
        sparse.set(10, 1);
        sparse.set(10, 2);
        assert_eq!(sparse.get(10), Some(2));
        sparse.set(5, 6);
        assert_eq!((sparse.get(5), sparse.get(10)), (Some(6), Some(2)));
        assert!(!sparse.touches(11, PAGE));
        assert!(SparseBytes::new().is_empty());
        assert!(!SparseBytes::new().touches(0, usize::MAX));
        assert_eq!(sparse.get(usize::MAX), None);
    }

    #[test]
    fn heap_follows_the_recorded_pages() {
        let mut sparse = SparseBytes::new();
        for position in (0..PAGE).step_by(17) {
            sparse.set(position + 12, 7);
        }
        sparse.shrink_to_fit();
        let one_page = sparse.heap_bytes();
        assert!(one_page < 1024, "{one_page}");
        sparse.set(40 * PAGE, 1);
        sparse.shrink_to_fit();
        assert!(sparse.heap_bytes() > one_page);
    }
}
