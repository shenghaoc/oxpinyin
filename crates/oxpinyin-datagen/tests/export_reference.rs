//! Full compile must equal the frozen oracle-derived full export,
//! entry for entry.
//!
//! The reference (`/tmp/oxpinyin-export` on the maintainer machine, or
//! `OXPINYIN_DATAGEN_REF_DIR`) holds the tables the retired
//! `oxpinyin-migrate export`/`convert` produced **from the pin-built
//! oracle's runtime data**. The native model20 compilation reproducing
//! them exactly is the proof that the forbidden
//! oracle-data → migration → oxpinyin architecture is unnecessary.
//! Requires the model cache and the reference dir; set
//! `OXPINYIN_DATAGEN_STRICT=1` to fail instead of skip when either is
//! absent (CI runs the strict variant with the reference restored from an
//! internal artifact).
use std::collections::BTreeMap;
use std::path::PathBuf;

use oxpinyin_datagen::system;

/// Checks whether strict data-generation mode is enabled through the environment.
///
/// # Examples
///
/// ```
/// let strict_mode = strict();
/// assert!(strict_mode == true || strict_mode == false);
/// ```
///
/// Returns `true` when `OXPINYIN_DATAGEN_STRICT` is set, and `false` otherwise.
fn strict() -> bool {
    std::env::var_os("OXPINYIN_DATAGEN_STRICT").is_some()
}

/// Selects the reference export directory from the environment or the default location.
///
/// The `OXPINYIN_DATAGEN_REF_DIR` environment variable takes precedence. When it is
/// unset, the default directory is used only if it exists.
///
/// # Examples
///
/// ```
/// let reference_directory = reference_dir();
/// assert!(reference_directory.is_none() || reference_directory.is_some());
/// ```
///
/// # Returns
///
/// `Some` with the configured or existing default directory, or `None` when no
/// reference directory is available.
fn reference_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OXPINYIN_DATAGEN_REF_DIR") {
        return Some(PathBuf::from(dir));
    }
    let default = PathBuf::from("/tmp/oxpinyin-export");
    default.is_dir().then_some(default)
}

/// Locates the cached model directory when it is available.
///
/// # Panics
///
/// Panics if a configured model directory exists but cannot be used.
///
/// # Examples
///
/// ```
/// let model_directory = model_dir();
/// if let Some(path) = model_directory {
///     assert!(path.is_dir());
/// }
/// ```
fn model_dir() -> Option<PathBuf> {
    match pinyin_oracle::model_cache::locate_model_dir() {
        Ok(Some(dir)) => Some(dir),
        Ok(None) => None,
        Err(e) => panic!("model dir set but unusable: {e:?}"),
    }
}

/// Loads all key-value rows from a reference table into an ordered map.
///
/// # Examples
///
/// ```
/// let rows = read_reference(std::path::Path::new("reference.redb"));
/// assert_eq!(rows.get(b"key".as_slice()), Some(&b"value".to_vec()));
/// ```
///
/// # Panics
///
/// Panics if the reference table cannot be read.
///
fn read_reference(path: &std::path::Path) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let mut rows = BTreeMap::new();
    oxpinyin_data::table::for_each_row(path, |key, value| {
        rows.insert(key.to_vec(), value.to_vec());
        Ok::<(), oxpinyin_data::TableError>(())
    })
    .unwrap();
    rows
}

/// Verifies that compiled table entries exactly match the reference entries.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeMap;
///
/// let compiled = vec![(b"ni".to_vec(), b"你".to_vec())];
/// let mut reference = BTreeMap::new();
/// reference.insert(b"ni".to_vec(), b"你".to_vec());
///
/// compare("pinyin_index", &compiled, &reference);
/// ```
///
/// `name` identifies the table in assertion messages. `compiled` contains the
/// generated entries, while `reference` contains the expected entries.
fn compare(name: &str, compiled: &[(Vec<u8>, Vec<u8>)], reference: &BTreeMap<Vec<u8>, Vec<u8>>) {
    let compiled_map: BTreeMap<&Vec<u8>, &Vec<u8>> = compiled.iter().map(|(k, v)| (k, v)).collect();
    assert_eq!(
        compiled_map.len(),
        reference.len(),
        "{name}: entry count differs"
    );
    let mut differing = 0;
    let mut first: Option<String> = None;
    for (key, want) in reference {
        match compiled_map.get(key) {
            Some(got) if *got == want => {}
            matched => {
                differing += 1;
                if first.is_none() {
                    first = Some(format!(
                        "key {:?}: compiled {:?} vs reference {:?}",
                        String::from_utf8_lossy(key),
                        matched,
                        want
                    ));
                }
            }
        }
    }
    assert_eq!(
        differing,
        0,
        "{name}: {differing} entries differ; first: {}",
        first.unwrap_or_else(|| "none".to_owned())
    );
    eprintln!(
        "{name}: all {} entries identical to the oracle-derived export",
        reference.len()
    );
}

#[test]
fn full_compile_matches_the_oracle_derived_export() {
    let (Some(model), Some(reference)) = (model_dir(), reference_dir()) else {
        if strict() {
            panic!(
                "OXPINYIN_DATAGEN_STRICT=1 but model20 cache or export reference \
                 (OXPINYIN_DATAGEN_REF_DIR | /tmp/oxpinyin-export) is absent"
            );
        }
        eprintln!("skipping: model20 cache or export reference absent");
        return;
    };

    let (tables, stats) = system::compile(&model, system::Subset::Full).unwrap();
    eprintln!("stats: {stats:?}");

    compare(
        "pinyin_index",
        &tables.pinyin_index,
        &read_reference(&reference.join("pinyin_index.redb")),
    );
    compare(
        "phrase_index",
        &tables.phrase_index,
        &read_reference(&reference.join("phrase_index.redb")),
    );
    compare(
        "bigram",
        &tables.bigram,
        &read_reference(&reference.join("bigram.redb")),
    );
}
