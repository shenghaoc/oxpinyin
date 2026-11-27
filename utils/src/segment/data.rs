//! The system tables the pin's segmentation utilities load from the
//! current working directory (`ngseg.cpp:145-171`, `spseg.cpp:240-259`,
//! `mergeseq.cpp:239-258`).
//!
//! `SystemTableInfo2::load("table.conf")`, then
//! `FacadePhraseTable3::load("phrase_index.bin", NULL)` (its answer
//! ignored), then `load_phrase_index` over the `SYSTEM_FILE` rows
//! (`utils/utils_helper.h:73-95`: a chunk that fails to load writes
//! `load %s failed!` and ends the program), then — `ngseg` only — the
//! system bigram's `attach("bigram.db", ATTACH_READONLY)` (its answer
//! ignored). Relative names are opened as they stand, as the pin's C
//! library opens them.

use std::path::Path;

use crate::utils_helper::SystemLibraryError;
use oxpinyin::data::phrase_library::{LibraryError, RowFault};
use oxpinyin::data::{BigramTable, PhraseLibraries};
use oxpinyin::data::{TableConf, TableConfError};
use oxpinyin_tool_support::data::PhraseIndexTable;
use oxpinyin_tool_support::input as tool;

/// The loaded system data.
pub(crate) struct Tables {
    /// `table.conf`, read by the pin's scanner.
    pub conf: TableConf,
    /// `FacadePhraseTable3`'s system table; `None` is a failed load, whose
    /// answer every utility ignores.
    pub phrase: Option<PhraseIndexTable>,
    /// `FacadePhraseIndex`'s loaded `SYSTEM_FILE` libraries.
    pub libraries: PhraseLibraries,
    /// `Bigram::attach`'s answer; `ngseg` only, and ignored, so a failed
    /// attach is `None`.
    pub bigram: Option<BigramTable>,
}

/// Which model a segmentation tool loads beside the unigram tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Gram {
    /// The unigram tables only.
    Unigram,
    /// The system bigram too.
    Bigram,
}

/// Loads `table.conf`, the phrase table, the system libraries and, when
/// [`Gram::Bigram`], the system bigram, ending the program where the
/// pin does: a `table.conf` the scanner refused writes the utility's line
/// and exits `ENOENT`; a chunk that did not load writes `load %s failed!`
/// and exits `ENOENT`; a malformed `table.conf` the pin `abort()`s on (or
/// reads uninitialised) is oxpinyin's refusal.
pub(crate) fn load_or_exit(gram: Gram) -> Tables {
    let conf = match TableConf::load(c"table.conf") {
        Ok(conf) => conf,
        // `SystemTableInfo2::load`'s `abort()` sites, and the
        // uninitialised `source table format:` read (class (b)).
        Err(TableConfError::Abort(_) | TableConfError::UninitialisedSourceFormat) => tool::abort(),
        // `fopen`'s own `open table.conf failed.` line was already
        // written by the reader; the utility adds its line.
        Err(TableConfError::Unopenable | TableConfError::Header) => {
            tool::segmentation::report_table_conf_failed();
            tool::exit(tool::ENOENT);
        }
    };
    // `phrase_table.load(SYSTEM_PHRASE_INDEX, NULL)` — the return is
    // ignored: a failed attach leaves a table that answers every search
    // with SEARCH_NONE.
    let phrase = PhraseIndexTable::open(Path::new("phrase_index.bin")).ok();
    // `load_phrase_index`: `const char * binfile =
    // table_info->m_system_filename;` — the raw column, not joined to a
    // directory. A second row for one nibble is the pin's
    // `ERROR_ALREADY_EXISTS`, whose answer the loop ignores.
    let libraries = match crate::utils_helper::load_system_libraries(&conf) {
        Ok(libraries) => libraries,
        // `MemoryChunk::load`'s `assert(ret_len == sizeof(length))` on a
        // directory named as a system chunk (`memory_chunk.h:429`).
        Err(SystemLibraryError::Chunk(_, LibraryError::Row(RowFault::ChunkHeaderRead))) => {
            tool::abort()
        }
        // A NULL name, which the pin passes to `%s` as NULL.
        Err(SystemLibraryError::MissingName) => {
            tool::segmentation::report_load_failed(None);
            tool::exit(tool::ENOENT);
        }
        Err(SystemLibraryError::Chunk(name, _)) => {
            let name = oxpinyin::input::c_path(Path::new(name.as_ref()));
            tool::segmentation::report_load_failed(name.as_deref());
            tool::exit(tool::ENOENT);
        }
    };
    let bigram = (gram == Gram::Bigram)
        .then(|| oxpinyin_tool_support::data::open_bigram(Path::new("bigram.db")).ok())
        .flatten();
    Tables {
        conf,
        phrase,
        libraries,
        bigram,
    }
}
