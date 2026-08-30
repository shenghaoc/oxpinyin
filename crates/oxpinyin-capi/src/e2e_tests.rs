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

use std::ptr;

use oxpinyin_user::{FIRST_USER_TOKEN, SENTENCE_START, UserStore};

use crate::candidates::{
    pinyin_choose_candidate, pinyin_choose_predicted_candidate, pinyin_clear_constraint,
    pinyin_get_candidate, pinyin_is_user_candidate, pinyin_remove_user_candidate, pinyin_train,
};
use crate::config::pinyin_mask_out;
use crate::context::{oxpinyin_init_for_fixtures, pinyin_save};
use crate::instance::{pinyin_alloc_instance, pinyin_reset};
use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
    pinyin_end_get_phrases, pinyin_iterator_add_phrase, pinyin_iterator_has_next_phrase,
};
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::sentence::{pinyin_get_sentence, pinyin_guess_candidates, pinyin_guess_sentence};
use crate::state::{instance_mut, instance_ref, user_store_file};
use crate::test_support::{DEFAULT_SORT, TempUserDir, candidate, cstr, open, system_dir};
use crate::types::{LookupCandidate, PinyinInstance};
use crate::user_data::pinyin_remember_user_input;

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
    // NOT the previous sentence's last token. The fresh start is the
    // explicit `pinyin_reset` (upstream's frontend reset-on-commit
    // contract, `pinyin.cpp:2693`): since the R5 revert (register #8) the
    // engine no longer imitates it — an evolved re-parse continues the
    // committed composition with its store.
    crate::instance::pinyin_reset(instance);
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
    let context = oxpinyin_init_for_fixtures(system.as_ptr(), empty.as_ptr());
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
    // (10190 / 10190 / 98930 of 98930 / absent 0) is
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
    let store_file = user_dir.path.join(user_store_file());

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
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &mut cand
        ));
        (index, cand)
    };
    let ni_cursor = pinyin_choose_candidate(instance, 0, ni_ptr);
    assert_eq!(ni_cursor, 2, "choosing 你 consumes the ni group");
    // Guess at the post-choose cursor, not offset 0: the window is anchored at
    // the caller offset (`pinyin.cpp:2224`, mirrored by the C2 fix), so the
    // remaining 好 group is offered at offset 2, never at 0 — where the window
    // is the whole `nihao` (你/尼/…, no 好). The frontend passes the choose
    // cursor here (ibus's `key_rest(cursor)` idiom).
    assert!(pinyin_guess_candidates(
        instance,
        ni_cursor as usize,
        DEFAULT_SORT
    ));
    let hao_ptr = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|c| c.text.as_bytes() == "好".as_bytes())
            .expect("好 is offered for hao");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &mut cand
        ));
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
    let store_path = user_dir.path.join(user_store_file());
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

#[test]
fn network_index_accepts_the_same_add_path() {
    let user_dir = TempUserDir::new("network-add");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let iter = pinyin_begin_add_phrases(context, 6);
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("网词").as_ptr(),
        cstr("wangci").as_ptr(),
        5,
    ));
    pinyin_end_add_phrases(iter);

    let export = pinyin_begin_get_phrases(context, 6);
    assert_eq!(
        drain_phrases(export),
        vec![("网词".to_owned(), "wang'ci".to_owned(), 5)]
    );
    crate::iterators::pinyin_end_get_phrases(export);

    let user_export = pinyin_begin_get_phrases(context, 7);
    assert!(!crate::iterators::pinyin_iterator_has_next_phrase(
        user_export
    ));
    crate::iterators::pinyin_end_get_phrases(user_export);

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn offset_decode_rows_carry_prefix_context() {
    let user_dir = TempUserDir::new("offset-ctx");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("nihao");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);

    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));

    let (ni_index, ni_ptr) = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|c| c.text.as_bytes() == "\u{4f60}".as_bytes())
            .expect("\u{4f60} is offered for nihao");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &mut cand
        ));
        (index, cand)
    };
    assert!(pinyin_choose_candidate(instance, 0, ni_ptr) > 0);

    assert!(pinyin_guess_sentence(instance));

    let mut sentence: *mut std::os::raw::c_char = ptr::null_mut();
    assert!(
        pinyin_get_sentence(instance, 0, &mut sentence),
        "remaining input should produce at least one sentence row"
    );
    assert!(!sentence.is_null());
    let sentence_str = crate::ffi::take_owned_cstr(sentence);
    assert!(
        sentence_str.starts_with("\u{4f60}"),
        "sentence row at offset should carry the selected prefix: got {sentence_str:?}"
    );

    assert!(pinyin_guess_candidates(instance, ni_index, DEFAULT_SORT));
    {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let nbest: Vec<_> = inst
            .candidates
            .iter()
            .filter(|c| {
                c.candidate_type == crate::types::lookup_candidate_type_t::NBEST_MATCH_CANDIDATE
            })
            .collect();
        assert!(
            !nbest.is_empty(),
            "n-best row candidates should be present after offset decode"
        );
        for cand in &nbest {
            let text = cand.text.to_str().expect("UTF-8 candidate");
            assert!(
                text.starts_with("\u{4f60}"),
                "n-best candidate at offset should carry prefix: got {text:?}"
            );
        }
    }

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// L2 + `pinyin_clear_constraint` through the ABI: a chosen forcing
/// survives the frontend's full-buffer re-parse (the store is instance
/// state that only `pinyin_reset` clears), and clear-by-offset answers
/// upstream's defined bools.
#[test]
fn the_forcing_survives_the_reparse_and_clears_by_offset() {
    let user_dir = TempUserDir::new("constraint-lifetime");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // A null instance answers false, never aborts.
    assert!(!pinyin_clear_constraint(ptr::null_mut(), 0));

    let input = cstr("nihao");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let ni_ptr = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|c| c.text.as_bytes() == "\u{4f60}".as_bytes())
            .expect("\u{4f60} is offered for nihao");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &mut cand
        ));
        cand
    };
    assert!(pinyin_choose_candidate(instance, 0, ni_ptr) > 0);

    // The frontend re-sends the whole buffer every keystroke; the forcing
    // is instance state, so it survives and the decoded row still carries
    // the chosen prefix.
    let extended = cstr("nihaohao");
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, extended.as_ptr()),
        8
    );
    assert!(pinyin_guess_sentence(instance));
    let mut sentence: *mut std::os::raw::c_char = ptr::null_mut();
    assert!(
        pinyin_get_sentence(instance, 0, &mut sentence),
        "the extended input decodes rows"
    );
    let sentence_str = crate::ffi::take_owned_cstr(sentence);
    assert!(
        sentence_str.starts_with('\u{4f60}'),
        "the forcing survived the re-parse: got {sentence_str:?}"
    );

    // A free offset answers false; a hit inside the forcing's interior
    // (its NoSearch cell) un-forces the whole run; the freed cells answer
    // false afterwards.
    assert!(!pinyin_clear_constraint(instance, 5), "cell 5 is free");
    assert!(
        pinyin_clear_constraint(instance, 1),
        "cell 1 is the run interior"
    );
    assert!(!pinyin_clear_constraint(instance, 0), "the run is free now");

    // `pinyin_reset` clears the store outright.
    assert!(pinyin_reset(instance));
    assert!(!pinyin_clear_constraint(instance, 0));

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// The forcing survives a shrinking re-parse and the re-type — the
/// backspace-after-choose contract: constraints are instance state that
/// only `pinyin_reset` clears; `validate` drops a forcing only when it
/// stops spelling under the shrunk buffer. The fixture model cannot run
/// the constrained walk (the pre-frequency fallback has no constrained
/// form), so this test pins the LIFETIME: the forcing's cell is still
/// clearable after the whole ladder and the re-type. The walk-level
/// proof runs on real tables — the differential's bp phase — and at the
/// engine level with the trellis fixture model.
#[test]
fn the_forcing_survives_a_shrinking_reparse_and_the_retype() {
    let user_dir = TempUserDir::new("constraint-backspace");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("nihaoshijie");
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, input.as_ptr()),
        "nihaoshijie".len()
    );
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let ni_ptr = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|c| c.text.as_bytes() == "\u{4f60}".as_bytes())
            .expect("\u{4f60} is offered");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &mut cand
        ));
        cand
    };
    assert!(pinyin_choose_candidate(instance, 0, ni_ptr) > 0);

    // Backspace down the ladder and re-type past the shrink: the
    // composition stayed open the whole way (the buffer shrank TO the
    // cursor at "ni", which is not a selection-committed shape).
    for buffer in [
        "nihaoshij",
        "nihaoshi",
        "nihaosh",
        "nihaos",
        "nihao",
        "niha",
        "ni",
    ] {
        let shrunk = cstr(buffer);
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, shrunk.as_ptr()),
            buffer.len()
        );
    }
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, input.as_ptr()),
        "nihaoshijie".len()
    );

    // The forcing's cell survived the ladder and the re-type: a hit
    // anywhere inside its run still clears it. Pre-fix, the first shrink
    // dropped it and this answered false.
    assert!(
        pinyin_clear_constraint(instance, 1),
        "the forcing survived the shrink ladder and the re-type"
    );
    assert!(!pinyin_clear_constraint(instance, 0), "the run is free now");

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// The other half of the parse rule, after the R5 revert (register #8): a
/// composition a SELECTION consumed still continues when the buffer
/// evolved from it — upstream's parse path never touches the constraint
/// store (`pinyin.cpp:1497-1517`) and only `pinyin_reset` clears it
/// (`pinyin.cpp:2693-2704`), so the pre-revert fresh start (the frontend's
/// reset-between-compositions contract the #141 cursor flows pinned)
/// dropped forcings upstream keeps. The window stays anchored at the
/// stale composition offset, and dropping the surviving forcing re-opens
/// the walk at 0. A buffer that shrank to the cursor never was that
/// shape (the open-composition rule above).
#[test]
fn a_selection_committed_composition_reparses_with_its_store() {
    let user_dir = TempUserDir::new("constraint-committed");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("nihao");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);
    // No `pinyin_guess_sentence` here: it seeds the W14 n-best rows and
    // the \u{4f60}\u{597d} the list then offers is their row-0 mirror — a
    // row-0 choose constrains nothing (exactly upstream), and the
    // survival probe below would be vacuous. The plain guess offers the
    // \u{4f60}\u{597d} PHRASE, whose choose writes the forcing.
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let full_ptr = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|c| c.text.as_bytes() == "\u{4f60}\u{597d}".as_bytes())
            .expect("\u{4f60}\u{597d} is offered");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &mut cand
        ));
        cand
    };
    // Consumes the whole buffer: the commit branch.
    assert_eq!(pinyin_choose_candidate(instance, 0, full_ptr), 5);

    // The next parse CONTINUES the committed composition: the forcing
    // survives into the new buffer, where the next guess's validate would
    // drop it if it stopped spelling. It still spells over 0..5, so the
    // run is live — the probe answers true exactly because the store
    // survived (the pre-revert rule answered false here).
    let next = cstr("nihaoshijie");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, next.as_ptr()), 11);
    assert!(
        pinyin_clear_constraint(instance, 0),
        "the \u{4f60}\u{597d} forcing survived the committed re-parse"
    );
    assert!(!pinyin_clear_constraint(instance, 0), "the run is free now");

    // Dropping the last forcing re-opens the composition at 0: an evolved
    // re-send keeps it, and the head window answers \u{4f60} again — the
    // old fresh-start expectation, now reached through the store's death.
    assert_eq!(pinyin_parse_more_full_pinyins(instance, next.as_ptr()), 11);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        assert!(
            inst.candidates
                .iter()
                .any(|c| c.text.as_bytes() == "\u{4f60}".as_bytes()),
            "the re-opened composition offers \u{4f60} at offset 0"
        );
    }

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// The R5 continuation must not carry a committed selection past its own
/// input: choose \u{4f60}\u{597d} for "nihao" (the commit branch), then
/// re-parse the SHORTER "ni" without a reset. The clamp cut the
/// composition below the selection, so the surviving forcing overruns
/// the new input and the record follows the survivors — the same drop
/// the next guess's validate would apply, applied at the clamp. Enter
/// then commits text valid for "ni", never the stale \u{4f60}\u{597d}
/// the pre-fix continuation answered.
#[test]
fn a_shrinking_reparse_reconciles_the_committed_selection() {
    use oxpinyin_engine::{KeyInput, KeyOutcome, LogicalKey};

    let user_dir = TempUserDir::new("constraint-committed-shrink");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let input = cstr("nihao");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);
    // No `pinyin_guess_sentence`: the rows it seeds would make the
    // \u{4f60}\u{597d} a row-0 mirror, and a row-0 choose constrains
    // nothing — the forcing this test reconciles away must exist.
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let full_ptr = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|c| c.text.as_bytes() == "\u{4f60}\u{597d}".as_bytes())
            .expect("\u{4f60}\u{597d} is offered");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &mut cand
        ));
        cand
    };
    // Consumes the whole buffer: the commit branch.
    assert_eq!(pinyin_choose_candidate(instance, 0, full_ptr), 5);

    // The shrinking re-parse continues the committed composition (R5,
    // register #8 — "ni" is a prefix of the stored buffer) and reconciles
    // it to the shortened buffer.
    let shrunk = cstr("ni");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, shrunk.as_ptr()), 2);
    {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        assert!(
            inst.session.selected_tokens().is_empty(),
            "the clamped selection reconciled away"
        );
        assert_eq!(
            inst.session.composition_offset(),
            0,
            "the composition re-opened at the survivor end"
        );
    }

    // Enter commits the raw input — the session's Enter law — and the
    // committed text is now valid for the two-byte input. The pre-fix
    // continuation answered the stale \u{4f60}\u{597d} here: a selection
    // whose span reached past the current input, with the overrun
    // forcing still live behind it.
    let outcome = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_mut(instance) };
        inst.session
            .process_key(&KeyInput::plain(LogicalKey::Enter))
            .expect("enter on a composing session cannot fail")
    };
    assert_eq!(
        outcome,
        KeyOutcome::Commit("ni".to_owned()),
        "the commit answers text valid for the current input"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn predicted_tie_groups_are_text_ascending_including_user_rows() {
    use crate::sentence::pinyin_guess_predicted_candidates_with_punctuations;
    use crate::types::lookup_candidate_type_t;

    /// Mirror of predict.rs's private `amplified_frequency` — the same
    /// mirroring convention as `amplified_law_mirrors_the_session_pinning_values`.
    fn amplified(baked: u64, total: u64) -> u64 {
        if total == 0 {
            return 0;
        }
        let possibility = (1.0_f32 - 0.312_699_f32) * baked as f32 / total as f32;
        u64::from((possibility * 256.0 * 256.0 * 256.0) as u32)
    }

    let user_dir = TempUserDir::new("pred-defined-order");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // Populate the USER seam: the prefix phrase 中 (a fixture phrase too,
    // so the system side also contributes), then two user phrases under
    // it whose TEXT order (华 U+534E before 年 U+5E74) is the REVERSE of
    // their token order (中年 is added first, so its token is lower).
    // Under the removed token pre-sort the tie group emitted 中年 then
    // 中华; the defined order is text-ascending.
    let iter = pinyin_begin_add_phrases(context, 7);
    assert!(!iter.is_null());
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("\u{4e2d}").as_ptr(),
        cstr("zhong").as_ptr(),
        9
    ));
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("\u{4e2d}\u{5e74}").as_ptr(),
        cstr("zhongnian").as_ptr(),
        7
    ));
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("\u{4e2d}\u{534e}").as_ptr(),
        cstr("zhonghua").as_ptr(),
        7
    ));
    pinyin_end_add_phrases(iter);

    let prefix = cstr("\u{4e2d}");
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        prefix.as_ptr()
    ));

    // SAFETY: live instance immediately after the guess.
    let inst = unsafe { instance_ref(instance) };
    let total = inst
        .lm
        .amplified_total(inst.dict.system().unigram_map().len() as u64);
    let rows: Vec<(String, usize, u64)> = inst
        .candidates
        .iter()
        .filter(|c| c.candidate_type == lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE)
        .map(|c| {
            let baked = c
                .token
                .and_then(|t| inst.dict.system().unigram_count(t.value()))
                .unwrap_or(0);
            (
                c.text.to_str().expect("candidate text is UTF-8").to_owned(),
                c.text
                    .to_str()
                    .expect("candidate text is UTF-8")
                    .chars()
                    .count(),
                amplified(baked, total),
            )
        })
        .collect();

    // The user rows survived the prefix slice and reached the list.
    let texts: Vec<&str> = rows.iter().map(|(text, _, _)| text.as_str()).collect();
    assert!(texts.contains(&"\u{534e}"), "中华 sliced to 华: {texts:?}");
    assert!(texts.contains(&"\u{5e74}"), "中年 sliced to 年: {texts:?}");

    // DEFINED ORDER: the emitted list is sorted by the comparator's keys
    // (length desc, amplified frequency desc) with TEXT ASCENDING inside
    // every tie group — equivalently, no later row in the same group may
    // sort before an earlier one. Covers the user seam: 华 and 年 tie
    // (both user rows, equal baked count 0) and must appear 华-first.
    for (i, (text_a, len_a, freq_a)) in rows.iter().enumerate() {
        for (text_b, len_b, freq_b) in rows.iter().skip(i + 1) {
            if len_a == len_b && freq_a == freq_b {
                assert!(
                    text_a <= text_b,
                    "tie group out of defined order: {text_a:?} before {text_b:?} \
                     in {rows:?}"
                );
            }
        }
    }
    let hua = texts
        .iter()
        .position(|t| *t == "\u{534e}")
        .expect("华 present");
    let nian = texts
        .iter()
        .position(|t| *t == "\u{5e74}")
        .expect("年 present");
    assert!(
        hua < nian,
        "text order, not token order, inside the user tie group"
    );
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn choosing_from_a_reanchored_window_uses_the_anchored_span() {
    let user_dir = TempUserDir::new("c2-reanchor-choose");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // The composition is anchored at 0 after a parse. Guessing at offset 2
    // BEFORE any choose re-anchors the window there (no cached list at 2),
    // so the rows the caller sees come from the offset-2 window.
    let _ = candidate(instance, "nihao", 0);
    assert!(pinyin_guess_candidates(instance, 2, DEFAULT_SORT));
    let hao = {
        // SAFETY: live instance immediately after the guess.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|cd| cd.text.as_bytes() == "好".as_bytes())
            .expect("好 is offered at offset 2");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &mut cand
        ));
        cand
    };
    // The chosen row's span is anchor-relative (hao covers raw 2..5). With
    // the fix the composition advances from the ANCHOR (2 + 3 = 5);
    // resolving the index against the composition-anchored cached list
    // instead would select a different row and answer a different cursor.
    assert_eq!(pinyin_choose_candidate(instance, 2, hao), 5);

    // The skipped prefix (the raw bytes [0, 2) before the anchor) is
    // retained in the committed text: a re-anchored selection must not
    // drop the typed-but-unselected bytes (the gap
    // `rebuild_selection_from_constraints` preserves). commit() resets the
    // session, so it is read last.
    let committed = {
        // SAFETY: the instance is live; commit takes &mut and resets.
        unsafe {
            crate::state::instance_mut(instance)
                .session
                .commit()
                .expect("commit")
        }
    };
    assert_eq!(
        committed,
        "ni".to_owned() + "好",
        "the typed prefix survives the re-anchored choose"
    );
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// The CR-flagged regression on PR #234: `RuntimeDict::phrase_prefix_exists`
/// used to short-circuit on the plain system probe, ignoring the
/// library-visibility mask. That leaked GBK-only continuations into
/// the n-best widen probe after `pinyin_unload_phrase_library(2)`,
/// and — as a side effect of the shared unload plumbing — kept GBK
/// candidate rows on the surface after a fresh scan too. This test
/// pins the surface: the candidate list rescanned after an unload must
/// carry no GBK-nibble tokens, and the reload must restore them. The
/// widen-probe path also has to survive the mask armed without
/// regressing the mixed rows the fixture happens to have.
#[test]
fn unloading_gbk_hides_its_tokens_from_the_candidate_surface() {
    use crate::config::{pinyin_load_phrase_library, pinyin_unload_phrase_library};

    let user_dir = TempUserDir::new("gbk-unload-hides");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // A GBK-carrying prefix — `ni` — has GBK entries mixed with the
    // system ones on the fixture's `ni` row. The initial guess must
    // include at least one GBK candidate.
    let gbk_before = collect_gbk_candidate_tokens(instance, "ni");
    assert!(
        !gbk_before.is_empty(),
        "the mini fixture must carry at least one GBK candidate under ni"
    );

    // Unload GBK. A fresh parse re-scans through the visibility mask
    // (`RuntimeDict::lookup_into` retains only visible tokens); the
    // rescanned list must carry no GBK-nibble candidate.
    assert!(pinyin_unload_phrase_library(context, 2));
    assert!(pinyin_reset(instance));
    let gbk_after = collect_gbk_candidate_tokens(instance, "ni");
    assert!(
        gbk_after.is_empty(),
        "GBK tokens leaked into candidates after unload: {gbk_after:?}"
    );

    // The sentence-decode path drives `phrase_prefix_exists` through
    // the runtime dict. With the mask armed, the widen probe must
    // still terminate and the fixture's mixed rows must still
    // support a non-empty sentence — the survival guarantee the fix
    // preserves.
    assert!(pinyin_guess_sentence(instance));
    let mut sentence: *mut std::os::raw::c_char = ptr::null_mut();
    assert!(pinyin_get_sentence(instance, 0, &mut sentence));
    assert!(
        !sentence.is_null(),
        "sentence decode must yield an n-best row"
    );
    let text = crate::ffi::take_owned_cstr(sentence);
    assert!(!text.is_empty(), "sentence text is not empty");

    // Reload GBK: the entries return to the surface.
    assert!(pinyin_load_phrase_library(context, 2));
    assert!(pinyin_reset(instance));
    let gbk_reloaded = collect_gbk_candidate_tokens(instance, "ni");
    assert_eq!(
        gbk_reloaded, gbk_before,
        "GBK tokens must return in the same order after reload"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// Reset the instance, re-parse `input`, guess a fresh candidate list,
/// and return the GBK-nibble (nibble 2) tokens that surface.
fn collect_gbk_candidate_tokens(instance: *mut PinyinInstance, input: &str) -> Vec<u32> {
    let text = cstr(input);
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, text.as_ptr()),
        input.len(),
        "full input parses"
    );
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    // SAFETY: live instance immediately after the guess.
    let inst = unsafe { instance_ref(instance) };
    inst.candidates
        .iter()
        .filter_map(|c| c.token.map(|t| t.value()))
        .filter(|token| (token >> 24) == 2)
        .collect()
}

/// Parse-termination gates: C1 (FORCE_TONE) and B2 (stop bytes), plus the
/// inherited apostrophe class — measured first-hand on the rebuilt pin and
/// in the uncovered-surface differential's phase-B/phase-C probes.
#[cfg(test)]
mod parse_termination {
    use crate::candidates::pinyin_get_n_candidate;
    use crate::config::pinyin_set_options;
    use crate::parse::pinyin_parse_more_full_pinyins;
    use crate::sentence::{pinyin_guess_candidates, pinyin_guess_sentence};
    use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, open};

    /// The parity word plus the measured profiles.
    const PARITY: u32 = 0x18a;
    const USE_TONE: u32 = 0x20;
    const FORCE_TONE: u32 = 0x40;

    fn parse_len(
        context: *mut crate::types::PinyinContext,
        instance: *mut crate::types::PinyinInstance,
        word: u32,
        input: &str,
    ) -> usize {
        assert!(pinyin_set_options(context, word));
        let text = cstr(input);
        pinyin_parse_more_full_pinyins(instance, text.as_ptr())
    }

    #[test]
    fn force_tone_rejects_toneless_input_under_use_tone() {
        // C1: under USE_TONE|FORCE_TONE (the 0x60 profile) the pin parses
        // toneless input to 0 bytes — measured on the rebuilt pin:
        // opt:0x60-nihao@0:parsed=0, opt:0x60-zai6@0:parsed=0, while the
        // toned opt:0x60-ni3hao3@0:parsed=7.
        let user_dir = TempUserDir::new("c1-force");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        let word = PARITY | USE_TONE | FORCE_TONE;

        assert_eq!(parse_len(context, instance, word, "nihao"), 0);
        assert!(
            !pinyin_guess_sentence(instance),
            "no sentence on an empty parse"
        );
        assert!(
            !pinyin_guess_candidates(instance, 0, DEFAULT_SORT),
            "the pin's empty-matrix early return (pinyin.cpp:2193): \
             no rows and no engine fallback row on an empty parse"
        );
        assert_eq!(
            parse_len(context, instance, word, "zai6"),
            0,
            "'6' is not a tone digit"
        );

        assert_eq!(
            parse_len(context, instance, word, "ni3hao3"),
            7,
            "toned input parses"
        );
        assert!(pinyin_guess_sentence(instance));
        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let mut n = 0;
        assert!(pinyin_get_n_candidate(instance, &mut n));
        assert!(n > 0, "the toned window is non-empty");

        assert_eq!(parse_len(context, instance, word, "zai4"), 4);

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

    #[test]
    fn force_tone_is_inert_without_use_tone() {
        // The pin's force-tone check is nested inside the USE_TONE branch
        // (pinyin_parser2.cpp:176-190), so the 0x1ca shape (parity plus
        // FORCE_TONE, no USE_TONE) parses exactly like the parity word —
        // measured opt:0x1ca-* identical to the control.
        let user_dir = TempUserDir::new("c1-inert");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        assert_eq!(
            parse_len(context, instance, PARITY | FORCE_TONE, "nihao"),
            5
        );
        assert_eq!(parse_len(context, instance, PARITY | FORCE_TONE, "zai6"), 3);

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

    #[test]
    fn stop_bytes_terminate_the_parse() {
        // B2: the pin stops consuming at the first byte no key matches;
        // the capi parse seam must let those bytes reach the decoder.
        // Measured: punctparse-space-mid:parsed=2,
        // punctparse-fullwidth:parsed=0, punctparse-apostrophe-mid:parsed=6.
        let user_dir = TempUserDir::new("b2-stop");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        assert_eq!(parse_len(context, instance, PARITY, "ni hao"), 2);
        assert!(
            pinyin_guess_sentence(instance),
            "the ni window still decodes"
        );
        assert_eq!(parse_len(context, instance, PARITY, "\u{ff0c}nihao"), 0);
        assert_eq!(
            parse_len(context, instance, PARITY, "ni'hao"),
            6,
            "internal run unchanged"
        );

        // The inherited apostrophe class, folded in: trailing and
        // standalone runs are consumed by the pin's propagation
        // (F-E-14 table: ni' → 3; nihao' → 6 by the same law).
        assert_eq!(parse_len(context, instance, PARITY, "nihao'"), 6);
        assert_eq!(parse_len(context, instance, PARITY, "ni'"), 3);
        assert_eq!(parse_len(context, instance, PARITY, "'''"), 3);
        // Apostrophe-only parses hold no keys but are NOT empty parses:
        // the pin answers true with zero rows there, never the engine's
        // raw-input fallback row.
        assert!(pinyin_guess_sentence(instance));
        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let mut n = 0;
        assert!(pinyin_get_n_candidate(instance, &mut n));
        assert_eq!(
            n, 0,
            "no candidates and no fallback row for a keyless parse"
        );

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}
