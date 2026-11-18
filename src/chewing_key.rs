//! The upstream `ChewingKey` value and its display surface.
//!
//! `_ChewingKey` (`src/storage/chewing_key.h:41-48`) packs four bitfields
//! into one `guint16` — initial 5 bits, middle 2, final 5, tone 3, one
//! zero padding bit. This module carries the same four elements unpacked,
//! renders them through the ported `content_table` / `chewing_key_table`
//! ([`crate::chewing_key_data`]), and provides the packed two-byte form
//! the C ABI hands across the boundary ([`ChewingKey::to_packed`]).
//!
//! The renderers mirror `_ChewingKey::get_*_string`
//! (`src/storage/chewing_key.cpp:34-134`): `content_table` lookup by
//! table index, tone appended per renderer (digits for pinyin / luoma /
//! secondary zhuyin, tone marks for zhuyin, bare for shengmu / yunmu).
//! Upstream asserts the initial, middle and final against the table
//! dimensions (`get_table_index`, `chewing_key.cpp:28-32`) and the tone
//! `< CHEWING_NUMBER_OF_TONES` in the renderers. Hand-crafted bit patterns
//! that violate those preconditions render gracefully here — zero table
//! index — instead of aborting; the C adapters abort on such keys before
//! they render (register row 120).

use crate::chewing_key_data::{
    CHEWING_KEY_TABLE, CONTENT_KEYS, NUM_FINALS, NUM_INITIALS, NUM_MIDDLES, content_strings,
    spelling_key,
};

/// One pinyin key: the upstream `_ChewingKey` elements unpacked.
///
/// The element values are the upstream `ChewingInitial` / `ChewingMiddle`
/// / `ChewingFinal` / `ChewingTone` enum values (`chewing_enum.h`).
/// [`ChewingKey::from_pinyin`] builds a key from a full-pinyin spelling.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChewingKey {
    /// `m_initial` — a `ChewingInitial` value.
    pub initial: u8,
    /// `m_middle` — a `ChewingMiddle` value.
    pub middle: u8,
    /// `m_final` — a `ChewingFinal` value. (`final` is reserved.)
    pub final_: u8,
    /// `m_tone` — a `ChewingTone` value.
    pub tone: u8,
}

impl ChewingKey {
    /// The zero key: every element `CHEWING_ZERO_*`.
    #[cfg(any(test, feature = "test-hooks"))]
    pub const ZERO: Self = Self {
        initial: 0,
        middle: 0,
        final_: 0,
        tone: 0,
    };

    /// Builds a key from upstream element values.
    #[must_use]
    pub const fn new(initial: u8, middle: u8, final_: u8, tone: u8) -> Self {
        Self {
            initial,
            middle,
            final_,
            tone,
        }
    }

    /// Whether `pinyin_compare_with_tones` answers 0 for this query key
    /// against one `stored` key (`src/storage/pinyin_phrase3.h:108-144` at
    /// 074a2219): the per-syllable test of the phrase-table searches and the
    /// pronunciation match.
    ///
    /// The pin makes three passes over the phrase (initials, then middle and
    /// final pairs, then tones) and returns at the first non-zero
    /// difference. It answers 0 only when every position passes all three
    /// tests, so a multi-key comparison is this test at each position:
    ///
    /// * `pinyin_compare_initial3` (`:68-71`): equal initials;
    /// * `pinyin_compare_middle_and_final3` (`:73-94`): equal middle and
    ///   final, or a zero middle and final on either side, so an incomplete
    ///   key matches any final;
    /// * `pinyin_compare_tone3` (`:96-105`): equal tones, or a zero tone on
    ///   either side.
    #[must_use]
    pub const fn matches_with_tones(self, stored: Self) -> bool {
        self.initial == stored.initial
            && ((self.middle == stored.middle && self.final_ == stored.final_)
                || (self.middle == 0 && self.final_ == 0)
                || (stored.middle == 0 && stored.final_ == 0))
            && (self.tone == stored.tone || self.tone == 0 || stored.tone == 0)
    }

    /// The key with `m_tone` set to `tone`.
    #[must_use]
    pub const fn with_tone(mut self, tone: u8) -> Self {
        self.tone = tone;
        self
    }

    /// Whether the initial, middle and final lie inside the table
    /// dimensions that `_ChewingKey::get_table_index` asserts
    /// (`chewing_key.cpp:28-32`). A key packed from caller memory can carry
    /// more (initial 24..=31, final 18..=31).
    #[must_use]
    pub const fn elements_in_dims(self) -> bool {
        (self.initial as usize) < NUM_INITIALS
            && (self.middle as usize) < NUM_MIDDLES
            && (self.final_ as usize) < NUM_FINALS
    }

    /// Whether the tone is below `CHEWING_NUMBER_OF_TONES` (6), which the
    /// pin's string renderers assert (`chewing_key.cpp:40`, `:75`, `:92`,
    /// `:105`). The packed tone field holds 0..=7.
    #[must_use]
    pub const fn tone_in_dims(self) -> bool {
        self.tone < 6
    }

    /// `_ChewingKey::get_table_index` (`chewing_key.cpp:27-35`): the
    /// `content_table` row for this key's elements, 0 for the zero key
    /// and for every combination the table marks invalid (`-1`).
    ///
    /// Upstream asserts the elements against the table dimensions; a
    /// hand-crafted out-of-dims key answers 0 here instead, and the C
    /// adapters abort on it (register row 120).
    #[must_use]
    pub fn table_index(self) -> usize {
        let initial = usize::from(self.initial);
        let middle = usize::from(self.middle);
        let final_ = usize::from(self.final_);
        if initial >= NUM_INITIALS || middle >= NUM_MIDDLES || final_ >= NUM_FINALS {
            return 0;
        }
        let entry = CHEWING_KEY_TABLE[(initial * NUM_MIDDLES + middle) * NUM_FINALS + final_];
        usize::try_from(entry).unwrap_or(0)
    }

    /// One display field of the key (empty for the zero and invalid keys).
    fn content_string(self, field: usize) -> &'static str {
        content_strings()
            .field(self.table_index(), field)
            .unwrap_or_default()
    }

    /// The key for a full-pinyin spelling, tone zero.
    #[must_use]
    pub fn from_pinyin(text: &str) -> Option<Self> {
        let spelling = spelling_key(text)?;
        let position = CONTENT_KEYS
            .binary_search_by(|row| row.spelling.cmp(&spelling))
            .ok()?;
        let row = CONTENT_KEYS.get(position)?;
        Some(Self {
            initial: row.initial,
            middle: row.middle,
            final_: row.final_,
            tone: CHEWING_ZERO_TONE,
        })
    }

    /// The packed two-byte form the C ABI carries: the `guint16` bitfield
    /// (initial `0..5`, middle `5..7`, final `7..12`, tone `12..15`,
    /// zero padding `15`), matching `_ChewingKey`'s declaration order
    /// under the little-endian bitfield allocation both engines build
    /// with. Byte identity against the pinned oracle is held by the frozen key
    /// cases in `tests/it/keys.rs` of `oxpinyin-capi`.
    #[must_use]
    pub const fn to_packed(self) -> u16 {
        ((self.initial as u16) & 0x1f)
            | (((self.middle as u16) & 0x3) << 5)
            | (((self.final_ as u16) & 0x1f) << 7)
            | (((self.tone as u16) & 0x7) << 12)
    }

    /// Unpacks [`ChewingKey::to_packed`] form; the padding bit is dropped.
    #[must_use]
    pub const fn from_packed(bits: u16) -> Self {
        Self {
            initial: (bits & 0x1f) as u8,
            middle: ((bits >> 5) & 0x3) as u8,
            final_: ((bits >> 7) & 0x1f) as u8,
            tone: ((bits >> 12) & 0x7) as u8,
        }
    }

    /// Canonical pinyin spelling of this key.
    #[must_use]
    pub fn text(self) -> &'static str {
        self.pinyin_spelling()
    }

    /// The row's canonical spelling without tone.
    #[must_use]
    pub fn pinyin_spelling(self) -> &'static str {
        self.content_string(0)
    }

    /// `_ChewingKey::get_pinyin_string` (`chewing_key.cpp:47-58`): the
    /// canonical spelling, tone digit appended for a non-zero tone.
    #[must_use]
    pub fn pinyin_string(self) -> String {
        self.pinyin_text().to_string()
    }

    /// How `_ChewingKey::get_pinyin_string` builds its result
    /// (`chewing_key.cpp:53-57`): `g_strdup` of the spelling for the zero
    /// tone, `g_strdup_printf("%s%d", …)` otherwise.
    #[must_use]
    pub fn pinyin_text(self) -> KeyText {
        digit_text(self.content_string(0), self.tone)
    }

    /// `_ChewingKey::get_shengmu_string` (`chewing_key.cpp:60-65`): the
    /// initial column; no tone.
    #[must_use]
    pub fn shengmu_string(self) -> &'static str {
        self.content_string(1)
    }

    /// `_ChewingKey::get_yunmu_string` (`chewing_key.cpp:67-72`): the
    /// middle+final column; no tone.
    #[must_use]
    pub fn yunmu_string(self) -> &'static str {
        self.content_string(2)
    }

    /// `_ChewingKey::get_zhuyin_string` (`chewing_key.cpp:74-89`): the
    /// zhuyin spelling; zero and first tones bare, tones 2..5 with their
    /// tone mark.
    #[must_use]
    pub fn zhuyin_string(self) -> String {
        self.zhuyin_text().to_string()
    }

    /// How `_ChewingKey::get_zhuyin_string` builds its result
    /// (`chewing_key.cpp:80-88`): `g_strdup` of the spelling for the zero
    /// and first tones, `g_strdup_printf("%s%s", …)` with the tone mark
    /// otherwise.
    #[must_use]
    pub fn zhuyin_text(self) -> KeyText {
        let base = self.content_string(3);
        match self.tone {
            2..=5 => KeyText::Mark(base, tone_symbol(self.tone)),
            _ => KeyText::Plain(base),
        }
    }

    /// `_ChewingKey::get_luoma_pinyin_string` (`chewing_key.cpp:91-105`):
    /// the luoma spelling, tone digit appended for a non-zero tone —
    /// including the first tone, unlike zhuyin — built as the pin builds
    /// it (`chewing_key.cpp:97-101`).
    #[must_use]
    pub fn luoma_pinyin_text(self) -> KeyText {
        digit_text(self.content_string(4), self.tone)
    }

    /// `_ChewingKey::get_secondary_zhuyin_string`
    /// (`chewing_key.cpp:107-121`): the secondary zhuyin spelling, tone
    /// digit appended for a non-zero tone — including the first tone —
    /// built as the pin builds it (`chewing_key.cpp:110-114`).
    #[must_use]
    pub fn secondary_zhuyin_text(self) -> KeyText {
        digit_text(self.content_string(5), self.tone)
    }
}

/// A key rendering as `_ChewingKey::get_*_string` builds it
/// (`chewing_key.cpp:47-121`): which GLib call the pin makes and with what
/// arguments. The C adapters make that call; [`std::fmt::Display`] writes
/// the same bytes for Rust callers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyText {
    /// `g_strdup(spelling)`.
    Plain(&'static str),
    /// `g_strdup_printf("%s%d", spelling, tone)`.
    Digit(&'static str, u8),
    /// `g_strdup_printf("%s%s", spelling, chewing_tone_table[tone])`.
    Mark(&'static str, &'static str),
}

impl std::fmt::Display for KeyText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plain(base) => f.write_str(base),
            Self::Digit(base, tone) => write!(f, "{base}{tone}"),
            Self::Mark(base, mark) => write!(f, "{base}{mark}"),
        }
    }
}

/// The pinyin-family branch: bare for the zero tone, `%s%d` otherwise. A
/// hand-crafted key with a two-digit tone renders without a suffix (see
/// [`digit`]).
const fn digit_text(base: &'static str, tone: u8) -> KeyText {
    match (tone, digit(tone)) {
        (CHEWING_ZERO_TONE, _) | (_, None) => KeyText::Plain(base),
        (_, Some(_)) => KeyText::Digit(base, tone),
    }
}

/// The tone digit appended by the pinyin-family renderers (upstream's
/// `%d`); tones above 5 are unreachable from the packed form. A
/// hand-crafted key with a two-digit tone renders without a suffix
/// rather than panicking on the `b'0' + tone` overflow.
const fn digit(tone: u8) -> Option<char> {
    match tone {
        0..=9 => Some((b'0' + tone) as char),
        _ => None,
    }
}

include!("tone_symbols.rs");

#[cfg(test)]
mod tests;
