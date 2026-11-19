//! The system data directory's file names for the compiled-in backend.
//!
//! libpinyin names its files in `src/pinyin_internal.h` (`pinyin_index.bin`,
//! `phrase_index.bin`, `bigram.db`, `punct.bin`, the `addon_*` pair) and its
//! per-library chunk files in `table.conf`. Those are the native names on
//! all three backends — Berkeley DB, Kyoto Cabinet and tkrzw — and the
//! names the runtime opens, the same ones a libpinyin build on that
//! backend opens.
//!
//! The chunk files are backend-independent (`MemoryChunk` on every build
//! of libpinyin) and keep their names everywhere.

/// One of the six DBM files of a system data directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemDbm {
    /// `ChewingLargeTable2` over the system libraries (`pinyin_index.bin`).
    PinyinIndex,
    /// `PhraseLargeTable3` over the system libraries (`phrase_index.bin`).
    PhraseIndex,
    /// The system `Bigram` (`bigram.db`) — a hash container.
    Bigram,
    /// `PunctTable` (`punct.bin`).
    Punct,
    /// `ChewingLargeTable2` over the addon libraries
    /// (`addon_pinyin_index.bin`).
    AddonPinyinIndex,
    /// `PhraseLargeTable3` over the addon libraries
    /// (`addon_phrase_index.bin`).
    AddonPhraseIndex,
}

impl SystemDbm {
    /// The name libpinyin gives the file (`pinyin_internal.h:57-66`).
    #[must_use]
    pub const fn libpinyin_name(self) -> &'static str {
        match self {
            Self::PinyinIndex => "pinyin_index.bin",
            Self::PhraseIndex => "phrase_index.bin",
            Self::Bigram => "bigram.db",
            Self::Punct => "punct.bin",
            Self::AddonPinyinIndex => "addon_pinyin_index.bin",
            Self::AddonPhraseIndex => "addon_phrase_index.bin",
        }
    }

    /// The file name: libpinyin's own, the same on Kyoto Cabinet, tkrzw and
    /// Berkeley DB (`pinyin_internal.h:57-66` does not vary with the DBM).
    #[must_use]
    pub fn file_name(self) -> String {
        self.libpinyin_name().to_owned()
    }
}

// Default SYSTEM_FILE entries, data/table.conf.in:8–28.
/// Stock system-library indices and base names. An installed configuration can select
/// other rows.
pub const SYSTEM_LIBRARY_NAMES: &[(u8, &str)] = &[
    (1, "gb_char"),
    (2, "gbk_char"),
    (3, "opengram"),
    (4, "merged"),
];

// Addon rows, data/table.conf.in:8–28.
/// Stock addon-library indices and base names. See [`SYSTEM_LIBRARY_NAMES`] for the
/// default dictionary’s stock libraries.
pub const ADDON_LIBRARY_NAMES: &[(u8, &str)] = &[
    (4, "art"),
    (5, "culture"),
    (6, "economy"),
    (7, "geology"),
    (8, "history"),
    (9, "life"),
    (10, "nature"),
    (11, "people"),
    (12, "science"),
    (13, "society"),
    (14, "sport"),
    (15, "technology"),
];

/// The four system libraries' chunk files by nibble — `table.conf`'s
/// `default …_DICTIONARY` rows' system files.
///
/// `<name>.bin` for each of [`SYSTEM_LIBRARY_NAMES`], spelled out because
/// a `const` cannot concatenate; the test below holds the two in step.
#[cfg(test)]
pub const SYSTEM_LIBRARY_FILES: &[(u8, &str)] = &[
    (1, "gb_char.bin"),
    (2, "gbk_char.bin"),
    (3, "opengram.bin"),
    (4, "merged.bin"),
];

/// The twelve addon libraries' chunk files by addon index — `table.conf`'s
/// `addon N …` rows.
///
/// Addon indexes share the nibble space with the system
/// libraries (art is 4, like merged) but live in a second facade upstream
/// (`m_addon_phrase_index`), so they never collide.
///
/// `<name>.bin` for each of [`ADDON_LIBRARY_NAMES`], spelled out because
/// a `const` cannot concatenate; the test below holds the two in step.
#[cfg(test)]
pub const ADDON_LIBRARY_FILES: &[(u8, &str)] = &[
    (4, "art.bin"),
    (5, "culture.bin"),
    (6, "economy.bin"),
    (7, "geology.bin"),
    (8, "history.bin"),
    (9, "life.bin"),
    (10, "nature.bin"),
    (11, "people.bin"),
    (12, "science.bin"),
    (13, "society.bin"),
    (14, "sport.bin"),
    (15, "technology.bin"),
];

#[cfg(test)]
mod tests;
