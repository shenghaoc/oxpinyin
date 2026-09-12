//! The Berkeley DB backend against files the real libpinyin wrote.
//!
//! Every other test in this crate reads files oxpinyin itself produced,
//! which proves the backend is self-consistent and nothing about
//! compatibility. These read a data directory a BerkeleyDB-built
//! libpinyin actually installed — the same system libdb this backend
//! links, through the same seams production uses
//! (`RawReadStore::open_hash_read_only` for `bigram.db`,
//! `ReadStore::open_read_only` for the B-tree index tables).
//!
//! The write gate is the other file: `examples/bdb_write_profile.rs`
//! writes a `bigram.db` through the backend's own `write_user_bigram`
//! seam for `tools/bdb/hash-walk.c`, a C harness with no Rust in it, to
//! check — a drop-in that trains a user's profile writes back into
//! libpinyin's files, and a mismatch there is silent corruption.
//!
//! # Presence
//!
//! These tests need a real installed data directory, so they are
//! `#[ignore]`d: run them with `--include-ignored`, and a missing
//! directory is then a **failure**, never a skip — the same discipline
//! every other real-input test in the tree follows
//! (`docs/testing/README.md`). Point `OXPINYIN_LIBPINYIN_DATA_DIR` at a
//! BerkeleyDB-built libpinyin data directory (the pin's configure
//! default; the oracle cell builds one, and a stock Debian/Ubuntu
//! `libpinyin-data` install provides `/usr/lib/<triplet>/libpinyin/data`).

#![cfg(feature = "bdb")]

use std::ops::Bound;
use std::path::{Path, PathBuf};

use oxpinyin_store::{BdbStore, RawReadStore, ReadStore, StoreError, WriteStore};

/// Where the distro installs libpinyin's runtime data: `$(libdir)`
/// `/libpinyin/data` (`data/Makefile.am`'s `libpinyin_dbdir`) — a
/// multiarch *library* path, not `$datadir`. Anything looking under
/// `share` finds nothing.
fn system_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OXPINYIN_LIBPINYIN_DATA_DIR") {
        return PathBuf::from(dir);
    }
    for dir in [
        "/usr/lib/x86_64-linux-gnu/libpinyin/data",
        "/usr/lib/aarch64-linux-gnu/libpinyin/data",
        "/usr/lib64/libpinyin/data",
        "/usr/lib/libpinyin/data",
    ] {
        if Path::new(dir).join("bigram.db").is_file() {
            return PathBuf::from(dir);
        }
    }
    panic!(
        "missing input: no libpinyin data directory found — set \
         OXPINYIN_LIBPINYIN_DATA_DIR to a BerkeleyDB-built libpinyin's \
         data dir (the oracle cell installs one; a stock Debian/Ubuntu \
         libpinyin-data provides /usr/lib/<triplet>/libpinyin/data)"
    );
}

/// Every row of the system `bigram.db`, sorted by key — the shape
/// `persistence::load_bigram` consumes.
fn bigram_rows(dir: &Path) -> Vec<(Vec<u8>, Vec<u8>)> {
    let store = BdbStore::open_hash_read_only(&dir.join("bigram.db"))
        .expect("the system bigram opens as a Berkeley DB hash");
    let mut rows = Vec::new();
    store
        .range_raw(Bound::Unbounded, Bound::Unbounded, &mut |key, value| {
            rows.push((key.to_vec(), value.to_vec()));
            Ok(())
        })
        .expect("the system bigram walks");
    rows
}

#[test]
#[ignore = "needs a real libpinyin BerkeleyDB data dir (OXPINYIN_LIBPINYIN_DATA_DIR); run with --include-ignored"]
fn the_real_system_bigram_walks_whole_and_ordered() {
    let rows = bigram_rows(&system_data_dir());
    assert!(
        !rows.is_empty(),
        "an installed system bigram is never empty"
    );
    // Hash containers have no key order of their own; `range_raw`'s
    // contract is ascending key bytes regardless, and the walk must be
    // both complete and duplicate-free against `count_raw`'s count.
    let mut sorted = rows.clone();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    sorted.dedup_by(|a, b| a.0 == b.0);
    assert_eq!(
        rows, sorted,
        "range_raw must walk a hash in ascending key order"
    );
    let store = BdbStore::open_hash_read_only(&system_data_dir().join("bigram.db"))
        .expect("reopen for the count");
    assert_eq!(store.count_raw().expect("count"), rows.len() as u64);
}

#[test]
#[ignore = "needs a real libpinyin BerkeleyDB data dir (OXPINYIN_LIBPINYIN_DATA_DIR); run with --include-ignored"]
fn the_real_system_bigram_carries_libpinyins_key_and_value_shapes() {
    let rows = bigram_rows(&system_data_dir());
    // `ngram_bdb.cpp`: the key is one raw `phrase_token_t` (4 bytes,
    // native-endian — `db_key.data = &index; db_key.size =
    // sizeof(phrase_token_t)`), and the value is a whole `SingleGram`
    // chunk (`ngram.cpp:31-74`): a 4-byte `total_freq`, then 8-byte
    // `SingleGramItem`s, the array kept ascending by token.
    for (key, value) in &rows {
        assert_eq!(key.len(), 4, "every bigram key is one phrase_token_t");
        assert!(
            value.len() >= 4 && (value.len() - 4) % 8 == 0,
            "a SingleGram chunk is total_freq plus whole items: {} bytes",
            value.len()
        );
    }
    // A point read agrees with the walk on the same key.
    let dir = system_data_dir();
    let store = BdbStore::open_hash_read_only(&dir.join("bigram.db")).expect("open");
    let (key, value) = &rows[0];
    assert_eq!(
        store.get_raw(key).expect("point read"),
        Some(value.clone()),
        "get_raw must return the bytes the walk saw"
    );
}

#[test]
#[ignore = "needs a real libpinyin BerkeleyDB data dir (OXPINYIN_LIBPINYIN_DATA_DIR); run with --include-ignored"]
fn the_real_index_tree_walks_in_byte_order() {
    let dir = system_data_dir();
    // `phrase_large_table3_bdb.cpp` opens the phrase index as a
    // `DB_BTREE` with no `set_bt_compare`, so the order is raw-byte
    // `memcmp` — and the chewing index keys on packed 16-bit
    // `ChewingKey` structs, so every key is a whole number of them.
    for name in ["phrase_index.bin", "pinyin_index.bin"] {
        let path = dir.join(name);
        if !path.is_file() {
            continue; // a partial install; the bigram tests covered presence
        }
        let store = BdbStore::open_read_only(&path)
            .unwrap_or_else(|error| panic!("{name} opens as a DB_BTREE: {error}"));
        let mut previous: Option<Vec<u8>> = None;
        let mut count = 0_u64;
        store
            .range_raw(Bound::Unbounded, Bound::Unbounded, &mut |key, _value| {
                if let Some(previous) = &previous {
                    assert!(
                        previous.as_slice() < key,
                        "{name}: keys must strictly ascend in byte order"
                    );
                }
                previous = Some(key.to_vec());
                count += 1;
                Ok(())
            })
            .expect("walk");
        assert!(count > 0, "{name} on a real install is populated");
        assert_eq!(
            store.count_raw().expect("count"),
            count,
            "{name}: the walk is complete"
        );
    }
}

#[test]
#[ignore = "needs a real libpinyin BerkeleyDB data dir (OXPINYIN_LIBPINYIN_DATA_DIR); run with --include-ignored"]
fn a_written_user_bigram_reads_back_through_the_user_seam() {
    // The write half, at the store tier: `write_user_bigram`'s default
    // (create_hash + put_raw + compact + rename) is the whole path a
    // Berkeley DB backend needs — unlike Kyoto Cabinet, whose user
    // bigram is a snapshot stream, this backend's user bigram is a
    // genuine DB_HASH file, the same container as the system one.
    let dir = std::env::temp_dir().join(format!("oxpinyin-bdb-user-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmpdir");
    let path = dir.join("user_bigram.db");

    // SingleGram-shaped rows (`ngram.cpp:31-74`): total_freq plus
    // token/freq pairs, ascending by token, keys the raw 4 bytes of the
    // previous token. Tokens crossing 256 in low and high bytes, inserted
    // out of order.
    fn gram(total: u32, items: &[(u32, u32)]) -> Vec<u8> {
        let mut bytes = total.to_le_bytes().to_vec();
        let mut items: Vec<_> = items.to_vec();
        items.sort_by_key(|(token, _)| *token);
        for (token, freq) in items {
            bytes.extend_from_slice(&token.to_le_bytes());
            bytes.extend_from_slice(&freq.to_le_bytes());
        }
        bytes
    }
    let rows: Vec<(Vec<u8>, Vec<u8>)> = [
        (
            0x0000_00ff_u32,
            gram(67, &[(0x0100_05db, 52), (0x0000_00ff, 7), (0x0100_a271, 8)]),
        ),
        (0x0000_0001, gram(15, &[(0x0000_0001, 15)])),
        (
            0x0300_1801,
            gram(96, &[(0x0100_05db, 52), (0x0300_18ff, 44)]),
        ),
    ]
    .into_iter()
    .map(|(prev, value)| (prev.to_le_bytes().to_vec(), value))
    .collect();

    BdbStore::write_user_bigram(&path, &rows).expect("write the user bigram");

    let store = BdbStore::open_user_bigram(&path).expect("the user seam opens it");
    let mut read_back: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    store
        .range_raw(Bound::Unbounded, Bound::Unbounded, &mut |key, value| {
            read_back.push((key.to_vec(), value.to_vec()));
            Ok(())
        })
        .expect("walk");
    let mut expected = rows.clone();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(read_back, expected, "byte-for-byte through both seams");
    assert_eq!(store.count_raw().expect("count"), 3);

    // Bounded walks on the unordered container: a hash cursor walks in
    // bucket order, so `range_raw` must apply the bounds in its sorted
    // collect — rows below the lower bound are omitted (not visited),
    // the excluded-equality edge holds, and the upper bound stops the
    // walk.
    let keys = |lo: Bound<&[u8]>, hi: Bound<&[u8]>| -> Vec<Vec<u8>> {
        let mut keys = Vec::new();
        store
            .range_raw(lo, hi, &mut |key, _value| {
                keys.push(key.to_vec());
                Ok(())
            })
            .expect("bounded walk");
        keys
    };
    assert_eq!(
        keys(Bound::Included(&expected[1].0), Bound::Unbounded),
        vec![expected[1].0.clone(), expected[2].0.clone()],
        "an included lower bound keeps the bound row and drops what is below"
    );
    assert_eq!(
        keys(Bound::Excluded(&expected[1].0), Bound::Unbounded),
        vec![expected[2].0.clone()],
        "an excluded lower bound drops the bound row too"
    );
    assert_eq!(
        keys(Bound::Unbounded, Bound::Excluded(&expected[1].0)),
        vec![expected[0].0.clone()],
        "an excluded upper bound stops before the bound row"
    );

    // The file is a hash database, not a B-tree: a tree open must
    // refuse it rather than guess, exactly as a KC hash open refuses a
    // TreeDB file.
    match BdbStore::open_read_only(&path) {
        Err(StoreError::Backend(_) | StoreError::Io(_)) => {}
        Err(other) => panic!("unexpected error shape: {other}"),
        Ok(_) => panic!("a DB_HASH file must not open as a DB_BTREE"),
    }

    std::fs::remove_dir_all(&dir).expect("cleanup");
}
