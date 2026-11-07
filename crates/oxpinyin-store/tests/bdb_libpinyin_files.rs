//! The Berkeley DB backend against a file the real libpinyin wrote, and its
//! user-bigram seam.
//!
//! Every other test in this crate reads files oxpinyin itself produced,
//! which proves the backend is self-consistent and nothing about
//! compatibility. `fixtures/w3/db/k_mixture_model.db` is the pin's own
//! `import_k_mixture_model` output (`fixtures/w3/README.md`): a `DB_HASH`
//! file libpinyin's libdb wrote, read here through the seam production uses
//! for `bigram.db` (`RawReadStore::open_hash_read_only`).
//!
//! The write gate is the other file: `examples/bdb_write_profile.rs`
//! writes a `bigram.db` through the backend's own `write_user_bigram`
//! seam for `tools/bdb/hash-walk.c`, a C harness with no Rust in it, to
//! check — a drop-in that trains a user's profile writes back into
//! libpinyin's files, and a mismatch there is silent corruption.

#![cfg(feature = "bdb")]

use std::ops::Bound;
use std::path::Path;

use oxpinyin_store::{BdbStore, RawReadStore, ReadStore, StoreError, WriteStore};

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

#[test]
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
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
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
            .range_raw(lo, hi, |key, _value| {
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
