//! tests/storage/test_punct_table.cpp:25-77 at 074a2219.
use super::*;
use crate::storage::dbm::Tree;
use crate::storage::punctuation::{append_punctuation, remove_punctuation};
use oxpinyin_capi_marshal::phrase_text::phrase_text_bytes;
use oxpinyin_data::row_format::punct::decode_puncts;
use std::ffi::CString;

fn print_table(table: &Tree) {
    for key in required(table.native_keys()) {
        let token = token(&key);
        output::punct_token(token);
        let value = table.get(&key).unwrap_or_default();
        if !value.is_empty() {
            let puncts: Vec<_> = decode_puncts(&value)
                .iter()
                .map(|text| phrase_text_bytes(text).into_owned())
                .collect();
            let line = CString::new(puncts.join(&b' ')).unwrap_or_else(|_| tool::abort());
            output::punctuations(&line);
        }
    }
}

/// Execute the program under its fixed names ([`crate::fixed_tmp`]).
pub fn main() {
    let table = required(Tree::create_for_test(
        crate::fixed_tmp::path("punct.bin").as_path(),
    ));
    output::message(Message::Created);
    print_table(&table);
    for punctuation in [c"……", c"…", c"？"] {
        append_punctuation(&table, 1, punctuation);
    }
    output::message(Message::Inserted);
    print_table(&table);
    let _ = remove_punctuation(&table, 1, "…");
    output::message(Message::Removed);
    print_table(&table);
    required(table.save_db(crate::fixed_tmp::path("snapshot.db").as_path()));
    drop(table);
    let table = required(Tree::load_db(
        crate::fixed_tmp::path("snapshot.db").as_path(),
    ));
    output::message(Message::Reloaded);
    print_table(&table);
    for punctuation in ["……", "？"] {
        let _ = remove_punctuation(&table, 1, punctuation);
        output::message(Message::Removed);
        print_table(&table);
    }
}
