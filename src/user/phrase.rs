//! User phrase-index types and the token-layout constants pinned in
//! `docs/findings/user-store.md` §3.
//!
//! New user-file phrases live in the `USER_DICTIONARY` or
//! `NETWORK_DICTIONARY` sub-index. Token allocation is "max token in the
//! sub-index + 1" (`range.m_range_end`, bumped past the reserved zero id).
//! This module names those constants and the value types the store records;
//! it does not talk to the store.

use crate::vocab::{
    PHRASE_MASK, SyllableKey, USER_DICTIONARY, phrase_index_library_index, phrase_index_make_token,
};

use crate::vocab::PhraseToken;

/// `count` used when the caller passes "default" (`pinyin.cpp:521`; §3.2).
pub const DEFAULT_PHRASE_COUNT: u64 = 5;

/// Unigram seed factor on a *new* user phrase (`pinyin.cpp:522`; §3.2).
/// Distinct from [`crate::user::UNIGRAM_FACTOR`] (7), which is the *training*
/// path.
pub const ADD_PHRASE_UNIGRAM_FACTOR: u64 = 3;

// PhraseItem::add_pronunciation copies tones with the parsed keys
// (phrase_index.cpp:56–91). Import reads tone under USE_TONE (pinyin.cpp:630).
/// A pronunciation key with a syllable identifier or tagged phonetic fields in the low 13
/// bits and the tone in the upper three. [`toned_key`] constructs this representation.
/// Use [`crate::parser::ChewingKey`] for the packed C key.
pub type PinyinKey = u16;

/// Bit position of the tone inside a [`PinyinKey`].
pub const KEY_TONE_SHIFT: u32 = 13;

/// The syllable-index bits of a [`PinyinKey`].
pub const KEY_SYLLABLE_MASK: PinyinKey = (1 << KEY_TONE_SHIFT) - 1;

/// `CHEWING_NUMBER_OF_TONES` less one: the highest tone a key carries
/// (`chewing_enum.h`, tones 0..=5).
pub const MAX_KEY_TONE: u8 = 5;

/// Marks syllable bits that are a packed `ChewingKey`'s initial, middle and
/// final fields, not a dense id: a key with no syllable-table row. The pin
/// appends such a fuzzy final alternate to the matrix with no table check
/// and stores it like any key (`phonetic_key_matrix.cpp:285-306`,
/// `pinyin.cpp:3594-3606`, `:3647-3663` at 074a2219). The dense ids end far
/// below the tag.
const KEY_PACKED_TAG: PinyinKey = 1 << 12;

/// The packed field bits under [`KEY_PACKED_TAG`].
const KEY_PACKED_FIELDS: PinyinKey = KEY_PACKED_TAG - 1;

/// `SyllableKey::index`'s tag for the same packed form.
const SYLLABLE_PACKED_TAG: usize = 0x8000;

/// Combines a syllable identifier or tagged matrix key with a tone in `0..=5`. The fields
/// must fit their representation. A representable index still needs a corresponding
/// inventory entry or usable packed fields to resolve.
#[must_use]
pub fn toned_key(syllable: usize, tone: u8) -> Option<PinyinKey> {
    let syllable = if syllable & SYLLABLE_PACKED_TAG == 0 {
        PinyinKey::try_from(syllable).ok()?
    } else {
        KEY_PACKED_TAG | (PinyinKey::try_from(syllable).ok()? & KEY_PACKED_FIELDS)
    };
    if syllable > KEY_SYLLABLE_MASK || tone > MAX_KEY_TONE {
        return None;
    }
    Some(syllable | (PinyinKey::from(tone) << KEY_TONE_SHIFT))
}

/// The `ChewingKey` that `key` names, tone included: a dense id's table
/// row, or the packed fields of a key with no row. `None` when the id names
/// no key, or the packed fields are zero or outside the table dimensions.
#[must_use]
pub(crate) fn key_chewing(key: PinyinKey) -> Option<crate::parser::ChewingKey> {
    let bits = key & KEY_SYLLABLE_MASK;
    let fields = if bits & KEY_PACKED_TAG == 0 {
        SyllableKey::from_index(usize::from(bits))?.chewing_key()
    } else {
        let packed = crate::parser::ChewingKey::from_packed(bits & KEY_PACKED_FIELDS);
        if bits & KEY_PACKED_FIELDS == 0 || !packed.elements_in_dims() {
            return None;
        }
        packed
    };
    Some(fields.with_tone(key_tone(key)))
}

/// The syllable index of `key`, tone stripped.
#[cfg(test)]
#[must_use]
pub const fn key_syllable(key: PinyinKey) -> usize {
    (key & KEY_SYLLABLE_MASK) as usize
}

/// The tone of `key` (0 = zero tone).
#[must_use]
pub const fn key_tone(key: PinyinKey) -> u8 {
    (key >> KEY_TONE_SHIFT) as u8
}

/// `_ChewingKey::get_pinyin_string` (`chewing_key.cpp:47-58`) over a
/// [`PinyinKey`]: the canonical spelling, the tone digit appended for a
/// non-zero tone. `None` when the syllable bits name no key or the tone
/// is out of range.
#[must_use]
pub fn key_pinyin_string(key: PinyinKey) -> Option<String> {
    let text = key_chewing(key)?.pinyin_spelling();
    match key_tone(key) {
        0 => Some(text.to_owned()),
        tone @ 1..=MAX_KEY_TONE => Some(format!("{text}{tone}")),
        _ => None,
    }
}

/// Whether `token` lives in [`USER_DICTIONARY`] — the same nibble test as
/// `pinyin_is_user_candidate` (`pinyin.cpp:3716-3719`, §3.2).
#[must_use]
pub const fn is_user_token(token: PhraseToken) -> bool {
    phrase_index_library_index(token) == USER_DICTIONARY
}

/// First token allocated in an empty user sub-index.
///
/// Empty `get_range` reports `m_range_end = 1` (`phrase_index.cpp:640-644`);
/// `FacadePhraseIndex` then wraps it with `MAKE_TOKEN(7, 1)`. The reserved-id
/// bump in `_add_phrase` (`pinyin.cpp:592-594`) is a no-op on that value
/// because the low 24 bits are already non-zero. Starting `range_end` at 0
/// would hit the bump and land on the same token.
#[cfg(test)]
pub const FIRST_USER_TOKEN: PhraseToken = phrase_index_make_token(USER_DICTIONARY, 1);

/// The token `_add_phrase` allocates from a sub-index whose `get_range` end
/// is `end` (`pinyin.cpp:589-594` at 074a2219). `FacadePhraseIndex::get_range`
/// masks the sub-index end into the library nibble (`phrase_index.cpp:620`,
/// `PHRASE_INDEX_MAKE_TOKEN`), and a zero low word is bumped past the
/// reserved id. Nothing checks for overflow: a library whose last slot is
/// live has the end `PHRASE_MASK + 1`, which wraps to slot 1. The addon promotion takes the masked end without the bump
/// (`pinyin.cpp:2541-2545`), so it wraps to slot 0.
#[must_use]
pub(crate) fn add_phrase_token(library: u8, end: u32) -> PhraseToken {
    let token = phrase_index_make_token(library, end);
    if token & PHRASE_MASK == 0 {
        token + 1
    } else {
        token
    }
}

/// A user-phrase pronunciation: the pinyin key sequence, its stored count,
/// and whether the user pinyin index carries it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserPronunciation {
    keys: Vec<PinyinKey>,
    count: u64,
    indexed: bool,
}

impl UserPronunciation {
    /// Key sequence for this reading.
    #[must_use]
    pub fn keys(&self) -> &[PinyinKey] {
        &self.keys
    }

    /// Pronunciation count (`add_pronunciation`'s `delta`, §3.2).
    #[must_use]
    pub const fn count(&self) -> u64 {
        self.count
    }

    /// Whether the user pinyin index carries this reading — the lookup
    /// finds the phrase under it. `_add_phrase` indexes the reading a new
    /// phrase is created with (`pinyin.cpp:597-600`) and never one added
    /// to an existing phrase (`pinyin.cpp:569-582`, `add_pronunciation`
    /// without `add_index`).
    #[cfg(any(test, feature = "test-hooks"))]
    #[must_use]
    pub const fn indexed(&self) -> bool {
        self.indexed
    }

    /// This reading rendered as `'`-joined `_ChewingKey::get_pinyin_string`
    /// spellings (e.g. `ni'hao`, `ce4'shi4`), or `None` when a stored key
    /// is not a valid [`SyllableKey`] id.
    ///
    /// The single rendering path the §9 phrase and bigram exports share, so
    /// an unrenderable key is skipped identically on both surfaces.
    #[must_use]
    pub(crate) fn render_pinyin(&self) -> Option<String> {
        self.keys
            .iter()
            .map(|key| key_pinyin_string(*key))
            .collect::<Option<Vec<_>>>()
            .map(|parts| parts.join("'"))
    }

    pub(crate) const fn new(keys: Vec<PinyinKey>, count: u64, indexed: bool) -> Self {
        Self {
            keys,
            count,
            indexed,
        }
    }
}

/// A user phrase with its library token, stored UCS-4 text and pronunciations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserPhrase {
    token: PhraseToken,
    text: Vec<u32>,
    pronunciations: Vec<UserPronunciation>,
}

impl UserPhrase {
    /// The allocated token, including the configured target library's index.
    #[cfg(any(test, feature = "test-hooks"))]
    #[must_use]
    pub const fn token(&self) -> PhraseToken {
        self.token
    }

    /// Exact stored words, including zero words and non-scalars.
    #[cfg(any(test, feature = "test-hooks"))]
    #[must_use]
    pub fn words(&self) -> &[u32] {
        &self.text
    }

    /// Text for lookup consumers, with the stored length and outward prefix.
    #[must_use]
    pub fn phrase_text(&self) -> Option<crate::vocab::PhraseText> {
        crate::vocab::PhraseText::from_stored_words(self.text.iter().copied().map(Some))
    }

    /// Pronunciations in insertion order — `add_pronunciation` appends
    /// (`phrase_index.cpp:86-91`).
    #[must_use]
    pub fn pronunciations(&self) -> &[UserPronunciation] {
        &self.pronunciations
    }

    pub(crate) const fn new(
        token: PhraseToken,
        text: Vec<u32>,
        pronunciations: Vec<UserPronunciation>,
    ) -> Self {
        Self {
            token,
            text,
            pronunciations,
        }
    }
}

#[cfg(test)]
mod tests;
