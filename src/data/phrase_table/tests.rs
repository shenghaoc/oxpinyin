// Nextest isolates the process-wide read-fault key by running each test in its own process.
use super::*;

use std::collections::BTreeMap;
use std::sync::Mutex;

struct MemoryDbm {
    data: Mutex<BTreeMap<Vec<u8>, Vec<u8>>>,
}

impl MemoryDbm {
    fn new() -> Self {
        Self {
            data: Mutex::new(BTreeMap::new()),
        }
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        self.data.lock().unwrap().insert(key, value);
    }
}

impl ChewingDbm for MemoryDbm {
    fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
        value.clear();
        let data = self.data.lock().unwrap();
        let stored = data.get(key);
        if let Some(stored) = stored {
            value.extend_from_slice(stored);
        }
        Ok(stored.is_some())
    }

    fn contains(&self, key: &[u8]) -> Result<bool, DictError> {
        Ok(self.data.lock().unwrap().contains_key(key))
    }

    fn walk(
        &self,
        lo: &[u8],
        hi: Option<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<bool, DictError>,
    ) -> Result<(), DictError> {
        let data = self.data.lock().unwrap();
        for (key, value) in data.range(lo.to_vec()..) {
            if hi.is_some_and(|hi| key.as_slice() >= hi) {
                break;
            }
            if visit(key, value)? {
                break;
            }
        }
        Ok(())
    }
}

#[test]
fn search_suggestion_collects_every_longer_phrase_under_the_prefix() {
    let dbm = MemoryDbm::new();
    // 你 is a marker, 你好 / 你们 / 你好吗 are phrases, 好 is a neighbour.
    dbm.put(encode_ucs4_key("你"), Vec::new());
    dbm.put(
        encode_ucs4_key("你好"),
        encode_tokens(&[0x0100_0099, 0x0200_0001]),
    );
    dbm.put(encode_ucs4_key("你好吗"), encode_tokens(&[0x0300_0005]));
    dbm.put(encode_ucs4_key("你们"), encode_tokens(&[0x0100_0098]));
    dbm.put(encode_ucs4_key("好"), encode_tokens(&[0x0100_0011]));
    let table = PhraseTable::new(dbm);
    let words = |text: &str| text.chars().map(u32::from).collect::<Vec<_>>();
    let mut tokens = table.search_suggestion(&words("你")).unwrap();
    tokens.sort_unstable();
    assert_eq!(
        tokens,
        vec![0x0100_0098, 0x0100_0099, 0x0200_0001, 0x0300_0005]
    );
    // The prefix's own tokens are not suggestions.
    let tokens = table.search_suggestion(&words("你好")).unwrap();
    assert_eq!(tokens, vec![0x0300_0005]);
    // A prefix that is no key at all answers nothing, even though
    // longer phrases would extend it.
    let dbm = MemoryDbm::new();
    dbm.put(encode_ucs4_key("你好"), encode_tokens(&[0x0100_0099]));
    let table = PhraseTable::new(dbm);
    assert!(table.search_suggestion(&words("你")).unwrap().is_empty());
    assert!(table.search_suggestion(&[]).unwrap().is_empty());
}

#[test]
fn search_finds_exact_phrase() {
    let dbm = MemoryDbm::new();
    let key = encode_ucs4_key("你好");
    let value = encode_tokens(&[0x0100_0099]);
    dbm.put(key, value);

    let table = PhraseTable::new(dbm);
    let tokens = table.search("你好").unwrap();
    assert_eq!(tokens, vec![0x0100_0099]);
}

#[test]
fn search_miss_returns_empty() {
    let dbm = MemoryDbm::new();
    let table = PhraseTable::new(dbm);
    let tokens = table.search("不存在").unwrap();
    assert!(tokens.is_empty());
}

#[test]
fn search_empty_input_returns_empty() {
    let dbm = MemoryDbm::new();
    let table = PhraseTable::new(dbm);
    let tokens = table.search("").unwrap();
    assert!(tokens.is_empty());
}

#[test]
fn search_prefix_marker_returns_empty() {
    let dbm = MemoryDbm::new();
    let key = encode_ucs4_key("你");
    dbm.put(key, Vec::new());

    let table = PhraseTable::new(dbm);
    let tokens = table.search("你").unwrap();
    assert!(tokens.is_empty());
}

fn native_table() -> PhraseTable {
    PhraseTable::new(
        DefaultDbm::attach(
            &crate::user::native_fixture::system_dir().join("phrase_index.bin"),
            false,
        )
        .unwrap(),
    )
}

/// SEARCH_NONE on failed get/check (phrase_large_table3_tkrzwdb.cpp:136-139,
/// _bdb.cpp:196-198, _kyotodb.cpp:174-177 at 074a2219).
#[test]
fn a_failed_phrase_table_read_is_a_miss() {
    let table = native_table();
    crate::store::fail_point_read_of(&encode_ucs4_key("测测"), 0);
    assert!(table.search("测测").unwrap().is_empty());
}

#[test]
fn a_failed_phrase_table_read_is_a_miss_for_zhuyin() {
    let table = native_table();
    crate::store::fail_point_read_of(&encode_ucs4_key("你你"), 0);
    assert!(table.search("你你").unwrap().is_empty());
}

/// A failed cursor read ends the walk after the prior rows
/// (phrase_large_table3_tkrzwdb.cpp:171-186 at 074a2219).
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
