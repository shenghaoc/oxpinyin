//! A single nonshipping REPL for the ten upstream interactive modes.
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::unreachable
    )
)]

use crate::tables::Index;
use oxpinyin::data::{FileType, TableConf};
use oxpinyin::input::tool::{self, storage::TableFile};
use std::path::{Path, PathBuf};

mod lookup;
mod parser;
mod sessions;
mod tables;

fn present<T>(value: Option<T>) -> T {
    value.unwrap_or_else(|| tool::abort())
}

fn conf(data: &Path) -> TableConf {
    let path = present(oxpinyin::input::c_path(&data.join("table.conf")));
    TableConf::load(&path).unwrap_or_else(|_| {
        eprintln!("cannot load {}", data.join("table.conf").display());
        std::process::exit(2);
    })
}

fn main() {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_default();
    let mode = args.next().unwrap_or_default();
    let Some(data) = args.next() else {
        eprintln!(
            "usage: oxpinyin-repl <pinyin|phrase|chewing|zhuyin|parser2|phrase_table|chewing_table|matrix|pinyin_lookup|phrase_lookup> <data-directory> [parser options]"
        );
        std::process::exit(2);
    };
    let data = PathBuf::from(data);
    let c_data = present(oxpinyin::input::c_path(&data));
    match mode.to_str() {
        Some("pinyin") => sessions::pinyin(&c_data),
        Some("phrase") => sessions::phrase(&c_data),
        Some("chewing") => sessions::chewing(&c_data),
        Some("zhuyin") => sessions::zhuyin(&c_data),
        Some("parser2") => parser::main(std::iter::once(program).chain(args)),
        Some("phrase_table") => tables::phrase_main(&data),
        Some("chewing_table") => tables::chewing_main(&data),
        Some("matrix") => lookup::matrix_main(&data),
        Some("pinyin_lookup") => lookup::pinyin_main(&data),
        Some("phrase_lookup") => lookup::phrase_main(&data),
        _ => {
            eprintln!("unknown REPL mode: {}", mode.to_string_lossy());
            std::process::exit(2);
        }
    }
}

pub(crate) fn load_text_tables(
    data: &Path,
    conf: &TableConf,
    chewing: Option<&oxpinyin_utils::storage::Tree>,
    phrase: Option<&oxpinyin_utils::storage::Tree>,
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
            .and_then(|value| oxpinyin::input::c_path(Path::new(value.as_ref())));
        // g_build_filename keeps the data prefix for names with a leading
        // separator; Path::join would replace it.
        let path = present(tool::storage::build_filename(
            oxpinyin::input::c_path(data).as_deref(),
            name.as_deref().unwrap_or(c""),
        ));
        let Some(mut file) = TableFile::open(&path) else {
            tool::phrase_tests::open_failed(name.as_deref());
            tool::exit(tool::ENOENT);
        };
        if let Some(table) = chewing {
            oxpinyin_utils::storage::gen_binary_files::load_chewing(
                Some(table),
                &mut file,
                conf.phonetic_type(),
            );
        }
        file.rewind();
        if let Some(table) = phrase {
            oxpinyin_utils::storage::gen_binary_files::load_phrase_table(
                Some(table),
                &mut file,
                true,
            );
        }
        file.rewind();
        let items = oxpinyin_utils::storage::gen_binary_files::read_phrase_items(
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
