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
//! Upstream asserts tone `< CHEWING_NUMBER_OF_TONES` and indexes
//! `chewing_key_table` unguarded; hand-crafted bit patterns that violate
//! those preconditions render gracefully here — zero table index —
//! instead of aborting (the no-abort policy,
//! `docs/findings/upstream-divergences.md`).

use crate::chewing_key_data::{
    CHEWING_KEY_TABLE, CONTENT_TABLE, NUM_FINALS, NUM_INITIALS, NUM_MIDDLES,
};

/// `CHEWING_ZERO_TONE` (`src/storage/chewing_enum.h:92`).
pub const CHEWING_ZERO_TONE: u8 = 0;

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

    /// The key with `m_tone` set to `tone`.
    #[must_use]
    pub const fn with_tone(mut self, tone: u8) -> Self {
        self.tone = tone;
        self
    }

    /// `_ChewingKey::get_table_index` (`chewing_key.cpp:27-35`): the
    /// `content_table` row for this key's elements, 0 for the zero key
    /// and for every combination the table marks invalid (`-1`).
    ///
    /// Upstream asserts the elements against the table dimensions; a
    /// hand-crafted out-of-dims key answers 0 here instead (no-abort).
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

    /// The key's `content_table` row (row 0 for the zero and invalid keys,
    /// whose strings are all empty — upstream renders them the same way
    /// before the API-level `get_table_index() == 0` guard).
    #[must_use]
    pub fn content(self) -> &'static crate::chewing_key_data::ContentItem {
        &CONTENT_TABLE[self.table_index()]
    }

    /// The key for a full-pinyin spelling, tone zero.
    #[must_use]
    pub fn from_pinyin(text: &str) -> Option<Self> {
        let position = CONTENT_TABLE
            .binary_search_by(|row| row.pinyin.cmp(text))
            .ok()?;
        let row = &CONTENT_TABLE[position];
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
    /// with. Byte identity against the pinned oracle is verified by the
    /// key-surface differential (`tools/bisection/key-surface-diff.c`).
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

    /// The row's canonical spelling without the tone — what
    /// `_ChewingKey::get_pinyin_string` renders before its `%d`.
    #[must_use]
    pub fn pinyin_spelling(self) -> &'static str {
        self.content().pinyin
    }

    /// `_ChewingKey::get_pinyin_string` (`chewing_key.cpp:47-58`): the
    /// canonical spelling, tone digit appended for a non-zero tone.
    #[must_use]
    pub fn pinyin_string(self) -> String {
        let base = self.content().pinyin;
        if self.tone == CHEWING_ZERO_TONE {
            base.to_owned()
        } else {
            format!("{base}{}", digit(self.tone))
        }
    }

    /// `_ChewingKey::get_shengmu_string` (`chewing_key.cpp:60-65`): the
    /// initial column; no tone.
    #[must_use]
    pub fn shengmu_string(self) -> &'static str {
        self.content().shengmu
    }

    /// `_ChewingKey::get_yunmu_string` (`chewing_key.cpp:67-72`): the
    /// middle+final column; no tone.
    #[must_use]
    pub fn yunmu_string(self) -> &'static str {
        self.content().yunmu
    }

    /// `_ChewingKey::get_zhuyin_string` (`chewing_key.cpp:74-89`): the
    /// zhuyin spelling; zero and first tones bare, tones 2..5 with their
    /// tone mark.
    #[must_use]
    pub fn zhuyin_string(self) -> String {
        let base = self.content().zhuyin;
        match self.tone {
            CHEWING_ZERO_TONE | 1 => base.to_owned(),
            2..=5 => format!("{base}{}", tone_symbol(self.tone)),
            _ => base.to_owned(),
        }
    }

    /// `_ChewingKey::get_luoma_pinyin_string` (`chewing_key.cpp:91-105`):
    /// the luoma spelling, tone digit appended for a non-zero tone —
    /// including the first tone, unlike zhuyin.
    #[must_use]
    pub fn luoma_pinyin_string(self) -> String {
        let base = self.content().luoma;
        if self.tone == CHEWING_ZERO_TONE {
            base.to_owned()
        } else {
            format!("{base}{}", digit(self.tone))
        }
    }

    /// `_ChewingKey::get_secondary_zhuyin_string`
    /// (`chewing_key.cpp:107-121`): the secondary zhuyin spelling, tone
    /// digit appended for a non-zero tone — including the first tone.
    #[must_use]
    pub fn secondary_zhuyin_string(self) -> String {
        let base = self.content().secondary;
        if self.tone == CHEWING_ZERO_TONE {
            base.to_owned()
        } else {
            format!("{base}{}", digit(self.tone))
        }
    }
}

/// The tone digit appended by the pinyin-family renderers (upstream's
/// `%d`); tones above 5 are unreachable from the packed form.
const fn digit(tone: u8) -> char {
    (b'0' + tone) as char
}

/// `chewing_tone_table` (`src/storage/zhuyin_table.h:12593-12600`), the
/// zhuyin tone marks — the same table `crate::scheme` renders with.
const fn tone_symbol(tone: u8) -> &'static str {
    match tone {
        1 => " ",
        2 => "ˊ",
        3 => "ˇ",
        4 => "ˋ",
        5 => "˙",
        _ => "",
    }
}

#[cfg(test)]
mod chewing_key_data_tests {
    use super::ChewingKey;
    use crate::chewing_key_data::{CHEWING_KEY_TABLE, CONTENT_TABLE, NUM_FINALS, NUM_MIDDLES};

    /// Every content row's components resolve, through
    /// `chewing_key_table`, to the row's own index — the two ported
    /// tables cross-validate (row 0, the zero key, is exempt: its
    /// components are the all-zero key, which maps to row 0 anyway).
    #[test]
    fn tables_cross_validate() {
        for (index, row) in CONTENT_TABLE.iter().enumerate() {
            if index == 0 {
                continue;
            }
            let entry = CHEWING_KEY_TABLE[(usize::from(row.initial) * NUM_MIDDLES
                + usize::from(row.middle))
                * NUM_FINALS
                + usize::from(row.final_)];
            assert_eq!(entry, index as i16, "row {index} ({})", row.pinyin);
        }
    }

    /// The rows stay sorted by spelling, so `from_pinyin` may binary
    /// search; the zero row sorts first.
    #[test]
    fn content_rows_are_sorted() {
        for pair in CONTENT_TABLE.windows(2) {
            assert!(pair[0].pinyin <= pair[1].pinyin);
        }
        assert_eq!(CONTENT_TABLE[0].pinyin, "");
    }

    /// The zero key renders empty everywhere and answers table index 0;
    /// the packed zero key unpacks to zero.
    #[test]
    fn zero_key_renders_empty() {
        let key = ChewingKey::ZERO;
        assert_eq!(key.table_index(), 0);
        assert_eq!(key.pinyin_string(), "");
        assert_eq!(key.shengmu_string(), "");
        assert_eq!(key.yunmu_string(), "");
        assert_eq!(key.zhuyin_string(), "");
        assert_eq!(key.luoma_pinyin_string(), "");
        assert_eq!(key.secondary_zhuyin_string(), "");
        assert_eq!(key.to_packed(), 0);
    }

    /// Renderer spot checks against the pinned content rows: tone digit
    /// placement differs per renderer (zhuyin bare at tone 1, luoma and
    /// secondary zhuyin digit at tone 1).
    #[test]
    fn renderers_match_the_pinned_rows() {
        let zhang = ChewingKey::from_pinyin("zhang").expect("row");
        assert_eq!(zhang.shengmu_string(), "zh");
        assert_eq!(zhang.yunmu_string(), "ang");
        assert_eq!(zhang.zhuyin_string(), "ㄓㄤ");
        assert_eq!(zhang.luoma_pinyin_string(), "jhang");
        assert_eq!(zhang.secondary_zhuyin_string(), "jang");

        let zhang3 = zhang.with_tone(3);
        assert_eq!(zhang3.pinyin_string(), "zhang3");
        assert_eq!(zhang3.zhuyin_string(), "ㄓㄤˇ");
        assert_eq!(zhang3.luoma_pinyin_string(), "jhang3");
        assert_eq!(zhang3.secondary_zhuyin_string(), "jang3");

        let jong = ChewingKey::from_pinyin("zhong").expect("row");
        assert_eq!(jong.luoma_pinyin_string(), "jhong");
        assert_eq!(jong.secondary_zhuyin_string(), "jung");
        assert_eq!(
            jong.with_tone(1).luoma_pinyin_string(),
            "jhong1",
            "luoma appends the first-tone digit"
        );
        assert_eq!(
            jong.with_tone(1).secondary_zhuyin_string(),
            "jung1",
            "secondary zhuyin appends the first-tone digit"
        );
        assert_eq!(
            jong.with_tone(1).zhuyin_string(),
            "ㄓㄨㄥ",
            "zhuyin stays bare at the first tone"
        );

        let b = ChewingKey::from_pinyin("b").expect("initial-only row");
        assert_eq!(b.shengmu_string(), "b");
        assert_eq!(b.yunmu_string(), "");
        assert_eq!(b.zhuyin_string(), "ㄅ");
        assert_eq!(b.luoma_pinyin_string(), "None");
        assert_eq!(b.secondary_zhuyin_string(), "None");
    }
}
