//! Kyoto Cabinet paths of the store that no other native test reaches, with
//! expectations read from the pin's source at 074a2219 (nothing here was
//! produced by running the pin).
//!
//! The Kyoto Cabinet backend is verified by native tests only
//! (`cargo test -p oxpinyin-store --no-default-features --features kyotocabinet`).
#![cfg(feature = "kyotocabinet")]

use std::ops::{Bound, ControlFlow};
use std::path::PathBuf;

use oxpinyin_store::{KcStore, RawReadStore, ReadStore, StoreError, WriteStore, WriteTxn};

/// A scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-store-kc-native-{tag}-{}",
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

#[test]
fn the_record_count_is_every_record_the_iterate_walk_visits() {
    // `Bigram::get_all_items` (ngram_kyotodb.cpp:186-196 at 074a2219) runs
    // one `iterate` that visits every record exactly once
    // (`KeyCollectVisitor::visit_full`, :171-177); `count_raw` is the
    // library's own count of that same record set, on a tree and on a
    // hash, and follows a removal (`Bigram::remove`, :155-161).
    let dir = Scratch::new("count");
    for (name, hash) in [("tree.kct", false), ("hash.kch", true)] {
        let path = dir.file(name);
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
    let dir = Scratch::new("deferred-ro");
    let path = dir.file("flex.kch");
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
fn a_deferred_write_reads_back_its_own_buffered_writes() {
    // The pin's `store` and `remove` act on `m_db` at once
    // (flexible_ngram_kyotodb.h:227-252 at 074a2219), so every later `load`
    // (:192-ff) or `get_all_items` `iterate` (:262-272) sees them: a
    // buffered put shadows the stored record, a buffered removal hides it,
    // and the walk is the stored rows with the buffer laid over them.
    let dir = Scratch::new("deferred-overlay");
    let store = KcStore::create(&dir.file("flex.kct")).expect("create");
    store
        .write(|txn| {
            txn.put("rows", b"a", b"1")?;
            txn.put("rows", b"b", b"2")?;
            txn.put("rows", b"c", b"3")?;
            txn.put("other", b"z", b"9")?;
            txn.put("gone", b"x", b"1")
        })
        .expect("seed");

    let (seen, found) = store
        .write_deferred(|txn| {
            txn.put("rows", b"a", b"A")?;
            txn.put("rows", b"d", b"4")?;
            txn.remove("rows", b"b")?;
            txn.remove("gone", b"x")?;

            let mut value = Vec::new();
            let mut read = |key: &[u8]| -> Result<Option<Vec<u8>>, StoreError> {
                Ok(txn.get("rows", key, &mut value)?.then(|| value.clone()))
            };
            assert_eq!(read(b"a")?, Some(b"A".to_vec()), "buffered put shadows");
            assert_eq!(read(b"b")?, None, "buffered removal hides");
            assert_eq!(read(b"c")?, Some(b"3".to_vec()), "stored row falls through");
            assert_eq!(read(b"d")?, Some(b"4".to_vec()), "buffered insert");

            let walk = |lo: Bound<&[u8]>, hi: Bound<&[u8]>| -> Result<Vec<Vec<u8>>, StoreError> {
                let mut keys = Vec::new();
                txn.scan("rows", lo, hi, |key, _| {
                    keys.push(key.to_vec());
                    Ok(ControlFlow::Continue(()))
                })?;
                Ok(keys)
            };
            let all = walk(Bound::Unbounded, Bound::Unbounded)?;
            let bounded = walk(Bound::Excluded(b"a"), Bound::Included(b"c"))?;
            let mut first = Vec::new();
            txn.scan("rows", Bound::Unbounded, Bound::Unbounded, |key, _| {
                first.push(key.to_vec());
                Ok(ControlFlow::Break(()))
            })?;
            // "other" holds one stored row; "gone" had its only row removed.
            let flags = (
                txn.is_empty("rows")?,
                txn.is_empty("other")?,
                txn.is_empty("gone")?,
                txn.is_empty("never")?,
            );
            Ok((all, bounded, first, flags))
        })
        .expect("deferred write");
    let (all, bounded, first, flags) = seen;
    let keys = |names: &[&[u8]]| -> Vec<Vec<u8>> { names.iter().map(|k| k.to_vec()).collect() };
    assert_eq!(all, keys(&[b"a", b"c", b"d"]));
    assert_eq!(bounded, keys(&[b"c"]));
    assert_eq!(first, keys(&[b"a"]), "a break stops the merged walk");
    assert_eq!(flags, (false, false, true, true));
    assert!(found, "both removals found a record (:251)");

    // After the closure the buffer is applied to the database.
    let mut rows = Vec::new();
    store
        .range("rows", Bound::Unbounded, Bound::Unbounded, |key, value| {
            rows.push((key.to_vec(), value.to_vec()));
            Ok(())
        })
        .expect("stored rows");
    assert_eq!(
        rows,
        vec![
            (b"a".to_vec(), b"A".to_vec()),
            (b"c".to_vec(), b"3".to_vec()),
            (b"d".to_vec(), b"4".to_vec()),
        ]
    );
    assert!(store.is_empty("gone").expect("is_empty"));
}

#[test]
fn an_attached_bigram_saves_as_a_replaced_snapshot() {
    // `Bigram::save_db` (ngram_kyotodb.cpp:82-108 at 074a2219): unlink the
    // destination (:86-88), then `dump_snapshot` of the attached database
    // (:90). `Bigram::load_db` reads such a file back into a `StashDB` with
    // `load_snapshot` (:54-64), which is the user-bigram seam.
    let dir = Scratch::new("save-bigram");
    let bigram = KcStore::create_hash(&dir.file("bigram.kch")).expect("attach");
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

    let destination = dir.file("saved.snapshot");
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
    let dir = Scratch::new("unlink-fails");
    let blocked = dir.file("blocked");
    std::fs::create_dir(&blocked).expect("directory in the way");
    std::fs::write(blocked.join("keep"), b"x").expect("file inside");

    let bigram = KcStore::create_hash(&dir.file("bigram.kch")).expect("attach");
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
