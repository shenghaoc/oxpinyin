use super::*;

use crate::data::memory_dbm::MemoryDbm;

// Rust PunctTable::empty exposes no open table or entries.
#[test]
fn an_empty_table_answers_nothing() {
    let table = PunctTable::<MemoryDbm>::empty();
    assert!(!table.is_open());
    assert!(table.punctuations(0x0100_0295).unwrap().is_empty());
}

// 074a2219 punct_table.cpp:148-164 returns the entry for the requested token.
#[test]
fn lookup_finds_punctuation() {
    let dbm = MemoryDbm::new();
    let token: u32 = 0x0100_0295;
    let value = [0xff0c_u32, 0, 0x3002, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    dbm.put(token.to_le_bytes().to_vec(), value);

    let table = PunctTable::new(dbm);
    for (query, expected) in [(token, &["，", "。"][..]), (token + 1, &[][..])] {
        assert_eq!(table.punctuations(query).unwrap(), expected);
    }
}
