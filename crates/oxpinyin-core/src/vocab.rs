//! Decoder vocabulary: the concrete types the engine seam names.
//!
//! `docs/findings/core-trait-seam.md` leaves `Dictionary` and `LanguageModel`
//! generic over associated types so `oxpinyin-core` depends on nothing. The
//! session API in `docs/findings/session-api.md` has to pin those
//! associated types to something concrete, or every later task would widen its
//! own bounds. These are those types.

use core::fmt;

use crate::options::OptionBits;
use crate::syllables::{
    FULL_PINYIN_SYLLABLE_MAP, INCOMPLETE_PINYIN_KEY_MAP, OPTION_ONLY_COMPLETE_SYLLABLE_MAP,
    ZHUYIN_ONLY_COMPLETE_SYLLABLE_MAP,
};
use crate::{ChewingKey, PhraseText};
use crate::{
    Completeness, FULL_PINYIN_SYLLABLE_COUNT, INCOMPLETE_PINYIN_KEY_COUNT, INCOMPLETE_PINYIN_KEYS,
    OPTION_ONLY_COMPLETE_SYLLABLE_COUNT, OPTION_ONLY_COMPLETE_SYLLABLES,
    ZHUYIN_ONLY_COMPLETE_SYLLABLE_COUNT, ZHUYIN_ONLY_COMPLETE_SYLLABLES, full_pinyin_syllable,
    option_alias_canonical,
};

/// Number of table keys addressable by a [`SyllableKey`].
/// Packed matrix alternates outside the table are not included.
///
/// **Not constant across stages.** This is 440 for Stage 1 — the 405 complete
/// syllables, the 23 initial-only keys, the two option-only correction
/// targets (`eng`, `nun`), and the ten zhuyin-only syllables reachable
/// through the zhuyin index alone. Stage 3 additions take ids *after* the
/// current end rather than renumbering, so the existing ids stay stable
/// while this count grows. Anything that persists a key id must record the
/// inventory it was written against; anything that sizes an array by this
/// value must recompute it rather than hard-code 440.
pub const SYLLABLE_KEY_COUNT: usize = FULL_PINYIN_SYLLABLE_COUNT
    + INCOMPLETE_PINYIN_KEY_COUNT
    + OPTION_ONLY_COMPLETE_SYLLABLE_COUNT
    + ZHUYIN_ONLY_COMPLETE_SYLLABLE_COUNT;

include!(concat!(env!("OUT_DIR"), "/chewing_keys.rs"));

const _: () = assert!(CHEWING_KEYS.len() == SYLLABLE_KEY_COUNT);

/// One decoder-visible key: a table syllable, an initial-only key, or a
/// packed matrix alternate without a syllable-table row.
///
/// Ids are dense and frozen: `0..405` are [`crate::full_pinyin_syllables`] in their
/// pinned upstream numeric-ID order, `405..428` are
/// [`INCOMPLETE_PINYIN_KEYS`] in ascending byte order, `428..430` are
/// [`OPTION_ONLY_COMPLETE_SYLLABLES`], and `430..440` are
/// [`ZHUYIN_ONLY_COMPLETE_SYLLABLES`]. The ordering is part of the decoder's
/// determinism, so it is never derived from a hash map.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SyllableKey(u16);

impl SyllableKey {
    /// Returns the key spelled exactly `text`, or `None` if there is no such
    /// key.
    ///
    /// Lookup is exact: no case folding, tone stripping or fuzzy matching.
    /// Membership goes through the generated phf maps (build.rs, from the
    /// frozen inventories) instead of a linear scan.
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

    /// Returns the key for a canonical spelling, including option-only
    /// correction targets and zhuyin-only syllables that [`Self::from_text`]
    /// deliberately omits.
    ///
    /// This mirrors `content_table` membership; [`Self::from_text`] mirrors
    /// `pinyin_index` membership. Correction aliases
    /// ([`Self::from_option_text`]) resolve only within the pinyin-reachable
    /// tiers, so tier 4 never becomes a full-pinyin parse target.
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
    pub fn from_chewing_key(key: ChewingKey) -> Option<Self> {
        if key.table_index() == 0 {
            return None;
        }
        Self::from_canonical_text(key.pinyin_spelling())
    }

    /// Keeps a matrix key even when the pin's final swap produces no
    /// syllable-table row (phonetic_key_matrix.cpp:287-306). The high bit
    /// tags a tone-zero packed key; canonical keys keep their dense ids.
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

    /// [`Self::from_option_text`] plus the matched row's correction
    /// distance, `pinyin_index_item_t::m_distance` (`pinyin_parser2.h:45`,
    /// read at `pinyin_parser2.cpp:147` at 074a2219).
    ///
    /// In the pin's `pinyin_index` (`pinyin_parser_table.h:9-673`) every
    /// `PINYIN_CORRECT_*` row and the two `PINYIN_AMB_S_SH` /
    /// `PINYIN_AMB_Z_ZH` rows (`sua`, `zua`) carry distance 1 and every
    /// other row 0. Those gated rows are exactly the aliases
    /// [`option_alias_canonical`] admits, so an exact spelling is 0 and an
    /// alias is 1.
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
    #[cfg(any(test, feature = "tools"))]
    pub(crate) fn fuzzy_alternatives(self, options: OptionBits) -> Vec<Self> {
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

    /// The key's lowercase ASCII spelling.
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

    /// The key's `ChewingKey`, tone zero: the fields of its `content_table`
    /// row, by index, equal to `ChewingKey::from_pinyin(self.text())`.
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

/// The pin's initial and final swaps on the stored key, including invalid
/// final combinations, which the pin appends without a table check.
#[must_use]
pub fn fuzzy_chewing_alternatives(key: ChewingKey, options: OptionBits) -> Vec<ChewingKey> {
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

/// Opaque identifier of a phrase in a dictionary and language model.
///
/// The engine never interprets the number; it only compares tokens and hands
/// them to a [`crate::model::LanguageModel`]. That keeps the token layout entirely a
/// backend concern, so a fixture adapter and a table-backed loader can use
/// different numbering without the decoder noticing.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PhraseToken(u32);

impl PhraseToken {
    /// Builds a token from its backend-defined numeric value.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// The backend-defined numeric value.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PhraseToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// One dictionary hit: a phrase and the token that scores it.
///
/// The entry does not restate how many keys it covers. A lookup is made for an
/// exact key slice, so the caller already knows.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PhraseEntry {
    token: PhraseToken,
    text: PhraseText,
    /// Pronunciation possibility `(matched, total)` over the phrase item's
    /// stored pronunciations (W14's polyphone discount): the fraction of
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

    /// Sets the pronunciation possibility (builder; [`PhraseEntry::new`]
    /// leaves it unset so every existing construction keeps its meaning).
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
