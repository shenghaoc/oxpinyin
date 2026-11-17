use super::*;

#[test]
fn only_a_failed_native_open_is_a_failed_attach() {
    let backend = || StoreError::Backend("x".into());
    assert_eq!(attach_outcome(Ok(7)).unwrap(), Some(7));
    assert_eq!(
        attach_outcome::<u8>(Err(OpenFailure::Open(backend()))).unwrap(),
        None
    );
    // An unsupported libdb has no pin counterpart: an ordinary error.
    assert!(attach_outcome::<u8>(Err(OpenFailure::Before(backend()))).is_err());
}

/// Every pin attach dies where `db_create` fails, and so does this:
/// SIGABRT, with no Rust panic text or other line before it.
#[test]
fn a_failed_db_create_aborts_where_the_pin_asserts() {
    let stderr = crate::death_test::assert_aborts(
        "bdb::ffi::tests::a_failed_db_create_aborts_where_the_pin_asserts",
        || {
            let _ = attach_outcome::<u8>(Err(OpenFailure::Create(StoreError::Backend("x".into()))));
        },
    );
    assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
}

/// A cursor on fresh buffers, as the first cursor on a handle gets: the
/// uncached path the spare-buffer reuse must answer like.
fn fresh_cursor(db: &Db) -> Cursor<'_> {
    drop(db.cursor_buffers.take());
    db.cursor().unwrap()
}

type OwnedRow = Option<(Vec<u8>, Vec<u8>)>;

/// Each answer `cursor` gives to `seeks`, in order, as owned rows.
fn reads(cursor: &mut Cursor<'_>, seeks: &[Seek<'_>]) -> Vec<OwnedRow> {
    seeks
        .iter()
        .map(|&seek| {
            cursor
                .get(seek)
                .unwrap()
                .map(|row| (row.key.to_vec(), row.value.to_vec()))
        })
        .collect()
}

fn row(key: &[u8], value: &[u8]) -> OwnedRow {
    Some((key.to_vec(), value.to_vec()))
}

#[test]
fn reused_cursors_read_what_fresh_cursors_read_while_another_row_is_live() {
    const WALK: [Seek<'static>; 3] = [Seek::First, Seek::Next, Seek::Next];
    let db = Db::open_in_memory_tree().unwrap();
    db.put(b"a", b"first").unwrap();
    db.put(b"b", b"second").unwrap();
    // A finished cursor leaves its buffers for the next one.
    drop(db.cursor().unwrap());
    let mut outer = db.cursor().unwrap();
    let outer_row = outer.get(Seek::First).unwrap().unwrap();
    {
        let mut nested = db.cursor().unwrap();
        assert_eq!(
            nested.get(Seek::AtOrAfter(b"b")).unwrap().unwrap().value,
            b"second"
        );
        db.put(b"a", b"overwritten during walk").unwrap();
        assert_eq!(
            (outer_row.key, outer_row.value),
            (b"a".as_slice(), b"first".as_slice())
        );
    }
    // The nested cursor's buffers serve the next cursor while the outer
    // row is live.
    let reused = reads(&mut db.cursor().unwrap(), &WALK);
    assert_eq!(reused, reads(&mut fresh_cursor(&db), &WALK));
    assert_eq!(
        reused,
        [
            row(b"a", b"overwritten during walk"),
            row(b"b", b"second"),
            None
        ]
    );
    assert_eq!(outer_row.value, b"first");
    assert_eq!(outer.get(Seek::Next).unwrap().unwrap().value, b"second");
    drop(outer);
    db.put(b"a", b"overwritten after walk").unwrap();
    let next = reads(&mut db.cursor().unwrap(), &WALK);
    assert_eq!(next, reads(&mut fresh_cursor(&db), &WALK));
    assert_eq!(next[0], row(b"a", b"overwritten after walk"));
}

#[test]
fn reused_cursor_reads_match_fresh_cursors_across_growth_and_misses() {
    struct TempPath(std::path::PathBuf);
    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    // Keep large records file-backed: an unnamed BDB's tiny cache can
    // spill into its compiled-in /var/tmp instead of the test's temp dir.
    let path = TempPath(
        std::env::temp_dir().join(format!("oxpinyin-cursor-values-{}.db", std::process::id())),
    );
    let db = Db::open(&path.0, DB_BTREE, false, true, USER_FILE_MODE).unwrap();
    db.put(b"a", b"first").unwrap();
    db.put(b"z", b"last").unwrap();
    // A missing seek target between rows, an exact seek and DB_NEXT all
    // reach the same value, including through DB_BUFFER_SMALL.
    let seeks = [
        Seek::AtOrAfter(b"l"),
        Seek::AtOrAfter(b"m"),
        Seek::First,
        Seek::Next,
        Seek::Next,
        Seek::Next,
        Seek::AtOrAfter(b"zz"),
    ];
    for len in [0, 1, INITIAL_BUFFER, INITIAL_BUFFER + 1, 397_899] {
        let expected: Vec<u8> = (0..len).map(|index| (index % 251) as u8).collect();
        db.put(b"m", &expected).unwrap();
        for _ in 0..2 {
            let reused = reads(&mut db.cursor().unwrap(), &seeks);
            assert_eq!(reused, reads(&mut fresh_cursor(&db), &seeks));
            assert_eq!(
                reused,
                [
                    row(b"m", &expected),
                    row(b"m", &expected),
                    row(b"a", b"first"),
                    row(b"m", &expected),
                    row(b"z", b"last"),
                    None,
                    None,
                ]
            );
        }
    }
}

#[test]
fn key_walks_visit_the_keys_of_the_row_walk_in_order() {
    struct TempPath(std::path::PathBuf);
    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    // File-backed, as in the test above, for the overflow record.
    let path =
        TempPath(std::env::temp_dir().join(format!("oxpinyin-key-walk-{}.db", std::process::id())));
    let db = Db::open(&path.0, DB_HASH, false, true, USER_FILE_MODE).unwrap();
    for (index, len) in [0, 1, INITIAL_BUFFER, INITIAL_BUFFER + 1, 397_899]
        .into_iter()
        .enumerate()
    {
        let value: Vec<u8> = (0..len).map(|byte| (byte % 251) as u8).collect();
        db.put(&u32::try_from(index).unwrap().to_le_bytes(), &value)
            .unwrap();
    }
    let mut rows = Vec::new();
    let mut cursor = db.cursor().unwrap();
    let mut seek = Seek::First;
    while let Some(row) = cursor.get(seek).unwrap() {
        rows.push(row.key.to_vec());
        seek = Seek::Next;
    }
    drop(cursor);
    let mut keys = Vec::new();
    let mut cursor = db.cursor().unwrap();
    let mut seek = Seek::First;
    while let Some(key) = cursor.key(seek).unwrap() {
        keys.push(key.to_vec());
        seek = Seek::Next;
    }
    assert_eq!(keys, rows);
    assert_eq!(keys.len(), 5);
}

#[test]
fn reused_cursor_seeks_match_fresh_cursors_across_key_growth() {
    // Key-buffer growth, apart from the large-value disk I/O of the
    // file-backed test above: DB_SET_RANGE input/output and DB_NEXT
    // output must survive key and value buffer retries.
    let db = Db::open_in_memory_tree().unwrap();
    let long_key = vec![b'm'; INITIAL_BUFFER + 1];
    db.put(b"a", b"first").unwrap();
    db.put(b"z", b"last").unwrap();
    let seeks = [
        Seek::AtOrAfter(b"m"),
        Seek::AtOrAfter(&long_key),
        Seek::First,
        Seek::Next,
    ];
    for len in [0, 1, INITIAL_BUFFER, INITIAL_BUFFER + 1] {
        let expected: Vec<u8> = (0..len).map(|index| (index % 251) as u8).collect();
        db.put(&long_key, &expected).unwrap();
        // A cursor that read nothing leaves one-page buffers to reuse.
        drop(db.cursor().unwrap());
        let reused = reads(&mut db.cursor().unwrap(), &seeks);
        assert_eq!(reused, reads(&mut fresh_cursor(&db), &seeks));
        assert_eq!(
            reused,
            [
                row(&long_key, &expected),
                row(&long_key, &expected),
                row(b"a", b"first"),
                row(&long_key, &expected),
            ]
        );
    }
}

#[test]
fn empty_and_missing_ranges_remain_empty_after_buffer_reuse() {
    use crate::{BdbStore, RawReadStore, WriteStore, WriteTxn};
    use std::ops::Bound::{Excluded, Included, Unbounded};
    let store = BdbStore::create_in_memory().unwrap();
    assert_eq!(store.count_raw().unwrap(), 0);
    store.write(|txn| txn.put_raw(b"b", b"value")).unwrap();
    for _ in 0..2 {
        for (lo, hi) in [
            (Unbounded, Excluded(b"b".as_slice())),
            (Included(b"b".as_slice()), Excluded(b"b".as_slice())),
            (Included(b"c".as_slice()), Unbounded),
        ] {
            let mut count = 0;
            store
                .range_raw(lo, hi, |_, _| {
                    count += 1;
                    Ok(())
                })
                .unwrap();
            assert_eq!(count, 0);
        }
        let mut seen = Vec::new();
        store
            .range_raw(Included(b"a"), Unbounded, |key, value| {
                seen.push((key.to_vec(), value.to_vec()));
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, vec![(b"b".to_vec(), b"value".to_vec())]);
    }
}

#[test]
fn a_read_only_handle_refuses_put_and_del() {
    struct TempPath(std::path::PathBuf);
    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    // `Bigram::store` and `remove`, and the `FlexibleBigram` twins, are a
    // bare `DB->put` / `DB->del` (ngram_bdb.cpp:161, :174;
    // flexible_ngram_bdb.h:212, :233 at 074a2219); on a handle opened
    // `DB_RDONLY` (bdb_utils.h:32-33) libdb refuses them and the method
    // answers false. The wrapper's answer is `ReadOnly`, and the record
    // is unchanged.
    let path = TempPath(
        std::env::temp_dir().join(format!("oxpinyin-read-only-{}.db", std::process::id())),
    );
    let db = Db::open(&path.0, DB_HASH, false, true, USER_FILE_MODE).unwrap();
    db.put(b"key", b"value").unwrap();
    drop(db);
    let db = Db::open(&path.0, DB_HASH, true, false, USER_FILE_MODE).unwrap();
    assert!(matches!(
        db.put(b"key", b"other"),
        Err(StoreError::ReadOnly)
    ));
    assert!(matches!(db.del(b"key"), Err(StoreError::ReadOnly)));
    let mut value = Vec::new();
    assert!(db.get(b"key", &mut value).unwrap());
    assert_eq!(value, b"value");
}

#[test]
fn unnamed_opens_use_the_pins_unthreaded_create_flags() {
    for db in [
        Db::open_in_memory_tree().unwrap(),
        Db::open_in_memory_hash_attach().unwrap().unwrap(),
    ] {
        let get_flags = method!(db.handle, get_open_flags, "DB->get_open_flags").unwrap();
        let mut flags = 0;
        // SAFETY: the handle and output local are live, and nothing
        // else uses this database.
        check(unsafe { get_flags(db.handle, &raw mut flags) }, "get flags").unwrap();
        assert_eq!(flags, sys::DB_CREATE);
    }
}
