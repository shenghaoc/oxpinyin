//! End-to-end tests: the training entry points, driven through the C ABI
//! functions themselves.
//!
//! The unit tests call the `#[unsafe(no_mangle)]` symbols directly (they are
//! ordinary crate functions when the crate is compiled for testing) and read
//! the counts back through the instance's own store handle — the committed
//! state, exactly as a T7 export iterator would observe it. (redb 4.1
//! refuses a second in-process write handle on the same file, so a
//! reopen-and-read would be the wrong shape anyway.) The system tables are
//! the committed mini fixture (`fixtures/w3`); no model bytes are added by
//! these tests.
//!
//! These are the §2 sequences through the wired path: the training seed
//! arithmetic (69, 138, 414, …), `sentence_start` as the first predecessor,
//! the predicted flat `+69`, remember-as-index-only, and
//! `InvalidPhrase` → `false`.

use std::ffi::CString;
use std::os::raw::c_uint;
use std::path::PathBuf;
use std::ptr;

use oxpinyin_user::{FIRST_USER_TOKEN, SENTENCE_START, UserStore};

use crate::candidates::{
    pinyin_choose_candidate, pinyin_choose_predicted_candidate, pinyin_get_candidate,
    pinyin_is_user_candidate, pinyin_remove_user_candidate, pinyin_train,
};
use crate::config::pinyin_mask_out;
use crate::context::{pinyin_init, pinyin_save};
use crate::instance::{pinyin_alloc_instance, pinyin_reset};
use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
    pinyin_end_get_phrases, pinyin_iterator_add_phrase, pinyin_iterator_has_next_phrase,
};
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::sentence::pinyin_guess_candidates;
use crate::state::{USER_STORE_FILE, instance_ref};
use crate::types::{LookupCandidate, PinyinContext, PinyinInstance};
use crate::user_data::pinyin_remember_user_input;

/// `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY`.
const DEFAULT_SORT: c_uint = 0x1e;

/// The committed W3 mini fixture's system tables.
fn system_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
}

/// A fresh, process-unique user directory (removed on drop via the guard).
struct TempUserDir {
    path: PathBuf,
}

impl TempUserDir {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("oxpinyin-capi-{tag}-{}.d", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp user dir");
        Self { path }
    }
}

impl Drop for TempUserDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn cstr(value: impl AsRef<str>) -> CString {
    CString::new(value.as_ref().as_bytes()).expect("no interior NUL")
}

/// `pinyin_init` + `pinyin_alloc_instance` over the mini fixture.
fn open(user_dir: &str) -> (*mut PinyinContext, *mut PinyinInstance) {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir);
    let context = pinyin_init(system.as_ptr(), user.as_ptr());
    assert!(!context.is_null(), "pinyin_init must open the mini fixture");
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    (context, instance)
}

/// The instance's user store handle (the same connection the entry points
/// write through; every update commits before returning).
fn store_of(instance: *mut PinyinInstance) -> &'static UserStore {
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`; the store is a value field, so the shared
    // reference is valid while the instance lives. The `'static` lifetime is
    // a test convenience: every use sits inside this function's caller and
    // never outlives the instance.
    let inst = unsafe { instance_ref(instance) };
    inst.user.as_ref().expect("instance carries a user store")
}

/// Parses `text`, guesses candidates, and returns the pointer to candidate
/// `index` (borrowed into the instance's snapshot until the next guess).
fn candidate(instance: *mut PinyinInstance, text: &str, index: c_uint) -> *mut LookupCandidate {
    let input = cstr(text);
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, input.as_ptr()),
        text.len(),
        "full input parses"
    );
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(
        pinyin_get_candidate(instance, index, &mut cand),
        "candidate {index} exists"
    );
    assert!(!cand.is_null());
    cand
}

/// The token snapshotted on the candidate pointer.
fn token_of(instance: *mut PinyinInstance, cand: *mut LookupCandidate) -> u32 {
    // SAFETY: `cand` was produced by `pinyin_get_candidate` on `instance`
    // and no later guess invalidated the snapshot.
    let inst = unsafe { instance_ref(instance) };
    inst.candidates
        .iter()
        .find(|c| std::ptr::eq(*c, cand.cast::<crate::state::CapiCandidate>()))
        .and_then(|c| c.token)
        .expect("a phrase candidate carries its token")
        .value()
}

#[test]
fn training_through_the_abi_records_the_pinned_counts() {
    let user_dir = TempUserDir::new("train");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // ── pinyin_train: the doubling seed sequence, sentence_start first ──
    let first = candidate(instance, "nihao", 0);
    let t1 = token_of(instance, first);
    assert!(pinyin_choose_candidate(instance, 0, first) > 0);

    // 69 on first selection; the predecessor is sentence_start.
    assert!(pinyin_train(instance, 0));
    {
        let store = store_of(instance);
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), 69);
        assert_eq!(store.bigram_total(SENTENCE_START).unwrap(), 69);
        assert_eq!(store.unigram_delta(t1).unwrap(), 483); // 69 * 7
    }

    // 138 on reselection (count 207), then 414 (count 621): the pinned
    // sequence 69, 138, 414, … through the wired path.
    assert!(pinyin_train(instance, 0));
    assert_eq!(
        store_of(instance).bigram_count(SENTENCE_START, t1).unwrap(),
        207
    );
    assert!(pinyin_train(instance, 0));
    assert_eq!(
        store_of(instance).bigram_count(SENTENCE_START, t1).unwrap(),
        621
    );
    assert_eq!(
        store_of(instance).unigram_delta(t1).unwrap(),
        483 + 966 + 2898
    );

    // A new composition starts fresh at sentence_start: the predecessor is
    // NOT the previous sentence's last token.
    let second = candidate(instance, "nihao", 1);
    let t2 = token_of(instance, second);
    assert_ne!(t1, t2, "distinct candidate indexes carry distinct tokens");
    assert!(pinyin_choose_candidate(instance, 0, second) > 0);
    assert!(pinyin_train(instance, 0));
    {
        let store = store_of(instance);
        assert_eq!(store.bigram_count(SENTENCE_START, t2).unwrap(), 69);
        assert_eq!(store.bigram_count(t1, t2).unwrap(), 0);
    }

    // ── pinyin_choose_predicted_candidate: flat +69, no doubling ──
    let predicted = candidate(instance, "zhongguo", 0);
    let t4 = token_of(instance, predicted);
    // Nothing selected in this composition yet: predecessor is
    // sentence_start (upstream's _get_previous_token default).
    assert!(pinyin_choose_predicted_candidate(instance, predicted));
    {
        let store = store_of(instance);
        assert_eq!(store.bigram_count(SENTENCE_START, t4).unwrap(), 69);
        assert_eq!(store.unigram_delta(t4).unwrap(), 483);
    }
    // Flat again — 138, not the training path's 414.
    assert!(pinyin_choose_predicted_candidate(instance, predicted));
    {
        let store = store_of(instance);
        assert_eq!(store.bigram_count(SENTENCE_START, t4).unwrap(), 138);
        assert_eq!(store.unigram_delta(t4).unwrap(), 966);
    }

    // After a selection in the same composition, the predicted predecessor
    // is the last selected token.
    let other = candidate(instance, "zhongguo", 1);
    let t5 = token_of(instance, other);
    assert!(pinyin_choose_candidate(instance, 0, other) > 0);
    assert!(pinyin_choose_predicted_candidate(instance, predicted));
    assert_eq!(store_of(instance).bigram_count(t5, t4).unwrap(), 69);
    assert_eq!(
        store_of(instance).bigram_count(SENTENCE_START, t4).unwrap(),
        138
    ); // unchanged
    assert!(pinyin_choose_predicted_candidate(instance, predicted));
    assert_eq!(store_of(instance).bigram_count(t5, t4).unwrap(), 138);

    // ── pinyin_remember_user_input: index-only, no training ──
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()),
        5
    );
    assert!(pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        -1
    ));
    let (user_token, pronunciation, ni_key, hao_key) = {
        let store = store_of(instance);
        let user_token = store
            .token_for_phrase("你好")
            .unwrap()
            .expect("phrase indexed");
        assert_eq!(user_token, FIRST_USER_TOKEN);
        // New-phrase seeding: default count 5 × add-phrase factor 3 — and no
        // bigram anywhere (remember does not train).
        assert_eq!(store.unigram_delta(user_token).unwrap(), 15);
        assert_eq!(store.bigram_count(SENTENCE_START, user_token).unwrap(), 0);
        assert_eq!(store.bigram_total(user_token).unwrap(), 0);
        let phrase = store.phrase(user_token).unwrap().expect("phrase stored");
        assert_eq!(phrase.text(), "你好");
        assert_eq!(phrase.pronunciations().len(), 1);
        // The current composition "nihao" supplies the pronunciation keys.
        let ni = oxpinyin_core::SyllableKey::from_text("ni")
            .expect("frozen key")
            .index() as u16;
        let hao = oxpinyin_core::SyllableKey::from_text("hao")
            .expect("frozen key")
            .index() as u16;
        assert_eq!(phrase.pronunciations()[0].keys(), &[ni, hao]);
        assert_eq!(phrase.pronunciations()[0].count(), 5);
        (
            user_token,
            phrase.pronunciations()[0].keys().to_vec(),
            ni,
            hao,
        )
    };
    assert_eq!(pronunciation, [ni_key, hao_key]);

    // Re-remembering merges onto the same token: no new allocation, no
    // unigram reseeding, the pronunciation count accumulates.
    assert!(pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        7
    ));
    {
        let store = store_of(instance);
        assert_eq!(store.token_for_phrase("你好").unwrap(), Some(user_token));
        assert_eq!(store.next_user_token().unwrap(), FIRST_USER_TOKEN + 1);
        assert_eq!(store.unigram_delta(user_token).unwrap(), 15);
        let phrase = store.phrase(user_token).unwrap().expect("phrase stored");
        assert_eq!(phrase.pronunciations().len(), 1);
        assert_eq!(phrase.pronunciations()[0].count(), 12);
    }

    // Invalid inputs: empty phrase, a bad count, and a key-count mismatch
    // (composition "n" is one key against the two-character phrase) all
    // return false without touching the index.
    assert!(!pinyin_remember_user_input(instance, cstr("").as_ptr(), -1));
    assert!(!pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        -2
    ));
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("n").as_ptr()),
        1
    );
    assert!(!pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        -1
    ));
    assert_eq!(
        store_of(instance).next_user_token().unwrap(),
        FIRST_USER_TOKEN + 1
    );

    // ── pinyin_is_user_candidate: system candidates are not user phrases ──
    // (user phrases enter candidate lists with the T4 decode merge.)
    let system_cand = candidate(instance, "nihao", 0);
    assert!(!pinyin_is_user_candidate(instance, system_cand));

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn training_entry_points_refuse_without_a_user_store() {
    let empty = cstr("");
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let context = pinyin_init(system.as_ptr(), empty.as_ptr());
    assert!(
        !context.is_null(),
        "an empty user dir is not an init failure"
    );
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());

    // Upstream pinyin_train refuses without a user dir (pinyin.cpp:2669);
    // the other training entry points degrade the same way.
    assert!(!pinyin_train(instance, 0));
    // pinyin_save refuses without a user dir too (pinyin.cpp:1133).
    assert!(!pinyin_save(context));

    let cand = candidate(instance, "nihao", 0);
    assert!(!pinyin_choose_predicted_candidate(instance, cand));
    assert!(!pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        -1
    ));
    assert!(!pinyin_mask_out(context, 0, 0));

    // Selection still works: recording the constraint needs no store.
    assert!(pinyin_choose_candidate(instance, 0, cand) > 0);

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn mask_out_clears_user_entries_and_leaves_the_flag_alone() {
    let user_dir = TempUserDir::new("mask");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // Two remembered phrases, then the frontend's "user" clear:
    // PHRASE_INDEX_LIBRARY_MASK against MAKE_TOKEN(USER_DICTIONARY, 0).
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()),
        5
    );
    assert!(pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        -1
    ));
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("shijie").as_ptr()),
        6
    );
    assert!(pinyin_remember_user_input(
        instance,
        cstr("世界").as_ptr(),
        -1
    ));
    assert!(pinyin_mask_out(context, 0x0F00_0000, 0x0700_0000));

    // The phrase export is empty; the store lost the phrases but keeps its
    // monotonic allocation cursor; nothing armed m_modified (remember and
    // mask both stay off upstream's set-sites).
    let iter = crate::iterators::pinyin_begin_get_phrases(context, 7);
    assert!(!iter.is_null());
    assert!(!crate::iterators::pinyin_iterator_has_next_phrase(iter));
    crate::iterators::pinyin_end_get_phrases(iter);
    {
        let store = store_of(instance);
        assert!(store.token_for_phrase("你好").unwrap().is_none());
        assert!(store.token_for_phrase("世界").unwrap().is_none());
        assert_eq!(store.next_user_token().unwrap(), FIRST_USER_TOKEN + 2);
        assert!(!store.is_modified());
    }
    assert!(!pinyin_save(context), "masking does not arm the save gate");

    // The "all" clear removes the remaining count tables too.
    assert!(pinyin_mask_out(context, 0, 0));
    {
        let store = store_of(instance);
        assert_eq!(store.unigram_total().unwrap(), 0);
        assert!(!store.is_modified());
    }

    // remove_user_candidate: null and system-token candidates report false
    // (user tokens never surface in candidate lists on the current ABI —
    // collection reads the system dictionary only).
    assert!(!pinyin_remove_user_candidate(instance, ptr::null_mut()));
    let cand = candidate(instance, "nihao", 0);
    assert!(!pinyin_remove_user_candidate(instance, cand));

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// Snapshot of phrase candidates as `(token, text, cost)`.
fn phrase_snapshot(instance: *mut PinyinInstance) -> Vec<(u32, String, i64)> {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_ref(instance) };
    inst.session
        .candidates()
        .iter()
        .filter_map(|candidate| {
            Some((
                candidate.token()?.value(),
                candidate.text().to_owned(),
                candidate.cost(),
            ))
        })
        .collect()
}

#[test]
fn empty_user_store_decode_is_identical_across_instances() {
    // (a) at the C ABI / mini-fixture scale: two empty stores, same input,
    // bit-identical phrase candidates. The corpus-scale empty-store pin
    // (10136 / 10182 / 94456 of 98930 / absent 1) is
    // `real_tables_session_reports_parity`.
    let a_dir = TempUserDir::new("empty-a");
    let b_dir = TempUserDir::new("empty-b");
    let (ctx_a, inst_a) = open(a_dir.path.to_str().expect("UTF-8 path"));
    let (ctx_b, inst_b) = open(b_dir.path.to_str().expect("UTF-8 path"));

    let _ = candidate(inst_a, "nihao", 0);
    let _ = candidate(inst_b, "nihao", 0);
    assert_eq!(phrase_snapshot(inst_a), phrase_snapshot(inst_b));

    crate::instance::pinyin_free_instance(inst_a);
    crate::instance::pinyin_free_instance(inst_b);
    crate::context::pinyin_fini(ctx_a);
    crate::context::pinyin_fini(ctx_b);
}

#[test]
fn populated_store_cheapens_the_trained_candidate() {
    // Populated-store pin — separate from the empty-store corpus contract.
    // Training sequence (reproducible):
    //   1. parse "nihao", guess, snapshot costs
    //   2. choose candidate 0, pinyin_train once (seed 69, unigram 483)
    //   3. reset, parse "nihao", guess again
    // The trained token's decoder cost is strictly below its empty-store
    // cost: the additive merge raised the unigram (empty-history ranking
    // on the mini fixture, which has no real-frequency construction).
    let user_dir = TempUserDir::new("populated");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let first = candidate(instance, "nihao", 0);
    let token = token_of(instance, first);
    let before = phrase_snapshot(instance);
    let before_cost = before
        .iter()
        .find(|(t, _, _)| *t == token)
        .map(|(_, _, cost)| *cost)
        .expect("the chosen token is in the empty-store list");

    assert!(pinyin_choose_candidate(instance, 0, first) > 0);
    assert!(pinyin_train(instance, 0));
    assert_eq!(store_of(instance).unigram_delta(token).unwrap(), 483);

    assert!(pinyin_reset(instance));
    let _ = candidate(instance, "nihao", 0);
    let after = phrase_snapshot(instance);
    let after_cost = after
        .iter()
        .find(|(t, _, _)| *t == token)
        .map(|(_, _, cost)| *cost)
        .expect("the trained token is still offered");
    assert!(
        after_cost < before_cost,
        "training must cheapen the trained token: before={before_cost} after={after_cost}"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn an_instance_without_a_selection_has_nothing_to_train() {
    let user_dir = TempUserDir::new("noselection");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // Upstream refuses when no sentence result exists (pinyin.cpp:2674):
    // with no candidate chosen, the sentence record is empty.
    assert!(!pinyin_train(instance, 0));

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn save_gates_on_dirty_and_roundtrips_through_the_abi() {
    let user_dir = TempUserDir::new("save");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let store_file = user_dir.path.join(USER_STORE_FILE);

    // A clean context: pinyin_save is the §4 unmodified no-op (upstream
    // returns false, pinyin.cpp:1136) and leaves the file untouched.
    let before = std::fs::metadata(&store_file)
        .expect("store file exists")
        .modified()
        .expect("mtime");
    assert!(!pinyin_save(context));
    let after = std::fs::metadata(&store_file)
        .expect("store file exists")
        .modified()
        .expect("mtime");
    assert_eq!(before, after, "a clean save must not touch the file");
    assert!(!pinyin_save(context));

    // Train once: the save gate arms, a dirty save returns true and clears.
    let first = candidate(instance, "nihao", 0);
    let t1 = token_of(instance, first);
    assert!(pinyin_choose_candidate(instance, 0, first) > 0);
    assert!(pinyin_train(instance, 0));
    assert!(pinyin_save(context));
    assert!(!pinyin_save(context), "the save cleared m_modified");

    // Decode against the populated store (T4's merge) — the state the
    // reopened store must reproduce.
    assert!(pinyin_reset(instance));
    let _ = candidate(instance, "nihao", 0);
    let decode_before = phrase_snapshot(instance);
    let counts = {
        let store = store_of(instance);
        (
            store.bigram_count(SENTENCE_START, t1).unwrap(),
            store.bigram_total(SENTENCE_START).unwrap(),
            store.unigram_delta(t1).unwrap(),
            store.unigram_total().unwrap(),
        )
    };

    // Teardown does NOT save (the §6 shutdown decision: upstream has no
    // flush) — the counts survive anyway because every training update is
    // a durable redb commit.
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);

    // Reopen: counts, allocation state, and decode all resume.
    let (context2, instance2) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let _ = candidate(instance2, "nihao", 0);
    assert_eq!(
        phrase_snapshot(instance2),
        decode_before,
        "decode-after-reopen must match the pre-save populated-store decode"
    );
    {
        let store = store_of(instance2);
        assert!(!store.is_modified(), "a reopen starts clean");
        assert_eq!(store.bigram_count(SENTENCE_START, t1).unwrap(), counts.0);
        assert_eq!(store.bigram_total(SENTENCE_START).unwrap(), counts.1);
        assert_eq!(store.unigram_delta(t1).unwrap(), counts.2);
        assert_eq!(store.unigram_total().unwrap(), counts.3);
    }
    assert!(
        !pinyin_save(context2),
        "the reopened store saves as a no-op"
    );

    crate::instance::pinyin_free_instance(instance2);
    crate::context::pinyin_fini(context2);
}

// The matching deallocator for buffers the export iterators hand out
// (`ffi::owned_cstr` allocates with libc `malloc`, which `g_free` and
// `free` both release on Linux).
unsafe extern "C" {
    fn free(ptr: *mut std::ffi::c_void);
}

/// Frees a caller-owned buffer the export iterators hand out.
fn free_exported(ptr: *mut std::ffi::c_char) {
    if !ptr.is_null() {
        // SAFETY: the iterator allocates with libc `malloc` (see
        // `owned_cstr`); `free` is the matching deallocator for the test.
        unsafe {
            free(ptr.cast());
        }
    }
}

/// Reads a caller-owned, NUL-terminated export string and frees it.
fn take_exported(ptr: *mut std::ffi::c_char) -> String {
    // SAFETY: the iterator hands out NUL-terminated, valid UTF-8 text.
    let text = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_str()
        .expect("UTF-8 export string")
        .to_owned();
    free_exported(ptr);
    text
}

/// Drains an export iterator into owned `(phrase, pinyin, count)` triples,
/// freeing each caller-owned buffer as it goes.
fn drain_phrases(iter: *mut crate::types::ExportIterator) -> Vec<(String, String, i32)> {
    let mut rows = Vec::new();
    while crate::iterators::pinyin_iterator_has_next_phrase(iter) {
        let mut phrase: *mut crate::types::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::types::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        assert!(crate::iterators::pinyin_iterator_get_next_phrase(
            iter,
            &mut phrase,
            &mut pinyin,
            &mut count,
        ));
        rows.push((take_exported(phrase), take_exported(pinyin), count));
    }
    rows
}

/// Drains a bigram export iterator the same way.
fn drain_bigrams(iter: *mut crate::types::BigramExportIterator) -> Vec<(String, String, i32)> {
    let mut rows = Vec::new();
    while crate::iterators::pinyin_bigram_iterator_has_next_phrase(iter) {
        let mut phrase: *mut crate::types::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::types::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        assert!(crate::iterators::pinyin_bigram_iterator_get_next_phrase(
            iter,
            &mut phrase,
            &mut pinyin,
            &mut count,
        ));
        rows.push((take_exported(phrase), take_exported(pinyin), count));
    }
    rows
}

#[test]
fn import_iterators_add_per_phrase_and_arm_modified_at_end() {
    let user_dir = TempUserDir::new("import");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // Null-safety first: begin/add/end all tolerate null exactly like the
    // export quartet.
    assert!(pinyin_begin_add_phrases(ptr::null_mut(), 7).is_null());
    assert!(!pinyin_iterator_add_phrase(
        ptr::null_mut(),
        cstr("你好").as_ptr(),
        cstr("nihao").as_ptr(),
        1,
    ));
    pinyin_end_add_phrases(ptr::null_mut());

    let iter = pinyin_begin_add_phrases(context, 7);
    assert!(!iter.is_null());

    // Default count -1 is 5; a same-reading re-add accumulates 5+7=12 —
    // upstream `_add_phrase` / `PhraseItem::add_pronunciation`, not an
    // add-or-update. Trailing unparsed pinyin bytes are ignored, exactly
    // like `FullPinyinParser2`'s longest-prefix final step.
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("你好").as_ptr(),
        cstr("nihao").as_ptr(),
        -1,
    ));
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("你好").as_ptr(),
        cstr("nihao").as_ptr(),
        7,
    ));
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("世界").as_ptr(),
        cstr("shijieXYZ").as_ptr(),
        3,
    ));

    // Bad pinyin (no complete keys, incomplete-only, empty) reports false
    // and writes nothing for that call.
    assert!(!pinyin_iterator_add_phrase(
        iter,
        cstr("你好").as_ptr(),
        cstr("n").as_ptr(),
        1,
    ));
    assert!(!pinyin_iterator_add_phrase(
        iter,
        cstr("你好").as_ptr(),
        cstr("!!!").as_ptr(),
        1,
    ));
    assert!(!pinyin_iterator_add_phrase(
        iter,
        cstr("你好").as_ptr(),
        cstr("").as_ptr(),
        1,
    ));

    // Adds are per-phrase durable commits, but upstream's m_modified
    // set-site is _end_add_phrases (pinyin.cpp:658) — a save before end is
    // the unmodified no-op.
    assert!(!pinyin_save(context));
    assert!(!store_of(instance).is_modified());

    pinyin_end_add_phrases(iter);

    // End armed the shared dirty flag; the next save compacts and clears it.
    assert!(store_of(instance).is_modified());
    assert!(pinyin_save(context));
    assert!(!store_of(instance).is_modified());

    let export = pinyin_begin_get_phrases(context, 7);
    assert_eq!(
        drain_phrases(export),
        vec![
            ("你好".to_owned(), "ni'hao".to_owned(), 12),
            ("世界".to_owned(), "shi'jie".to_owned(), 3),
        ]
    );
    pinyin_end_get_phrases(export);

    // A non-user index returns a live handle but has no writable store:
    // add reports false, end is still a valid release.
    let system = pinyin_begin_add_phrases(context, 1);
    assert!(!system.is_null());
    assert!(!pinyin_iterator_add_phrase(
        system,
        cstr("测试").as_ptr(),
        cstr("ce'shi").as_ptr(),
        1,
    ));
    pinyin_end_add_phrases(system);

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn empty_import_batch_arms_modified_and_saves() {
    let user_dir = TempUserDir::new("import-empty");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let iter = pinyin_begin_add_phrases(context, 7);
    assert!(!iter.is_null());
    pinyin_end_add_phrases(iter);

    // Upstream sets m_modified unconditionally at end (:657-658), even with
    // no add calls, so the next save is a write cycle, not a no-op.
    assert!(store_of(instance).is_modified());
    assert!(pinyin_save(context));
    assert!(!store_of(instance).is_modified());

    let export = pinyin_begin_get_phrases(context, 7);
    assert!(!pinyin_iterator_has_next_phrase(export));
    pinyin_end_get_phrases(export);

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn export_iterators_walk_the_stored_triples() {
    let user_dir = TempUserDir::new("export");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // Null-safety and the empty-store shape first.
    assert!(crate::iterators::pinyin_begin_get_phrases(ptr::null_mut(), 7).is_null());
    assert!(crate::iterators::pinyin_begin_get_bigram_phrases(ptr::null_mut()).is_null());
    assert!(!crate::iterators::pinyin_iterator_has_next_phrase(
        ptr::null_mut()
    ));
    assert!(!crate::iterators::pinyin_bigram_iterator_has_next_phrase(
        ptr::null_mut()
    ));

    // Remember two phrases; the second reading merges into the first.
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()),
        5
    );
    assert!(pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        -1
    ));
    assert!(pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        7
    ));
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("shijie").as_ptr()),
        6
    );
    assert!(pinyin_remember_user_input(
        instance,
        cstr("世界").as_ptr(),
        3
    ));

    // §9 phrase export: (phrase, `'`-joined pinyin, pronunciation count),
    // token order.
    let iter = crate::iterators::pinyin_begin_get_phrases(context, 7);
    assert!(!iter.is_null());
    assert_eq!(
        drain_phrases(iter),
        vec![
            ("你好".to_owned(), "ni'hao".to_owned(), 12),
            ("世界".to_owned(), "shi'jie".to_owned(), 3),
        ]
    );
    // Exhaustion: has_next false, get_next false.
    assert!(!crate::iterators::pinyin_iterator_has_next_phrase(iter));
    assert!(!crate::iterators::pinyin_iterator_get_next_phrase(
        iter,
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut(),
    ));
    crate::iterators::pinyin_end_get_phrases(iter);

    // A non-user index exports nothing (system sub-indexes are not this
    // store's data).
    let system = crate::iterators::pinyin_begin_get_phrases(context, 1);
    assert!(!system.is_null());
    assert!(!crate::iterators::pinyin_iterator_has_next_phrase(system));
    crate::iterators::pinyin_end_get_phrases(system);

    // Train one multi-phrase sentence: (你 → 好) = 69.
    let _ = candidate(instance, "nihao", 0);
    let (_, ni_ptr) = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|c| c.text.as_bytes() == "你".as_bytes())
            .expect("你 is offered for ni");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(instance, index as c_uint, &mut cand));
        (index, cand)
    };
    assert!(pinyin_choose_candidate(instance, 0, ni_ptr) > 0);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let hao_ptr = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|c| c.text.as_bytes() == "好".as_bytes())
            .expect("好 is offered for hao");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(instance, index as c_uint, &mut cand));
        cand
    };
    assert!(pinyin_choose_candidate(instance, 2, hao_ptr) > 0);
    assert!(pinyin_train(instance, 0));

    // §9 bigram export: sentence_start rows skipped; phrase = prev+next
    // text; pinyin = prev'next; count = stored × 2.
    let bigram = crate::iterators::pinyin_begin_get_bigram_phrases(context);
    assert!(!bigram.is_null());
    assert_eq!(
        drain_bigrams(bigram),
        vec![("你好".to_owned(), "ni'hao".to_owned(), 138)]
    );
    assert!(!crate::iterators::pinyin_bigram_iterator_has_next_phrase(
        bigram
    ));
    crate::iterators::pinyin_end_get_bigram_phrases(bigram);

    // The safe Rust export wrappers drive the same ABI iterators and free
    // the caller-owned buffers with the matching libc allocator.
    assert_eq!(
        crate::user_bigram_rows(context).unwrap(),
        vec![crate::ExportedBigramRow {
            phrase: "你好".to_owned(),
            pinyin: "ni'hao".to_owned(),
            count: 138,
        }]
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn import_pinyin_canonicalizes_unseparated_and_trailing_bytes() {
    let parsed = crate::import_pinyin("nihaoXYZ").expect("parses");
    assert_eq!(parsed.key_count, 2);
    assert_eq!(parsed.canonical, "ni'hao");
    assert_eq!(crate::import_pinyin("ni'hao").unwrap().canonical, "ni'hao");
}

#[test]
fn user_only_bigram_export_fails_when_rows_need_system_tables() {
    let user_dir = TempUserDir::new("user-only-bigram");
    let store_path = user_dir.path.join(USER_STORE_FILE);
    let mut store = UserStore::open(&store_path).expect("open empty store");
    // System tokens (library nibble != 7). One training seed (69) is at
    // the §9 first-seed threshold, so a real export would emit a row.
    store.observe_selection(2, 3).expect("train system tokens");
    drop(store);

    let context = crate::open_user_import_context(&user_dir.path);
    assert!(!context.is_null());
    assert!(crate::user_phrase_rows(context).unwrap().is_empty());
    assert!(crate::user_bigram_rows(context).is_none());
    crate::close_user_import_context(context);
}
