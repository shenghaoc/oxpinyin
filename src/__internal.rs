//! Unsupported interfaces for this workspace's command-line tools.
//! These interfaces may change without notice.
//! Rustdoc hides this module, and any crate can still import its public items.

pub use crate::data::chewing_table::{
    ChewingTable, DefaultDbm, RawChewingDbm, visit_matching_items,
};
pub use crate::data::phrase_library::backing::LibraryBytes;
pub use crate::data::phrase_table::PhraseTable;
pub use crate::direct::parse as scan_direct;
pub use crate::engine::nbest::nbest_sentences_generation;
pub use crate::engine::phrase::phrase_segment_ucs4;
pub use crate::engine::session::matrix::{ParsedMatrix, ScanMatrix};
pub use crate::engine::session::{apostrophe_extended, build_scan_matrix_over};
pub use crate::input::chunk::read::{ChunkLoadError, load as load_chunk};
pub use crate::input::native::{c_stderr, fopen};
#[cfg(windows)]
pub use crate::input::native::{errno_location, wide_path};
pub use crate::input::path::build_path;
pub use crate::scheme::parse_zhuyin_direct_key;
pub use crate::single_gram::layout::is_complete;
pub use crate::user::persistence::journal_item_bytes;
pub use crate::user::replay::ReplayedLibrary;
pub use crate::vocab::fuzzy::{fuzzy_chewing_finals, fuzzy_chewing_initials};

/// File extension for [`crate::store::DefaultStore`]'s native tables — one per peer;
/// the store forces its database type through open parameters, so the
/// extension is naming, not detection.
#[cfg(feature = "kyotocabinet")]
pub const DEFAULT_STORE_EXT: &str = "kct";
/// File extension for [`crate::store::DefaultStore`]'s native tables (tkrzw `TreeDBM`).
#[cfg(feature = "tkrzw")]
pub const DEFAULT_STORE_EXT: &str = "tkt";
/// File extension for [`crate::store::DefaultStore`]'s native tables (Berkeley DB).
/// The DBM files libpinyin itself uses carry its own names
/// (`bigram.db`, `pinyin_index.bin`, …); this extension names this
/// backend's session-scratch containers and the `fixtures/w3`
/// directory of this backend.
#[cfg(feature = "bdb")]
pub const DEFAULT_STORE_EXT: &str = "db";

/// The newline a native text-mode `fopen("w")` writes: CRLF on Windows,
/// LF on Unix. The pin's `user.conf` and generated text tables inherit it.
pub const TEXT_NEWLINE: &str = if cfg!(windows) { "\r\n" } else { "\n" };
