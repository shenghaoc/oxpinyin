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
use std::path::Path;

use oxpinyin_data::table_info::{FileName, TableConf};
use oxpinyin_data::PhraseLibraries;
use oxpinyin_input::tool::{self, k_mixture_model as edge};
use oxpinyin_kmm::phrase::PhraseIndex;

/// `sentence_start` (`src/include/novel_types.h:122`).
const SENTENCE_START: u32 = 1;

/// One loaded phrase index.
pub struct SystemPhraseIndex {
    libraries: PhraseLibraries,
}

impl PhraseIndex for SystemPhraseIndex {
    fn token_to_string(&self, token: u32) -> Option<Vec<u8>> {
        if token >> 24 == 0 {
            return if token == SENTENCE_START {
                Some(b"<start>".to_vec())
            } else {
                edge::report_unknown_token(token as i32);
                None
            };
        }
        let Some(text) = self.libraries.phrase_text(token) else {
            edge::report_unknown_token(token as i32);
            return None;
        };
        let words: Vec<u32> = text.outward_words().collect();
        tool::ucs4_to_utf8(&words)
    }
}

/// `SystemTableInfo2::load(SYSTEM_TABLE_INFO)` (`import:275`,
/// `export:128`, `gen:330`): the cwd-relative `table.conf`, or the pin's
/// `load table.conf failed.` and `ENOENT`.
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
pub fn load_phrase_index(conf: &TableConf) -> SystemPhraseIndex {
    let mut libraries = PhraseLibraries::empty();
    for (nibble, row) in conf.system_libraries() {
        let Some(system) = row.system.as_ref() else {
            // A row without a system chunk makes the pin read through a NULL
            // name; the empty name is the same no-file answer.
            edge::report_chunk_load_failed(c"");
            tool::exit(tool::ENOENT);
        };
        let path = system.under(Path::new(""));
        if libraries.load(nibble, &path).is_err() {
            edge::report_chunk_load_failed(&c_name(system));
            tool::exit(tool::ENOENT);
        }
    }
    SystemPhraseIndex { libraries }
}

/// Load both, in the pin's order.
pub fn load_system_phrases() -> SystemPhraseIndex {
    let conf = load_table_conf();
    load_phrase_index(&conf)
}

/// A `table.conf` file name as a C string for the pin's `%s`.
fn c_name(name: &FileName) -> CString {
    let os: &OsStr = name.as_ref();
    CString::new(os.as_encoded_bytes()).unwrap_or_default()
}
