//! The `SYSTEM_FILE` libraries of `table.conf`, as
//! `FacadePhraseIndex` loads and stores them (`utils/utils_helper.h:73-145`,
//! `src/storage/phrase_index.cpp` at libpinyin 074a2219).
//!
//! `load_phrase_index` maps each `SYSTEM_FILE` row's `m_system_filename`
//! in sub-index order; a chunk that does not load writes
//! `load %s failed!` and ends the program. `save_phrase_index` stores the
//! same rows with `SubPhraseIndex::store`, whose bytes
//! [`oxpinyin_data::chunk_write::build_chunk`] emits.

use core::ffi::c_int;
use std::collections::BTreeMap;
use std::ffi::CString;
use std::path::Path;

use oxpinyin_data::chunk_format::chunk_checksum;
use oxpinyin_data::chunk_write::build_chunk;
use oxpinyin_data::phrase_library::{LibraryError, PhraseItemView, PhraseLibrary};
use oxpinyin_data::table_info::{FileType, TableConf};

use oxpinyin_data::chunk_write::PHRASE_MASK;
use oxpinyin_input::tool;

use crate::storage::{library_of, name_c_string};

/// One loaded library: its `SYSTEM_FILE` name and resident items in
/// ascending slot order.
pub(crate) struct Library {
    /// `m_system_filename`, the cwd-relative chunk the pin opens.
    pub name: CString,
    /// Slot → item, `compact()` order.
    pub items: BTreeMap<u32, oxpinyin_data::chunk_write::ChunkItem>,
    /// `SubPhraseIndex::m_total_freq`, the stored header value the pin
    /// keeps and adjusts.
    pub total: u32,
}

impl Library {
    /// The complete `MemoryChunk` file `SubPhraseIndex::store` writes.
    pub(crate) fn chunk(&self) -> Vec<u8> {
        let items: Vec<(u32, oxpinyin_data::chunk_write::ChunkItem)> = self
            .items
            .iter()
            .map(|(slot, item)| (*slot, item.clone()))
            .collect();
        let mut file = build_chunk(&items).unwrap_or_else(|_| tool::abort());
        // `SubPhraseIndex::store` writes the resident `m_total_freq`, which
        // equals the item sum for every compacted chunk; keep the stored
        // value if a foreign chunk's header disagrees.
        if u32::from_le_bytes([file[8], file[9], file[10], file[11]]) != self.total {
            file[8..12].copy_from_slice(&self.total.to_le_bytes());
            let checksum = chunk_checksum(&file[8..]);
            file[4..8].copy_from_slice(&checksum.to_le_bytes());
        }
        file
    }
}

/// The sixteen sub-index slots' loaded libraries.
pub(crate) struct Libraries {
    by_index: [Option<Library>; 16],
}

impl Libraries {
    /// `load_phrase_index(phrase_files, phrase_index)`: every `SYSTEM_FILE`
    /// default row, in sub-index order, with a failed load ending the
    /// program at `ENOENT`.
    pub(crate) fn load(conf: &TableConf) -> Self {
        Self::load_rows(conf, false, &[FileType::SystemFile], tool::ENOENT)
    }

    /// The pin's library loop over one `pinyin_table_info_t` array
    /// (default or addon), keeping the rows whose type is in `types`.
    ///
    /// `gen_unigram`'s `generate_unigram` loads both `SYSTEM_FILE` and
    /// `DICTIONARY` rows of the array it is handed
    /// (`utils/training/gen_unigram.cpp:41-70`), and `gen_deleted_ngram`
    /// ends a failed load at `ENODATA` (`gen_deleted_ngram.cpp:72-73`)
    /// where `gen_ngram` uses `ENOENT` (`gen_ngram.cpp:71-72`). Every
    /// other site is `load_phrase_index` over the default `SYSTEM_FILE`
    /// rows (`utils/utils_helper.h:73-95`).
    pub(crate) fn load_rows(
        conf: &TableConf,
        addon: bool,
        types: &[FileType],
        failure_status: c_int,
    ) -> Self {
        let mut by_index: [Option<Library>; 16] = Default::default();
        for index in 0..16_u8 {
            let row = if addon {
                conf.addon_row(index)
            } else {
                conf.default_row(index)
            };
            let Some(row) = row else {
                continue;
            };
            if !types.contains(&row.file_type) {
                continue;
            }
            let Some(name) = row.system.as_ref() else {
                // `MemoryChunk::load(NULL)` in the pin.
                tool::abort();
            };
            let c_name = name_c_string(name.as_ref());
            let library = match PhraseLibrary::open(Path::new(name.as_ref())) {
                Ok(library) => library,
                Err(LibraryError::Row(_)) => tool::abort(),
                Err(_) => {
                    tool::storage::report_load_failed(c_name.as_deref());
                    tool::exit(failure_status);
                }
            };
            let Some(items) = items_of(&library) else {
                tool::abort();
            };
            by_index[usize::from(index)] = Some(Library {
                name: c_name.unwrap_or_default(),
                items,
                total: library.total_freq(),
            });
        }
        Self { by_index }
    }

    /// `FacadePhraseIndex::get_phrase_index_total_freq`
    /// (`src/storage/phrase_index.cpp:237-248`): the loaded libraries'
    /// stored totals added as each one loads, a `guint32` that wraps.
    pub(crate) fn total_freq(&self) -> u32 {
        self.by_index
            .iter()
            .flatten()
            .fold(0_u32, |total, library| total.wrapping_add(library.total))
    }

    /// `FacadePhraseIndex::add_unigram_frequency`
    /// (`src/storage/phrase_index.h:617-625`,
    /// `phrase_index.cpp:150-178`).
    pub(crate) fn add_unigram(&mut self, token: u32, delta: u32) {
        let Some(library) = self.by_index[usize::from(library_of(token))].as_mut() else {
            return;
        };
        let slot = token & PHRASE_MASK;
        let Some(item) = library.items.get_mut(&slot) else {
            return;
        };
        // `protect total_freq overflow`.
        if delta > 0 && library.total > library.total.wrapping_add(delta) {
            return;
        }
        item.unigram = item.unigram.wrapping_add(delta);
        library.total = library.total.wrapping_add(delta);
    }

    /// `generate_unigram`'s range walk (`utils/training/gen_unigram.cpp:62-69`):
    /// `SubPhraseIndex::add_unigram_frequency(token, 1)` for every token of
    /// `get_range`'s `[range_begin, range_end)`, which is `1` up to the
    /// last resident slot — exactly the resident items.
    pub(crate) fn add_unigram_to_every_item(&mut self) {
        for library in self.by_index.iter_mut().flatten() {
            // `protect total_freq overflow`: once the stored total cannot
            // take another unit every remaining call returns the same
            // overflow without changing anything.
            for item in library.items.values_mut() {
                if library.total > library.total.wrapping_add(1) {
                    break;
                }
                item.unigram = item.unigram.wrapping_add(1);
                library.total = library.total.wrapping_add(1);
            }
        }
    }

    /// The library loaded in sub-index slot `index`.
    pub(crate) fn by_library(&self, index: u8) -> Option<&Library> {
        self.by_index.get(usize::from(index))?.as_ref()
    }

    /// `FacadePhraseIndex::get_phrase_item`: `None` is
    /// `ERROR_NO_ITEM`/`ERROR_NO_SUB_PHRASE_INDEX`.
    pub(crate) fn item(&self, token: u32) -> Option<&oxpinyin_data::chunk_write::ChunkItem> {
        self.by_index[usize::from(library_of(token))]
            .as_ref()?
            .items
            .get(&(token & PHRASE_MASK))
    }

    /// `save_phrase_index(phrase_files, phrase_index)`: every `SYSTEM_FILE`
    /// row's chunk, overwritten with `MemoryChunk::save`. A save failure is
    /// the pin's `save %s failed.` and `exit(ENOENT)`.
    pub(crate) fn save(&self, conf: &TableConf) {
        self.save_rows(conf, false, FileType::SystemFile);
    }

    /// The store loop of `save_phrase_index` (`utils/utils_helper.h:97-120`)
    /// or `save_dictionary` (`:122-145`) over one table array: every row of
    /// `file_type`, in sub-index order.
    pub(crate) fn save_rows(&self, conf: &TableConf, addon: bool, file_type: FileType) {
        for index in 0..16_u8 {
            let row = if addon {
                conf.addon_row(index)
            } else {
                conf.default_row(index)
            };
            let Some(row) = row else {
                continue;
            };
            if row.file_type != file_type {
                continue;
            }
            let Some(library) = &self.by_index[usize::from(index)] else {
                continue;
            };
            save_library(&library.name, &library.chunk());
        }
    }
}

/// `taglib_token_to_string` (`src/storage/tag_utility.cpp:384-405`): the
/// phrase text of `token`, the special `"<start>"` for token 1, or `None`
/// after the pin's `error: unknown token:%d.` line.
pub(crate) fn token_to_string(libraries: &Libraries, token: u32) -> Option<CString> {
    if crate::storage::library_of(token) == 0 {
        if token == 1 {
            return Some(c"<start>".to_owned());
        }
        tool::storage::report_unknown_token(c_int::try_from(token).unwrap_or(c_int::MAX));
        return None;
    }
    let Some(item) = libraries.item(token) else {
        tool::storage::report_unknown_token(c_int::try_from(token).unwrap_or(c_int::MAX));
        return None;
    };
    let words: Vec<u32> = item.phrase.clone();
    crate::segment::c_string(&tool::ucs4_to_utf8(&words)?)
}

/// Saves one fully framed `MemoryChunk` through `MemoryChunk::save`'s
/// calls.
pub(crate) fn save_library(name: &CString, file: &[u8]) {
    let length = u32::from_le_bytes([file[0], file[1], file[2], file[3]]);
    let checksum = u32::from_le_bytes([file[4], file[5], file[6], file[7]]);
    if !tool::storage::save_chunk(name, length, checksum, &file[8..]) {
        tool::storage::report_save_failed(name);
        tool::exit(tool::ENOENT);
    }
}

/// Every resident item of `library` as a writable chunk item.
fn items_of(
    library: &PhraseLibrary,
) -> Option<BTreeMap<u32, oxpinyin_data::chunk_write::ChunkItem>> {
    let mut items = BTreeMap::new();
    for (slot, view) in library.items() {
        items.insert(slot, item_of(&view)?);
    }
    Some(items)
}

/// One mapped item as a chunk item.
fn item_of(view: &PhraseItemView<'_>) -> Option<oxpinyin_data::chunk_write::ChunkItem> {
    let phrase: Option<Vec<u32>> = view.phrase_codes().collect();
    let mut prons = Vec::new();
    for pronunciation in view.pronunciations() {
        let keys: Option<Vec<u16>> = pronunciation
            .keys
            .chunks_exact(2)
            .map(|bytes| Some(u16::from_le_bytes([bytes[0], bytes[1]])))
            .collect();
        prons.push((keys?, pronunciation.freq));
    }
    Some(oxpinyin_data::chunk_write::ChunkItem {
        phrase: phrase?,
        unigram: view.unigram(),
        prons,
    })
}
