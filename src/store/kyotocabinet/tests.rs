use rstest::rstest;
use std::ops::{Bound, ControlFlow};

use super::{KcStore, KcUserBigramDb};
use crate::store::{RawReadOwned, RawReadStore, UserBigramDb, WriteStore, WriteTxn};

const OREADER: u32 = 1;
const OWRITER: u32 = 2;
const OCREATE: u32 = 4;

// 074a2219 ngram_kyotodb.cpp:43-50 and chewing_large_table2_kyotodb.cpp:65-74 synchronize before close.
#[test]
fn opened_system_attachments_synchronize_before_close() {
    use super::ffi::{NativeCall, take_native_calls};
    use crate::store::SystemAttachment;

    for hash in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("store.db");
        let SystemAttachment::Open(store) = SystemAttachment::attach(&file, hash, false).unwrap()
        else {
            panic!("{hash}: writable attach failed");
        };
        let handle = store.db.address();
        take_native_calls();
        store.set_raw(b"first", b"1").unwrap();
        store.set_raw(b"second", b"2").unwrap();
        drop(store);
        assert_eq!(
            take_native_calls(),
            [
                (handle, NativeCall::Set),
                (handle, NativeCall::Set),
                (handle, NativeCall::Synchronize(false)),
                (handle, NativeCall::Close),
                (handle, NativeCall::Delete),
            ],
            "writable hash: {hash}"
        );

        let SystemAttachment::Open(store) = SystemAttachment::open(&file, hash).unwrap() else {
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

// Register row 157 releases the object after the requested native open fails.
#[test]
fn a_failed_bigram_attach_attempts_the_open_and_releases_the_object() {
    use super::ffi::{NativeCall, OCREATE, OREADER, OWRITER, take_native_calls};
    use crate::store::SystemAttachment;
    use std::os::unix::ffi::OsStrExt as _;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing/bigram.db");
    for read_only in [true, false] {
        take_native_calls();
        assert!(matches!(
            SystemAttachment::attach_bigram(&path, read_only).unwrap(),
            SystemAttachment::Unopened
        ));
        let calls = take_native_calls();
        let handle = calls.first().expect("native open").0;
        let mode = if read_only {
            OREADER
        } else {
            OREADER | OWRITER | OCREATE
        };
        assert_eq!(
            calls,
            [
                (
                    handle,
                    NativeCall::Open(path.as_os_str().as_bytes().to_vec(), mode)
                ),
                (handle, NativeCall::Delete),
            ],
            "read only: {read_only}"
        );
        assert!(!path.exists());
    }
}

// 074a2219 chewing_large_table2_kyotodb.cpp:65-74,100-102 and ngram_kyotodb.cpp:43-60 prescribe both owners' lifecycle.
#[rstest]
#[case::tree(false)]
#[case::stash(true)]
fn in_memory_owners_open_dash_and_synchronize_before_close(#[case] stash: bool) {
    use super::ffi::{NativeCall, take_native_calls};
    take_native_calls();
    let handle = if stash {
        KcUserBigramDb::empty().unwrap().db.address()
    } else {
        KcStore::create_in_memory().unwrap().db.address()
    };
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
        ]
    );
}

// 074a2219 chewing_large_table2_kyotodb.cpp:94-106 retains the ProtoTreeDB populated before load_snapshot fails.
#[test]
fn a_malformed_user_index_snapshot_keeps_its_valid_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("store.db");
    KcStore::write_user_index(&file, |sink| {
        sink(b"first", b"1")?;
        sink(b"second", b"")
    })
    .unwrap();
    let mut bytes = std::fs::read(&file).unwrap();
    assert_eq!(bytes.pop(), Some(0xff), "the snapshot's end mark");
    bytes.push(0x01);
    std::fs::write(&file, bytes).unwrap();

    let store = KcStore::open_user_index(&file).expect("the partial container stays");
    assert_eq!(store.read_raw(b"first").unwrap(), Some(b"1".to_vec()));
    assert_eq!(store.read_raw(b"second").unwrap(), Some(Vec::new()));
}

// 074a2219 flexible_ngram_kyotodb.h:143-155 retries CREATE on the same HashDB; register row 157 releases it.
#[rstest]
#[case::single(false)]
#[case::create_retry(true)]
fn a_create_retry_reopens_the_same_hashdb_and_a_failed_open_releases_it(#[case] retry: bool) {
    use super::ffi::{NativeCall, take_native_calls};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing/model.db");
    let bytes = path.as_os_str().as_encoded_bytes().to_vec();
    take_native_calls();
    let mut store = KcStore::allocate_native(true, false).unwrap();
    let handle = store.db.address();
    assert!(!store.try_open_native(&path, false).unwrap());
    let mut expected = vec![(handle, NativeCall::Open(bytes.clone(), OREADER | OWRITER))];
    if retry {
        assert!(!store.try_open_native(&path, true).unwrap());
        expected.push((handle, NativeCall::Open(bytes, OREADER | OWRITER | OCREATE)));
    }
    drop(store);
    expected.push((handle, NativeCall::Delete));
    assert_eq!(take_native_calls(), expected);
}

// 074a2219 flexible_ngram_kyotodb.h:86-92 synchronizes in reset for reader and writer alike.
#[test]
fn flush_synchronizes_a_read_only_container_natively() {
    use super::ffi::{NativeCall, take_native_calls};

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("store.db");
    drop(KcStore::create_hash(&file).unwrap());
    let store = KcStore::open_hash_read_only(&file).unwrap();
    take_native_calls();
    store.flush().expect("an OREADER HashDB synchronizes");
    let calls = take_native_calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].1, NativeCall::Synchronize(false));
}

// 074a2219 ngram_kyotodb.cpp:163-196 collects keys in the StashDB's native walk order.
#[test]
fn keys_walk_the_stash_in_its_cursor_order() {
    let db = KcUserBigramDb::empty().unwrap();
    for token in 0_u32..16 {
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
    assert_eq!(walked.len(), 16);
    assert!(
        !walked.is_sorted(),
        "the fixture distinguishes native and sorted order"
    );
    assert_eq!(db.keys().unwrap(), walked);
}

// 074a2219 ngram_kyotodb.cpp:152,160 refuses writes to an OREADER database; the Rust guard returns ReadOnly.
#[rstest]
#[case::set(false)]
#[case::remove(true)]
fn a_read_only_container_refuses_set_and_remove_before_any_native_call(#[case] remove: bool) {
    use super::ffi::take_native_calls;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("reader.kch");
    let writer = KcStore::create_hash(&file).unwrap();
    writer.set_raw(b"k", b"v").unwrap();
    drop(writer);
    let reader = KcStore::open_hash_read_only(&file).unwrap();
    take_native_calls();
    let result = if remove {
        reader.remove_raw(b"k").map(drop)
    } else {
        reader.set_raw(b"k", b"w")
    };
    assert!(matches!(result, Err(crate::store::StoreError::ReadOnly)));
    assert!(take_native_calls().is_empty());
    assert_eq!(reader.read_raw(b"k").unwrap(), Some(b"v".to_vec()));
}

// RawReadStore::scan_raw uses an ordered cursor or sorts unordered rows before applying bounds and Break.
#[rstest]
#[case::ordered(false)]
#[case::unordered(true)]
fn scan_raw_is_ascending_and_bounded_on_every_container(#[case] hash: bool) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scan.db");
    let writer = if hash {
        KcStore::create_hash(&path)
    } else {
        KcStore::create(&path)
    }
    .unwrap();
    let keys: &[&[u8]] = &[b"\0", b"\x01", b"\x01A", b"\xff"];
    writer
        .write(|txn| {
            for &key in keys.iter().rev() {
                txn.put_raw(key, key)?;
            }
            Ok(())
        })
        .unwrap();
    drop(writer);
    let reader = if hash {
        KcStore::open_hash_read_only(&path)
    } else {
        KcStore::open_read_only(&path)
    }
    .unwrap();
    assert_eq!(reader.count_raw().unwrap(), keys.len() as u64);
    let mut native = Vec::new();
    reader.native_keys(&mut native).unwrap();
    assert_eq!(native.len(), keys.len());
    for (lo, hi, stop, expected) in [
        (Bound::Unbounded, Bound::Unbounded, false, keys),
        (
            Bound::Excluded(keys[1]),
            Bound::Included(keys[2]),
            false,
            &keys[2..3],
        ),
        (
            Bound::Included(keys[1]),
            Bound::Excluded(keys[3]),
            true,
            &keys[1..2],
        ),
        (
            Bound::Included(keys[2]),
            Bound::Excluded(keys[1]),
            false,
            &keys[..0],
        ),
    ] {
        let mut visited = Vec::new();
        reader
            .scan_raw(lo, hi, |key, value| {
                assert_eq!(key, value);
                visited.push(key.to_vec());
                Ok(if stop {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                })
            })
            .unwrap();
        assert_eq!(visited, expected);
    }
}

// 074a2219 chewing_large_table2_kyotodb.cpp:87-89 and ngram_kyotodb.cpp:117-119 select the class independently of the path.
#[test]
fn hash_paths_create_and_reopen_tree_and_hash_files() {
    let root = tempfile::tempdir().unwrap();
    for (directory, filename) in [
        ("parent#type=kch", "store.db"),
        ("parent", "store#type=kch.db"),
    ] {
        let parent = root.path().join(directory);
        std::fs::create_dir(&parent).unwrap();
        for hash in [false, true] {
            let path = parent.join(format!("{hash}-{filename}"));
            let store = if hash {
                KcStore::create_hash(&path).unwrap()
            } else {
                KcStore::create(&path).unwrap()
            };
            store.write(|txn| txn.put_raw(b"key", b"value")).unwrap();
            drop(store);
            assert!(path.is_file(), "the literal filename must be created");
            let store = if hash {
                KcStore::open_hash_read_only(&path).unwrap()
            } else {
                KcStore::open_read_only(&path).unwrap()
            };
            let mut value = Vec::new();
            assert!(store.get_raw(b"key", &mut value).unwrap());
            assert_eq!(value, b"value");
        }
    }
}
