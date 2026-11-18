use std::path::PathBuf;

use super::{
    Class, Db, OPEN_DEFAULT, OPEN_NO_CREATE, STATUS_NOT_FOUND_ERROR, STATUS_PRECONDITION_ERROR,
    STATUS_SUCCESS, TkrzwUserBigramDb, copy_tkrzwdb, open_hash, open_hash_create, open_system,
};
use crate::store::{StoreError, UserBigramDb, WriteStore, WriteTxn};

/// `num_records` as the header on disk holds it, eight big-endian bytes
/// at 24 (`META_OFFSET_NUM_RECORDS`); only `Synchronize` and `Close`
/// write it.
fn header_record_count(path: &std::path::Path) -> u64 {
    let bytes = std::fs::read(path).unwrap();
    u64::from_be_bytes(bytes[24..32].try_into().unwrap())
}

fn reset_syncs() -> usize {
    super::RESET_SYNCS.with(std::cell::Cell::get)
}

/// A directory under the temp directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("oxpinyin-tkrzw-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sorted_keys(db: &TkrzwUserBigramDb) -> Vec<Option<[u8; 4]>> {
    let mut keys = db.keys().unwrap();
    keys.sort();
    keys
}

/// The tkrzw status code a backend error carries.
fn backend_code(error: &StoreError) -> Option<i32> {
    match error {
        StoreError::Backend(error) => error.code(),
        StoreError::Io(_)
        | StoreError::ReadOnly
        | StoreError::InvalidInput(_)
        | StoreError::ReadAfterCheck
        | StoreError::ShortArrayHeader
        | StoreError::FatalCopy => None,
    }
}

#[test]
fn a_failed_open_retains_the_concrete_object_whose_calls_answer_as_the_pins() {
    // `m_db = new TreeDBM; return m_db->Open(...).IsOK();`
    // (`chewing_large_table2_tkrzwdb.cpp:87-88`,
    // `phrase_large_table3_tkrzwdb.cpp:77-79`, `punct_table_tkrzwdb.cpp:
    // 70-72`) and `m_db = new HashDBM; return m_db->Open(...).IsOK();`
    // (`ngram_tkrzwdb.cpp:96-98`): a failed attach returns false with
    // the allocated object still owned, and the pin's later calls reach
    // it. A missing file under `OPEN_NO_CREATE` fails in the file
    // layer with `ENOENT`, which `GetErrnoStatus` classifies as
    // `NOT_FOUND_ERROR` (`tkrzw_file_mmap.cc:110-177`,
    // `tkrzw_lib_common.cc:247-278`); every later record call answers
    // `PRECONDITION_ERROR` "not opened database" (`tkrzw_dbm_hash.cc:
    // 400-401`, `tkrzw_dbm_tree.cc:563-564`; the iterator's `First`
    // and `Jump` likewise, `tkrzw_dbm_tree.cc:2142-2176`), and so do
    // the owner's `Synchronize` and `Close` at reset
    // (`tkrzw_dbm_tree.cc:919-924`, `:511-515`).
    let dir = TempDir::new("failed-open");
    for class in [Class::Tree, Class::Hash] {
        let path = dir.0.join(format!("{class:?}-missing"));
        let mut db = Db::new(class);
        let status = db.open(&path, false, OPEN_NO_CREATE).unwrap();
        assert_eq!(status.code, STATUS_NOT_FOUND_ERROR, "{class:?}");
        assert!(
            db.is_retained(),
            "{class:?}: the native object survives its failed Open"
        );
        assert!(!db.is_open(), "{class:?}");
        assert!(!path.exists(), "{class:?}: OPEN_NO_CREATE created nothing");

        let mut value = Vec::new();
        let get = db.get(b"key", &mut value).unwrap_err();
        assert_eq!(
            backend_code(&get),
            Some(STATUS_PRECONDITION_ERROR),
            "{class:?}: {get}"
        );
        let check = db.check(b"key").unwrap_err();
        assert_eq!(
            backend_code(&check),
            Some(STATUS_PRECONDITION_ERROR),
            "{class:?}"
        );
        let mut cursor = db.cursor().unwrap();
        assert_eq!(cursor.first().code, STATUS_PRECONDITION_ERROR, "{class:?}");
        assert_eq!(
            cursor.jump(b"key").code,
            STATUS_PRECONDITION_ERROR,
            "{class:?}"
        );
        drop(cursor);
        let keys = db.keys().unwrap_err();
        assert_eq!(
            backend_code(&keys),
            Some(STATUS_PRECONDITION_ERROR),
            "{class:?}"
        );
        let sync = db.synchronize(false).unwrap_err();
        assert_eq!(
            backend_code(&sync),
            Some(STATUS_PRECONDITION_ERROR),
            "{class:?}"
        );
        // The object is still the native one, and a later successful
        // `Open` of the same object works: the pin re-attaches through
        // `reset()` and a new allocation, but the library's own state
        // machine is what retention preserves.
        assert!(db.is_retained(), "{class:?}");
        let status = db.open(&path, true, OPEN_DEFAULT).unwrap();
        assert_eq!(
            status.code,
            STATUS_SUCCESS,
            "{class:?}: {}",
            super::message_text(&status)
        );
        assert!(db.is_open(), "{class:?}");
        db.set(b"key", b"value").unwrap();
        assert!(db.get(b"key", &mut value).unwrap());
        assert_eq!(value, b"value");
    }
}

#[test]
fn a_failed_system_attach_keeps_the_object_and_its_later_calls_are_inert() {
    // `pinyin_init` ignores every table's attach answer and later calls
    // the same object (pinyin.cpp:357,369,396,440 at 074a2219): on
    // tkrzw the retained TreeDBM or HashDBM answers PRECONDITION_ERROR,
    // which each owner reads as a miss, and reset synchronizes (Tree
    // owners), closes and deletes it.
    let dir = TempDir::new("system-attach");
    for hash in [false, true] {
        let handle = match open_system(&dir.0.join("missing"), hash, true).unwrap() {
            Err(handle) => handle,
            Ok(_) => panic!("a missing file opened (hash: {hash})"),
        };
        assert!(handle.db.is_retained());
        assert!(!handle.db.is_open());
        handle.read(b"key");
        handle.suggest(b"key");
        drop(handle);
        assert!(!dir.0.join("missing").exists(), "no file was created");
    }
    // The same call opens a present file.
    let path = dir.0.join("present.db");
    {
        let store = open_hash_create(&path).unwrap();
        store.db.set(b"k", b"v").unwrap();
        store.db.synchronize(false).unwrap();
    }
    let mut value = Vec::new();
    match open_system(&path, true, true).unwrap() {
        Ok(store) => assert!(store.db.get(b"k", &mut value).unwrap()),
        Err(_) => panic!("a present file did not open"),
    }
}

#[test]
fn a_failed_bigram_attach_keeps_its_hashdbm_until_reset() {
    use crate::store::SystemAttachment;

    // `Bigram::attach` makes one `Open` on a new `HashDBM`, with no
    // CREATE retry and no signature check, and keeps the object when
    // the open fails (ngram_tkrzwdb.cpp:86-99). `load`, `store` and
    // `get_all_items` reach it (`Get`, `Set`, `ProcessEach`: :102-133,
    // :160-172), and `reset()` closes and deletes it with no
    // `Synchronize` (:40-46).
    let dir = TempDir::new("failed-bigram");
    let path = dir.0.join("missing").join("bigram.db");
    for read_only in [true, false] {
        let handle = match open_system(&path, true, read_only).unwrap() {
            Err(handle) => handle,
            Ok(_) => panic!("read only: {read_only}: a missing directory opened"),
        };
        assert!(handle.db.is_retained(), "read only: {read_only}");
        assert!(!handle.db.is_open(), "read only: {read_only}");
        handle.read(b"tkn\0");
        // `store` and `get_all_items` return the refused statuses.
        assert!(handle.write(b"tkn\0", b"gram").is_err());
        assert!(handle.walk().is_err());
        let before = reset_syncs();
        drop(handle);
        assert_eq!(reset_syncs(), before, "read only: {read_only}");

        let SystemAttachment::Unopened(db) =
            SystemAttachment::attach_bigram(&path, read_only).unwrap()
        else {
            panic!("read only: {read_only}: a missing directory opened");
        };
        assert!(!db.is_finished(), "read only: {read_only}");
        db.finish();
        assert!(!path.exists(), "no file was created");
    }
}

#[test]
fn a_never_opened_memory_class_is_usable_and_its_reset_calls_are_inert() {
    // `m_db = new BabyDBM;` / `new TinyDBM;` with no `Open`: records
    // are stored (`chewing_large_table2_tkrzwdb.cpp:104`,
    // `ngram_tkrzwdb.cpp:58`), `Synchronize(false)` returns SUCCESS
    // without file I/O and `Close` answers PRECONDITION_ERROR
    // (`tkrzw_dbm_baby.cc:583-596`, `:394-398`; `tkrzw_dbm_tiny.cc:
    // 455-468`, `:256-260`), both ignored by the pin's `reset()`.
    for class in [Class::Baby, Class::Tiny] {
        let mut db = Db::new(class);
        assert!(!db.is_open(), "{class:?}");
        db.set(b"k", b"v").unwrap();
        let mut value = Vec::new();
        assert!(db.get(b"k", &mut value).unwrap(), "{class:?}");
        assert_eq!(value, b"v");
        assert_eq!(db.count().unwrap(), 1);
        db.synchronize(false).unwrap();
        let close = db.close();
        assert_eq!(close.code, STATUS_PRECONDITION_ERROR, "{class:?}");
        // The record survives the inert close.
        assert!(db.get(b"k", &mut value).unwrap(), "{class:?}");
    }
}

/// A container whose default TinyDBM is built at once, as the pin's
/// `new TinyDBM` is (`ngram_tkrzwdb.cpp:52`): the uncached path the lazy
/// container must answer like.
fn eager() -> TkrzwUserBigramDb {
    TkrzwUserBigramDb {
        db: std::cell::OnceCell::from(Db::new(Class::Tiny)),
    }
}

/// `Bigram::load_db` as the pin runs it (`ngram_tkrzwdb.cpp:54-58`): an
/// eager TinyDBM filled by the `copy_tkrzwdb` walk of every file that
/// opens, with no skip for a file tkrzw counts empty.
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

/// What a caller observes of `db`: each probe's point read, the key
/// walk and the emptiness answer.
type Answers = (Vec<Option<Vec<u8>>>, Vec<Option<[u8; 4]>>, bool);

fn answers(db: &TkrzwUserBigramDb, probes: &[&[u8]]) -> Answers {
    let reads = probes
        .iter()
        .map(|key| {
            // A miss must clear the caller's buffer, so start it dirty.
            let mut value = vec![1, 2, 3];
            let found = db.get(key, &mut value).unwrap();
            assert!(found || value.is_empty(), "a miss clears the buffer");
            found.then_some(value)
        })
        .collect();
    (reads, db.keys().unwrap(), db.is_empty().unwrap())
}

/// The file `db` saves to `path`, with its timestamp (bytes 48..56,
/// the only part that differs between two saves) zeroed.
fn saved(db: &TkrzwUserBigramDb, path: &std::path::Path) -> Vec<u8> {
    db.save_db(path).unwrap();
    let mut bytes = std::fs::read(path).unwrap();
    bytes[48..56].fill(0);
    bytes
}

#[test]
fn known_empty_containers_answer_as_an_eager_tiny_dbm() {
    let dir = TempDir::new("lazy-empty");
    let missing = dir.0.join("missing.db");
    let probes: [&[u8]; 2] = [b"absent", b""];
    let expected: Answers = (vec![None, None], Vec::new(), true);
    for db in [
        TkrzwUserBigramDb::empty().unwrap(),
        TkrzwUserBigramDb::load_db(&missing).unwrap(),
    ] {
        let native = eager();
        assert_eq!(answers(&db, &probes), answers(&native, &probes));
        assert_eq!(answers(&db, &probes), expected);
        db.remove(b"absent").unwrap();
        native.remove(b"absent").unwrap();
        assert_eq!(answers(&db, &probes), answers(&native, &probes));

        let lazy_path = dir.0.join("lazy.db");
        assert_eq!(
            saved(&db, &lazy_path),
            saved(&native, &dir.0.join("native.db")),
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

#[test]
fn a_lazy_container_answers_as_an_eager_tiny_dbm_through_its_first_sets() {
    let probes: [&[u8]; 3] = [b"", b"next", b"absent"];
    let db = TkrzwUserBigramDb::empty().unwrap();
    let native = eager();
    let before = db.keys().unwrap();
    for container in [&db, &native] {
        container.store(b"", b"").unwrap();
    }
    assert_eq!(answers(&db, &probes), answers(&native, &probes));
    assert_eq!(answers(&db, &probes).0[0], Some(Vec::new()));
    assert!(!db.is_empty().unwrap());
    assert!(before.is_empty(), "an earlier key snapshot stays empty");
    for container in [&db, &native] {
        container.remove(b"").unwrap();
    }
    assert_eq!(answers(&db, &probes), answers(&native, &probes));
    assert!(db.is_empty().unwrap());
    for container in [&db, &native] {
        container.store(b"next", b"value").unwrap();
    }
    assert_eq!(answers(&db, &probes), answers(&native, &probes));
    assert_eq!(answers(&db, &probes).0[1], Some(b"value".to_vec()));
}

#[test]
fn loading_answers_as_the_full_copy_walk_for_empty_unhealthy_and_filled_files() {
    let dir = TempDir::new("load-walk");
    let tokens: Vec<[u8; 4]> = (1_u32..=3).map(u32::to_le_bytes).collect();
    let mut probes: Vec<&[u8]> = tokens.iter().map(<[u8; 4]>::as_slice).collect();
    probes.push(b"absent");
    let filled: Vec<_> = tokens.iter().copied().map(Some).collect();

    // A profile the pin saves before learning a bigram: tkrzw opens the
    // file healthy and counts no record.
    let no_rows = dir.0.join("no-rows.db");
    TkrzwUserBigramDb::empty()
        .unwrap()
        .save_db(&no_rows)
        .unwrap();

    // A copy taken while the writer still has the file open: its
    // metadata is the one the writer's open wrote (not closed, no
    // records), while its buckets already reach three records. The
    // pin's `copy_tkrzwdb` walks the buckets and copies them.
    let live = dir.0.join("live.db");
    let unhealthy = dir.0.join("unhealthy.db");
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

    // A file saved with rows.
    let rows = dir.0.join("rows.db");
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

#[test]
fn loading_preserves_the_pins_colliding_bucket_chain_and_values() {
    // PrimaryHash at tkrzw 9ee8416 puts these three little-endian
    // tokens in default bucket 74121. Derived from HashMurmur's
    // four-byte case and PrimaryHash's fold, not from an oracle run.
    // The HashDBM iterator sorts their byte strings as b, a, c;
    // TinyDBM prepends, so its chain (and save walk) must be c, a, b.
    let a = 0x0100_0d46_u32.to_le_bytes();
    let b = 0x0100_4404_u32.to_le_bytes();
    let c = 0x0100_4dd8_u32.to_le_bytes();
    let rows = [
        (a.to_vec(), vec![0, 1, 0, 255]),
        (b.to_vec(), Vec::new()),
        (c.to_vec(), (0_u8..=255).cycle().take(804).collect()),
    ];
    let dir = TempDir::new("collision-order");
    let path = dir.0.join("user_bigram.db");
    let file = open_hash_create(&path).unwrap();
    // Deliberately differs from the source's iterator order.
    for (key, value) in &rows {
        file.db.set(key, value).unwrap();
    }
    drop(file);

    // The load's copy reads the file in its iterator order.
    let file = open_hash(&path).unwrap();
    let mut source_keys = Vec::new();
    copy_tkrzwdb(&file.db, |key, _| {
        source_keys.push(key.to_vec());
        Ok(())
    })
    .unwrap();
    assert_eq!(source_keys, [b.to_vec(), a.to_vec(), c.to_vec()]);
    drop(file);

    // The save's copy walks the loaded container in its chain order.
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
}

#[test]
fn single_record_set_copies_empty_binary_and_replacement_values() {
    let db = TkrzwUserBigramDb::empty().unwrap();
    db.store(b"", b"").unwrap();
    db.store(b"\0k\xff!", b"\0value\xff").unwrap();
    assert_eq!(db.db.get().unwrap().count().unwrap(), 2);
    let mut value = Vec::new();
    assert!(db.get(b"", &mut value).unwrap());
    assert!(value.is_empty());
    assert!(db.get(b"\0k\xff!", &mut value).unwrap());
    assert_eq!(value, b"\0value\xff");

    let before = db.keys().unwrap();
    db.store(b"\0k\xff!", b"").unwrap();
    assert!(db.get(b"\0k\xff!", &mut value).unwrap());
    assert!(value.is_empty());
    assert_eq!(db.db.get().unwrap().count().unwrap(), 2);
    assert_eq!(
        db.keys().unwrap(),
        before,
        "overwrite preserves chain position"
    );
}

#[test]
fn removing_an_absent_record_is_the_librarys_not_found() {
    // `m_db->Remove(key).IsOK()` is false for an absent key
    // (`ngram_tkrzwdb.cpp:140`); the status comes back as `Ok(false)`
    // for the caller's rule, and the trait's `remove` makes it a no-op.
    let db = TkrzwUserBigramDb::empty().unwrap();
    db.remove(b"absent").unwrap();
    db.store(b"here", b"v").unwrap();
    let native = db.db.get().unwrap();
    assert!(!native.remove(b"absent").unwrap());
    assert!(native.remove(b"here").unwrap());
    assert!(!native.remove(b"here").unwrap());
    db.remove(b"absent").unwrap();
    assert_eq!(native.count().unwrap(), 0);
}

#[test]
fn a_deferred_write_and_remove_leave_the_synchronize_to_flush() {
    // `FlexibleBigram::store` and `remove` are a bare `Set` and
    // `Remove` (`flexible_ngram_tkrzwdb.h:236-246`, `:259-263`); the
    // header's record count reaches the file only at the `reset()`
    // `Synchronize(false)` that `flush` makes. `write` commits with
    // that sync, as every other caller expects.
    let dir = TempDir::new("deferred-write");
    let path = dir.0.join("flexible.db");
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

#[test]
fn a_system_tree_synchronizes_before_close_and_a_hash_does_not() {
    // The Tree owners' `reset()` is `Synchronize(false)`, `Close`,
    // `delete`; the bigram's HashDBM is closed with no explicit sync
    // (`ngram_tkrzwdb.cpp:36-45`). The read-only open makes the sync
    // a native `PRECONDITION_ERROR` the pin ignores, so the call is
    // counted, not read back from the file.
    let dir = TempDir::new("system-reset");
    let tree = dir.0.join("pinyin_index.bin");
    let hash = dir.0.join("bigram.db");
    let mut db = Db::new(Class::Tree);
    assert_eq!(
        db.open(&tree, true, OPEN_DEFAULT).unwrap().code,
        STATUS_SUCCESS
    );
    drop(db);
    open_hash_create(&hash).unwrap();

    let before = reset_syncs();
    drop(open_system(&tree, false, true).unwrap().ok().unwrap());
    assert_eq!(reset_syncs(), before + 1);
    drop(open_system(&hash, true, true).unwrap().ok().unwrap());
    assert_eq!(reset_syncs(), before + 1);
    drop(
        open_system(&dir.0.join("missing.bin"), false, true)
            .unwrap()
            .err()
            .unwrap(),
    );
    assert_eq!(reset_syncs(), before + 2);
}
