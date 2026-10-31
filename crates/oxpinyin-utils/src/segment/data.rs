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

use std::ffi::CString;
use std::path::Path;

use oxpinyin_data::table_info::{TableConf, TableConfError};
use oxpinyin_data::{BigramTable, PhraseIndexTable, PhraseLibraries};
use oxpinyin_input::tool;

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

/// Where loading stops.
pub(crate) enum TablesError {
    /// `SystemTableInfo2::load` refused or died.
    TableConf(TableConfError),
    /// One `SYSTEM_FILE` chunk did not load; the name is the raw column
    /// (`None` for a NULL name, which the pin passes to `%s` as NULL).
    Chunk(Option<CString>),
}

/// Loads `table.conf`, the phrase table, the system libraries and, when
/// `with_bigram` is set, the system bigram.
pub(crate) fn load(with_bigram: bool) -> Result<Tables, TablesError> {
    let conf = TableConf::load(c"table.conf").map_err(TablesError::TableConf)?;
    // `phrase_table.load(SYSTEM_PHRASE_INDEX, NULL)` — the return is
    // ignored: a failed attach leaves a table that answers every search
    // with SEARCH_NONE.
    let phrase = PhraseIndexTable::open(Path::new("phrase_index.bin")).ok();
    let mut libraries = PhraseLibraries::empty();
    for (index, row) in conf.system_libraries() {
        // `load_phrase_index`: `const char * binfile =
        // table_info->m_system_filename;` — the raw column, not joined to
        // a directory.
        let Some(name) = row.system.as_ref() else {
            return Err(TablesError::Chunk(None));
        };
        let c_name = oxpinyin_input::c_path(Path::new(name.as_ref()));
        match libraries.load(index, Path::new(name.as_ref())) {
            // A second row for one nibble is the pin's
            // `ERROR_ALREADY_EXISTS`, whose answer the loop ignores.
            Ok(_) => {}
            Err(_) => return Err(TablesError::Chunk(c_name)),
        }
    }
    let bigram = with_bigram
        .then(|| BigramTable::open(Path::new("bigram.db")).ok())
        .flatten();
    Ok(Tables {
        conf,
        phrase,
        libraries,
        bigram,
    })
}

/// [`load`], ending the program where the pin does: a `table.conf` the
/// scanner refused writes the utility's line and exits `ENOENT`; a chunk
/// that did not load writes `load %s failed!` and exits `ENOENT`; a
/// malformed `table.conf` the pin `abort()`s on (or reads uninitialised)
/// is oxpinyin's refusal.
pub(crate) fn load_or_exit(with_bigram: bool) -> Tables {
    match load(with_bigram) {
        Ok(tables) => tables,
        Err(TablesError::TableConf(error)) => match error {
            // `SystemTableInfo2::load`'s `abort()` sites, and the
            // uninitialised `source table format:` read (class (b)).
            TableConfError::Abort(_) | TableConfError::UninitialisedSourceFormat => tool::abort(),
            // `fopen`'s own `open table.conf failed.` line was already
            // written by the reader; the utility adds its line.
            TableConfError::Unopenable | TableConfError::Header => {
                tool::segmentation::report_table_conf_failed();
                tool::exit(tool::ENOENT);
            }
        },
        Err(TablesError::Chunk(name)) => {
            tool::segmentation::report_load_failed(name.as_deref());
            tool::exit(tool::ENOENT);
        }
    }
}
