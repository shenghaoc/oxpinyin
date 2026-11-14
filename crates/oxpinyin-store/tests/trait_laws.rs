//! Black-box trait laws over the public `Store` surface that no backend
//! unit test holds: the literal `#` in a store path, the 511/512-byte key
//! boundary, the removal of an absent key, and the raw bytes of a user
//! bigram row that ends in a partial item.
//!
//! Exactly one backend is compiled per build (the crate's
//! `compile_error!` guards), so `DefaultStore` resolves this file's
//! backend the same way every consumer's does, and the
//! `store-backends.yml` matrix runs this file once per peer by running
//! the whole workspace sweep under each backend feature.
//!
//! No backend leaves a file beside the store: tkrzw locks the data file
//! itself with `fcntl` (`tkrzw_file_mmap.cc:124-133` at 1.0.34), Berkeley
//! DB opens without an environment, and Kyoto Cabinet's only companion is
//! its `<file>.wal`. The guard below removes the store's own path on drop.

use oxpinyin_store::{DEFAULT_STORE_EXT, DefaultStore, RawReadStore, WriteStore, WriteTxn};

/// Owns a temporary store path and removes the data file or directory
/// when it drops — including on panic, so a failed law leaves nothing
/// behind in the temp dir.
struct TempPath(std::path::PathBuf);

impl std::ops::Deref for TempPath {
    type Target = std::path::Path;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        cleanup(&self.0);
    }
}

/// A point read as an owned value: `None` on a miss.
fn read(store: &DefaultStore, key: &[u8]) -> Option<Vec<u8>> {
    let mut value = Vec::new();
    store.get_raw(key, &mut value).unwrap().then_some(value)
}

fn cleanup(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir_all(path);
}

fn temp_path(tag: &str) -> TempPath {
    let path = std::env::temp_dir().join(format!(
        "oxpinyin-store-trait-laws-{tag}-{}.{DEFAULT_STORE_EXT}",
        std::process::id()
    ));
    cleanup(&path);
    TempPath(path)
}

/// A literal '#' belongs to either the parent directory or the DBM
/// filename. In particular, `#type=` must never select a different class.
/// This covers both persistent KC classes; the C facade fixes their
/// basenames, so its contract cases cannot rename these DBM files.
#[test]
fn hash_paths_create_and_reopen_tree_and_hash_files() {
    let root = temp_path("hash-path");
    std::fs::create_dir(&*root).unwrap();
    for (directory, filename) in [
        ("parent#type=kch", "store.db"),
        ("parent", "store#type=kch.db"),
    ] {
        let parent = root.join(directory);
        std::fs::create_dir(&parent).unwrap();
        for hash in [false, true] {
            let path = parent.join(format!("{hash}-{filename}"));
            let store = if hash {
                DefaultStore::create_hash(&path).unwrap()
            } else {
                DefaultStore::create(&path).unwrap()
            };
            store.write(|txn| txn.put_raw(b"key", b"value")).unwrap();
            drop(store);
            assert!(path.is_file(), "the literal filename must be created");
            let store = if hash {
                DefaultStore::open_hash_read_only(&path).unwrap()
            } else {
                DefaultStore::open_read_only(&path).unwrap()
            };
            let mut value = Vec::new();
            assert!(store.get_raw(b"key", &mut value).unwrap());
            assert_eq!(value, b"value");
        }
    }
}

/// The 511/512-byte key boundary: every peer accepts both.
#[test]
fn key_length_edges_follow_the_documented_contract() {
    let path = temp_path("key-length");
    let store = DefaultStore::create(&path).unwrap();

    let key_511 = vec![b'k'; 511];
    store
        .write(|txn| txn.put_raw(&key_511, b"edge-511"))
        .expect("a 511-byte key is legal on every peer");
    assert_eq!(read(&store, &key_511), Some(b"edge-511".to_vec()));

    let key_512 = vec![b'k'; 512];
    store
        .write(|txn| txn.put_raw(&key_512, b"edge-512"))
        .expect("a 512-byte key is legal on every peer");
    assert_eq!(read(&store, &key_512), Some(b"edge-512".to_vec()));
}

/// Removing an absent key is a no-op, not an error.
#[test]
fn removing_an_absent_key_succeeds() {
    let path = temp_path("absent-remove");
    let store = DefaultStore::create(&path).unwrap();
    store.write(|txn| txn.put_raw(b"kept", b"1")).unwrap();
    store
        .write(|txn| txn.remove_raw(b"never-there"))
        .expect("removing an absent key must succeed");
    assert_eq!(read(&store, b"kept"), Some(b"1".to_vec()));
}

/// A user bigram row whose items end in a partial record loads as raw
/// bytes: the load decodes nothing, so the row, partial bytes and all, is
/// the value every later read takes apart the pin's way.
#[test]
fn partial_user_bigram_items_keep_defined_reads_and_raw_bytes() {
    let root = temp_path("partial-bigram-items");
    std::fs::create_dir(&*root).unwrap();
    let path = root.join("user_bigram.db");
    let key = 0x0700_0001_u32.to_le_bytes();
    let record = [9_u32.to_le_bytes(), 7_u32.to_le_bytes()].concat();
    for tail_len in 1..8 {
        for complete_items in 0..=1 {
            let mut value = 7_u32.to_le_bytes().to_vec();
            if complete_items == 1 {
                value.extend_from_slice(&record);
            }
            value.extend(std::iter::repeat_n(0xa5, tail_len));
            DefaultStore::write_user_bigram(&path, &[(key.to_vec(), value.clone())])
                .expect("write user bigram");

            let raw = DefaultStore::open_user_bigram(&path).expect("open raw");
            let mut rows = Vec::new();
            raw.copy_raw(|key, value| rows.push((key.to_vec(), value.to_vec())))
                .expect("walk");
            assert_eq!(rows, [(key.to_vec(), value.clone())]);

            assert_eq!(read(&raw, &key), Some(value));
        }
    }
}
