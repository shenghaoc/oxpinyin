//! The drop-in invariant at the Rust runtime seam: an **unmodified
//! libpinyin data directory** opens through `Runtime::open` and decodes.
//!
//! Gated on `OXPINYIN_LIBPINYIN_DATA_DIR`, a libpinyin install's `data/`
//! built with the same DBM as this crate's selected backend (Kyoto
//! Cabinet or tkrzw). The perf-matrix container provides both
//! (`/opt/libpinyin-{kc,tkrzw}/lib/libpinyin/data`); local runs without
//! the variable skip. The C-ABI half of the invariant — libpinyin.so and
//! libpinyin_capi.so on the same directory, every driver's log identical
//! — is `tools/bisection/run-same-data-dir-diff.sh`.
//!
//! Nothing here is converted, imported or copied: the directory is the
//! test input.
#![cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]

use std::path::PathBuf;

use oxpinyin_core::{Dictionary, LanguageModel, PhraseToken, SyllableKey};
use oxpinyin_engine::{EmptyConfigSource, KeyOutcome};
use oxpinyin_runtime::Runtime;

fn data_dir() -> Option<PathBuf> {
    std::env::var_os("OXPINYIN_LIBPINYIN_DATA_DIR").map(PathBuf::from)
}

fn syllables(text: &str) -> Vec<SyllableKey> {
    text.split('\'')
        .map(|s| SyllableKey::from_text(s).expect("frozen syllable"))
        .collect()
}

#[test]
fn an_unmodified_libpinyin_data_directory_opens_and_decodes() {
    let Some(dir) = data_dir() else {
        eprintln!("OXPINYIN_LIBPINYIN_DATA_DIR unset — skipping");
        return;
    };
    // The directory is libpinyin's own: its table.conf names the DBM the
    // install was built with, and every file keeps libpinyin's name.
    let conf = std::fs::read_to_string(dir.join("table.conf")).expect("table.conf");
    assert!(
        conf.contains("database format:KyotoCabinet") || conf.contains("database format:Tkrzw"),
        "not a libpinyin data dir: {conf}"
    );
    for name in [
        "pinyin_index.bin",
        "phrase_index.bin",
        "bigram.db",
        "punct.bin",
        "gb_char.bin",
        "addon_pinyin_index.bin",
        "art.bin",
    ] {
        assert!(dir.join(name).is_file(), "{name} missing");
    }

    let runtime = Runtime::open(&dir, None).expect("libpinyin's data dir opens as is");
    let dict = runtime.dict();
    let lm = runtime.lm();

    // The facade sizes are the pin's: 138,096 items over the four system
    // libraries, real corpus counts from the chunk items.
    assert_eq!(dict.visible_item_count(), 138_096);
    let total = LanguageModel::unigram_total(&lm)
        .expect("query")
        .expect("real unigrams");
    assert_eq!(
        total, 51_051_831,
        "Σ item unigram of the pinned model: 50,913,735 \\1-gram counts + one per item"
    );

    // Dictionary lookups through the pinyin DBM + chunk files.
    let nihao = dict.lookup(&syllables("ni'hao")).expect("lookup");
    assert!(
        nihao.iter().any(|entry| entry.text() == "你好"),
        "{nihao:?}"
    );
    let entry = nihao
        .iter()
        .find(|entry| entry.text() == "你好")
        .expect("你好");
    assert!(
        entry
            .pronunciation_possibility()
            .is_some_and(|(m, t)| m > 0 && m <= t),
        "possibility from the chunk pronunciations"
    );
    assert!(
        dict.phrase_prefix_exists(&syllables("zhong"))
            .expect("probe")
    );
    // A valid syllable no phrase in the fixture is keyed on: an empty
    // lookup, no panic.
    let lone = SyllableKey::from_text("beng").expect("frozen syllable");
    let _ = dict.lookup(&[lone]).expect("lookup");

    // Phrase DBM: text → tokens, and token → text back through the chunks.
    let tokens = dict.tokens_for_text("中国");
    assert!(!tokens.is_empty());
    for token in &tokens {
        assert_eq!(
            dict.system().phrase_text(token.value()).as_deref(),
            Some("中国")
        );
        assert!(
            LanguageModel::unigram_freq(&lm, token)
                .expect("query")
                .is_some(),
            "every chunk item carries a count"
        );
    }

    // Prediction through the phrase DBM's suggestion walk.
    let suggestions = dict.system().suggest_after("中").expect("suggest");
    assert!(
        suggestions.iter().any(|(_, text)| text == "中国"),
        "中 suggests 中国: {} rows",
        suggestions.len()
    );

    // Punctuation through punct.bin.
    let de = dict.tokens_for_text("的");
    assert!(
        de.iter()
            .any(|token| !dict.punctuations(token.value()).is_empty())
    );

    // The addon facade: addon_pinyin_index.bin + art.bin on demand.
    assert!(runtime.load_addon(4, &dir), "art loads from the same dir");
    assert!(!runtime.load_addon(4, &dir), "second load is false");
    let art = dict
        .lookup_addon(&syllables("er'huang"))
        .expect("addon lookup");
    assert!(art.iter().any(|entry| entry.text() == "二簧"), "{art:?}");

    // A session decodes over the same handles.
    let mut session = runtime.new_session(&EmptyConfigSource).expect("session");
    assert_eq!(
        session.type_pinyin("nihao").expect("typing"),
        KeyOutcome::Consumed
    );
    let first = session
        .candidates()
        .get(0)
        .expect("candidates")
        .text()
        .to_owned();
    assert_eq!(first, "你好");
    session.guess_sentence().expect("sentence");
    assert_eq!(session.sentence_text(0).expect("row 0"), "你好");
    let _ = PhraseToken::new(0);
}
