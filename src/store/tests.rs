use std::ops::{
    Bound::{Excluded, Included, Unbounded},
    ControlFlow,
};

use rstest::rstest;

use super::{
    DefaultStore, DefaultUserBigramDb, GramReadOwned, RawReadOwned, RawReadStore, StoreError,
    UserBigramDb, WriteStore,
};

fn put_rows(store: &DefaultStore, rows: &[(&[u8], &[u8])]) {
    for &(key, value) in rows {
        store.set_raw(key, value).unwrap();
    }
}

// RawReadStore::range_raw compares raw byte bounds and visits each matching key and value.
#[test]
fn empty_bounds_never_match_or_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.db");
    let rows: &[(&[u8], &[u8])] = &[(b"a", b"1"), (b"b", b"2"), (b"c", b"3"), (b"d", b"4")];
    let writer = DefaultStore::create(&path).unwrap();
    put_rows(&writer, rows);
    drop(writer);
    let db = DefaultStore::open_read_only(&path).unwrap();
    for (lo, hi, expected) in [
        (
            Included(b"b".as_slice()),
            Included(b"c".as_slice()),
            &rows[1..3],
        ),
        (Included(b"b"), Unbounded, &rows[1..]),
        (Included(b""), Unbounded, rows),
        (Excluded(b""), Unbounded, rows),
        (Unbounded, Included(b""), &rows[..0]),
        (Unbounded, Excluded(b""), &rows[..0]),
    ] {
        let mut count = 0;
        db.range_raw(lo, hi, |key, value| {
            assert_eq!((key, value), expected[count]);
            count += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(count, expected.len());
    }
}

// RawReadStore::scan_raw stops at Break or Err and preserves the visitor's error.
#[rstest]
#[case::stop(false)]
#[case::error(true)]
fn scan_break_stops_and_error_propagates(#[case] fail: bool) {
    let dir = tempfile::tempdir().unwrap();
    let db = DefaultStore::create(&dir.path().join("tree.db")).unwrap();
    put_rows(&db, &[(b"a", b"1"), (b"b", b"2"), (b"c", b"3")]);
    let mut seen = Vec::new();
    let result = db.scan_raw(Unbounded, Unbounded, |key, _| {
        seen.push(key.to_vec());
        if key == b"b" {
            if fail {
                return Err(StoreError::InvalidInput("visitor failed"));
            }
            return Ok(ControlFlow::Break(()));
        }
        Ok(ControlFlow::Continue(()))
    });
    assert_eq!(seen, [b"a", b"b"]);
    match result {
        Ok(()) => assert!(!fail),
        Err(StoreError::InvalidInput("visitor failed")) => assert!(fail),
        other => panic!("unexpected visitor result {other:?}"),
    }
}

// RawReadStore::get_raw replaces a dirty buffer on a hit and clears it on a miss.
#[test]
fn point_reads_refill_one_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let db = DefaultStore::create(&dir.path().join("tree.db")).unwrap();
    let long = [7; 5000];
    put_rows(&db, &[(b"long", &long), (b"short", b"ab"), (b"empty", b"")]);
    let mut value = Vec::new();
    for (key, expected) in [
        (b"long".as_slice(), Some(long.as_slice())),
        (b"short", Some(b"ab")),
        (b"empty", Some(b"")),
        (b"missing", None),
    ] {
        value.push(1);
        assert_eq!(db.get_raw(key, &mut value).unwrap(), expected.is_some());
        assert_eq!(value, expected.unwrap_or_default());
    }
}

// RawReadStore::contains_raw and get_raw distinguish an empty continuation marker from a miss.
#[test]
fn empty_value_is_a_record() {
    let db = DefaultStore::create_in_memory().unwrap();
    let empty = Vec::new();
    put_rows(&db, &[(b"existing", b"v")]);
    put_rows(&db, &[(b"fresh", &empty), (b"existing", &empty)]);
    for key in [b"fresh".as_slice(), b"existing"] {
        assert_eq!(db.read_raw(key).unwrap(), Some(Vec::new()));
        assert!(db.contains_raw(key).unwrap());
    }
    assert!(!db.contains_raw(b"absent").unwrap());
}

// WriteStore returns ReadOnly on mutation of an opened reader and preserves its data.
#[test]
fn read_only_reopen_preserves_reads_and_rejects_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reader.db");
    let writer = DefaultStore::create(&path).unwrap();
    put_rows(&writer, &[(b"k", b"old")]);
    drop(writer);
    let reader = DefaultStore::open_read_only(&path).unwrap();
    let result = reader.set_raw(b"k", b"new");
    assert!(result.is_err(), "a read-only handle refuses a write");
    assert_eq!(
        reader.read_raw(b"k").unwrap().as_deref(),
        Some(b"old".as_slice())
    );
}

// WriteStore::create_in_memory produces a fresh, mutable ordered tree.
#[test]
fn in_memory_store_behaves_as_a_tree_store() {
    let db = DefaultStore::create_in_memory().unwrap();
    assert_eq!(db.count_raw().unwrap(), 0);
    put_rows(&db, &[(b"b", b"2"), (b"a", b"1")]);
    let mut rows = Vec::new();
    db.range_raw(Unbounded, Unbounded, |k, v| {
        rows.push((k.to_vec(), v.to_vec()));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        rows,
        [
            (b"a".to_vec(), b"1".to_vec()),
            (b"b".to_vec(), b"2".to_vec())
        ]
    );
}

mod key_ordering {
    use super::*;

    // docs/findings/store-key-ordering.md specifies unsigned lexicographic order for walks and bounds.
    #[test]
    fn walks_raw_keys_in_byte_order_and_that_is_not_integer_order() {
        let dir = tempfile::tempdir().unwrap();
        let db = DefaultStore::create(&dir.path().join("tree.db")).unwrap();
        let keys = [0xff_u32, 1, 0x100, 0x0100_0000].map(u32::to_le_bytes);
        for key in &keys {
            put_rows(&db, &[(key, b"v")]);
        }
        for (lo, hi, expected) in [
            (
                Unbounded,
                Unbounded,
                &[[0, 0, 0, 1], [0, 1, 0, 0], [1, 0, 0, 0], [255, 0, 0, 0]][..],
            ),
            (
                Included(keys[2].as_slice()),
                Excluded(keys[0].as_slice()),
                &[[0, 1, 0, 0], [1, 0, 0, 0]][..],
            ),
        ] {
            let mut walked = Vec::new();
            db.range_raw(lo, hi, |key, _| {
                walked.push(key.to_vec());
                Ok(())
            })
            .unwrap();
            assert_eq!(walked, expected);
        }
    }
}

// 074a2219 ngram_bdb.cpp:67-70, ngram_kyotodb.cpp:58-64, ngram_tkrzwdb.cpp:52-58 leave an empty container.
#[test]
fn a_missing_file_loads_an_empty_container() {
    let dir = tempfile::tempdir().unwrap();
    let db = DefaultUserBigramDb::load_db(&dir.path().join("missing.db")).unwrap();
    assert!(db.keys().unwrap().is_empty());
}

// UserBigramDb::store, get, remove and keys operate on the same predecessor rows.
#[test]
fn store_get_remove_and_the_walk_see_the_same_records() {
    let db = DefaultUserBigramDb::empty().unwrap();
    let [kept, removed, absent] = [8_u32, 7, 999].map(u32::to_le_bytes);
    for key in [&kept, &removed] {
        db.store(key, b"gram").unwrap();
    }
    db.remove(&removed).unwrap();
    db.remove(&absent).unwrap();
    assert_eq!(db.read_gram(&removed).unwrap(), None);
    assert_eq!(db.read_gram(&kept).unwrap(), Some(b"gram".to_vec()));
    assert_eq!(db.keys().unwrap(), [Some(kept)]);
}

// UserBigramDb::keys represents each non-four-byte predecessor in an external file as None.
#[rstest]
#[case::short(b"abc")]
#[case::long(b"abcde")]
fn keys_mark_a_key_that_is_not_four_bytes_in_its_place(#[case] malformed: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("user_bigram.db");
    let key = 3_u32.to_le_bytes();
    crate::user::native_fixture::craft_user_bigram(
        &path,
        &[
            (key.to_vec(), b"gram".to_vec()),
            (malformed.to_vec(), b"gram".to_vec()),
        ],
    );
    let db = DefaultUserBigramDb::load_db(&path).unwrap();
    let mut keys = db.keys().unwrap();
    keys.sort();
    assert_eq!(keys, [None, Some(key)]);
}

// 074a2219 tests/storage/test_ngram.cpp:39-46,69-79 attaches, walks, saves and reloads both predecessors.
#[test]
fn a_native_hash_snapshot_keeps_both_predecessor_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    std::fs::write(&path, []).unwrap();
    let bigram = DefaultStore::create_hash(&path).expect("attach empty file");
    let rows = [
        [16_u32, 1, 16, 2, 1, 3, 32, 4, 4, 6, 2],
        [32_u32, 2, 1, 3, 32, 4, 4, 5, 8, 6, 2],
    ];
    let rows = rows.map(|row| {
        row.into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect::<Vec<_>>()
    });
    for (token, row) in [1_u32, 2].into_iter().zip(&rows) {
        bigram.set_raw(&token.to_ne_bytes(), row).unwrap();
    }
    let mut keys = Vec::new();
    bigram.native_keys(&mut keys).unwrap();
    let order: [u32; 2] = if cfg!(feature = "bdb") {
        [2, 1]
    } else {
        [1, 2]
    };
    assert_eq!(keys, order.map(|token| token.to_ne_bytes().to_vec()));

    let snapshot = dir.path().join("snapshot.db");
    std::fs::write(&snapshot, b"stale").unwrap();
    bigram.save_bigram_db(&snapshot).expect("snapshot");
    drop(bigram);

    let copy = DefaultUserBigramDb::load_db(&snapshot).expect("load snapshot");
    for (token, row) in [1_u32, 2].into_iter().zip(rows) {
        assert_eq!(copy.read_gram(&token.to_ne_bytes()).unwrap(), Some(row));
    }
}

// UserBigramDb::save_db writes all live records in the native user-bigram format.
#[test]
fn save_then_load_keeps_every_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("user_bigram.db");
    let db = DefaultUserBigramDb::empty().expect("empty");
    for token in [1_u32, 0x0100_0001] {
        db.store(
            &token.to_le_bytes(),
            &[token.to_le_bytes(), [9, 0, 0, 0]].concat(),
        )
        .expect("store");
    }
    db.save_db(&path).expect("save");
    let reloaded = DefaultUserBigramDb::load_db(&path).expect("load");
    let mut before = db.keys().expect("keys");
    let mut after = reloaded.keys().expect("keys");
    before.sort();
    after.sort();
    assert_eq!(before, after);
    for token in [1_u32, 0x0100_0001] {
        assert_eq!(
            reloaded.read_gram(&token.to_le_bytes()).expect("get"),
            db.read_gram(&token.to_le_bytes()).expect("get")
        );
    }
}

// 074a2219 ngram_bdb.cpp:84-86, ngram_kyotodb.cpp:86-88 and ngram_tkrzwdb.cpp:69-72 refuse an unlink error.
#[rstest]
#[case::index(false)]
#[case::bigram(true)]
fn a_save_fails_when_the_old_path_cannot_be_unlinked(#[case] bigram: bool) {
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("keep"), b"x").unwrap();
    let result = if bigram {
        DefaultUserBigramDb::empty().unwrap().save_db(&blocked)
    } else {
        DefaultStore::write_user_index(&blocked, |sink| sink(b"k", b"v"))
    };
    assert!(matches!(result, Err(StoreError::Io(_))));
    assert_eq!(std::fs::read(blocked.join("keep")).unwrap(), b"x");
}
