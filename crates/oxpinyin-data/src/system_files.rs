//! The system data directory's file names for the compiled-in backend.
//!
//! libpinyin names its files in `src/pinyin_internal.h` (`pinyin_index.bin`,
//! `phrase_index.bin`, `bigram.db`, `punct.bin`, the `addon_*` pair) and its
//! per-library chunk files in `table.conf`. On a backend libpinyin itself
//! builds against — Kyoto Cabinet or tkrzw — those are the names the
//! runtime opens, so an unmodified libpinyin install's `data/` is the
//! runtime's input. On redb and LMDB the same records live in that
//! backend's own container under `<stem>.<ext>`.
//!
//! The chunk files are backend-independent (`MemoryChunk` on every build
//! of libpinyin) and keep their names everywhere.

use oxpinyin_store::{DEFAULT_STORE_EXT, DEFAULT_STORE_IS_LIBPINYIN_DBM};

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
    /// The file's base name without any extension.
    #[must_use]
    pub const fn stem(self) -> &'static str {
        match self {
            Self::PinyinIndex => "pinyin_index",
            Self::PhraseIndex => "phrase_index",
            Self::Bigram => "bigram",
            Self::Punct => "punct",
            Self::AddonPinyinIndex => "addon_pinyin_index",
            Self::AddonPhraseIndex => "addon_phrase_index",
        }
    }

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

    /// The file name for the compiled-in backend: libpinyin's own on
    /// Kyoto Cabinet and tkrzw, `<stem>.<ext>` on redb and LMDB.
    #[must_use]
    pub fn file_name(self) -> String {
        if DEFAULT_STORE_IS_LIBPINYIN_DBM {
            self.libpinyin_name().to_owned()
        } else {
            format!("{}.{DEFAULT_STORE_EXT}", self.stem())
        }
    }

    /// Whether the file is a hash container (`bigram.db` is a KC HashDB /
    /// tkrzw HashDBM; every other DBM is a tree).
    #[must_use]
    pub const fn is_hash(self) -> bool {
        matches!(self, Self::Bigram)
    }
}

/// The four system libraries' chunk files by nibble — `table.conf`'s
/// `default …_DICTIONARY` rows' system files.
pub const SYSTEM_LIBRARY_FILES: &[(u8, &str)] = &[
    (1, "gb_char.bin"),
    (2, "gbk_char.bin"),
    (3, "opengram.bin"),
    (4, "merged.bin"),
];

/// The twelve addon libraries' chunk files by addon index — `table.conf`'s
/// `addon N …` rows. Addon indexes share the nibble space with the system
/// libraries (art is 4, like merged) but live in a second facade upstream
/// (`m_addon_phrase_index`), so they never collide.
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

/// The chunk file of addon library `index`, if `table.conf` names one.
#[must_use]
pub fn addon_library_file(index: u8) -> Option<&'static str> {
    ADDON_LIBRARY_FILES
        .iter()
        .find(|(nibble, _)| *nibble == index)
        .map(|(_, file)| *file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_backend() {
        let name = SystemDbm::Bigram.file_name();
        if DEFAULT_STORE_IS_LIBPINYIN_DBM {
            assert_eq!(name, "bigram.db");
            assert_eq!(SystemDbm::PinyinIndex.file_name(), "pinyin_index.bin");
        } else {
            assert_eq!(name, format!("bigram.{DEFAULT_STORE_EXT}"));
        }
        assert!(SystemDbm::Bigram.is_hash());
        assert!(!SystemDbm::Punct.is_hash());
        assert_eq!(addon_library_file(4), Some("art.bin"));
        assert_eq!(addon_library_file(15), Some("technology.bin"));
        assert_eq!(addon_library_file(3), None);
    }
}
