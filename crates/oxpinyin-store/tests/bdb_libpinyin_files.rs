//! The Berkeley DB backend against a file the real libpinyin wrote, and its
//! user-bigram seam.
//!
//! Every other test in this crate reads files oxpinyin itself produced,
//! which proves the backend is self-consistent and nothing about
//! compatibility. `fixtures/w3/db/k_mixture_model.db` is the pin's own
//! `import_k_mixture_model` output (`fixtures/w3/README.md`): a `DB_HASH`
//! file libpinyin's libdb wrote, read here through the seam production uses
//! for `bigram.db` (`RawReadStore::open_hash_read_only`).
//! `fixtures/w3/db/`'s `bigram.db`, `phrase_index.bin` and `pinyin_index.bin` were written by
//! `oxpinyin-utils`; their rows are frozen against the pin's own data sequence (pin capture C1).
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

/// The frozen utility cases' `rows_digest` (`crates/oxpinyin-utils/tests/common/mod.rs`): the
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
