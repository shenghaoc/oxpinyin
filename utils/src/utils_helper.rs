//! Shared counterparts of the pin's `utils/utils_helper.h` operations.

use std::ffi::{CStr, CString, c_int};
use std::path::Path;

use oxpinyin::data::PhraseLibraries;
use oxpinyin::data::phrase_library::{LibraryError, PhraseLibrary};
use oxpinyin::data::{FileName, FileType, TableConf, TableRow};
use oxpinyin_tool_support::input as tool;

pub mod taglib;

// The LINE_TYPE values shared by the three tagged-model readers.
pub(crate) const BEGIN_LINE: c_int = 1;
pub(crate) const END_LINE: c_int = 2;
pub(crate) const GRAM_1_LINE: c_int = 3;
pub(crate) const GRAM_2_LINE: c_int = 4;
pub(crate) const GRAM_1_ITEM_LINE: c_int = 5;
pub(crate) const GRAM_2_ITEM_LINE: c_int = 6;

/// The registrations and output slots shared by the tagged-model readers.
pub(crate) struct TaggedModel {
    pub(crate) tags: taglib::TagLib,
    pub(crate) line_type: c_int,
    values: Vec<CString>,
    required: taglib::Required,
}

impl TaggedModel {
    pub(crate) fn new() -> Self {
        Self {
            tags: taglib::TagLib::init(),
            line_type: 0,
            values: Vec::new(),
            required: taglib::Required::default(),
        }
    }

    pub(crate) fn read(&mut self, line: &CStr, checked: bool) -> Result<bool, taglib::Undefined> {
        self.tags.read(
            line,
            checked,
            &mut self.line_type,
            &mut self.values,
            &mut self.required,
        )
    }

    /// The importers abort where the pin's read is undefined.
    pub(crate) fn read_or_abort(&mut self, line: &CStr) -> bool {
        self.read(line, false).unwrap_or_else(|_| tool::abort())
    }

    /// `check_result(taglib_read(...))` in the importers.
    pub(crate) fn check_read(&mut self, line: &CStr) {
        if !self.read(line, true).unwrap_or_else(|_| tool::abort()) {
            tool::abort();
        }
    }

    /// `TAGLIB_GET_PHRASE_STRING` (`utils_helper.h:35-40`).
    pub(crate) fn phrase(&self, index: usize) -> &CStr {
        self.values.get(index).map_or(c"", CString::as_c_str)
    }

    /// `TAGLIB_GET_TOKEN` (`utils_helper.h:27-33`). Callers keep the signed
    /// result for `%d` or cast it to the pin's unsigned phrase token.
    pub(crate) fn token(&self, index: usize) -> c_int {
        tool::atoi(self.phrase(index))
    }

    /// The input to `TAGLIB_GET_TAGVALUE`'s conversion (`utils_helper.h:42-49`).
    pub(crate) fn value(&self, name: &[u8]) -> &CStr {
        self.required.get(name).map_or(c"", CString::as_c_str)
    }
}

pub(crate) enum SystemLibraryError<'a> {
    MissingName,
    Chunk(&'a FileName, LibraryError),
}

/// Which half of `table.conf` a utility walks: the default tables, or the
/// addon tables (`utils_helper.h:73-95`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TableSet {
    /// The `default` rows.
    Default,
    /// The `addon` rows.
    Addon,
}

impl TableSet {
    /// The row of sub-index `index` in this half.
    pub(crate) fn row(self, conf: &TableConf, index: u8) -> Option<&TableRow> {
        match self {
            Self::Default => conf.default_row(index),
            Self::Addon => conf.addon_row(index),
        }
    }
}

/// Visit the selected table rows in sub-index order, keeping the filename
/// attached to any read fault (`utils_helper.h:73-95`).
pub(crate) fn load_library_rows<'a>(
    conf: &'a TableConf,
    tables: TableSet,
    types: &[FileType],
    mut load: impl FnMut(u8, &FileName) -> Result<(), LibraryError>,
) -> Result<(), SystemLibraryError<'a>> {
    for index in 0..16_u8 {
        let row = tables.row(conf, index);
        let Some(row) = row.filter(|row| types.contains(&row.file_type)) else {
            continue;
        };
        let name = row.system.as_ref().ok_or(SystemLibraryError::MissingName)?;
        load(index, name).map_err(|error| SystemLibraryError::Chunk(name, error))?;
    }
    Ok(())
}

/// `load_phrase_index` over the default system rows. Callers keep their
/// own policies for a NULL name and for row faults.
pub(crate) fn load_system_libraries(
    conf: &TableConf,
) -> Result<PhraseLibraries, SystemLibraryError<'_>> {
    let mut libraries = PhraseLibraries::empty();
    load_library_rows(
        conf,
        TableSet::Default,
        &[FileType::SystemFile],
        |index, name| {
            oxpinyin_tool_support::data::load_heap_library(
                &mut libraries,
                index,
                Path::new(name.as_ref()),
            )?;
            Ok(())
        },
    )?;
    Ok(libraries)
}

/// The save and reattach sequence of `save_phrase_index` and
/// `save_dictionary` (`utils_helper.h:97-145`).
pub(crate) fn save_and_reload_library(name: &CStr, file: &[u8]) -> PhraseLibrary {
    let path = crate::training::path_of(name);
    if let Err(oxpinyin::input::chunk::ChunkSaveError::Io(_)) =
        oxpinyin::input::chunk::save(&path, file)
    {
        tool::storage::report_save_failed(name);
        tool::exit(tool::ENOENT);
    }
    oxpinyin_tool_support::data::library_from_payload(file[8..].to_vec())
        .unwrap_or_else(|_| tool::abort())
}

/// `taglib_token_to_string` (`tag_utility.cpp:384-405`). A missing item
/// writes the unknown-token diagnostic. A present item whose text cannot
/// be read or converted yields no text without that diagnostic.
pub(crate) fn tag_token_text<Item, Words: AsRef<[u32]>>(
    token: u32,
    special_library: bool,
    lookup: impl FnOnce(u32) -> Option<Item>,
    words: impl FnOnce(Item) -> Option<Words>,
    unknown_token: impl FnOnce(u32),
) -> Option<Vec<u8>> {
    if special_library {
        if token == 1 {
            return Some(b"<start>".to_vec());
        }
    } else if let Some(item) = lookup(token) {
        return tool::ucs4_to_utf8(words(item)?.as_ref());
    }
    unknown_token(token);
    None
}
