//! Drop-in parity differential: KC output vs a real pin-built libpinyin
//! data directory.
//!
//! Gated on `OXPINYIN_LIBPINYIN_DATA_DIR` (a directory holding a
//! KC-built libpinyin install's `data/`: `pinyin_index.bin`,
//! `phrase_index.bin`, `punct.bin`, `bigram.db`,
//! `addon_pinyin_index.bin`, `addon_phrase_index.bin`, and the sixteen
//! per-library `*.bin` chunk files) plus the model20 cache. The
//! perf-matrix container provides both; local runs skip.
//!
//! The chunk comparison is byte-exact; the DBM comparisons are
//! row-stream exact (same `(key, value)` pairs in ascending key order).
//! The container bytes of a DBM are the writing KC's own layout, so a
//! byte comparison would be a test of Kyoto Cabinet, not of this
//! compiler.
#![cfg(feature = "kyotocabinet")]

use std::path::PathBuf;

use oxpinyin_datagen::{addon, punct, system};
use oxpinyin_store::{KcStore, RawReadStore, ReadStore};

fn data_dir() -> Option<PathBuf> {
    std::env::var_os("OXPINYIN_LIBPINYIN_DATA_DIR").map(PathBuf::from)
}

fn model_dir() -> Option<PathBuf> {
    match pinyin_oracle::model_cache::locate_model_dir() {
        Ok(Some(dir)) => Some(dir),
        Ok(None) => None,
        Err(e) => panic!("model dir set but unusable: {e:?}"),
    }
}

fn rows_of(path: &PathBuf, hash: bool) -> Vec<(Vec<u8>, Vec<u8>)> {
    let store = if hash {
        KcStore::open_hash_read_only(path).expect("open hash")
    } else {
        KcStore::open_read_only(path).expect("open tree")
    };
    let mut rows = Vec::new();
    store
        .range_raw(
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            &mut |key, value| {
                rows.push((key.to_vec(), value.to_vec()));
                Ok(())
            },
        )
        .expect("scan");
    rows
}

fn assert_rows_equal(name: &str, generated: &[(Vec<u8>, Vec<u8>)], real: &PathBuf, hash: bool) {
    let actual = rows_of(real, hash);
    let mut expected: Vec<(Vec<u8>, Vec<u8>)> = generated.to_vec();
    // The writer may emit rows in a different order than the container's
    // physical order (numeric token vs LE-byte); a DBM's keyspace is
    // order-independent, so compare both in ascending key-byte order.
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        expected.len(),
        actual.len(),
        "{name}: row count {} vs real {}",
        expected.len(),
        actual.len()
    );
    for (index, (got, want)) in expected.iter().zip(actual.iter()).enumerate() {
        assert_eq!(
            got, want,
            "{name}: row {index}: key {:02x?} mismatch",
            got.0
        );
    }
}

/// Row-stream comparison tolerant of struct tail-padding: the pinyin
/// index values are `PinyinIndexItem2<L>` arrays whose 4-alignment tail
/// padding is uninitialized stack memory upstream
/// (`ChewingTableEntry::add_index` inserts the raw struct) —
/// unreproducible garbage the real files carry and this compiler
/// deterministically zeroes (`docs/findings/datagen-compat-2026-09-01.md`,
/// the padding divergence). Every field inside the stride is compared
/// exactly; only padding bytes are ignored.
fn assert_rows_equal_ignoring_padding(
    name: &str,
    generated: &[(Vec<u8>, Vec<u8>)],
    real: &PathBuf,
) {
    let actual = rows_of(real, false);
    assert_eq!(
        generated.len(),
        actual.len(),
        "{name}: row count {} vs real {}",
        generated.len(),
        actual.len()
    );
    for (index, (got, want)) in generated.iter().zip(actual.iter()).enumerate() {
        assert_eq!(got.0, want.0, "{name}: row {index}: key mismatch");
        assert_eq!(
            got.1.len(),
            want.1.len(),
            "{name}: row {index}: value length {} vs {}",
            got.1.len(),
            want.1.len()
        );
        let syllables = got.0.len() / 2;
        let stride = (4 + 2 * syllables + 3) & !3;
        assert_eq!(got.1.len() % stride, 0, "{name}: row {index}: ragged");
        for record in 0..got.1.len() / stride {
            let s = record * stride;
            let fields = 4 + 2 * syllables;
            assert_eq!(
                &got.1[s..s + fields],
                &want.1[s..s + fields],
                "{name}: row {index} record {record}: token/keys mismatch"
            );
        }
    }
}

/// Per-key point comparison for a hash DB (`bigram.db`): KC HashDB
/// cursors cannot iterate the whole keyspace (`kccurjumpkey` with the
/// empty key answers no-record on an unordered container), so every
/// generated key is looked up in the real file and its value compared,
/// plus a count check guards against extra real rows.
fn assert_hash_equal(name: &str, generated: &[(Vec<u8>, Vec<u8>)], real: &PathBuf) {
    let store = KcStore::open_hash_read_only(real).expect("open hash");
    for (index, (key, value)) in generated.iter().enumerate() {
        let got = store.get_raw(key).expect("get_raw");
        assert_eq!(
            got.as_deref(),
            Some(value.as_slice()),
            "{name}: row {index}: key {key:02x?} value mismatch"
        );
    }
}

#[test]
fn kc_output_matches_the_pin_built_data_dir() {
    let Some(data) = data_dir() else {
        eprintln!("OXPINYIN_LIBPINYIN_DATA_DIR unset — skipping");
        return;
    };
    let Some(model) = model_dir() else {
        eprintln!("no model20 cache — skipping");
        return;
    };

    let sys = system::compile_libpinyin(&model, system::Subset::Full).expect("system");
    let add = addon::compile_libpinyin(&model, addon::Subset::Full).expect("addon");
    let punct_rows = punct::compile_libpinyin(&model).expect("punct");

    // ---- chunk files: byte-exact ---------------------------------------
    for (name, bytes) in sys.chunks.iter().chain(add.chunks.iter()) {
        let real = data.join(name);
        assert!(real.is_file(), "{name} missing from the data dir");
        let actual = std::fs::read(&real).expect("read real chunk");
        assert_eq!(
            *bytes,
            actual,
            "{name}: generated {} bytes vs real {}",
            bytes.len(),
            actual.len()
        );
    }

    // ---- DBM row streams ------------------------------------------------
    // The pinyin index carries struct tail-padding that is uninitialized
    // stack memory upstream; compare its fields, not the padding bytes.
    assert_rows_equal_ignoring_padding(
        "pinyin_index.bin",
        &sys.pinyin_index,
        &data.join("pinyin_index.bin"),
    );
    assert_rows_equal(
        "phrase_index.bin",
        &sys.phrase_index,
        &data.join("phrase_index.bin"),
        false,
    );
    assert_hash_equal("bigram.db", &sys.bigram, &data.join("bigram.db"));
    assert_rows_equal("punct.bin", &punct_rows, &data.join("punct.bin"), false);
    assert_rows_equal_ignoring_padding(
        "addon_pinyin_index.bin",
        &add.pinyin_index,
        &data.join("addon_pinyin_index.bin"),
    );
    assert_rows_equal(
        "addon_phrase_index.bin",
        &add.phrase_index,
        &data.join("addon_phrase_index.bin"),
        false,
    );
}
