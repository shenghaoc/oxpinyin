//! `--mini` must reproduce the committed `fixtures/w3/<backend>/` data
//! directory of the compiled backend.
//!
//! The fixtures are the `--mini` compile of the same pinned model20,
//! committed once per backend (`fixtures/w3/README.md`). Comparison is at
//! the record level for the DBMs (container bytes depend on the writing
//! library's version) and byte-exact for the chunk files.
//!
//! Requires the model cache (`tools/model/fetch-model.sh`); set
//! `OXPINYIN_DATAGEN_STRICT=1` to fail instead of skip when it is absent.

use std::path::PathBuf;

use oxpinyin_datagen::write::{Backend, DbmFile};
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

fn compiled_backend() -> Backend {
    [
        (cfg!(feature = "kyotocabinet"), Backend::KyotoCabinet),
        (cfg!(feature = "tkrzw"), Backend::Tkrzw),
        (cfg!(feature = "lmdb"), Backend::Lmdb),
        (cfg!(feature = "redb"), Backend::Redb),
    ]
    .into_iter()
    .find_map(|(on, backend)| on.then_some(backend))
    .expect("one backend is compiled in")
}

fn fixtures_w3(backend: Backend) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/w3")
        .join(backend.extension())
}

fn assert_same_rows(
    name: &str,
    backend: Backend,
    produced: &[(Vec<u8>, Vec<u8>)],
    frozen: &std::path::Path,
) {
    let frozen_rows = backend.read_all_raw(frozen).unwrap();
    let mut produced: Vec<(Vec<u8>, Vec<u8>)> = produced.to_vec();
    produced.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        produced.len(),
        frozen_rows.len(),
        "{name}: row count differs (produced {} vs frozen {})",
        produced.len(),
        frozen_rows.len()
    );
    for (index, (a, b)) in produced.iter().zip(frozen_rows.iter()).enumerate() {
        assert_eq!(a, b, "{name}: row {index} differs");
    }
    eprintln!(
        "{name}: all {} rows identical to the fixture",
        frozen_rows.len()
    );
}

#[test]
fn mini_compile_reproduces_the_committed_fixture_directory() {
    let Some(model) = model_dir() else {
        assert!(
            !strict(),
            "OXPINYIN_DATAGEN_STRICT=1 but no model20 cache is present"
        );
        eprintln!("skipping: model20 cache absent (run tools/model/fetch-model.sh)");
        return;
    };
    let backend = compiled_backend();
    let fixtures = fixtures_w3(backend);
    assert!(
        fixtures.is_dir(),
        "no committed fixture set at {}",
        fixtures.display()
    );

    let (tables, stats) = system::compile(&model, system::Subset::MiniFixture).unwrap();
    eprintln!("mini stats: {stats:?}");
    let addons = addon::compile(&model, addon::Subset::MiniFixture).unwrap();
    let punct_rows = punct::compile(&model).unwrap();

    for (name, bytes) in tables.chunks.iter().chain(addons.chunks.iter()) {
        let frozen = std::fs::read(fixtures.join(name)).unwrap();
        assert_eq!(
            *bytes, frozen,
            "{name}: chunk bytes differ from the fixture"
        );
    }
    for (dbm, entries) in [
        (DbmFile::PinyinIndex, &tables.pinyin_index),
        (DbmFile::PhraseIndex, &tables.phrase_index),
        (DbmFile::Punct, &punct_rows),
        (DbmFile::AddonPinyinIndex, &addons.pinyin_index),
        (DbmFile::AddonPhraseIndex, &addons.phrase_index),
    ] {
        let name = backend.dbm_file_name(dbm);
        assert_same_rows(&name, backend, entries, &fixtures.join(&name));
    }
    let bigram = fixtures.join(backend.dbm_file_name(DbmFile::Bigram));
    for (key, value) in &tables.bigram {
        assert_eq!(
            backend.get_hash(&bigram, key).unwrap().as_deref(),
            Some(value.as_slice()),
            "bigram.db: key {key:02x?}"
        );
    }
    eprintln!(
        "bigram: all {} rows identical to the fixture",
        tables.bigram.len()
    );
}
