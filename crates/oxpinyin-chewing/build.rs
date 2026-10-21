//! Generate integer-only content rows from the original pinned literals.

use std::{env, fmt::Write as _, fs, path::PathBuf};

#[path = "src/content_table_source.rs"]
mod source;
#[path = "../../tools/codegen/string_tables.rs"]
mod string_tables;

fn main() {
    println!("cargo:rerun-if-changed=src/content_table_source.rs");
    println!("cargo:rerun-if-changed=../../tools/codegen/string_tables.rs");
    let mut pool = string_tables::StringPool::default();
    let mut output = String::from("// Generated from content_table_source.rs; do not edit.\n");
    pool.table(
        &mut output,
        "CONTENT_STRINGS",
        "content_strings",
        source::CONTENT_TABLE.iter().map(|row| {
            [
                row.pinyin,
                row.shengmu,
                row.yunmu,
                row.zhuyin,
                row.luoma,
                row.secondary,
            ]
        }),
        "crate::static_strings",
    );
    writeln!(
        output,
        "pub(crate) static CONTENT_KEYS: [[u8; 3]; {}] = [",
        source::CONTENT_TABLE.len()
    )
    .expect("write key declaration");
    for row in source::CONTENT_TABLE {
        writeln!(output, "[{}, {}, {}],", row.initial, row.middle, row.final_).expect("write key");
    }
    writeln!(output, "];").expect("write key end");
    pool.finish(&mut output);
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    fs::write(out_dir.join("content_table.rs"), output).expect("write content table");
}
