#[test]
fn a_missing_file_leaves_the_retained_failed_object_not_an_empty_table() {
    // `PunctTable::attach` allocates the TreeDBM and returns the failed
    // Open's answer with the object still owned (`punct_table_tkrzwdb.cpp:
    // 70-72`, `punct_table_kyotodb.cpp:37-47`, BDB `db_create` then
    // `open`); the facade ignores the answer (`pinyin.cpp:440`) and
    // later calls that object. An empty table would call nothing.
    let dir = tempfile::tempdir().expect("scratch directory");
    let mut table = super::PunctTable::attach(&dir.path().join("punct.bin")).unwrap();
    assert!(!table.is_open());
    assert!(!table.attachment_finished());
    table.finish_attachment();
    assert!(table.attachment_finished());
}

use super::*;

use crate::user::data::memory_dbm::MemoryDbm;

#[test]
fn an_empty_table_answers_nothing() {
    let table = PunctTable::<MemoryDbm>::empty();
    assert!(!table.is_open());
    assert!(table.punctuations(0x0100_0295).unwrap().is_empty());
}

#[test]
fn lookup_finds_punctuation() {
    let dbm = MemoryDbm::new();
    let token: u32 = 0x0100_0295;
    let value = encode_puncts(&["，", "。"]);
    dbm.put(token.to_le_bytes().to_vec(), value);

    let table = PunctTable::new(dbm);
    let result = table.punctuations(token).unwrap();
    assert_eq!(result, ["，", "。"]);
}

#[test]
fn lookup_miss_returns_empty() {
    let dbm = MemoryDbm::new();
    let table = PunctTable::new(dbm);
    let result = table.punctuations(0x0100_0295).unwrap();
    assert!(result.is_empty());
}
