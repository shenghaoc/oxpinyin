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

// Rust prefix_upper_bound returns the first byte string beyond the prefix range.
#[test]
fn prefix_upper_bound_is_the_next_sibling() {
    assert_eq!(prefix_upper_bound(&[1, 2]), Some(vec![1, 3]));
    assert_eq!(prefix_upper_bound(&[1, 0xFF]), Some(vec![2]));
    assert_eq!(prefix_upper_bound(&[0xFF, 0xFF]), None);
    assert_eq!(prefix_upper_bound(&[]), None);
}

// Rust prefix_keys_match requires enough stored syllables for the query.
#[test]
fn prefix_filter_checks_the_stored_length() {
    let ni = ChewingKey::from_pinyin("ni").unwrap();
    let hao = ChewingKey::from_pinyin("hao").unwrap();
    for (query, stored, expected) in [(&[ni][..], &[ni, hao][..], true), (&[ni, ni], &[ni], false)]
    {
        assert_eq!(prefix_keys_match(query, stored), expected);
    }
}

// 074a2219 chewing_large_table2.h:145-150 visits matching records in row order.
#[test]
fn search_keeps_matching_tokens_in_stored_order() {
    let ba = ChewingKey::from_pinyin("ba").unwrap().with_tone(1);
    let ni = ChewingKey::from_pinyin("ni").unwrap();
    let hao = ChewingKey::from_pinyin("hao").unwrap();
    let dbm = MemoryDbm::new();
    let mut value = Vec::new();
    for (token, tone) in [(9, 3), (7, 1), (3, 0), (7, 1)] {
        let mut record =
            crate::data::row_format::pinyin_index::encode_item(token, &[ba.with_tone(tone)]);
        record[6..].fill(0xff);
        value.extend(record);
    }
    dbm.put(index_key(&[ba]), value);
    dbm.put(index_key(&[ni]), Vec::new());
    let table = ChewingTable::new(dbm);
    for (query, expected) in [
        (&[ba][..], &[7, 3, 7][..]),
        (&[ni], &[]),
        (&[hao], &[]),
        (&[], &[]),
    ] {
        assert_eq!(table.search(query).unwrap(), expected);
    }
}

// 074a2219 pinyin_phrase3.h:108-144 permits any final for an initial-only syllable.
#[test]
fn an_incomplete_query_uses_the_initial_only_key_and_filters_by_the_query() {
    let dbm = MemoryDbm::new();
    let ba = ChewingKey::from_pinyin("ba").unwrap();
    let bo = ChewingKey::from_pinyin("bo").unwrap();
    let b = ChewingKey::new(ba.initial, 0, 0, 0);
    dbm.put(
        encode_incomplete_key(&[b]),
        encode_items(&[
            PinyinIndexItem {
                token: 0x0100_0001,
                keys: vec![ba.with_tone(1)],
            },
            PinyinIndexItem {
                token: 0x0100_0002,
                keys: vec![bo.with_tone(2)],
            },
        ]),
    );
    let table = ChewingTable::new(dbm);

    let items = table.search(&[b]).unwrap();
    assert_eq!(items.len(), 2);

    let items = table.search(&[ba]).unwrap();
    assert!(items.is_empty());
}

// 074a2219 pinyin_phrase3.h:108-144 compares each complete syllable in a mixed query.
#[test]
fn a_mixed_query_filters_the_complete_syllables_inside_the_incomplete_space() {
    let dbm = MemoryDbm::new();
    let ni = ChewingKey::from_pinyin("ni").unwrap();
    let na = ChewingKey::from_pinyin("na").unwrap();
    let hao = ChewingKey::from_pinyin("hao").unwrap();
    let h = ChewingKey::new(hao.initial, 0, 0, 0);
    let n = ChewingKey::new(ni.initial, 0, 0, 0);
    dbm.put(
        encode_incomplete_key(&[n, h]),
        encode_items(&[
            PinyinIndexItem {
                token: 0x0100_0099,
                keys: vec![ni.with_tone(3), hao.with_tone(3)],
            },
            PinyinIndexItem {
                token: 0x0100_0098,
                keys: vec![na.with_tone(4), hao.with_tone(3)],
            },
        ]),
    );
    let table = ChewingTable::new(dbm);
    let items = table.search(&[ni, h]).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0], 0x0100_0099);
}

// 074a2219 chewing_large_table2_tkrzwdb.cpp:516-541 dispatches on the number of whole keys.
#[test]
fn has_overlong_extension_reads_keys_past_sixteen_words_under_the_query() {
    let ni = ChewingKey::from_pinyin("ni").unwrap();
    let hao = ChewingKey::from_pinyin("hao").unwrap();
    let mut odd = encode_complete_key(&[ni; 17]);
    odd.push(7);
    let mut short_odd = encode_complete_key(&[ni; 8]);
    short_odd.push(7);
    for (query, row, overlong) in [
        (&[ni][..], encode_complete_key(&[ni; 16]), false),
        (&[ni], encode_complete_key(&[ni; 17]), true),
        (&[ni], odd, true),
        (&[ni], short_odd, false),
        (&[ni], encode_complete_key(&[hao; 17]), false),
        (&[], encode_complete_key(&[ni; 17]), false),
    ] {
        let dbm = MemoryDbm::new();
        dbm.put(encode_complete_key(&[ni]), Vec::new());
        dbm.put(row, Vec::new());
        assert_eq!(
            ChewingTable::new(dbm)
                .has_overlong_extension(query)
                .unwrap(),
            overlong
        );
    }
}

// Rust walk_suggestions combines the prefix existence check and the overlong-key scan.
#[test]
fn suggestion_walk_matches_the_separate_probes() {
    let ni = ChewingKey::from_pinyin("ni").unwrap();
    let hao = ChewingKey::from_pinyin("hao").unwrap();
    let ni_hao = || {
        (
            encode_complete_key(&[ni, hao]),
            encode_items(&[PinyinIndexItem {
                token: 0x0100_0099,
                keys: vec![ni.with_tone(3), hao.with_tone(3)],
            }]),
        )
    };
    let overlong = || {
        let mut odd = encode_complete_key(&[ni; 17]);
        odd.push(7);
        (odd, Vec::new())
    };
    let marker = || (encode_complete_key(&[ni]), Vec::new());
    let cases: [Vec<(Vec<u8>, Vec<u8>)>; 5] = [
        vec![marker()],
        vec![marker(), ni_hao()],
        vec![ni_hao()],
        vec![marker(), overlong()],
        vec![marker(), ni_hao(), overlong()],
    ];
    for (case, rows) in cases.into_iter().enumerate() {
        let dbm = MemoryDbm::new();
        for (key, value) in rows {
            dbm.put(key, value);
        }
        let table = ChewingTable::new(dbm);
        let mut fused = Vec::new();
        let fused_overlong = table
            .walk_suggestions(&[ni], |len, items| {
                fused.push((len, items.to_vec()));
                Ok(false)
            })
            .unwrap();
        let exists = table.key_exists(&[ni]).unwrap();
        let separate_overlong = exists && table.has_overlong_extension(&[ni]).unwrap();
        assert_eq!(fused_overlong, separate_overlong, "case {case}");
        if !separate_overlong {
            let expected = if case == 1 {
                vec![(
                    2,
                    vec![PinyinIndexItem {
                        token: 0x0100_0099,
                        keys: vec![ni.with_tone(3), hao.with_tone(3)],
                    }],
                )]
            } else {
                Vec::new()
            };
            assert_eq!(fused, expected, "case {case}");
        }
    }
}

// Rust RawChewingDbm::walk translates visitor stop and preserves visitor errors.
#[test]
fn raw_walk_stops_the_store_scan_and_keeps_errors_distinct() {
    use crate::store::{DefaultStore, RawReadStore as _, WriteStore as _, WriteTxn as _};
    let dir = tempfile::tempdir().expect("scratch directory");
    let path = dir.path().join(format!(
        "walk_stop.{}",
        crate::__internal::DEFAULT_STORE_EXT
    ));
    let store = DefaultStore::create(&path).unwrap();
    store
        .write(|txn| {
            for i in 0_u8..8 {
                txn.put_raw(&[i], &[i])?;
            }
            Ok(())
        })
        .unwrap();
    drop(store);
    let dbm = RawChewingDbm::new(DefaultStore::open_read_only(&path).unwrap());

    let mut seen = Vec::new();
    dbm.walk(&[2], Some(&[8]), |key, _| {
        seen.push(key[0]);
        Ok(seen.len() == 2)
    })
    .unwrap();
    assert_eq!(seen, vec![2, 3]);

    let mut count = 0;
    dbm.walk(&[2], Some(&[8]), |_, _| {
        count += 1;
        Ok(false)
    })
    .unwrap();
    assert_eq!(count, 6);

    let err = dbm
        .walk(&[2], Some(&[8]), |_, _| Err(DictError::Parse))
        .unwrap_err();
    assert!(matches!(err, DictError::Parse), "{err:?}");
}

fn native_table() -> ChewingTable {
    ChewingTable::new(
        DefaultDbm::attach(
            &crate::user::native_fixture::system_dir().join("pinyin_index.bin"),
            false,
        )
        .unwrap(),
    )
}

// 074a2219 chewing_large_table2_tkrzwdb.cpp:146-149 treats a failed get as a miss.
#[test]
fn a_failed_pinyin_table_read_is_a_miss() {
    let table = native_table();
    let keys = [
        ChewingKey::from_pinyin("ni").unwrap(),
        ChewingKey::from_pinyin("hao").unwrap(),
    ];
    assert!(!table.search(&keys).unwrap().is_empty());
    crate::store::fail_point_read_of(&index_key(&keys), 0);
    assert!(table.search(&keys).unwrap().is_empty());
}

// 074a2219 chewing_large_table2_tkrzwdb.cpp:511 skips the walk after a failed existence probe.
#[test]
fn a_failed_pinyin_table_existence_read_is_a_miss() {
    let table = native_table();
    let keys = [ChewingKey::from_pinyin("ni").unwrap()];
    assert!(table.key_exists(&keys).unwrap());
    crate::store::fail_point_read_of(&index_key(&keys), 0);
    assert!(!table.key_exists(&keys).unwrap());
    assert!(
        !table
            .walk_suggestions(&keys, |_, _| panic!("a miss must not walk"))
            .unwrap()
    );
}

// 074a2219 chewing_large_table2_tkrzwdb.cpp:516-541 ends the walk at a failed cursor read.
#[test]
fn a_failed_pinyin_table_cursor_read_ends_the_walk() {
    let table = native_table();
    let ni = ChewingKey::from_pinyin("ni").unwrap();
    let hao = ChewingKey::from_pinyin("hao").unwrap();
    crate::store::fail_point_read_of(&index_key(&[ni, hao]), 0);
    let mut lengths = Vec::new();
    assert!(
        !table
            .walk_suggestions(&[ni], |length, _| {
                lengths.push(length);
                Ok(false)
            })
            .unwrap()
    );
    assert!(!lengths.contains(&2));
}

#[cfg(feature = "kyotocabinet")]
// 074a2219 chewing_large_table2_kyotodb.cpp:159-176 checks the body read after the size check.
#[test]
fn a_pinyin_table_body_read_failure_after_the_check_aborts() {
    const NAME: &str =
        "data::chewing_table::tests::a_pinyin_table_body_read_failure_after_the_check_aborts";
    if std::env::var("OXPINYIN_DATA_CHILD").as_deref() == Ok(NAME) {
        let table = native_table();
        let key = ChewingKey::from_pinyin("ni").unwrap();
        crate::store::fail_point_read_of(&key.to_packed().to_le_bytes(), 1);
        let _ = table.search(&[key]);
        panic!("the checked body read must abort");
    }
    let output = crate::user::test_child::run_exact(NAME, [("OXPINYIN_DATA_CHILD", NAME)]);
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(output.status.signal(), Some(6));
    }
    #[cfg(windows)]
    assert_eq!(
        output.status.code(),
        Some(i32::from_ne_bytes(0xC000_0409_u32.to_ne_bytes()))
    );
}
