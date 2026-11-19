//! Reads the system dictionary, phrase libraries, bigram model and punctuation tables.
//! Configuration selects the system libraries. Opening them maps and checks their phrase
//! chunks. Successor and index lookups read individual DBM rows. Extension searches can
//! walk a key range.
//!
//! The readers use [`crate::store`] for DBM access and [`crate::input`] for native I/O.

// ChewingLargeTable2, PhraseLargeTable3, FacadePhraseIndex, Bigram and PunctTable.
// Initialization: pinyin.cpp:331–396. See docs/findings/data-formats.md §§4–7.
pub mod bigram_table;
pub mod c_stderr;
pub(crate) mod chewing_table;
pub mod chunk_format;
pub mod chunk_write;
pub mod dict;
pub mod lm;
#[cfg(test)]
pub(crate) mod memory_dbm;
pub mod phrase_libraries;
pub mod phrase_library;
pub(crate) mod phrase_table;
pub mod punct;
pub mod row_format;
pub mod single_gram;
#[cfg(test)]
#[path = "../../tests/support/mod.rs"]
pub(crate) mod support;
pub mod system_files;
pub mod table;
pub mod table_conf;
pub mod table_entries;
pub mod table_info;
pub mod user_files;

pub use crate::model::UserCountDelta;
pub use bigram_table::BigramTable;
pub use dict::{AddonDictionary, DictError, PhraseIndexTable, PinyinIndexTable, SystemDictionary};
pub use lm::{BigramLanguageModel, BigramRow, LmError, merge_bigram};
// The compiled-in backend's native-table extension and `database format:`
// token. This module reads the DBMs through `crate::store` directly; the
// re-export lets the consumers above it (`runtime`, the C crates, tests and
// fuzz targets) name fixture directories and check `table.conf` against the
// backend the build selected without naming `store` themselves.
pub use crate::store::DEFAULT_STORE_DB_FORMAT;
#[cfg(any(test, feature = "test-hooks"))]
pub use crate::store::DEFAULT_STORE_EXT;
pub use phrase_libraries::PhraseLibraries;
pub use punct::PunctTable;
#[cfg(test)]
pub use system_files::{ADDON_LIBRARY_FILES, SYSTEM_LIBRARY_FILES};
pub use system_files::{ADDON_LIBRARY_NAMES, SYSTEM_LIBRARY_NAMES, SystemDbm};
pub use table::TableError;
pub use table_conf::PINNED_LAMBDA;

// Native row decoding for the pin test-program tools.
pub use chewing_table::{index_key as chewing_index_key, search_row as search_chewing_row};
