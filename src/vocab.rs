//! Syllable keys, phrase tokens, and dictionary entries shared by the engine and its
//! models. [`PhraseText`] retains phrase text as UTF-8 or stored UCS-4 words.

use core::fmt;

use crate::options::OptionBits;
use crate::parser::{ChewingKey, Completeness};
use crate::syllables::{
    FULL_PINYIN_SYLLABLE_COUNT, INCOMPLETE_PINYIN_KEY_COUNT, INCOMPLETE_PINYIN_KEYS,
    OPTION_ONLY_COMPLETE_SYLLABLE_COUNT, OPTION_ONLY_COMPLETE_SYLLABLES,
    ZHUYIN_ONLY_COMPLETE_SYLLABLE_COUNT, ZHUYIN_ONLY_COMPLETE_SYLLABLES, full_pinyin_syllable,
    option_alias_canonical,
};
use crate::syllables::{
    FULL_PINYIN_SYLLABLE_MAP, INCOMPLETE_PINYIN_KEY_MAP, OPTION_ONLY_COMPLETE_SYLLABLE_MAP,
    ZHUYIN_ONLY_COMPLETE_SYLLABLE_MAP,
};
pub use crate::ucs4::{OutwardWords, PhraseText};

/// Number of dense table keys. Packed matrix alternatives are outside this inventory.
pub(crate) const SYLLABLE_KEY_COUNT: usize = FULL_PINYIN_SYLLABLE_COUNT
    + INCOMPLETE_PINYIN_KEY_COUNT
    + OPTION_ONLY_COMPLETE_SYLLABLE_COUNT
    + ZHUYIN_ONLY_COMPLETE_SYLLABLE_COUNT;

include!(concat!(env!("OUT_DIR"), "/chewing_keys.rs"));

const _: () = assert!(CHEWING_KEYS.len() == SYLLABLE_KEY_COUNT);

/// A decoder key drawn from the syllable inventory or from a matrix substitution.
///
/// The 440 dense IDs address complete full-pinyin spellings, initial-only keys,
/// option-only targets and Zhuyin-only syllables in that order. These IDs round-trip
/// through [`Self::from_index`]. A matrix alternative without a table row holds packed
/// elements, has empty text and counts as [`Completeness::Complete`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SyllableKey(u16);

impl SyllableKey {
    /// Looks up an exact complete pinyin spelling or an initial-only key. No option is
    /// required for an exact initial. Parsers decide whether to admit it. Case, tone digits
    /// and fuzzy spellings are not normalized.
    #[must_use]
    pub fn from_text(text: &str) -> Option<Self> {
        FULL_PINYIN_SYLLABLE_MAP.get(text).map_or_else(
            || {
                INCOMPLETE_PINYIN_KEY_MAP.get(text).and_then(|&index| {
                    Self::from_index(FULL_PINYIN_SYLLABLE_COUNT + index as usize)
                })
            },
            |&index| Self::from_index(index as usize),
        )
    }

    // content_table provides canonical spellings; pinyin_index supplies spelling aliases,
    // pinyin_parser2.cpp:38–57,114–147.
    /// Looks up a canonical spelling, including the option-only correction targets and
    /// Zhuyin-only syllables. [`Self::from_option_text`] also accepts enabled pinyin spelling
    /// aliases.
    #[must_use]
    pub fn from_canonical_text(text: &str) -> Option<Self> {
        if let Some(&index) = FULL_PINYIN_SYLLABLE_MAP.get(text) {
            return Self::from_index(index as usize);
        }
        if let Some(&index) = INCOMPLETE_PINYIN_KEY_MAP.get(text) {
            return Self::from_index(FULL_PINYIN_SYLLABLE_COUNT + index as usize);
        }
        let option_start = FULL_PINYIN_SYLLABLE_COUNT + INCOMPLETE_PINYIN_KEY_COUNT;
        if let Some(&index) = OPTION_ONLY_COMPLETE_SYLLABLE_MAP.get(text) {
            return Self::from_index(option_start + index as usize);
        }
        let zhuyin_start = option_start + OPTION_ONLY_COMPLETE_SYLLABLE_COUNT;
        ZHUYIN_ONLY_COMPLETE_SYLLABLE_MAP
            .get(text)
            .and_then(|&index| Self::from_index(zhuyin_start + index as usize))
    }

    /// The key a stored [`ChewingKey`] names, decoded from its initial,
    /// middle and final fields through the pin's `chewing_key_table`
    /// (`chewing_key.cpp:29-36` at 074a2219), tone ignored.
    ///
    /// Every key a pin parser can write is a `content_table` row
    /// (`pinyin_parser2.cpp:114`, `:146`; `zhuyin_parser2.cpp:95`), and
    /// every row has a key here, so no pin-written key is left out,
    /// including the option-only `eng`/`nun` and the zhuyin-only `den`.
    /// `None` for the zero key and for fields the table marks invalid,
    /// including final alternates the pin appends to the matrix.
    #[must_use]
    pub(crate) fn from_chewing_key(key: ChewingKey) -> Option<Self> {
        if key.table_index() == 0 {
            return None;
        }
        Self::from_canonical_text(key.pinyin_spelling())
    }

    // The final swap appends without a table check, phonetic_key_matrix.cpp:287–306.
    /// Keeps the phonetic fields of a matrix substitution even if they have no canonical
    /// spelling. Table keys use dense IDs and other alternatives use a tagged, tone-zero
    /// packed key.
    #[must_use]
    pub fn from_matrix_key(key: ChewingKey) -> Self {
        Self::from_chewing_key(key).unwrap_or_else(|| Self(0x8000 | key.with_tone(0).to_packed()))
    }

    /// Returns the key for `text` under parser option bits.
    ///
    /// Exact spellings work without an option. Correction aliases and the
    /// parser-table ambiguity aliases are admitted only when their bit is set.
    #[must_use]
    pub fn from_option_text(text: &str, options: OptionBits) -> Option<Self> {
        Self::from_option_text_with_distance(text, options).map(|(key, _)| key)
    }

    // pinyin_index_item_t::m_distance, pinyin_parser2.h:45 and pinyin_parser2.cpp:147. The
    // correction rows and the sua and zua ambiguity aliases in pinyin_parser_table.h:9–673
    // carry distance one.
    /// Looks up an option-enabled spelling and its correction distance. An exact spelling has
    /// distance zero. An enabled spelling alias has distance one.
    #[must_use]
    pub fn from_option_text_with_distance(text: &str, options: OptionBits) -> Option<(Self, u8)> {
        if let Some(key) = Self::from_text(text) {
            return Some((key, 0));
        }
        let canonical = option_alias_canonical(text, options)?;
        Self::from_canonical_text(canonical).map(|key| (key, 1))
    }

    /// Matrix-level fuzzy alternates, upstream append order.
    ///
    /// `fuzzy_syllable_step` applies initials, re-fetches the column, then
    /// finals (`phonetic_key_matrix.cpp:238-306`), so an initial bit and a
    /// final bit compose (`can` → `chang` under `AMB_C_CH` + `AMB_AN_ANG`).
    #[must_use]
    pub fn fuzzy_alternatives(self, options: OptionBits) -> Vec<Self> {
        fuzzy_chewing_alternatives(self.chewing_key(), options)
            .into_iter()
            .map(Self::from_matrix_key)
            .collect()
    }

    /// Returns the key with dense id `index`, or `None` when out of range.
    #[must_use]
    pub fn from_index(index: usize) -> Option<Self> {
        if index >= SYLLABLE_KEY_COUNT {
            return None;
        }
        u16::try_from(index).ok().map(Self)
    }

    /// The dense table id, or a tagged packed value for a matrix alternate.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Returns the lowercase ASCII spelling, or an empty string for a matrix key without a
    /// spelling row.
    #[must_use]
    pub fn text(self) -> &'static str {
        if self.0 & 0x8000 != 0 {
            return self.chewing_key().text();
        }
        let index = self.index();
        if let Some(syllable) = full_pinyin_syllable(index) {
            return syllable;
        }
        let incomplete_start = FULL_PINYIN_SYLLABLE_COUNT;
        if let Some(key) = INCOMPLETE_PINYIN_KEYS.get(index - incomplete_start) {
            return key;
        }
        let option_start = FULL_PINYIN_SYLLABLE_COUNT + INCOMPLETE_PINYIN_KEY_COUNT;
        if let Some(syllable) = OPTION_ONLY_COMPLETE_SYLLABLES.get(index - option_start) {
            return syllable;
        }
        let zhuyin_start = option_start + OPTION_ONLY_COMPLETE_SYLLABLE_COUNT;
        ZHUYIN_ONLY_COMPLETE_SYLLABLES
            .get(index - zhuyin_start)
            .copied()
            .unwrap_or_default()
    }

    /// Returns the phonetic elements stored for this syllable or packed matrix alternative.
    #[must_use]
    pub fn chewing_key(self) -> ChewingKey {
        if self.0 & 0x8000 != 0 {
            ChewingKey::from_packed(self.0)
        } else {
            CHEWING_KEYS[self.index()]
        }
    }

    /// Whether this key is a complete syllable or an initial-only key.
    #[must_use]
    pub const fn completeness(self) -> Completeness {
        if self.0 & 0x8000 != 0 {
            return Completeness::Complete;
        }
        if self.index() < FULL_PINYIN_SYLLABLE_COUNT
            || self.index() >= FULL_PINYIN_SYLLABLE_COUNT + INCOMPLETE_PINYIN_KEY_COUNT
        {
            Completeness::Complete
        } else {
            Completeness::Partial
        }
    }
}

// fuzzy_syllable_step, phonetic_key_matrix.cpp:238–306.
/// Returns the enabled initial and final substitutions in matrix order, preserving tone.
/// The original key is omitted. Final substitutions apply to the original and to each
/// admitted initial alternative, even when the resulting final has no spelling row.
#[must_use]
fn fuzzy_chewing_alternatives(key: ChewingKey, options: OptionBits) -> Vec<ChewingKey> {
    let initials = fuzzy_chewing_initials(key, options);
    let mut out = initials.clone();
    for source in std::iter::once(key).chain(initials) {
        out.extend(fuzzy_chewing_finals(source, options));
    }
    out
}

/// Initial swaps only; the matrix applies these to its column snapshot.
#[must_use]
pub(crate) fn fuzzy_chewing_initials(key: ChewingKey, options: OptionBits) -> Vec<ChewingKey> {
    let mut initials = Vec::new();
    for (bit, a, b) in [
        (OptionBits::PINYIN_AMB_C_CH, "c", "ch"),
        (OptionBits::PINYIN_AMB_Z_ZH, "z", "zh"),
        (OptionBits::PINYIN_AMB_S_SH, "s", "sh"),
        (OptionBits::PINYIN_AMB_L_R, "l", "r"),
        (OptionBits::PINYIN_AMB_L_N, "l", "n"),
        (OptionBits::PINYIN_AMB_F_H, "f", "h"),
        (OptionBits::PINYIN_AMB_G_K, "g", "k"),
    ] {
        if !options.contains(bit) {
            continue;
        }
        if let (Some(a), Some(b)) = (ChewingKey::from_pinyin(a), ChewingKey::from_pinyin(b)) {
            for (from, to) in [(a.initial, b.initial), (b.initial, a.initial)] {
                if key.initial == from {
                    let mut alternate = key;
                    alternate.initial = to;
                    if alternate.table_index() != 0 {
                        initials.push(alternate);
                    }
                }
            }
        }
    }
    initials
}

/// Final swaps only, including combinations with no syllable-table row.
#[must_use]
pub(crate) fn fuzzy_chewing_finals(source: ChewingKey, options: OptionBits) -> Vec<ChewingKey> {
    let mut out = Vec::new();
    for (bit, a, b) in FUZZY_FINALS {
        if !options.contains(bit) {
            continue;
        }
        for (from, to) in [(a, b), (b, a)] {
            if source.final_ == from {
                let mut alternate = source;
                alternate.final_ = to;
                out.push(alternate);
            }
        }
    }
    out
}

/// The final swaps of `fuzzy_syllable_step`
/// (`phonetic_key_matrix.cpp:301-306`), as `ChewingFinal` values
/// (`chewing_enum.h:67-68`, `:73-74`, `:80-81`). `in` and `ing` are no
/// syllables of their own, so their finals cannot come from a spelling.
const FUZZY_FINALS: [(OptionBits, u8, u8); 3] = [
    (OptionBits::PINYIN_AMB_AN_ANG, 3, 4),
    (OptionBits::PINYIN_AMB_EN_ENG, 9, 10),
    (OptionBits::PINYIN_AMB_IN_ING, 16, 17),
];

impl fmt::Display for SyllableKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.text())
    }
}

// PHRASE_MASK, novel_types.h:41.
/// Mask for the low 24 bits that select a phrase slot in its library.
pub const PHRASE_MASK: u32 = 0x00FF_FFFF;

// PHRASE_INDEX_LIBRARY_MASK, novel_types.h:42.
/// Mask for bits 24 through 27 that select a phrase library.
pub const PHRASE_INDEX_LIBRARY_MASK: u32 = 0x0F00_0000;

// PHRASE_INDEX_LIBRARY_COUNT, novel_types.h:43.
/// Number of addressable phrase libraries.
pub const PHRASE_INDEX_LIBRARY_COUNT: u8 = 16;

// MAX_PHRASE_LENGTH, novel_types.h:119; pinyin.cpp:639–644,3678.
/// Maximum number of keys in a phrase lookup. Import accepts fewer than this many UCS-4
/// words, with at least one word and one key per word.
pub const MAX_PHRASE_LENGTH: usize = 16;

// null_token, novel_types.h:121.
/// Token denoting the absence of a phrase.
pub const NULL_TOKEN: u32 = 0;

// sentence_start, novel_types.h:122.
/// Predecessor token of the first phrase in a sentence.
pub const SENTENCE_START: u32 = 1;

// TSI_DICTIONARY, novel_types.h:155.
/// Default Zhuyin phrase library.
pub const TSI_DICTIONARY: u8 = 1;

// GBK_DICTIONARY, novel_types.h:156.
/// Library for the GBK dictionary.
pub const GBK_DICTIONARY: u8 = 2;

// ADDON_DICTIONARY, novel_types.h:159; addon.bin is a USER_FILE entry. See
// docs/findings/phrase-union.md and docs/findings/addon-choose-promotion.md.
/// Destination library for a chosen addon phrase promoted into the default dictionary.
pub const ADDON_DICTIONARY: u8 = 5;

// NETWORK_DICTIONARY, novel_types.h:160.
/// Library for network phrases.
pub const NETWORK_DICTIONARY: u8 = 6;

// USER_DICTIONARY, novel_types.h:161; docs/findings/user-store.md §3.
/// Library for user phrases.
pub const USER_DICTIONARY: u8 = 7;

// PHRASE_INDEX_LIBRARY_INDEX, novel_types.h:44.
/// Extracts the library index from bits 24 through 27 of a [`PhraseToken`].
#[must_use]
pub const fn phrase_index_library_index(token: u32) -> u8 {
    ((token & PHRASE_INDEX_LIBRARY_MASK) >> 24) as u8
}

// PHRASE_INDEX_MAKE_TOKEN, novel_types.h:45–46.
/// Builds a [`PhraseToken`] from the low four bits of the library index and the low 24
/// bits of the slot.
#[must_use]
pub const fn phrase_index_make_token(phrase_index: u8, token: u32) -> u32 {
    (((phrase_index as u32) << 24) & PHRASE_INDEX_LIBRARY_MASK) | (token & PHRASE_MASK)
}

// phrase_token_t is guint32, novel_types.h:33,41–46.
/// Identifies a dictionary phrase in the dictionary and language model. Use
/// [`phrase_index_library_index`] to extract the library and [`phrase_index_make_token`]
/// to combine it with a slot.
pub type PhraseToken = u32;

/// One dictionary hit: a phrase and the token that scores it.
///
/// The entry does not restate how many keys it covers. A lookup is made for an
/// exact key slice, so the caller already knows.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PhraseEntry {
    token: PhraseToken,
    text: PhraseText,
    /// Pronunciation possibility `(matched, total)` over the phrase item's
    /// stored pronunciations: the fraction of
    /// the item's pronunciation frequency that spells the looked-up key
    /// sequence. `None` when the backend carries no per-pronunciation
    /// counts (the candidate scan never reads it; only the sentence
    /// trellis does).
    pronunciation: Option<(u64, u64)>,
}

impl PhraseEntry {
    /// Builds an entry.
    ///
    /// Phrase text is a [`PhraseText`]: ordinary hits are one to four CJK
    /// scalars and stay inline (no heap). Callers may pass a [`String`],
    /// `&str`, `CompactString`, or [`PhraseText`].
    #[must_use]
    pub fn new(token: PhraseToken, text: impl Into<PhraseText>) -> Self {
        Self {
            token,
            text: text.into(),
            pronunciation: None,
        }
    }

    /// The scoring token.
    #[must_use]
    pub const fn token(&self) -> PhraseToken {
        self.token
    }

    /// The phrase's outward text.
    #[must_use]
    pub fn text(&self) -> &PhraseText {
        &self.text
    }

    /// The outward text as `str`, when every stored word was a scalar
    /// value.
    #[cfg(test)]
    #[must_use]
    pub fn text_str(&self) -> Option<&str> {
        self.text.as_str()
    }

    /// The item's stored character count, from the text's own hint.
    #[must_use]
    pub fn phrase_length(&self) -> usize {
        self.text.phrase_length()
    }

    /// The pronunciation possibility `(matched, total)`, when the backend
    /// carries per-pronunciation counts.
    ///
    /// Upstream's `get_pronunciation_possibility`
    /// (`storage/phrase_index.h:129-160`): matched-pronunciation frequency
    /// over the item's total. `None` means no discount — the possibility
    /// is 1.
    #[must_use]
    pub const fn pronunciation_possibility(&self) -> Option<(u64, u64)> {
        self.pronunciation
    }

    /// Sets the matched and total pronunciation counts. [`Self::new`] leaves these counts
    /// unset.
    #[must_use]
    pub const fn with_pronunciation_possibility(mut self, matched: u64, total: u64) -> Self {
        self.pronunciation = Some((matched, total));
        self
    }

    /// Replace the pronunciation possibility of an existing entry.
    pub const fn set_pronunciation_possibility(&mut self, matched: u64, total: u64) {
        self.pronunciation = Some((matched, total));
    }

    /// Consumes the entry and returns its phrase text.
    #[must_use]
    pub(crate) fn into_text(self) -> PhraseText {
        self.text
    }
}

#[cfg(test)]
mod tests;
