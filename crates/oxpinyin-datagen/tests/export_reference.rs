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

fn strict() -> bool {
    std::env::var_os("OXPINYIN_DATAGEN_STRICT").is_some()
}

fn reference_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("OXPINYIN_DATAGEN_REF_DIR") {
        return Some(PathBuf::from(dir));
    }
    let default = PathBuf::from("/tmp/oxpinyin-export");
    default.is_dir().then_some(default)
}

fn model_dir() -> Option<PathBuf> {
    match pinyin_oracle::model_cache::locate_model_dir() {
        Ok(Some(dir)) => Some(dir),
        Ok(None) => None,
        Err(e) => panic!("model dir set but unusable: {e:?}"),
    }
}

fn read_reference(path: &std::path::Path) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let mut rows = BTreeMap::new();
    oxpinyin_data::table::for_each_row(path, |key, value| {
        rows.insert(key.to_vec(), value.to_vec());
        Ok::<(), oxpinyin_data::TableError>(())
    })
    .unwrap();
    rows
}

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
