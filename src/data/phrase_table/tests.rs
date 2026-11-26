use super::*;

use crate::store::{DefaultStore, WriteStore as _};

// 074a2219 phrase_large_table3_tkrzwdb.cpp:155-190 scans descendants only for a present prefix.
#[test]
fn search_suggestion_collects_every_longer_phrase_under_the_prefix() {
    let dbm = DefaultStore::create_in_memory().unwrap();
    dbm.set_raw(&encode_ucs4_key("你"), &Vec::new()).unwrap();
    dbm.set_raw(
        &encode_ucs4_key("你好"),
        &encode_tokens(&[0x0100_0099, 0x0200_0001]),
    )
    .unwrap();
    dbm.set_raw(&encode_ucs4_key("你好吗"), &encode_tokens(&[0x0300_0005]))
        .unwrap();
    dbm.set_raw(&encode_ucs4_key("你们"), &encode_tokens(&[0x0100_0098]))
        .unwrap();
    dbm.set_raw(&encode_ucs4_key("好"), &encode_tokens(&[0x0100_0011]))
        .unwrap();
    let table = PhraseTable::new(DefaultDbm::new(dbm));
    let words = |text: &str| text.chars().map(u32::from).collect::<Vec<_>>();
    let mut tokens = table.search_suggestion(&words("你")).unwrap();
    tokens.sort_unstable();
    assert_eq!(
        tokens,
        vec![0x0100_0098, 0x0100_0099, 0x0200_0001, 0x0300_0005]
    );
    let tokens = table.search_suggestion(&words("你好")).unwrap();
    assert_eq!(tokens, vec![0x0300_0005]);
    let dbm = DefaultStore::create_in_memory().unwrap();
    dbm.set_raw(&encode_ucs4_key("你好"), &encode_tokens(&[0x0100_0099]))
        .unwrap();
    let table = PhraseTable::new(DefaultDbm::new(dbm));
    assert!(table.search_suggestion(&words("你")).unwrap().is_empty());
    assert!(table.search_suggestion(&[]).unwrap().is_empty());
}

// 074a2219 phrase_large_table3_tkrzwdb.cpp:124-150 decodes the exact row.
#[test]
fn exact_search_distinguishes_populated_marker_missing_and_empty_keys() {
    let dbm = DefaultStore::create_in_memory().unwrap();
    dbm.set_raw(&encode_ucs4_key("你好"), &encode_tokens(&[0x0100_0099]))
        .unwrap();
    dbm.set_raw(&encode_ucs4_key("你"), &Vec::new()).unwrap();
    let table = PhraseTable::new(DefaultDbm::new(dbm));
    for (query, expected) in [
        ("你好", &[0x0100_0099][..]),
        ("你", &[]),
        ("不存在", &[]),
        ("", &[]),
    ] {
        assert_eq!(table.search(query).unwrap(), expected);
    }
}

fn native_table() -> PhraseTable {
    PhraseTable::new(
        DefaultDbm::attach(
            &crate::user::native_fixture::system_dir().join("phrase_index.bin"),
            crate::store::DbmKind::Tree,
        )
        .unwrap(),
    )
}

// 074a2219 phrase_large_table3_tkrzwdb.cpp:171-186 retains rows preceding a failed cursor read.
#[test]
fn a_failed_phrase_table_cursor_read_keeps_the_rows_before_it() {
    use crate::store::RawReadStore as _;
    let prefix = encode_ucs4_key("你");
    let store = crate::store::DefaultStore::open_read_only(
        &crate::user::native_fixture::system_dir().join("phrase_index.bin"),
    )
    .unwrap();
    let mut extensions = Vec::new();
    store
        .range_raw(
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            |key, value| {
                if key.len() > prefix.len() && key.starts_with(&prefix) && !value.is_empty() {
                    extensions.push((key.to_vec(), decode_tokens(value).unwrap()));
                }
                Ok(())
            },
        )
        .unwrap();
    extensions.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(extensions.len() >= 2);
    let table = native_table();
    crate::store::fail_point_read_of(&extensions[1].0, 0);
    assert_eq!(
        table.search_suggestion(&[u32::from('你')]).unwrap(),
        extensions[0].1
    );
}
