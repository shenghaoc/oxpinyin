// Nextest isolates the process-wide read-fault key by running each test in its own process.
use super::*;

// ── In-memory DBM for unit testing ───────────────────────────

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
fn prefix_upper_bound_is_the_next_sibling() {
    assert_eq!(prefix_upper_bound(&[1, 2]), Some(vec![1, 3]));
    assert_eq!(prefix_upper_bound(&[1, 0xFF]), Some(vec![2]));
    assert_eq!(prefix_upper_bound(&[0xFF, 0xFF]), None);
    assert_eq!(prefix_upper_bound(&[]), None);
}

#[test]
fn the_row_filter_follows_the_three_upstream_comparisons() {
    let ni3 = ChewingKey::from_pinyin("ni").unwrap().with_tone(3);
    let ni = ChewingKey::from_pinyin("ni").unwrap();
    let n = ChewingKey::new(ni.initial, 0, 0, 0);
    let na = ChewingKey::from_pinyin("na").unwrap();
    // Whether a one-record row of `stored` survives the filter for `query`.
    let keys_match = |query: &[ChewingKey], stored: &[ChewingKey]| {
        let row = encode_items(&[PinyinIndexItem {
            token: 7,
            keys: stored.to_vec(),
        }]);
        search_row(query, Some(&row)).unwrap() == (true, vec![7])
    };
    assert!(keys_match(&[ni], &[ni3]), "zero tone matches any tone");
    assert!(keys_match(&[ni3], &[ni]), "either side zero");
    assert!(!keys_match(&[ni3.with_tone(1)], &[ni3]), "tones differ");
    assert!(
        keys_match(&[n], &[ni3]),
        "incomplete query matches any final"
    );
    assert!(keys_match(&[ni3], &[n]), "incomplete stored matches too");
    assert!(!keys_match(&[na], &[ni3]), "finals differ");
    assert!(prefix_keys_match(&[ni], &[ni3, na]));
    assert!(!prefix_keys_match(&[ni, ni], &[ni3]));
}

fn table_with_ba() -> ChewingTable<MemoryDbm> {
    let dbm = MemoryDbm::new();
    let ba = ChewingKey::from_pinyin("ba").unwrap();
    let ba_item = PinyinIndexItem {
        token: 0x0100_0001,
        keys: vec![ba.with_tone(1)],
    };
    let key = encode_complete_key(&[ba]);
    let value = encode_items(&[ba_item]);
    dbm.put(key, value);
    let ikey = encode_incomplete_key(&[ba]);
    let ivalue = encode_items(&[PinyinIndexItem {
        token: 0x0100_0001,
        keys: vec![ChewingKey::new(ba.initial, 0, 0, 0)],
    }]);
    dbm.put(ikey, ivalue);
    ChewingTable::new(dbm)
}

#[test]
fn search_preserves_matching_token_order_and_duplicates_at_every_stride() {
    let ba = ChewingKey::from_pinyin("ba").unwrap();
    for length in 1..=16 {
        let query = vec![ba.with_tone(1); length];
        let mut value = Vec::new();
        for (token, tone) in [(9, 3), (7, 1), (3, 0), (7, 1)] {
            let keys = vec![ba.with_tone(tone); length];
            let mut record = crate::data::row_format::pinyin_index::encode_item(token, &keys);
            record[4 + 2 * length..].fill(0xff);
            value.extend_from_slice(&record);
        }
        let dbm = MemoryDbm::new();
        dbm.put(index_key(&query), value);
        let table = ChewingTable::new(dbm);
        assert_eq!(table.search(&query).unwrap(), [7, 3, 7], "L={length}");
    }
}

#[test]
fn search_miss_returns_none() {
    let table = table_with_ba();
    let zhong = ChewingKey::from_pinyin("zhong").unwrap();
    let items = table.search(&[zhong]).unwrap();
    assert!(items.is_empty());
}

#[test]
fn search_prefix_marker_returns_continued() {
    let dbm = MemoryDbm::new();
    let ni = ChewingKey::from_pinyin("ni").unwrap();
    let hao = ChewingKey::from_pinyin("hao").unwrap();

    let key2 = encode_complete_key(&[ni, hao]);
    let item = PinyinIndexItem {
        token: 0x0100_0099,
        keys: vec![ni.with_tone(3), hao.with_tone(3)],
    };
    dbm.put(key2, encode_items(&[item]));
    let key1 = encode_complete_key(&[ni]);
    dbm.put(key1, Vec::new());

    let table = ChewingTable::new(dbm);

    let items = table.search(&[ni]).unwrap();
    assert!(items.is_empty());

    let items = table.search(&[ni, hao]).unwrap();
    assert_eq!(items.len(), 1);
}

#[test]
fn search_empty_keys_returns_none() {
    let table = table_with_ba();
    let items = table.search(&[]).unwrap();
    assert!(items.is_empty());
}

#[test]
fn an_incomplete_query_uses_the_initial_only_key_and_filters_by_the_query() {
    // Upstream stores the full keys in both key spaces; the incomplete
    // space's records are told apart by `pinyin_compare_with_tones`.
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

    // The initial alone: both records.
    let items = table.search(&[b]).unwrap();
    assert_eq!(items.len(), 2);

    // A complete query goes to the complete space, which this fixture
    // does not carry.
    let items = table.search(&[ba]).unwrap();
    assert!(items.is_empty());
}

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
    // "ni" + "h": the incomplete-space key `n h`, records filtered to
    // those whose first syllable is `ni`.
    let items = table.search(&[ni, h]).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0], 0x0100_0099);
}

#[test]
fn has_overlong_extension_reads_keys_past_sixteen_words_under_the_query() {
    let ni = ChewingKey::from_pinyin("ni").unwrap();
    let hao = ChewingKey::from_pinyin("hao").unwrap();
    let table_with = |rows: &[Vec<u8>]| {
        let dbm = MemoryDbm::new();
        dbm.put(encode_complete_key(&[ni]), Vec::new());
        for row in rows {
            dbm.put(row.clone(), Vec::new());
        }
        ChewingTable::new(dbm)
    };
    // Sixteen words is the longest key the pin's switch instantiates.
    let sixteen = encode_complete_key(&[ni; 16]);
    assert!(
        !table_with(std::slice::from_ref(&sixteen))
            .has_overlong_extension(&[ni])
            .unwrap()
    );
    // Seventeen words under the query: the pin's `default: abort()`.
    let seventeen = encode_complete_key(&[ni; 17]);
    assert!(
        table_with(std::slice::from_ref(&seventeen))
            .has_overlong_extension(&[ni])
            .unwrap()
    );
    // A trailing odd byte still reads as seventeen words (`size / 2`).
    let mut odd = seventeen.clone();
    odd.push(7);
    assert!(table_with(&[odd]).has_overlong_extension(&[ni]).unwrap());
    // Eight-and-a-half words is not past sixteen.
    let mut short_odd = encode_complete_key(&[ni; 8]);
    short_odd.push(7);
    assert!(
        !table_with(&[short_odd])
            .has_overlong_extension(&[ni])
            .unwrap()
    );
    // A seventeen-word key under another prefix is not an extension.
    let elsewhere = encode_complete_key(&[hao; 17]);
    assert!(
        !table_with(&[elsewhere])
            .has_overlong_extension(&[ni])
            .unwrap()
    );
    // No query, nothing to extend.
    assert!(
        !table_with(&[seventeen])
            .has_overlong_extension(&[])
            .unwrap()
    );
}

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
        // The uncached path: the existence probe and the key-length scan,
        // each on its own.
        let exists = table.key_exists(&[ni]).unwrap();
        let separate_overlong = exists && table.has_overlong_extension(&[ni]).unwrap();
        assert_eq!(fused_overlong, separate_overlong, "case {case}");
        if !separate_overlong {
            // The rows a suggestion walk visits: the `ni'hao` record when
            // the query's own key exists, nothing otherwise.
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

#[test]
fn raw_walk_stops_the_store_scan_and_keeps_errors_distinct() {
    use crate::store::{DefaultStore, RawReadStore as _, WriteStore as _, WriteTxn as _};
    let dir = tempfile::tempdir().expect("scratch directory");
    let path = dir
        .path()
        .join(format!("walk_stop.{}", crate::store::DEFAULT_STORE_EXT));
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

    // A stop after two rows: only two rows are visited and the walk
    // succeeds — the store scan is aborted, not run to the end.
    let mut seen = Vec::new();
    dbm.walk(&[2], Some(&[8]), |key, _| {
        seen.push(key[0]);
        Ok(seen.len() == 2)
    })
    .unwrap();
    assert_eq!(seen, vec![2, 3]);

    // No stop: every row of the range.
    let mut count = 0;
    dbm.walk(&[2], Some(&[8]), |_, _| {
        count += 1;
        Ok(false)
    })
    .unwrap();
    assert_eq!(count, 6);

    // A visitor error comes back as itself, not as the sentinel that
    // aborted the store scan.
    let err = dbm
        .walk(&[2], Some(&[8]), |_, _| {
            Err(DictError::Parse("boom".to_owned()))
        })
        .unwrap_err();
    assert!(
        matches!(err, DictError::Parse(ref m) if m == "boom"),
        "{err:?}"
    );
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

/// A failed get/check is SEARCH_NONE on every backend
/// (chewing_large_table2_tkrzwdb.cpp:146-149, _bdb.cpp:182-184,
/// _kyotodb.cpp:160-163 at 074a2219).
#[test]
fn a_failed_pinyin_table_read_is_a_miss() {
    let table = native_table();
    let keys = [
        ChewingKey::from_pinyin("ni").unwrap(),
        ChewingKey::from_pinyin("hao").unwrap(),
    ];
    assert!(!table.search(&keys).unwrap().is_empty());
    crate::store::read_faults::fail_point_read_of(&index_key(&keys), 0);
    assert!(table.search(&keys).unwrap().is_empty());
}

/// The existence check also treats a failed read as a miss
/// (chewing_large_table2_tkrzwdb.cpp:511 at 074a2219).
#[test]
fn a_failed_pinyin_table_existence_read_is_a_miss() {
    let table = native_table();
    let keys = [ChewingKey::from_pinyin("ni").unwrap()];
    assert!(table.key_exists(&keys).unwrap());
    crate::store::read_faults::fail_point_read_of(&index_key(&keys), 0);
    assert!(!table.key_exists(&keys).unwrap());
    assert!(
        !table
            .walk_suggestions(&keys, |_, _| panic!("a miss must not walk"))
            .unwrap()
    );
}

/// Zhuyin su3 supplies ni3, with the tone cleared in the stored key.
#[test]
fn a_failed_pinyin_table_existence_read_is_a_miss_for_zhuyin() {
    let table = native_table();
    let parse = crate::ZhuyinParser::with_scheme(crate::ZhuyinScheme::Standard).parse_with_options(
        b"su3",
        crate::OptionBits::USE_TONE | crate::OptionBits::FORCE_TONE,
    );
    let key = parse.keys()[0]
        .key()
        .chewing_key()
        .with_tone(parse.keys()[0].tone());
    crate::store::read_faults::fail_point_read_of(&(key.to_packed() & 0x0fff).to_le_bytes(), 0);
    assert!(!table.key_exists(&[key]).unwrap());
    assert!(table.search(&[key]).unwrap().is_empty());
}

/// A failed cursor get ends the walk (chewing_large_table2_tkrzwdb.cpp:516-541).
#[test]
fn a_failed_pinyin_table_cursor_read_ends_the_walk() {
    let table = native_table();
    let ni = ChewingKey::from_pinyin("ni").unwrap();
    let hao = ChewingKey::from_pinyin("hao").unwrap();
    crate::store::read_faults::fail_point_read_of(&index_key(&[ni, hao]), 0);
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

/// IndexItem pointer subtraction keeps only whole records
/// (chewing_large_table2.h:145-150; compatibility policy row 128).
#[test]
fn a_misaligned_pinyin_table_row_keeps_its_whole_records() {
    let scratch = tempfile::tempdir().unwrap();
    let system = crate::user::native_fixture::items_system(
        &crate::user::native_fixture::system_dir(),
        scratch.path(),
        &[(&[0x4e19], &[0x1081], 100)],
        100,
    );
    let mut row = 0x0100_0001_u32.to_le_bytes().to_vec();
    row.extend(0x1081_u16.to_le_bytes());
    row.extend([0; 2 + 4]);
    crate::user::native_fixture::craft_tree(
        &system.join("pinyin_index.bin"),
        &[((0x1081_u16 & 4095).to_le_bytes().to_vec(), row)],
    );
    let table =
        ChewingTable::new(DefaultDbm::attach(&system.join("pinyin_index.bin"), false).unwrap());
    assert_eq!(
        table
            .search(&[ChewingKey::from_pinyin("ba").unwrap()])
            .unwrap(),
        [0x0100_0001]
    );
    let library =
        crate::data::phrase_library::PhraseLibrary::open(&system.join("gb_char.bin")).unwrap();
    assert_eq!(
        library.item_fields(1).unwrap().phrase_text(),
        Some("丙".into())
    );
}

/// The size check succeeds; the checked body read aborts
/// (chewing_large_table2_kyotodb.cpp:159-176, pinyin_utils.h:27-31).
#[cfg(feature = "kyotocabinet")]
#[test]
fn a_pinyin_table_body_read_failure_after_the_check_aborts() {
    const NAME: &str =
        "data::chewing_table::tests::a_pinyin_table_body_read_failure_after_the_check_aborts";
    if std::env::var("OXPINYIN_DATA_CHILD").as_deref() == Ok(NAME) {
        let table = native_table();
        let key = ChewingKey::from_pinyin("ni").unwrap();
        crate::store::read_faults::fail_point_read_of(&key.to_packed().to_le_bytes(), 1);
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
