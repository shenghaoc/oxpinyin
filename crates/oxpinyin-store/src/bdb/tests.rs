use std::ops::{Bound, ControlFlow};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

use super::BdbStore;
use crate::{RawReadStore, StoreError, SystemAttachment, WriteStore, WriteTxn};

// A visitor reads the store it is walking, on both an ordered tree and
// an unordered hash; the raw tier is what the data crate's chewing,
// phrase, bigram, addon and punctuation readers go through.
#[test]
fn raw_readers_keep_rows_valid_during_reentrant_reads() {
    let dir = std::env::temp_dir().join(format!(
        "oxpinyin-bdb-shared-raw-readers-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).unwrap();
    for hash in [false, true] {
        let path = dir.join(if hash { "hash.db" } else { "tree.db" });
        let store = if hash {
            BdbStore::create_hash(&path).unwrap()
        } else {
            BdbStore::create(&path).unwrap()
        };
        store
            .write(|txn| {
                for key in 0_u8..8 {
                    txn.put_raw(&[key], &vec![key; if key == 3 { 9000 } else { 20 }])?;
                }
                Ok(())
            })
            .unwrap();
        drop(store);
        let store = if hash {
            BdbStore::open_hash_read_only(&path).unwrap()
        } else {
            BdbStore::open_read_only(&path).unwrap()
        };
        let mut read = Vec::new();
        for _ in 0..2 {
            assert!(store.get_raw(&[3], &mut read).unwrap());
            assert_eq!(read, [3; 9000]);
            assert!(store.contains_raw(&[3]).unwrap());
            let mut keys = Vec::new();
            store
                .scan_raw(
                    Bound::Included(&[2]),
                    Bound::Excluded(&[6]),
                    |key, value| {
                        assert!(store.get_raw(key, &mut read).unwrap());
                        assert_eq!(read, value);
                        assert!(value.iter().all(|byte| *byte == key[0]));
                        keys.push(key[0]);
                        Ok(ControlFlow::Continue(()))
                    },
                )
                .unwrap();
            assert_eq!(keys, [2, 3, 4, 5]);
        }
        drop(store);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oxpinyin-bdb-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A `user_bigram.db` with one gram, saved by a working container.
fn saved_bigram(dir: &std::path::Path) -> std::path::PathBuf {
    use crate::UserBigramDb as _;
    let path = dir.join("user_bigram.db");
    let db = super::BdbUserBigramDb::load_db(&path).unwrap();
    db.store(&7_u32.to_le_bytes(), &[1, 2, 3, 4]).unwrap();
    db.save_db(&path).unwrap();
    path
}

/// `ngram_bdb.cpp:51-52`: `assert(ret == 0)` after `db_create`. The
/// process dies of SIGABRT, with no Rust panic text or other line.
#[test]
fn a_failed_user_bigram_db_create_aborts_where_load_db_asserts() {
    let stderr = crate::death_test::assert_aborts(
        "bdb::tests::a_failed_user_bigram_db_create_aborts_where_load_db_asserts",
        || {
            // The open dies before any file is read, so no directory.
            let _ = super::BdbUserBigramDb::load_db_with(
                std::path::Path::new("never-read/user_bigram.db"),
                || super::ffi::Db::failed_in_memory_hash_attach(super::ffi::FailedStep::Create),
            );
        },
    );
    assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
}

#[test]
fn a_failed_in_memory_open_leaves_the_pins_failed_container() {
    use crate::UserBigramDb as _;
    let dir = temp_dir("bigram-db-open");
    let path = saved_bigram(&dir);
    // `ngram_bdb.cpp:54-57`: the in-memory `DB->open` fails and
    // `load_db` returns before the file is read; the init carries on.
    let db = super::BdbUserBigramDb::load_db_with(&path, || {
        super::ffi::Db::failed_in_memory_hash_attach(super::ffi::FailedStep::Open)
    })
    .unwrap();
    assert!(db.is_failed());
    let key = 7_u32.to_le_bytes();
    let mut value = vec![9];
    // `load`: a miss, though the file holds the gram (`:140-142`).
    assert!(!db.get(&key, &mut value).unwrap());
    assert!(value.is_empty());
    // `store` and `remove` answer false (`:161-162`, `:174-175`).
    assert!(db.store(&key, &[1, 2, 3, 4]).is_err());
    assert!(db.remove(&key).is_err());
    // `get_all_items`: the emptied array (`:178-191`).
    assert!(db.keys().unwrap().is_empty());
    assert!(db.is_empty().unwrap());

    // `save_db` answers false after it has created the empty hash
    // file (`:84-100`): the caller renames that file into place.
    let tmp = dir.join("user_bigram.db.tmp");
    std::fs::write(&tmp, b"stale").unwrap();
    assert!(db.save_db(&tmp).is_err());
    let saved = super::BdbUserBigramDb::load_db(&tmp).unwrap();
    assert!(!saved.is_failed());
    assert!(saved.keys().unwrap().is_empty());
    // The profile's own file is not touched by the save.
    let kept = super::BdbUserBigramDb::load_db(&path).unwrap();
    assert_eq!(kept.keys().unwrap(), vec![Some(key)]);
    drop((db, saved, kept));
    std::fs::remove_dir_all(dir).unwrap();
}

// A Berkeley DB system attach whose `DB->open` fails.
//
// The pin keeps that handle and calls it again (`chewing_large_table2_bdb.cpp:
// 87-93` at 074a2219); libdb answers each call with `BDB1565` on stderr.
// Berkeley DB requires `DB->close` after a failed open and no other call on
// the handle (`dbopen.html`, `dbclose.html`, 5.3.28), so the store closes it
// and the later read, suggestion and reset reach nothing. libdb writes to the
// process's stderr, so the test runs the attachment in a child process and
// reads that stream.

/// Set in the child: the path of the missing system file.
const CHILD: &str = "OXPINYIN_STORE_BDB_FAILED_ATTACHMENT";

const TEST: &str = "bdb::tests::a_failed_system_attachment_makes_no_call_on_its_handle";

#[test]
fn a_failed_system_attachment_makes_no_call_on_its_handle() {
    if let Some(path) = std::env::var_os(CHILD) {
        let SystemAttachment::Unopened(db) =
            SystemAttachment::open(Path::new(&path), false).expect("attach")
        else {
            panic!("a missing file opened");
        };
        db.read(&[0; 8]);
        db.suggest(&[0; 8]);
        db.finish();
        assert!(db.is_finished());
        return;
    }
    let missing = std::env::temp_dir().join(format!(
        "oxpinyin-store-bdb-failed-attachment-{}.bin",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&missing);
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", TEST, "--test-threads=1"])
        .env(CHILD, &missing)
        .output()
        .expect("child test");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "child failed: {stderr}");
    // A stale TEST name filters out every test, and the child exits 0
    // without the attachment running.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().any(|line| line == "running 1 test"),
        "the child ran no test named {TEST}: {stdout}"
    );
    assert!(
        !stderr.contains("BDB1565"),
        "a call reached the closed handle: {stderr}"
    );
    assert!(!missing.exists(), "the read-only attach created no file");
}

// Berkeley DB paths of the store that no other native test reaches, with
// expectations read from the pin's source at 074a2219 (nothing here was
// produced by running the pin).
//
// The Berkeley DB backend is verified by native tests only
// (`cargo test -p oxpinyin-store --no-default-features --features bdb`).

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
    let dir = tempfile::tempdir().expect("scratch dir");
    let path = dir.path().join("user.db");
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
    let dir = tempfile::tempdir().expect("scratch dir");
    let source = dir.path().join("bigram.db");
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

    let destination = dir.path().join("saved.db");
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
    let dir = tempfile::tempdir().expect("scratch dir");
    let store = BdbStore::create(&dir.path().join("tree.db")).expect("create");
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
    let dir = tempfile::tempdir().expect("scratch dir");
    let path = dir.path().join("bigram.db");
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
    let tree = BdbStore::create(&dir.path().join("tree.db")).expect("create");
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
    let dir = tempfile::tempdir().expect("scratch dir");
    for hash in [false, true] {
        let path = dir.path().join(if hash { "hash.db" } else { "tree.db" });
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
    let dir = tempfile::tempdir().expect("scratch dir");
    let store = BdbStore::create_hash(&dir.path().join("bigram.db")).expect("create_hash");
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
    let dir = tempfile::tempdir().expect("scratch dir");
    let blocked = dir.path().join("blocked");
    std::fs::create_dir(&blocked).expect("directory in the way");
    std::fs::write(blocked.join("keep"), b"x").expect("file inside");

    let bigram = BdbStore::create_hash(&dir.path().join("bigram.db")).expect("attach");
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

mod pin_data {
    //! The Berkeley DB backend against files the real libpinyin wrote.
    //!
    //! Every other test in this crate reads files oxpinyin itself produced,
    //! which proves the backend is self-consistent and nothing about
    //! compatibility. `fixtures/w3/db/k_mixture_model.db` is the pin's own
    //! `import_k_mixture_model` output (`fixtures/w3/README.md`): a `DB_HASH`
    //! file libpinyin's libdb wrote, read here through the seam production uses
    //! for `bigram.db` (`RawReadStore::open_hash_read_only`).
    //! `fixtures/w3/db/`'s `bigram.db`, `phrase_index.bin` and `pinyin_index.bin` were written by
    //! `oxpinyin-utils`; their rows are frozen against the pin's own data sequence (pin capture C1).

    use std::ops::Bound;

    use std::path::Path;

    use crate::{BdbStore, RawReadStore};

    use sha2::{Digest as _, Sha256};

    /// `fixtures/w3/db/`: `oxpinyin-utils` wrote it (`fixtures/w3/README.md`).
    fn w3_db(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/w3/db")
            .join(name)
    }

    /// Every row of FILE, sorted by key, through the given seam.
    fn rows_of(store: &BdbStore) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut rows = Vec::new();
        store
            .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
                rows.push((key.to_vec(), value.to_vec()));
                Ok(())
            })
            .expect("the file walks");
        rows
    }

    /// The frozen utility cases' `rows_digest` (`crates/oxpinyin-utils/tests/it/support.rs`): the
    /// SHA-256 of one `<key hex> <value hex>` line per sorted row, unmasked.
    fn rows_digest(rows: &[(Vec<u8>, Vec<u8>)]) -> String {
        let hex = |bytes: &[u8]| {
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let text: String = rows
            .iter()
            .map(|(key, value)| format!("{} {}\n", hex(key), hex(value)))
            .collect();
        hex(&Sha256::digest(text.as_bytes()))
    }

    /// The pin's own data sequence over the W3 subset, `gen_binary_files --gen-punct-table`,
    /// `import_interpolation` and `gen_unigram` on the bdb cell (`data/Makefile.am:60-62` at
    /// 074a2219): pin capture of 2026-11-07, runs 1 and 2 identical, observation
    /// `utility/bdb/data-sequence`. Each file's rows equal the committed file's, so these are the
    /// pin-written evidence for `fixtures/w3/db/`.
    const PIN_BIGRAM_ROWS: (usize, &str) = (
        65,
        "f83b7af73d9816a24032344f3941eb51cead0d874c0ec6753d308b003595b0f0",
    );

    const PIN_PHRASE_INDEX_ROWS: (usize, &str) = (
        736,
        "795c7fb51c1676472d23d7784b27f1f11530af78813d6beafbda444c6cc358e8",
    );

    const PIN_PINYIN_INDEX_ROWS: (usize, &str) = (
        182,
        "0bd2c7caf8a9dbed1d9040f6c7f1e2aa9399c392d4093dc768f3a5e172d298e1",
    );

    /// The system bigram the pin's data sequence writes (C1): its rows are the pin's, and every row
    /// has libpinyin's shapes. `ngram_bdb.cpp`: the key is one raw `phrase_token_t` (4 bytes,
    /// native-endian: `db_key.data = &index; db_key.size = sizeof(phrase_token_t)`), and the value is
    /// a whole `SingleGram` chunk (`ngram.cpp:31-74`): a 4-byte `total_freq`, then 8-byte
    /// `SingleGramItem`s. A point read agrees with the walk.
    #[test]
    fn the_real_system_bigram_carries_libpinyins_key_and_value_shapes() {
        let store = BdbStore::open_hash_read_only(&w3_db("bigram.db")).expect("a Berkeley DB hash");
        let rows = rows_of(&store);
        assert_eq!((rows.len(), rows_digest(&rows).as_str()), PIN_BIGRAM_ROWS);
        for (key, value) in &rows {
            assert_eq!(key.len(), 4, "every bigram key is one phrase_token_t");
            assert!(
                value.len() >= 4 && (value.len() - 4) % 8 == 0,
                "a SingleGram chunk is total_freq plus whole items: {} bytes",
                value.len()
            );
        }
        let (key, value) = &rows[0];
        let mut read = Vec::new();
        assert!(store.get_raw(key, &mut read).expect("point read"));
        assert_eq!(&read, value, "get_raw must return the bytes the walk saw");
    }

    /// The index trees the pin's data sequence writes (C1): their rows are the pin's, and the walk is
    /// raw-byte order. `phrase_large_table3_bdb.cpp` and `chewing_large_table2_bdb.cpp` open them as
    /// `DB_BTREE` with no `set_bt_compare`, so the order is `memcmp`, and the walk is complete
    /// against `count_raw`.
    #[test]
    fn the_real_index_tree_walks_in_byte_order() {
        for (name, pin) in [
            ("phrase_index.bin", PIN_PHRASE_INDEX_ROWS),
            ("pinyin_index.bin", PIN_PINYIN_INDEX_ROWS),
        ] {
            let store = BdbStore::open_read_only(&w3_db(name))
                .unwrap_or_else(|error| panic!("{name} opens as a DB_BTREE: {error}"));
            let rows = rows_of(&store);
            assert_eq!((rows.len(), rows_digest(&rows).as_str()), pin, "{name}");
            assert!(
                rows.windows(2).all(|pair| pair[0].0 < pair[1].0),
                "{name}: keys strictly ascend in byte order"
            );
            assert_eq!(
                store.count_raw().expect("count"),
                rows.len() as u64,
                "{name}: the walk is complete"
            );
        }
    }

    /// Native-endian `u32` words: the k-mixture records are arrays of them
    /// (`utils/training/k_mixture_model.h:110-154` at libpinyin 074a2219).
    fn words(values: &[u32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect()
    }

    #[test]
    fn a_pin_written_hash_walks_whole_and_ordered() {
        const START: u32 = 1;
        const NIHAO: u32 = 16_802_309;
        const ZHONGGUO: u32 = 16_817_937;
        // `fixtures/w3/kmm/k_mixture_model.text` in the pin's record layout:
        // an eight-byte zero key holding `KMMP` and the magic header `WC N
        // total_freq` (`flexible_ngram_bdb.h:338-356` at 074a2219), then
        // one token key per row holding `WC freq` and each item's `token WC
        // N_n_0 n_1 Mr`, in token order.
        let mut magic = b"KMMP".to_vec();
        magic.extend(words(&[3, 1, 3]));
        let expected: Vec<(Vec<u8>, Vec<u8>)> = vec![
            (vec![0; 8], magic),
            (
                words(&[START]),
                words(&[2, 0, NIHAO, 1, 1, 1, 1, ZHONGGUO, 1, 1, 1, 1]),
            ),
            (words(&[NIHAO]), words(&[0, 2])),
            (words(&[ZHONGGUO]), words(&[1, 1, NIHAO, 1, 1, 1, 1])),
        ];

        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3/db/k_mixture_model.db");
        let store = BdbStore::open_hash_read_only(&path).expect("a Berkeley DB hash");
        let mut rows = Vec::new();
        store
            .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
                rows.push((key.to_vec(), value.to_vec()));
                Ok(())
            })
            .expect("the file walks");
        // Hash containers have no key order of their own; `range_raw`'s contract
        // is ascending key bytes regardless.
        let mut sorted = expected.clone();
        sorted.sort();
        assert_eq!(rows, sorted);
        assert_eq!(store.count_raw().expect("count"), 4);

        // A point read agrees with the walk.
        let mut read = Vec::new();
        assert!(store.get_raw(&sorted[0].0, &mut read).expect("point read"));
        assert_eq!(read, sorted[0].1);
    }
}
