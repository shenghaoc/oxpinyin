//! Assembly-level integration tests for the shared runtime.
//!
//! Moved out of `src/lib.rs`'s inline tests: every one drives the public
//! seam — `Runtime::open_fixtures`/`open`, session surfaces, store
//! accessors — against the committed `fixtures/w3` tables, which is
//! integration territory (components together, real files, no private
//! access).
//!
//! Fixture mode: `fixtures/w3` has no `interpolation2.text`, so production
//! `Runtime::open` must refuse it and `open_fixtures` accepts it. Both
//! contracts are exercised below.

use std::path::{Path, PathBuf};

use oxpinyin_core::{Dictionary, LanguageModel, PhraseToken, SyllableKey};
use oxpinyin_engine::{EmptyConfigSource, KeyOutcome, Selection};
use oxpinyin_runtime::{OpenError, Runtime, RuntimeSession};
use oxpinyin_user::{FIRST_USER_TOKEN, PinyinKey, SENTENCE_START};

fn w3_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
}

#[test]
fn sends_and_syncs() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeSession>();
    assert_send_sync::<Runtime>();
}

#[test]
fn opens_the_w3_fixture_and_decodes_nihao() {
    let runtime =
        Runtime::open_fixtures(&w3_dir(), None).expect("fixture dir opens in fixture mode");
    let mut session = runtime.new_session(&EmptyConfigSource).expect("session");
    let outcome = session.type_pinyin("nihao").expect("batch typing");
    assert_eq!(outcome, KeyOutcome::Consumed);
    let first = session
        .candidates()
        .get(0)
        .expect("nihao has candidates in the fixture")
        .text()
        .to_owned();
    assert_eq!(first, "你好");

    session.guess_sentence().expect("sentence guess");
    let best = session.sentence_text(0).expect("n-best row 0");
    assert_eq!(best, "你好");
}

#[test]
fn production_open_requires_the_interpolation_model() {
    match Runtime::open(&w3_dir(), None) {
        Err(OpenError::ModelMissing(path)) => {
            assert!(path.ends_with("interpolation2.text"), "{path:?}");
        }
        Err(other) => panic!("expected ModelMissing, got {other}"),
        Ok(_) => panic!("fixture dir must not open in production mode"),
    }
}

#[test]
fn selection_advances_and_commit_returns_the_chosen_text() {
    let runtime = Runtime::open_fixtures(&w3_dir(), None).expect("open");
    let mut session = runtime.new_session(&EmptyConfigSource).expect("session");
    session.type_pinyin("nihao").expect("typing");
    let advanced = session.select(0).expect("first candidate selects");
    assert_eq!(advanced, Selection::Completed);
    assert_eq!(session.commit().expect("commit"), "你好");
    assert!(!session.is_composing());
}

#[test]
fn usable_user_dir_feeds_the_merged_lookup_and_overlay() {
    // The one case the other tests never reach: a user store that
    // actually opens. Learning must surface through BOTH moved seams —
    // the dictionary's user-phrase merge and the language model's
    // unigram overlay — via nothing but public accessors.
    let dir = std::env::temp_dir().join(format!(
        "oxpinyin-runtime-user-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp user dir");

    let runtime = Runtime::open_fixtures(&w3_dir(), Some(&dir)).expect("open");
    let mut store = runtime.user_store().expect("usable user dir opens a store");
    let syllables: Vec<SyllableKey> = ["ni", "hao"]
        .iter()
        .map(|s| SyllableKey::from_text(s).expect("fixture key"))
        .collect();
    let keys: Vec<PinyinKey> = syllables
        .iter()
        .map(|key| PinyinKey::try_from(key.index()).expect("frozen syllable inventory fits u16"))
        .collect();

    // Two scalars for two keys; the pre-add probe proves this phrase
    // arrives from the user layer, not the fixture tables.
    let phrase = "你鎄";
    let merged = || runtime.dict().lookup(&syllables).expect("merged lookup");
    assert!(
        !merged().iter().any(|entry| entry.text() == phrase),
        "fixture already carries the probe phrase"
    );

    let token = store.add_phrase(phrase, &keys, None).expect("learn it");

    // Dictionary seam: the merged lookup returns the learned phrase.
    let entries = merged();
    assert!(
        entries.iter().any(|entry| entry.text() == phrase),
        "learned phrase missing from merged lookup: {:?}",
        entries
            .iter()
            .map(|e| e.text().to_owned())
            .collect::<Vec<_>>()
    );

    // Language-model seam: user counts must reach the overlay. Fixture
    // mode has no real unigram table, where the public contract is
    // `None`; with one loaded, the merged answer and the LM total must
    // move by exactly the store's own delta.
    let store_before = store.unigram_total().expect("store total");
    // Capture the overlay total *before* the observation so the
    // real-unigram branch measures the delta the observation causes.
    // Fixture mode has no real unigram table (contract: `None`), so this
    // stays `None` there and the branch below is skipped.
    let lm_before = LanguageModel::unigram_total(&runtime.lm()).expect("total query");
    store
        .observe_selection(SENTENCE_START, FIRST_USER_TOKEN)
        .expect("observe a selection");
    let store_after = store.unigram_total().expect("store total");
    assert!(
        store_after > store_before,
        "observing must grow the store's unigram total: {store_before} -> {store_after}"
    );

    if LanguageModel::has_real_unigrams(&runtime.lm()) {
        let freq = LanguageModel::unigram_freq(&runtime.lm(), &PhraseToken::new(token))
            .expect("overlay query");
        assert!(freq.is_some(), "overlay must answer with real unigrams");
        let lm_before = lm_before.expect("real unigrams before observation");
        let lm_after = LanguageModel::unigram_total(&runtime.lm())
            .expect("total query")
            .expect("real unigrams");
        assert_eq!(
            lm_after - lm_before,
            store_after - store_before,
            "LM total must absorb exactly the store's delta"
        );
    }

    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn regular_file_as_user_dir_degrades_to_no_user_state() {
    let marker = std::env::temp_dir().join(format!(
        "oxpinyin-runtime-file-user-{}.tmp",
        std::process::id()
    ));
    std::fs::write(&marker, b"not a directory").expect("temp file");
    let runtime = Runtime::open_fixtures(&w3_dir(), Some(&marker));
    // A non-directory user path must not fail init (capi contract); it
    // simply leaves the engine without learning state.
    if let Err(error) = &runtime {
        panic!("expected open to succeed over a file user dir: {error}");
    }
    assert!(runtime.expect("open").user_store().is_none());
    std::fs::remove_file(&marker).expect("cleanup temp file");
}

#[test]
fn empty_user_path_means_no_user_state_not_a_cwd_file() {
    // capi contract: an empty user dir string disables learning; the
    // merged runtime must not fall back to creating `user_store.redb`
    // in the process's working directory.
    let runtime = Runtime::open_fixtures(&w3_dir(), Some(Path::new(""))).expect("open");
    assert!(runtime.user_store().is_none());
}

#[test]
fn addon_unigram_totals_stay_none_until_a_library_loads() {
    let runtime = Runtime::open_fixtures(&w3_dir(), None).expect("open");
    let lm = runtime.lm();
    assert_eq!(
        LanguageModel::addon_unigram_total(&lm).expect("infallible query"),
        None,
        "no addon library is loaded"
    );
    // The committed w3 carries the addon_4 pair; loading is idempotent.
    let index = 4_u8;
    assert!(runtime.load_addon(index, &w3_dir()));
    assert!(!runtime.load_addon(index, &w3_dir()));
    assert!(
        LanguageModel::addon_unigram_total(&lm)
            .expect("infallible query")
            .is_some(),
        "the loaded addon library owns items"
    );
}

#[test]
fn phrase_prefix_exists_survives_the_gbk_unload_and_the_reload_restores_the_fast_path() {
    // The CR-flagged bug on PR #234: `phrase_prefix_exists` used to
    // return `true` for a syllable prefix whose only extending entries
    // sit under a library the caller has since unloaded, letting the
    // n-best widen probe extend paths that lead nowhere visible. The
    // routing now goes through `phrase_prefix_exists_visible` with the
    // library-mask callback.
    //
    // The pure-GBK hiding is exercised at the `SystemDictionary` unit
    // test (there is no GBK-only pinyin row in the mini fixture — every
    // GBK-carrying row carries non-GBK entries too). This test pins the
    // runtime seam: the survival case must not regress with the mask
    // armed, and the dead-end guarantee `nbest::widen_probe` needs for
    // termination must survive both branches (`mask == 0` fast path
    // and the visibility-filtered probe).
    let runtime = Runtime::open_fixtures(&w3_dir(), None).expect("open");
    let dict = runtime.dict();

    let ni_hao: Vec<SyllableKey> = ["ni", "hao"]
        .iter()
        .map(|s| SyllableKey::from_text(s).expect("fixture key"))
        .collect();
    let dead_end: Vec<SyllableKey> = ["zhuang", "zhuang"]
        .iter()
        .map(|s| SyllableKey::from_text(s).expect("fixture key"))
        .collect();

    // Baseline (mask clear, fast path): `ni` prefixes `ni,hao`, the
    // full sequence is a stored phrase, and a dead-end sequence
    // reports no continuation.
    assert!(dict.phrase_prefix_exists(&ni_hao[..1]).unwrap());
    assert!(dict.phrase_prefix_exists(&ni_hao[..]).unwrap());
    assert!(!dict.phrase_prefix_exists(&dead_end).unwrap());

    // Arm the visibility mask by unloading GBK. Answers stay `true`
    // for `ni,hao` because every mini-fixture row that carries GBK
    // entries carries non-GBK ones too — the visibility filter finds
    // a surviving entry. The dead-end sequence must still answer
    // `false`, so the widen probe still terminates.
    assert!(runtime.unload_library(2), "first GBK unload arms the mask");
    assert!(dict.phrase_prefix_exists(&ni_hao[..1]).unwrap());
    assert!(dict.phrase_prefix_exists(&ni_hao[..]).unwrap());
    assert!(!dict.phrase_prefix_exists(&dead_end).unwrap());

    // Reload restores the plain probe's fast path.
    assert!(runtime.load_library(2), "reload clears the mask");
    assert!(dict.phrase_prefix_exists(&ni_hao[..1]).unwrap());
    assert!(dict.phrase_prefix_exists(&ni_hao[..]).unwrap());
    assert!(!dict.phrase_prefix_exists(&dead_end).unwrap());
}
