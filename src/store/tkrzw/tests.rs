use super::{
    Class, Db, OPEN_DEFAULT, OPEN_NO_CREATE, STATUS_SUCCESS, TkrzwStore, TkrzwUserBigramDb,
    copy_tkrzwdb, open_hash, open_hash_create, open_system,
};
use crate::store::{GramReadOwned, RawReadOwned, StoreError, UserBigramDb, WriteStore, WriteTxn};

fn header_record_count(path: &std::path::Path) -> u64 {
    let bytes = std::fs::read(path).unwrap();
    u64::from_be_bytes(bytes[24..32].try_into().unwrap())
}

fn reset_syncs() -> usize {
    super::RESET_SYNCS.with(std::cell::Cell::get)
}

fn sorted_keys(db: &TkrzwUserBigramDb) -> Vec<Option<[u8; 4]>> {
    let mut keys = db.keys().unwrap();
    keys.sort();
    keys
}

fn eager() -> TkrzwUserBigramDb {
    TkrzwUserBigramDb {
        db: std::cell::OnceCell::from(Db::new(Class::Tiny)),
    }
}

fn eager_load(path: &std::path::Path) -> TkrzwUserBigramDb {
    let this = eager();
    let mut file = Db::new(Class::Hash);
    if file
        .open(path, false, OPEN_NO_CREATE)
        .is_ok_and(|status| status.code == STATUS_SUCCESS)
    {
        let db = this.db.get().unwrap();
        copy_tkrzwdb(&file, |key, value| db.set(key, value)).unwrap();
    }
    this
}

type Answers = (Vec<Option<Vec<u8>>>, Vec<Option<[u8; 4]>>, bool);

fn answers(db: &TkrzwUserBigramDb, probes: &[&[u8]]) -> Answers {
    let reads = probes
        .iter()
        .map(|key| {
            let mut value = vec![1, 2, 3];
            let found = db.get(key, &mut value).unwrap();
            assert!(found || value.is_empty(), "a miss clears the buffer");
            found.then_some(value)
        })
        .collect();
    (reads, db.keys().unwrap(), db.is_empty().unwrap())
}

fn saved(db: &TkrzwUserBigramDb, path: &std::path::Path) -> Vec<u8> {
    db.save_db(path).unwrap();
    let mut bytes = std::fs::read(path).unwrap();
    // The file timestamp changes between saves.
    bytes[48..56].fill(0);
    bytes
}

// 074a2219 ngram_tkrzwdb.cpp:48-62,65-83 constructs and saves an empty TinyDBM.
#[test]
fn known_empty_containers_answer_as_an_eager_tiny_dbm() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.db");
    let probes: [&[u8]; 1] = [b"miss"];
    let expected: Answers = (vec![None], Vec::new(), true);
    for db in [
        TkrzwUserBigramDb::empty().unwrap(),
        TkrzwUserBigramDb::load_db(&missing).unwrap(),
    ] {
        let native = eager();
        assert_eq!(answers(&db, &probes), answers(&native, &probes));
        assert_eq!(answers(&db, &probes), expected);
        db.remove(b"miss").unwrap();
        native.remove(b"miss").unwrap();
        assert_eq!(answers(&db, &probes), answers(&native, &probes));

        let lazy_path = dir.path().join("lazy.db");
        assert_eq!(
            saved(&db, &lazy_path),
            saved(&native, &dir.path().join("native.db")),
            "only the timestamp may differ"
        );
        assert_eq!(
            answers(&TkrzwUserBigramDb::load_db(&lazy_path).unwrap(), &probes),
            answers(&eager_load(&lazy_path), &probes)
        );
    }
    assert!(
        !missing.exists(),
        "the missing-file open must not create it"
    );
}

// 074a2219 ngram_tkrzwdb.cpp:125-140 applies Set and Remove to one live TinyDBM.
#[test]
fn a_lazy_container_answers_as_an_eager_tiny_dbm_through_its_first_sets() {
    let probes: [&[u8]; 3] = [b"init", b"next", b"miss"];
    let db = TkrzwUserBigramDb::empty().unwrap();
    let native = eager();
    let before = db.keys().unwrap();
    for container in [&db, &native] {
        container.store(b"init", b"gram").unwrap();
    }
    assert_eq!(answers(&db, &probes), answers(&native, &probes));
    assert_eq!(answers(&db, &probes).0[0], Some(b"gram".to_vec()));
    assert!(!db.is_empty().unwrap());
    assert!(before.is_empty(), "an earlier key snapshot stays empty");
    for container in [&db, &native] {
        container.remove(b"init").unwrap();
    }
    assert_eq!(answers(&db, &probes), answers(&native, &probes));
    assert!(db.is_empty().unwrap());
    for container in [&db, &native] {
        container.store(b"next", b"value").unwrap();
    }
    assert_eq!(answers(&db, &probes), answers(&native, &probes));
    assert_eq!(answers(&db, &probes).0[1], Some(b"value".to_vec()));
}

// 074a2219 ngram_tkrzwdb.cpp:54-60 walks every opened file, even with stale metadata.
#[test]
fn loading_answers_as_the_full_copy_walk_for_empty_unhealthy_and_filled_files() {
    let dir = tempfile::tempdir().unwrap();
    let tokens: Vec<[u8; 4]> = (1_u32..=3).map(u32::to_le_bytes).collect();
    let mut probes: Vec<&[u8]> = tokens.iter().map(<[u8; 4]>::as_slice).collect();
    probes.push(b"absent");
    let filled: Vec<_> = tokens.iter().copied().map(Some).collect();

    let no_rows = dir.path().join("no-rows.db");
    TkrzwUserBigramDb::empty()
        .unwrap()
        .save_db(&no_rows)
        .unwrap();

    let live = dir.path().join("live.db");
    let unhealthy = dir.path().join("unhealthy.db");
    let writer = open_hash_create(&live).unwrap();
    for token in &tokens {
        writer.db.set(token, b"gram").unwrap();
    }
    std::fs::copy(&live, &unhealthy).unwrap();
    drop(writer);
    let file = open_hash(&unhealthy).unwrap();
    assert_eq!(file.db.count().unwrap(), 0);
    assert!(!file.db.is_healthy());
    drop(file);

    let rows = dir.path().join("rows.db");
    let db = TkrzwUserBigramDb::empty().unwrap();
    for token in &tokens {
        db.store(token, b"gram").unwrap();
    }
    db.save_db(&rows).unwrap();

    for (path, keys) in [
        (&no_rows, Vec::new()),
        (&unhealthy, filled.clone()),
        (&rows, filled),
    ] {
        let loaded = TkrzwUserBigramDb::load_db(path).unwrap();
        assert_eq!(
            answers(&loaded, &probes),
            answers(&eager_load(path), &probes),
            "{}",
            path.display()
        );
        assert_eq!(sorted_keys(&loaded), keys, "{}", path.display());
    }
}

// 074a2219 ngram_tkrzwdb.cpp:54-60 and tkrzwdb_utils.h:56-69 insert in source iterator order.
#[test]
fn loading_preserves_the_pins_colliding_bucket_chain_and_values() {
    let a = 0x0100_0d46_u32.to_le_bytes();
    let b = 0x0100_4404_u32.to_le_bytes();
    let c = 0x0100_4dd8_u32.to_le_bytes();
    let rows = [
        (a.to_vec(), vec![0, 1, 0, 255]),
        (b.to_vec(), Vec::new()),
        (c.to_vec(), (0_u8..=255).cycle().take(804).collect()),
    ];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("user_bigram.db");
    let file = open_hash_create(&path).unwrap();
    for (key, value) in &rows {
        file.db.set(key, value).unwrap();
    }
    drop(file);

    let file = open_hash(&path).unwrap();
    let mut source_keys = Vec::new();
    copy_tkrzwdb(&file.db, |key, _| {
        source_keys.push(key.to_vec());
        Ok(())
    })
    .unwrap();
    assert_eq!(source_keys, [b.to_vec(), a.to_vec(), c.to_vec()]);
    drop(file);

    let loaded = TkrzwUserBigramDb::load_db(&path).unwrap();
    let expected = vec![rows[2].clone(), rows[0].clone(), rows[1].clone()];
    let mut actual = Vec::new();
    copy_tkrzwdb(loaded.db.get().unwrap(), |key, value| {
        actual.push((key.to_vec(), value.to_vec()));
        Ok(())
    })
    .unwrap();
    assert_eq!(
        actual, expected,
        "native bucket-chain order, not sorted rows"
    );

    let before = loaded.keys().unwrap();
    loaded.store(&a, b"").unwrap();
    assert_eq!(loaded.read_gram(&a).unwrap(), Some(Vec::new()));
    assert_eq!(loaded.keys().unwrap(), before);
}

// 074a2219 flexible_ngram_tkrzwdb.h:84-90,236-246,257-263 synchronizes on reset.
#[test]
fn a_deferred_write_and_remove_leave_the_synchronize_to_flush() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flexible.db");
    let store = open_hash_create(&path).unwrap();
    store
        .write_deferred(|txn| txn.put_raw(b"key1", b"one"))
        .unwrap();
    store
        .write_deferred(|txn| txn.put_raw(b"key2", b"two"))
        .unwrap();
    assert_eq!(header_record_count(&path), 0);
    store.flush().unwrap();
    assert_eq!(header_record_count(&path), 2);
    store.write_deferred(|txn| txn.remove_raw(b"key1")).unwrap();
    assert_eq!(header_record_count(&path), 2);
    store.flush().unwrap();
    assert_eq!(header_record_count(&path), 1);
    store.write(|txn| txn.put_raw(b"key3", b"three")).unwrap();
    assert_eq!(header_record_count(&path), 2);
}

// 074a2219 chewing_large_table2_tkrzwdb.cpp:63-69, ngram_tkrzwdb.cpp:40-46 and register row 157.
#[test]
fn a_system_tree_synchronizes_before_close_and_a_hash_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let tree = dir.path().join("pinyin_index.bin");
    let hash = dir.path().join("bigram.db");
    let mut db = Db::new(Class::Tree);
    assert_eq!(
        db.open(&tree, true, OPEN_DEFAULT).unwrap().code,
        STATUS_SUCCESS
    );
    drop(db);
    open_hash_create(&hash).unwrap();

    let before = reset_syncs();
    drop(open_system(&tree, false, true).unwrap().unwrap());
    assert_eq!(reset_syncs(), before + 1);
    drop(open_system(&hash, true, true).unwrap().unwrap());
    assert_eq!(reset_syncs(), before + 1);
    assert!(
        open_system(&dir.path().join("missing.bin"), false, true)
            .unwrap()
            .is_none()
    );
    assert_eq!(reset_syncs(), before + 1);
}

// Frozen linked-library observations in docs/findings/tkrzw-distro-compat.md cover all four DBM classes.
#[test]
fn tkrzw_library_honours_its_sentinel_protocol() {
    type Create = fn(&std::path::Path) -> Result<TkrzwStore, StoreError>;

    const CAUSE: &str = "the linked libtkrzw does not recognise its own NOOP/REMOVE \
             sentinels (known cause: an LTO build of tkrzw older than 1.0.34); see \
             docs/findings/tkrzw-distro-compat.md";

    #[expect(
        clippy::needless_pass_by_value,
        reason = "The helper owns and closes the native database before the fixture directory is released"
    )]
    fn check(class: &str, store: TkrzwStore) {
        store
            .write(|txn| {
                txn.put_raw(b"kept", b"v")?;
                txn.put_raw(b"removed", b"v")
            })
            .unwrap();

        store.write(|txn| txn.remove_raw(b"absent")).unwrap();
        assert_eq!(
            store.read_raw(b"absent").unwrap(),
            None,
            "{class}: removing an absent record stored one: {CAUSE}"
        );

        store.write(|txn| txn.remove_raw(b"removed")).unwrap();
        assert_eq!(
            store.read_raw(b"removed").unwrap(),
            None,
            "{class}: a removed record is still stored: {CAUSE}"
        );

        assert_eq!(store.read_raw(b"kept").unwrap(), Some(b"v".to_vec()));
    }

    let file_classes: [(&str, Create); 2] = [
        ("TreeDBM", TkrzwStore::create),
        ("HashDBM", TkrzwStore::create_hash),
    ];
    for (class, create) in file_classes {
        let dir = tempfile::tempdir().unwrap();
        check(class, create(&dir.path().join(class)).unwrap());
    }

    check("BabyDBM", TkrzwStore::create_in_memory().unwrap());

    let bigram = TkrzwUserBigramDb::empty().unwrap();
    bigram.store(b"kept", b"v").unwrap();
    bigram.store(b"gone", b"v").unwrap();
    bigram.remove(b"miss").unwrap();
    assert_eq!(
        bigram.read_gram(b"miss").unwrap(),
        None,
        "TinyDBM: removing an absent record stored one: {CAUSE}"
    );
    bigram.remove(b"gone").unwrap();
    assert_eq!(
        bigram.read_gram(b"gone").unwrap(),
        None,
        "TinyDBM: a removed record is still stored: {CAUSE}"
    );
    assert_eq!(bigram.read_gram(b"kept").unwrap(), Some(b"v".to_vec()));
}
