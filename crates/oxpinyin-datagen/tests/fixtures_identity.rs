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
//! Requires the model cache (`tools/model/fetch-model.sh`); set
//! `OXPINYIN_DATAGEN_STRICT=1` to fail instead of skip when it is absent
//! (CI does).
use std::path::PathBuf;

use oxpinyin_datagen::write::Backend;
use oxpinyin_datagen::{addon, punct, system};

/// Determines whether strict mode is enabled through the `OXPINYIN_DATAGEN_STRICT` environment variable.
///
/// # Examples
///
/// ```
/// assert_eq!(
///     strict(),
///     std::env::var_os("OXPINYIN_DATAGEN_STRICT").is_some()
/// );
/// ```
fn strict() -> bool {
    std::env::var_os("OXPINYIN_DATAGEN_STRICT").is_some()
}

/// Locates the cached model directory when available.
///
/// Returns `None` when no model cache is configured. Panics when a configured
/// model directory cannot be used.
///
/// # Examples
///
/// ```
/// let cached_model = model_dir();
/// if let Some(path) = cached_model {
///     assert!(path.is_dir());
/// }
/// ```
fn model_dir() -> Option<PathBuf>
fn model_dir() -> Option<PathBuf> {
    match pinyin_oracle::model_cache::locate_model_dir() {
        Ok(Some(dir)) => Some(dir),
        Ok(None) => None,
        Err(e) => panic!("model dir set but unusable: {e:?}"),
    }
}

/// Resolves the committed `w3` fixture directory.

///

/// # Examples

///

/// ```

/// let path = fixtures_w3();

/// assert!(path.ends_with("fixtures/w3"));

/// ```
fn fixtures_w3() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3")
}

/// Creates a unique temporary directory for generated output.
///
/// Any existing directory at the generated path is removed before the new directory is created.
///
/// # Examples
///
/// ```
/// let dir = temp_dir("example");
/// assert!(dir.is_dir());
/// let _ = std::fs::remove_dir_all(dir);
/// ```
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

/// Compares the rows in a produced Redb table with those in a frozen fixture.
///
/// Panics if either table cannot be read, their row counts differ, or any row differs.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
///
/// assert_same_rows(
///     "pinyin",
///     Path::new("target/produced.redb"),
///     Path::new("fixtures/w3/pinyin.redb"),
/// );
/// ```
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
