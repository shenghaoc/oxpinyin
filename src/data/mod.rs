//! Reads the system dictionary, phrase libraries, bigram model, and punctuation tables.
//! Configuration selects the system libraries. Opening them maps and checks their phrase
//! chunks. Successor and index lookups read individual DBM rows. Extension searches can
//! walk a key range.
//!
//! The readers use [`crate::store`] for DBM access and [`crate::input`] for native I/O.

// ChewingLargeTable2, PhraseLargeTable3, FacadePhraseIndex, Bigram and PunctTable.
// Initialization: pinyin.cpp:331–396. See docs/findings/data-formats.md §§4–7.
mod bigram_table;
mod c_stderr;
pub(crate) mod chewing_table;
pub mod chunk_format;
mod dict;
#[cfg(test)]
pub(crate) use dict::tests as dict_tests;
mod library_set;
mod lm;
mod phrase_libraries;
pub mod phrase_library;
pub(crate) mod phrase_table;
mod punct;
pub mod row_format;
#[cfg(test)]
#[path = "../../tests/support/mod.rs"]
pub(crate) mod support;
mod system_files;
mod table;
#[cfg(test)]
mod table_entries;
mod table_info;
pub mod user_files;

pub use crate::model::UserCountDelta;
// The compiled-in backend's native-table extension and `database format:`
// token. This module reads the DBMs through `crate::store` directly; the
// re-export lets the consumers above it (`runtime`, the C crates, tests and
// fuzz targets) name fixture directories and check `table.conf` against the
// backend the build selected without naming `store` themselves.
pub use crate::store::DEFAULT_STORE_DB_FORMAT;
// Native row decoding for the pin test-program tools.
pub use chewing_table::index_key as chewing_index_key;

pub use bigram_table::BigramTable;
pub use library_set::LibrarySet;

pub use c_stderr::{path_bytes, table_conf_load_failed};

pub use dict::{AddonDictionary, DictError, SystemDictionary, SystemTables};
pub(crate) use dict::{SearchQuery, toned_query};

pub(crate) use lm::library_visible;
pub use lm::{BigramLanguageModel, BigramTables, LmError, merge_bigram};

pub use phrase_libraries::{LibraryPronunciation, PhraseLibraries};

pub use punct::PunctTable;

pub use system_files::{ADDON_LIBRARY_NAMES, SYSTEM_LIBRARY_NAMES, SystemDbm};

pub use table::TableError;

pub use table_info::{
    DatabaseFormat, FileName, FileType, PhoneticType, TableConf, TableConfError, TableConfSite,
    TableRow,
};

pub(crate) use c_stderr::{mmap_failed, rename_failed};
#[cfg(test)]
pub use system_files::{ADDON_LIBRARY_FILES, SYSTEM_LIBRARY_FILES};
#[cfg(test)]
pub(crate) use table_entries::{phrase_index_entries, pinyin_index_entries};

pub(crate) use table_info::DefaultRowStep;
