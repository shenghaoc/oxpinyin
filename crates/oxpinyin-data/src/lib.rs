//! Reads libpinyin's system data directly: the pinyin and phrase DBMs
//! (`ChewingLargeTable2`, `PhraseLargeTable3`), the mmap'd per-library
//! phrase-index chunk files (`FacadePhraseIndex`), the bigram (`Bigram`)
//! and the punctuation table (`PunctTable`) — the same files a libpinyin
//! install ships on Kyoto Cabinet and tkrzw, and the same records in
//! redb's or LMDB's own container on those backends.
//!
//! Every reader is lazy: opening a directory costs the DBM handles and
//! the chunk-file mappings, and each lookup is a point read. Portable: no
//! glib, no Linux-only deps. Internal crate — the supported public API is
//! `oxpinyin-engine`.
//!
//! The format notes live under `docs/findings/`:
//! `libpinyin-system-data-formats-2026-09-01.md` (overview),
//! `pinyin-dbm-format-2026-09-01.md`, `phrase-dbm-format-2026-09-01.md`,
//! `bigram-punct-format-2026-09-01.md`; the custom-content loader follows
//! `data-formats.md`.
#![deny(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

pub mod bigram_table;
pub(crate) mod chewing_table;
pub mod content;
pub mod dict;
pub mod interp;
pub mod lm;
pub mod phrase_libraries;
pub mod phrase_library;
pub(crate) mod phrase_table;
pub mod punct;
pub mod system_files;
pub mod table;
pub mod table_conf;

pub use bigram_table::BigramTable;
pub use content::{ContentTable, LoadError, Record, TokenPair};
pub use dict::{AddonDictionary, AddonPhraseItem, DictError, SystemDictionary, ucs4_walk_key};
pub use interp::{
    InterpolationError, UnigramTable, parse_interpolation2, parse_interpolation2_from_reader,
};
pub use lm::{
    BigramLanguageModel, BigramRow, LmError, library_visible, merge_bigram, merge_counts,
};
pub use oxpinyin_core::UserCountDelta;
// The compiled-in backend's native-table extension, filename helper and
// drop-in flag, re-exported so consumers above this crate (runtime, capi,
// tests) name data files consistently with the backend the build
// selected — oxpinyin-runtime has no direct store edge.
pub use oxpinyin_store::{DEFAULT_STORE_EXT, DEFAULT_STORE_IS_LIBPINYIN_DBM, default_store_file};
pub use phrase_libraries::PhraseLibraries;
pub use punct::PunctTable;
pub use system_files::{ADDON_LIBRARY_FILES, SYSTEM_LIBRARY_FILES, SystemDbm, addon_library_file};
pub use table::{GenericLookupTable, LookupTable, TableError};
pub use table_conf::{Lambda, PINNED_LAMBDA, parse_table_conf_lambda, read_table_conf_lambda};
