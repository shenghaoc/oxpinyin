//! Assembly-level integration tests for the shared runtime.
//!
//! Moved out of `src/lib.rs`'s inline tests: every one drives the public
//! seam — `Runtime::open`, session surfaces, store accessors — against
//! the committed `fixtures/w3/<backend>` data directory (the `--mini`
//! compile of the pinned model, in the compiled-in backend's container),
//! which is integration territory (components together, real files, no
//! private access).

use std::path::PathBuf;

use oxpinyin_core::{Dictionary, LanguageModel, PhraseToken, SyllableKey};
use oxpinyin_engine::{EmptyConfigSource, KeyOutcome, Selection};
use oxpinyin_runtime::{OpenError, Runtime, RuntimeSession};
use oxpinyin_user::{FIRST_USER_TOKEN, PinyinKey, SENTENCE_START};

/// A fresh, uniquely named scratch dir, removed on drop (including unwind).
/// Declared before the runtime that opens it, so it drops after every
/// handle into it.
fn scratch_dir(prefix: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("temp dir")
}

fn w3_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT)
}

#[test]
fn trellis_records_filter_user_tones_and_refresh_after_import() {
    let tmp = scratch_dir("oxpinyin-trellis-records-");
    let dir = tmp.path().to_path_buf();
    let runtime = Runtime::open(&w3_dir(), Some(&dir)).expect("runtime");
    let dict = runtime.dict();
    let syllables = [
        SyllableKey::from_text("ni").unwrap(),
        SyllableKey::from_text("hao").unwrap(),
    ];
    assert!(
        !dict
            .trellis_records(&syllables, &[3, 3])
            .unwrap()
            .iter()
            .any(|entry| entry.text() == "你鎄")
    );
    let keys: Vec<_> = syllables
        .iter()
        .map(|key| oxpinyin_user::toned_key(key.index(), 3).unwrap())
        .collect();
    let token = runtime
        .user_store()
        .unwrap()
        .add_phrase("你鎄", &keys, Some(5))
        .unwrap();
    let records = dict.trellis_records(&syllables, &[3, 3]).unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|entry| entry.token().value() == token)
            .count(),
        1
    );
    assert!(
        dict.trellis_records(&syllables, &[1, 3])
            .unwrap()
            .iter()
            .all(|entry| entry.token().value() != token)
    );
    assert_eq!(
        dict.lookup(&syllables)
            .unwrap()
            .iter()
            .filter(|entry| entry.token().value() == token)
            .count(),
        1
    );
    drop(runtime);
    drop(dict);
    tmp.close().expect("cleanup");
}

#[test]
fn sends_and_syncs() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RuntimeSession>();
    assert_send_sync::<Runtime>();
}

#[test]
fn opens_the_w3_fixture_and_decodes_nihao() {
    let runtime = Runtime::open(&w3_dir(), None).expect("the fixture dir opens");
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
fn open_refuses_a_directory_without_the_dbms() {
    let tmp = scratch_dir("oxpinyin-runtime-empty-");
    let dir = tmp.path().to_path_buf();
    match Runtime::open(&dir, None) {
        Err(OpenError::Missing(path)) => {
            assert!(
                path.ends_with(oxpinyin_data::SystemDbm::PinyinIndex.file_name()),
                "{path:?}"
            );
        }
        Err(other) => panic!("expected Missing, got {other}"),
        Ok(_) => panic!("an empty dir must not open"),
    }
}

#[test]
fn open_reads_no_records_beyond_the_handles() {
    // The fixture opens with real counts straight from the chunk files:
    // no interpolation text, no fixture mode.
    let runtime = Runtime::open(&w3_dir(), None).expect("open");
    let lm = runtime.lm();
    assert!(LanguageModel::has_real_unigrams(&lm));
    let total = LanguageModel::unigram_total(&lm)
        .expect("query")
        .expect("real unigrams");
    assert!(total > 0, "the mini set carries \\1-gram counts");
    assert!(runtime.dict().visible_item_count() > 0);
}

#[test]
fn selection_advances_and_commit_returns_the_chosen_text() {
    let runtime = Runtime::open(&w3_dir(), None).expect("open");
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
    let tmp = scratch_dir("oxpinyin-runtime-user-");
    let dir = tmp.path().to_path_buf();

    let runtime = Runtime::open(&w3_dir(), Some(&dir)).expect("open");
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

    // Language-model seam: user counts must reach the overlay — the
    // merged answer and the LM total must move by exactly the store's
    // own delta.
    let store_before = store.unigram_total().expect("store total");
    // Capture the overlay total *before* the observation so the branch
    // below measures the delta the observation causes.
    let lm_before = LanguageModel::unigram_total(&runtime.lm()).expect("total query");
    store
        .observe_selection(SENTENCE_START, FIRST_USER_TOKEN)
        .expect("observe a selection");
    let store_after = store.unigram_total().expect("store total");
    assert!(
        store_after > store_before,
        "observing must grow the store's unigram total: {store_before} -> {store_after}"
    );

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

    drop(store);
}

#[test]
fn regular_file_as_user_dir_degrades_to_no_user_state() {
    let mut file = tempfile::Builder::new()
        .prefix("oxpinyin-runtime-file-user-")
        .suffix(".tmp")
        .tempfile()
        .expect("temp file");
    std::io::Write::write_all(&mut file, b"not a directory").expect("temp file");
    // Only the path is kept: no handle on the file stays open across the
    // open below.
    let marker = file.into_temp_path();
    let runtime = Runtime::open(&w3_dir(), Some(&*marker));
    // A non-directory user path must not fail init (capi contract); it
    // simply leaves the engine without learning state.
    if let Err(error) = &runtime {
        panic!("expected open to succeed over a file user dir: {error}");
    }
    assert!(runtime.expect("open").user_store().is_none());
    marker.close().expect("cleanup temp file");
}

#[test]
fn no_user_path_means_transient_user_state() {
    // capi contract: a NULL user dir disables learning (the pin's NULL
    // `m_user_dir`). The empty path is a different argument — the
    // working directory, as the pin's `""` is — and writes there, so it
    // has an executable of its own: `tests/empty_user_dir.rs`.
    let runtime = Runtime::open(&w3_dir(), None).expect("open");
    let store = runtime.user_store().expect("transient store");
    assert!(!store.has_user_directory());
}

#[test]
fn addon_unigram_totals_stay_none_until_a_library_loads() {
    let runtime = Runtime::open(&w3_dir(), None).expect("open");
    let lm = runtime.lm();
    assert_eq!(
        LanguageModel::addon_unigram_total(&lm).expect("infallible query"),
        None,
        "no addon library is loaded"
    );
    // The committed w3 carries the addon DBM pair and art.bin; loading is
    // idempotent.
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
    // The pure-GBK hiding is exercised at the `SystemDictionary`
    // integration test (`visible_prefix_probe_hides_unloaded_libraries`).
    // This test pins the runtime seam: the survival case must not
    // regress with the mask armed, and the dead-end guarantee
    // `nbest::widen_probe` needs for termination must survive both
    // branches (`mask == 0` fast path and the visibility-filtered probe).
    let runtime = Runtime::open(&w3_dir(), None).expect("open");
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

// The key-cost table is a function of library visibility, so any cache of it
// must be keyed on the visibility mask. This pins the invariant that makes
// the mask stamp necessary: the table genuinely differs under a GBK unload
// and is restored exactly on reload.
//
// This test used to be named `..._across_sessions` and interleaved three
// `new_session` calls with the three table computations. Those calls
// asserted nothing about key costs — only that a session still builds —
// because a `RuntimeLm` always reports real unigram frequencies and so no
// `new_session` call reaches the cache at all. They are dropped rather than
// kept as incidental coverage: the session side of that gate is now pinned
// directly by `new_session_does_not_populate_the_key_cost_cache` (a
// white-box unit test in the crate, where the private cache is visible),
// and the cache's stamp-truth under racing flips by
// `key_costs_cache_stays_stamp_true_under_concurrent_visibility_flips`.
#[test]
fn gbk_visibility_changes_the_key_cost_table() {
    let runtime = Runtime::open(&w3_dir(), None).expect("open");
    let dict = runtime.dict();
    let lm = runtime.lm();

    // These direct computations are exactly what the fallback branch
    // memoises per visibility mask.
    let loaded = oxpinyin_core::scoring::key_cost_table(&dict, &lm).expect("key costs (loaded)");

    assert!(runtime.unload_library(2), "first GBK unload arms the mask");
    let unloaded =
        oxpinyin_core::scoring::key_cost_table(&dict, &lm).expect("key costs (GBK unloaded)");
    // An unloaded library drops out of the lookups and the unigram
    // denominator, so at least one frozen key must cost differently — else
    // the cache could never go stale and this guard would be vacuous.
    assert_ne!(
        loaded, unloaded,
        "GBK unload must change the key-cost table, or the mask stamp is pointless"
    );

    assert!(runtime.load_library(2), "reload clears the mask");
    let reloaded =
        oxpinyin_core::scoring::key_cost_table(&dict, &lm).expect("key costs (GBK reloaded)");
    assert_eq!(
        loaded, reloaded,
        "reload must restore the original key costs"
    );
}

#[test]
fn rejected_unigram_add_keeps_item_and_library_counts_but_moves_facade_total() {
    let runtime = Runtime::open(&w3_dir(), None).expect("open");
    let dict = runtime.dict();
    let library = dict
        .system()
        .libraries()
        .library(0x0100_0000)
        .expect("library 1");
    let tokens: Vec<_> = library
        .token_range()
        .map(|local| 0x0100_0000 | local)
        .filter(|&token| dict.system_unigram_count(token).is_some())
        .take(2)
        .collect();
    assert_eq!(tokens.len(), 2);
    let remaining = u32::MAX - library.total_freq();
    assert!(dict.add_unigram_delta(tokens[0], u64::from(remaining)));
    assert_eq!(dict.unigram_delta(tokens[0]), Some(u64::from(remaining)));
    assert!(!dict.add_unigram_delta(tokens[1], 1));
    assert_eq!(dict.unigram_delta(tokens[1]), None);
    assert!(!dict.add_unigram_delta(tokens[0], 1));
    assert_eq!(dict.unigram_delta(tokens[0]), Some(u64::from(remaining)));
    assert_eq!(dict.unigram_total_delta(), remaining + 2);
    assert!(dict.add_unigram_delta(tokens[1], 0));
}

/// The facade total is a `guint32` that wraps (`m_total_freq += delta`,
/// `phrase_index.h:633`), whether or not the item accepts the delta.
#[test]
fn the_facade_total_delta_wraps_like_a_guint32() {
    let runtime = Runtime::open(&w3_dir(), None).expect("open");
    let dict = runtime.dict();
    let token = dict
        .system()
        .libraries()
        .library(0x0100_0000)
        .expect("library 1")
        .token_range()
        .map(|local| 0x0100_0000 | local)
        .find(|&token| dict.system_unigram_count(token).is_some())
        .expect("a token");
    let _ = dict.add_unigram_delta(token, u64::from(u32::MAX));
    let _ = dict.add_unigram_delta(token, 3);
    assert_eq!(dict.unigram_total_delta(), 2);
}

#[test]
fn fresh_nbest_step_observes_training_store_writes_and_mask_out() {
    use oxpinyin_core::NbestStepRow;
    let tmp = scratch_dir("oxpinyin-nbest-step-");
    let dir = tmp.path().to_path_buf();
    let runtime = Runtime::open(&w3_dir(), Some(&dir)).unwrap();
    let mut store = runtime.user_store().unwrap();
    let keys: Vec<_> = ["ni", "hao"]
        .into_iter()
        .map(|text| SyllableKey::from_text(text).unwrap().index() as PinyinKey)
        .collect();
    let token = PhraseToken::new(store.add_phrase("你鎄", &keys, Some(5)).unwrap());
    // A second successor keeps count/total from remaining exactly 1 when
    // the tested count changes, so a stale row cannot pass by normalization.
    let other = store.add_phrase("你鎬", &keys, Some(5)).unwrap();
    store.set_bigram_count(SENTENCE_START, other, 1000).unwrap();
    let prev = PhraseToken::new(SENTENCE_START);
    let lm = runtime.lm();
    let fresh = || {
        let actual = lm
            .nbest_step_costs_in_row(&prev, &token, &mut NbestStepRow::default())
            .unwrap();
        assert_eq!(actual, lm.nbest_step_costs(&prev, &token).unwrap());
        actual
    };
    let before = fresh();
    store
        .observe_selection(SENTENCE_START, token.value())
        .unwrap();
    let trained = fresh();
    assert_ne!(trained, before);
    store
        .set_bigram_count(SENTENCE_START, token.value(), 1000)
        .unwrap();
    let written = fresh();
    assert_ne!(written, trained);
    store.mask_out(u32::MAX, token.value()).unwrap();
    let masked = fresh();
    assert_ne!(masked, written);
    drop(lm);
    drop(store);
    drop(runtime);
    tmp.close().unwrap();
}

#[test]
fn cached_nbest_row_observes_a_cloned_stores_committed_write() {
    use oxpinyin_core::{NbestStepRow, NbestSuccessors};
    let tmp = scratch_dir("oxpinyin-nbest-snapshot-");
    let dir = tmp.path().to_path_buf();
    let runtime = Runtime::open(&w3_dir(), Some(&dir)).unwrap();
    let mut store = runtime.user_store().unwrap();
    let keys: Vec<_> = ["ni", "hao"]
        .into_iter()
        .map(|text| SyllableKey::from_text(text).unwrap().index() as PinyinKey)
        .collect();
    let token = PhraseToken::new(store.add_phrase("你鎄", &keys, Some(5)).unwrap());
    let other = store.add_phrase("你鎬", &keys, Some(5)).unwrap();
    store.set_bigram_count(SENTENCE_START, other, 1000).unwrap();
    let prev = PhraseToken::new(SENTENCE_START);
    let lm = runtime.lm();
    let mut row = NbestStepRow::default();
    // The step's merged row: whether it carries `successor`, and the item
    // costs of `token` read from it.
    let carries = |row: &mut NbestStepRow, successor: u32| match lm
        .nbest_successors_in_row(&prev, row)
        .unwrap()
        .unwrap()
    {
        NbestSuccessors::Row(gram) => {
            let mut items = Vec::new();
            gram.search_into(successor, successor, &mut items).unwrap();
            !items.is_empty()
        }
        NbestSuccessors::Miss => false,
    };
    let costs = |row: &mut NbestStepRow| {
        let item = match lm.nbest_successors_in_row(&prev, row).unwrap().unwrap() {
            NbestSuccessors::Row(gram) => gram
                .get_freq(token.value())
                .unwrap()
                .map(|freq| (freq, gram.total().unwrap())),
            NbestSuccessors::Miss => None,
        };
        lm.nbest_item_costs_in_row(&prev, &token, item, row)
            .unwrap()
    };
    let before = costs(&mut row);
    assert_eq!(before, lm.nbest_step_costs(&prev, &token).unwrap());
    assert!(carries(&mut row, other));
    // This is a deterministic concurrent interleaving: a different thread
    // commits after the first candidate populated this step's row, before
    // the next candidate reads its unigram deltas. The row must not retain
    // the old counts while scoring with the new phrase-index total.
    let mut writer = store.clone();
    std::thread::spawn(move || {
        writer
            .observe_selection(SENTENCE_START, token.value())
            .unwrap();
        writer
    })
    .join()
    .unwrap();
    let trained = lm.nbest_step_costs(&prev, &token).unwrap();
    assert_ne!(trained, before);
    assert!(carries(&mut row, token.value()));
    assert_eq!(costs(&mut row), trained);
    let mut writer = store.clone();
    std::thread::spawn(move || writer.mask_out(u32::MAX, other).unwrap())
        .join()
        .unwrap();
    let forgotten = lm.nbest_step_costs(&prev, &token).unwrap();
    assert_ne!(forgotten, trained);
    assert!(!carries(&mut row, other));
    assert_eq!(costs(&mut row), forgotten);
    drop(lm);
    drop(store);
    drop(runtime);
    tmp.close().unwrap();
}

/// The fused `trellis_ranges_continued` answers exactly as the two calls
/// it replaces: ranges and probe, over system rows, an empty-marker or
/// absent key, a user-table-only prefix, toned and incomplete spellings,
/// and a live library mask.
#[test]
fn trellis_ranges_continued_matches_the_ranges_and_prefix_pair() {
    let tmp = scratch_dir("oxpinyin-trellis-fused-");
    let dir = tmp.path().to_path_buf();
    let runtime = Runtime::open(&w3_dir(), Some(&dir)).expect("runtime");
    let dict = runtime.dict();
    let texts: [&[&str]; 9] = [
        &["ni"],
        &["ni", "hao"],
        &["hao"],
        &["n"],
        &["n", "h"],
        &["ni", "hao", "ma"],
        &["zuo", "zuo", "zuo"],
        &["zuo", "zuo"],
        &["xyz"],
    ];
    let tones: [&[u8]; 5] = [&[], &[3], &[3, 3], &[1, 3], &[0, 3, 0]];
    let paths: Vec<Vec<SyllableKey>> = texts
        .iter()
        .filter_map(|path| {
            path.iter()
                .map(|text| SyllableKey::from_text(text))
                .collect::<Option<Vec<_>>>()
        })
        .collect();
    assert!(paths.len() >= 8, "the key paths parse: {}", paths.len());
    let seen = std::cell::Cell::new((0_u32, 0_u32, 0_u32));
    let compare = |phase: &str| {
        for path in &paths {
            for tones in tones {
                let mut pair = oxpinyin_core::PhraseIndexRanges::new();
                dict.trellis_ranges(path, tones, &mut pair).unwrap();
                let continued = dict.phrase_prefix_exists(path).unwrap();
                let mut fused = oxpinyin_core::PhraseIndexRanges::new();
                let fused_continued = dict
                    .trellis_ranges_continued(path, tones, &mut fused)
                    .unwrap();
                assert_eq!(
                    format!("{pair:?}"),
                    format!("{fused:?}"),
                    "{phase}: ranges of {path:?} {tones:?}"
                );
                assert_eq!(
                    continued, fused_continued,
                    "{phase}: probe of {path:?} {tones:?}"
                );
                let (ranged, yes, no) = seen.get();
                seen.set((
                    ranged + u32::from(fused.iter().next().is_some()),
                    yes + u32::from(fused_continued),
                    no + u32::from(!fused_continued),
                ));
            }
        }
    };
    compare("system only");
    let user = |texts: &[&str]| -> Vec<_> {
        texts
            .iter()
            .map(|text| {
                oxpinyin_user::toned_key(SyllableKey::from_text(text).unwrap().index(), 3).unwrap()
            })
            .collect()
    };
    let mut store = runtime.user_store().unwrap();
    store
        .add_phrase("你鎄", &user(&["ni", "hao"]), Some(5))
        .unwrap();
    store
        .add_phrase("坐坐坐", &user(&["zuo", "zuo", "zuo"]), Some(5))
        .unwrap();
    compare("with user phrases");
    assert!(runtime.unload_library(2));
    compare("library 2 unloaded");
    let (ranged, yes, no) = seen.get();
    assert!(ranged > 0 && yes > 0 && no > 0, "{ranged} {yes} {no}");
    drop(runtime);
    drop(dict);
    tmp.close().expect("cleanup");
}

// The n-best keeps what it reads of a key path's user lookup only for the
// span search that found the path. A write to the store between two
// guesses on one session must reach the second guess, both ways.
#[test]
fn a_store_write_between_guesses_reaches_the_second_guess() {
    let tmp = scratch_dir("oxpinyin-guess-memo-");
    let dir = tmp.path().to_path_buf();
    let runtime = Runtime::open(&w3_dir(), Some(&dir)).unwrap();
    let mut store = runtime.user_store().unwrap();
    let mut session = runtime.new_session(&EmptyConfigSource).unwrap();
    session.type_pinyin("nihao").unwrap();
    let mut guess = || {
        session.guess_sentence().unwrap();
        session.sentence_text(0).unwrap().to_owned()
    };
    assert_eq!(guess(), "你好");

    let keys: Vec<_> = ["ni", "hao"]
        .into_iter()
        .map(|text| SyllableKey::from_text(text).unwrap().index() as PinyinKey)
        .collect();
    let token = store.add_phrase("你鎄", &keys, Some(5)).unwrap();
    store.set_bigram_count(SENTENCE_START, token, 1000).unwrap();
    assert_eq!(guess(), "你鎄", "the second guess prices the added phrase");

    store.mask_out(u32::MAX, token).unwrap();
    assert_eq!(guess(), "你好", "the third guess no longer lists it");
    drop(session);
    drop(store);
    drop(runtime);
    tmp.close().unwrap();
}

// A memo another dictionary filled with its own type is never read: the
// runtime prices the token as `trellis_item` does, and does not panic.
#[test]
fn a_foreign_path_memo_falls_back_to_trellis_item() {
    let runtime = Runtime::open(&w3_dir(), None).unwrap();
    let dict = runtime.dict();
    let syllables = [
        SyllableKey::from_text("ni").unwrap(),
        SyllableKey::from_text("hao").unwrap(),
    ];
    let tones = [0, 0];
    let mut ranges = oxpinyin_core::PhraseIndexRanges::new();
    dict.trellis_ranges(&syllables, &tones, &mut ranges)
        .unwrap();
    let tokens: Vec<u32> = ranges.walk_tokens().map(|(token, ())| token).collect();
    assert!(!tokens.is_empty(), "vacuity guard: nihao ranges tokens");

    let foreign = oxpinyin_core::TrellisPathMemo::default();
    assert_eq!(foreign.get_or_try_init(|| Ok::<_, ()>(0_u8)), Ok(Some(&0)));
    let own = oxpinyin_core::TrellisPathMemo::default();
    for token in tokens.iter().copied().chain([0, u32::MAX]) {
        let expected = dict.trellis_item(&syllables, &tones, token).unwrap();
        assert_eq!(
            dict.trellis_item_on_path(&syllables, &tones, token, &foreign)
                .unwrap(),
            expected,
            "foreign memo, token {token:#x}"
        );
        assert_eq!(
            dict.trellis_item_on_path(&syllables, &tones, token, &own)
                .unwrap(),
            expected,
            "own memo, token {token:#x}"
        );
    }
    assert!(
        tokens.iter().any(|&token| {
            dict.trellis_item(&syllables, &tones, token)
                .unwrap()
                .is_some()
        }),
        "vacuity guard: some token prices"
    );
}
