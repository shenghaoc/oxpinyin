//! The pin's heap chunk and phrase-index test programs (074a2219).
use crate::chunk_tests::check;
use oxpinyin_data::chunk_format::chunk_checksum;
use oxpinyin_data::phrase_library::PhraseLibrary;
use oxpinyin_data::table_info::{FileType, TableConf, TableConfError};
use oxpinyin_input::tool::storage::TableFile;
use oxpinyin_input::{chunk, tool};
use oxpinyin_user::tool::Index;
use std::ffi::CString;
use std::path::Path;

pub mod index;
pub mod logger;
pub mod lookup;
pub mod tables;

pub(crate) fn present<T>(value: Option<T>) -> T {
    value.unwrap_or_else(|| tool::abort())
}

pub(crate) fn conf() -> TableConf {
    match TableConf::load(c"../../data/table.conf") {
        Ok(conf) => conf,
        Err(TableConfError::Abort(_) | TableConfError::UninitialisedSourceFormat) => tool::abort(),
        Err(TableConfError::Unopenable | TableConfError::Header) => {
            tool::storage::report_table_conf_failed();
            tool::exit(tool::ENOENT);
        }
    }
}

pub(crate) fn payload(path: &Path) -> Option<Vec<u8>> {
    match chunk::load(path) {
        Ok(loaded) => loaded.finish(chunk_checksum),
        Err(chunk::ChunkLoadError::Header(_)) => tool::abort(),
        Err(chunk::ChunkLoadError::Io(_) | chunk::ChunkLoadError::ShortHeader) => None,
    }
}

pub(crate) fn library(payload: Vec<u8>) -> PhraseLibrary {
    PhraseLibrary::from_payload(payload).unwrap_or_else(|_| tool::exit(tool::ENOENT))
}

pub(crate) fn stored(index: &Index) -> Vec<u8> {
    present(index.store())
}

pub(crate) fn reload(index: &Index) -> Index {
    let bytes = stored(index);
    Index::from_library(library(bytes[8..].to_vec()))
}

pub(crate) fn save(path: &Path, bytes: &[u8]) {
    match chunk::save(path, bytes) {
        Ok(()) | Err(chunk::ChunkSaveError::Io(_)) => {}
        Err(chunk::ChunkSaveError::Header { .. }) => tool::abort(),
    }
}

pub(crate) fn load_text(conf: &TableConf) -> [Option<Index>; 16] {
    load_text_tables(conf, None, None)
}

pub(crate) fn load_text_tables(
    conf: &TableConf,
    chewing: Option<&crate::storage::dbm::Tree>,
    phrase: Option<&crate::storage::dbm::Tree>,
) -> [Option<Index>; 16] {
    let mut indices = core::array::from_fn(|_| None);
    for slot in 0..16_u8 {
        let Some(row) = conf.default_row(slot) else {
            continue;
        };
        if row.file_type != FileType::SystemFile {
            continue;
        }
        let name = row
            .table
            .as_ref()
            .and_then(|value| oxpinyin_input::c_path(Path::new(value.as_ref())));
        // g_build_filename keeps the data prefix for names with a leading
        // separator; Path::join would replace it.
        let path = present(tool::storage::build_filename(
            Some(c"../../data"),
            name.as_deref().unwrap_or(c""),
        ));
        let Some(mut file) = TableFile::open(&path) else {
            tool::phrase_tests::open_failed(name.as_deref());
            tool::exit(tool::ENOENT);
        };
        if let Some(table) = chewing {
            crate::storage::gen_binary_files::load_chewing(
                Some(table),
                &mut file,
                conf.phonetic_type(),
            );
        }
        file.rewind();
        if let Some(table) = phrase {
            crate::storage::gen_binary_files::load_phrase_table(Some(table), &mut file, true);
        }
        file.rewind();
        let items = crate::storage::gen_binary_files::read_phrase_items(
            slot,
            &mut file,
            conf.phonetic_type(),
            true,
        );
        let mut index = Index::default();
        for (token, item) in items {
            index.add(token, &item);
        }
        indices[usize::from(slot)] = Some(index);
    }
    indices
}
