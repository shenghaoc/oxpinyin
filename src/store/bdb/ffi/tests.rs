use super::*;

// attach_outcome propagates failures detected before DB->open.
#[test]
fn only_a_failed_native_open_is_a_failed_attach() {
    let error = OpenFailure::Before(StoreError::Backend("unsupported runtime".into()));
    assert!(
        matches!(attach_outcome::<Db>(Err(error)), Err(StoreError::Backend(e)) if e.message == "unsupported runtime")
    );
}

fn fresh_cursor(db: &Db) -> Cursor<'_> {
    drop(db.cursor_buffers.take());
    db.cursor().unwrap()
}

type OwnedRow = Option<(Vec<u8>, Vec<u8>)>;

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

fn row(key: &[u8], value: &[u8]) -> (Vec<u8>, Vec<u8>) {
    (key.to_vec(), value.to_vec())
}

// Cursor owns its DB_DBT_USERMEM buffers until drop, including while a nested cursor runs.
#[test]
fn reused_cursors_read_what_fresh_cursors_read_while_another_row_is_live() {
    const WALK: [Seek<'static>; 3] = [Seek::First, Seek::Next, Seek::Next];
    let db = Db::open_in_memory_tree().unwrap();
    db.put(b"a", b"first").unwrap();
    db.put(b"b", b"second").unwrap();
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
    let reused = reads(&mut db.cursor().unwrap(), &WALK);
    assert_eq!(reused, reads(&mut fresh_cursor(&db), &WALK));
    assert_eq!(
        reused,
        [
            Some(row(b"a", b"overwritten during walk")),
            Some(row(b"b", b"second")),
            None
        ]
    );
    assert_eq!(outer_row.value, b"first");
    assert_eq!(outer.get(Seek::Next).unwrap().unwrap().value, b"second");
    drop(outer);
    db.put(b"a", b"overwritten after walk").unwrap();
    let next = reads(&mut db.cursor().unwrap(), &WALK);
    assert_eq!(next, reads(&mut fresh_cursor(&db), &WALK));
    assert_eq!(next[0], Some(row(b"a", b"overwritten after walk")));
}

// Cursor::read retries DB_BUFFER_SMALL with the original seek and retains independent key and value buffers.
#[test]
fn reused_cursor_reads_match_fresh_cursors_across_growth_and_misses() {
    let dir = tempfile::tempdir().unwrap();
    for (case, (key_len, value_len)) in [
        (1, INITIAL_BUFFER),
        (1, INITIAL_BUFFER + 1),
        (INITIAL_BUFFER + 1, 1),
        (INITIAL_BUFFER + 1, INITIAL_BUFFER + 1),
    ]
    .into_iter()
    .enumerate()
    {
        let db = Db::open(
            &dir.path().join(format!("{case}.db")),
            DB_BTREE,
            false,
            true,
            USER_FILE_MODE,
        )
        .unwrap();
        let key = vec![b'm'; key_len];
        let value: Vec<_> = (0..value_len).map(|i| (i % 251) as u8).collect();
        db.put(b"a", b"first").unwrap();
        db.put(&key, &value).unwrap();
        db.put(b"z", b"last").unwrap();
        drop(db.cursor().unwrap());
        let seeks = [
            Seek::AtOrAfter(b"l"),
            Seek::AtOrAfter(&key),
            Seek::First,
            Seek::Next,
            Seek::Next,
            Seek::Next,
            Seek::AtOrAfter(b"zz"),
        ];
        let reused = reads(&mut db.cursor().unwrap(), &seeks);
        assert_eq!(reused, reads(&mut fresh_cursor(&db), &seeks));
        assert_eq!(
            reused,
            [
                Some(row(&key, &value)),
                Some(row(&key, &value)),
                Some(row(b"a", b"first")),
                Some(row(&key, &value)),
                Some(row(b"z", b"last")),
                None,
                None
            ]
        );
    }
}

// Cursor::key requests no value bytes and visits the native row keys in order.
#[test]
fn key_walks_visit_the_keys_of_the_row_walk_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("hash.db"),
        DB_HASH,
        false,
        true,
        USER_FILE_MODE,
    )
    .unwrap();
    for (index, len) in [1, 397_899].into_iter().enumerate() {
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
    assert_eq!(keys.len(), 2);
}

// 074a2219 ngram_bdb.cpp:161,174 and bdb_utils.h:32-33 refuse writes on a DB_RDONLY handle.
#[rstest::rstest]
#[case::put(false)]
#[case::delete(true)]
fn a_read_only_handle_refuses_put_and_del(#[case] delete: bool) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hash.db");
    let db = Db::open(&path, DB_HASH, false, true, USER_FILE_MODE).unwrap();
    db.put(b"key", b"value").unwrap();
    drop(db);
    let db = Db::open(&path, DB_HASH, true, false, USER_FILE_MODE).unwrap();
    let result = if delete {
        db.del(b"key").map(drop)
    } else {
        db.put(b"key", b"other")
    };
    assert!(matches!(result, Err(StoreError::ReadOnly)));
    let mut value = Vec::new();
    assert!(db.get(b"key", &mut value).unwrap());
    assert_eq!(value, b"value");
}

// 074a2219 chewing_large_table2_bdb.cpp:58-60 and ngram_bdb.cpp:54-55 use DB_CREATE alone.
#[test]
fn unnamed_opens_use_the_pins_unthreaded_create_flags() {
    for db in [
        Db::open_in_memory_tree().unwrap(),
        Db::open_in_memory_hash_attach().unwrap().unwrap(),
    ] {
        let get_flags = method!(db.handle, get_open_flags, "DB->get_open_flags").unwrap();
        let mut flags = 0;
        // SAFETY: the handle and output are live and exclusively accessed.
        check(unsafe { get_flags(db.handle, &raw mut flags) }, "get flags").unwrap();
        assert_eq!(flags, sys::DB_CREATE);
    }
}
