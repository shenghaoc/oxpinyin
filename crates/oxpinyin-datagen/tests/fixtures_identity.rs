//! Mini compile must reproduce the committed `fixtures/w3/` tables.
//!
//! The fixtures were produced by the retired `oxpinyin-migrate` exporters
//! from the same pinned model20 (checksummed by
//! `fixtures/w3/fixtures.sha256`). Reproducing them from the native
//! compiler proves the model20 derivation without the oracle.
//!
//! Comparison is at the key/value level: the frozen files were written by
//! redb 4.1.0 and the lockfile has since moved to newer redb releases
//! whose container layout differs, so raw file bytes are only
//! reproducible under the writing redb version. The committed files
//! themselves are guarded by `fixtures/w3/fixtures.sha256`, which stays
//! untouched.
//!
//! **Gated on the redb peer feature.** Under exactly-one-backend, this
//! test only compiles when the redb peer is selected
//! (`--no-default-features --features redb`) — the frozen `.redb`
//! fixtures the test reads back can only be opened by the redb reader,
//! and that reader is only compiled when the peer is enabled. The other
//! three peers have their own frozen fixture sets (`fixtures/w3/*.kct`,
//! `*.lmdb`, `*.tkt`) which cross_backend.rs exercises through the
//! compiled peer.
//!
//! Requires the model cache (`tools/model/fetch-model.sh`); set
//! `OXPINYIN_DATAGEN_STRICT=1` to fail instead of skip when it is absent
//! (CI does).
#![cfg(feature = "redb")]

use std::path::PathBuf;

use oxpinyin_datagen::write::Backend;
use oxpinyin_datagen::{addon, punct, system};

fn strict() -> bool {
    std::env::var_os("OXPINYIN_DATAGEN_STRICT").is_some()
}

fn model_dir() -> Option<PathBuf> {
    match pinyin_oracle::model_cache::locate_model_dir() {
        Ok(Some(dir)) => Some(dir),
        Ok(None) => None,
        Err(e) => panic!("model dir set but unusable: {e:?}"),
    }
}

fn fixtures_w3() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oxpinyin-datagen-fixtures-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn assert_same_rows(name: &str, produced: &std::path::Path, frozen: &std::path::Path) {
    let produced = Backend::Redb.read_all(produced).unwrap();
    let frozen = Backend::Redb.read_all(frozen).unwrap();
    assert_eq!(
        produced.len(),
        frozen.len(),
        "{name}: row count differs (produced {} vs frozen {})",
        produced.len(),
        frozen.len()
    );
    for (index, (a, b)) in produced.iter().zip(frozen.iter()).enumerate() {
        assert_eq!(a, b, "{name}: row {index} differs");
    }
    eprintln!("{name}: all {} rows identical to fixtures/w3", frozen.len());
}

#[test]
fn mini_compile_reproduces_frozen_w3_tables() {
    let Some(model) = model_dir() else {
        if strict() {
            panic!("OXPINYIN_DATAGEN_STRICT=1 but no model20 cache is present");
        }
        eprintln!("skipping: model20 cache absent (run tools/model/fetch-model.sh)");
        return;
    };
    let out = temp_dir("mini");
    let fixtures = fixtures_w3();

    let (tables, stats) = system::compile(&model, system::Subset::MiniFixture).unwrap();
    eprintln!("mini stats: {stats:?}");

    for (base, entries) in [
        ("pinyin_index", &tables.pinyin_index),
        ("phrase_index", &tables.phrase_index),
        ("bigram", &tables.bigram),
    ] {
        let path = Backend::Redb.table_path(&out, base);
        Backend::Redb.write(&path, entries).unwrap();
        assert_same_rows(base, &path, &fixtures.join(format!("{base}.redb")));
    }
    let addon_tables = addon::compile(&model, addon::Subset::MiniFixture).unwrap();
    for library in &addon_tables {
        for (base, entries) in [
            (
                format!("addon_{}_pinyin_index", library.index),
                &library.pinyin_index,
            ),
            (
                format!("addon_{}_phrase_index", library.index),
                &library.phrase_index,
            ),
        ] {
            let path = Backend::Redb.table_path(&out, &base);
            Backend::Redb.write(&path, entries).unwrap();
            assert_same_rows(&base, &path, &fixtures.join(format!("{base}.redb")));
        }
    }
    let punct_entries = punct::compile(&model).unwrap();
    let punct_path = Backend::Redb.table_path(&out, "punct");
    Backend::Redb.write(&punct_path, &punct_entries).unwrap();
    assert_same_rows("punct", &punct_path, &fixtures.join("punct.redb"));

    let _ = std::fs::remove_dir_all(&out);
}
