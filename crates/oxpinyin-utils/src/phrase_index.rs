//! The system phrase index the KMM text utilities validate and print through
//! (`utils/utils_helper.h:73-95` and `src/storage/tag_utility.cpp:360-417`
//! at `074a2219`).
//!
//! `load_phrase_index` reads the cwd-relative `table.conf`, then loads every
//! `SYSTEM_FILE` row's system chunk by its own file name, in sub-index
//! order. A failed chunk load prints `load %s failed!\n` and exits
//! `ENOENT`; a failed `table.conf` load prints `load table.conf failed.\n`
//! and exits `ENOENT`. `taglib_token_to_string` resolves library zero
//! through the pin's special table (`<start>` for `sentence_start`; every
//! other token prints `error: unknown token:%d.\n` and answers NULL) and
//! every other token through the loaded chunk, encoding its UCS-4 words
//! with `g_ucs4_to_utf8`.

use std::ffi::{CString, OsStr};

use crate::k_mixture_model::phrase::PhraseIndex;
use crate::utils_helper::{SystemLibraryError, load_system_libraries, tag_token_text};
use oxpinyin_data::PhraseLibraries;
use oxpinyin_data::phrase_library::LibraryError;
use oxpinyin_data::table_info::{FileName, TableConf};
use oxpinyin_input::tool::{self, k_mixture_model as edge};

/// One loaded phrase index.
pub struct SystemPhraseIndex {
    libraries: PhraseLibraries,
}

impl PhraseIndex for SystemPhraseIndex {
    fn token_to_string(&self, token: u32) -> Option<Vec<u8>> {
        tag_token_text(
            token,
            // 074a2219 tag_utility.cpp:391.
            oxpinyin_core::phrase_index_library(token) == 0,
            |token| self.libraries.phrase_text(token),
            |text| Some(text.outward_words().collect::<Vec<_>>()),
            |token| edge::report_unknown_token(token as i32),
        )
    }
}

/// `SystemTableInfo2::load(SYSTEM_TABLE_INFO)` (`import:275`,
/// `export:128`, `gen:330`): the cwd-relative `table.conf`, or the pin's
/// `load table.conf failed.` and `ENOENT`.
#[must_use]
pub fn load_table_conf() -> TableConf {
    match TableConf::load(c"table.conf") {
        Ok(conf) => conf,
        Err(_) => {
            edge::report_table_conf_load_failed();
            tool::exit(tool::ENOENT);
        }
    }
}

/// `load_phrase_index` (`utils_helper.h:73-95`).
#[must_use]
pub fn load_phrase_index(conf: &TableConf) -> SystemPhraseIndex {
    let libraries = match load_system_libraries(conf) {
        Ok(libraries) => libraries,
        Err(SystemLibraryError::MissingName) => {
            // `MemoryChunk::load(NULL)` is undefined in the pin.
            edge::report_chunk_load_failed(c"");
            tool::exit(tool::ENOENT);
        }
        Err(SystemLibraryError::Chunk(_, LibraryError::Row(_))) => tool::abort(),
        Err(SystemLibraryError::Chunk(
            name,
            LibraryError::Io(_)
            | LibraryError::Format(_)
            | LibraryError::Layout(_)
            | LibraryError::Unmappable { .. },
        )) => {
            edge::report_chunk_load_failed(&c_name(name));
            tool::exit(tool::ENOENT);
        }
    };
    SystemPhraseIndex { libraries }
}

/// Load both, in the pin's order.
#[must_use]
pub fn load_system_phrases() -> SystemPhraseIndex {
    let conf = load_table_conf();
    load_phrase_index(&conf)
}

/// A `table.conf` file name as a C string for the pin's `%s`.
fn c_name(name: &FileName) -> CString {
    let os: &OsStr = name.as_ref();
    CString::new(oxpinyin_input::path::path_to_bytes(std::path::Path::new(
        os,
    )))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{PhraseIndex, SystemPhraseIndex};
    use oxpinyin_data::PhraseLibraries;

    #[test]
    fn sentence_start_is_the_special_table() {
        let phrases = SystemPhraseIndex {
            libraries: PhraseLibraries::empty(),
        };
        assert_eq!(phrases.token_to_string(1), Some(b"<start>".to_vec()));
        assert_eq!(phrases.token_to_string(2), None);
        assert!(phrases.validate_token_with_string(1, b"<start>"));
        assert!(!phrases.validate_token_with_string(2, b"x"));
    }
}
