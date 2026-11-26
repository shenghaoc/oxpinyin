use std::ops::{Bound, ControlFlow};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

use super::BdbStore;
use crate::store::{RawReadStore, SystemAttachment, WriteStore};

// BdbStore's ordered cursor keeps its row valid across a point read on the same handle.
#[test]
fn raw_readers_keep_rows_valid_during_reentrant_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.db");
    let writer = BdbStore::create(&path).unwrap();
    writer.set_raw(b"a", b"short").unwrap();
    writer.set_raw(b"b", &[3; 9000]).unwrap();
    drop(writer);
    let db = BdbStore::open_read_only(&path).unwrap();
    let mut seen = Vec::new();
    let mut read = Vec::new();
    db.range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
        assert!(db.get_raw(key, &mut read)?);
        assert_eq!(read, value);
        seen.push(key.to_vec());
        Ok(())
    })
    .unwrap();
    assert_eq!(seen, [b"a", b"b"]);
}

fn saved_bigram(dir: &std::path::Path) -> std::path::PathBuf {
    use crate::store::UserBigramDb as _;
    let path = dir.join("user_bigram.db");
    let db = super::BdbUserBigramDb::load_db(&path).unwrap();
    db.store(&7_u32.to_le_bytes(), &[1, 2, 3, 4]).unwrap();
    db.save_db(&path).unwrap();
    path
}

// 074a2219 ngram_bdb.cpp:51-52 asserts on failed db_create before opening the unnamed hash.
#[test]
fn a_failed_user_bigram_db_create_aborts_where_load_db_asserts() {
    let stderr = crate::store::death_test::assert_aborts(
        "store::bdb::tests::a_failed_user_bigram_db_create_aborts_where_load_db_asserts",
        || {
            let _ = super::BdbUserBigramDb::load_db_with(
                std::path::Path::new("never-read/user_bigram.db"),
                || super::ffi::Db::failed_in_memory_hash_attach(super::ffi::FailedStep::Create),
            );
        },
    );
    assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
}

thread_local! {
    pub(super) static FAIL_MASK_CURSOR_READ: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// 074a2219 phrase_large_table3_bdb.cpp:406 and chewing_large_table2_bdb.cpp:538 assert on cursor read errors.
#[test]
fn mask_all_aborts_on_a_failed_cursor_read() {
    let stderr = crate::store::death_test::assert_aborts_after_setup(
        "store::bdb::tests::mask_all_aborts_on_a_failed_cursor_read",
        || BdbStore::create_in_memory().unwrap(),
        |store| {
            FAIL_MASK_CURSOR_READ.set(true);
            store.mask_all();
        },
    );
    assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
}

// 074a2219 phrase_large_table3_bdb.cpp:400 and chewing_large_table2_bdb.cpp:507 assert on DB_CURRENT write failure.
#[test]
fn mask_all_aborts_on_a_failed_cursor_update() {
    crate::store::death_test::assert_aborts_in_tempdir(
        "store::bdb::tests::mask_all_aborts_on_a_failed_cursor_update",
        |dir| {
            let store = BdbStore::create(&dir.join("table.db")).unwrap();
            store.set_raw(b"first", b"old value").unwrap();
        },
        |dir| {
            BdbStore::open_read_only(&dir.join("table.db"))
                .unwrap()
                .mask_all()
        },
        |dir| {
            let store = BdbStore::open_read_only(&dir.join("table.db")).unwrap();
            let mut value = Vec::new();
            assert!(store.get_raw(b"first", &mut value).unwrap());
            assert_eq!(value, b"old value");
        },
    );
}

// Register row 113 preserves failed-container reads, writes and the empty file left by save.
#[test]
fn a_failed_in_memory_open_leaves_the_pins_failed_container() {
    use crate::store::UserBigramDb as _;
    let dir = tempfile::tempdir().unwrap();
    let path = saved_bigram(dir.path());
    let db = super::BdbUserBigramDb::load_db_with(&path, || {
        super::ffi::Db::failed_in_memory_hash_attach(super::ffi::FailedStep::Open)
    })
    .unwrap();
    assert!(db.is_failed());
    let key = 7_u32.to_le_bytes();
    let mut value = vec![9];
    assert!(!db.get(&key, &mut value).unwrap());
    assert!(value.is_empty());
    assert!(db.store(&key, &[1, 2, 3, 4]).is_err());
    assert!(db.remove(&key).is_err());
    assert!(db.keys().unwrap().is_empty());
    assert!(db.is_empty().unwrap());

    let tmp = dir.path().join("user_bigram.db.tmp");
    std::fs::write(&tmp, b"stale").unwrap();
    assert!(db.save_db(&tmp).is_err());
    let saved = super::BdbUserBigramDb::load_db(&tmp).unwrap();
    assert!(!saved.is_failed());
    assert!(saved.keys().unwrap().is_empty());
    let kept = super::BdbUserBigramDb::load_db(&path).unwrap();
    assert_eq!(kept.keys().unwrap(), vec![Some(key)]);
}

const CHILD: &str = "OXPINYIN_STORE_BDB_FAILED_ATTACHMENT";

const TEST: &str = "store::bdb::tests::a_failed_system_attachment_makes_no_call_on_its_handle";

// Register row 113 closes the failed native handle and suppresses subsequent BDB1565 diagnostics.
#[test]
fn a_failed_system_attachment_makes_no_call_on_its_handle() {
    if let Some(path) = std::env::var_os(CHILD) {
        assert!(matches!(
            SystemAttachment::open(Path::new(&path), crate::store::DbmKind::Tree).expect("attach"),
            SystemAttachment::Unopened
        ));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.bin");
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", TEST, "--test-threads=1"])
        .env(CHILD, &missing)
        .output()
        .expect("child test");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "child failed: {stderr}");
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

// 074a2219 chewing_large_table2_bdb.cpp:148-149 creates a user index as DB_BTREE with mode 0600.
#[test]
fn a_user_index_is_created_as_a_mode_0600_btree() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let path = dir.path().join("user.db");
    BdbStore::write_user_index(&path, |sink| {
        sink(b"k1", b"v1")?;
        sink(b"k2", b"v2")
    })
    .expect("write_user_index");
    assert_eq!(mode_of(&path), 0o600);

    let tree = BdbStore::open_read_only(&path).expect("a btree file");
    let mut value = Vec::new();
    assert!(tree.get_raw(b"k2", &mut value).expect("get"));
    assert_eq!(value, b"v2");
    assert!(BdbStore::open_hash_read_only(&path).is_err());
}

// 074a2219 ngram_bdb.cpp:84-104 unlinks, creates a mode-0600 hash, and copies the records.
#[test]
fn an_attached_bigram_saves_as_a_replaced_mode_0600_hash_copy() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let source = dir.path().join("bigram.db");
    let bigram = BdbStore::create_hash(&source).expect("attach");
    let rows: [(&[u8], &[u8]); 3] = [
        (&[1, 0, 0, 0], b"gram-one"),
        (&[2, 0, 0, 0], b"gram-two"),
        (&[0, 1, 0, 0], b""),
    ];
    for (key, value) in rows {
        bigram.set_raw(key, value).expect("store");
    }

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
    let mut value = Vec::new();
    assert!(
        bigram
            .get_raw(&[1, 0, 0, 0], &mut value)
            .expect("source get")
    );
    assert_eq!(value, b"gram-one");
}

// 074a2219 chewing_large_table2_bdb.cpp:579-595 advances past an exact prefix; RawReadStore specifies both bounds.
#[test]
fn a_tree_walk_after_an_excluded_key_starts_at_the_next_record() {
    use Bound::{Excluded, Included, Unbounded};
    let dir = tempfile::tempdir().unwrap();
    let db = BdbStore::create(&dir.path().join("tree.db")).unwrap();
    for key in [b"b", b"c", b"d"] {
        db.set_raw(key, b"v").unwrap();
    }
    for (lo, hi, expected) in [
        (
            Excluded(b"b".as_slice()),
            Included(b"d".as_slice()),
            b"cd".as_slice(),
        ),
        (Excluded(b"b"), Excluded(b"d"), b"c"),
        (Excluded(b"b"), Included(b"b"), b""),
        (Included(b"a"), Unbounded, b"bcd"),
        (Included(b"e"), Unbounded, b""),
    ] {
        let keys = raw_keys(&db, lo, hi);
        assert_eq!(keys, expected.iter().map(|&k| vec![k]).collect::<Vec<_>>());
    }
}

// RawReadStore sorts and bounds DB_HASH scans; 074a2219 ngram_bdb.cpp:178-212 walks each native key once.
#[test]
fn a_hash_walk_is_cut_at_the_bounds_and_counts_every_record() {
    let dir = tempfile::tempdir().unwrap();
    let db = BdbStore::create_hash(&dir.path().join("bigram.db")).unwrap();
    let mut keys = [0xff_u32, 1, 0x100, 0x0100_05db].map(u32::to_le_bytes);
    for key in &keys {
        db.set_raw(key, b"gram").unwrap();
    }
    keys.sort();
    let mut native = Vec::new();
    db.native_keys(&mut native).unwrap();
    native.sort();
    assert_eq!(native, keys);
    assert_eq!(db.count_raw().unwrap(), keys.len() as u64);
    assert_eq!(
        raw_keys(&db, Bound::Excluded(&keys[0]), Bound::Included(&keys[2])),
        keys[1..=2]
    );
    let mut prefix = Vec::new();
    db.scan_raw(Bound::Unbounded, Bound::Unbounded, |key, _| {
        prefix.push(key.to_vec());
        Ok(if prefix.len() == 2 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        })
    })
    .unwrap();
    assert_eq!(prefix, keys[..2]);
}

// 074a2219 ngram_bdb.cpp:94-100 and bdb_utils.h:49-52 create the destination before a soft cursor failure.
#[test]
fn a_bigram_save_stops_when_its_source_cursor_does_not_open() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let source = BdbStore::create_hash(&dir.path().join("source.db")).expect("source");
    source.set_raw(b"k", b"v").expect("record");
    let destination = dir.path().join("destination.db");
    crate::store::fail_iterator_creation_after(0);
    let result = source.save_bigram_db(&destination);
    crate::store::fail_iterator_creation_after(usize::MAX);
    assert!(result.is_err(), "the pin returns false");
    assert!(
        destination.is_file(),
        "the pin creates the destination first"
    );
}

mod pin_data {
    use std::ops::Bound;

    use std::path::Path;

    use crate::store::{BdbStore, RawReadStore};

    use sha2::{Digest as _, Sha256};

    fn w3_db(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/w3/db")
            .join(name)
    }

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

    // Frozen utility/bdb/data-sequence pins the hash rows; ngram.cpp:31-74 specifies their SingleGram layout.
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

    // Frozen utility/bdb/data-sequence pins both tree digests and the store contract requires byte order.
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

    fn words(values: &[u32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect()
    }

    // Frozen fixtures/w3/db/k_mixture_model.db and its KMM text specify the native hash records.
    #[test]
    fn a_pin_written_hash_walks_whole_and_ordered() {
        const START: u32 = 1;
        const NIHAO: u32 = 16_802_309;
        const ZHONGGUO: u32 = 16_817_937;
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

        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/w3/db/k_mixture_model.db");
        let store = BdbStore::open_hash_read_only(&path).expect("a Berkeley DB hash");
        let mut rows = Vec::new();
        store
            .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
                rows.push((key.to_vec(), value.to_vec()));
                Ok(())
            })
            .expect("the file walks");
        let mut sorted = expected.clone();
        sorted.sort();
        assert_eq!(rows, sorted);
        assert_eq!(store.count_raw().expect("count"), 4);

        let mut read = Vec::new();
        assert!(store.get_raw(&sorted[0].0, &mut read).expect("point read"));
        assert_eq!(read, sorted[0].1);
    }
}
