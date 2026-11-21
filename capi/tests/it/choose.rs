//! Choosing a candidate, the candidate-type asserts and removing a user
//! candidate.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    DEFAULT_SORT, LookupCandidate, PinyinContext, PinyinInstance, TempUserDir, assert_aborts, cstr,
    gram_bytes, open, pinyin_choose_candidate, pinyin_choose_predicted_candidate,
    pinyin_get_candidate, pinyin_get_candidate_nbest_index, pinyin_guess_candidates,
    pinyin_guess_predicted_candidates_with_punctuations, pinyin_guess_sentence,
    pinyin_remove_user_candidate, *,
};

use std::ptr;

/// Parses `nihao`, guesses the sentence and the candidates at 0 with sort 0.
///
/// # Safety
///
/// `context` is NULL or a live context: `pinyin_init` returned it and `pinyin_fini` has not taken
/// it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn guessed_nihao(context: *mut PinyinContext) -> *mut PinyinInstance {
    // SAFETY: `context` meets this function's `# Safety` contract; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    instance
}

/// `choose_candidate` asserts on a predicted prefix row.
#[test]
fn choose_candidate_aborts_on_a_predicted_prefix_row() {
    assert_aborts(
        "choose::choose_candidate_aborts_on_a_predicted_prefix_row",
        || {
            let user = TempUserDir::new("choose-prefix-row");
            let (_context, instance) = open_at(&user.path);
            assert!(unsafe {
                pinyin_guess_predicted_candidates_with_punctuations(instance, c"你".as_ptr())
            });
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
            let row = unsafe { row_of(instance, PREDICTED_PREFIX) };
            // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
            unsafe { pinyin_choose_candidate(instance, 0, row) };
        },
    );
}

/// `choose_candidate` asserts on a predicted bigram row: the first half of
/// `assert(PREDICTED_BIGRAM_CANDIDATE != type && PREDICTED_PREFIX_CANDIDATE
/// != type)` (`pinyin.cpp:2507-2508`). The row comes from a user gram of 你.
#[test]
fn choose_candidate_aborts_on_a_predicted_bigram_row() {
    assert_aborts(
        "choose::choose_candidate_aborts_on_a_predicted_bigram_row",
        || {
            let user = TempUserDir::new("choose-bigram-row");
            let (context, instance) = open_at(&user.path);
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
            let prefixes = unsafe { lookup(instance, "你") };
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
            let successor = unsafe { lookup(instance, "好") }[0];
            // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above
            // and is not yet freed; nothing uses either handle after this call; only this thread
            // calls into them.
            unsafe { close(context, instance) };
            let gram = gram_bytes(20, &[(successor, 20)]);
            let rows: Vec<_> = prefixes
                .iter()
                .map(|token| (token.to_le_bytes().to_vec(), gram.clone()))
                .collect();
            replace_user_bigram(&user.path, &rows);
            let (_context, instance) = open_at(&user.path);
            assert!(unsafe {
                pinyin_guess_predicted_candidates_with_punctuations(instance, c"你".as_ptr())
            });
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
            let row = unsafe { row_of(instance, PREDICTED_BIGRAM) };
            // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
            unsafe { pinyin_choose_candidate(instance, 0, row) };
        },
    );
}

/// `choose_predicted_candidate` asserts on a NORMAL row.
#[test]
fn choose_predicted_candidate_aborts_on_a_normal_row() {
    assert_aborts(
        "choose::choose_predicted_candidate_aborts_on_a_normal_row",
        || {
            let user = TempUserDir::new("choose-predicted-normal");
            let (context, _) = open_at(&user.path);
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            let instance = unsafe { guessed_nihao(context) };
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
            let row = unsafe { row_of(instance, NORMAL) };
            // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
            unsafe { pinyin_choose_predicted_candidate(instance, row) };
        },
    );
}

/// `get_candidate_nbest_index` asserts on a NORMAL row.
#[test]
fn nbest_index_aborts_on_a_normal_row() {
    assert_aborts("choose::nbest_index_aborts_on_a_normal_row", || {
        let user = TempUserDir::new("nbest-normal");
        let (context, _) = open_at(&user.path);
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let instance = unsafe { guessed_nihao(context) };
        let mut index = 0xAB_u8;
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let row = unsafe { row_of(instance, NORMAL) };
        // SAFETY: the row still belongs to this snapshot and index is a writable local slot.
        unsafe { pinyin_get_candidate_nbest_index(instance, row, &raw mut index) };
    });
}

/// `remove_user_candidate` asserts on a sentence row.
#[test]
fn remove_user_candidate_aborts_on_a_sentence_row() {
    assert_aborts(
        "choose::remove_user_candidate_aborts_on_a_sentence_row",
        || {
            let user = TempUserDir::new("remove-sentence-row");
            let (context, _) = open_at(&user.path);
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            let instance = unsafe { guessed_nihao(context) };
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
            let row = unsafe { row_of(instance, NBEST) };
            // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
            unsafe { pinyin_remove_user_candidate(instance, row) };
        },
    );
}

/// `remove_user_candidate` asserts on a NORMAL system token.
#[test]
fn remove_user_candidate_aborts_on_a_system_token() {
    assert_aborts(
        "choose::remove_user_candidate_aborts_on_a_system_token",
        || {
            let user = TempUserDir::new("remove-system-token");
            let (context, _) = open_at(&user.path);
            // SAFETY: `context` was opened above and is not yet freed; only this thread calls into
            // it.
            let instance = unsafe { guessed_nihao(context) };
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
            let row = unsafe { row_of(instance, NORMAL) };
            // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
            unsafe { pinyin_remove_user_candidate(instance, row) };
        },
    );
}

/// `remove_user_candidate` asserts where the user phrase table is gone.
#[test]
fn remove_user_candidate_aborts_without_the_phrase_table() {
    assert_aborts(
        "choose::remove_user_candidate_aborts_without_the_phrase_table",
        || {
            let user = TempUserDir::new("remove-no-phrase-table");
            saved_user_phrase(&user.path);
            std::fs::rename(
                user.path.join("user_phrase_index.bin"),
                user.path.join("user_phrase_index.bin.gone"),
            )
            .expect("move the phrase table");
            let (instance, cand) = user_row_of_dada(&user.path);
            unsafe { pinyin_remove_user_candidate(instance, cand) };
        },
    );
}

/// `remove_user_candidate` asserts on a reading the user pinyin index does
/// not hold. A second import of the same text into the same library adds its
/// reading to the item and to no index (`pinyin.cpp:561-582`), so the
/// removal's second `pinyin_table->remove_index` fails its
/// `assert(ERROR_OK == retval)` (`:3754-3759`).
#[test]
fn remove_user_candidate_aborts_on_a_reading_the_index_lacks() {
    assert_aborts(
        "choose::remove_user_candidate_aborts_on_a_reading_the_index_lacks",
        || {
            let user = TempUserDir::new("remove-unindexed-reading");
            let (context, instance) = open_at(&user.path);
            assert_eq!(
                // SAFETY: `context` was opened above and is not yet freed; only this thread calls
                // into it.
                unsafe {
                    import_batch(
                        context,
                        7,
                        &[("龘龘", "da2'da2", 5), ("龘龘", "da1'da1", 5)],
                    )
                },
                [true, true]
            );
            assert!(unsafe { crate::support::pinyin_save(context) });
            // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above
            // and is not yet freed; nothing uses either handle after this call; only this thread
            // calls into them.
            unsafe { close(context, instance) };
            let (instance, cand) = user_row_of_dada(&user.path);
            unsafe { pinyin_remove_user_candidate(instance, cand) };
        },
    );
}

/// `remove_user_candidate` asserts on a residual user gram: a total of 7
/// over one item of 3.
#[test]
fn remove_user_candidate_aborts_on_a_residual_bigram_gram() {
    assert_aborts(
        "choose::remove_user_candidate_aborts_on_a_residual_bigram_gram",
        || {
            let user = TempUserDir::new("remove-residual-gram");
            let token = saved_user_phrase(&user.path);
            replace_user_bigram(
                &user.path,
                &[(1_u32.to_le_bytes().to_vec(), gram_bytes(7, &[(token, 3)]))],
            );
            let (instance, cand) = user_row_of_dada(&user.path);
            unsafe { pinyin_remove_user_candidate(instance, cand) };
        },
    );
}

/// `choose_candidate` at offset 1 asserts on a list guessed without the
/// sentence row.
#[test]
fn choose_candidate_aborts_at_offset_one_without_a_sentence() {
    assert_aborts(
        "choose::choose_candidate_aborts_at_offset_one_without_a_sentence",
        || {
            let user = TempUserDir::new("choose-without-sentence");
            let (_context, instance) = open_at(&user.path);
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            assert_eq!(unsafe { parse(instance, "nihao") }, 5);
            // SORT_WITHOUT_SENTENCE_CANDIDATE.
            assert!(unsafe { pinyin_guess_candidates(instance, 0, 0x1) });
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
            let row = unsafe { row_of(instance, NORMAL) };
            // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
            unsafe { pinyin_choose_candidate(instance, 1, row) };
        },
    );
}

/// A punctuation row of a prediction chooses: true, twice.
#[test]
fn a_predicted_punctuation_row_chooses() {
    let user = TempUserDir::new("choose-punctuation");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, c"好".as_ptr())
    });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { row_of(instance, PREDICTED_PUNCTUATION) };
    assert!(unsafe { pinyin_choose_predicted_candidate(instance, row) });
    assert!(unsafe { pinyin_choose_predicted_candidate(instance, row) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// An NBEST row chosen after a reparse that places no key answers the
/// empty matrix's `size() - 1`, -1 (`pinyin.cpp:2514-2520`,
/// `phonetic_key_matrix.cpp:34-38` at 074a2219), not the consumed length.
#[test]
fn an_nbest_choose_after_a_keyless_reparse_answers_minus_one() {
    let user = TempUserDir::new("keyless-nbest-choose");
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { row_of(instance, NBEST) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "'''") }, 3);
    assert_eq!(
        unsafe { crate::support::pinyin_choose_candidate(instance, 0, row) },
        -1
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A choose with a caller offset other than the row's `m_begin` forces the
/// row's own span and answers the caller's offset plus that span's length:
/// `guint8 len = add_constraint(m_begin, m_end, token); return offset + len;`
/// (074a2219 `pinyin.cpp:2582-2588`, `phonetic_lookup.cpp:61-86`). The row
/// came from a guess at 2, so its span is `[2, 5)` whatever offset the
/// choose names.
#[test]
fn a_choose_at_another_offset_answers_that_offset_plus_the_row_span() {
    let user = TempUserDir::new("choose-other-offset");
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert!(unsafe { pinyin_guess_candidates(instance, 2, crate::support::DEFAULT_SORT) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { row_with(instance, NORMAL, "好") };
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, row) }, 3);
    // The forcing is the row's `[2, 5)`, not one from the caller's 0.
    assert!(!unsafe { crate::support::pinyin_clear_constraint(instance, 0) });
    assert!(unsafe { crate::support::pinyin_clear_constraint(instance, 2) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The span length the pin adds is a `guint8` (`pinyin.cpp:2582`): a row
/// across a separator run longer than 255 bytes answers its length modulo
/// 256.
#[test]
fn a_choose_over_a_long_separator_run_wraps_the_span_length() {
    let user = TempUserDir::new("choose-long-run");
    let (context, instance) = open_at(&user.path);
    let input = format!("ni{}hao", "'".repeat(300));
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, &input) }, input.len());
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::support::DEFAULT_SORT) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { row_with(instance, NORMAL, "你好") };
    assert_eq!(
        unsafe { pinyin_choose_candidate(instance, 0, row) },
        i32::try_from(input.len() % 256).expect("small")
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// For the transformed input schemes, the caller's offsets are in the ORIGINAL input's
/// coordinates and the pin builds every window from `start = offset`
/// (`pinyin.cpp:2224-2262`), so under double pinyin a lookup at 0 behind a
/// chosen 你好 shows the offset-0 window, and choosing 你 from it
/// answers the span's end in original coordinates, `0 + len = 2`.
#[test]
fn choosing_behind_the_composition_under_double_pinyin_answers_the_chosen_span() {
    /// # Safety
    ///
    /// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
    /// `pinyin_free_instance` has not freed it.
    ///
    /// Calls are serial across the context, its sibling instances and its iterators: no other call
    /// through any of them, and no borrow of their state, overlaps this call (the crate's Serial
    /// calls rule). The returned row is valid until the snapshot is invalidated or the instance is
    /// freed.
    unsafe fn row(instance: *mut PinyinInstance, text: &str) -> *mut LookupCandidate {
        // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into
        // it.
        let index = unsafe { capture_rows(instance) }
            .iter()
            .position(|(_, row, _)| row == text)
            .unwrap_or_else(|| panic!("{text} is offered"));
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(unsafe {
            pinyin_get_candidate(
                instance,
                u32::try_from(index).expect("small candidate index"),
                &raw mut cand,
            )
        });
        cand
    }
    let user_dir = TempUserDir::new("row37-double-behind-choose");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // Microsoft double pinyin (scheme 2): n+i, then h+ao (`k`).
    assert!(unsafe { crate::support::pinyin_set_double_pinyin_scheme(context, 2) });
    let keys = cstr("nihk");
    assert_eq!(
        unsafe { crate::support::pinyin_parse_more_double_pinyins(instance, keys.as_ptr()) },
        4
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let phrase = unsafe { row(instance, "你好") };
    assert_eq!(
        // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
        unsafe { pinyin_choose_candidate(instance, 0, phrase) },
        4,
        "the chosen phrase consumes both keys, in original coordinates"
    );
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let word = unsafe { row(instance, "你") };
    assert_eq!(
        // SAFETY: the row belongs to this live instance and no call has invalidated its snapshot.
        unsafe { pinyin_choose_candidate(instance, 0, word) },
        2,
        "the pin's offset + len for the key-wide choose behind the composition"
    );
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// The shorter forcing is cleared before a later clear reaches the old tail.
#[test]
fn a_clear_through_a_freed_run_start_aborts() {
    assert_aborts_after_setup(
        "choose::a_clear_through_a_freed_run_start_aborts",
        || instance_with_a_freed_constraint_head("nihao"),
        |(_user, _context, instance)| {
            unsafe { crate::support::pinyin_clear_constraint(instance, 3) };
        },
    );
}

/// Adding a forcing also clears the covered cells before writing the new span.
#[test]
fn choosing_across_a_freed_run_start_aborts() {
    assert_aborts_after_setup(
        "choose::choosing_across_a_freed_run_start_aborts",
        || {
            let (user, context, instance) = instance_with_a_freed_constraint_head("nihao");
            assert!(unsafe { pinyin_guess_candidates(instance, 2, DEFAULT_SORT) });
            let next = unsafe { row_with(instance, NORMAL, "好") };
            (user, context, instance, next)
        },
        |(_user, _context, instance, next)| {
            unsafe { pinyin_choose_candidate(instance, 2, next) };
        },
    );
}

#[test]
fn punctuation_allows_clear_until_a_live_head_is_freed() {
    let (_user, context, instance) = instance_after_punctuation("nihaoma");
    // A clear through the original interior still finds the head.
    assert!(unsafe { crate::support::pinyin_clear_constraint(instance, 3) });
    assert!(!unsafe { crate::support::pinyin_clear_constraint(instance, 6) });
    put_and_clear_a_shorter_forcing(instance);
    // The shorter forcing's interior was overwritten and then cleared.
    assert!(!unsafe { crate::support::pinyin_clear_constraint(instance, 1) });
    // These offsets were outside the original phrase span.
    assert!(!unsafe { crate::support::pinyin_clear_constraint(instance, 6) });
    assert!(!unsafe { crate::support::pinyin_clear_constraint(instance, 7) });
    unsafe { close(context, instance) };
}

#[test]
fn reset_discards_punctuation_constraint_history() {
    let (_user, context, instance) = instance_with_a_freed_constraint_head("nihao");
    assert!(unsafe { crate::support::pinyin_reset(instance) });
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(!unsafe { crate::support::pinyin_clear_constraint(instance, 3) });
    unsafe { close(context, instance) };
}

fn instance_with_a_freed_constraint_head(
    input: &str,
) -> (TempUserDir, *mut PinyinContext, *mut PinyinInstance) {
    let (user, context, instance) = instance_after_punctuation(input);
    put_and_clear_a_shorter_forcing(instance);
    (user, context, instance)
}

fn instance_after_punctuation(
    input: &str,
) -> (TempUserDir, *mut PinyinContext, *mut PinyinInstance) {
    let user = TempUserDir::new("punctuation-forcing");
    let (context, instance) = open_at(&user.path);
    assert_eq!(unsafe { parse(instance, input) }, input.len());
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let phrase = unsafe { row_with(instance, NORMAL, "你好") };
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, phrase) }, 5);
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, c"好".as_ptr())
    });
    let punctuation = unsafe { row_of(instance, PREDICTED_PUNCTUATION) };
    assert_eq!(
        unsafe { pinyin_choose_candidate(instance, 0, punctuation) },
        0
    );
    (user, context, instance)
}

fn put_and_clear_a_shorter_forcing(instance: *mut PinyinInstance) {
    assert!(unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
    let first = unsafe { row_with(instance, NORMAL, "你") };
    assert_eq!(unsafe { pinyin_choose_candidate(instance, 0, first) }, 2);
    assert!(unsafe { crate::support::pinyin_clear_constraint(instance, 0) });
}
