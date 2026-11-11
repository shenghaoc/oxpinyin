//! End-to-end tests: the training entry points, driven through the C ABI
//! functions themselves.
//!
//! The unit tests call the `#[unsafe(no_mangle)]` symbols directly (they are
//! ordinary crate functions when the crate is compiled for testing) and read
//! the counts back through the instance's own store handle — the committed
//! state, exactly as a T7 export iterator would observe it. (The store
//! backend refuses a second in-process write handle on the same file, so a
//! reopen-and-read would be the wrong shape anyway.) The system tables are
//! the committed mini fixture (`fixtures/w3`); no model bytes are added by
//! these tests.
//!
//! These are the §2 sequences through the wired path: the training seed
//! arithmetic (69, 138, 414, …), `sentence_start` as the first predecessor,
//! the predicted flat `+69`, remember-as-index-only, and
//! `InvalidPhrase` → `false`.

use oxpinyin_core::{Dictionary as _, PhraseToken};
use oxpinyin_engine::CandidateKind;
use oxpinyin_facade::TextSpan;
use std::ffi::c_uint;
use std::ptr;

// Windows only: the C API's UTF-8 paths reach the wide-character (UTF-16) file API.
#[cfg(windows)]
#[test]
fn utf8_c_api_paths_open_cjk_directories_and_files() {
    let root = tempfile::tempdir().unwrap();
    let system = root.path().join("系统");
    let user = root.path().join("用户");
    std::fs::create_dir(&system).unwrap();
    std::fs::create_dir(&user).unwrap();
    for entry in std::fs::read_dir(system_dir()).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), system.join(entry.file_name())).unwrap();
    }
    std::fs::rename(system.join("gb_char.bin"), system.join("汉字.bin")).unwrap();
    let conf = std::fs::read_to_string(system.join("table.conf")).unwrap();
    std::fs::write(
        system.join("table.conf"),
        conf.replace("gb_char.bin", "汉字.bin"),
    )
    .unwrap();
    let system = cstr(system.to_str().unwrap());
    let user_name = cstr(user.to_str().unwrap());
    let context = crate::context::pinyin_init(system.as_ptr(), user_name.as_ptr());
    assert!(!context.is_null());
    pinyin_end_add_phrases(pinyin_begin_add_phrases(
        context,
        oxpinyin_user::USER_DICTIONARY,
    ));
    assert!(pinyin_save(context));
    crate::context::pinyin_fini(context);
    assert!(user.join("user.conf").is_file());
}

use oxpinyin_user::USER_DICTIONARY;
use oxpinyin_user::{FIRST_USER_TOKEN, SENTENCE_START, UserStore};

use crate::candidates::{
    pinyin_choose_candidate, pinyin_choose_predicted_candidate, pinyin_clear_constraint,
    pinyin_get_candidate, pinyin_is_user_candidate, pinyin_remove_user_candidate, pinyin_train,
};
use crate::config::{pinyin_mask_out, pinyin_set_options};
use crate::context::{pinyin_init, pinyin_save};
use crate::instance::{pinyin_alloc_instance, pinyin_reset};
use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
    pinyin_end_get_phrases, pinyin_iterator_add_phrase, pinyin_iterator_has_next_phrase,
};
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::sentence::{pinyin_get_sentence, pinyin_guess_candidates, pinyin_guess_sentence};
use crate::state::{CapiCandidate, instance_mut, instance_ref};
use crate::test_support::{DEFAULT_SORT, TempUserDir, candidate, cstr, open, system_dir};
use crate::types::{LookupCandidate, PinyinInstance, lookup_candidate_type_t};
use crate::user_data::pinyin_remember_user_input;

/// Read the live store for one assertion; no storage reference escapes.
fn with_store<R>(instance: *mut PinyinInstance, read: impl FnOnce(&UserStore) -> R) -> R {
    // SAFETY: tests call this only with a live instance, between serialized
    // C operations. The closure cannot return a borrow into the store.
    let inst = unsafe { instance_ref(instance) };
    read(inst.core.user.expect("instance carries a user store"))
}

fn next_user_token(store: &UserStore) -> u32 {
    store
        .phrases()
        .expect("phrases")
        .iter()
        .map(|phrase| phrase.token())
        .max()
        .map_or(FIRST_USER_TOKEN, |token| token + 1)
}

/// The token snapshotted on the candidate pointer.
fn token_of(instance: *mut PinyinInstance, cand: *mut LookupCandidate) -> u32 {
    // SAFETY: `cand` was produced by `pinyin_get_candidate` on `instance`
    // and no later guess invalidated the snapshot.
    let inst = unsafe { instance_ref(instance) };
    inst.state
        .candidates
        .iter()
        .find(|c| std::ptr::eq(*c, cand.cast::<crate::state::CapiCandidate>()))
        .and_then(|c| c.token)
        .expect("a phrase candidate carries its token")
        .value()
}

/// #752: the pin's `_pre_compute_tokens` refuses a phrase if any character
/// has no dictionary token (074a2219 pinyin.cpp:3098-3130, 3685-3695).
/// NUL termination is covered differentially in contract-diff.py.
#[test]
fn remember_user_input_requires_known_characters() {
    let user_dir = TempUserDir::new("remember-known");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(pinyin_set_options(context, 0x28));
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()),
        5
    );

    for phrase in [b"ni\0".as_slice(), "你i\0".as_bytes(), "😀好\0".as_bytes()] {
        assert!(!pinyin_remember_user_input(
            instance,
            phrase.as_ptr().cast(),
            1
        ));
        assert_eq!(with_store(instance, next_user_token), FIRST_USER_TOKEN);
        let export = pinyin_begin_get_phrases(context, 7);
        assert!(!export.is_null());
        assert!(!pinyin_iterator_has_next_phrase(export));
        pinyin_end_get_phrases(export);
    }

    // Known characters remain eligible.
    assert!(pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        1
    ));
    assert_eq!(
        with_store(instance, |store| store.token_for_phrase("你好")).unwrap(),
        Some(FIRST_USER_TOKEN)
    );

    // Eligibility comes from the phrase table, including user tokens:
    // importing the previously unknown characters makes `ni` eligible.
    let import = pinyin_begin_add_phrases(context, 7);
    assert!(!import.is_null());
    for (character, reading) in [("n", "ni3"), ("i", "hao3")] {
        assert!(pinyin_iterator_add_phrase(
            import,
            cstr(character).as_ptr(),
            cstr(reading).as_ptr(),
            1
        ));
    }
    pinyin_end_add_phrases(import);
    assert!(pinyin_remember_user_input(instance, cstr("ni").as_ptr(), 1));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// The pin refuses an empty or oversized phrase before token lookup
/// (074a2219 pinyin.cpp:3677-3681). Invalid UTF-8 also has length zero.
#[test]
fn remember_user_input_rejects_empty_and_oversized_known_phrases() {
    let user_dir = TempUserDir::new("remember-length-gate");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(pinyin_set_options(context, 0x28));
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("ni").as_ptr()),
        2
    );

    for length in [0, oxpinyin_user::MAX_PHRASE_LENGTH, 65_536] {
        assert!(!pinyin_remember_user_input(
            instance,
            cstr("你".repeat(length)).as_ptr(),
            1
        ));
    }
    for invalid in [
        b"\xff\0".as_slice(),
        b"\xe4\xbd\xa0\xff\0".as_slice(),
        b"\xe4\xbd\0".as_slice(),
        b"\xc0\x80\0".as_slice(),
    ] {
        assert!(!pinyin_remember_user_input(
            instance,
            invalid.as_ptr().cast(),
            1
        ));
    }
    assert_eq!(with_store(instance, next_user_token), FIRST_USER_TOKEN);
    let export = pinyin_begin_get_phrases(context, 7);
    assert!(!export.is_null());
    assert!(!pinyin_iterator_has_next_phrase(export));
    pinyin_end_get_phrases(export);

    // The repeated character is eligible; the oversized probes above do
    // not depend on an unknown-character rejection.
    assert!(pinyin_remember_user_input(instance, cstr("你").as_ptr(), 1));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// §9: the LONGER-candidate choose surface. At a word leaving
/// `SORT_WITHOUT_LONGER_CANDIDATE` clear (ibus preset 0x1c), the guess
/// surfaces a type-7 row; choosing it answers cursor 1 (the pin's LONGER
/// branch, `pinyin.cpp:2521-2530`), records no selection, trains the
/// row's token `+483` unigram through the overlay, and the `pinyin_train`
/// that follows walks the live sentence result and writes nothing more.
#[test]
fn longer_choose_trains_the_row_unigram_and_answers_cursor_one() {
    let user_dir = TempUserDir::new("longer-choose");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // ibus preset 1 (the GSettings default): bit 0x2 clear.
    const IBUS_DEFAULT_SORT: c_uint = 0x1c;
    // "ni": the w3 mini keys hold two strictly-longer extensions
    // ("ni'hao" 你好, "ni'men" 你们 — `MINI_KEYS`), and the walk's
    // 2-key cap admits both; the winner is the max-unigram one.
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("ni").as_ptr()),
        2,
        "full input parses"
    );
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 0, IBUS_DEFAULT_SORT));

    // Surface the type-7 row.
    let mut longer: *mut LookupCandidate = std::ptr::null_mut();
    let mut n: c_uint = 0;
    assert!(crate::candidates::pinyin_get_n_candidate(
        instance, &raw mut n
    ));
    let mut longer_index = usize::MAX;
    for i in 0..n {
        let mut cand: *mut LookupCandidate = std::ptr::null_mut();
        assert!(crate::candidates::pinyin_get_candidate(
            instance,
            i,
            &raw mut cand
        ));
        let mut ctype: lookup_candidate_type_t = lookup_candidate_type_t::NORMAL_CANDIDATE;
        assert!(crate::candidates::pinyin_get_candidate_type(
            instance,
            cand,
            &raw mut ctype
        ));
        if ctype == lookup_candidate_type_t::LONGER_CANDIDATE {
            longer = cand;
            longer_index = i as usize;
            break;
        }
    }
    assert!(
        !longer.is_null(),
        "the w3 fixture offers a LONGER row for ni at 0x1c"
    );

    // The token the row trains.
    let token = {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`; the borrow ends with this block.
        let inst = unsafe { instance_ref(instance) };
        inst.state.candidates[longer_index]
            .token
            .expect("a LONGER row carries its token")
    };

    let shipped = with_store(instance, |store| store.system_unigram(token.value())).expect("item");

    // Choose: cursor 1, exactly the pin's LONGER branch.
    assert_eq!(pinyin_choose_candidate(instance, 0, longer), 1);

    // +483 through the canonical phrase-index count rows; no duplicate
    // user-bigram row.
    {
        assert_eq!(
            with_store(instance, |store| store.system_unigram(token.value())),
            Some(shipped + 483)
        );
    }
    assert_eq!(
        with_store(instance, |store| store.bigram_total(SENTENCE_START)).unwrap(),
        0
    );

    // The train that follows leans on the live sentence result and
    // writes nothing more: the stored count stays at the choose's 483 and no
    // bigram row appears (the pin's constraint-free `train_result3`
    // observes nothing).
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_train(instance, 0));
    {
        assert_eq!(
            with_store(instance, |store| store.system_unigram(token.value())),
            Some(shipped + 483)
        );
    }
    assert_eq!(
        with_store(instance, |store| store.bigram_total(SENTENCE_START)).unwrap(),
        0
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// The first snapshot row of `kind`, with its token.
fn predicted_row(
    instance: *mut PinyinInstance,
    kind: lookup_candidate_type_t,
) -> (*mut LookupCandidate, u32) {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_ref(instance) };
    let index = inst
        .candidates
        .iter()
        .position(|c| c.candidate_type() == kind && c.token.is_some())
        .expect("the prediction holds a row of this kind");
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(pinyin_get_candidate(
        instance,
        u32::try_from(index).expect("small candidate index"),
        &raw mut cand
    ));
    (cand, token_of(instance, cand))
}

/// `_get_previous_token(instance, 0)` (`pinyin.cpp:1711-1740`): a predicted
/// bigram is trained after the longest token of the prediction prefix, not
/// after `sentence_start`.
#[rstest::rstest]
fn a_predicted_bigram_trains_from_the_prediction_prefix(#[values(false, true)] finalized: bool) {
    use crate::predict::pinyin_guess_predicted_candidates;

    let user_dir = TempUserDir::new("predicted-prefix");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    if finalized {
        crate::test_support::finalize_public_context(context, instance);
    }
    let prefix = cstr("你");
    let ni = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        inst.core.dict.tokens_for_text("你")[0].value()
    };

    // A prefix choice raises only its unigram. Seed an existing bigram
    // explicitly to test the separate bigram-choice predecessor.
    assert!(pinyin_guess_predicted_candidates(instance, prefix.as_ptr()));
    let (prefix_row, word) = predicted_row(
        instance,
        lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE,
    );
    assert!(pinyin_choose_predicted_candidate(instance, prefix_row));
    assert_eq!(
        with_store(instance, |store| store.bigram_count(ni, word)).unwrap(),
        0
    );
    // SAFETY: the live context has no reader retained across this write.
    unsafe { crate::state::instance_context_mut(instance) }
        .core
        .user_store_mut()
        .unwrap()
        .set_bigram_count(ni, word, 10)
        .unwrap();

    assert!(pinyin_guess_predicted_candidates(instance, prefix.as_ptr()));
    let (bigram_row, bigram_word) = predicted_row(
        instance,
        lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE,
    );
    assert_eq!(bigram_word, word, "the bigram row is the trained word");
    let before = with_store(instance, |store| store.bigram_count(ni, word)).unwrap();
    let sentence_start_before =
        with_store(instance, |store| store.bigram_count(SENTENCE_START, word)).unwrap();
    assert!(pinyin_choose_predicted_candidate(instance, bigram_row));
    assert_eq!(
        with_store(instance, |store| store.bigram_count(ni, word)).unwrap(),
        before + 69,
        "the predecessor is the prefix token"
    );
    assert_eq!(
        with_store(instance, |store| store.bigram_count(SENTENCE_START, word)).unwrap(),
        sentence_start_before,
        "not sentence_start"
    );

    crate::instance::pinyin_free_instance(instance);
    if !finalized {
        crate::context::pinyin_fini(context);
    }
}

/// A sole missing prefix row still chooses. The facade total takes the
/// fixed 483 (`pinyin.cpp:2608-2616`, `phrase_index.h:628-634`), wrapping,
/// while the absent item is not written; an unloaded library leaves the
/// total alone and still answers true (#1231 ruling).
#[rstest::rstest]
fn choosing_a_sole_missing_prefix_row_advances_the_facade_total(
    #[values(false, true)] finalized: bool,
) {
    let user_dir = TempUserDir::new("missing-prefix-choose");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    if finalized {
        crate::test_support::finalize_public_context(context, instance);
    }
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_mut(instance) };
    // Library 1 is loaded; this slot is absent from the mini fixture, so the
    // item read fails exactly as a journal-removed phrase item does.
    let token = 0x0100_FFFE;
    assert!(inst.core.dict.phrase_table_token_enabled(token));
    assert!(!inst.core.dict.token_exists(token));
    inst.state.candidates.push(CapiCandidate {
        text: TextSpan::NULL,
        text_ptr: ptr::null(),
        kind: CandidateKind::Phrase,
        type_tag: lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE.tag(),
        nbest_index: 0,
        token: Some(PhraseToken::new(token)),
        source_index: 0,
    });
    let candidate = &mut inst.state.candidates[0] as *mut CapiCandidate as *mut LookupCandidate;
    let before = inst.core.lm.amplified_total();
    assert!(pinyin_choose_predicted_candidate(instance, candidate));
    // SAFETY: the C operation completed; re-borrow its live instance.
    let inst = unsafe { instance_ref(instance) };
    let after = inst.core.lm.amplified_total();
    assert_eq!(
        (after.wrapping_sub(before)) as u32,
        483,
        "the loaded facade total takes the fixed increment"
    );
    assert!(
        inst.core
            .dict
            .live_items()
            .read(token, |_| ())
            .found()
            .is_none(),
        "the missing item is not written"
    );

    assert!(
        // SAFETY: the preceding instance read has ended before the owner write.
        unsafe { crate::state::instance_context_mut(instance) }
            .core
            .runtime
            .unload_library(1)
    );
    // SAFETY: re-borrow the live instance after the owner write.
    let inst = unsafe { instance_ref(instance) };
    let before = inst.core.lm.amplified_total();
    assert!(pinyin_choose_predicted_candidate(instance, candidate));
    // SAFETY: the C operation completed; re-borrow its live instance.
    let inst = unsafe { instance_ref(instance) };
    assert_eq!(
        inst.core.lm.amplified_total(),
        before,
        "an unloaded library has no facade total to advance"
    );

    crate::instance::pinyin_free_instance(instance);
    if !finalized {
        crate::context::pinyin_fini(context);
    }
}

/// Forces a phrase after the last sentence lookup that the decoded result
/// does not hold: the stale-forcing shape of `pinyin_train`.
fn force_a_stale_phrase(instance: *mut PinyinInstance) -> u32 {
    let input = cstr("zhongguo");
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, input.as_ptr()),
        "zhongguo".len()
    );
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(
        instance,
        0,
        crate::test_support::DEFAULT_SORT
    ));
    // The last word row: not the phrase the sentence decoded at 0.
    let index = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        inst.state
            .candidates
            .iter()
            .rposition(|c| {
                c.candidate_type() == lookup_candidate_type_t::NORMAL_CANDIDATE && c.token.is_some()
            })
            .expect("a word candidate")
    };
    let mut other: *mut LookupCandidate = ptr::null_mut();
    assert!(pinyin_get_candidate(
        instance,
        u32::try_from(index).expect("small index"),
        &raw mut other
    ));
    let forced = token_of(instance, other);
    assert!(pinyin_choose_candidate(instance, 0, other) > 0);
    forced
}

/// `train_result3`'s `assert(token == constraint->m_token)`
/// (`phonetic_lookup.h:868`): a phrase forced after the last sentence lookup
/// no longer matches the decoded result, and the process aborts as the
/// pin's does (register row 6).
#[test]
fn training_a_stale_forcing_aborts() {
    crate::test_support::assert_aborts("e2e_tests::training_a_stale_forcing_aborts", || {
        let user_dir = TempUserDir::new("train-stale");
        let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        force_a_stale_phrase(instance);
        pinyin_train(instance, 0);
    });
}

/// Looking the sentence up again after a stale forcing brings the two back
/// in step, and training goes ahead.
#[test]
fn a_new_sentence_lookup_trains_a_forcing() {
    let user_dir = TempUserDir::new("train-restep");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    force_a_stale_phrase(instance);
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_train(instance, 0), "the lookup is back in step");

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// The pin opens the directories it is given by their bytes (`g_strdup`,
/// `pinyin.cpp:331-336`), so a name that is not UTF-8 is a directory like any
/// other, for the system tables and for the user dir alike (#587).
// Unix only: byte-string directory names (macOS refuses them, which the test asserts).
#[cfg(unix)]
#[test]
fn directories_with_non_utf8_names_open() {
    use std::ffi::{CString, OsStr};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    /// Removes the fixture however the test ends, a failed assertion included.
    struct RemoveFixture<'a> {
        system: &'a std::path::Path,
        user: &'a std::path::Path,
    }
    impl Drop for RemoveFixture<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.system);
            let _ = std::fs::remove_dir_all(self.user);
        }
    }

    let scratch = std::env::temp_dir();
    let pid = std::process::id();
    let named = |prefix: &str| {
        let mut name = prefix.as_bytes().to_vec();
        name.extend_from_slice(b"\xff");
        name.extend_from_slice(pid.to_string().as_bytes());
        scratch.join(OsStr::from_bytes(&name))
    };
    let system = named("oxpinyin-system-");
    let user = named("oxpinyin-user-");
    let _ = std::fs::remove_file(&system);
    let _ = std::fs::remove_dir_all(&user);
    let _remove_fixture = RemoveFixture {
        system: &system,
        user: &user,
    };
    // macOS's expected result: APFS refuses both names with `EILSEQ`, so the
    // fixture never exists and no oxpinyin code runs.
    if cfg!(target_os = "macos") {
        for refused in [
            std::os::unix::fs::symlink(system_dir(), &system),
            std::fs::create_dir(&user),
        ] {
            let error = refused.expect_err("APFS refuses a non-UTF-8 name");
            assert_eq!(error.raw_os_error(), Some(libc::EILSEQ));
        }
        return;
    }
    std::os::unix::fs::symlink(system_dir(), &system).expect("symlink to the fixtures");
    std::fs::create_dir(&user).expect("user dir");

    let c_path = |path: &std::path::Path| {
        CString::new(path.as_os_str().to_owned().into_vec()).expect("no NUL in the path")
    };
    let (system_c, user_c) = (c_path(&system), c_path(&user));
    let context = pinyin_init(system_c.as_ptr(), user_c.as_ptr());
    assert!(!context.is_null(), "a non-UTF-8 name is a directory");
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    let _ = pinyin_save(context);
    assert!(
        std::fs::read_dir(&user).expect("user dir").next().is_some(),
        "the context opened the user dir it was given"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn null_user_directory_refuses_training_but_keeps_mutable_state() {
    // No user store is a NULL user dir. An empty string is a user dir —
    // the working directory, as the pin's is (#619).
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let context = pinyin_init(system.as_ptr(), ptr::null());
    assert!(!context.is_null(), "a NULL user dir is not an init failure");
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());

    // Upstream pinyin_train refuses without a user dir (pinyin.cpp:2669);
    // the other training entry points degrade the same way.
    assert!(!pinyin_train(instance, 0));
    // pinyin_save refuses without a user dir too (pinyin.cpp:1133).
    assert!(!pinyin_save(context));

    // (`pinyin_choose_predicted_candidate` takes a predicted row only; a
    // normal row is the pin's assert, `pinyin.cpp:2593`.)
    let cand = candidate(instance, "nihao", 0);
    assert!(pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        -1
    ));
    assert!(pinyin_mask_out(context, 0, 0));

    // Selection still works: recording the constraint needs no store.
    assert!(pinyin_choose_candidate(instance, 0, cand) > 0);

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// Snapshot of phrase candidates as `(token, text, cost)`.
fn phrase_snapshot(instance: *mut PinyinInstance) -> Vec<(u32, String, i64)> {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let mut inst = unsafe { instance_mut(instance) };
    // The facade defers the parse's list build to its first reader.
    inst.core
        .session
        .refresh_candidates()
        .expect("deferred candidate build");
    inst.core
        .session
        .candidates()
        .iter()
        .filter_map(|candidate| {
            Some((
                candidate.token()?.value(),
                candidate.text().as_str().unwrap_or_default().to_owned(),
                candidate.cost(),
            ))
        })
        .collect()
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
    // `pinyin_init` writes the profile marker (`check_format`'s
    // user.conf, counter ratchet and all); the durable store is the
    // pin's file set, written only at `pinyin_save`.
    let marker = user_dir.path.join("user.conf");
    assert!(marker.exists(), "init writes the user.conf marker");

    // A clean context: pinyin_save is the §4 unmodified no-op (upstream
    // returns false, pinyin.cpp:1136) and leaves every file untouched.
    let before = std::fs::metadata(&marker)
        .expect("marker exists")
        .modified()
        .expect("mtime");
    assert!(!pinyin_save(context));
    let after = std::fs::metadata(&marker)
        .expect("marker exists")
        .modified()
        .expect("mtime");
    assert_eq!(before, after, "a clean save must not touch the profile");
    assert!(!pinyin_save(context));

    // Train once: the save gate arms, a dirty save returns true, clears,
    // and leaves the pin's own files behind — the bigram hash among them.
    let first = candidate(instance, "nihao", 0);
    let t1 = token_of(instance, first);
    assert!(pinyin_choose_candidate(instance, 0, first) > 0);
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_train(instance, 0));
    assert!(pinyin_save(context));
    assert!(!pinyin_save(context), "the save cleared m_modified");
    assert!(
        user_dir
            .path
            .join(oxpinyin_data::user_files::UserDbm::Bigram.file_name())
            .exists(),
        "the dirty save wrote the user bigram"
    );

    // Decode against the populated store (T4's merge) — the state the
    // reopened store must reproduce.
    assert!(pinyin_reset(instance));
    let _ = candidate(instance, "nihao", 0);
    let decode_before = phrase_snapshot(instance);
    let counts = {
        (
            with_store(instance, |store| store.bigram_count(SENTENCE_START, t1)).unwrap(),
            with_store(instance, |store| store.bigram_total(SENTENCE_START)).unwrap(),
            with_store(instance, |store| store.system_unigram(t1)),
            with_store(instance, |store| store.unigram_total()).unwrap(),
        )
    };

    // Teardown does NOT save (the §6 shutdown decision: upstream has no
    // flush) — the counts survive because the dirty save above already
    // wrote the profile, exactly the pin's own durability shape.
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
        assert!(
            !with_store(instance2, |store| store.is_modified()),
            "a reopen starts clean"
        );
        assert_eq!(
            with_store(instance2, |store| store.bigram_count(SENTENCE_START, t1)).unwrap(),
            counts.0
        );
        assert_eq!(
            with_store(instance2, |store| store.bigram_total(SENTENCE_START)).unwrap(),
            counts.1
        );
        assert_eq!(
            with_store(instance2, |store| store.system_unigram(t1)),
            counts.2
        );
        assert_eq!(
            with_store(instance2, |store| store.unigram_total()).unwrap(),
            counts.3
        );
    }
    assert!(
        !pinyin_save(context2),
        "the reopened store saves as a no-op"
    );

    crate::instance::pinyin_free_instance(instance2);
    crate::context::pinyin_fini(context2);
}

// The deallocator for buffers the export iterators hand out (GLib and the
// conversion encoder allocate with libc `malloc`, which `g_free` and `free`
// both release on Linux).
unsafe extern "C" {
    fn free(ptr: *mut std::ffi::c_void);
}

/// Frees a caller-owned buffer the export iterators hand out.
fn free_exported(ptr: *mut std::ffi::c_char) {
    if !ptr.is_null() {
        // SAFETY: the iterator's buffers come from libc `malloc`, directly
        // or through GLib; `free` releases them.
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
            &raw mut phrase,
            &raw mut pinyin,
            &raw mut count,
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
        let more = crate::iterators::pinyin_bigram_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut pinyin,
            &raw mut count,
        );
        rows.push((take_exported(phrase), take_exported(pinyin), count));
        // Register row 36: `get_next` answers `has_next` after the row
        // (`pinyin.cpp:896-911`), so `false` on the last one.
        assert_eq!(
            more,
            crate::iterators::pinyin_bigram_iterator_has_next_phrase(iter)
        );
    }
    rows
}

/// The open counter `user.conf` records, as the next init reads it.
fn recorded_open_counter(user_dir: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(user_dir.join("user.conf"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("open counter:"))
        .and_then(|value| value.trim().parse().ok())
}

#[test]
fn two_contexts_on_one_user_dir_are_independent() {
    // #538: a second pinyin_init on a user dir another live context has
    // open is a context of its own, as upstream's is — no state is held
    // per directory. Each init raises the open counter (pinyin.cpp:185-187),
    // B does not see what A learned and has not saved, each save answers
    // for its own context's modifications, and each fini lowers its own
    // copy of the counter (:1194-1200).
    let user_dir = TempUserDir::new("two-contexts");
    let dir = user_dir.path.to_str().expect("UTF-8 path");
    let (ctx_a, inst_a) = open(dir);
    let (ctx_b, inst_b) = open(dir);
    assert_eq!(recorded_open_counter(&user_dir.path), Some(2));

    let iter = pinyin_begin_add_phrases(ctx_a, 7);
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("你好").as_ptr(),
        cstr("nihao").as_ptr(),
        5,
    ));
    pinyin_end_add_phrases(iter);
    let export = pinyin_begin_get_phrases(ctx_b, 7);
    assert!(
        drain_phrases(export).is_empty(),
        "B sees A's unsaved import"
    );
    pinyin_end_get_phrases(export);

    assert!(!pinyin_save(ctx_b), "B changed nothing");
    assert!(pinyin_save(ctx_a));

    crate::instance::pinyin_free_instance(inst_a);
    crate::instance::pinyin_free_instance(inst_b);
    crate::context::pinyin_fini(ctx_a);
    assert_eq!(recorded_open_counter(&user_dir.path), Some(0));
    crate::context::pinyin_fini(ctx_b);
    assert_eq!(recorded_open_counter(&user_dir.path), Some(1));
}

#[test]
fn import_iterators_add_per_phrase_and_arm_modified_at_end() {
    let user_dir = TempUserDir::new("import");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // Null-safety first: add/end tolerate null exactly like the export
    // quartet. `begin` with a NULL context is covered by
    // `null_context_import_handle_is_returned_and_freed`: the pin returns an
    // iterator there (`pinyin.cpp:506-512`).
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
    assert!(!with_store(instance, |store| store.is_modified()));

    pinyin_end_add_phrases(iter);

    // End armed the shared dirty flag; the next save compacts and clears it.
    assert!(with_store(instance, |store| store.is_modified()));
    assert!(pinyin_save(context));
    assert!(!with_store(instance, |store| store.is_modified()));

    let export = pinyin_begin_get_phrases(context, 7);
    assert_eq!(
        drain_phrases(export),
        vec![
            ("你好".to_owned(), "ni'hao".to_owned(), 12),
            ("世界".to_owned(), "shi'jie".to_owned(), 3),
        ]
    );
    pinyin_end_get_phrases(export);

    // A loaded system index accepts imports through its sparse overlay
    // (074a2219 pinyin.cpp:585-608).
    let system = pinyin_begin_add_phrases(context, 1);
    assert!(!system.is_null());
    assert!(pinyin_iterator_add_phrase(
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
    assert!(with_store(instance, |store| store.is_modified()));
    assert!(pinyin_save(context));
    assert!(!with_store(instance, |store| store.is_modified()));

    let export = pinyin_begin_get_phrases(context, 7);
    assert!(!pinyin_iterator_has_next_phrase(export));
    pinyin_end_get_phrases(export);

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// Trains one multi-phrase sentence through choose/guess/train: choosing 你
/// consumes the ni group, the 好 group is guessed at the post-choose cursor
/// (never offset 0), and the (你 → 好) bigram lands at 69.
fn train_one_multiphrase_sentence(instance: *mut PinyinInstance) {
    // Train one multi-phrase sentence: (你 → 好) = 69.
    let _ = candidate(instance, "nihao", 0);
    let (_, ni_ptr) = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|c| c.text_bytes() == "你".as_bytes())
            .expect("你 is offered for ni");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &raw mut cand
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
        usize::try_from(ni_cursor).expect("non-negative cursor"),
        DEFAULT_SORT
    ));
    let hao_ptr = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|c| c.text_bytes() == "好".as_bytes())
            .expect("好 is offered for hao");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &raw mut cand
        ));
        cand
    };
    assert!(pinyin_choose_candidate(instance, 2, hao_ptr) > 0);
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_train(instance, 0));
}

/// Register row 62: a `USER_FILE` reading the probe counts
/// (`pinyin.cpp:682`) but `get_pinyin_string` cannot spell (`:728` →
/// `chewing_key.cpp:48`) is read, not skipped, and the pin's assert aborts
/// the process. No recorded contract diff harness fixture can plant the reading in the
/// pin's sub-index, so this death test holds the site.
// Unix only: the out-params live in a page shared through `mmap(MAP_SHARED)` (`assert_aborts_sharing`).
#[cfg(unix)]
#[test]
fn export_iterator_aborts_on_a_reading_it_cannot_spell() {
    // The out-params live in a page shared with the child: `pinyin.cpp:703`
    // writes NULL, NULL and -1 before the assert, over these sentinels.
    let slot = std::mem::size_of::<usize>();
    let page = crate::test_support::assert_aborts_sharing(
        "e2e_tests::export_iterator_aborts_on_a_reading_it_cannot_spell",
        &vec![0xAB; 2 * slot + 4],
        export_an_unspellable_reading,
    );
    assert!(
        page[..2 * slot].iter().all(|&byte| byte == 0),
        "NULL phrase and pinyin"
    );
    assert_eq!(
        std::os::raw::c_int::from_ne_bytes(
            page[2 * slot..2 * slot + 4].try_into().expect("4 bytes")
        ),
        -1
    );
}

/// Exports the user dictionary after planting a reading one tone past
/// `MAX_KEY_TONE` behind a spellable one; the aborting read writes its
/// out-params into `page` (phrase, pinyin, then count).
// Unix only: the out-params live in a page shared through `mmap(MAP_SHARED)` (`assert_aborts_sharing`).
#[cfg(unix)]
fn export_an_unspellable_reading(page: *mut u8) {
    let user_dir = TempUserDir::new("export-unspellable");
    let (context, _instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    {
        // SAFETY: `context` is a live `pinyin_init` handle, borrowed
        // uniquely for this block.
        let ctx = unsafe { crate::state::context_mut(context) };
        let store = ctx.core.user_store_mut().expect("the context owns a store");
        let syllable = |text: &str| {
            oxpinyin_core::SyllableKey::from_text(text)
                .expect("syllable")
                .index()
        };
        let ni = oxpinyin_user::toned_key(syllable("ni"), 0).expect("key");
        store
            .add_phrase_in(USER_DICTIONARY, &"你".into(), &[ni], Some(5))
            .expect("a reading the engine spells");
        // One past `MAX_KEY_TONE`: the add checks lengths only.
        let past_the_tones = oxpinyin_user::toned_key(syllable("hao"), oxpinyin_user::MAX_KEY_TONE)
            .expect("key")
            + (1 << oxpinyin_user::KEY_TONE_SHIFT);
        store
            .add_phrase_in(USER_DICTIONARY, &"好".into(), &[past_the_tones], Some(3))
            .expect("stored unchecked");
    }
    let iter = crate::iterators::pinyin_begin_get_phrases(context, 7);
    assert!(!iter.is_null());
    let mut phrase: *mut crate::types::GChar = ptr::null_mut();
    let mut pinyin: *mut crate::types::GChar = ptr::null_mut();
    let mut count: std::os::raw::c_int = 0;
    assert!(crate::iterators::pinyin_iterator_get_next_phrase(
        iter,
        &raw mut phrase,
        &raw mut pinyin,
        &raw mut count,
    ));
    assert_eq!(
        (take_exported(phrase), take_exported(pinyin), count),
        ("你".to_owned(), "ni".to_owned(), 5)
    );
    // The probe selected the second item: it has a pronunciation.
    assert!(crate::iterators::pinyin_iterator_has_next_phrase(iter));
    // The read of the second item aborts.
    let slot = std::mem::size_of::<usize>();
    // SAFETY: the page holds two pointer slots then a `c_int`, each aligned
    // (the mapping is page-aligned).
    let (phrase_out, pinyin_out, count_out) = unsafe {
        (
            page.cast::<*mut crate::types::GChar>(),
            page.add(slot).cast::<*mut crate::types::GChar>(),
            page.add(2 * slot).cast::<std::os::raw::c_int>(),
        )
    };
    crate::iterators::pinyin_iterator_get_next_phrase(iter, phrase_out, pinyin_out, count_out);
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
            .position(|c| c.text_bytes() == "\u{4f60}\u{597d}".as_bytes())
            .expect("\u{4f60}\u{597d} is offered");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &raw mut cand
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
            inst.state
                .candidates
                .iter()
                .any(|c| c.text_bytes() == "\u{4f60}".as_bytes()),
            "the re-opened composition offers \u{4f60} at offset 0"
        );
    }

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn predicted_tie_groups_are_text_ascending_including_user_rows() {
    use crate::sentence::pinyin_guess_predicted_candidates_with_punctuations;
    use crate::types::lookup_candidate_type_t;

    /// Mirror of the engine's `amplified_frequency` at the pinned λ, so
    /// the expected order is computed without the production code.
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
    let total = inst.core.lm.amplified_total();
    let rows: Vec<(String, usize, u64)> = inst
        .candidates
        .iter()
        .filter(|c| c.candidate_type() == lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE)
        .map(|c| {
            let baked = c.token.map_or(0, |token| {
                inst.core
                    .dict
                    .live_items()
                    .effective_unigram(token.value())
                    .unwrap_or(0)
                    + inst
                        .core
                        .user
                        .as_ref()
                        .and_then(|store| store.unigram_delta(token.value()).ok())
                        .unwrap_or(0)
            });
            (
                c.text_str().expect("candidate text is UTF-8").to_owned(),
                c.text_str()
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
    // (both user rows, equal live count 21) and must appear 华-first.
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

/// Register row 37 for the transformed input schemes (maintainer ruling
/// 2026-10-03): the caller's offsets are in the ORIGINAL input's
/// coordinates and the pin builds every window from `start = offset`
/// (`pinyin.cpp:2224-2262`), so under double pinyin a lookup at 0 behind a
/// chosen 你好 shows the offset-0 window (a lookup that used the cached,
/// composition-anchored list showed none of it), and choosing 你 from it
/// answers the span's end in original coordinates, `0 + len = 2`.
#[test]
fn choosing_behind_the_composition_under_double_pinyin_answers_the_chosen_span() {
    fn row(instance: *mut PinyinInstance, text: &str) -> *mut LookupCandidate {
        // SAFETY: live instance immediately after a guess.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|cd| cd.text_bytes() == text.as_bytes())
            .unwrap_or_else(|| panic!("{text} is offered"));
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &raw mut cand
        ));
        cand
    }
    let user_dir = TempUserDir::new("row37-double-behind-choose");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // Microsoft double pinyin (scheme 2): n+i, then h+ao (`k`).
    assert!(crate::config::pinyin_set_double_pinyin_scheme(context, 2));
    let keys = cstr("nihk");
    assert_eq!(
        crate::parse::pinyin_parse_more_double_pinyins(instance, keys.as_ptr()),
        4
    );
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    assert_eq!(
        pinyin_choose_candidate(instance, 0, row(instance, "你好")),
        4,
        "the chosen phrase consumes both keys, in original coordinates"
    );
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    assert_eq!(
        pinyin_choose_candidate(instance, 0, row(instance, "你")),
        2,
        "the pin's offset + len for the key-wide choose behind the composition"
    );
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// A lookup strictly inside a double-pinyin key answers the sentence rows
/// alone: the pin's matrix holds the key only on the column its key rest
/// begins on, so a mid-key `pinyin_guess_candidates` searches an empty
/// column (`pinyin.cpp:2224-2262`; the zhuyin facade's #577 law). Offset 0
/// and offset 2 are key starts and list phrases.
#[test]
fn a_mid_key_lookup_under_double_pinyin_lists_only_the_sentence_rows() {
    let user_dir = TempUserDir::new("double-mid-key");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(crate::config::pinyin_set_double_pinyin_scheme(context, 2));
    let keys = cstr("nihk");
    assert_eq!(
        crate::parse::pinyin_parse_more_double_pinyins(instance, keys.as_ptr()),
        4
    );
    assert!(pinyin_guess_sentence(instance));
    let kinds = |instance: *mut PinyinInstance| -> Vec<lookup_candidate_type_t> {
        // SAFETY: live instance immediately after a guess.
        let inst = unsafe { instance_ref(instance) };
        inst.state
            .candidates
            .iter()
            .map(|cd| cd.candidate_type())
            .collect()
    };
    for offset in [0, 2] {
        assert!(pinyin_guess_candidates(instance, offset, DEFAULT_SORT));
        assert!(
            kinds(instance).contains(&lookup_candidate_type_t::NORMAL_CANDIDATE),
            "offset {offset} is a key start and lists phrases"
        );
    }
    for offset in [1, 3] {
        assert!(pinyin_guess_candidates(instance, offset, DEFAULT_SORT));
        let kinds = kinds(instance);
        assert!(!kinds.is_empty(), "the sentence rows are listed");
        assert!(
            kinds
                .iter()
                .all(|kind| *kind == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE),
            "offset {offset} is mid-key: sentence rows only"
        );
    }
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[rstest::rstest]
#[case::unchanged(None, 5, false)]
#[case::longer(Some("nihaoma"), 7, false)]
#[case::shorter(Some("ni"), 2, true)]
#[case::empty(Some(""), 0, true)]
#[case::stale_constraint(Some("ma"), 2, true)]
#[case::unconsumed_suffix(Some("ni!hao"), 2, true)]
#[case::zero_keys(Some("''''''''"), 8, true)]
fn choosing_from_a_reanchored_window_uses_the_anchored_span(
    #[case] reparse: Option<&str>,
    #[case] consumed: usize,
    #[case] refused: bool,
) {
    let user_dir = TempUserDir::new("c2-reanchor-choose");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // The composition is anchored at 0 after a parse. Guessing at offset 2
    // BEFORE any choose re-anchors the window there (no cached list at 2),
    // so the rows the caller sees come from the offset-2 window.
    let _ = candidate(instance, "nihao", 0);
    if reparse == Some("ma") {
        let mut original = ptr::null_mut();
        let index = {
            // SAFETY: the guess completed; the instance remains live.
            let inst = unsafe { instance_ref(instance) };
            inst.candidates
                .iter()
                .position(|row| row.text_bytes() == "你".as_bytes())
                .expect("你 candidate")
        };
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut original
        ));
        assert_eq!(pinyin_choose_candidate(instance, 0, original), 2);
        // This forcing becomes invalid under "ma". Even a refused choose
        // must validate it away (phonetic_lookup.cpp:120-168 at 074a2219).
    }
    assert!(pinyin_guess_candidates(instance, 2, DEFAULT_SORT));
    let hao = {
        // SAFETY: live instance immediately after the guess.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|cd| cd.text_bytes() == "好".as_bytes())
            .expect("好 is offered at offset 2");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &raw mut cand
        ));
        cand
    };
    let expected_token = token_of(instance, hao);
    let expected_tokens = [PhraseToken::new(expected_token)];
    if let Some(input) = reparse {
        // At 074a2219 pinyin.cpp:1497-1525 leaves the candidate intact;
        // :2580-2587 chooses its saved token and span, not a new row index.
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, cstr(input).as_ptr()),
            consumed
        );
    }
    // The chosen row's span is anchor-relative (hao covers raw 2..5). With
    // the fix the composition advances from the ANCHOR (2 + 3 = 5);
    // resolving the index against the composition-anchored cached list
    // instead would select a different row and answer a different cursor.
    // add_constraint refuses end > matrix.size(); Pinyin returns offset + 0
    // (074a2219 phonetic_lookup.cpp:61-65; pinyin.cpp:2581-2589).
    assert_eq!(
        pinyin_choose_candidate(instance, 2, hao),
        if refused { 2 } else { 5 }
    );
    assert_eq!(
        // SAFETY: the choose completed and the instance is still live.
        unsafe { instance_ref(instance) }
            .core
            .session
            .selected_tokens(),
        if refused {
            &[][..]
        } else {
            &expected_tokens[..]
        }
    );
    assert!(!crate::candidates::pinyin_clear_constraint(instance, 0));
    if refused {
        assert_eq!(pinyin_choose_candidate(instance, 2, hao), 2);
        assert!(!crate::candidates::pinyin_clear_constraint(instance, 2));
        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
        return;
    }

    // The skipped prefix (the raw bytes [0, 2) before the anchor) is
    // retained in the committed text: a re-anchored selection must not
    // drop the typed-but-unselected bytes (the gap
    // `rebuild_selection_from_constraints` preserves).
    let committed = {
        // SAFETY: the instance is live; this reads the retained selection.
        unsafe {
            crate::state::instance_ref(instance)
                .core
                .session
                .selected_text()
                .clone()
        }
    };
    assert_eq!(
        committed, "ni好",
        "the typed prefix survives the re-anchored choose"
    );
    assert!(crate::candidates::pinyin_clear_constraint(instance, 2));
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
    assert!(pinyin_get_sentence(instance, 0, &raw mut sentence));
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
    inst.state
        .candidates
        .iter()
        .filter_map(|c| c.token.map(oxpinyin_core::PhraseToken::value))
        .filter(|token| (token >> 24) == 2)
        .collect()
}

/// Parse-termination gates: C1 (`FORCE_TONE`) and B2 (stop bytes), plus the
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
        assert!(pinyin_get_n_candidate(instance, &raw mut n));
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
        // Apostrophe-only parses hold no keys but are NOT empty parses: no
        // candidate and never the engine's raw-input fallback row. The
        // sentence guess answers `false` there: `get_nbest_match` returns
        // before it clears anything when the matrix has no step
        // (`phonetic_lookup.h:743-745`; measured at 074a2219 on bdb for one,
        // two and three apostrophes under the default and the parity word).
        assert!(!pinyin_guess_sentence(instance));
        // `pinyin_guess_candidates` frees the list and answers `false` for
        // the same step-less matrix (`pinyin.cpp:2193-2196`).
        assert!(!pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let mut n = 0;
        assert!(pinyin_get_n_candidate(instance, &raw mut n));
        assert_eq!(
            n, 0,
            "no candidates and no fallback row for a keyless parse"
        );

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}

/// The chewing batch seam's option forwarding — row 30 of the
/// compatibility policy: `pinyin_parse_more_chewings` passes the whole
/// option word through after the `ZHUYIN_CORRECT_ALL` strip
/// (`pinyin.cpp:1582-1609` at the pin), so a caller-set `FORCE_TONE`
/// reaches the zhuyin parser's per-keyboard law.
#[cfg(test)]
mod chewing_batch_force_tone {
    use crate::config::pinyin_set_options;
    use crate::parse::pinyin_parse_more_chewings;
    use crate::test_support::{TempUserDir, cstr, open};

    const PARITY: u32 = 0x18a;
    const USE_TONE: u32 = 0x20;
    const FORCE_TONE: u32 = 0x40;

    fn chewing_len(
        context: *mut crate::types::PinyinContext,
        instance: *mut crate::types::PinyinInstance,
        word: u32,
        input: &str,
    ) -> usize {
        assert!(pinyin_set_options(context, word));
        let text = cstr(input);
        pinyin_parse_more_chewings(instance, text.as_ptr())
    }

    #[test]
    fn force_tone_crosses_the_pinyin_facade_seam() {
        // The register's measured row-30 shape: under USE_TONE|FORCE_TONE
        // the pin consumes 0 on toneless `su` (STANDARD ㄋㄧ) where the
        // seam-deficient build consumed 2; a toned syllable parses.
        let user_dir = TempUserDir::new("row30-force");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        let word = PARITY | USE_TONE | FORCE_TONE;

        assert_eq!(
            chewing_len(context, instance, word, "su"),
            0,
            "toneless syllable refuses under FORCE_TONE"
        );
        assert_eq!(
            chewing_len(context, instance, word, "su3"),
            3,
            "toned syllable parses under FORCE_TONE"
        );

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

    #[test]
    fn the_seam_strips_only_the_zhuyin_corrections() {
        // Without FORCE_TONE the forwarded word parses exactly as the
        // three-argument law did — the strip changes nothing a caller
        // can observe on the STANDARD keyboard (its corrections are the
        // parser's own, re-or'd inside `parse_detailed`), and
        // FORCE_TONE without USE_TONE stays inert (the nested check).
        let user_dir = TempUserDir::new("row30-strip");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

        assert_eq!(chewing_len(context, instance, PARITY | USE_TONE, "su"), 2);
        assert_eq!(chewing_len(context, instance, PARITY | USE_TONE, "su3"), 3);
        assert_eq!(
            chewing_len(context, instance, PARITY | FORCE_TONE, "su"),
            2,
            "FORCE_TONE is nested inside USE_TONE"
        );

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}

#[test]
fn masking_system_imports_restores_originals_and_removes_appended_items() {
    let user_dir = TempUserDir::new("mask-system-imports");
    let dir = user_dir.path.to_str().expect("UTF-8 path");
    let (context, instance) = open(dir);
    let export = pinyin_begin_get_phrases(context, 1);
    let original = drain_phrases(export);
    pinyin_end_get_phrases(export);
    assert!(!original.is_empty());
    let import = pinyin_begin_add_phrases(context, 1);
    assert!(pinyin_iterator_add_phrase(
        import,
        cstr(&original[0].0).as_ptr(),
        cstr(&original[0].1).as_ptr(),
        5
    ));
    assert!(pinyin_iterator_add_phrase(
        import,
        cstr("你你你").as_ptr(),
        cstr("ni'ni'ni").as_ptr(),
        5
    ));
    pinyin_end_add_phrases(import);
    let export = pinyin_begin_get_phrases(context, 1);
    let modified = drain_phrases(export);
    pinyin_end_get_phrases(export);
    assert_ne!(modified, original);
    assert!(modified.iter().any(|row| row.0 == "你你你"));
    assert!(crate::config::pinyin_mask_out(context, 0, 0));
    let export = pinyin_begin_get_phrases(context, 1);
    assert_eq!(drain_phrases(export), original);
    pinyin_end_get_phrases(export);
    assert!(pinyin_save(context));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
    let (context, instance) = open(dir);
    let export = pinyin_begin_get_phrases(context, 1);
    assert_eq!(drain_phrases(export), original);
    pinyin_end_get_phrases(export);
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// Register rows 63 and 66: on a fresh instance the pin's
/// `offset >= matrix.size() - 1` is `offset >= SIZE_MAX`
/// (`pinyin.cpp:2927-2928`, `:2950-2951`), so `SIZE_MAX` answers `false`
/// with the out-param NULLed before `get_column_size` could assert.
#[test]
fn the_key_getters_answer_false_at_size_max_on_an_empty_matrix() {
    let _slots = crate::test_support::key_slot_guard();
    let user_dir = TempUserDir::new("key-size-max");
    let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let mut key = ptr::NonNull::<crate::types::ChewingKey>::dangling().as_ptr();
    assert!(!crate::cursor::pinyin_get_pinyin_key(
        instance,
        usize::MAX,
        &raw mut key
    ));
    assert!(key.is_null());
    let mut rest = ptr::NonNull::<crate::types::ChewingKeyRest>::dangling().as_ptr();
    assert!(!crate::cursor::pinyin_get_pinyin_key_rest(
        instance,
        usize::MAX,
        &raw mut rest
    ));
    assert!(rest.is_null());
}

/// Imports 你好 into libraries `1..=libraries` of the fixture, which loads
/// libraries 1 to 7, so the phrase-table search holds `libraries` tokens.
fn hold_nihao_in_libraries(context: *mut crate::types::PinyinContext, libraries: u8) {
    for library in 1..=libraries {
        let iter = pinyin_begin_add_phrases(context, library);
        assert!(pinyin_iterator_add_phrase(
            iter,
            cstr("你好").as_ptr(),
            cstr("ni3'hao3").as_ptr(),
            1,
        ));
        pinyin_end_add_phrases(iter);
    }
}

/// `reduce_tokens` clears and fills the caller's array before it validates
/// the count (`phrase_large_table3.h:79-95`). On a NULL array each GLib
/// call logs a critical: one for `g_array_set_size`, one
/// `g_array_append_vals` per loaded library (`phrase_index.h:762-772`).
/// The pin then dies of the assert on five tokens (`pinyin.cpp:2664`), so
/// the criticals come first. GLib's default handler writes them to stderr.
#[test]
fn lookup_tokens_into_a_null_array_logs_the_criticals_before_it_aborts() {
    let user_dir = TempUserDir::new("lookup-null-five");
    let path = user_dir.path.to_str().expect("UTF-8 path").to_owned();
    let stderr = crate::test_support::assert_aborts(
        "e2e_tests::lookup_tokens_into_a_null_array_logs_the_criticals_before_it_aborts",
        || {
            // SAFETY: GLib's own default handler, installed for every domain.
            unsafe {
                glib_sys::g_log_set_default_handler(
                    Some(glib_sys::g_log_default_handler),
                    ptr::null_mut(),
                );
            }
            let (context, instance) = open(&path);
            hold_nihao_in_libraries(context, 5);
            crate::dict::pinyin_lookup_tokens(instance, cstr("你好").as_ptr(), ptr::null_mut());
        },
    );
    let (context, instance) = open(&path);
    // SAFETY: `instance` is live until it is freed below, and the
    // reference is not used after that.
    let inst = unsafe { instance_ref(instance) };
    let loaded = (0..16_u32)
        .filter(|library| inst.core.dict.phrase_table_token_enabled(library << 24))
        .count();
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
    let stderr = String::from_utf8_lossy(&stderr);
    let criticals: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("CRITICAL"))
        .collect();
    assert!(loaded > 0);
    assert_eq!(criticals.len(), 1 + loaded, "{stderr}");
    assert!(criticals[0].contains("g_array_set_size"), "{stderr}");
    assert!(
        criticals[1..]
            .iter()
            .all(|line| line.contains("g_array_append_vals")),
        "{stderr}"
    );
}

/// Four tokens pass `reduce_tokens`' count, so the NULL-array lookup
/// answers instead of aborting.
#[test]
fn lookup_tokens_into_a_null_array_answers_with_four_tokens() {
    let user_dir = TempUserDir::new("lookup-null-four");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    hold_nihao_in_libraries(context, 4);
    assert!(crate::dict::pinyin_lookup_tokens(
        instance,
        cstr("你好").as_ptr(),
        ptr::null_mut()
    ));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// Opens `user_dir` over the fixture, answering the raw handle.
fn init_at(user_dir: &std::path::Path) -> *mut crate::types::PinyinContext {
    let system = cstr(system_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir.to_str().expect("UTF-8 path"));
    pinyin_init(system.as_ptr(), user.as_ptr())
}

/// Register row 91: init's library loop merges `gb_char.dbin` and
/// `next_record` asserts on its non-null `MODIFY_HEADER` token
/// (`phrase_index_logger.h:202`).
#[test]
fn init_aborts_on_a_log_header_token() {
    crate::test_support::assert_aborts("e2e_tests::init_aborts_on_a_log_header_token", || {
        let user_dir = TempUserDir::new("init-header-token");
        crate::test_support::plant_log(
            &user_dir.path,
            "gb_char.dbin",
            &crate::test_support::logs::header_token(),
        );
        let _ = init_at(&user_dir.path);
    });
}

/// Register row 91 before row 76: library 1's merge asserts before the loop
/// reaches library 4 (`pinyin.cpp:276` before `:256`), so a later chunk the
/// pin never opens cannot turn the abort into a NULL answer.
#[test]
fn init_aborts_on_a_log_before_an_unmappable_library() {
    crate::test_support::assert_aborts(
        "e2e_tests::init_aborts_on_a_log_before_an_unmappable_library",
        || {
            let scratch = tempfile::tempdir().expect("scratch");
            let system = crate::test_support::private_system(scratch.path());
            std::fs::remove_file(system.join("merged.bin")).expect("unlink the library");
            std::fs::write(system.join("merged.bin"), b"abc").expect("short library");
            let user = TempUserDir::new("init-log-before-unmappable");
            crate::test_support::plant_log(
                &user.path,
                "gb_char.dbin",
                &crate::test_support::logs::header_token(),
            );
            let system_c = cstr(system.to_str().expect("UTF-8 path"));
            let user_c = cstr(user.path.to_str().expect("UTF-8 path"));
            let _ = pinyin_init(system_c.as_ptr(), user_c.as_ptr());
        },
    );
}

/// Register row 108: init's merge stores a one-byte ADD payload and
/// `add_phrase_item` reads its unigram (`memory_chunk.h:390`).
#[test]
fn init_aborts_on_a_short_add_payload() {
    crate::test_support::assert_aborts("e2e_tests::init_aborts_on_a_short_add_payload", || {
        let user_dir = TempUserDir::new("init-short-add");
        crate::test_support::plant_log(
            &user_dir.path,
            "gb_char.dbin",
            &crate::test_support::logs::short_add(),
        );
        let _ = init_at(&user_dir.path);
    });
}

/// Register row 91's control: `merge` returns at a mismatched MODIFY before
/// `next_record` reads the bad header, and init completes.
#[test]
fn init_completes_when_the_merge_stops_before_a_bad_header() {
    let user_dir = TempUserDir::new("init-stop-before-header");
    let payload = [
        crate::test_support::logs::mismatched_modify(),
        crate::test_support::logs::header_token(),
    ]
    .concat();
    crate::test_support::plant_log(&user_dir.path, "gb_char.dbin", &payload);
    let context = init_at(&user_dir.path);
    assert!(!context.is_null());
    crate::context::pinyin_fini(context);
}

/// Register row 91 on the reload path: `pinyin_load_phrase_library` of the
/// unloaded GBK library merges its saved `.dbin` (`pinyin.cpp:276`).
#[test]
fn reload_aborts_on_a_log_header_token() {
    crate::test_support::assert_aborts("e2e_tests::reload_aborts_on_a_log_header_token", || {
        let user_dir = TempUserDir::new("reload-header-token");
        let context = init_at(&user_dir.path);
        assert!(!context.is_null());
        assert!(crate::config::pinyin_unload_phrase_library(context, 2));
        crate::test_support::plant_log(
            &user_dir.path,
            "gbk_char.dbin",
            &crate::test_support::logs::header_token(),
        );
        let _ = crate::config::pinyin_load_phrase_library(context, 2);
    });
}

/// Register row 97: `mask_out` reloads library 1 and `_mask_out_records`
/// appends a kept ADD with both chunk pointers (`phrase_index_logger.h:245`).
#[test]
fn mask_out_aborts_on_a_surviving_add() {
    crate::test_support::assert_aborts("e2e_tests::mask_out_aborts_on_a_surviving_add", || {
        let user_dir = TempUserDir::new("mask-surviving-add");
        let context = init_at(&user_dir.path);
        assert!(!context.is_null());
        let payload = [
            crate::test_support::logs::header(),
            crate::test_support::logs::add(),
        ]
        .concat();
        crate::test_support::plant_log(&user_dir.path, "gb_char.dbin", &payload);
        let _ = pinyin_mask_out(context, 0xffff_ffff, 0x0100_0001);
    });
}

/// Register row 85: `pinyin_mask_out` loads token 1's gram, which the mask
/// leaves alone, and `SingleGram::mask_out` reads its total from three bytes
/// (`ngram.cpp:80`, `memory_chunk.h:390`).
#[test]
fn mask_out_aborts_on_a_short_bigram_value() {
    crate::test_support::assert_aborts(
        "e2e_tests::mask_out_aborts_on_a_short_bigram_value",
        || {
            let user_dir = TempUserDir::new("mask-short-gram");
            crate::test_support::plant_bigram(
                &user_dir.path,
                vec![(1_u32.to_le_bytes().to_vec(), vec![7, 0, 0])],
            );
            let context = init_at(&user_dir.path);
            assert!(!context.is_null());
            let _ = pinyin_mask_out(context, 0xffff_ffff, 0);
        },
    );
}

/// Row 85's control: a key the mask selects is removed without loading its
/// value (`ngram_bdb.cpp:231-238`), and the mask completes.
#[test]
fn mask_out_of_a_selected_short_key_completes() {
    let user_dir = TempUserDir::new("mask-short-gram-selected");
    crate::test_support::plant_bigram(
        &user_dir.path,
        vec![(1_u32.to_le_bytes().to_vec(), vec![7, 0, 0])],
    );
    let context = init_at(&user_dir.path);
    assert!(!context.is_null());
    assert!(pinyin_mask_out(context, 0xffff_ffff, 1));
    crate::context::pinyin_fini(context);
}

/// Row 98 (class (b)) keeps its refusal: a partial item tail under a key the
/// mask leaves alone answers `false` (`ngram.cpp:76-86`).
#[test]
fn mask_out_refuses_a_partial_bigram_tail() {
    let user_dir = TempUserDir::new("mask-partial-tail");
    let mut value = 7_u32.to_le_bytes().to_vec();
    value.extend_from_slice(&[9, 0, 0, 0, 7, 0, 0, 0, 1]);
    crate::test_support::plant_bigram(&user_dir.path, vec![(1_u32.to_le_bytes().to_vec(), value)]);
    let context = init_at(&user_dir.path);
    assert!(!context.is_null());
    assert!(!pinyin_mask_out(context, 0xffff_ffff, 0));
    crate::context::pinyin_fini(context);
}

/// Register row 85 on the export walk: the pin's begin reads no gram
/// (`pinyin.cpp:775-787`), and `has_next` loads each predecessor and
/// `retrieve_all` reads its total (`:889`, `ngram.cpp:133-141`), so begin
/// answers an iterator and the walk aborts where it loads the short gram.
#[test]
fn bigram_export_aborts_where_the_walk_loads_a_short_predecessor() {
    crate::test_support::assert_aborts(
        "e2e_tests::bigram_export_aborts_where_the_walk_loads_a_short_predecessor",
        || {
            let user_dir = TempUserDir::new("export-short-gram");
            crate::test_support::plant_bigram(
                &user_dir.path,
                vec![(1_u32.to_le_bytes().to_vec(), vec![7, 0, 0])],
            );
            let context = init_at(&user_dir.path);
            assert!(!context.is_null());
            let iter = crate::iterators::pinyin_begin_get_bigram_phrases(context);
            assert!(!iter.is_null(), "begin reads no gram");
            let _ = crate::iterators::pinyin_bigram_iterator_has_next_phrase(iter);
        },
    );
}

/// Register row 93 through `pinyin_remember_user_input`: the pin's
/// `_remember_phrase_recur` calls `_add_phrase` for USER_DICTIONARY
/// (`pinyin.cpp:3702`), so a phrase table whose row for the phrase holds two
/// library-7 tokens trips `assert(PHRASE_INDEX_LIBRARY_INDEX(token) != index)`
/// (`:554`) there as it does on the import path.
#[test]
fn remember_user_input_aborts_on_two_same_library_tokens() {
    crate::test_support::assert_aborts(
        "e2e_tests::remember_user_input_aborts_on_two_same_library_tokens",
        || {
            let user_dir = TempUserDir::new("remember-two-tokens");
            crate::test_support::set_phrase_table_row(
                &user_dir.path,
                "你好",
                &[0x0700_0001, 0x0700_0002],
            );
            let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
            assert_eq!(
                pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()),
                5
            );
            let _ = pinyin_remember_user_input(instance, cstr("你好").as_ptr(), -1);
        },
    );
}

/// As above, for `:568`: the lone library-7 token names a stored phrase of
/// another length (`你好` -> the three-character `你好嗷`).
#[test]
fn remember_user_input_aborts_on_an_item_length_mismatch() {
    crate::test_support::assert_aborts(
        "e2e_tests::remember_user_input_aborts_on_an_item_length_mismatch",
        || {
            let user_dir = TempUserDir::new("remember-length");
            remember_against_a_redirected_row(&user_dir, "你好嗷", "ni'hao'ao");
        },
    );
}

/// As above, for `:571`: the lone library-7 token names another phrase of the
/// same length (`你好` -> `你們`).
#[test]
fn remember_user_input_aborts_on_an_item_text_mismatch() {
    crate::test_support::assert_aborts(
        "e2e_tests::remember_user_input_aborts_on_an_item_text_mismatch",
        || {
            let user_dir = TempUserDir::new("remember-text");
            remember_against_a_redirected_row(&user_dir, "你們", "ni'men");
        },
    );
}

/// `_remember_phrase_recur` computes `end = matrix.size() - 1`, which wraps
/// for an empty matrix, and then asserts in `get_column_size(0)`
/// (`phonetic_key_matrix.h:102`; register row 66). A fresh instance holds
/// one.
#[test]
fn remember_user_input_aborts_on_a_fresh_instance() {
    crate::test_support::assert_aborts(
        "e2e_tests::remember_user_input_aborts_on_a_fresh_instance",
        || {
            let user_dir = TempUserDir::new("remember-fresh");
            let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
            let _ = pinyin_remember_user_input(instance, cstr("你好").as_ptr(), -1);
        },
    );
}

/// As above, for a parse that consumed bytes but placed no key (three
/// apostrophes): `fill_matrix` leaves the matrix empty.
#[test]
fn remember_user_input_aborts_on_a_keyless_parse() {
    crate::test_support::assert_aborts(
        "e2e_tests::remember_user_input_aborts_on_a_keyless_parse",
        || {
            let user_dir = TempUserDir::new("remember-keyless");
            let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
            assert_eq!(
                pinyin_parse_more_full_pinyins(instance, cstr("'''").as_ptr()),
                3
            );
            let _ = pinyin_remember_user_input(instance, cstr("你好").as_ptr(), -1);
        },
    );
}

/// The empty-matrix abort follows the token computation: a character with
/// no token refuses first (`pinyin.cpp:3685-3695`).
#[test]
fn remember_user_input_refuses_an_unknown_character_before_the_empty_matrix() {
    let user_dir = TempUserDir::new("remember-unknown-empty");
    let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(!pinyin_remember_user_input(
        instance,
        cstr("😀").as_ptr(),
        -1
    ));
}

/// The preflight runs only where `_remember_phrase_recur` reaches
/// `_add_phrase`. With the crafted 你好 row and only `ni` parsed, the one
/// path has one key for a two-character phrase and is refused at
/// `pinyin.cpp:3600-3601`, so the call answers `false`.
#[test]
fn remember_user_input_refuses_a_short_path_before_the_preflight() {
    let user_dir = TempUserDir::new("remember-short-path");
    crate::test_support::set_phrase_table_row(&user_dir.path, "你好", &[0x0700_0001, 0x0700_0002]);
    let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("ni").as_ptr()),
        2
    );
    assert!(!pinyin_remember_user_input(
        instance,
        cstr("你好").as_ptr(),
        -1
    ));
}

/// A key the next character's item does not pronounce ends its branch
/// (`pinyin.cpp:3640-3645`): 好 does not pronounce `ni`, so 好你 reaches no
/// path under `nihao`, whatever its crafted row holds.
#[test]
fn remember_user_input_refuses_a_path_the_item_does_not_pronounce() {
    let user_dir = TempUserDir::new("remember-unpronounced");
    crate::test_support::set_phrase_table_row(&user_dir.path, "好你", &[0x0700_0001, 0x0700_0002]);
    let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()),
        5
    );
    assert!(!pinyin_remember_user_input(
        instance,
        cstr("好你").as_ptr(),
        -1
    ));
}

/// Imports `phrase` into library 7 once per reading and returns its token.
fn import_readings(
    context: *mut crate::types::PinyinContext,
    instance: *mut PinyinInstance,
    phrase: &str,
    readings: &[&str],
) -> u32 {
    let import = pinyin_begin_add_phrases(context, 7);
    for reading in readings {
        assert!(pinyin_iterator_add_phrase(
            import,
            cstr(phrase).as_ptr(),
            cstr(reading).as_ptr(),
            1
        ));
    }
    pinyin_end_add_phrases(import);
    with_store(instance, |store| store.token_for_phrase(phrase))
        .expect("readable store")
        .expect("the import gave the phrase a token")
}

/// The stored readings of `token` as `(packed first key, frequency)`.
fn stored_readings(instance: *mut PinyinInstance, token: u32) -> Vec<(u16, u32)> {
    with_store(instance, |store| store.item_pronunciations(token))
        .expect("readable store")
        .expect("an item")
        .iter()
        .map(|(keys, freq)| (keys[0].to_packed(), *freq))
        .collect()
}

/// The remember walk reads the matrix the parse built. Double pinyin keeps
/// the parsed tone (`ni3` is the reading `ni3`), and a different tone finds no
/// path.
#[test]
fn remember_user_input_keeps_the_double_pinyin_tone() {
    use crate::config::pinyin_set_double_pinyin_scheme;
    use crate::parse::pinyin_parse_more_double_pinyins;

    let ni3 = oxpinyin_core::ChewingKey::from_pinyin("ni")
        .expect("ni is a key")
        .with_tone(3)
        .to_packed();
    for (input, remembered) in [("ni3", true), ("ni2", false)] {
        let user_dir = TempUserDir::new("remember-double-tone");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        let token = import_readings(context, instance, "农", &["ni3"]);
        assert!(pinyin_set_options(context, 0x28));
        assert!(pinyin_set_double_pinyin_scheme(context, 2));
        assert_eq!(
            pinyin_parse_more_double_pinyins(instance, cstr(input).as_ptr()),
            input.len()
        );
        assert_eq!(
            pinyin_remember_user_input(instance, cstr("农").as_ptr(), 3),
            remembered,
            "{input}"
        );
        let expected = if remembered { 1 + 3 } else { 1 };
        assert_eq!(
            stored_readings(instance, token),
            [(ni3, expected)],
            "{input}"
        );
    }
}

/// 弄 with its `nong` and `long` readings, parsed as `ns` in Microsoft
/// double pinyin under `L_N`. Returns the readings before any remember.
fn parsed_nong_under_l_n(
    user_dir: &TempUserDir,
) -> (
    *mut crate::types::PinyinContext,
    *mut PinyinInstance,
    u32,
    Vec<(u16, u32)>,
) {
    use crate::config::pinyin_set_double_pinyin_scheme;
    use crate::parse::pinyin_parse_more_double_pinyins;

    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let token = import_readings(context, instance, "弄", &["nong", "long"]);
    let before = stored_readings(instance, token);
    assert_eq!(before.len(), 2);
    assert!(pinyin_set_options(context, 0x8 | (1 << 15)));
    assert!(pinyin_set_double_pinyin_scheme(context, 2));
    assert_eq!(
        pinyin_parse_more_double_pinyins(instance, cstr("ns").as_ptr()),
        2
    );
    (context, instance, token, before)
}

/// Both readings' frequencies rose by the count of 3.
fn assert_both_readings_rose(instance: *mut PinyinInstance, token: u32, before: &[(u16, u32)]) {
    let after = stored_readings(instance, token);
    assert_eq!(after.len(), 2);
    for ((_, was), (_, now)) in before.iter().zip(&after) {
        assert_eq!(*now, *was + 3);
    }
}

/// The matrix comes from the parse, not from the options now: turning `L_N`
/// off after the parse leaves `long` in it.
#[test]
fn remember_user_input_keeps_the_parse_time_fuzzy_alternatives() {
    let user_dir = TempUserDir::new("remember-parse-time-fuzzy");
    let (context, instance, token, before) = parsed_nong_under_l_n(&user_dir);
    assert!(pinyin_set_options(context, 0x8));
    assert!(pinyin_remember_user_input(instance, cstr("弄").as_ptr(), 3));
    assert_both_readings_rose(instance, token, &before);
}

/// Persists `other` in library 7, points 你好's phrase-table row at its
/// token, reopens, and remembers 你好 under the composition `nihao`.
fn remember_against_a_redirected_row(user_dir: &TempUserDir, other: &str, reading: &str) {
    let path = user_dir.path.to_str().expect("UTF-8 path").to_owned();
    let (context, instance) = open(&path);
    let it = pinyin_begin_add_phrases(context, 7);
    assert!(pinyin_iterator_add_phrase(
        it,
        cstr(other).as_ptr(),
        cstr(reading).as_ptr(),
        1
    ));
    pinyin_end_add_phrases(it);
    let token = with_store(instance, |store| store.token_for_phrase(other))
        .expect("readable store")
        .expect("the setup phrase has a token");
    assert!(pinyin_save(context));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
    crate::test_support::set_phrase_table_row(&user_dir.path, "你好", &[token]);
    let (_context, instance) = open(&path);
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()),
        5
    );
    let _ = pinyin_remember_user_input(instance, cstr("你好").as_ptr(), -1);
}

/// Whether a directory reads as a chunk on this host's temporary file
/// system (register row 96); where it does not, a directory library is row
/// 76's unmappable one (class (b)).
#[cfg(unix)]
fn directory_reads_as_a_chunk_here() -> bool {
    let probe = tempfile::tempdir().expect("probe dir");
    std::fs::write(probe.path().join("e".repeat(25)), b"x").expect("fill the probe");
    oxpinyin_data::phrase_library::directory_reads_as_a_chunk(probe.path())
}

/// A context over a private system directory, with its user dir.
#[cfg(unix)]
fn init_private(
    scratch: &std::path::Path,
) -> (std::path::PathBuf, *mut crate::types::PinyinContext) {
    let system = crate::test_support::private_system(scratch);
    let user = scratch.join("user");
    std::fs::create_dir_all(&user).expect("user dir");
    let system_c = cstr(system.to_str().expect("UTF-8 path"));
    let user_c = cstr(user.to_str().expect("UTF-8 path"));
    (system, pinyin_init(system_c.as_ptr(), user_c.as_ptr()))
}

/// Register row 96 at init: the library loop maps `merged.bin`
/// (`pinyin.cpp:255`) and `MemoryChunk::mmap` asserts on the directory
/// (`memory_chunk.h:493`); where a directory does not read as a chunk the
/// init answers NULL (row 76).
// Unix only: a directory opens as a file, so the pin's `MemoryChunk::mmap` judges it by its
// size (`directory_reads_as_a_chunk`). Windows refuses the open, and oxpinyin skips the row
// without a word there (`oxpinyin-data`, `phrase_libraries.rs`), which no pin observation
// covers.
#[cfg(unix)]
#[test]
fn init_aborts_on_a_directory_library() {
    let body = || {
        let scratch = tempfile::tempdir().expect("scratch");
        let system = crate::test_support::private_system(scratch.path());
        crate::test_support::directory_library(&system, "merged.bin");
        let user = TempUserDir::new("init-directory-library");
        let system_c = cstr(system.to_str().expect("UTF-8 path"));
        let user_c = cstr(user.path.to_str().expect("UTF-8 path"));
        pinyin_init(system_c.as_ptr(), user_c.as_ptr())
    };
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts("e2e_tests::init_aborts_on_a_directory_library", || {
            let _ = body();
        });
    } else {
        assert!(body().is_null());
    }
}

/// One call that maps a system library again over a private system
/// directory whose library is a directory (register row 96): the save and
/// the mask-out map `merged.bin` (`pinyin.cpp:955`, `:1264`), the reload of
/// the unloaded GBK library maps `gbk_char.bin` (`:256`).
#[cfg(unix)]
fn remap_a_directory_library(site: &str) -> bool {
    let scratch = tempfile::tempdir().expect("scratch");
    let (system, context) = init_private(scratch.path());
    assert!(!context.is_null());
    match site {
        "save" => {
            // SAFETY: `context` is a live `pinyin_init` handle, borrowed
            // uniquely for this statement.
            let ctx = unsafe { crate::state::context_mut(context) };
            ctx.core.user_store_mut().expect("store").mark_modified();
            crate::test_support::directory_library(&system, "merged.bin");
            pinyin_save(context)
        }
        "mask" => {
            crate::test_support::directory_library(&system, "merged.bin");
            pinyin_mask_out(context, 0xff00_0000, 0x0400_0000)
        }
        "addon" => {
            crate::test_support::directory_library(&system, "art.bin");
            crate::config::pinyin_load_addon_phrase_library(context, 4)
        }
        _ => {
            assert!(crate::config::pinyin_unload_phrase_library(context, 2));
            crate::test_support::directory_library(&system, "gbk_char.bin");
            crate::config::pinyin_load_phrase_library(context, 2)
        }
    }
}

/// Register row 96 at save; row 76's completed save where a directory does
/// not read as a chunk.
// Unix only: a directory opens as a file, so the pin's `MemoryChunk::mmap` judges it by its
// size (`directory_reads_as_a_chunk`). Windows refuses the open, and oxpinyin skips the row
// without a word there (`oxpinyin-data`, `phrase_libraries.rs`), which no pin observation
// covers.
#[cfg(unix)]
#[test]
fn save_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts("e2e_tests::save_aborts_on_a_directory_library", || {
            let _ = remap_a_directory_library("save");
        });
    } else {
        assert!(remap_a_directory_library("save"));
    }
}

/// Register row 96 at mask-out, after the user tables are masked.
// Unix only, for the reason `init_aborts_on_a_directory_library` gives.
#[cfg(unix)]
#[test]
fn mask_out_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts(
            "e2e_tests::mask_out_aborts_on_a_directory_library",
            || {
                let _ = remap_a_directory_library("mask");
            },
        );
    } else {
        assert!(remap_a_directory_library("mask"));
    }
}

/// Register row 96 at the reload of an unloaded library.
// Unix only, for the reason `init_aborts_on_a_directory_library` gives.
#[cfg(unix)]
#[test]
fn reload_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts(
            "e2e_tests::reload_aborts_on_a_directory_library",
            || {
                let _ = remap_a_directory_library("reload");
            },
        );
    } else {
        assert!(remap_a_directory_library("reload"));
    }
}

/// Register row 96 at the addon load, which maps `art.bin` through the same
/// `MemoryChunk::mmap` (`pinyin.cpp:290`); row 76's `false` elsewhere.
// Unix only, for the reason `init_aborts_on_a_directory_library` gives.
#[cfg(unix)]
#[test]
fn addon_load_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts(
            "e2e_tests::addon_load_aborts_on_a_directory_library",
            || {
                let _ = remap_a_directory_library("addon");
            },
        );
    } else {
        assert!(!remap_a_directory_library("addon"));
    }
}

/// The predecessor tokens of the bigram export fixture, in the order the
/// walk loads them (the backend's key order), and the successor both name.
fn export_order_fixture() -> (u32, u32, u32) {
    let scratch = TempUserDir::new("export-order-probe");
    let (context, instance) = open(scratch.path.to_str().expect("UTF-8 path"));
    // The first three characters the fixture's dictionary holds.
    let found: Vec<(&str, u32)> = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        ["你", "好", "我", "的", "是", "中", "国", "人"]
            .into_iter()
            .filter_map(|text| {
                inst.core
                    .dict
                    .tokens_for_text(text)
                    .first()
                    .map(|token| (text, token.value()))
            })
            .take(3)
            .collect()
    };
    assert_eq!(found.len(), 3, "the fixture holds three of the characters");
    let ((ni_text, ni), (hao_text, hao), (_, wo)) = (found[0], found[1], found[2]);
    crate::context::pinyin_fini(context);
    let probe = TempUserDir::new("export-order");
    crate::test_support::plant_bigram(
        &probe.path,
        vec![
            (ni.to_le_bytes().to_vec(), whole_gram(hao)),
            (wo.to_le_bytes().to_vec(), whole_gram(hao)),
        ],
    );
    let context = init_at(&probe.path);
    let iter = crate::iterators::pinyin_begin_get_bigram_phrases(context);
    let mut rows = Vec::new();
    while crate::iterators::pinyin_bigram_iterator_has_next_phrase(iter) {
        let mut phrase: *mut crate::types::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::types::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        crate::iterators::pinyin_bigram_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut pinyin,
            &raw mut count,
        );
        rows.push((take_exported(phrase), take_exported(pinyin), count));
    }
    crate::iterators::pinyin_end_get_bigram_phrases(iter);
    crate::context::pinyin_fini(context);
    assert_eq!(rows.len(), 2, "{rows:?}");
    if rows[0].0 == format!("{ni_text}{hao_text}") {
        (ni, wo, hao)
    } else {
        (wo, ni, hao)
    }
}

/// A gram with one item to `successor` at count 100: above the export
/// threshold, so the walk hands out its row at count 200.
fn whole_gram(successor: u32) -> Vec<u8> {
    [
        100_u32.to_le_bytes(),
        successor.to_le_bytes(),
        100_u32.to_le_bytes(),
    ]
    .concat()
}

/// Register row 85 inside `get_next`: the pin writes the row's phrase,
/// pinyin and count (`pinyin.cpp:904-906`), advances, and only then runs the
/// `has_next` that loads the next predecessor and asserts on its short
/// total (`:910`, `:889`). The out-params live in a page shared with the
/// child, so the writes are read back after the abort.
// Unix only: the out-params live in a page shared through `mmap(MAP_SHARED)` (`assert_aborts_sharing`).
#[cfg(unix)]
#[test]
fn bigram_get_next_writes_its_row_before_the_short_predecessor_aborts() {
    let (first, second, successor) = export_order_fixture();
    let slot = std::mem::size_of::<usize>();
    let sentinel = vec![0xAB; 2 * slot + 4];
    let page = crate::test_support::assert_aborts_sharing(
        "e2e_tests::bigram_get_next_writes_its_row_before_the_short_predecessor_aborts",
        &sentinel,
        |page| {
            let user_dir = TempUserDir::new("export-row-then-short");
            crate::test_support::plant_bigram(
                &user_dir.path,
                vec![
                    (first.to_le_bytes().to_vec(), whole_gram(successor)),
                    (second.to_le_bytes().to_vec(), vec![7, 0, 0]),
                ],
            );
            let context = init_at(&user_dir.path);
            let iter = crate::iterators::pinyin_begin_get_bigram_phrases(context);
            assert!(crate::iterators::pinyin_bigram_iterator_has_next_phrase(
                iter
            ));
            // SAFETY: two pointer slots then a `c_int`, aligned in the page.
            let (phrase, pinyin, count) = unsafe {
                (
                    page.cast::<*mut crate::types::GChar>(),
                    page.add(slot).cast::<*mut crate::types::GChar>(),
                    page.add(2 * slot).cast::<std::os::raw::c_int>(),
                )
            };
            crate::iterators::pinyin_bigram_iterator_get_next_phrase(iter, phrase, pinyin, count);
        },
    );
    let pointer = |at: usize| &page[at..at + slot];
    assert!(
        pointer(0) != &sentinel[..slot] && pointer(0).iter().any(|&b| b != 0),
        "phrase written"
    );
    assert!(
        pointer(slot) != &sentinel[..slot] && pointer(slot).iter().any(|&b| b != 0),
        "pinyin written"
    );
    assert_eq!(
        std::os::raw::c_int::from_ne_bytes(
            page[2 * slot..2 * slot + 4].try_into().expect("4 bytes")
        ),
        200
    );
}

/// A partial item tail (`4 + 8k + 1` bytes): row 98's value, which the pin's
/// `retrieve_all` reads past (class (b)).
fn partial_gram(successor: u32) -> Vec<u8> {
    let mut gram = whole_gram(successor);
    gram.push(1);
    gram
}

/// Register rows 85 and 98 in walk order: the pin's begin reads no gram
/// (`pinyin.cpp:775-787`), so a short predecessor the walk loads before a
/// partial one aborts at `has_next` (`:889`) and the partial one is never
/// read.
#[test]
fn bigram_export_aborts_at_a_short_predecessor_before_a_partial_one() {
    let (first, second, successor) = export_order_fixture();
    crate::test_support::assert_aborts(
        "e2e_tests::bigram_export_aborts_at_a_short_predecessor_before_a_partial_one",
        || {
            let user_dir = TempUserDir::new("export-short-then-partial");
            crate::test_support::plant_bigram(
                &user_dir.path,
                vec![
                    (first.to_le_bytes().to_vec(), vec![7, 0, 0]),
                    (second.to_le_bytes().to_vec(), partial_gram(successor)),
                ],
            );
            let context = init_at(&user_dir.path);
            let iter = crate::iterators::pinyin_begin_get_bigram_phrases(context);
            assert!(!iter.is_null(), "begin reads no gram");
            let _ = crate::iterators::pinyin_bigram_iterator_has_next_phrase(iter);
        },
    );
}

/// Row 98 (class (b)) on the export walk: the walk ends with one warning
/// where it loads the partial value, after the rows before it.
#[test]
fn bigram_export_ends_at_a_partial_predecessor() {
    let (first, second, successor) = export_order_fixture();
    let user_dir = TempUserDir::new("export-whole-then-partial");
    crate::test_support::plant_bigram(
        &user_dir.path,
        vec![
            (first.to_le_bytes().to_vec(), whole_gram(successor)),
            (second.to_le_bytes().to_vec(), partial_gram(successor)),
        ],
    );
    let context = init_at(&user_dir.path);
    let iter = crate::iterators::pinyin_begin_get_bigram_phrases(context);
    assert!(!iter.is_null(), "begin reads no gram");
    assert!(crate::iterators::pinyin_bigram_iterator_has_next_phrase(
        iter
    ));
    let mut phrase: *mut crate::types::GChar = ptr::null_mut();
    let mut pinyin: *mut crate::types::GChar = ptr::null_mut();
    let mut count: std::os::raw::c_int = 0;
    // The row is written, then the walk meets the partial value.
    assert!(!crate::iterators::pinyin_bigram_iterator_get_next_phrase(
        iter,
        &raw mut phrase,
        &raw mut pinyin,
        &raw mut count,
    ));
    assert_eq!(count, 200);
    take_exported(phrase);
    take_exported(pinyin);
    assert!(!crate::iterators::pinyin_bigram_iterator_has_next_phrase(
        iter
    ));
    crate::iterators::pinyin_end_get_bigram_phrases(iter);
    crate::context::pinyin_fini(context);
}

/// Both export handles retain one shared runtime after fini; advancing the
/// cursor re-borrows its storage, including with no surviving instance.
#[test]
fn export_iterator_outlives_context() {
    let user_dir = TempUserDir::new("orphan-export");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let import = pinyin_begin_add_phrases(context, 5);
    assert!(pinyin_iterator_add_phrase(
        import,
        cstr("你好").as_ptr(),
        cstr("ni'hao").as_ptr(),
        7
    ));
    pinyin_end_add_phrases(import);
    train_one_multiphrase_sentence(instance);
    let phrases = crate::iterators::pinyin_begin_get_phrases(context, 5);
    let bigrams = crate::iterators::pinyin_begin_get_bigram_phrases(context);
    assert!(!phrases.is_null());
    assert!(!bigrams.is_null());
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
    assert_eq!(
        drain_phrases(phrases),
        vec![("你好".to_owned(), "ni'hao".to_owned(), 7)]
    );
    assert_eq!(
        drain_bigrams(bigrams),
        vec![("你好".to_owned(), "ni'hao".to_owned(), 138)]
    );
    crate::iterators::pinyin_end_get_phrases(phrases);
    crate::iterators::pinyin_end_get_bigram_phrases(bigrams);
}

// ---------------------------------------------------------------------
// Ports of contract-diff cases whose pin side reads freed or
// heap-dependent memory (undefined behaviour), so no frozen pin
// reference can hold them. Each asserts oxpinyin's defined behaviour;
// the expectations come from the pin's source at 074a2219 and the
// project's rulings, not from running the pin.
// ---------------------------------------------------------------------

/// The pin's candidate getter lends `m_phrase_string` (`pinyin.cpp:2873-2877`),
/// then prediction frees it before `_compute_prefixes` reads the argument
/// (`:1335-1342`, `:2420-2424`). The pin result is undefined. Oxpinyin
/// recognizes the alias by pointer identity and returns a stable empty
/// prediction; caller-owned text still predicts normally afterward.
#[test]
fn borrowed_prediction_prefix_clears_candidates_and_owned_text_recovers() {
    let user = TempUserDir::new("borrowed-prediction-prefix");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let borrowed_candidate = candidate(instance, "nihao", 0);
    let mut borrowed_text = std::ptr::null();
    assert!(crate::candidates::pinyin_get_candidate_string(
        instance,
        borrowed_candidate,
        &raw mut borrowed_text,
    ));
    assert!(!borrowed_text.is_null());
    assert!(!crate::predict::pinyin_guess_predicted_candidates(
        instance,
        borrowed_text,
    ));
    let mut count = u32::MAX;
    assert!(crate::candidates::pinyin_get_n_candidate(
        instance,
        &raw mut count
    ));
    assert_eq!(count, 0);
    assert!(crate::predict::pinyin_guess_predicted_candidates(
        instance,
        cstr("你好").as_ptr(),
    ));
    assert!(crate::candidates::pinyin_get_n_candidate(
        instance,
        &raw mut count
    ));
    assert_eq!(
        count, 0,
        "the mini fixture has no predicted suggestion rows"
    );
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// The `open <user dir>/user.conf failed.` line a fresh user directory
/// makes `pinyin_init` write: `UserTableInfo::load` prints it when the
/// marker is missing, with no trailing newline (`table_info.cpp:332` at
/// 074a2219).
fn fresh_user_conf_line(user_dir: &std::path::Path) -> String {
    // `g_build_filename` joins with the host's separator.
    format!(
        "open {}{}user.conf failed.",
        user_dir.display(),
        std::path::MAIN_SEPARATOR
    )
}

/// A second `pinyin_fini` on one context (#573, register row 105): the pin
/// deletes the context twice (`pinyin.cpp:1194-1222`) and dies of SIGSEGV;
/// the ruling is a silent return. The init's own `user.conf` line is the
/// only output.
#[test]
fn fini_twice_returns_silently() {
    let run = crate::test_support::run_in_child("e2e_tests::fini_twice_returns_silently", |user| {
        let context = init_at(user);
        assert!(!context.is_null());
        let instance = pinyin_alloc_instance(context);
        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
        crate::context::pinyin_fini(context);
    });
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        fresh_user_conf_line(&run.user_dir)
    );
}

/// GLib log records the child's default handler collected:
/// `(domain, level, message)`.
static GLIB_LOG: std::sync::Mutex<Vec<(String, u32, String)>> = std::sync::Mutex::new(Vec::new());

/// Collects every GLib log record instead of printing it, so the child's
/// stderr holds only the library's raw `fprintf` lines.
extern "C" fn collect_glib_log(
    domain: *const std::ffi::c_char,
    level: glib_sys::GLogLevelFlags,
    message: *const std::ffi::c_char,
    _data: glib_sys::gpointer,
) {
    let text = |ptr: *const std::ffi::c_char| {
        if ptr.is_null() {
            String::new()
        } else {
            // SAFETY: GLib hands NUL-terminated strings that outlive the call.
            unsafe { std::ffi::CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned()
        }
    };
    GLIB_LOG
        .lock()
        .expect("log lock")
        .push((text(domain), level, text(message)));
}

/// `#919`, `table-conf-919-short-row-merged`: the five-word row
/// `default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin` plus
/// the next line's first word `SYSTEM_FILE` is one valid six-word row
/// (`fscanf("%255s ... %255s\n")` skips white space across newlines and
/// takes whole words, `table_info.cpp:254-258`), so GB_DICTIONARY is
/// re-pointed at the GBK files; the trailing `default RESERVED NULL NULL
/// NULL` is five words, `6 != num` drops it (`:260-261`). The library and
/// table then disagree and the pin reads a NULL item (SIGSEGV, class (b)),
/// so only oxpinyin's answer is defined. The session is the case's:
/// look up, learn a sentence, import a phrase, save, finalise.
///
/// The save writes every library's log to `<user file>.tmp` first and
/// renames afterwards (`pinyin.cpp:925-1090`); libraries 1 and 2 now share
/// `gbk_char.dbin`, so the second rename finds no tmp file and prints
/// `rename ... failed.` (`pinyin.cpp:1059-1062`), the one raw stderr line
/// beside the init's missing-marker line (`table_info.cpp:332`).
#[test]
fn table_conf_919_short_row_merged() {
    let run =
        crate::test_support::run_in_child("e2e_tests::table_conf_919_short_row_merged", |user| {
            let system = crate::test_support::private_system(user.parent().expect("scratch"));
            let conf = system.join("table.conf");
            let stock = std::fs::read_to_string(&conf).expect("stock table.conf");
            std::fs::remove_file(&conf).expect("unlink the link");
            std::fs::write(
                &conf,
                stock
                    + "default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin\n"
                    + "SYSTEM_FILE default RESERVED NULL NULL NULL\n",
            )
            .expect("crafted table.conf");
            // SAFETY: the handler is a plain function and `user_data` is unused.
            unsafe {
                glib_sys::g_log_set_default_handler(Some(collect_glib_log), ptr::null_mut());
            }
            let system_c = cstr(system.to_str().expect("UTF-8 path"));
            let user_c = cstr(user.to_str().expect("UTF-8 path"));
            let context = crate::context::pinyin_init(system_c.as_ptr(), user_c.as_ptr());
            assert!(!context.is_null());
            for text in ["nihao", "yishu"] {
                let inst = pinyin_alloc_instance(context);
                pinyin_parse_more_full_pinyins(inst, cstr(text).as_ptr());
                pinyin_guess_sentence(inst);
                let mut sentence: *mut std::ffi::c_char = ptr::null_mut();
                pinyin_get_sentence(inst, 0, &raw mut sentence);
                free_exported(sentence);
                pinyin_guess_candidates(inst, 0, 0);
            }
            let inst = pinyin_alloc_instance(context);
            pinyin_parse_more_full_pinyins(inst, cstr("nihao").as_ptr());
            pinyin_guess_sentence(inst);
            let train = pinyin_train(inst, 0);
            let it = pinyin_begin_add_phrases(context, 7);
            let add = pinyin_iterator_add_phrase(
                it,
                cstr("测试词").as_ptr(),
                cstr("ce'shi'ci").as_ptr(),
                1,
            );
            pinyin_end_add_phrases(it);
            let save = pinyin_save(context);
            crate::context::pinyin_fini(context);
            assert!(train && add && save, "train {train} add {add} save {save}");
            // The user files: GB_DICTIONARY's `gb_char.dbin` is gone, the
            // GBK file name serves libraries 1 and 2 (each library's log is a
            // 26-byte empty `MemoryChunk` here), the addon and network files
            // are 27 bytes and the imported phrase makes `user.bin` 71.
            let mut sizes = std::collections::BTreeMap::new();
            let mut marker = String::new();
            for entry in std::fs::read_dir(user).expect("user dir") {
                let entry = entry.expect("entry");
                let name = entry.file_name().to_string_lossy().into_owned();
                let bytes = std::fs::read(entry.path()).expect("read");
                if name == "user.conf" {
                    marker = String::from_utf8_lossy(&bytes).into_owned();
                } else {
                    sizes.insert(name, bytes.len());
                }
            }
            // The index and bigram stores are written in the backend's own
            // container format, so only their presence is fixed.
            let present: Vec<&str> = sizes.keys().map(String::as_str).collect();
            assert_eq!(
                present,
                [
                    "addon.bin",
                    "gbk_char.dbin",
                    "merged.dbin",
                    "network.bin",
                    "opengram.dbin",
                    "user.bin",
                    "user_bigram.db",
                    "user_phrase_index.bin",
                    "user_pinyin_index.bin",
                ]
            );
            for (name, size) in [
                ("addon.bin", 27),
                ("gbk_char.dbin", 26),
                ("merged.dbin", 26),
                ("network.bin", 27),
                ("opengram.dbin", 26),
                ("user.bin", 71),
            ] {
                assert_eq!(sizes[name], size, "{name}");
            }
            // The open counter the init raised, the fini lowered.
            // user.conf is written in text mode: CRLF on Windows.
            let newline = oxpinyin_input::TEXT_NEWLINE;
            assert!(
                marker.starts_with(&format!(
                    "binary format version:7{newline}model data version:14{newline}"
                )) && marker.ends_with(&format!("{newline}open counter:0{newline}")),
                "{marker}"
            );
            // Two lookups, two warnings: the merged row's library and table
            // disagree, and the pin's NULL item read becomes one warning per
            // candidate-list fill.
            let records = GLIB_LOG.lock().expect("log lock").clone();
            let warning = (
                "libpinyin".to_owned(),
                glib_sys::G_LOG_LEVEL_WARNING,
                "pinyin_guess_candidates: a candidate's phrase item cannot be read".to_owned(),
            );
            assert_eq!(records, [warning.clone(), warning]);
        });
    let user = run.user_dir.display();
    // `g_build_filename` joins with the host's separator, and the UCRT's
    // text-mode stderr writes `\n` as `\r\n`.
    let sep = std::path::MAIN_SEPARATOR;
    let newline = oxpinyin_input::TEXT_NEWLINE;
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        format!(
            "open {user}{sep}user.conf failed.\
             rename {user}{sep}gbk_char.dbin.tmp to {user}{sep}gbk_char.dbin failed.{newline}"
        )
    );
}

/// One step of the bigram export walk: `has_next` answered false, or a
/// row `get_next` handed out (`get_next`'s answer, phrase, pinyin, count).
#[derive(Debug, PartialEq)]
enum ExportStep {
    NoNext,
    Row(bool, String, String, i32),
}

/// Plants the user-gram layout of the `stored_user_gram` contract cases
/// (`user_gram_layout`) and walks the bigram export at most sixteen steps,
/// stopping after two consecutive `has_next` answers of false.
///
/// Session one adds the user phrase 龘龘 (`da2'da2`, count 5) to library 7
/// and saves. The gram under 你 is then replaced, `total` first and the
/// `(token, count)` items verbatim in the stored order the layout names,
/// and a second session walks the export. Every count passes the export
/// threshold (68); the walk doubles each count (register row 1).
fn export_walk(tag: &str, items: impl FnOnce(u32, u32, u32) -> Vec<(u32, u32)>) -> Vec<ExportStep> {
    let user_dir = TempUserDir::new(tag);
    let path = user_dir.path.to_str().expect("UTF-8 path").to_owned();
    let (context, instance) = open(&path);
    let it = pinyin_begin_add_phrases(context, 7);
    assert!(pinyin_iterator_add_phrase(
        it,
        cstr("龘龘").as_ptr(),
        cstr("da2'da2").as_ptr(),
        5
    ));
    pinyin_end_add_phrases(it);
    let (ni, hao, hao_alt, dalong) = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        let system = |text: &str| {
            inst.core
                .dict
                .tokens_for_text(text)
                .first()
                .expect("the fixture holds the character")
                .value()
        };
        let dalong = inst
            .core
            .user
            .expect("instance carries a user store")
            .token_for_phrase("龘龘")
            .expect("readable store")
            .expect("the imported phrase has a token");
        (system("你"), system("好"), system("号"), dalong)
    };
    assert!(pinyin_save(context));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);

    lay_gram(&user_dir.path, ni, &items(hao, hao_alt, dalong));
    walk_bigram_export(&user_dir.path)
}

/// Lays a planted `user_bigram.db` holding PREDECESSOR's gram of ITEMS, `(token, count)` in the
/// stored order with their total first, over the profile at USER_DIR.
fn lay_gram(user_dir: &std::path::Path, predecessor: u32, items: &[(u32, u32)]) {
    let total: u32 = items.iter().map(|&(_, count)| count).sum();
    let mut gram = total.to_le_bytes().to_vec();
    for &(token, count) in items {
        gram.extend_from_slice(&token.to_le_bytes());
        gram.extend_from_slice(&count.to_le_bytes());
    }
    // `plant_bigram` writes a whole profile; only its `user_bigram.db` is
    // wanted, laid over the session-one profile that holds the phrase.
    let planted = TempUserDir::new("export-gram-plant");
    crate::test_support::plant_bigram(
        &planted.path,
        vec![(predecessor.to_le_bytes().to_vec(), gram)],
    );
    std::fs::copy(
        planted.path.join("user_bigram.db"),
        user_dir.join("user_bigram.db"),
    )
    .expect("lay the planted bigram over the profile");
}

/// Opens USER_DIR and walks the bigram export at most sixteen steps, stopping after two
/// consecutive `has_next` answers of false.
fn walk_bigram_export(user_dir: &std::path::Path) -> Vec<ExportStep> {
    let context = init_at(user_dir);
    assert!(!context.is_null());
    let iter = crate::iterators::pinyin_begin_get_bigram_phrases(context);
    let mut steps = Vec::new();
    let mut falses = 0;
    while falses < 2 && steps.len() < 16 {
        if !crate::iterators::pinyin_bigram_iterator_has_next_phrase(iter) {
            falses += 1;
            steps.push(ExportStep::NoNext);
            continue;
        }
        falses = 0;
        let mut phrase: *mut crate::types::GChar = ptr::null_mut();
        let mut pinyin: *mut crate::types::GChar = ptr::null_mut();
        let mut count: std::os::raw::c_int = 0;
        let ret = crate::iterators::pinyin_bigram_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut pinyin,
            &raw mut count,
        );
        steps.push(ExportStep::Row(
            ret,
            take_exported(phrase),
            take_exported(pinyin),
            count,
        ));
    }
    crate::iterators::pinyin_end_get_bigram_phrases(iter);
    crate::context::pinyin_fini(context);
    steps
}

/// `user-gram-unsorted-export`: 你's stored gram holds `[龘龘 90, 号 80,
/// 好 70]`, descending by token, not the ascending order the pin writes.
/// The pin's export reads `retrieve_all`, which appends the raw items in
/// stored order (`ngram.cpp:133-151` at 074a2219), and the walk hands the
/// array out in that order; the `std::lower_bound` over the raw array
/// (`:86-99`) is a lookup and does not re-sort it. The walk's first
/// `has_next` answers false (its do/while loads the last predecessor's gram
/// on a false answer, register row 36); the last row's `get_next` answer is
/// the walk's, `false` at the end of the gram.
#[test]
fn user_gram_unsorted_export_follows_the_stored_order() {
    let steps = export_walk("export-unsorted", |hao, hao_alt, dalong| {
        let mut items = vec![(dalong, 90), (hao_alt, 80), (hao, 70)];
        items.sort_by(|a, b| b.cmp(a));
        items
    });
    let row = |ok: bool, phrase: &str, pinyin: &str, count: i32| {
        ExportStep::Row(ok, phrase.to_owned(), pinyin.to_owned(), count)
    };
    assert_eq!(
        steps,
        [
            ExportStep::NoNext,
            row(true, "你龘龘", "ni'da2'da2", 180),
            row(true, "你号", "ni'hao", 160),
            row(false, "你好", "ni'hao", 140),
            ExportStep::NoNext,
            ExportStep::NoNext,
        ]
    );
}

/// `user-gram-duplicate-export`: 你's stored gram holds 好 and 龘龘 twice,
/// sorted: `[好 70, 好 80, 龘龘 90, 龘龘 100]`. `retrieve_all` appends
/// every stored item, duplicates included (`ngram.cpp:133-151` at
/// 074a2219), so the export lists each twice, in stored order.
#[test]
fn user_gram_duplicate_export_lists_every_stored_item() {
    let steps = export_walk("export-duplicate", |hao, _, dalong| {
        vec![(hao, 70), (hao, 80), (dalong, 90), (dalong, 100)]
    });
    let row = |ok: bool, phrase: &str, pinyin: &str, count: i32| {
        ExportStep::Row(ok, phrase.to_owned(), pinyin.to_owned(), count)
    };
    assert_eq!(
        steps,
        [
            ExportStep::NoNext,
            row(true, "你好", "ni'hao", 140),
            row(true, "你好", "ni'hao", 160),
            row(true, "你龘龘", "ni'da2'da2", 180),
            row(false, "你龘龘", "ni'da2'da2", 200),
            ExportStep::NoNext,
            ExportStep::NoNext,
        ]
    );
}

// ---------------------------------------------------------------------
// NULL arguments the pin never reads (row 41's non-crashing probes). Each
// test hands valid objects and the one NULL argument the pin ignores; the
// expectations are the pin's answer at 074a2219 (cited beside each), not a
// pin run.
// ---------------------------------------------------------------------

/// A parsed pinyin key and key rest at offset 0 of `text`, borrowed from the
/// process-wide key slots (hold `key_slot_guard`).
fn parsed_key_and_rest(
    instance: *mut PinyinInstance,
    text: &str,
) -> (
    *mut crate::types::ChewingKey,
    *mut crate::types::ChewingKeyRest,
) {
    let input = cstr(text);
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, input.as_ptr()),
        text.len()
    );
    let mut key = ptr::null_mut();
    assert!(crate::cursor::pinyin_get_pinyin_key(
        instance,
        0,
        &raw mut key
    ));
    let mut rest = ptr::null_mut();
    assert!(crate::cursor::pinyin_get_pinyin_key_rest(
        instance,
        0,
        &raw mut rest
    ));
    (key, rest)
}

/// `pinyin_get_candidate_type` never reads `instance` (`pinyin.cpp:2866-2871`):
/// it writes `*type` and answers `true`.
#[test]
fn null_instance_candidate_type_answers() {
    let user = TempUserDir::new("null-instance-candidate-type");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let cand = candidate(instance, "nihao", 0);
    let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
    assert!(crate::candidates::pinyin_get_candidate_type(
        ptr::null_mut(),
        cand,
        &raw mut kind
    ));
    assert_eq!(kind, lookup_candidate_type_t::NORMAL_CANDIDATE);
}

/// `pinyin_get_candidate_string` never reads `instance` (`pinyin.cpp:2873-2878`).
#[test]
fn null_instance_candidate_string_answers() {
    let user = TempUserDir::new("null-instance-candidate-string");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let cand = candidate(instance, "nihao", 0);
    let mut text = ptr::null();
    assert!(crate::candidates::pinyin_get_candidate_string(
        ptr::null_mut(),
        cand,
        &raw mut text
    ));
    assert!(!text.is_null());
    // SAFETY: the candidate text is NUL-terminated and borrowed from the instance.
    let text = unsafe { std::ffi::CStr::from_ptr(text) };
    assert_eq!(text.to_str().expect("UTF-8"), "你好");
}

/// `pinyin_get_candidate_nbest_index` never reads `instance`
/// (`pinyin.cpp:2880-2886`; the assert at `:2883` reads the candidate).
#[test]
fn null_instance_candidate_nbest_index_answers() {
    let user = TempUserDir::new("null-instance-nbest-index");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let input = cstr("nihao");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let mut cand = ptr::null_mut();
    assert!(pinyin_get_candidate(instance, 0, &raw mut cand));
    let mut index = u8::MAX;
    assert!(crate::candidates::pinyin_get_candidate_nbest_index(
        ptr::null_mut(),
        cand,
        &raw mut index
    ));
    assert_eq!(index, 0);
}

/// `pinyin_is_user_candidate` never reads `instance` (`pinyin.cpp:3712-3724`):
/// a normal candidate in `USER_DICTIONARY` answers `true`.
#[test]
fn null_instance_is_user_candidate_answers() {
    let user = TempUserDir::new("null-instance-is-user");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let iter = pinyin_begin_add_phrases(context, USER_DICTIONARY);
    assert!(pinyin_iterator_add_phrase(
        iter,
        cstr("你号").as_ptr(),
        cstr("ni3'hao4").as_ptr(),
        1
    ));
    pinyin_end_add_phrases(iter);
    let _ = candidate(instance, "nihao", 0);
    let mut count = 0;
    assert!(crate::candidates::pinyin_get_n_candidate(
        instance,
        &raw mut count
    ));
    let mut found = false;
    for index in 0..count {
        let mut cand = ptr::null_mut();
        assert!(pinyin_get_candidate(instance, index, &raw mut cand));
        if pinyin_is_user_candidate(instance, cand) {
            assert!(pinyin_is_user_candidate(ptr::null_mut(), cand));
            found = true;
        }
    }
    assert!(found, "the imported user phrase is a candidate");
}

/// The four string renderers never read `instance` (`pinyin.cpp:2707-2749`).
#[test]
fn null_instance_key_string_renderers_answer() {
    let _slots = crate::test_support::key_slot_guard();
    let user = TempUserDir::new("null-instance-key-strings");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let (key, _) = parsed_key_and_rest(instance, "ni3");
    let render = |getter: extern "C" fn(
        *mut PinyinInstance,
        *mut crate::types::ChewingKey,
        *mut *mut std::ffi::c_char,
    ) -> bool| {
        let mut out = ptr::null_mut();
        assert!(getter(ptr::null_mut(), key, &raw mut out));
        crate::ffi::take_owned_cstr(out)
    };
    assert_eq!(render(crate::cursor::pinyin_get_pinyin_string), "ni3"); // :2718-2727
    assert_eq!(render(crate::cursor::pinyin_get_zhuyin_string), "ㄋㄧˇ"); // :2707-2716
    assert_eq!(render(crate::keys::pinyin_get_luoma_pinyin_string), "ni3"); // :2729-2738
    assert_eq!(
        render(crate::keys::pinyin_get_secondary_zhuyin_string),
        "ni3"
    ); // :2740-2749
}

/// `pinyin_get_pinyin_strings` never reads `instance` (`pinyin.cpp:2751-2763`);
/// each of `shengmu` and `yunmu` is optional.
#[test]
fn null_instance_pinyin_strings_answer() {
    let _slots = crate::test_support::key_slot_guard();
    let user = TempUserDir::new("null-instance-pinyin-strings");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let (key, _) = parsed_key_and_rest(instance, "ni3");
    let (mut shengmu, mut yunmu) = (ptr::null_mut(), ptr::null_mut());
    assert!(crate::cursor::pinyin_get_pinyin_strings(
        ptr::null_mut(),
        key,
        &raw mut shengmu,
        &raw mut yunmu
    ));
    assert_eq!(crate::ffi::take_owned_cstr(shengmu), "n");
    assert_eq!(crate::ffi::take_owned_cstr(yunmu), "i");
    assert!(crate::cursor::pinyin_get_pinyin_strings(
        ptr::null_mut(),
        key,
        ptr::null_mut(),
        ptr::null_mut()
    ));
}

/// `pinyin_get_pinyin_is_incomplete` never reads `instance`
/// (`pinyin.cpp:2765-2774`): an initial-only key answers `true`.
#[test]
fn null_instance_is_incomplete_answers() {
    let _slots = crate::test_support::key_slot_guard();
    let user = TempUserDir::new("null-instance-is-incomplete");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    assert!(pinyin_set_options(context, 0x28));
    let (key, _) = parsed_key_and_rest(instance, "n");
    assert!(crate::keys::pinyin_get_pinyin_is_incomplete(
        ptr::null_mut(),
        key
    ));
    let (complete, _) = parsed_key_and_rest(instance, "ni3");
    assert!(!crate::keys::pinyin_get_pinyin_is_incomplete(
        ptr::null_mut(),
        complete
    ));
}

/// `pinyin_get_pinyin_key_rest_positions` never reads `instance`, and reads
/// `key_rest` only to fill a non-NULL `begin` or `end`
/// (`pinyin.cpp:2967-2977`).
#[test]
fn key_rest_positions_follow_the_pins_reads() {
    let _slots = crate::test_support::key_slot_guard();
    let user = TempUserDir::new("key-rest-positions-null");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let (_, rest) = parsed_key_and_rest(instance, "ni3");
    let (mut begin, mut end) = (u16::MAX, u16::MAX);
    // NULL instance: answered (`:2967-2977` never reads it).
    assert!(crate::cursor::pinyin_get_pinyin_key_rest_positions(
        ptr::null_mut(),
        rest,
        &raw mut begin,
        &raw mut end
    ));
    assert_eq!((begin, end), (0, 3));
    // NULL `key_rest` with both outs NULL: no read, `true` (`:2970-2975`).
    assert!(crate::cursor::pinyin_get_pinyin_key_rest_positions(
        instance,
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut()
    ));
    // NULL `key_rest` with an out: the pin reads it (row 41), so this stays guarded.
    begin = u16::MAX;
    assert!(!crate::cursor::pinyin_get_pinyin_key_rest_positions(
        instance,
        ptr::null_mut(),
        &raw mut begin,
        ptr::null_mut()
    ));
    assert!(!crate::cursor::pinyin_get_pinyin_key_rest_positions(
        instance,
        ptr::null_mut(),
        ptr::null_mut(),
        &raw mut end
    ));
    assert_eq!(begin, u16::MAX);
}

/// `pinyin_get_pinyin_key_rest_length` never reads `instance`
/// (`pinyin.cpp:2979-2984`).
#[test]
fn null_instance_key_rest_length_answers() {
    let _slots = crate::test_support::key_slot_guard();
    let user = TempUserDir::new("null-instance-key-rest-length");
    let (_context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    let (_, rest) = parsed_key_and_rest(instance, "ni3");
    let mut length = u16::MAX;
    assert!(crate::cursor::pinyin_get_pinyin_key_rest_length(
        ptr::null_mut(),
        rest,
        &raw mut length
    ));
    assert_eq!(length, 3);
}

/// The NULL handles of the constraint, export and removal entries answer
/// `false` (or NULL) and read nothing (register row 41).
#[test]
fn null_handles_answer_false() {
    assert!(!pinyin_clear_constraint(ptr::null_mut(), 0));
    assert!(crate::iterators::pinyin_begin_get_phrases(ptr::null_mut(), 7).is_null());
    assert!(crate::iterators::pinyin_begin_get_bigram_phrases(ptr::null_mut()).is_null());
    assert!(!pinyin_iterator_has_next_phrase(ptr::null_mut()));
    assert!(!crate::iterators::pinyin_bigram_iterator_has_next_phrase(
        ptr::null_mut()
    ));
    let user = TempUserDir::new("null-handles");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    assert!(!pinyin_remove_user_candidate(instance, ptr::null_mut()));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// `pinyin_begin_add_phrases` stores the context unread and returns the
/// iterator (`pinyin.cpp:506-512`). `pinyin_iterator_add_phrase` returns
/// `false` before it reads the context for a NULL phrase (`:624-625`) and
/// where it would read it (`_add_phrase`, `:526`) the pin crashes (row 41);
/// `pinyin_end_add_phrases` reads it at `:657` (row 41), so the handle is freed.
#[test]
fn null_context_import_handle_is_returned_and_freed() {
    let iter = pinyin_begin_add_phrases(ptr::null_mut(), 1);
    assert!(!iter.is_null());
    assert!(!pinyin_iterator_add_phrase(
        iter,
        ptr::null(),
        cstr("ni3'hao3").as_ptr(),
        1
    ));
    assert!(!pinyin_iterator_add_phrase(
        iter,
        cstr("你好").as_ptr(),
        cstr("ni3'hao3").as_ptr(),
        1
    ));
    pinyin_end_add_phrases(iter);
}

/// The index assert (`pinyin.cpp:466`) precedes any read of the context, so a
/// NULL context aborts for index 16; an in-range non-GBK index answers
/// `false` (`:470-471`) without reading it.
#[test]
fn null_context_unload_phrase_library_orders_assert_first() {
    assert!(!crate::config::pinyin_unload_phrase_library(
        ptr::null_mut(),
        2
    ));
    crate::test_support::assert_aborts(
        "e2e_tests::null_context_unload_phrase_library_orders_assert_first",
        || {
            crate::config::pinyin_unload_phrase_library(ptr::null_mut(), 16);
        },
    );
}

/// `pinyin_unload_addon_phrase_library`: the assert at `pinyin.cpp:499`
/// precedes the context read at `:502`.
#[test]
fn null_context_unload_addon_phrase_library_aborts_on_index_16() {
    crate::test_support::assert_aborts(
        "e2e_tests::null_context_unload_addon_phrase_library_aborts_on_index_16",
        || {
            crate::config::pinyin_unload_addon_phrase_library(ptr::null_mut(), 16);
        },
    );
}

/// `_remember_phrase_recur` calls `_add_phrase` for every complete path and
/// ORs the answers (`pinyin.cpp:3654-3659`). With `L_N` the composition
/// `nong` holds two keys over one span, `nong` and `long`, and a character
/// whose item pronounces both is added along both paths: each reading's
/// frequency rises by the count.
#[test]
fn remember_user_input_adds_along_every_complete_path() {
    let user_dir = TempUserDir::new("remember-two-paths");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let import = pinyin_begin_add_phrases(context, 7);
    for reading in ["nong", "long"] {
        assert!(pinyin_iterator_add_phrase(
            import,
            cstr("弄").as_ptr(),
            cstr(reading).as_ptr(),
            1
        ));
    }
    pinyin_end_add_phrases(import);
    let token = with_store(instance, |store| store.token_for_phrase("弄"))
        .expect("readable store")
        .expect("the import gave 弄 a token");
    let readings = |instance| {
        with_store(instance, |store| store.item_pronunciations(token))
            .expect("readable store")
            .expect("an item")
            .iter()
            .map(|(keys, freq)| (keys.len(), *freq))
            .collect::<Vec<_>>()
    };
    let before = readings(instance);
    assert_eq!(before.len(), 2, "two imported readings");

    assert!(pinyin_set_options(context, 0x8 | (1 << 15)));
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, cstr("nong").as_ptr()),
        4
    );
    assert!(pinyin_remember_user_input(instance, cstr("弄").as_ptr(), 3));
    let after = readings(instance);
    assert_eq!(after.len(), before.len());
    for ((_, was), (_, now)) in before.iter().zip(&after) {
        assert_eq!(*now, *was + 3, "each path adds its reading once");
    }
}

// ── Pin capture of 2026-11-07 (tools/oracle/pin_capture.py, runs 1 and 2 identical) ──
//
// Each test below names its observation. The values are the pin's on the pinned
// libpinyin-data; every one asserted here depends only on the test's own imports, parse
// and options, so the w3 mini fixture holds it too.

/// Every row of the current candidate list as (type, string, n-best index of an NBEST row).
pub(crate) fn capture_rows(
    instance: *mut PinyinInstance,
) -> Vec<(lookup_candidate_type_t, String, Option<u8>)> {
    let mut count = 0;
    assert!(crate::candidates::pinyin_get_n_candidate(
        instance,
        &raw mut count
    ));
    (0..count)
        .map(|index| {
            let mut cand: *mut LookupCandidate = ptr::null_mut();
            assert!(pinyin_get_candidate(instance, index, &raw mut cand));
            let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
            assert!(crate::candidates::pinyin_get_candidate_type(
                instance,
                cand,
                &raw mut kind
            ));
            let mut text: *const crate::types::GChar = ptr::null();
            assert!(crate::candidates::pinyin_get_candidate_string(
                instance,
                cand,
                &raw mut text
            ));
            // SAFETY: a true answer points `text` at the row's borrowed string.
            let text = unsafe { std::ffi::CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned();
            let nbest = (kind == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE).then(|| {
                let mut index = 0_u8;
                assert!(crate::candidates::pinyin_get_candidate_nbest_index(
                    instance,
                    cand,
                    &raw mut index
                ));
                index
            });
            (kind, text, nbest)
        })
        .collect()
}

/// Imports ROWS into LIBRARY in one batch; answers each add's flag.
pub(crate) fn import_batch(
    context: *mut crate::types::PinyinContext,
    library: u8,
    rows: &[(&str, &str, i32)],
) -> Vec<bool> {
    let iter = pinyin_begin_add_phrases(context, library);
    assert!(!iter.is_null());
    let added = rows
        .iter()
        .map(|&(phrase, reading, count)| {
            pinyin_iterator_add_phrase(iter, cstr(phrase).as_ptr(), cstr(reading).as_ptr(), count)
        })
        .collect();
    pinyin_end_add_phrases(iter);
    added
}

/// Library INDEX's export rows, through `begin`/`has_next`/`get_next`.
pub(crate) fn export_of(
    context: *mut crate::types::PinyinContext,
    index: u32,
) -> Vec<(String, String, i32)> {
    let iter = pinyin_begin_get_phrases(context, index);
    assert!(
        !iter.is_null(),
        "begin never answers NULL on a live context (pinyin.cpp:662-687)"
    );
    let rows = drain_phrases(iter);
    pinyin_end_get_phrases(iter);
    rows
}

/// Saves, then reopens the context on the same user directory.
fn save_and_reopen(
    context: *mut crate::types::PinyinContext,
    instance: *mut PinyinInstance,
    user_dir: &std::path::Path,
) -> (*mut crate::types::PinyinContext, *mut PinyinInstance) {
    assert!(pinyin_save(context));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
    open(user_dir.to_str().expect("UTF-8 path"))
}
/// S3: an import count other than -1 reaches the item as its `guint32` bit pattern
/// (`pinyin.cpp:523-524`, `:578`, `:601`); the export starts at -1 and writes the count only when
/// it is above 0 (`:703`, `:738-739`), so 0 exports as -1. A second add onto `INT_MAX` passes the
/// overflow guard (`phrase_index.cpp:78`) and stores 0x80000000. Every add answers true, and a
/// save and reopen keep the row. Observations `import-count/0`, `/-2`, `/INT_MIN`, `/INT_MAX`
/// and `/INT_MAX+1`.
#[test]
fn import_counts_reach_the_export_as_the_pin_stores_them() {
    type Row = (&'static str, &'static str, i32);
    let cases: [(&[Row], Row); 5] = [
        (&[("零次", "ling'ci", 0)], ("零次", "ling'ci", -1)),
        (&[("负二", "fu'er", -2)], ("负二", "fu'er", -2)),
        (
            &[("最小", "zui'xiao", i32::MIN)],
            ("最小", "zui'xiao", i32::MIN),
        ),
        (
            &[("最大", "zui'da", i32::MAX)],
            ("最大", "zui'da", i32::MAX),
        ),
        (
            &[("最大", "zui'da", i32::MAX), ("最大", "zui'da", 1)],
            ("最大", "zui'da", i32::MIN),
        ),
    ];
    for (rows, (phrase, reading, count)) in cases {
        let user_dir = TempUserDir::new("import-count");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert!(import_batch(context, 7, rows).iter().all(|&added| added));
        let expected = vec![(phrase.to_owned(), reading.to_owned(), count)];
        assert_eq!(export_of(context, 7), expected, "{rows:?}");
        let (context, instance) = save_and_reopen(context, instance, &user_dir.path);
        assert_eq!(export_of(context, 7), expected, "{rows:?} after reopen");
        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}

/// D1: an import into a slot with no sub-index answers false: `get_range` fails
/// (`pinyin.cpp:589`; `phrase_index.cpp:610-613`) and `result` stays false (`pinyin.cpp:608`).
/// Library 7 stays empty. Observations `import/library/0` and `import/library/15`.
///
/// D2: libraries 16 and 255 read past the 16-slot array (`phrase_index.cpp:611`,
/// `phrase_index.h:441`), undefined behaviour whose pinyin outcome is not stable (compatibility
/// register row 43: the audit's D-12 recorded 255 accepted). oxpinyin refuses them; no capture.
#[test]
fn an_import_into_a_slot_without_a_sub_index_is_refused() {
    let user_dir = TempUserDir::new("import-no-sub-index");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    for library in [0, 15, 16, 255] {
        assert_eq!(
            import_batch(context, library, &[("你好", "nihao", 1)]),
            [false],
            "library {library}"
        );
    }
    assert_eq!(export_of(context, 7), []);
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// D3: a second reading merged into an item is appended and never indexed (`pinyin.cpp:578`,
/// `phrase_index.cpp:88-90`), and lowers the item's pronunciation possibility, the matched
/// frequency over the total (`phrase_index.h:136-163`): 测侧 loses the sentence to 侧测 once its
/// `xi'xi` reading carries 9,000,000 of 10,000,000. Observation `import/dilution/cece`.
#[test]
fn an_unindexed_second_reading_dilutes_the_sentence_score() {
    let user_dir = TempUserDir::new("import-dilution");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let sentence = || {
        let input = cstr("cece");
        assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 4);
        assert!(pinyin_guess_sentence(instance));
        let mut text: *mut std::os::raw::c_char = ptr::null_mut();
        assert!(pinyin_get_sentence(instance, 0, &raw mut text));
        assert!(pinyin_reset(instance));
        crate::ffi::take_owned_cstr(text)
    };
    assert_eq!(
        import_batch(
            context,
            1,
            &[("测侧", "ce'ce", 1_000_000), ("侧测", "ce'ce", 600_000)]
        ),
        [true, true]
    );
    assert_eq!(sentence(), "测侧");
    assert_eq!(
        import_batch(context, 1, &[("测侧", "xi'xi", 9_000_000)]),
        [true]
    );
    assert_eq!(sentence(), "侧测");
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// D4: a keyed reparse leaves the n-best results in place (`pinyin.cpp:1497-1524`; only reset
/// clears them, `:2700`): the sentence reads the stale result (`:1464-1474`), and the candidate
/// list prepends it as its NBEST row (`:1934-1951`) until a new guess replaces it. Observation
/// `assembly/stale-nbest/nihao-zhongguo`; its row count is the pinned data's and is not asserted.
#[test]
fn a_reparse_keeps_the_stale_nbest_result_until_the_next_guess() {
    let user_dir = TempUserDir::new("stale-nbest");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let sentence = || {
        let mut text: *mut std::os::raw::c_char = ptr::null_mut();
        assert!(pinyin_get_sentence(instance, 0, &raw mut text));
        crate::ffi::take_owned_cstr(text)
    };
    let nihao = cstr("nihao");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, nihao.as_ptr()), 5);
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence(), "你好");
    let zhongguo = cstr("zhongguo");
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, zhongguo.as_ptr()),
        8
    );
    assert_eq!(sentence(), "你好", "the stale result");
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    assert_eq!(
        capture_rows(instance)[0],
        (
            lookup_candidate_type_t::NBEST_MATCH_CANDIDATE,
            "你好".to_owned(),
            Some(0)
        )
    );
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence(), "中国");
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// D15: one-character user phrases under readings that differ only in the stored key, listed
/// for the incomplete key `n` (0x18a): their order is the user pinyin index's key order, the
/// ranges `convert` compresses (`storage/chewing_large_table2.h:91-133`). On the pinned data they
/// are rows 788-796 of 797, in this order; the system rows around them are the data's and are
/// not asserted. Observation `tie/user-n`.
#[test]
fn user_rows_for_an_incomplete_key_follow_the_stored_key_order() {
    let user_dir = TempUserDir::new("tie-user-n");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // 𦰡 takes two readings, the second appended; the candidate order is `pinyin.cpp:2184-2308` at 074a2219.
    let phrases = [
        ("𦰡", "na"),
        ("𫐐", "ni"),
        ("𫠜", "ni"),
        ("𨺙", "ni"),
        ("𫔶", "nie"),
        ("𬪩", "nong"),
        ("𦰡", "nuo"),
        ("𫟅", "nen"),
        ("𫵷", "nao"),
        ("𬇕", "nu"),
    ];
    let rows: Vec<(&str, &str, i32)> = phrases
        .iter()
        .map(|&(phrase, reading)| (phrase, reading, 1))
        .collect();
    assert!(import_batch(context, 7, &rows).iter().all(|&added| added));
    assert!(pinyin_set_options(context, 0x18a));
    let n = cstr("n");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, n.as_ptr()), 1);
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let user: Vec<String> = capture_rows(instance)
        .into_iter()
        .filter(|(_, text, _)| phrases.iter().any(|&(phrase, _)| phrase == text))
        .map(|(kind, text, _)| {
            assert_eq!(kind, lookup_candidate_type_t::NORMAL_CANDIDATE, "{text}");
            text
        })
        .collect();
    assert_eq!(user, ["𦰡", "𫵷", "𫟅", "𬪩", "𫐐", "𫠜", "𨺙", "𫔶", "𬇕"]);
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// D6: the bigram export lists an item only when its count is above
/// `threshold = initial_seed - 1`, 68 (`pinyin.cpp:791-792`, `:809` at 074a2219), and doubles
/// it (`:906`): 你's gram `[好 68, 号 69]` exports only 你号, at 138. Source-derived, with no
/// capture, per the 2026-11-01 ruling: the unpatched pin joins the pinyin strings with
/// `g_strjoinv` over an unterminated array (`:849`, `:862`; compatibility register row 1).
#[cfg(unix)]
#[test]
fn the_bigram_export_skips_a_count_at_the_threshold() {
    let steps = export_walk("export-threshold", |hao, hao_alt, _| {
        vec![(hao, 68), (hao_alt, 69)]
    });
    assert_eq!(
        steps,
        [
            ExportStep::NoNext,
            ExportStep::Row(false, "你号".to_owned(), "ni'hao".to_owned(), 138),
            ExportStep::NoNext,
            ExportStep::NoNext,
        ]
    );
}

/// D7: the export lists every reading of the predecessor against every reading of the
/// successor, the predecessor's outer (`pinyin.cpp:839-869` at 074a2219). 龘龘 holds two
/// readings, `da2'da2` and the appended `da4'da4` (`phrase_index.cpp:88-90`), and its own gram
/// holds `[龘龘 100]`: four rows, each at 200 (`:906`). `get_next` answers `has_next` after each
/// row (`:910`), so false only on the last. Source-derived, as D6 is (register row 1).
#[cfg(unix)]
#[test]
fn the_bigram_export_crosses_both_items_readings() {
    let user_dir = TempUserDir::new("export-cross-product");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert_eq!(
        import_batch(
            context,
            7,
            &[("龘龘", "da2'da2", 5), ("龘龘", "da4'da4", 5)]
        ),
        [true, true]
    );
    let dalong = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        inst.core
            .user
            .expect("instance carries a user store")
            .token_for_phrase("龘龘")
            .expect("readable store")
            .expect("the imported phrase has a token")
    };
    assert!(pinyin_save(context));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
    lay_gram(&user_dir.path, dalong, &[(dalong, 100)]);
    let row = |more: bool, pinyin: &str| {
        ExportStep::Row(more, "龘龘龘龘".to_owned(), pinyin.to_owned(), 200)
    };
    assert_eq!(
        walk_bigram_export(&user_dir.path),
        [
            ExportStep::NoNext,
            row(true, "da2'da2'da2'da2"),
            row(true, "da2'da2'da4'da4"),
            row(true, "da4'da4'da2'da2"),
            row(false, "da4'da4'da4'da4"),
            ExportStep::NoNext,
            ExportStep::NoNext,
        ]
    );
}

fn unigram_item_count(instance: *mut crate::types::PinyinInstance, token: u32) -> (bool, u32) {
    let mut count = u32::MAX;
    let found = crate::dict::pinyin_token_get_unigram_frequency(instance, token, &raw mut count);
    (found, count)
}

/// U1: the add changes the item serialized by a later armed save
/// (074a2219 `phrase_index.cpp:173-175`, `pinyin.cpp:978-981,1132-1145`).
#[test]
fn user_unigram_increment_survives_a_later_save() {
    let user = TempUserDir::new("user-unigram-save");
    let path = user.path.to_str().unwrap();
    let (context, instance) = open(path);
    assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 5)]), [true]);
    let token = with_store(instance, |store| {
        store.token_for_phrase("龘龘").unwrap().unwrap()
    });
    let (found, base) = unigram_item_count(instance, token);
    assert!(found);
    assert!(pinyin_save(context));
    assert!(crate::dict::pinyin_token_add_unigram_frequency(
        instance, token, 9
    ));
    assert!(
        !pinyin_save(context),
        "the unigram add alone leaves save unarmed"
    );
    assert!(import_batch(context, 7, &[]).is_empty());
    assert!(pinyin_save(context));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);

    let (context, instance) = open(path);
    assert_eq!(unigram_item_count(instance, token), (true, base + 9));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// U4: the library and slot masks ignore the high nibble
/// (074a2219 `novel_types.h:41-44`, `phrase_index.cpp:154,186`).
#[test]
fn unigram_get_and_add_ignore_the_token_tag() {
    let user = TempUserDir::new("tagged-unigram");
    let (context, instance) = open(user.path.to_str().unwrap());
    assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 5)]), [true]);
    let tokens = {
        // SAFETY: the instance is live and this read ends before mutations.
        let inst = unsafe { instance_ref(instance) };
        [
            inst.core.dict.tokens_for_text("你")[0].value(),
            inst.core
                .user
                .unwrap()
                .token_for_phrase("龘龘")
                .unwrap()
                .unwrap(),
        ]
    };
    for token in tokens {
        let (found, base) = unigram_item_count(instance, token);
        assert!(found);
        let tagged = token | 0xf000_0000;
        assert_eq!(unigram_item_count(instance, tagged), (true, base));
        assert!(crate::dict::pinyin_token_add_unigram_frequency(
            instance, tagged, 1
        ));
        assert_eq!(unigram_item_count(instance, token), (true, base + 1));
    }
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// U5: mask-out removes the live item and subtracts its whole unigram
/// from the facade total (074a2219 `phrase_index.cpp:233,713-715`).
#[test]
fn removing_a_user_item_takes_its_unigram_increment_out_of_the_total() {
    let user = TempUserDir::new("user-unigram-remove");
    let (context, instance) = open(user.path.to_str().unwrap());
    let total = || {
        // SAFETY: the instance is live for each call and no borrow escapes.
        unsafe { instance_ref(instance) }.core.lm.amplified_total()
    };
    let before = total();
    assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 0)]), [true]);
    let token = with_store(instance, |store| {
        store.token_for_phrase("龘龘").unwrap().unwrap()
    });
    assert!(crate::dict::pinyin_token_add_unigram_frequency(
        instance, token, 9
    ));
    assert_eq!(total(), before + 9);
    assert!(pinyin_mask_out(context, u32::MAX, token));
    assert_eq!(unigram_item_count(instance, token), (false, 0));
    assert_eq!(total(), before);
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// U6: the persisted sub-index total is independent of item sums. The
/// pin guards that total and wraps the item's `guint32` (074a2219
/// `phrase_index.cpp:169-175,345`).
#[test]
fn user_unigram_item_wraps_when_its_sub_index_total_accepts_the_add() {
    use oxpinyin_data::chunk_format::{CHUNK_HEADER_SIZE, build_memory_chunk};
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};

    let user = TempUserDir::new("user-unigram-wrap");
    let path = user.path.to_str().unwrap();
    let context = init_at(&user.path);
    assert!(!context.is_null());
    crate::context::pinyin_fini(context);
    let chunk = build_chunk(&[(
        1,
        ChunkItem {
            phrase: vec![u32::from('词')],
            unigram: u32::MAX - 2,
            prons: vec![],
        },
    )])
    .unwrap();
    let mut payload = chunk[CHUNK_HEADER_SIZE..].to_vec();
    payload[..4].copy_from_slice(&3_u32.to_le_bytes());
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&payload).unwrap(),
    )
    .unwrap();

    let (context, instance) = open(path);
    let token = FIRST_USER_TOKEN;
    assert_eq!(unigram_item_count(instance, token), (true, u32::MAX - 2));
    assert!(crate::dict::pinyin_token_add_unigram_frequency(
        instance, token, 4
    ));
    assert_eq!(unigram_item_count(instance, token), (true, 1));
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

/// At 074a2219, `pinyin.cpp:812-815` reads both live items each time a pair
/// loads. `:904-910` copies the buffered row before loading the next pair.
#[test]
fn bigram_export_reads_a_new_pronunciation_when_the_next_pair_loads() {
    for live_successor in [false, true] {
        use crate::iterators::{
            pinyin_begin_get_bigram_phrases, pinyin_bigram_iterator_get_next_phrase,
            pinyin_bigram_iterator_has_next_phrase, pinyin_end_get_bigram_phrases,
        };

        let user_dir = TempUserDir::new("export-live-item");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 5)]), [true]);
        let (user_token, system_token) = {
            // SAFETY: `instance` was just allocated and has not been freed.
            let inst = unsafe { instance_ref(instance) };
            let predecessor = inst
                .core
                .user
                .unwrap()
                .token_for_phrase("龘龘")
                .unwrap()
                .unwrap();
            (predecessor, inst.core.dict.tokens_for_text("好")[0].value())
        };
        let (predecessor, successor) = if live_successor {
            (system_token, user_token)
        } else {
            (user_token, system_token)
        };
        assert!(pinyin_save(context));
        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
        lay_gram(
            &user_dir.path,
            predecessor,
            &[(successor, 69), (successor, 70)],
        );

        let context = init_at(&user_dir.path);
        assert!(!context.is_null());
        let iter = pinyin_begin_get_bigram_phrases(context);
        assert!(!pinyin_bigram_iterator_has_next_phrase(iter));
        assert!(pinyin_bigram_iterator_has_next_phrase(iter));
        assert_eq!(import_batch(context, 7, &[("龘龘", "da4'da4", 5)]), [true]);

        let row = || {
            let (mut phrase, mut pinyin, mut count) = (ptr::null_mut(), ptr::null_mut(), 0);
            let more = pinyin_bigram_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut pinyin,
                &raw mut count,
            );
            ExportStep::Row(more, take_exported(phrase), take_exported(pinyin), count)
        };
        let expected = |more, reading: &str, count| {
            let (phrase, pinyin) = if live_successor {
                ("好龘龘", format!("hao'{reading}"))
            } else {
                ("龘龘好", format!("{reading}'hao"))
            };
            ExportStep::Row(more, phrase.to_owned(), pinyin, count)
        };
        assert_eq!(row(), expected(true, "da2'da2", 138));
        assert_eq!(row(), expected(true, "da2'da2", 140));
        assert_eq!(row(), expected(false, "da4'da4", 140));
        pinyin_end_get_bigram_phrases(iter);
        crate::context::pinyin_fini(context);
    }
}
