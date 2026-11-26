//! The `PhraseItem` wire view (074a2219 `phrase_index.h:56-163`): an item's
//! header, UCS-4 text and pronunciations, read in place from its bytes.
//! Every layer that reads an item borrows it through this view.

use crate::vocab::PhraseText;

/// `phrase_item_header` (`phrase_index.h:56`): `{u8 phrase_length,
/// u8 n_pronunciations, u32 unigram}` ahead of every item's UCS-4 text.
pub const ITEM_HEADER_SIZE: usize = 6;

/// `sizeof(ChewingKey)` — a 16-bit bitfield (`chewing_key.h:41`), the
/// width one pronunciation key occupies inside an item.
pub const CHEWING_KEY_SIZE: usize = size_of::<u16>();

/// Reads a little-endian `u32` at `offset`, or `None` past the end.
pub(crate) fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(*bytes.get(offset..)?.first_chunk()?))
}

/// One pronunciation of a phrase item: packed `ChewingKey` bytes and frequency.
#[derive(Clone, Copy)]
pub struct PronunciationView<'a> {
    /// The packed `ChewingKey[L]` bytes, in stored order.
    pub keys: &'a [u8],
    /// This pronunciation's frequency.
    pub freq: u32,
}

/// A phrase item as it sits in the mapped entry area — upstream's
/// `PhraseItem` over `SubPhraseIndex::get_phrase_item`'s view, without
/// the copy.
///
/// `get_phrase_length`/`get_n_pronunciation`/
/// `get_unigram_frequency`/`get_phrase_string`/
/// `get_nth_pronunciation` become reads below.
#[derive(Clone, Copy)]
pub struct PhraseItemView<'a> {
    bytes: &'a [u8],
}

impl<'a> PhraseItemView<'a> {
    /// Read a complete item from its wire bytes. Refuse a truncated header, text or reading.
    #[must_use]
    pub fn from_bytes(bytes: &'a [u8]) -> Option<Self> {
        let view = Self {
            bytes: bytes.get(..ITEM_HEADER_SIZE)?,
        };
        Some(Self {
            bytes: bytes.get(..view.total_size()?)?,
        })
    }

    // PhraseItem field accessors, phrase_index.cpp:180–205.
    /// Borrows an item beginning at `bytes`. The six-byte header must be present. A view can
    /// end before the last declared field, so check each field before using it.
    #[must_use]
    pub fn fields(bytes: &'a [u8]) -> Option<Self> {
        let header = bytes.get(..ITEM_HEADER_SIZE)?;
        let total = PhraseItemView { bytes: header }.total_size()?;
        Some(Self {
            bytes: bytes.get(..total).unwrap_or(bytes),
        })
    }

    /// The item's bytes as `get_phrase_item` hands them out: the header-derived
    /// length from its offset, nothing beyond it.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

impl PhraseItemView<'_> {
    /// `PhraseItem::get_phrase_length`.
    #[must_use]
    pub fn phrase_length(&self) -> usize {
        self.bytes.first().copied().unwrap_or(0) as usize
    }

    /// `PhraseItem::get_n_pronunciation`.
    #[must_use]
    pub fn n_pronunciations(&self) -> usize {
        self.bytes.get(1).copied().unwrap_or(0) as usize
    }

    /// `PhraseItem::get_unigram_frequency` — the item's stored unigram
    /// count (the field `gen_unigram`'s +1 sweep and
    /// `add_unigram_frequency` write).
    #[must_use]
    pub fn unigram(&self) -> u32 {
        u32_at(self.bytes, 2).unwrap_or(0)
    }

    // PhraseItem::get_phrase_string and g_ucs4_to_utf8, lookup/lookup.cpp:39–68.
    /// Reads the declared UCS-4 words for one phrase. Every word must be readable. Before the
    /// first zero, values must be below `0x8000_0000`. The first zero ends the outward text,
    /// and accepted non-scalar words retain their stored representation.
    #[must_use]
    pub fn phrase_text(&self) -> Option<PhraseText> {
        PhraseText::from_stored_words(self.phrase_codes())
    }

    /// Whether [`Self::phrase_text`] answers a text, without building it.
    #[must_use]
    pub(crate) fn phrase_text_converts(&self) -> bool {
        PhraseText::stored_words_convert(self.phrase_codes())
    }

    /// Whether every declared word of the text lies inside the item's bytes.
    #[must_use]
    pub(crate) fn phrase_words_present(&self) -> bool {
        self.phrase_words().is_some()
    }

    /// The item's stored words, as `get_phrase_string` copies them into the
    /// caller's buffer (074a2219 `phrase_index.h:105-110`). `None` where the
    /// bytes end before a declared word.
    pub(crate) fn stored_words(
        &self,
    ) -> Option<smallvec::SmallVec<[u32; crate::vocab::MAX_PHRASE_LENGTH]>> {
        self.phrase_words().map(|stored| {
            stored
                .iter()
                .map(|word| u32::from_le_bytes(*word))
                .collect()
        })
    }

    /// The item's stored UCS-4 words, as `get_phrase_string` copies them
    /// (074a2219 `phrase_index.h:105-110`), where the bytes hold every
    /// declared word.
    #[must_use]
    pub(crate) fn phrase_words(&self) -> Option<&[[u8; 4]]> {
        let end = ITEM_HEADER_SIZE.checked_add(4 * self.phrase_length())?;
        Some(self.bytes.get(ITEM_HEADER_SIZE..end)?.as_chunks::<4>().0)
    }

    /// The phrase's stored UCS-4 words in order, `None` where the item's
    /// bytes end before a declared word.
    pub fn phrase_codes(&self) -> impl Iterator<Item = Option<u32>> + '_ {
        let length = self.phrase_length();
        (0..length).map(move |index| {
            let offset = ITEM_HEADER_SIZE.checked_add(4 * index)?;
            u32_at(self.bytes, offset)
        })
    }

    // PhraseItem::get_nth_pronunciation, phrase_index.cpp:180–205.
    /// Borrows one reading's packed keys and frequency. Supply a stored reading index whose
    /// keys and frequency are present.
    #[must_use]
    pub fn pronunciation(&self, index: usize) -> Option<PronunciationView<'_>> {
        if index >= self.n_pronunciations() {
            return None;
        }
        let stride = CHEWING_KEY_SIZE
            .checked_mul(self.phrase_length())?
            .checked_add(4)?;
        let start = ITEM_HEADER_SIZE
            .checked_add(4 * self.phrase_length())?
            .checked_add(index.checked_mul(stride)?)?;
        let keys_start = start;
        let freq_start = keys_start.checked_add(stride.checked_sub(4)?)?;
        let keys_end = keys_start.checked_add(CHEWING_KEY_SIZE * self.phrase_length())?;
        let keys = self.bytes.get(keys_start..keys_end)?;
        let freq = u32_at(self.bytes, freq_start)?;
        Some(PronunciationView { keys, freq })
    }

    /// Visits every declared pronunciation position, in stored order. An incomplete reading,
    /// whose packed keys or frequency the item's bytes do not hold, still yields a placeholder
    /// with empty keys and zero frequency. [`Self::pronunciation`] returns `None` for such a
    /// reading, so it tells a missing reading apart from a stored one.
    pub fn pronunciations(&self) -> impl Iterator<Item = PronunciationView<'_>> + '_ {
        (0..self.n_pronunciations()).map(|index| {
            self.pronunciation(index)
                .unwrap_or(PronunciationView { keys: &[], freq: 0 })
        })
    }

    /// The item's total byte length in the entry area —
    /// `phrase_item_header + L·4 + n·(L·sizeof(ChewingKey) + 4)`
    /// (`SubPhraseIndex::get_phrase_item`). `None` when the stored
    /// header claims a length that overflows or leaves the entry area.
    fn total_size(&self) -> Option<usize> {
        let length = self.phrase_length();
        let per_pron = CHEWING_KEY_SIZE
            .checked_mul(length)?
            .checked_add(4)?
            .checked_mul(self.n_pronunciations())?;
        ITEM_HEADER_SIZE
            .checked_add(4 * length)?
            .checked_add(per_pron)
    }
}
