//! Decoder vocabulary: the concrete types the engine seam names.
//!
//! `docs/findings/core-trait-seam.md` leaves `Dictionary` and `LanguageModel`
//! generic over associated types so `oxpinyin-core` depends on nothing. The
//! session API frozen in `docs/findings/session-api.md` has to pin those
//! associated types to something concrete, or every later task would widen its
//! own bounds. These are those types.

use core::fmt;

use crate::options::{
    OptionBits, PINYIN_AMB_AN_ANG, PINYIN_AMB_C_CH, PINYIN_AMB_EN_ENG, PINYIN_AMB_F_H,
    PINYIN_AMB_G_K, PINYIN_AMB_IN_ING, PINYIN_AMB_L_N, PINYIN_AMB_L_R, PINYIN_AMB_S_SH,
    PINYIN_AMB_Z_ZH,
};
use crate::{
    Completeness, FULL_PINYIN_SYLLABLE_COUNT, FULL_PINYIN_SYLLABLES, INCOMPLETE_PINYIN_KEY_COUNT,
    INCOMPLETE_PINYIN_KEYS, OPTION_ONLY_COMPLETE_SYLLABLE_COUNT, OPTION_ONLY_COMPLETE_SYLLABLES,
    option_alias_canonical,
};

/// Number of keys addressable by a [`SyllableKey`].
///
/// **Not constant across stages.** This is 430 for Stage 1 — the 405 complete
/// syllables, the 23 initial-only keys, and the two option-only correction
/// targets (`eng`, `nun`). Stage 2 additions take ids *after* the current end
/// rather than renumbering, so the existing ids stay stable while this count
/// grows. Anything that persists a key id must record the inventory it was
/// written against; anything that sizes an array by this value must recompute
/// it rather than hard-code 430.
pub const SYLLABLE_KEY_COUNT: usize =
    FULL_PINYIN_SYLLABLE_COUNT + INCOMPLETE_PINYIN_KEY_COUNT + OPTION_ONLY_COMPLETE_SYLLABLE_COUNT;

/// One decoder-visible pinyin key: a complete syllable or an initial-only key.
///
/// Ids are dense and frozen: `0..405` are [`FULL_PINYIN_SYLLABLES`] in their
/// pinned upstream numeric-ID order, `405..428` are
/// [`INCOMPLETE_PINYIN_KEYS`] in ascending byte order, and `428..430` are
/// [`OPTION_ONLY_COMPLETE_SYLLABLES`]. The ordering is part of the decoder's
/// determinism, so it is never derived from a hash map.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SyllableKey(u16);

impl SyllableKey {
    /// Returns the key spelled exactly `text`, or `None` if there is no such
    /// key.
    ///
    /// Lookup is exact: no case folding, tone stripping or fuzzy matching.
    #[must_use]
    pub fn from_text(text: &str) -> Option<Self> {
        if let Some(index) = FULL_PINYIN_SYLLABLES
            .iter()
            .position(|syllable| *syllable == text)
        {
            return Self::from_index(index);
        }
        INCOMPLETE_PINYIN_KEYS
            .iter()
            .position(|key| *key == text)
            .and_then(|index| Self::from_index(FULL_PINYIN_SYLLABLE_COUNT + index))
    }

    /// Returns the key for a canonical spelling, including option-only
    /// correction targets that [`Self::from_text`] deliberately omits.
    #[must_use]
    pub fn from_canonical_text(text: &str) -> Option<Self> {
        if let Some(index) = FULL_PINYIN_SYLLABLES
            .iter()
            .position(|syllable| *syllable == text)
        {
            return Self::from_index(index);
        }
        if let Some(index) = INCOMPLETE_PINYIN_KEYS.iter().position(|key| *key == text) {
            return Self::from_index(FULL_PINYIN_SYLLABLE_COUNT + index);
        }
        OPTION_ONLY_COMPLETE_SYLLABLES
            .iter()
            .position(|syllable| *syllable == text)
            .and_then(|index| {
                Self::from_index(FULL_PINYIN_SYLLABLE_COUNT + INCOMPLETE_PINYIN_KEY_COUNT + index)
            })
    }

    /// Returns the key for `text` under parser option bits.
    ///
    /// Exact spellings work without an option. Correction aliases and the
    /// parser-table ambiguity aliases are admitted only when their bit is set.
    #[must_use]
    pub fn from_option_text(text: &str, options: OptionBits) -> Option<Self> {
        if let Some(key) = Self::from_text(text) {
            return Some(key);
        }
        let canonical = option_alias_canonical(text, options)?;
        Self::from_canonical_text(canonical)
    }

    /// Matrix-level fuzzy alternates for this key, in upstream append order.
    ///
    /// This reproduces `fuzzy_syllable_step`
    /// (`phonetic_key_matrix.cpp:238-315`). The alternates share this key's
    /// byte span; the caller decides whether the resulting spelling is a real
    /// key.
    #[must_use]
    pub fn fuzzy_alternatives(self, options: OptionBits) -> Vec<Self> {
        let text = self.text();
        let mut out = Vec::new();

        if options.contains(PINYIN_AMB_C_CH) {
            swap_initial(text, "c", "ch", &mut out);
            swap_initial(text, "ch", "c", &mut out);
        }
        if options.contains(PINYIN_AMB_Z_ZH) {
            swap_initial(text, "z", "zh", &mut out);
            swap_initial(text, "zh", "z", &mut out);
        }
        if options.contains(PINYIN_AMB_S_SH) {
            swap_initial(text, "s", "sh", &mut out);
            swap_initial(text, "sh", "s", &mut out);
        }
        if options.contains(PINYIN_AMB_L_R) {
            swap_initial(text, "l", "r", &mut out);
            swap_initial(text, "r", "l", &mut out);
        }
        if options.contains(PINYIN_AMB_L_N) {
            swap_initial(text, "l", "n", &mut out);
            swap_initial(text, "n", "l", &mut out);
        }
        if options.contains(PINYIN_AMB_F_H) {
            swap_initial(text, "f", "h", &mut out);
            swap_initial(text, "h", "f", &mut out);
        }
        if options.contains(PINYIN_AMB_G_K) {
            swap_initial(text, "g", "k", &mut out);
            swap_initial(text, "k", "g", &mut out);
        }

        if options.contains(PINYIN_AMB_AN_ANG) {
            swap_final(text, "an", "ang", &mut out);
            swap_final(text, "ang", "an", &mut out);
        }
        if options.contains(PINYIN_AMB_EN_ENG) {
            swap_final(text, "en", "eng", &mut out);
            swap_final(text, "eng", "en", &mut out);
        }
        if options.contains(PINYIN_AMB_IN_ING) {
            swap_final(text, "in", "ing", &mut out);
            swap_final(text, "ing", "in", &mut out);
        }

        out
    }

    /// Returns the key with dense id `index`, or `None` when out of range.
    #[must_use]
    pub fn from_index(index: usize) -> Option<Self> {
        if index >= SYLLABLE_KEY_COUNT {
            return None;
        }
        u16::try_from(index).ok().map(Self)
    }

    /// The dense id of this key.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// The key's lowercase ASCII spelling.
    #[must_use]
    pub fn text(self) -> &'static str {
        let index = self.index();
        if let Some(syllable) = FULL_PINYIN_SYLLABLES.get(index) {
            return syllable;
        }
        let incomplete_start = FULL_PINYIN_SYLLABLE_COUNT;
        if let Some(key) = INCOMPLETE_PINYIN_KEYS.get(index - incomplete_start) {
            return key;
        }
        let option_start = FULL_PINYIN_SYLLABLE_COUNT + INCOMPLETE_PINYIN_KEY_COUNT;
        OPTION_ONLY_COMPLETE_SYLLABLES
            .get(index - option_start)
            .copied()
            .unwrap_or_default()
    }

    /// Whether this key is a complete syllable or an initial-only key.
    #[must_use]
    pub const fn completeness(self) -> Completeness {
        if self.index() < FULL_PINYIN_SYLLABLE_COUNT
            || self.index() >= FULL_PINYIN_SYLLABLE_COUNT + INCOMPLETE_PINYIN_KEY_COUNT
        {
            Completeness::Complete
        } else {
            Completeness::Partial
        }
    }

    /// Length in bytes of [`SyllableKey::text`].
    #[must_use]
    pub fn len(self) -> usize {
        self.text().len()
    }

    /// Always `false`; every key has a non-empty spelling.
    ///
    /// Present because clippy asks any type with `len` for an `is_empty`, and a
    /// constant answer is more honest than omitting it.
    #[must_use]
    pub fn is_empty(self) -> bool {
        false
    }
}

fn swap_initial(text: &str, from: &str, to: &str, out: &mut Vec<SyllableKey>) {
    let Some(rest) = text.strip_prefix(from) else {
        return;
    };
    let mut candidate = String::with_capacity(to.len() + rest.len());
    candidate.push_str(to);
    candidate.push_str(rest);
    if let Some(key) = SyllableKey::from_canonical_text(&candidate) {
        out.push(key);
    }
}

fn swap_final(text: &str, from: &str, to: &str, out: &mut Vec<SyllableKey>) {
    let Some(prefix) = text.strip_suffix(from) else {
        return;
    };
    let mut candidate = String::with_capacity(prefix.len() + to.len());
    candidate.push_str(prefix);
    candidate.push_str(to);
    if let Some(key) = SyllableKey::from_canonical_text(&candidate) {
        out.push(key);
    }
}

impl fmt::Display for SyllableKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.text())
    }
}

/// Opaque identifier of a phrase in a dictionary and language model.
///
/// The engine never interprets the number; it only compares tokens and hands
/// them to a [`crate::LanguageModel`]. That keeps the token layout entirely a
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
    text: String,
}

impl PhraseEntry {
    /// Builds an entry.
    ///
    /// `const` despite the `String`: this function allocates nothing, it only
    /// moves a string the caller already built, which const evaluation
    /// permits. Verified on the pinned 1.97.1 — a const context can call it,
    /// but only with `String::new()`. Building a *non-empty* `String` at
    /// compile time is still rejected (`String::from` in a `const` is E0015,
    /// "cannot call non-const associated function"), so the qualifier costs
    /// nothing and buys little today; it is kept so the door stays open, not
    /// because anything relies on it.
    #[must_use]
    pub const fn new(token: PhraseToken, text: String) -> Self {
        Self { token, text }
    }

    /// The scoring token.
    #[must_use]
    pub const fn token(&self) -> PhraseToken {
        self.token
    }

    /// The phrase text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Consumes the entry and returns its phrase text.
    #[must_use]
    pub fn into_text(self) -> String {
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::{PhraseEntry, PhraseToken, SYLLABLE_KEY_COUNT, SyllableKey};
    use crate::OptionBits;
    use crate::{
        Completeness, FULL_PINYIN_SYLLABLES, INCOMPLETE_PINYIN_KEYS, OPTION_ONLY_COMPLETE_SYLLABLES,
    };

    #[test]
    fn every_frozen_key_round_trips_through_its_id() {
        for index in 0..SYLLABLE_KEY_COUNT {
            let key = SyllableKey::from_index(index).expect("index is in range");
            assert_eq!(key.index(), index);
            assert_eq!(SyllableKey::from_canonical_text(key.text()), Some(key));
            assert_eq!(key.len(), key.text().len());
            assert!(!key.is_empty());
        }
        assert_eq!(SyllableKey::from_index(SYLLABLE_KEY_COUNT), None);
    }

    #[test]
    fn completeness_follows_the_frozen_id_ranges() {
        for syllable in FULL_PINYIN_SYLLABLES {
            let key = SyllableKey::from_text(syllable).expect("complete syllable");
            assert_eq!(key.completeness(), Completeness::Complete);
        }
        for incomplete in INCOMPLETE_PINYIN_KEYS {
            let key = SyllableKey::from_text(incomplete).expect("initial-only key");
            assert_eq!(key.completeness(), Completeness::Partial);
            assert!(key.index() >= FULL_PINYIN_SYLLABLES.len());
        }
        for option_only in OPTION_ONLY_COMPLETE_SYLLABLES {
            let key = SyllableKey::from_canonical_text(option_only).expect("option-only key");
            assert_eq!(key.completeness(), Completeness::Complete);
            assert!(key.index() >= FULL_PINYIN_SYLLABLES.len() + INCOMPLETE_PINYIN_KEYS.len());
        }
    }

    #[test]
    fn unknown_spellings_have_no_key() {
        for text in ["", "NI", "ni2", "qqq", "\u{4f60}", "lv1"] {
            assert_eq!(SyllableKey::from_text(text), None, "text: {text:?}");
        }
    }

    #[test]
    fn option_text_resolves_correction_aliases_under_their_bits() {
        use crate::{PINYIN_CORRECT_GN_NG, PINYIN_CORRECT_MG_NG};

        let gn = OptionBits::from_bits(PINYIN_CORRECT_GN_NG);
        let mg = OptionBits::from_bits(PINYIN_CORRECT_MG_NG);
        assert_eq!(
            SyllableKey::from_option_text("agn", gn).map(SyllableKey::text),
            Some("ang")
        );
        assert_eq!(
            SyllableKey::from_option_text("amg", mg).map(SyllableKey::text),
            Some("ang")
        );
        assert_eq!(
            SyllableKey::from_option_text("amg", OptionBits::default()),
            None
        );
    }

    #[test]
    fn phrase_entries_carry_a_token_and_text() {
        let entry = PhraseEntry::new(PhraseToken::new(7), "\u{4f60}\u{597d}".to_owned());
        assert_eq!(entry.token().value(), 7);
        assert_eq!(entry.text(), "\u{4f60}\u{597d}");
        assert_eq!(entry.clone().into_text(), "\u{4f60}\u{597d}");
        assert_eq!(entry.token().to_string(), "7");
    }
}
