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
    // The spelling column is also stored inline as an integer key: the
    // spelling's bytes, zero-padded to eight, read big-endian. No spelling
    // holds a NUL byte, so the integer order is the byte order of the
    // spellings and integer equality is spelling equality.
    writeln!(
        output,
        "pub(crate) static CONTENT_KEYS: [ContentKey; {}] = [",
        source::CONTENT_TABLE.len()
    )
    .expect("write key declaration");
    let mut previous = None;
    for row in source::CONTENT_TABLE {
        let bytes = row.pinyin.as_bytes();
        assert!(
            bytes.len() <= 8 && !bytes.contains(&0),
            "content spelling {:?} does not fit an 8-byte key",
            row.pinyin
        );
        let mut padded = [0_u8; 8];
        padded[..bytes.len()].copy_from_slice(bytes);
        let spelling = u64::from_be_bytes(padded);
        assert!(
            previous.is_none_or(|previous| previous < spelling),
            "content spellings must be strictly increasing"
        );
        previous = Some(spelling);
        writeln!(
            output,
            "ContentKey {{ spelling: {spelling:#018x}, initial: {}, middle: {}, final_: {} }},",
            row.initial, row.middle, row.final_
        )
        .expect("write key");
    }
    writeln!(output, "];").expect("write key end");
    pool.finish(&mut output);
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    fs::write(out_dir.join("content_table.rs"), output).expect("write content table");
}
