//! Reads libpinyin's system data directly: the pinyin and phrase DBMs
//! (`ChewingLargeTable2`, `PhraseLargeTable3`), the mmap'd per-library
//! phrase-index chunk files (`FacadePhraseIndex`), the bigram (`Bigram`)
//! and the punctuation table (`PunctTable`) — the same files a libpinyin
//! install ships on Kyoto Cabinet, tkrzw and Berkeley DB, and the same
//! records in the backend's own container.
//!
//! Every reader is lazy: opening a directory costs the DBM handles and
//! the chunk-file mappings, and each lookup is a point read. Portable: no
//! glib, no Linux-only deps. Internal crate — the supported external interfaces
//! are the libpinyin-compatible and libzhuyin-compatible C APIs and ABIs.
//!
//! The format notes live under `docs/findings/`:
//! `data-formats.md` (the overview and, in §4-§7, the pinyin index, the
//! phrase index, the bigram and the punctuation table).
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
#[path = "../../../tests/support/mod.rs"]
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
// token, re-exported so consumers above this crate (runtime, capi, tests,
// fuzz) name fixture directories and check `table.conf` consistently with
// the backend the build selected — oxpinyin has no direct store
// edge.
pub use crate::store::{DEFAULT_STORE_DB_FORMAT, DEFAULT_STORE_EXT};
pub use phrase_libraries::PhraseLibraries;
pub use punct::PunctTable;
pub use system_files::{
    ADDON_LIBRARY_FILES, ADDON_LIBRARY_NAMES, SYSTEM_LIBRARY_FILES, SYSTEM_LIBRARY_NAMES, SystemDbm,
};
pub use table::TableError;
pub use table_conf::PINNED_LAMBDA;

// Native row decoding for the pin test-program tools.
pub use chewing_table::{index_key as chewing_index_key, search_row as search_chewing_row};
