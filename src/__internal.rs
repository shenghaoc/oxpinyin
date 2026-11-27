//! Unsupported interfaces for this workspace's command-line tools and tests.
//! These interfaces may change without notice.

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

// The items of each module that the tools, the fuzz targets and the test fixtures
// name, under the owning module's own path.
pub mod data {
    pub use crate::data::{
        BigramTable, BigramTables, DEFAULT_STORE_DB_FORMAT, DictError, FileName, FileType,
        LibrarySet, PhoneticType, PhraseLibraries, PunctTable, SystemTables, TableConfError,
        TableError, TableRow, chewing_index_key,
    };
    pub mod chunk_format {
        pub use crate::data::chunk_format::{
            CHUNK_HEADER_SIZE, build_memory_chunk, chunk_checksum,
        };
    }
    pub mod phrase_library {
        pub use crate::data::phrase_library::{
            ChunkItem, FIRST_ITEM_OFFSET, INDEX_ONE, LibraryError, PhraseItemView, PhraseLibrary,
            RowFault, SEPARATOR, build_chunk, directory_reads_as_a_chunk,
        };
    }
    pub mod row_format {
        pub use crate::data::row_format::encode_token_key;
        pub mod phrase_index {
            pub use crate::data::row_format::phrase_index::{
                decode_tokens, encode_ucs4_key, encode_ucs4_key_scalars,
            };
        }
        pub mod pinyin_index {
            pub use crate::data::row_format::pinyin_index::{
                PinyinIndexItem, decode_items, encode_complete_key, encode_incomplete_key,
                encode_item,
            };
        }
        pub mod punct {
            pub use crate::data::row_format::punct::decode_puncts;
        }
    }
    pub mod user_files {
        pub use crate::data::user_files::{SystemVersions, UserDbm, UserFileLayout, UserTableInfo};
    }
}
pub mod engine {
    pub use crate::engine::{NbestShape, Session, SessionState};
}
pub mod graph {
    pub use crate::graph::Edge;
}
pub mod input {
    pub use crate::input::{FileName, path_from_bytes, path_to_bytes};
    pub mod chunk {
        pub use crate::input::chunk::save;
    }
}
pub mod model {
    pub use crate::model::{LanguageModel, PhraseList};
}
pub mod parser {
    pub use crate::parser::{Completeness, DoublePinyinParser, FullPinyinParser, ZhuyinParser};
}
pub mod single_gram {
    pub use crate::single_gram::{
        SingleGramRecord, checked_get_freq, encode_single_gram, get_total_freq, retrieve_all,
    };
}
pub mod store {
    pub use crate::store::{
        AccessMode, BackendError, DEFAULT_STORE_DB_FORMAT, DbmKind, DefaultStore,
        DefaultUserBigramDb, RawReadStore, StoreError, SystemAttachment, UserBigramDb, WriteStore,
    };
    // The copy fault exists only with test hooks.
    #[cfg(all(feature = "test-hooks", any(feature = "tkrzw", feature = "bdb")))]
    pub use crate::store::fail_destination_write_after;
}
pub mod user {
    pub use crate::user::{
        CEILING_SEED, KEY_TONE_SHIFT, LibraryLoopRows, MAX_KEY_TONE, SystemLibrary,
        SystemOriginals, add_system_reading, key_pinyin_string,
    };
}
pub mod vocab {
    pub use crate::vocab::{PhraseEntry, SENTENCE_START, phrase_index_library_index};
}

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
