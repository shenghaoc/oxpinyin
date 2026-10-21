//! Zhuyin indexes for the bopomofo parser: the pinned global
//! `zhuyin_index` (Simple keyboards), the Discrete keyboards'
//! `hsu_zhuyin_index`/`eten26_zhuyin_index`, and the per-key
//! zhuyin-validity tone masks.
//!
//! All tables are extracted from libpinyin `2.11.91`
//! (`0c5e80e1200f84fab185d1c5bde458b770a0636c`): the index rows from
//! `pinyin_parser_table.h` joined to canonical spellings through
//! `content_table[]`, the masks from `valid_zhuyin_table`
//! (`src/storage/zhuyin_table.h:491`) indexed through each content
//! row's `ChewingKey` triple. Source-embedded tables, copyable under
//! the project source policy; not model-archive data.

use crate::vocab::SYLLABLE_KEY_COUNT;

/// A decoded view of one pinned zhuyin spelling index.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ZhuyinIndex {
    strings: oxpinyin_chewing::StringTable<3>,
    flags: &'static [u32],
}

impl ZhuyinIndex {
    pub(crate) fn position(self, spelling: &str) -> Option<usize> {
        self.strings.position(spelling)
    }

    pub(crate) fn get(
        self,
        index: usize,
    ) -> Option<(&'static str, &'static str, &'static str, u32)> {
        let [input, canonical, pinyin] = self.strings.get(index)?;
        Some((input, canonical, pinyin, *self.flags.get(index)?))
    }

    #[must_use]
    pub(crate) fn entries(
        self,
    ) -> impl ExactSizeIterator<Item = (&'static str, &'static str, &'static str, u32)>
    + DoubleEndedIterator
    + Clone {
        self.strings
            .entries()
            .zip(self.flags)
            .map(|([input, canonical, pinyin], &flags)| (input, canonical, pinyin, flags))
    }
}

// Keep the generator's row counts checked in non-test builds too.
const _: [(); ZHUYIN_PINYIN_COUNT] = [(); crate::string_tables::ZHUYIN_PINYIN_MAP_FLAGS.len()];
const _: [(); HSU_ZHUYIN_COUNT] = [(); crate::string_tables::HSU_ZHUYIN_INDEX_FLAGS.len()];
const _: [(); ETEN26_ZHUYIN_COUNT] = [(); crate::string_tables::ETEN26_ZHUYIN_INDEX_FLAGS.len()];

/// Number of entries in [`zhuyin_pinyin_map`]: 417 plain + 1062
/// `ZHUYIN_CORRECT_SHUFFLE` + 14 `ZHUYIN_INCOMPLETE` rows, 1:1 with the
/// pinned `zhuyin_index`.
pub const ZHUYIN_PINYIN_COUNT: usize = 1493;

/// `(input spelling, canonical spelling, canonical pinyin, row flags)`
/// quadruples, sorted by input spelling bytes — the binary-search
/// invariant of the pinned table.
///
/// The canonical spelling is `content_table[idx].m_chewing` — what
/// upstream's `ChewingKey::get_zhuyin_string` renders, so a shuffled
/// input (ㄅㄝㄧ) displays as its canonical spelling (ㄅㄧㄝ) exactly like
/// the oracle's auxiliary text. Plain and incomplete rows carry the input
/// spelling back; only shuffle rows differ.
///
/// Row flags are the upstream option bits consulted by
/// `check_chewing_options` (`src/storage/zhuyin_parser2.cpp:43-63`):
/// `ZHUYIN_INCOMPLETE` rows parse only under that option,
/// `ZHUYIN_CORRECT_SHUFFLE` rows only under theirs. The `IS_ZHUYIN`
/// (always present) and `PINYIN_INCOMPLETE` (pinyin-surface bookkeeping)
/// bits are structural upstream and gate nothing here, so they are not
/// carried.
#[must_use]
pub(crate) const fn zhuyin_pinyin_map() -> ZhuyinIndex {
    ZhuyinIndex {
        strings: crate::string_tables::zhuyin_pinyin_map(),
        flags: &crate::string_tables::ZHUYIN_PINYIN_MAP_FLAGS,
    }
}

/// Tone-validity bitmask per [`crate::SyllableKey`] id.
///
/// Bit `t` is set iff `_ChewingKey::is_valid_zhuyin` is true for that key
/// at `m_tone == t` — the compact form of the pinned `valid_zhuyin_table`
/// (`src/storage/zhuyin_table.h:491`) evaluated at every content-row
/// `ChewingKey` triple of the 440-key inventory. The 405 full-pinyin
/// values are unchanged from the first W13 table; ids `405..428` (the
/// initial-only keys, all 14 zhuyin rows among them parse-dead) and the
/// five dead zhuyin-only spellings are genuinely zero.
pub static VALID_ZHUYIN_TONES: [u8; SYLLABLE_KEY_COUNT] = [
    0x3f, 0x3f, 0x1f, 0x1f, 0x1f, 0x3f, 0x1f, 0x1b, 0x1b, 0x1f, 0x3b, 0x1b, 0x1f, 0x1f, 0x1b, 0x1b,
    0x1f, 0x1b, 0x1b, 0x3f, 0x3f, 0x1b, 0x1f, 0x1f, 0x1f, 0x1f, 0x11, 0x07, 0x1f, 0x1f, 0x1f, 0x11,
    0x17, 0x17, 0x1f, 0x1f, 0x1f, 0x1f, 0x1f, 0x1f, 0x1f, 0x1f, 0x1b, 0x1f, 0x1f, 0x1f, 0x1f, 0x1f,
    0x3f, 0x1d, 0x1f, 0x1f, 0x17, 0x0f, 0x13, 0x3f, 0x1b, 0x1b, 0x3b, 0x1f, 0x25, 0x09, 0x1b, 0x3f,
    0x00, 0x1b, 0x1b, 0x37, 0x1b, 0x03, 0x1b, 0x1b, 0x3f, 0x1b, 0x1b, 0x1b, 0x3f, 0x1f, 0x13, 0x1b,
    0x1f, 0x1f, 0x1f, 0x3f, 0x1f, 0x1f, 0x1f, 0x05, 0x1f, 0x1f, 0x3f, 0x1b, 0x1b, 0x1b, 0x1b, 0x3f,
    0x09, 0x1f, 0x1b, 0x3b, 0x1b, 0x1f, 0x1b, 0x1b, 0x1b, 0x1b, 0x1b, 0x19, 0x3f, 0x0f, 0x1f, 0x1f,
    0x1f, 0x1f, 0x17, 0x0b, 0x1d, 0x37, 0x1f, 0x3f, 0x1f, 0x17, 0x15, 0x1f, 0x3f, 0x1f, 0x1f, 0x3f,
    0x1f, 0x3f, 0x1b, 0x1b, 0x1f, 0x3f, 0x1b, 0x1b, 0x1b, 0x3b, 0x1f, 0x1b, 0x1f, 0x1b, 0x1b, 0x1b,
    0x1b, 0x1f, 0x1b, 0x1f, 0x19, 0x0b, 0x1b, 0x1b, 0x1b, 0x1b, 0x1b, 0x0b, 0x1f, 0x1f, 0x1b, 0x19,
    0x3f, 0x1d, 0x1d, 0x3f, 0x1f, 0x31, 0x1f, 0x1d, 0x3f, 0x09, 0x1d, 0x1d, 0x1f, 0x1f, 0x1d, 0x1f,
    0x3f, 0x21, 0x1d, 0x3f, 0x3f, 0x1d, 0x1f, 0x1f, 0x1d, 0x11, 0x3f, 0x1d, 0x1d, 0x0d, 0x1f, 0x21,
    0x3d, 0x3f, 0x1f, 0x1f, 0x1d, 0x1f, 0x13, 0x0d, 0x1d, 0x13, 0x3f, 0x1d, 0x1d, 0x3d, 0x1d, 0x1f,
    0x3d, 0x1f, 0x37, 0x19, 0x11, 0x1d, 0x1d, 0x1d, 0x15, 0x19, 0x17, 0x0d, 0x1d, 0x1f, 0x00, 0x3d,
    0x1d, 0x1d, 0x09, 0x1d, 0x19, 0x11, 0x0f, 0x1f, 0x37, 0x1f, 0x1f, 0x1f, 0x1f, 0x1f, 0x1f, 0x1f,
    0x1f, 0x1f, 0x1f, 0x1b, 0x1f, 0x1f, 0x1f, 0x0f, 0x1f, 0x1f, 0x1b, 0x1f, 0x1f, 0x1f, 0x1f, 0x1f,
    0x1f, 0x0f, 0x0f, 0x3f, 0x1f, 0x17, 0x07, 0x0d, 0x1f, 0x1d, 0x19, 0x1d, 0x1f, 0x13, 0x1d, 0x1d,
    0x1d, 0x0d, 0x1d, 0x17, 0x11, 0x3b, 0x13, 0x1b, 0x1b, 0x1b, 0x11, 0x03, 0x03, 0x1b, 0x1f, 0x1b,
    0x17, 0x1b, 0x1f, 0x1b, 0x3f, 0x1f, 0x1b, 0x1b, 0x3b, 0x1f, 0x1f, 0x05, 0x3f, 0x1f, 0x3f, 0x1f,
    0x3f, 0x1b, 0x1b, 0x13, 0x1b, 0x1d, 0x19, 0x13, 0x3b, 0x17, 0x1f, 0x1f, 0x3f, 0x11, 0x05, 0x1f,
    0x1f, 0x1f, 0x1b, 0x1f, 0x1f, 0x3f, 0x1f, 0x1f, 0x1f, 0x1f, 0x1f, 0x3f, 0x1b, 0x1f, 0x1f, 0x1f,
    0x1f, 0x1b, 0x1b, 0x1f, 0x3f, 0x1f, 0x1f, 0x1f, 0x1f, 0x3f, 0x1f, 0x1f, 0x17, 0x1b, 0x1f, 0x1f,
    0x1f, 0x17, 0x3f, 0x1f, 0x3f, 0x1f, 0x3f, 0x1f, 0x1f, 0x1f, 0x23, 0x1f, 0x1f, 0x1f, 0x1f, 0x13,
    0x1f, 0x0f, 0x3b, 0x1f, 0x1b, 0x1f, 0x1d, 0x05, 0x19, 0x13, 0x3b, 0x1b, 0x1b, 0x1f, 0x1b, 0x1b,
    0x1b, 0x1f, 0x3f, 0x1f, 0x1b, 0x1b, 0x1f, 0x3f, 0x1b, 0x1b, 0x1f, 0x1b, 0x1f, 0x1f, 0x0b, 0x1b,
    0x1b, 0x1b, 0x1b, 0x1b, 0x17, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x15, 0x0b, 0x11,
    0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x05, 0x11,
];

/// Number of entries in [`hsu_zhuyin_index`].
pub const HSU_ZHUYIN_COUNT: usize = 500;

/// The pinned `hsu_zhuyin_index[]` (`pinyin_parser_table.h:2988`),
/// joined to canonical spellings through `content_table`:
/// 417 plain rows (identical to the global index) +
/// 78 `ZHUYIN_CORRECT_HSU` keyboard-remap rows + 5
/// `ZHUYIN_INCOMPLETE` rows, gated by the same
/// `check_chewing_options` rule with the keyboard's forced
/// correction bit.
#[must_use]
pub(crate) const fn hsu_zhuyin_index() -> ZhuyinIndex {
    ZhuyinIndex {
        strings: crate::string_tables::hsu_zhuyin_index(),
        flags: &crate::string_tables::HSU_ZHUYIN_INDEX_FLAGS,
    }
}

/// Number of entries in [`eten26_zhuyin_index`].
pub const ETEN26_ZHUYIN_COUNT: usize = 482;

/// The pinned `eten26_zhuyin_index[]` (`pinyin_parser_table.h:3491`),
/// joined to canonical spellings through `content_table`:
/// 417 plain rows (identical to the global index) +
/// 59 `ZHUYIN_CORRECT_ETEN26` keyboard-remap rows + 6
/// `ZHUYIN_INCOMPLETE` rows, gated by the same
/// `check_chewing_options` rule with the keyboard's forced
/// correction bit.
#[must_use]
pub(crate) const fn eten26_zhuyin_index() -> ZhuyinIndex {
    ZhuyinIndex {
        strings: crate::string_tables::eten26_zhuyin_index(),
        flags: &crate::string_tables::ETEN26_ZHUYIN_INDEX_FLAGS,
    }
}

/// The canonical Zhuyin spelling of a full-pinyin syllable, or `None` when
/// the pinned map holds no row for it.
///
/// Upstream renders one `ChewingKey` either way — `get_pinyin_string` and
/// `get_zhuyin_string` are two views of the same initial/middle/final
/// triple (`chewing_key.cpp:47-89`). Here the two spellings are related
/// through the pinned Zhuyin index instead, so this is the lookup that
/// stands in for `get_zhuyin_string` on a key parsed as pinyin.
///
/// The map is keyed by *input* spelling and carries shuffle rows whose
/// input differs from their canonical form, so the canonical row for a
/// pinyin is the one whose input equals its canonical spelling — the same
/// row `ChewingKey::get_zhuyin_string` would render.
#[must_use]
pub fn zhuyin_for_pinyin(pinyin: &str) -> Option<&'static str> {
    zhuyin_pinyin_map()
        .entries()
        .find(|(input, canonical, mapped, _)| *mapped == pinyin && input == canonical)
        .map(|(_, canonical, _, _)| canonical)
}

/// A full-pinyin syllable rendered as Zhuyin with its tone — the pin's
/// `ChewingKey::get_zhuyin_string` (`chewing_key.cpp:74-89`).
///
/// The zero tone and tone 1 render the bare spelling ("for first tone,
/// usually not display it"); tones 2..5 append their mark from the same
/// table the Zhuyin parser uses, so there is one tone table in the crate
/// rather than two that can drift.
#[must_use]
pub fn zhuyin_display_for_pinyin(pinyin: &str, tone: u8) -> Option<String> {
    let spelling = zhuyin_for_pinyin(pinyin)?;
    if (2..=5).contains(&tone) {
        return Some(format!("{spelling}{}", crate::scheme::tone_symbol(tone)));
    }
    Some(spelling.to_owned())
}

#[cfg(test)]
mod identity_tests {
    #[test]
    fn zhuyin_pinyin_map_matches_original_rows() {
        assert!(
            super::zhuyin_pinyin_map()
                .entries()
                .eq(super::reference::ZHUYIN_PINYIN_MAP)
        );
    }

    #[test]
    fn hsu_zhuyin_index_matches_original_rows() {
        assert!(
            super::hsu_zhuyin_index()
                .entries()
                .eq(super::reference::HSU_ZHUYIN_INDEX)
        );
    }

    #[test]
    fn eten26_zhuyin_index_matches_original_rows() {
        assert!(
            super::eten26_zhuyin_index()
                .entries()
                .eq(super::reference::ETEN26_ZHUYIN_INDEX)
        );
    }
}

#[cfg(test)]
mod reference {
    use super::{ETEN26_ZHUYIN_COUNT, HSU_ZHUYIN_COUNT, ZHUYIN_PINYIN_COUNT};
    use crate::options::{
        ZHUYIN_CORRECT_ETEN26, ZHUYIN_CORRECT_HSU, ZHUYIN_CORRECT_SHUFFLE, ZHUYIN_INCOMPLETE,
    };
    macro_rules! zhuyin_index {
        ($name:ident; $count:ident; $rows:expr) => {
            pub static $name: [(&str, &str, &str, u32); $count] = $rows;
        };
    }
    include!("table_data/zhuyin_indexes.rs");
}
