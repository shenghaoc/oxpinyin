//! Berkeley DB paths of the store that no other native test reaches, with
//! expectations read from the pin's source at 074a2219 (nothing here was
//! produced by running the pin).
//!
//! The Berkeley DB backend is verified by native tests only
//! (`cargo test -p oxpinyin-store --no-default-features --features bdb`).
#![cfg(feature = "bdb")]

use std::ops::Bound;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use oxpinyin_store::{BdbStore, RawReadStore, StoreError, WriteStore, WriteTxn};

/// A scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-store-bdb-native-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
}

fn raw_keys(store: &BdbStore, lo: Bound<&[u8]>, hi: Bound<&[u8]>) -> Vec<Vec<u8>> {
    let mut keys = Vec::new();
    store
        .range_raw(lo, hi, |key, _| {
            keys.push(key.to_vec());
            Ok(())
        })
        .expect("raw range");
    keys
}

#[test]
fn a_user_index_is_created_as_a_mode_0600_btree() {
    // `ChewingLargeTable2::save_db` / `PhraseLargeTable3::save_db` create the
    // user table with `tmp_db->open(..., DB_BTREE, DB_CREATE, 0600)`:
    // chewing_large_table2_bdb.cpp:148-149, phrase_large_table3_bdb.cpp:
    // 163-164 at 074a2219 (the system tables are 0644, :119/:110 of the
    // attach functions, and `create` keeps that).
    let dir = Scratch::new("create-user");
    let path = dir.file("user.db");
    let store = BdbStore::create_user(&path).expect("create_user");
    store
        .write(|txn| {
            txn.put_raw(b"k1", b"v1")?;
            txn.put_raw(b"k2", b"v2")
        })
        .expect("write");
    drop(store);
    assert_eq!(mode_of(&path), 0o600);

    // The file is a DB_BTREE: it reads back through the tree seam and is
    // refused by the hash seam (`DB->open` with DB_HASH on a btree file).
    let tree = BdbStore::open_read_only(&path).expect("a btree file");
    let mut value = Vec::new();
    assert!(tree.get_raw(b"k2", &mut value).expect("get"));
    assert_eq!(value, b"v2");
    assert!(BdbStore::open_hash_read_only(&path).is_err());
}

#[test]
fn an_attached_bigram_saves_as_a_replaced_mode_0600_hash_copy() {
    // `Bigram::save_db` (ngram_bdb.cpp:81-108 at 074a2219): unlink the
    // destination (:84-86), create a DB_HASH file with `DB_CREATE, 0600`
    // (:94-95), copy every record by cursor (`copy_bdb`, bdb_utils.h:
    // 43-74), sync, close. The source here is an attached `bigram.db`
    // (`Bigram::attach`, :110-125).
    let dir = Scratch::new("save-bigram");
    let source = dir.file("bigram.db");
    let bigram = BdbStore::create_hash(&source).expect("attach");
    let rows: [(&[u8], &[u8]); 3] = [
        (&[1, 0, 0, 0], b"gram-one"),
        (&[2, 0, 0, 0], b"gram-two"),
        (&[0, 1, 0, 0], b""),
    ];
    bigram
        .write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })
        .expect("store");

    let destination = dir.file("saved.db");
    std::fs::write(&destination, b"not a database: save_db unlinks it first").expect("old file");
    bigram.save_bigram_db(&destination).expect("save_db");
    assert_eq!(mode_of(&destination), 0o600);

    let saved = BdbStore::open_hash_read_only(&destination).expect("a DB_HASH file");
    let mut expected: Vec<(Vec<u8>, Vec<u8>)> = rows
        .iter()
        .map(|(key, value)| (key.to_vec(), value.to_vec()))
        .collect();
    expected.sort();
    let mut copied = Vec::new();
    saved
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            copied.push((key.to_vec(), value.to_vec()));
            Ok(())
        })
        .expect("walk");
    assert_eq!(copied, expected, "every record copied, none added");
    assert!(
        BdbStore::open_read_only(&destination).is_err(),
        "the copy is a hash file, not a btree"
    );
    // The source is untouched by the save.
    let mut value = Vec::new();
    assert!(
        bigram
            .get_raw(&[1, 0, 0, 0], &mut value)
            .expect("source get")
    );
    assert_eq!(value, b"gram-one");
}

#[test]
fn a_tree_walk_after_an_excluded_key_starts_at_the_next_record() {
    // `ChewingLargeTable2::search_suggestion` positions on the prefix key
    // with `DB_SET` and then reads "the next entry" with `DB_NEXT`
    // (chewing_large_table2_bdb.cpp:579-595 at 074a2219): the records
    // strictly after the key, ending where `bdb_chewing_continue_search`
    // stops. That is the walk an `Excluded` lower bound asks the seam for
    // on the raw keyspace.
    let dir = Scratch::new("excluded-tree");
    let store = BdbStore::create(&dir.file("tree.db")).expect("create");
    store
        .write(|txn| {
            for key in [b"a", b"b", b"c", b"d"] {
                txn.put_raw(key, b"raw")?;
            }
            Ok(())
        })
        .expect("write");

    let keys = |names: &[&[u8]]| -> Vec<Vec<u8>> { names.iter().map(|k| k.to_vec()).collect() };
    assert_eq!(
        raw_keys(&store, Bound::Excluded(b"b"), Bound::Included(b"d")),
        keys(&[b"c", b"d"]),
    );
    assert_eq!(
        raw_keys(&store, Bound::Excluded(b"b"), Bound::Excluded(b"d")),
        keys(&[b"c"]),
    );
    assert_eq!(
        raw_keys(&store, Bound::Excluded(b"b"), Bound::Included(b"b")),
        Vec::<Vec<u8>>::new(),
    );
    assert_eq!(
        raw_keys(&store, Bound::Excluded(b"b"), Bound::Unbounded),
        keys(&[b"c", b"d"]),
    );
    assert_eq!(
        raw_keys(&store, Bound::Excluded(b"a"), Bound::Excluded(b"c")),
        keys(&[b"b"]),
    );
    assert_eq!(
        raw_keys(&store, Bound::Excluded(b"d"), Bound::Unbounded),
        Vec::<Vec<u8>>::new(),
    );
    assert_eq!(
        raw_keys(&store, Bound::Excluded(b"a"), Bound::Included(b"b")),
        keys(&[b"b"]),
    );
}

#[test]
fn a_hash_walk_is_cut_at_the_bounds_and_counts_every_record() {
    // `Bigram::get_all_items` (ngram_bdb.cpp:178-215 at 074a2219) visits
    // every record of the DB_HASH once, in libdb's own order, and asserts
    // each key is four bytes (:199). A hash walk bounded by the seam is
    // that same record set restricted to the range and sorted by key; the
    // record count is the number of cursor steps before DB_NOTFOUND
    // (:198, :208).
    let dir = Scratch::new("hash-bounds");
    let path = dir.file("bigram.db");
    let store = BdbStore::create_hash(&path).expect("create_hash");
    let tokens: [u32; 5] = [
        0x0000_00ff,
        0x0000_0001,
        0x0100_05db,
        0x0300_1801,
        0x0000_0100,
    ];
    store
        .write(|txn| {
            for token in tokens {
                txn.put_raw(&token.to_le_bytes(), b"gram")?;
            }
            Ok(())
        })
        .expect("write");
    let mut sorted: Vec<Vec<u8>> = tokens.iter().map(|t| t.to_le_bytes().to_vec()).collect();
    sorted.sort();

    assert_eq!(store.count_raw().expect("count"), 5);
    let mut native = store.native_keys().expect("native walk");
    native.sort();
    assert_eq!(native, sorted, "the cursor walk visits each record once");

    assert_eq!(raw_keys(&store, Bound::Unbounded, Bound::Unbounded), sorted);
    assert_eq!(
        raw_keys(
            &store,
            Bound::Excluded(sorted[1].as_slice()),
            Bound::Included(sorted[3].as_slice())
        ),
        sorted[2..=3].to_vec(),
        "an excluded lower key is skipped, an included upper key is kept"
    );
    assert_eq!(
        raw_keys(
            &store,
            Bound::Included(sorted[1].as_slice()),
            Bound::Excluded(sorted[3].as_slice())
        ),
        sorted[1..3].to_vec(),
        "an excluded upper key ends the walk before it"
    );

    // A tree keeps the same count by the same walk.
    let tree = BdbStore::create(&dir.file("tree.db")).expect("create");
    tree.write(|txn| {
        txn.put_raw(b"x", b"1")?;
        txn.put_raw(b"y", b"2")
    })
    .expect("tree write");
    assert_eq!(tree.count_raw().expect("tree count"), 2);
    // Removing a record (`Bigram::remove`, ngram_bdb.cpp:165-176) leaves
    // one fewer cursor step.
    tree.write(|txn| txn.remove_raw(b"x")).expect("remove");
    assert_eq!(tree.count_raw().expect("tree count"), 1);
}

#[test]
fn a_deferred_write_on_a_read_only_handle_is_refused() {
    // `FlexibleBigram::store` and `remove` are a bare `DB->put` / `DB->del`
    // (flexible_ngram_bdb.h:212-213, :233-234 at 074a2219); on a handle
    // opened `DB_RDONLY` (`attach_options`, bdb_utils.h:32-33) libdb refuses
    // the call, `ret != 0`, and the method answers false. The seam's
    // answer is `ReadOnly`, with nothing applied.
    let dir = Scratch::new("deferred-ro");
    for hash in [false, true] {
        let path = dir.file(if hash { "hash.db" } else { "tree.db" });
        let store = if hash {
            BdbStore::create_hash(&path)
        } else {
            BdbStore::create(&path)
        }
        .expect("create");
        store.write(|txn| txn.put_raw(b"k", b"v")).expect("seed");
        drop(store);
        let reader = if hash {
            BdbStore::open_hash_read_only(&path)
        } else {
            BdbStore::open_read_only(&path)
        }
        .expect("read-only open");
        assert!(matches!(
            reader.write_deferred(|txn| txn.put_raw(b"k", b"other")),
            Err(StoreError::ReadOnly)
        ));
        assert!(matches!(
            reader.write_deferred(|txn| txn.remove_raw(b"k")),
            Err(StoreError::ReadOnly)
        ));
        let mut value = Vec::new();
        assert!(reader.get_raw(b"k", &mut value).expect("get"));
        assert_eq!(value, b"v", "hash: {hash}");
    }
}

#[test]
fn a_hash_walk_stops_when_the_visitor_breaks() {
    // `Bigram::get_all_items` visits every record (ngram_bdb.cpp:198-206 at
    // 074a2219); the seam's visitor may end the walk early. The sorted
    // collect then stops at the first `Break`, after exactly that row.
    let dir = Scratch::new("hash-break");
    let store = BdbStore::create_hash(&dir.file("bigram.db")).expect("create_hash");
    store
        .write(|txn| {
            for token in 1_u32..=4 {
                txn.put_raw(&token.to_le_bytes(), b"gram")?;
            }
            Ok(())
        })
        .expect("write");
    let mut seen = Vec::new();
    store
        .scan_raw(Bound::Unbounded, Bound::Unbounded, |key, _| {
            seen.push(key.to_vec());
            Ok(if seen.len() == 2 {
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            })
        })
        .expect("walk");
    let mut sorted: Vec<Vec<u8>> = (1_u32..=4).map(|t| t.to_le_bytes().to_vec()).collect();
    sorted.sort();
    assert_eq!(seen, sorted[..2].to_vec());
}

#[test]
fn a_save_fails_when_the_old_path_cannot_be_unlinked() {
    // Every `save_db` starts with `unlink(dbfile)` and returns false for any
    // failure but `ENOENT`: ngram_bdb.cpp:84-86, chewing_large_table2_bdb.cpp:
    // 138-140, phrase_large_table3_bdb.cpp:153-155 at 074a2219. A directory
    // at the path makes `unlink` fail with `EISDIR`, so nothing is created
    // and the directory is left alone.
    let dir = Scratch::new("unlink-fails");
    let blocked = dir.file("blocked");
    std::fs::create_dir(&blocked).expect("directory in the way");
    std::fs::write(blocked.join("keep"), b"x").expect("file inside");

    let bigram = BdbStore::create_hash(&dir.file("bigram.db")).expect("attach");
    bigram.write(|txn| txn.put_raw(b"k", b"v")).expect("store");
    assert!(matches!(
        bigram.save_bigram_db(&blocked),
        Err(StoreError::Io(_))
    ));
    assert!(matches!(
        BdbStore::write_user_index(&blocked, |sink| sink(b"k", b"v")),
        Err(StoreError::Io(_))
    ));
    assert!(blocked.join("keep").is_file(), "the directory is untouched");
}

#[test]
fn a_walk_from_a_key_longer_than_the_cursor_buffer_finds_the_next_record() {
    // The `DB_SET` + `DB_NEXT` walk of `search_suggestion`
    // (chewing_large_table2_bdb.cpp:579-595 at 074a2219) does not depend on
    // the key's length; the seek key here is longer than the 4096 bytes a
    // fresh cursor reads into, on the raw keyspace.
    let dir = Scratch::new("long-seek");
    let store = BdbStore::create(&dir.file("tree.db")).expect("create");
    let low = vec![1_u8; 5000];
    let high = vec![2_u8; 5000];
    store
        .write(|txn| {
            txn.put_raw(&low, b"low")?;
            txn.put_raw(&high, b"high")
        })
        .expect("write");

    assert_eq!(
        raw_keys(&store, Bound::Included(&low), Bound::Unbounded),
        vec![low.clone(), high.clone()]
    );
    assert_eq!(
        raw_keys(&store, Bound::Excluded(&low), Bound::Unbounded),
        vec![high.clone()]
    );
    assert_eq!(
        raw_keys(&store, Bound::Included(&low), Bound::Excluded(&high)),
        vec![low]
    );
}
