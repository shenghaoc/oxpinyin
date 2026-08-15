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

use pinyin_user::{FIRST_USER_TOKEN, SENTENCE_START, UserStore};

use crate::candidates::{
    pinyin_choose_candidate, pinyin_choose_predicted_candidate, pinyin_get_candidate,
    pinyin_is_user_candidate, pinyin_train,
};
use crate::context::pinyin_init;
use crate::instance::pinyin_alloc_instance;
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::sentence::pinyin_guess_candidates;
use crate::state::instance_ref;
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
        let path = std::env::temp_dir().join(format!("pinyin-capi-{tag}-{}.d", std::process::id()));
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
        let ni = pinyin_core::SyllableKey::from_text("ni")
            .expect("frozen key")
            .index() as u16;
        let hao = pinyin_core::SyllableKey::from_text("hao")
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

    let cand = candidate(instance, "nihao", 0);
    assert!(!pinyin_choose_predicted_candidate(instance, cand));
    assert!(!pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        -1
    ));

    // Selection still works: recording the constraint needs no store.
    assert!(pinyin_choose_candidate(instance, 0, cand) > 0);

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
