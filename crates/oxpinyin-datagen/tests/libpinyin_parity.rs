//! Drop-in parity differential: this crate's output vs a real pin-built
//! libpinyin data directory, on the same backend.
//!
//! Compiled for the Kyoto Cabinet and Tkrzw producers only — the two
//! backends whose files libpinyin itself defines. Gated on
//! `OXPINYIN_LIBPINYIN_DATA_DIR`: a libpinyin install's `data/` built
//! with the same DBM as this crate's selected backend (`pinyin_index.bin`,
//! `phrase_index.bin`, `punct.bin`, `bigram.db`, `addon_pinyin_index.bin`,
//! `addon_phrase_index.bin`, and the sixteen per-library `*.bin` chunk
//! files), plus a model directory resolved the way every model20 consumer
//! resolves it (`PINYIN_MODEL_DIR`, then the cache). Local runs without
//! the variable skip.
//!
//! `tools/datagen/libpinyin-drop-in-differential.sh` drives this test in
//! the perf-matrix container against the pinned model20 and against the
//! toned mini model under `fixtures/datagen-toned/`, whose libpinyin side
//! is produced by libpinyin's own `gen_binary_files` /
//! `import_interpolation` / `gen_unigram` from the same tables — the
//! canonical-source invariant: both implementations compile the text,
//! neither reads the other's output.
//!
//! The chunk comparison is byte-exact; the DBM comparisons are row-stream
//! exact (same `(key, value)` pairs in ascending key order), except that
//! the pinyin index tolerates the pin's uninitialized struct padding
//! (`docs/findings/datagen-compat-2026-09-01.md`). The container bytes of
//! a DBM are the writing library's own layout, so a byte comparison there
//! would test Kyoto Cabinet or Tkrzw, not this compiler.
#![cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]

use std::path::{Path, PathBuf};

use oxpinyin_datagen::{addon, punct, system};
use oxpinyin_store::{RawReadStore, ReadStore};

/// The store type that reads the selected backend's own files. Exactly
/// one backend feature is compiled in per build (`oxpinyin-store`
/// refuses two), so the two aliases never coexist.
#[cfg(feature = "kyotocabinet")]
type DropInStore = oxpinyin_store::KcStore;
#[cfg(feature = "tkrzw")]
type DropInStore = oxpinyin_store::TkrzwStore;

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

/// Every row of a tree container, in the container's ascending key order.
fn rows_of(path: &Path) -> Vec<(Vec<u8>, Vec<u8>)> {
    let store = DropInStore::open_read_only(path).expect("open tree");
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

fn assert_rows_equal(name: &str, generated: &[(Vec<u8>, Vec<u8>)], real: &Path) {
    let actual = rows_of(real);
    let mut expected: Vec<(Vec<u8>, Vec<u8>)> = generated.to_vec();
    // A DBM's keyspace is order-independent; compare both in ascending
    // key-byte order, which is also both tree containers' physical order.
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
/// exactly — token and every key with its tone bits; only padding bytes
/// are ignored.
fn assert_rows_equal_ignoring_padding(name: &str, generated: &[(Vec<u8>, Vec<u8>)], real: &Path) {
    let actual = rows_of(real);
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

/// Per-key point comparison for the hash container (`bigram.db`): a KC
/// HashDB cursor cannot be positioned from the empty key (unordered
/// container), so every generated key is looked up in the real file and
/// its value compared. Real rows the generator did not emit would be
/// missed by this direction alone; the row-count line printed here and
/// the writer's own read-back verification cover the counts.
fn assert_hash_equal(name: &str, generated: &[(Vec<u8>, Vec<u8>)], real: &Path) {
    let store = DropInStore::open_hash_read_only(real).expect("open hash");
    for (index, (key, value)) in generated.iter().enumerate() {
        let got = store.get_raw(key).expect("get_raw");
        assert_eq!(
            got.as_deref(),
            Some(value.as_slice()),
            "{name}: row {index}: key {key:02x?} value mismatch"
        );
    }
    eprintln!("{name}: {} rows verified by point read", generated.len());
}

#[test]
fn drop_in_output_matches_the_pin_built_data_dir() {
    let Some(data) = data_dir() else {
        eprintln!("OXPINYIN_LIBPINYIN_DATA_DIR unset — skipping");
        return;
    };
    let Some(model) = model_dir() else {
        eprintln!("no model20 cache — skipping");
        return;
    };

    let (sys, _) = system::compile(&model, system::Subset::Full).expect("system");
    let add = addon::compile(&model, addon::Subset::Full).expect("addon");
    let punct_rows = punct::compile(&model).expect("punct");

    // ---- chunk files: byte-exact ---------------------------------------
    // Tones, unigram (+1), pronunciation frequencies and item layout are
    // all inside these bytes.
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
        eprintln!("{name}: {} bytes byte-exact", bytes.len());
    }

    // ---- DBM row streams ------------------------------------------------
    assert_rows_equal_ignoring_padding(
        "pinyin_index.bin",
        &sys.pinyin_index,
        &data.join("pinyin_index.bin"),
    );
    assert_rows_equal(
        "phrase_index.bin",
        &sys.phrase_index,
        &data.join("phrase_index.bin"),
    );
    assert_hash_equal("bigram.db", &sys.bigram, &data.join("bigram.db"));
    assert_rows_equal("punct.bin", &punct_rows, &data.join("punct.bin"));
    assert_rows_equal_ignoring_padding(
        "addon_pinyin_index.bin",
        &add.pinyin_index,
        &data.join("addon_pinyin_index.bin"),
    );
    assert_rows_equal(
        "addon_phrase_index.bin",
        &add.phrase_index,
        &data.join("addon_phrase_index.bin"),
    );
    eprintln!(
        "pinyin_index.bin {} rows · phrase_index.bin {} rows · bigram.db {} rows · punct.bin {} rows · addon {} / {} rows",
        sys.pinyin_index.len(),
        sys.phrase_index.len(),
        sys.bigram.len(),
        punct_rows.len(),
        add.pinyin_index.len(),
        add.phrase_index.len()
    );
}
