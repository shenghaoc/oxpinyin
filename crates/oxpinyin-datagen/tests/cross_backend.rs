//! The compiled backend's data directory reads back through the real
//! runtime reader.
//!
//! Compiles the system tables once, writes them through the backend this
//! build selects (exactly one per build — the store refuses two), reads
//! every DBM back row by row through the store, and opens the directory
//! with `oxpinyin_data::SystemDictionary` — the production reader — for
//! point lookups. CI runs this once per peer build; together the four
//! runs are the backend matrix's data-level row.
//!
//! Requires the model cache; set `OXPINYIN_DATAGEN_STRICT=1` to fail
//! instead of skip when it is absent (CI does, once per feature build).

use std::path::PathBuf;

use oxpinyin_core::{Dictionary, SyllableKey};
use oxpinyin_data::SystemDictionary;
use oxpinyin_datagen::write::{Backend, DbmFile};
use oxpinyin_datagen::{punct, system};

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
            .map_or(0, |d| d.as_nanos())
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compiled_backend() -> Backend {
    let mut backends: Vec<Backend> = Vec::new();
    if cfg!(feature = "kyotocabinet") {
        backends.push(Backend::KyotoCabinet);
    }
    if cfg!(feature = "redb") {
        backends.push(Backend::Redb);
    }
    if cfg!(feature = "lmdb") {
        backends.push(Backend::Lmdb);
    }
    if cfg!(feature = "tkrzw") {
        backends.push(Backend::Tkrzw);
    }
    assert_eq!(
        backends.len(),
        1,
        "the exactly-one-backend guard should have selected one peer: {backends:?}"
    );
    backends[0]
}

fn syllables(text: &str) -> Vec<SyllableKey> {
    text.split('\'')
        .map(|s| SyllableKey::from_text(s).expect("frozen syllable"))
        .collect()
}

#[test]
fn the_compiled_backend_reads_back_through_the_runtime_reader() {
    let Some(model) = model_dir() else {
        assert!(
            !strict(),
            "OXPINYIN_DATAGEN_STRICT=1 but no model20 cache is present"
        );
        eprintln!("skipping: model20 cache absent (run tools/model/fetch-model.sh)");
        return;
    };
    let backend = compiled_backend();
    eprintln!("compiled peer: {backend:?}");
    let out = temp_dir(backend.feature());

    let (tables, stats) = system::compile(&model, system::Subset::Full).unwrap();
    eprintln!("stats: {stats:?}");
    let punct_rows = punct::compile(&model).unwrap();

    for (name, bytes) in &tables.chunks {
        std::fs::write(out.join(name), bytes).unwrap();
    }
    for (dbm, entries) in [
        (DbmFile::PinyinIndex, &tables.pinyin_index),
        (DbmFile::PhraseIndex, &tables.phrase_index),
        (DbmFile::Punct, &punct_rows),
    ] {
        let path = backend.write_dbm(&out, dbm, entries).unwrap();
        // The store walk yields exactly the compiled rows, byte-ordered.
        let rows = backend.read_all_raw(&path).unwrap();
        let mut expected: oxpinyin_datagen::Entries = entries.clone();
        expected.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(expected.len(), rows.len(), "{dbm:?}: row count");
        assert_eq!(expected, rows, "{dbm:?}: read-back differs");
    }
    let bigram_path = backend
        .write_dbm(&out, DbmFile::Bigram, &tables.bigram)
        .unwrap();
    for (key, value) in tables.bigram.iter().step_by(997) {
        assert_eq!(
            backend.get_hash(&bigram_path, key).unwrap().as_deref(),
            Some(value.as_slice()),
            "bigram point read"
        );
    }

    // The production reader opens the directory and answers lookups.
    let dict = SystemDictionary::open(&out).expect("the runtime reader opens the output");
    assert_eq!(
        dict.item_count(),
        stats.phrases,
        "every phrase item resolves"
    );
    let nihao = dict.lookup(&syllables("ni'hao")).unwrap();
    assert!(
        nihao.iter().any(|entry| entry.text() == "你好"),
        "ni'hao must find 你好: {nihao:?}"
    );
    let xian = dict.lookup(&syllables("xi'an")).unwrap();
    assert!(
        xian.iter().any(|entry| entry.text() == "西安"),
        "xi'an must find 西安"
    );
    assert!(dict.phrase_prefix_exists(&syllables("zhong")).unwrap());
    assert!(dict.lookup(&syllables("zzz")).unwrap().is_empty());
    let tokens = dict.tokens_for_text("你好").unwrap();
    assert!(
        tokens
            .iter()
            .all(|token| dict.phrase_text(*token).as_deref() == Some("你好")),
        "phrase DBM and chunk files agree on 你好: {tokens:?}"
    );
    let _ = std::fs::remove_dir_all(&out);
}
