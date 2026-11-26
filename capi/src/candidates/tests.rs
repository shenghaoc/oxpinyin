#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::test_support::{
    IBUS_DEFAULT_SORT, NORMAL, PREDICTED_PUNCTUATION, Pair, user_row_of_dada,
};
use oxpinyin::model::Dictionary as _;

use crate::test_support::packed_key;
use crate::test_support::{alloc, close, init_at, open_at, parse, token_of};
use crate::test_support::{candidate_at, saved_user_phrase};

use crate::candidates::{
    pinyin_choose_candidate, pinyin_choose_predicted_candidate, pinyin_get_candidate,
    pinyin_get_candidate_string, pinyin_get_candidate_type, pinyin_remove_user_candidate,
    pinyin_train,
};

use crate::config::{pinyin_load_addon_phrase_library, pinyin_unload_phrase_library};

use crate::context::pinyin_init;

use crate::dict::{pinyin_token_add_unigram_frequency, pinyin_token_get_unigram_frequency};

use crate::test_support::{capture_rows, predicted_row, with_store};

use crate::instance::{pinyin_alloc_instance, pinyin_reset};

use crate::parse::pinyin_parse_more_full_pinyins;

use crate::sentence::{
    pinyin_get_sentence, pinyin_guess_candidates,
    pinyin_guess_predicted_candidates_with_punctuations, pinyin_guess_sentence,
};

use crate::state::{CapiCandidate, instance_mut, instance_ref};

use crate::test_support::{
    DEFAULT_SORT, TempSystemDir, TempUserDir, candidate, cstr, lookup, open, system_dir,
};

use crate::types::{LookupCandidate, PinyinContext, PinyinInstance, lookup_candidate_type_t};

use crate::test_support::promotion_system;

use oxpinyin::facade::TextSpan;

use oxpinyin::vocab::SENTENCE_START;

use std::cell::Cell;

use std::ffi::c_char;

use std::os::raw::c_uint;

use std::path::Path;

use std::ptr;

/// The first row of `kind` in the current list.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn row_of(
    instance: *mut PinyinInstance,
    kind: lookup_candidate_type_t,
) -> *mut LookupCandidate {
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    let index = unsafe { capture_rows(instance) }
        .iter()
        .position(|(found, _, _)| *found == kind)
        .unwrap_or_else(|| panic!("a {kind:?} row is listed"));
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    unsafe { candidate_at(instance, index) }
}

/// The sentence of row 0 after a fresh `pinyin_guess_sentence`, or `None`
/// when the guess or the read answers `false`, or the read returns NULL.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn sentence_after_guess(instance: *mut PinyinInstance) -> Option<String> {
    if !unsafe { pinyin_guess_sentence(instance) } {
        return None;
    }
    let mut sentence: *mut c_char = ptr::null_mut();
    if !unsafe { pinyin_get_sentence(instance, 0, &raw mut sentence) } || sentence.is_null() {
        return None;
    }
    // SAFETY: a NUL-terminated GLib string the caller owns.
    let text = unsafe { std::ffi::CStr::from_ptr(sentence) }
        .to_str()
        .expect("UTF-8 sentence")
        .to_owned();
    // SAFETY: freed once.
    unsafe { glib_sys::g_free(sentence.cast()) };
    Some(text)
}

/// The tokens the instance's selection record holds, which follow its
/// forcings.
///
/// # Safety
///
/// `instance` is a live instance, not NULL: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn selected_tokens(instance: *mut PinyinInstance) -> Vec<u32> {
    // SAFETY: the live instance is read between serialized ABI calls.
    unsafe { crate::state::instance_ref(instance) }
        .core
        .session
        .selected_tokens()
        .to_vec()
}

// Removing a real imported candidate must mutate the retained store even
// after the public handle is gone; no pin call is valid in that state.
#[rstest::rstest]
fn removing_a_user_candidate_keeps_the_instance_owner(#[values(false, true)] finalized: bool) {
    let user = TempUserDir::new("remove-retained-owner");
    let token = saved_user_phrase(&user.path);
    let (instance, candidate) = user_row_of_dada(&user.path);
    let context = unsafe { crate::instance::pinyin_get_context(instance) };
    if finalized {
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses `context` after this call; only this thread calls into
        // them.
        unsafe { crate::test_support::finalize_public_context(context, instance) };
    }
    assert!(unsafe { pinyin_remove_user_candidate(instance, candidate) });
    // SAFETY: the instance remains live; the removal's exclusive borrow ended.
    let inst = unsafe { crate::state::instance_ref(instance) };
    assert!(inst.core.user.phrase(token).is_none());
    unsafe { crate::instance::pinyin_free_instance(instance) };
    if !finalized {
        unsafe { crate::context::pinyin_fini(context) };
    }
}

/// a punctuation row leaves the composition's constraints intact.
#[rstest::rstest]
#[case::composing(crate::test_support::DEFAULT_SORT, false, 0, Some("你好"))]
#[case::after_reset(crate::test_support::DEFAULT_SORT, true, 0, None)]
#[case::sort_without_sentence(0x1, false, 1, Some("你好"))]
fn a_punctuation_row_returns_the_cursor_without_adding_a_constraint(
    #[case] sort: u32,
    #[case] reset: bool,
    #[case] answer: i32,
    #[case] after: Option<&str>,
) {
    let user = TempUserDir::new("choose-punctuation-normal");
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, sort) });
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { sentence_after_guess(instance) }.as_deref(),
        Some("你好")
    );
    if reset {
        assert!(unsafe { pinyin_reset(instance) });
    }
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, c"好".as_ptr())
    });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { row_of(instance, PREDICTED_PUNCTUATION) };
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, row) }, answer);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { selected_tokens(instance) }, Vec::<u32>::new());
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { sentence_after_guess(instance) }.as_deref(), after);
    assert_eq!(
        unsafe { crate::candidates::pinyin_train(instance, 0) },
        after.is_some()
    );
    assert!(!unsafe { crate::candidates::pinyin_clear_constraint(instance, 0) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A private mini system whose library 1 holds 甲`word`乙 over
/// da1'da1'da1 and 丙 over ba1, each at 100.
fn phrase_text_system(parent: &Path, word: u32) -> std::path::PathBuf {
    crate::test_support::items_system(
        &system_dir(),
        parent,
        &[
            (&[0x7532, word, 0x4e59], &[0x1084; 3], 100),
            (&[0x4e19], &[0x1081], 100),
        ],
        200,
    )
}

/// A saved handle chooses its stored token and span after a reparse of the
/// same input. Sentence reads resolve that token's live words, including
/// embedded zero words and words outside Unicode's scalar range.
#[rstest::rstest]
#[case::zero_word(0, "甲丙".as_bytes())]
#[case::above_unicode(0x11_0000, b"\xe7\x94\xb2\xf4\x90\x80\x80\xe4\xb9\x99\xe4\xb8\x99")]
fn a_stored_words_row_keeps_its_token_and_span_through_its_handle(
    #[case] word: u32,
    #[case] sentence: &[u8],
) {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = phrase_text_system(scratch.path(), word);
    let user = TempUserDir::new("stored-words-handle");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "da1da1da1ba1") }, 12);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { row_of(instance, NORMAL) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "da1da1da1ba1") }, 12);
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, row) }, 9);
    // SAFETY: the live instance is read between serialized ABI calls.
    assert_eq!(unsafe { selected_tokens(instance) }, [0x0100_0001]);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    let mut text = ptr::null_mut();
    assert!(unsafe { pinyin_get_sentence(instance, 0, &raw mut text) });
    // SAFETY: get_sentence returned an owned, NUL-terminated GLib string.
    assert_eq!(
        unsafe { std::ffi::CStr::from_ptr(text) }.to_bytes(),
        sentence
    );
    // SAFETY: the owned sentence is freed once after the last read.
    unsafe { glib_sys::g_free(text.cast()) };
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// `SORT_WITHOUT_SENTENCE_CANDIDATE`: the choose leg that only trains.
const WITHOUT_SENTENCE: c_uint = 0x1;

/// The open fixture, with `ni` parsed.
struct Open {
    /// The directories the context reads, removed after it is freed.
    _dirs: Box<dyn std::any::Any>,
    context: *mut PinyinContext,
    instance: *mut PinyinInstance,
}

impl Open {
    fn new(tag: &str) -> Self {
        let user = TempUserDir::new(tag);
        let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; the fixture takes both handles; only this thread calls into them.
        unsafe { Self::parsed(Box::new(user), context, instance) }
    }

    /// # Safety
    ///
    /// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not
    /// taken it.
    ///
    /// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
    /// `pinyin_free_instance` has not freed it.
    ///
    /// `instance` belongs to `context`. The fixture takes both and frees them when it drops.
    ///
    /// Calls are serial across the context, its sibling instances and its iterators: no other call
    /// through any of them, and no borrow of their state, overlaps this call (the crate's Serial
    /// calls rule).
    unsafe fn parsed(
        dirs: Box<dyn std::any::Any>,
        context: *mut PinyinContext,
        instance: *mut PinyinInstance,
    ) -> Self {
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, cstr("ni").as_ptr()) },
            2
        );
        Self {
            _dirs: dirs,
            context,
            instance,
        }
    }

    /// Guesses `ni`'s candidates under `sort` and answers the first row
    /// `wanted` accepts, with its token. `wanted` runs as in [`Self::find_row`].
    fn row_of(
        &self,
        sort: c_uint,
        wanted: impl Fn(lookup_candidate_type_t, u32) -> bool,
    ) -> (*mut LookupCandidate, u32) {
        assert!(unsafe { pinyin_guess_candidates(self.instance, 0, sort) });
        self.find_row(wanted)
    }

    /// The first row of the current list that `wanted` accepts, with its token.
    ///
    /// `wanted` runs while this method holds a shared borrow of the instance's state, so it must
    /// not call through the fixture's context, its instances or its iterators: any such call
    /// overlaps the borrow, which the crate's Serial calls rule forbids.
    fn find_row(
        &self,
        wanted: impl Fn(lookup_candidate_type_t, u32) -> bool,
    ) -> (*mut LookupCandidate, u32) {
        let (index, token) = {
            // SAFETY: live instance immediately after the guess.
            let inst = unsafe { instance_ref(self.instance) };
            inst.candidates
                .iter()
                .enumerate()
                .find_map(|(index, row)| {
                    let token = row.token.get()?;
                    wanted(row.candidate_type(), token).then_some((index, token))
                })
                .expect("the fixture holds a row of this kind")
        };
        let mut row = ptr::null_mut();
        assert!(unsafe {
            pinyin_get_candidate(
                self.instance,
                u32::try_from(index).expect("small index"),
                &raw mut row,
            )
        });
        (row, token)
    }

    /// The row of the GBK library (2) under `ni`, and its token.
    fn gbk_row(&self, sort: c_uint) -> (*mut LookupCandidate, u32) {
        self.row_of(sort, |_, token| token >> 24 == 2)
    }

    /// The total a system library's sub-index holds once it is loaded
    /// and before any write: `SubPhraseIndex::load` reads it from the
    /// chunk (`phrase_index.cpp:345` at the pin `074a2219`).
    fn loaded_total(&self, library: u8) -> u32 {
        // SAFETY: as in `store`.
        unsafe { instance_ref(self.instance) }
            .core
            .dict
            .system()
            .libraries()
            .library(u32::from(library) << 24)
            .map(oxpinyin::data::phrase_library::PhraseLibrary::total_freq)
            .expect("the library is mapped")
    }

    /// The facade total the model divides by.
    fn amplified_total(&self) -> u64 {
        // SAFETY: as in `store`.
        unsafe { instance_ref(self.instance) }
            .core
            .lm
            .amplified_total()
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        unsafe { crate::instance::pinyin_free_instance(self.instance) };
        unsafe { crate::context::pinyin_fini(self.context) };
    }
}

// Both unigram-only choose branches must use the instance's owner, not
// its finalized public context. Keep the ordinary lifetime as a control.
#[rstest::rstest]
#[case::longer(IBUS_DEFAULT_SORT, lookup_candidate_type_t::LONGER_CANDIDATE)]
#[case::sort_bit(DEFAULT_SORT | WITHOUT_SENTENCE, lookup_candidate_type_t::NORMAL_CANDIDATE)]
fn choosing_a_unigram_row_keeps_the_instance_owner(
    #[case] sort: c_uint,
    #[case] kind: lookup_candidate_type_t,
    #[values(false, true)] finalized: bool,
) {
    let mut open = Open::new("choose-retained-owner");
    assert!(unsafe { pinyin_guess_sentence(open.instance) });
    let (row, _) = open.row_of(sort, |candidate_kind, _| candidate_kind == kind);
    if finalized {
        // SAFETY: `open.context` and `open.instance` come from the `Open` fixture, which frees them
        // only when it drops at the end of the test; the test sets `open.context` to NULL right
        // after this call, so nothing uses that context again; only this thread calls into them.
        unsafe { crate::test_support::finalize_public_context(open.context, open.instance) };
        open.context = ptr::null_mut();
    }
    let before = open.amplified_total();
    assert_eq!(unsafe { pinyin_choose_candidate(open.instance, 0, row) }, 1);
    assert_eq!(open.amplified_total() - before, 483);
}

/// The `SORT_WITHOUT_SENTENCE_CANDIDATE` leg trains the row's unigram and
/// answers `true` unless the add overflows (`pinyin.cpp:2565-2576`). A
/// library with no sub-index answers `ERROR_NO_SUB_PHRASE_INDEX`, not the
/// overflow.
#[test]
fn choosing_a_row_of_an_unloaded_library_under_the_sort_bit_answers_one() {
    let open = Open::new("sort-bit-unloaded");
    let (row, _) = open.gbk_row(DEFAULT_SORT | WITHOUT_SENTENCE);
    assert!(unsafe { pinyin_unload_phrase_library(open.context, 2) });

    assert_eq!(unsafe { pinyin_choose_candidate(open.instance, 0, row) }, 1);
}

/// `ERROR_INTEGER_OVERFLOW` is the one error the leg answers `false` for.
#[test]
fn choosing_a_row_whose_unigram_add_overflows_under_the_sort_bit_answers_zero() {
    let open = Open::new("sort-bit-overflow");
    let (row, gbk) = open.gbk_row(DEFAULT_SORT | WITHOUT_SENTENCE);
    let total = open.loaded_total(2);
    assert!(unsafe { pinyin_token_add_unigram_frequency(open.instance, gbk, u32::MAX - total) });

    assert_eq!(unsafe { pinyin_choose_candidate(open.instance, 0, row) }, 0);
}

/// The LONGER leg (`pinyin.cpp:2523-2531`) answers as the sort-bit leg.
#[test]
fn choosing_a_longer_row_whose_unigram_add_overflows_answers_zero() {
    let open = Open::new("longer-overflow");
    assert!(unsafe { pinyin_guess_sentence(open.instance) });
    let (row, token) = open.row_of(IBUS_DEFAULT_SORT, |kind, _| {
        kind == lookup_candidate_type_t::LONGER_CANDIDATE
    });
    let library = u8::try_from(token >> 24).expect("a library nibble");
    let total = open.loaded_total(library);
    assert!(unsafe { pinyin_token_add_unigram_frequency(open.instance, token, u32::MAX - total) });

    assert_eq!(unsafe { pinyin_choose_candidate(open.instance, 0, row) }, 0);
}

/// `phonetic_lookup.h:904`: training stores into the failed Berkeley DB
/// user bigram container and the pin's `check_result` dies. Only
/// running out of memory makes that container, so the training answer
/// is handed to the refusal directly.
#[test]
fn a_store_into_the_failed_user_bigram_aborts_training() {
    let stderr = crate::test_support::assert_aborts(
        "candidates::tests::a_store_into_the_failed_user_bigram_aborts_training",
        || {
            super::train_answer(Err(oxpinyin::engine::EngineError::TrainingAbort));
        },
    );
    // No Rust panic text or other line: the pin writes none here but
    // glibc's assert line, the register's policy-accepted missing line.
    assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
}

/// `_get_previous_token(instance, 0)` (`pinyin.cpp:1711-1740`): a predicted
/// bigram is trained after the longest token of the prediction prefix, not
/// after `sentence_start`.
#[test]
fn a_predicted_bigram_trains_from_the_prediction_prefix() {
    use crate::predict::pinyin_guess_predicted_candidates;

    let user_dir = TempUserDir::new("predicted-prefix");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let prefix = cstr("你");
    let ni = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        inst.core
            .dict
            .tokens_for_ucs4(&"你".chars().map(u32::from).collect::<Vec<_>>())[0]
    };

    // A prefix choice raises only its unigram. Seed an existing bigram
    // explicitly to test the separate bigram-choice predecessor.
    assert!(unsafe { pinyin_guess_predicted_candidates(instance, prefix.as_ptr()) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let (prefix_row, word) = unsafe {
        predicted_row(
            instance,
            lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE,
        )
    };
    assert!(unsafe { pinyin_choose_predicted_candidate(instance, prefix_row) });
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { with_store(instance, |store| store.bigram_count(ni, word)) }.unwrap(),
        0
    );
    // SAFETY: the live context has no reader retained across this write.
    unsafe { crate::state::instance_context_mut(instance) }
        .core
        .user_store_mut()
        .observe_predicted(ni, word)
        .unwrap();

    assert!(unsafe { pinyin_guess_predicted_candidates(instance, prefix.as_ptr()) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let (bigram_row, bigram_word) = unsafe {
        predicted_row(
            instance,
            lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE,
        )
    };
    assert_eq!(bigram_word, word, "the bigram row is the trained word");
    // Parsing replaces the matrix but leaves the prediction and its
    // prefixes intact (074a2219, pinyin.cpp:1497-1525).
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("hao").as_ptr()) },
        3
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let before = unsafe { with_store(instance, |store| store.bigram_count(ni, word)) }.unwrap();
    let sentence_start_before =
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { with_store(instance, |store| store.bigram_count(SENTENCE_START, word)) }.unwrap();
    assert!(unsafe { pinyin_choose_predicted_candidate(instance, bigram_row) });
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { with_store(instance, |store| store.bigram_count(ni, word)) }.unwrap(),
        before + 69,
        "the predecessor is the prefix token"
    );
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { with_store(instance, |store| store.bigram_count(SENTENCE_START, word)) }.unwrap(),
        sentence_start_before,
        "not sentence_start"
    );

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// A previously published prefix row still chooses after its item disappears. The facade total takes the
/// fixed 483 (`pinyin.cpp:2608-2616`, `phrase_index.h:628-634`),
/// while the absent item is not written; an unloaded library leaves the
/// total alone and still answers true.
#[rstest::rstest]
fn choosing_an_existing_prefix_row_after_item_removal_advances_the_facade_total(
    #[values(false, true)] finalized: bool,
) {
    let user_dir = TempUserDir::new("missing-prefix-choose");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    if finalized {
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses `context` after this call; only this thread calls into
        // them.
        unsafe { crate::test_support::finalize_public_context(context, instance) };
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
        type_tag: Cell::new(lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE.tag()),
        nbest_index: 0,
        token: Cell::new(Some(token)),
        begin: 0,
        end: 0,
    });
    let candidate = crate::state::candidate_ptr(&inst.state.candidates[0]);
    let before = inst.core.lm.amplified_total();
    assert!(unsafe { pinyin_choose_predicted_candidate(instance, candidate) });
    // SAFETY: the C operation completed; re-borrow its live instance.
    let inst = unsafe { instance_ref(instance) };
    let after = inst.core.lm.amplified_total();
    assert_eq!(
        after - before,
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
    assert!(unsafe { pinyin_choose_predicted_candidate(instance, candidate) });
    // SAFETY: the C operation completed; re-borrow its live instance.
    let inst = unsafe { instance_ref(instance) };
    assert_eq!(
        inst.core.lm.amplified_total(),
        before,
        "an unloaded library has no facade total to advance"
    );

    unsafe { crate::instance::pinyin_free_instance(instance) };
    if !finalized {
        unsafe { crate::context::pinyin_fini(context) };
    }
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
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let _ = unsafe { candidate(instance, "nihao", 0) };
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
        assert!(unsafe {
            pinyin_get_candidate(
                instance,
                u32::try_from(index).expect("small index"),
                &raw mut original,
            )
        });
        assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, original) }, 2);
        // This forcing becomes invalid under "ma". Even a refused choose
        // must validate it away (phonetic_lookup.cpp:120-168 at 074a2219).
    }
    assert!(unsafe { pinyin_guess_candidates(instance, 2, DEFAULT_SORT) });
    let hao = {
        // SAFETY: live instance immediately after the guess.
        let inst = unsafe { instance_ref(instance) };
        let index = inst
            .candidates
            .iter()
            .position(|cd| cd.text_bytes() == "好".as_bytes())
            .expect("好 is offered at offset 2");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(unsafe {
            pinyin_get_candidate(
                instance,
                u32::try_from(index).expect("small candidate index"),
                &raw mut cand,
            )
        });
        cand
    };
    // SAFETY: `instance` was opened above and is not yet freed; `hao` is a row of a live instance's
    // current list, and no call since has rebuilt that list; only this thread calls into them.
    let expected_token = unsafe { token_of(instance, hao) };
    let expected_tokens = [expected_token];
    if let Some(input) = reparse {
        // At 074a2219 pinyin.cpp:1497-1525 leaves the candidate intact;
        // :2580-2587 chooses its saved token and span, not a new row index.
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, cstr(input).as_ptr()) },
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
        unsafe { pinyin_choose_candidate(instance, 2, hao) },
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
    assert!(!unsafe { crate::candidates::pinyin_clear_constraint(instance, 0) });
    if refused {
        assert_eq!(unsafe { pinyin_choose_candidate(instance, 2, hao) }, 2);
        assert!(!unsafe { crate::candidates::pinyin_clear_constraint(instance, 2) });
        unsafe { crate::instance::pinyin_free_instance(instance) };
        unsafe { crate::context::pinyin_fini(context) };
        return;
    }

    assert!(unsafe { crate::candidates::pinyin_clear_constraint(instance, 2) });
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn unigram_or_zero(instance: *mut PinyinInstance, token: u32) -> u32 {
    let mut freq = 0;
    unsafe { pinyin_token_get_unigram_frequency(instance, token, &raw mut freq) };
    freq
}

/// `pinyin_choose_predicted_candidate` reads the row's type and token and
/// trains the receiving instance's context with the receiver's own
/// prefixes (`pinyin.cpp:2591-2640`, `_get_previous_token`, `:1711-1740`):
/// a prefix row raises the receiver's unigram by 483, and a bigram row
/// also raises `(receiver's prefix → token)` by 69.
#[rstest::rstest]
fn a_predicted_row_from_another_instance_trains_the_receiver(
    #[values(true, false)] same_context: bool,
) {
    use crate::candidates::pinyin_choose_predicted_candidate;
    use crate::predict::pinyin_guess_predicted_candidates;
    use crate::test_support::{predicted_row, with_store};

    let pair = Pair::open(same_context);
    // SAFETY: `pair.receiver()` comes from the `Pair` fixture, which frees its handles only when
    // `pair` drops at the end of the test; only this thread calls into it.
    let ni = *unsafe { lookup(pair.receiver(), "你") }
        .first()
        .expect("你");
    // SAFETY: `pair.receiver()` comes from the `Pair` fixture, which frees its handles only when
    // `pair` drops at the end of the test; only this thread calls into it.
    let hao = *unsafe { lookup(pair.receiver(), "好") }
        .first()
        .expect("好");
    assert!(unsafe { pinyin_guess_predicted_candidates(pair.producer(), cstr("你").as_ptr()) });
    // SAFETY: `pair.producer()` comes from the `Pair` fixture, which frees its handles only when
    // `pair` drops at the end of the test; only this thread calls into it.
    let (prefix_row, word) = unsafe {
        predicted_row(
            pair.producer(),
            lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE,
        )
    };
    let (receiver_before, producer_before) =
        // SAFETY: `pair.receiver()` comes from the `Pair` fixture, which frees its handles only when
        // `pair` drops at the end of the test; `pair.producer()` comes from the `Pair` fixture, which
        // frees its handles only when `pair` drops at the end of the test; only this thread calls
        // into them.
        (unsafe { unigram_or_zero(pair.receiver(), word) }, unsafe {
            unigram_or_zero(pair.producer(), word)
        });
    assert!(unsafe { pinyin_choose_predicted_candidate(pair.receiver(), prefix_row) });
    assert_eq!(
        // SAFETY: `pair.receiver()` comes from the `Pair` fixture, which frees its handles only when
        // `pair` drops at the end of the test; only this thread calls into it.
        unsafe { unigram_or_zero(pair.receiver(), word) },
        receiver_before + 483
    );
    if !same_context {
        assert_eq!(
            // SAFETY: `pair.producer()` comes from the `Pair` fixture, which frees its handles only
            // when `pair` drops at the end of the test; only this thread calls into it.
            unsafe { unigram_or_zero(pair.producer(), word) },
            producer_before
        );
    }

    // Seed both predecessor buckets. The producer holds 你 and the
    // receiver holds 好, including when they share one context.
    for &instance in &[pair.producer(), pair.receiver()] {
        // SAFETY: the live context has no reader retained across this write.
        let context = unsafe { crate::state::instance_context_mut(instance) };
        let store = context.core.user_store_mut();
        store.observe_predicted(ni, word).unwrap();
        store.observe_predicted(hao, word).unwrap();
    }
    assert!(unsafe { pinyin_guess_predicted_candidates(pair.producer(), cstr("你").as_ptr()) });
    // SAFETY: `pair.producer()` comes from the `Pair` fixture, which frees its handles only when
    // `pair` drops at the end of the test; only this thread calls into it.
    let (bigram_row, bigram_word) = unsafe {
        predicted_row(
            pair.producer(),
            lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE,
        )
    };
    assert_eq!(bigram_word, word);
    assert!(unsafe { pinyin_guess_predicted_candidates(pair.receiver(), cstr("好").as_ptr()) });
    let before =
        // SAFETY: `pair.receiver()` comes from the `Pair` fixture, which frees its handles only when
        // `pair` drops at the end of the test; only this thread calls into it.
        unsafe { with_store(pair.receiver(), |store| store.bigram_count(hao, word)) }.unwrap();
    // SAFETY: the receiver is live and all calls in this test are serial.
    let other_predecessor_before =
        unsafe { with_store(pair.receiver(), |store| store.bigram_count(ni, word)) }.unwrap();
    assert!(unsafe { pinyin_choose_predicted_candidate(pair.receiver(), bigram_row) });
    assert_eq!(
        // SAFETY: `pair.receiver()` comes from the `Pair` fixture, which frees its handles only when
        // `pair` drops at the end of the test; only this thread calls into it.
        unsafe { with_store(pair.receiver(), |store| store.bigram_count(hao, word)) }.unwrap(),
        before + 69
    );
    assert_eq!(
        // SAFETY: `pair.receiver()` comes from the `Pair` fixture, which frees its handles only when
        // `pair` drops at the end of the test; only this thread calls into it.
        unsafe { with_store(pair.receiver(), |store| store.bigram_count(ni, word)) }.unwrap(),
        other_predecessor_before
    );
    if !same_context {
        assert_eq!(
            // SAFETY: `pair.producer()` comes from the `Pair` fixture, which frees its handles only
            // when `pair` drops at the end of the test; only this thread calls into it.
            unsafe { with_store(pair.producer(), |store| store.bigram_count(ni, word)) }.unwrap(),
            69
        );
        assert_eq!(
            // SAFETY: `pair.producer()` comes from the `Pair` fixture, which frees its handles only
            // when `pair` drops at the end of the test; only this thread calls into it.
            unsafe { with_store(pair.producer(), |store| store.bigram_count(hao, word)) }.unwrap(),
            69
        );
    }
}

#[rstest::rstest]
fn chosen_addon_candidate_is_promoted_into_default_nibble_5(
    #[values(false, true)] finalized: bool,
) {
    let system = TempSystemDir::new("addon-promote");
    system.write(
        "interpolation2.text",
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    );
    let user = TempUserDir::new("addon-promote-user");
    let context = unsafe {
        pinyin_init(
            cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
            cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
        )
    };
    assert!(!context.is_null());
    assert!(unsafe { pinyin_load_addon_phrase_library(context, 4) });

    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    let input = cstr("erhuang");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        7
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });

    // Find the addon candidate: its snapshot index and display text.
    let (index, text) = {
        // SAFETY: live instance; snapshot valid until the next guess.
        let inst = unsafe { instance_ref(instance) };
        let (index, cand) = inst
            .candidates
            .iter()
            .enumerate()
            .find(|(_, c)| c.candidate_type() == lookup_candidate_type_t::ADDON_CANDIDATE)
            .expect("art addon offers an ADDON candidate for erhuang");
        (index, cand.text_str().expect("UTF-8 text").to_owned())
    };

    // Choose it through the C ABI: this must promote it into default nibble 5.
    let mut cand: *mut crate::types::LookupCandidate = std::ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &raw mut cand,
        )
    });
    assert!(!cand.is_null());
    if finalized {
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses `context` after this call; only this thread calls into
        // them.
        unsafe { crate::test_support::finalize_public_context(context, instance) };
    }
    assert!(unsafe { pinyin_choose_candidate(instance, 0, cand) } > 0);
    let mut kind = lookup_candidate_type_t::ADDON_CANDIDATE;
    assert!(unsafe { pinyin_get_candidate_type(instance, cand, &raw mut kind) });
    assert_eq!(kind, lookup_candidate_type_t::NORMAL_CANDIDATE);
    let mut rendered = std::ptr::null();
    assert!(unsafe { pinyin_get_candidate_string(instance, cand, &raw mut rendered) });
    assert!(!rendered.is_null());
    // SAFETY: the original candidate still owns a terminated display string.
    let rendered = unsafe { std::ffi::CStr::from_ptr(rendered) };
    assert_eq!(rendered.to_bytes(), text.as_bytes());

    // The snapshot candidate is rewritten to a NORMAL candidate at a nibble-5
    // token (`pinyin.cpp:2559-2560`).
    let promoted = {
        // SAFETY: snapshot untouched since the choose (no intervening guess).
        let inst = unsafe { instance_ref(instance) };
        let snapshot = &inst.state.candidates[index];
        assert_eq!(
            snapshot.candidate_type(),
            lookup_candidate_type_t::NORMAL_CANDIDATE,
            "the chosen addon candidate becomes NORMAL"
        );
        let token = snapshot
            .token
            .get()
            .expect("promoted candidate carries a token");
        assert_eq!(
            oxpinyin::vocab::phrase_index_library_index(token),
            oxpinyin::vocab::ADDON_DICTIONARY,
            "promoted token lives in default nibble 5"
        );
        token
    };

    // The user store now carries the phrase under nibble 5, with a reading.
    // SAFETY: live instance.
    let inst = unsafe { instance_ref(instance) };
    let store = inst.core.user;
    assert_eq!(
        store
            .token_for_phrase_in(oxpinyin::vocab::ADDON_DICTIONARY, &text)
            .expect("store read"),
        Some(promoted),
        "the promoted phrase resolves under nibble 5"
    );
    let phrase = store.phrase(promoted).expect("promoted phrase exists");
    assert_eq!(
        phrase.words(),
        text.chars().map(u32::from).collect::<Vec<_>>()
    );
    let reading = phrase
        .pronunciations()
        .first()
        .expect("promoted phrase copied at least one reading");

    // It surfaces through the default-facade user lookup as a normal candidate.
    let keys: Vec<oxpinyin::vocab::SyllableKey> = reading
        .keys()
        .iter()
        .map(|k| oxpinyin::vocab::SyllableKey::from_index(usize::from(*k)).expect("stored key"))
        .collect();
    let lookup = store.lookup();
    assert!(
        lookup
            .lookup(&keys)
            .iter()
            .any(|entry| entry.token() == promoted && entry.text().as_str() == Some(text.as_str())),
        "promoted phrase is found through the default-facade lookup"
    );

    unsafe { crate::instance::pinyin_free_instance(instance) };
    if !finalized {
        unsafe { crate::context::pinyin_fini(context) };
    }
}

/// An addon row's choose is refused after a shortening reparse, once the
/// promotion has run; the full input is parsed again and the same handle is
/// chosen again. The pin's first choose rewrote the row to its promoted
/// `m_token` in place (`pinyin.cpp:2559-2561`), so the second forces that
/// token over the row's span and answers `0 + 7` (`:2577-2590`), and
/// `pinyin_train` adds `69 * 7 = 483` to its unigram: the bigram from
/// `sentence_start` is new, so the seed is the initial one
/// (`phonetic_lookup.h:858-929`).
#[test]
fn a_repeat_choose_of_a_refused_promoted_row_forces_the_promoted_token() {
    const ADDON: u32 = 0x0400_0001;
    const PROMOTED: u32 = 0x0500_0001;
    let system = TempSystemDir::new("promoted-repeat");
    system.write(
        "interpolation2.text",
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    );
    let user = TempUserDir::new("promoted-repeat-user");
    let context = unsafe {
        pinyin_init(
            cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
            cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
        )
    };
    assert!(!context.is_null());
    assert!(unsafe { pinyin_load_addon_phrase_library(context, 4) });
    let instance = unsafe { pinyin_alloc_instance(context) };
    let input = cstr("erhuang");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        7
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let index = {
        // SAFETY: live instance; snapshot valid until the next guess.
        let inst = unsafe { instance_ref(instance) };
        inst.candidates
            .iter()
            .position(|c| {
                c.candidate_type() == lookup_candidate_type_t::ADDON_CANDIDATE
                    && c.token.get() == Some(ADDON)
            })
            .expect("the 二簧 ADDON row")
    };
    let mut cand: *mut crate::types::LookupCandidate = std::ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small"),
            &raw mut cand,
        )
    });
    let short = cstr("er");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, short.as_ptr()) },
        2
    );
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, cand) }, 0);
    // SAFETY: as above.
    let promoted = unsafe { instance_ref(instance) }.state.candidates[index]
        .token
        .get();
    assert_eq!(promoted, Some(PROMOTED));
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        7
    );
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, cand) }, 7);
    // SAFETY: as above.
    let tokens: Vec<u32> = unsafe { instance_ref(instance) }
        .core
        .session
        .selected_tokens()
        .to_vec();
    assert_eq!(tokens, [PROMOTED]);
    assert!(unsafe { crate::sentence::pinyin_guess_sentence(instance) });
    let mut sentence: *mut std::ffi::c_char = std::ptr::null_mut();
    assert!(unsafe { crate::sentence::pinyin_get_sentence(instance, 0, &raw mut sentence) });
    // SAFETY: a NUL-terminated GLib string the caller owns.
    let text = unsafe { std::ffi::CStr::from_ptr(sentence) }
        .to_str()
        .expect("UTF-8")
        .to_owned();
    // SAFETY: freed once.
    unsafe { glib_sys::g_free(sentence.cast()) };
    assert_eq!(text, "二簧");
    let unigram = |token: u32| {
        let mut freq = 0;
        let found = unsafe {
            crate::dict::pinyin_token_get_unigram_frequency(instance, token, &raw mut freq)
        };
        (found, freq)
    };
    assert_eq!(unigram(PROMOTED), (true, 1));
    assert!(unsafe { crate::candidates::pinyin_train(instance, 0) });
    assert_eq!(unigram(PROMOTED), (true, 1 + 483));
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// With row 5 `NOT_USED`, the pin reads an unwritten token (074a2219
/// phrase_index.cpp:610-613, pinyin.cpp:2541-2545). The defined outcome
/// leaves the addon candidate and default sub-indices as they were.
#[test]
fn choosing_an_addon_without_sub_index_5_skips_promotion() {
    let system = TempSystemDir::new("addon-promote-not-used");
    let conf = std::fs::read_to_string(system.path.join("table.conf")).expect("table.conf");
    let row = "default ADDON_DICTIONARY NULL NULL addon.bin USER_FILE";
    assert!(conf.contains(row));
    system.write(
        "table.conf",
        &conf.replace(row, "default ADDON_DICTIONARY NULL NULL NULL NOT_USED"),
    );
    system.write(
        "interpolation2.text",
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    );
    let user = TempUserDir::new("addon-promote-not-used-user");
    let context = unsafe {
        pinyin_init(
            cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
            cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
        )
    };
    assert!(!context.is_null());
    assert!(unsafe { pinyin_load_addon_phrase_library(context, 4) });
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    let input = cstr("erhuang");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        7
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let (index, token, text, keys, total) = {
        // SAFETY: live instance; the snapshot is valid until the next guess.
        let inst = unsafe { instance_ref(instance) };
        let store = inst.core.user;
        assert_eq!(store.addon_promotion_range_end(), None);
        let (index, cand) = inst
            .candidates
            .iter()
            .enumerate()
            .find(|(_, c)| c.candidate_type() == lookup_candidate_type_t::ADDON_CANDIDATE)
            .expect("art addon offers an ADDON candidate for erhuang");
        let token = cand.token.get().expect("addon token");
        let item = inst.core.dict.addon_phrase_item(token).expect("addon item");
        let view = oxpinyin::data::phrase_library::PhraseItemView::from_bytes(&item)
            .expect("addon item bytes");
        let keys: Vec<_> = view
            .pronunciation(0)
            .expect("first reading")
            .keys
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                let packed = oxpinyin::parser::ChewingKey::from_packed(u16::from_le_bytes(*pair));
                oxpinyin::vocab::SyllableKey::from_text(packed.pinyin_spelling())
                    .expect("stored key")
            })
            .collect();
        (
            index,
            token,
            cand.text_str().expect("addon text").to_owned(),
            keys,
            store.unigram_total(),
        )
    };
    let mut cand = std::ptr::null_mut();
    assert!(unsafe { pinyin_get_candidate(instance, index.try_into().unwrap(), &raw mut cand) });
    assert!(unsafe { pinyin_choose_candidate(instance, 0, cand) } > 0);

    let mut kind = lookup_candidate_type_t::NORMAL_CANDIDATE;
    assert!(unsafe { pinyin_get_candidate_type(instance, cand, &raw mut kind) });
    assert_eq!(kind, lookup_candidate_type_t::ADDON_CANDIDATE);
    {
        // SAFETY: the live instance still owns this snapshot.
        let inst = unsafe { instance_ref(instance) };
        assert_eq!(inst.state.candidates[index].token.get(), Some(token));
        let store = inst.core.user;
        assert_eq!(store.addon_promotion_range_end(), None);
        assert!(
            store
                .phrase_table_tokens_for_text(&text)
                .unwrap()
                .is_empty()
        );
        assert!(store.lookup().lookup(&keys).is_empty());
        assert!(store.phrases().is_empty());
        assert_eq!(store.unigram_total(), total);
        assert!(!store.is_modified());
    }
    assert!(!unsafe { crate::context::pinyin_save(context) });
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
    let files: Vec<_> = std::fs::read_dir(&user.path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files, [std::ffi::OsString::from("user.conf")]);
}

/// Forces a phrase after the last sentence lookup that the decoded result
/// does not hold: the stale-forcing shape of `pinyin_train`.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn force_a_stale_phrase(instance: *mut PinyinInstance) -> u32 {
    let input = cstr("zhongguo");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        "zhongguo".len()
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) });
    // The last word row: not the phrase the sentence decoded at 0.
    let index = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        inst.state
            .candidates
            .iter()
            .rposition(|c| {
                c.candidate_type() == lookup_candidate_type_t::NORMAL_CANDIDATE
                    && c.token.get().is_some()
            })
            .expect("a word candidate")
    };
    let mut other: *mut LookupCandidate = ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small index"),
            &raw mut other,
        )
    });
    // SAFETY: `instance` meets this function's `# Safety` contract; `other` is a row of a live
    // instance's current list, and no call since has rebuilt that list; only this thread calls into
    // them.
    let forced = unsafe { token_of(instance, other) };
    assert!(unsafe { pinyin_choose_candidate(instance, 0, other) } > 0);
    forced
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
    // "ni": the mini fixture holds two strictly longer extensions
    // ("ni'hao" 你好, "ni'men" 你们 — `MINI_KEYS`), and the walk's
    // 2-key cap admits both; the winner is the max-unigram one.
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, cstr("ni").as_ptr()) },
        2,
        "full input parses"
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, IBUS_DEFAULT_SORT) });

    // Surface the type-7 row.
    let mut longer: *mut LookupCandidate = std::ptr::null_mut();
    let mut n: c_uint = 0;
    assert!(unsafe { crate::candidates::pinyin_get_n_candidate(instance, &raw mut n) });
    let mut longer_index = usize::MAX;
    for i in 0..n {
        let mut cand: *mut LookupCandidate = std::ptr::null_mut();
        assert!(unsafe { crate::candidates::pinyin_get_candidate(instance, i, &raw mut cand) });
        let mut ctype: lookup_candidate_type_t = lookup_candidate_type_t::NORMAL_CANDIDATE;
        assert!(unsafe {
            crate::candidates::pinyin_get_candidate_type(instance, cand, &raw mut ctype)
        });
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
            .get()
            .expect("a LONGER row carries its token")
    };

    let shipped =
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { with_store(instance, |store| store.system_unigram(token)) }.expect("item");

    // Choose: cursor 1, exactly the pin's LONGER branch.
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, longer) }, 1);

    // +483 through the canonical phrase-index count rows; no duplicate
    // user-bigram row.
    {
        assert_eq!(
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { with_store(instance, |store| store.system_unigram(token)) },
            Some(shipped + 483)
        );
    }
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { with_store(instance, |store| store.bigram_total(SENTENCE_START)) }.unwrap(),
        0
    );

    // The train that follows leans on the live sentence result and
    // writes nothing more: the stored count stays at the choose's 483 and no
    // bigram row appears (the pin's constraint-free `train_result3`
    // observes nothing).
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_train(instance, 0) });
    {
        assert_eq!(
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { with_store(instance, |store| store.system_unigram(token)) },
            Some(shipped + 483)
        );
    }
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { with_store(instance, |store| store.bigram_total(SENTENCE_START)) }.unwrap(),
        0
    );

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// `train_result3`'s `assert(token == constraint->m_token)`
/// (`phonetic_lookup.h:868`): a phrase forced after the last sentence lookup
/// no longer matches the decoded result, and the process aborts as the
/// pin's does.
#[test]
fn training_a_stale_forcing_aborts() {
    crate::test_support::assert_aborts(
        "candidates::tests::training_a_stale_forcing_aborts",
        || {
            let user_dir = TempUserDir::new("train-stale");
            let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            unsafe { force_a_stale_phrase(instance) };
            unsafe { pinyin_train(instance, 0) };
        },
    );
}

/// Chooses the only addon row of `input` and answers the context, the
/// instance and the promoted token.
fn choose_addon_row(
    system: &std::path::Path,
    user: &std::path::Path,
    input: &str,
) -> (
    *mut crate::types::PinyinContext,
    *mut crate::types::PinyinInstance,
    u32,
) {
    let context = unsafe {
        pinyin_init(
            cstr(system.to_str().expect("UTF-8 path")).as_ptr(),
            cstr(user.to_str().expect("UTF-8 path")).as_ptr(),
        )
    };
    assert!(!context.is_null());
    assert!(unsafe { pinyin_load_addon_phrase_library(context, 4) });
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    let text = cstr(input);
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) },
        input.len()
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let index = {
        // SAFETY: live instance; the snapshot is valid until the next guess.
        let inst = unsafe { instance_ref(instance) };
        inst.candidates
            .iter()
            .position(|c| c.candidate_type() == lookup_candidate_type_t::ADDON_CANDIDATE)
            .expect("the crafted addon offers an ADDON candidate")
    };
    let mut cand: *mut crate::types::LookupCandidate = std::ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &raw mut cand,
        )
    });
    assert!(unsafe { pinyin_choose_candidate(instance, 0, cand) } > 0);
    let mut kind = lookup_candidate_type_t::ADDON_CANDIDATE;
    assert!(unsafe { pinyin_get_candidate_type(instance, cand, &raw mut kind) });
    assert_eq!(kind, lookup_candidate_type_t::NORMAL_CANDIDATE);
    // `pinyin.cpp:2561-2562`: the promoted token is the new default token.
    let promoted = {
        // SAFETY: no guess since the choose.
        let inst = unsafe { instance_ref(instance) };
        inst.state.candidates[index]
            .token
            .get()
            .expect("the rewritten candidate has a token")
    };
    (context, instance, promoted)
}

/// The tokens `pinyin_lookup_tokens` finds for `phrase`.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn looked_up_tokens(instance: *mut crate::types::PinyinInstance, phrase: &str) -> Vec<u32> {
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    let text = cstr(phrase);
    let found = unsafe { crate::dict::pinyin_lookup_tokens(instance, text.as_ptr(), array.cast()) };
    // SAFETY: a live GArray of four-byte elements.
    let tokens = unsafe {
        let view = &*array;
        if view.len == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(view.data.cast::<u32>(), view.len as usize).to_vec()
        }
    };
    assert_eq!(found, !tokens.is_empty());
    // SAFETY: allocated above, freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    tokens
}

/// Promotion copies each stored reading with its tones
/// (074a2219 `pinyin.cpp:2547-2558`): the promoted token answers the same
/// packed words the addon item stores.
#[test]
fn a_promoted_addon_item_keeps_every_toned_reading() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = promotion_system(scratch.path());
    let user = TempUserDir::new("addon-promote-toned");
    let (context, instance, promoted) = choose_addon_row(&system, &user.path, "erhuang");
    assert_eq!(
        oxpinyin::vocab::phrase_index_library_index(promoted),
        oxpinyin::vocab::ADDON_DICTIONARY
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { looked_up_tokens(instance, "二簧") }, [promoted]);

    let mut count = 0;
    assert!(unsafe {
        crate::dict::pinyin_token_get_n_pronunciation(instance, promoted, &raw mut count)
    });
    assert_eq!(count, 2);
    let stored = [
        [packed_key("er", 2), packed_key("huang", 2)],
        [packed_key("er", 4), packed_key("huang", 1)],
    ];
    for (nth, keys) in stored.iter().enumerate() {
        // SAFETY: a fresh GArray of two-byte elements, freed below.
        let array = unsafe { glib_sys::g_array_new(0, 0, 2) };
        assert!(unsafe {
            crate::dict::pinyin_token_get_nth_pronunciation(
                instance,
                promoted,
                u32::try_from(nth).expect("small index"),
                array.cast(),
            )
        });
        // SAFETY: a live GArray of two-byte elements.
        let words = unsafe {
            let view = &*array;
            std::slice::from_raw_parts(view.data.cast::<u16>(), view.len as usize).to_vec()
        };
        // SAFETY: allocated above, freed once.
        unsafe { glib_sys::g_array_free(array, 1) };
        assert_eq!(words, keys, "reading {nth} keeps its tones");
    }
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// A user pinyin-index record whose token has high bits set. The table
/// files it under its masked library (`chewing_large_table2.h:107`), the
/// candidate keeps its value (`pinyin.cpp:1784`) and is a user candidate
/// (`pinyin.cpp:3719`). Training stores the gram item under that value
/// with the first seed `23 * 3` and adds `69 * 7 = 483` to the unigram of
/// the item its masked library and slot name
/// (`phonetic_lookup.h:856-927`, `phrase_index.h:629`).
#[test]
fn a_candidate_under_high_bits_is_listed_and_trains_its_item() {
    let user = TempUserDir::new("tagged-candidate");
    let token = saved_user_phrase(&user.path);
    let path = user.path.join("user_pinyin_index.bin");
    let mut rows = crate::test_support::store_rows(&path, false);
    let mut retagged = 0;
    for (_, value) in &mut rows {
        for at in 0..value.len().saturating_sub(3) {
            if value[at..at + 4] == token.to_le_bytes() {
                value[at..at + 4].copy_from_slice(&(token | 0x8000_0000).to_le_bytes());
                retagged += 1;
            }
        }
    }
    assert!(retagged > 0, "the index holds the token");
    crate::test_support::craft_user_index(&path, &rows);
    let (instance, candidate) = user_row_of_dada(&user.path);
    let context = unsafe { crate::instance::pinyin_get_context(instance) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let (found, base) = unsafe { crate::test_support::unigram(instance, token) };
    assert!(found);
    assert!(unsafe { pinyin_choose_candidate(instance, 0, candidate) } > 0);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { crate::candidates::pinyin_train(instance, 0) });
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { crate::test_support::unigram(instance, token) },
        (true, base + 483)
    );
    {
        // SAFETY: the instance is live and this read ends before the close.
        let inst = unsafe { crate::state::instance_ref(instance) };
        assert_eq!(
            inst.core.user.bigram_successors(1).expect("the start gram"),
            [(token | 0x8000_0000, 69)]
        );
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// Choosing a retained pointer twice does not invalidate its handle.
#[test]
fn an_accepted_candidate_pointer_can_be_chosen_twice() {
    let user = TempUserDir::new("repeat-choose-handle");
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { crate::test_support::row_with(instance, NORMAL, "你") };
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, row) }, 2);
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, row) }, 2);
    let mut text = ptr::null();
    assert!(unsafe { pinyin_get_candidate_string(instance, row, &raw mut text) });
    assert_eq!(
        // SAFETY: the getter borrows the retained row, still live after both chooses.
        unsafe { std::ffi::CStr::from_ptr(text) }.to_bytes(),
        "你".as_bytes()
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// a punctuation row leaves the sentence candidate available.
#[test]
fn a_normal_choose_after_punctuation_dispatches_the_retained_span() {
    let user = TempUserDir::new("punctuation-normal-dispatch");
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, c"好".as_ptr())
    });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let punctuation = unsafe { row_of(instance, PREDICTED_PUNCTUATION) };
    assert_eq!(
        unsafe { pinyin_choose_candidate(instance, 0, punctuation) },
        0
    );
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    assert!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { capture_rows(instance) }
            .iter()
            .any(|(kind, _, _)| *kind == lookup_candidate_type_t::NBEST_MATCH_CANDIDATE)
    );
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let normal = unsafe { crate::test_support::row_with(instance, NORMAL, "你") };
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, normal) }, 2);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}
