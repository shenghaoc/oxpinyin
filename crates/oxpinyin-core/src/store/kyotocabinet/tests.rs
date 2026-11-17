use std::collections::BTreeMap;
use std::ops::{Bound, ControlFlow};
use std::path::PathBuf;

use super::{KcStore, KcUserBigramDb};
use crate::store::{
    GramReadOwned, RawReadOwned, RawReadStore, StoreError, UserBigramDb, WriteStore, WriteTxn,
};

/// A file under the temp directory, removed on drop.
struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str, ext: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-core-kc-{tag}-{}.{ext}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The pin's `attach_options` modes (`kyotodb_utils.h:32-45`).
const OREADER: u32 = 1;
const OWRITER: u32 = 2;
const OCREATE: u32 = 4;

#[test]
fn retained_failed_system_attachments_synchronize_before_close_once() {
    use super::ffi::{NativeCall, take_native_calls};
    use crate::store::SystemAttachment;

    // Audit each pin class, rather than inferring one from another:
    // chewing_large_table2_kyotodb.cpp:65-73 (main/addon pinyin),
    // phrase_large_table3_kyotodb.cpp:76-86 (main/addon phrase),
    // ngram_kyotodb.cpp:43-48 (HashDB), punct_table_kyotodb.cpp:37-47.
    for (table, hash) in [
        ("system-pinyin", false),
        ("system-phrase", false),
        ("system-bigram", true),
        ("addon-pinyin", false),
        ("addon-phrase", false),
        ("punctuation", false),
    ] {
        let missing = Temp::new(table, "missing");
        let SystemAttachment::Unopened(db) = SystemAttachment::open(&missing.0, hash).unwrap()
        else {
            panic!("missing {table} unexpectedly opened");
        };
        let opened = take_native_calls();
        let &[(handle, NativeCall::Open(_, OREADER))] = opened.as_slice() else {
            panic!("{table}: one read-only open, got {opened:?}");
        };
        db.finish();
        assert_eq!(
            take_native_calls(),
            [
                (handle, NativeCall::Synchronize(false)),
                (handle, NativeCall::Close),
                (handle, NativeCall::Delete),
            ],
            "{table}: native calls on the same retained object"
        );
        db.finish();
        drop(db);
        assert!(take_native_calls().is_empty(), "{table}");
    }
}

#[test]
fn opened_system_attachments_synchronize_before_close() {
    use super::ffi::{NativeCall, take_native_calls};
    use crate::store::SystemAttachment;

    // The successful attach has the same owner `reset()` as the failed
    // one: synchronize, close, delete (ngram_kyotodb.cpp:43-50,
    // chewing_large_table2_kyotodb.cpp:65-74).
    for hash in [false, true] {
        let file = Temp::new(&format!("opened-system-{hash}"), "db");
        if hash {
            drop(KcStore::create_hash(&file.0).unwrap());
        } else {
            drop(KcStore::create(&file.0).unwrap());
        }
        let SystemAttachment::Open(store) = SystemAttachment::open(&file.0, hash).unwrap() else {
            panic!("{hash}: an existing file did not open");
        };
        take_native_calls();
        let handle = store.db.address();
        drop(store);
        assert_eq!(
            take_native_calls(),
            [
                (handle, NativeCall::Synchronize(false)),
                (handle, NativeCall::Close),
                (handle, NativeCall::Delete),
            ],
            "hash: {hash}"
        );
    }
}

#[test]
fn a_failed_bigram_attach_keeps_its_hashdb_until_reset() {
    use super::ffi::{NativeCall, OCREATE, OREADER, OWRITER, take_native_calls};
    use crate::store::SystemAttachment;
    use std::os::unix::ffi::OsStrExt as _;

    // `Bigram::attach` makes one `open` on a new `HashDB`, with no
    // CREATE retry and no signature check, and keeps the object when
    // the open fails (ngram_kyotodb.cpp:110-120). `load`, `store` and
    // `get_all_items` then reach it: `check`, `set`, and an `iterate`
    // with no `count` before it (:124-153, :186-196). `reset()`
    // synchronizes, closes and deletes it (:43-50).
    let dir = Temp::new("failed-bigram", "missing");
    let path = dir.0.join("bigram.db");
    for read_only in [true, false] {
        take_native_calls();
        let SystemAttachment::Unopened(db) =
            SystemAttachment::attach_bigram(&path, read_only).unwrap()
        else {
            panic!("read only: {read_only}: a missing directory opened");
        };
        let opened = take_native_calls();
        let handle = opened.first().expect("native open").0;
        let mode = if read_only {
            OREADER
        } else {
            OREADER | OWRITER | OCREATE
        };
        assert_eq!(
            opened,
            [(
                handle,
                NativeCall::Open(path.as_os_str().as_bytes().to_vec(), mode)
            )],
            "read only: {read_only}: the object outlives the attach"
        );
        db.read(b"tkn\0");
        // The utilities write only through the read-write attach. The
        // refused `set` is `store`'s false; the refused `iterate` is
        // ignored and `get_all_items` answers true.
        if !read_only {
            assert!(db.write(b"tkn\0", b"gram").is_err());
        }
        db.walk().expect("get_all_items answers true");
        db.finish();
        let mut expected = vec![(handle, NativeCall::Check)];
        if !read_only {
            expected.push((handle, NativeCall::Set));
        }
        expected.extend([
            (handle, NativeCall::Iterate),
            (handle, NativeCall::Synchronize(false)),
            (handle, NativeCall::Close),
            (handle, NativeCall::Delete),
        ]);
        assert_eq!(take_native_calls(), expected, "read only: {read_only}");
        drop(db);
        assert!(take_native_calls().is_empty(), "read only: {read_only}");
        assert!(!path.exists(), "no file was created");
    }
}

#[test]
fn in_memory_owners_open_dash_and_synchronize_before_close() {
    use super::ffi::{NativeCall, take_native_calls};

    // The pin opens both in-memory containers as "-" with
    // OREADER|OWRITER|OCREATE (chewing_large_table2_kyotodb.cpp:100-102,
    // ngram_kyotodb.cpp:58-60), and its `reset()` synchronizes before it
    // closes (chewing_large_table2_kyotodb.cpp:65-74,
    // ngram_kyotodb.cpp:43-50).
    let snapshot = Temp::new("in-memory-owner", "bin");
    KcStore::write_user_index(&snapshot.0, |sink| sink(b"k", b"v")).unwrap();
    take_native_calls();
    // Each owner is made, dropped, and its object's address returned.
    type Owner = fn(&std::path::Path) -> usize;
    let owners: [(&str, Owner); 4] = [
        ("session store", |_| {
            KcStore::create_in_memory().unwrap().db.address()
        }),
        ("user index", |path| {
            KcStore::open_user_index(path).unwrap().db.address()
        }),
        ("user bigram", |_| {
            KcUserBigramDb::empty().unwrap().db.address()
        }),
        ("user bigram load", |path| {
            KcUserBigramDb::load_db(path).unwrap().db.address()
        }),
    ];
    for (owner, make) in owners {
        let handle = make(&snapshot.0);
        assert_eq!(
            take_native_calls(),
            [
                (
                    handle,
                    NativeCall::Open(b"-".to_vec(), OREADER | OWRITER | OCREATE)
                ),
                (handle, NativeCall::Synchronize(false)),
                (handle, NativeCall::Close),
                (handle, NativeCall::Delete),
            ],
            "{owner}"
        );
    }
}

#[test]
fn a_malformed_user_index_snapshot_keeps_its_valid_prefix() {
    // `BasicDB::load_snapshot` sets each record as it reads it and fails
    // at the bad tag after them (`kcdb.h:2339-2418`); the pin keeps that
    // ProtoTreeDB (`chewing_large_table2_kyotodb.cpp:94-106`).
    let file = Temp::new("partial-snapshot", "bin");
    KcStore::write_user_index(&file.0, |sink| {
        sink(b"first", b"1")?;
        sink(b"second", b"")
    })
    .unwrap();
    let mut bytes = std::fs::read(&file.0).unwrap();
    assert_eq!(bytes.pop(), Some(0xff), "the snapshot's end mark");
    bytes.push(0x01);
    std::fs::write(&file.0, bytes).unwrap();

    let store = KcStore::open_user_index(&file.0).expect("the partial container stays");
    assert_eq!(store.read_raw(b"first").unwrap(), Some(b"1".to_vec()));
    assert_eq!(store.read_raw(b"second").unwrap(), Some(Vec::new()));
}

#[test]
fn a_create_retry_reopens_the_same_hashdb_and_a_failed_open_deletes_without_close() {
    use super::ffi::{NativeCall as Call, take_native_calls};
    use crate::store::{AttachMode, HashAttach};

    // `flexible_ngram_kyotodb.h:143-155`: one `new HashDB`, then
    // `open(mode)` and, with ATTACH_CREATE, `open(mode | OCREATE)` on
    // the same object; without CREATE, `delete` and no `close`.
    let dir = Temp::new("retry-dir", "missing");
    let path = dir.0.join("model.db");
    let bytes = path.as_os_str().as_encoded_bytes().to_vec();
    take_native_calls();
    let HashAttach::Unopened(store) =
        KcStore::attach_hash(&path, AttachMode::ReadWriteCreate, (b"k", b"v")).unwrap()
    else {
        panic!("a missing directory opened");
    };
    let calls = take_native_calls();
    let handle = calls.first().expect("native open").0;
    assert_eq!(
        calls,
        [
            (handle, Call::Open(bytes.clone(), OREADER | OWRITER)),
            (
                handle,
                Call::Open(bytes.clone(), OREADER | OWRITER | OCREATE)
            ),
        ]
    );
    drop(store);

    take_native_calls();
    let HashAttach::Closed =
        KcStore::attach_hash(&path, AttachMode::ReadWrite, (b"k", b"v")).unwrap()
    else {
        panic!("a failed open without CREATE keeps no object");
    };
    let calls = take_native_calls();
    let handle = calls.first().expect("native open").0;
    assert_eq!(
        calls,
        [
            (handle, Call::Open(bytes, OREADER | OWRITER)),
            (handle, Call::Delete)
        ]
    );
}

#[test]
fn flush_synchronizes_a_read_only_container_natively() {
    use super::ffi::{NativeCall, take_native_calls};

    // `FlexibleBigram::reset()` synchronizes whatever the attach mode
    // (`flexible_ngram_kyotodb.h:86-92`); an OREADER HashDB answers true
    // (`kchashdb.h:969-983`).
    let file = Temp::new("readonly-flush", "kch");
    drop(KcStore::create_hash(&file.0).unwrap());
    let store = KcStore::open_hash_read_only(&file.0).unwrap();
    take_native_calls();
    store.flush().expect("an OREADER HashDB synchronizes");
    let calls = take_native_calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].1, NativeCall::Synchronize(false));
}

#[test]
fn an_empty_value_is_stored_on_every_container() {
    // An empty slice crosses the bridge as address 1, Kyoto Cabinet's
    // `Visitor::REMOVE` (kcdb.h:46); stored as such, `set` would delete
    // the record instead of emptying it (kcdb.h:1424-1445). `Vec::new()`
    // is the slice with no allocation.
    let empty: Vec<u8> = Vec::new();
    let check = |store: &KcStore, container: &str| {
        store
            .write(|txn| {
                txn.put_raw(b"present", b"value")?;
                txn.put_raw(b"present", &empty)?;
                txn.put_raw(b"fresh", &empty)
            })
            .unwrap();
        for key in [&b"present"[..], b"fresh"] {
            assert_eq!(
                store.read_raw(key).unwrap(),
                Some(Vec::new()),
                "{container}"
            );
            assert!(store.contains_raw(key).unwrap(), "{container}");
        }
    };
    let tree = Temp::new("empty-tree", "kct");
    check(&KcStore::create(&tree.0).unwrap(), "TreeDB");
    let hash = Temp::new("empty-hash", "kch");
    check(&KcStore::create_hash(&hash.0).unwrap(), "HashDB");
    check(&KcStore::create_in_memory().unwrap(), "ProtoTreeDB");
    let stash = KcUserBigramDb::empty().unwrap();
    stash.store(b"present", b"value").unwrap();
    stash.store(b"present", &empty).unwrap();
    assert_eq!(
        stash.read_gram(b"present").unwrap(),
        Some(Vec::new()),
        "StashDB"
    );
}

#[test]
fn keys_walk_the_stash_in_its_cursor_order() {
    // `keys()` is one `iterate`, the pin's `get_all_items` call; the
    // stash's cursor visits its buckets in the same order.
    let db = KcUserBigramDb::empty().unwrap();
    for token in 0_u32..3000 {
        db.store(&token.wrapping_mul(2_654_435_761).to_le_bytes(), &[1, 2, 3])
            .unwrap();
    }
    let mut cursor = db.db.cursor();
    let mut walked = Vec::new();
    if cursor.jump_first().unwrap() {
        while cursor.next_key().unwrap() {
            walked.push(<[u8; 4]>::try_from(cursor.key()).ok());
        }
    }
    assert_eq!(walked.len(), 3000);
    assert_eq!(db.keys().unwrap(), walked);
}

/// Keys that share prefixes, cross 0x00/0xFF and include the empty
/// key's neighbours, half with empty values.
fn rows() -> BTreeMap<Vec<u8>, Vec<u8>> {
    let mut rows = BTreeMap::new();
    for a in [0x00_u8, 0x01, 0x7F, 0x80, 0xFE, 0xFF] {
        rows.insert(vec![a], vec![]);
        for b in [0x00_u8, 0x41, 0xFF] {
            rows.insert(vec![a, b], vec![a, b, 1]);
            rows.insert(vec![a, b, b], if b == 0x41 { vec![] } else { vec![b; 40] });
        }
    }
    rows
}

/// An owned `[lo, hi]` pair.
type Range = (Bound<Vec<u8>>, Bound<Vec<u8>>);

fn bounds() -> Vec<Range> {
    let points: [&[u8]; 6] = [
        &[],
        &[0x00],
        &[0x01, 0x41],
        &[0x7F, 0xFF, 0xFF],
        &[0x80],
        &[0xFF, 0xFF],
    ];
    let mut out = vec![(Bound::Unbounded, Bound::Unbounded)];
    for lo in points {
        for hi in points {
            for (l, h) in [
                (Bound::Included(lo.to_vec()), Bound::Excluded(hi.to_vec())),
                (Bound::Excluded(lo.to_vec()), Bound::Included(hi.to_vec())),
                (Bound::Included(lo.to_vec()), Bound::Unbounded),
                (Bound::Unbounded, Bound::Excluded(hi.to_vec())),
            ] {
                out.push((l, h));
            }
        }
    }
    out
}

fn as_ref(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
    bound.as_ref().map(Vec::as_slice)
}

/// `scan_raw` over `store` agrees with the model for every bound pair,
/// and a visitor `Break` stops after exactly that row.
fn assert_scans_match(store: &KcStore, model: &BTreeMap<Vec<u8>, Vec<u8>>) {
    for (lo, hi) in bounds() {
        let lower = as_ref(&lo);
        let upper = as_ref(&hi);
        let expected: Vec<(Vec<u8>, Vec<u8>)> = model
            .iter()
            .filter(|(key, _)| crate::store::common::in_bounds(key, lower, upper))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let mut got = Vec::new();
        store
            .scan_raw(lower, upper, |key, value| {
                got.push((key.to_vec(), value.to_vec()));
                Ok(ControlFlow::Continue(()))
            })
            .unwrap();
        assert_eq!(got, expected, "bounds {lo:?} .. {hi:?}");
        let mut first = Vec::new();
        store
            .scan_raw(lower, upper, |key, _| {
                first.push(key.to_vec());
                Ok(ControlFlow::Break(()))
            })
            .unwrap();
        assert_eq!(
            first,
            expected
                .iter()
                .take(1)
                .map(|row| row.0.clone())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn a_failed_suggestion_makes_only_the_check_call() {
    use super::ffi::{NativeCall, take_native_calls};
    use crate::store::SystemAttachment;

    // `ChewingLargeTable2::search_suggestion` on a tree whose open
    // failed: its first call is `m_db->check(akbuf, aksiz)` and the
    // `-1` answer returns before any cursor is made
    // (chewing_large_table2_kyotodb.cpp:539-544 at 074a2219), so no
    // `jump`, `step` or `get` follows. After the owner's `reset()` the
    // object is gone and the call reaches nothing.
    let dir = Temp::new("failed-suggestion", "missing");
    let path = dir.0.join("pinyin_index.bin");
    take_native_calls();
    let SystemAttachment::Unopened(db) = SystemAttachment::open(&path, false).unwrap() else {
        panic!("a path in a missing directory opened");
    };
    let handle = take_native_calls().first().expect("native open").0;
    db.suggest(&[1, 0, 2, 0]);
    assert_eq!(take_native_calls(), [(handle, NativeCall::Check)]);
    db.finish();
    take_native_calls();
    db.suggest(&[1, 0, 2, 0]);
    db.read(&[1, 0, 2, 0]);
    assert!(take_native_calls().is_empty(), "a finished object is inert");
}

#[test]
fn a_read_only_container_refuses_set_and_remove_before_any_native_call() {
    use super::ffi::take_native_calls;
    use crate::store::StoreError;

    // A `Bigram`/`FlexibleBigram` attached `ATTACH_READONLY` is a KC
    // `OREADER` database, so its `set` and `remove` answer false
    // (ngram_kyotodb.cpp:152, :160; flexible_ngram_kyotodb.h:235, :251
    // at 074a2219). The wrapper's answer is `ReadOnly`, and the
    // refusal makes no native call.
    let file = Temp::new("readonly-set", "kch");
    let store = KcStore::create_hash(&file.0).unwrap();
    store.write(|txn| txn.put_raw(b"k", b"v")).unwrap();
    drop(store);
    let store = KcStore::open_hash_read_only(&file.0).unwrap();
    take_native_calls();
    assert!(matches!(
        store.db.set(b"k", b"w"),
        Err(StoreError::ReadOnly)
    ));
    assert!(matches!(store.db.remove(b"k"), Err(StoreError::ReadOnly)));
    assert!(take_native_calls().is_empty());
    assert_eq!(store.read_raw(b"k").unwrap(), Some(b"v".to_vec()));
}

#[test]
fn scan_raw_is_ascending_and_bounded_on_every_container() {
    let model = rows();
    let fill = |store: &KcStore| {
        store
            .write(|txn| {
                for (key, value) in &model {
                    txn.put_raw(key, value)?;
                }
                Ok(())
            })
            .unwrap();
    };
    let tree = Temp::new("scan-tree", "kct");
    let store = KcStore::create(&tree.0).unwrap();
    fill(&store);
    drop(store);
    assert_scans_match(&KcStore::open_read_only(&tree.0).unwrap(), &model);

    let hash = Temp::new("scan-hash", "kch");
    let store = KcStore::create_hash(&hash.0).unwrap();
    fill(&store);
    drop(store);
    assert_scans_match(&KcStore::open_hash_read_only(&hash.0).unwrap(), &model);

    let store = KcStore::create_in_memory().unwrap();
    fill(&store);
    assert_scans_match(&store, &model);

    let rows: Vec<(Vec<u8>, Vec<u8>)> = model.clone().into_iter().collect();
    let index = Temp::new("scan-index", "bin");
    KcStore::write_user_index(&index.0, |sink| {
        rows.iter().try_for_each(|(key, value)| sink(key, value))
    })
    .unwrap();
    assert_scans_match(&KcStore::open_user_index(&index.0).unwrap(), &model);

    let bigram = Temp::new("scan-bigram", "db");
    KcStore::write_user_bigram(&bigram.0, &rows).unwrap();
    assert_scans_match(&KcStore::open_user_bigram(&bigram.0).unwrap(), &model);
}

// Kyoto Cabinet paths of the store that no other native test reaches, with
// expectations read from the pin's source at 074a2219 (nothing here was
// produced by running the pin).
//
// The Kyoto Cabinet backend is verified by native tests only
// (`cargo test -p oxpinyin-core --no-default-features --features kyotocabinet`).

#[test]
fn the_record_count_is_every_record_the_iterate_walk_visits() {
    // `Bigram::get_all_items` (ngram_kyotodb.cpp:186-196 at 074a2219) runs
    // one `iterate` that visits every record exactly once
    // (`KeyCollectVisitor::visit_full`, :171-177); `count_raw` is the
    // library's own count of that same record set, on a tree and on a
    // hash, and follows a removal (`Bigram::remove`, :155-161).
    let dir = tempfile::tempdir().expect("scratch dir");
    for (name, hash) in [("tree.kct", false), ("hash.kch", true)] {
        let path = dir.path().join(name);
        let store = if hash {
            KcStore::create_hash(&path)
        } else {
            KcStore::create(&path)
        }
        .expect("create");
        store
            .write(|txn| {
                for token in [3_u32, 1, 2] {
                    txn.put_raw(&token.to_le_bytes(), b"gram")?;
                }
                Ok(())
            })
            .expect("write");
        assert_eq!(store.count_raw().expect("count"), 3, "{name}");
        assert_eq!(store.native_keys().expect("walk").len(), 3, "{name}");
        store
            .write(|txn| txn.remove_raw(&1_u32.to_le_bytes()))
            .expect("remove");
        assert_eq!(store.count_raw().expect("count"), 2, "{name}");
        assert_eq!(store.native_keys().expect("walk").len(), 2, "{name}");
    }
}

#[test]
fn a_deferred_write_on_a_read_only_handle_is_refused() {
    // `FlexibleBigram::store` and `remove` are a bare `set` / `remove`
    // (flexible_ngram_kyotodb.h:235, :251 at 074a2219); on an `OREADER`
    // database Kyoto Cabinet refuses both, the method answers false, and
    // the seam answers `ReadOnly` with nothing applied.
    let dir = tempfile::tempdir().expect("scratch dir");
    let path = dir.path().join("flex.kch");
    let store = KcStore::create_hash(&path).expect("create");
    store.write(|txn| txn.put_raw(b"k", b"v")).expect("seed");
    drop(store);
    let reader = KcStore::open_hash_read_only(&path).expect("read-only open");
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
    assert_eq!(value, b"v");
}

#[test]
fn an_attached_bigram_saves_as_a_replaced_snapshot() {
    // `Bigram::save_db` (ngram_kyotodb.cpp:82-108 at 074a2219): unlink the
    // destination (:86-88), then `dump_snapshot` of the attached database
    // (:90). `Bigram::load_db` reads such a file back into a `StashDB` with
    // `load_snapshot` (:54-64), which is the user-bigram seam.
    let dir = tempfile::tempdir().expect("scratch dir");
    let bigram = KcStore::create_hash(&dir.path().join("bigram.kch")).expect("attach");
    let rows: [(&[u8], &[u8]); 3] = [
        (&[1, 0, 0, 0], b"gram-one"),
        (&[2, 0, 0, 0], b"gram-two"),
        (&[0, 1, 0, 0], b"gram-three"),
    ];
    bigram
        .write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })
        .expect("store");

    let destination = dir.path().join("saved.snapshot");
    std::fs::write(&destination, b"stale contents that save_db unlinks").expect("old file");
    bigram.save_bigram_db(&destination).expect("save_db");

    let loaded = KcStore::open_user_bigram(&destination).expect("load_snapshot");
    let mut copied = Vec::new();
    loaded
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            copied.push((key.to_vec(), value.to_vec()));
            Ok(())
        })
        .expect("walk");
    let mut expected: Vec<(Vec<u8>, Vec<u8>)> = rows
        .iter()
        .map(|(key, value)| (key.to_vec(), value.to_vec()))
        .collect();
    expected.sort();
    assert_eq!(copied, expected, "the snapshot holds every record, once");
    assert_eq!(loaded.count_raw().expect("count"), 3);
}

#[test]
fn a_save_fails_when_the_old_path_cannot_be_unlinked() {
    // Every `save_db` starts with `unlink(dbfile)` and returns false for any
    // failure but `ENOENT`: ngram_kyotodb.cpp:86-88,
    // chewing_large_table2_kyotodb.cpp:125-127, phrase_large_table3_kyotodb.
    // cpp:140-142 at 074a2219. A directory at the path makes `unlink` fail
    // with `EISDIR`, so no snapshot is dumped and the directory is left
    // alone.
    let dir = tempfile::tempdir().expect("scratch dir");
    let blocked = dir.path().join("blocked");
    std::fs::create_dir(&blocked).expect("directory in the way");
    std::fs::write(blocked.join("keep"), b"x").expect("file inside");

    let bigram = KcStore::create_hash(&dir.path().join("bigram.kch")).expect("attach");
    bigram.write(|txn| txn.put_raw(b"k", b"v")).expect("store");
    assert!(matches!(
        bigram.save_bigram_db(&blocked),
        Err(StoreError::Io(_))
    ));
    assert!(matches!(
        KcStore::write_user_index(&blocked, |sink| sink(b"k", b"v")),
        Err(StoreError::Io(_))
    ));
    assert!(blocked.join("keep").is_file(), "the directory is untouched");
}
