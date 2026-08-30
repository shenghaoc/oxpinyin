//! The four backend producers must emit semantically identical tables
//! from the same canonical source.
//!
//! Compiles the system tables once and writes them through every backend
//! compiled into this build (redb always; LMDB/Tkrzw/Kyoto Cabinet behind
//! their cargo features), then reads each file back through its own store and
//! through the real loader (`oxpinyin_data::GenericLookupTable<S>`) and asserts
//! identical key/value streams and lookups. Combined with the store
//! crate's cross-backend ordering conformance tests, this is the backend
//! matrix's data-level row; the engine differential runs on the default
//! backend over tables proven identical here.
//!
//! Requires the model cache; set `OXPINYIN_DATAGEN_STRICT=1` to fail
//! instead of skip when it is absent (CI does, once per feature build).
use std::path::PathBuf;

use oxpinyin_datagen::system;
use oxpinyin_datagen::write::Backend;

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

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oxpinyin-datagen-cross-{name}-{}-{}",
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

#[test]
fn all_backends_emit_identical_tables() {
    let Some(model) = model_dir() else {
        if strict() {
            panic!("OXPINYIN_DATAGEN_STRICT=1 but no model20 cache is present");
        }
        eprintln!("skipping: model20 cache absent (run tools/model/fetch-model.sh)");
        return;
    };

    let mut backends = vec![Backend::Redb];
    if cfg!(feature = "lmdb") {
        backends.push(Backend::Lmdb);
    }
    if cfg!(feature = "tkrzw") {
        backends.push(Backend::Tkrzw);
    }
    if cfg!(feature = "kyotocabinet") {
        backends.push(Backend::KyotoCabinet);
    }
    eprintln!(
        "backends compiled in: {:?} (enable others with --features lmdb / --features tkrzw / --features kyotocabinet)",
        backends
    );

    let (tables, stats) = system::compile(&model, system::Subset::Full).unwrap();
    eprintln!("stats: {stats:?}");

    let bases: &[(&str, &oxpinyin_datagen::Entries)] = &[
        ("pinyin_index", &tables.pinyin_index),
        ("phrase_index", &tables.phrase_index),
        ("bigram", &tables.bigram),
    ];

    /// Per-backend read-back: one row set per compiled table.
    type PerBackend = Vec<oxpinyin_datagen::Entries>;
    let mut baseline: Option<PerBackend> = None;
    for backend in backends {
        let out = temp_dir(backend.extension());
        let mut per_table: PerBackend = Vec::new();
        for (base, entries) in bases {
            let path = backend.table_path(&out, base);
            backend.write(&path, entries).unwrap();
            let rows = backend.read_all(&path).unwrap();
            per_table.push(rows);
        }

        // Same key/value stream as every other backend.
        match &baseline {
            None => baseline = Some(per_table.clone()),
            Some(want) => {
                for (((base, _), got), want) in bases.iter().zip(&per_table).zip(want.iter()) {
                    assert_eq!(
                        got.len(),
                        want.len(),
                        "{base}: row count differs on {}",
                        backend.extension()
                    );
                    for (index, (a, b)) in got.iter().zip(want.iter()).enumerate() {
                        assert_eq!(
                            a,
                            b,
                            "{base}: row {index} differs on {}",
                            backend.extension()
                        );
                    }
                }
            }
        }

        // And the stream is exactly the compiled entries, byte-ordered.
        for ((base, entries), rows) in bases.iter().zip(per_table.iter()) {
            let mut expected: oxpinyin_datagen::Entries = (**entries).clone();
            expected.sort_by(|a, b| a.0.cmp(&b.0));
            assert_eq!(
                expected,
                *rows,
                "{base}: read-back differs on {}",
                backend.extension()
            );
        }

        // The real loader path agrees on point lookups.
        assert_spot_lookups(backend, &out, &tables.pinyin_index);
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// Probe keys for the loader-level check: present keys across shapes plus
/// an absent one.
const PROBE_KEYS: [&str; 4] = ["a", "ni'hao", "xi'an", "zzz'zzz"];

/// Loader-level check: `oxpinyin_data`'s generic table type opens each
/// backend's file and returns the compiled bytes for probe keys.
fn assert_spot_lookups(backend: Backend, out: &std::path::Path, compiled: &[(Vec<u8>, Vec<u8>)]) {
    use oxpinyin_data::table::GenericLookupTable;
    use oxpinyin_store::ReadStore;

    fn probes<S: ReadStore>(path: &std::path::Path) -> Vec<Option<Vec<u8>>> {
        let table = GenericLookupTable::<S>::open(path).unwrap();
        ["a", "ni'hao", "xi'an", "zzz'zzz"]
            .into_iter()
            .map(|key| table.get(key.as_bytes()).unwrap().map(<[u8]>::to_vec))
            .collect()
    }
    let path = backend.table_path(out, "pinyin_index");
    let got: Vec<Option<Vec<u8>>> = match backend {
        Backend::Redb => probes::<oxpinyin_store::RedbStore>(&path),
        #[cfg(feature = "lmdb")]
        Backend::Lmdb => probes::<oxpinyin_store::LmdbStore>(&path),
        #[cfg(feature = "tkrzw")]
        Backend::Tkrzw => probes::<oxpinyin_store::TkrzwStore>(&path),
        #[cfg(feature = "kyotocabinet")]
        Backend::KyotoCabinet => probes::<oxpinyin_store::KcStore>(&path),
        #[allow(unreachable_patterns)]
        _ => unreachable!("backend list is built from cfg"),
    };
    let expected: std::collections::BTreeMap<&[u8], &[u8]> = compiled
        .iter()
        .map(|(k, v)| (k.as_slice(), v.as_slice()))
        .collect();
    for (value, got) in PROBE_KEYS.iter().zip(&got) {
        let want = expected.get(value.as_bytes()).map(|v| v.to_vec());
        assert_eq!(
            *got,
            want,
            "lookup {value:?} differs on {}",
            backend.extension()
        );
    }
}
