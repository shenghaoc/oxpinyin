//! Decisions of the C boundary on the w3 mini fixture: refusals, aborts,
//! out-parameters and the files a call writes. Each test asserts only the
//! deciding values. An abort test asserts the abort status alone: the child's
//! only stderr is the first init's `open …/user.conf failed.` line
//! (docs/findings/pin-abort-assert-line.md: oxpinyin writes no assertion line).

use std::ffi::c_char;
use std::path::Path;
use std::ptr;

use crate::candidates::{
    pinyin_choose_candidate, pinyin_choose_predicted_candidate, pinyin_get_candidate,
    pinyin_get_candidate_nbest_index, pinyin_get_candidate_type, pinyin_get_n_candidate,
    pinyin_is_user_candidate, pinyin_remove_user_candidate,
};
use crate::config::{
    pinyin_load_phrase_library, pinyin_mask_out, pinyin_set_options, pinyin_unload_phrase_library,
};
use crate::context::{pinyin_fini, pinyin_init, pinyin_save};
use crate::cursor::{
    pinyin_get_left_pinyin_offset, pinyin_get_pinyin_key, pinyin_get_pinyin_key_rest,
    pinyin_get_right_pinyin_offset,
};
use crate::dict::{
    pinyin_token_add_unigram_frequency, pinyin_token_get_n_pronunciation,
    pinyin_token_get_unigram_frequency,
};
use crate::e2e_tests::{capture_rows, import_batch};
use crate::instance::{pinyin_alloc_instance, pinyin_free_instance, pinyin_reset};
use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_begin_get_bigram_phrases,
    pinyin_bigram_iterator_get_next_phrase, pinyin_bigram_iterator_has_next_phrase,
    pinyin_end_add_phrases,
};
use crate::keys::{pinyin_get_pinyin_is_incomplete, pinyin_parse_full_pinyin};
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::predict::pinyin_guess_predicted_candidates;
use crate::sentence::{
    pinyin_get_character_offset, pinyin_get_sentence, pinyin_guess_candidates,
    pinyin_guess_predicted_candidates_with_punctuations, pinyin_guess_sentence,
};
use crate::test_support::{
    TempUserDir, assert_aborts, craft_hash, craft_tree, craft_user_bigram, craft_user_index, cstr,
    gram_bytes, lookup, open, plant_bigram, private_system, set_phrase_table_row, system_dir,
    write_user_conf,
};
use crate::text::{pinyin_get_double_pinyin_auxiliary_text, pinyin_get_full_pinyin_auxiliary_text};
use crate::types::{
    ChewingKey, LookupCandidate, PinyinContext, PinyinInstance, lookup_candidate_type_t,
};

/// The value a test seeds an out-parameter with, to see it left alone.
const UNTOUCHED: u32 = 0x00AB_CDEF;
const NBEST: lookup_candidate_type_t = lookup_candidate_type_t::NBEST_MATCH_CANDIDATE;
const NORMAL: lookup_candidate_type_t = lookup_candidate_type_t::NORMAL_CANDIDATE;
const PREDICTED_BIGRAM: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE;
const PREDICTED_PREFIX: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE;
const PREDICTED_PUNCTUATION: lookup_candidate_type_t =
    lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE;

/// `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN`.
const SEPARATOR_OPTIONS: u32 = 0x18a;

/// A context on `system` and `user`; NULL when init refuses.
fn init_at(system: &Path, user: &Path) -> *mut PinyinContext {
    let system = cstr(system.to_str().expect("UTF-8 path"));
    let user = cstr(user.to_str().expect("UTF-8 path"));
    pinyin_init(system.as_ptr(), user.as_ptr())
}

/// A context and instance on the mini fixture and `user`.
fn open_at(user: &Path) -> (*mut PinyinContext, *mut PinyinInstance) {
    open(user.to_str().expect("UTF-8 path"))
}

fn alloc(context: *mut PinyinContext) -> *mut PinyinInstance {
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    instance
}

fn close(context: *mut PinyinContext, instance: *mut PinyinInstance) {
    pinyin_free_instance(instance);
    pinyin_fini(context);
}

fn parse(instance: *mut PinyinInstance, text: impl AsRef<[u8]>) -> usize {
    let text = std::ffi::CString::new(text.as_ref()).expect("no interior NUL");
    pinyin_parse_more_full_pinyins(instance, text.as_ptr())
}

/// The first row of `kind` in the current list.
fn row_of(instance: *mut PinyinInstance, kind: lookup_candidate_type_t) -> *mut LookupCandidate {
    let index = capture_rows(instance)
        .iter()
        .position(|(found, _, _)| *found == kind)
        .unwrap_or_else(|| panic!("a {kind:?} row is listed"));
    candidate_at(instance, index)
}

/// The first row of `kind` whose text is `text`.
fn row_with(
    instance: *mut PinyinInstance,
    kind: lookup_candidate_type_t,
    text: &str,
) -> *mut LookupCandidate {
    let index = capture_rows(instance)
        .iter()
        .position(|(found, row, _)| *found == kind && row == text)
        .unwrap_or_else(|| panic!("a {kind:?} row {text} is listed"));
    candidate_at(instance, index)
}

fn candidate_at(instance: *mut PinyinInstance, index: usize) -> *mut LookupCandidate {
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(pinyin_get_candidate(
        instance,
        u32::try_from(index).expect("small index"),
        &raw mut cand
    ));
    cand
}

fn n_candidate(instance: *mut PinyinInstance) -> u32 {
    let mut count = UNTOUCHED;
    assert!(pinyin_get_n_candidate(instance, &raw mut count));
    count
}

fn unigram(instance: *mut PinyinInstance, token: u32) -> (bool, u32) {
    let mut freq = UNTOUCHED;
    let ret = pinyin_token_get_unigram_frequency(instance, token, &raw mut freq);
    (ret, freq)
}

/// `get_sentence(index)` with a seeded out-pointer: the answer, and the
/// text or `None` when the pointer was left alone.
fn sentence_out(instance: *mut PinyinInstance, index: u8) -> (bool, Option<String>) {
    let seed = UNTOUCHED as usize as *mut c_char;
    let mut out = seed;
    let ret = pinyin_get_sentence(instance, index, &raw mut out);
    if out == seed {
        (ret, None)
    } else {
        (ret, Some(crate::ffi::take_owned_cstr(out)))
    }
}

/// Parses `nihao`, guesses the sentence and the candidates at 0 with sort 0.
fn guessed_nihao(context: *mut PinyinContext) -> *mut PinyinInstance {
    let instance = alloc(context);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 0, 0));
    instance
}

/// Adds the negation of the loaded facade total to token 0x01000001, so
/// the total the model divides by wraps to zero (register row 61).
fn wrap_facade_total_to_zero(instance: *mut PinyinInstance) {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let total = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    let total = u32::try_from(total).expect("a guint32 total");
    assert!(pinyin_token_add_unigram_frequency(
        instance,
        0x0100_0001,
        total.wrapping_neg()
    ));
    // SAFETY: as above.
    let after = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    assert_eq!(after, 0, "the facade total wraps to zero");
}

/// The first key of `text` as parsed by a scratch instance: its two bytes.
fn first_key_bytes(context: *mut PinyinContext, text: &str) -> Vec<u8> {
    let _slots = crate::test_support::key_slot_guard();
    let instance = alloc(context);
    assert!(parse(instance, text) > 0);
    let mut key: *mut ChewingKey = ptr::null_mut();
    assert!(pinyin_get_pinyin_key(instance, 0, &raw mut key));
    // SAFETY: a true answer points at a live two-byte ChewingKey, read
    // before the instance is freed.
    let bytes = unsafe { std::slice::from_raw_parts(key.cast::<u8>(), 2) }.to_vec();
    pinyin_free_instance(instance);
    bytes
}

/// A profile whose user pinyin index holds `keys`, each with an empty
/// value, under a conforming `user.conf` for `system`; then a context on it.
fn overlong_index_context(
    system: &Path,
    user: &Path,
    keys: impl FnOnce(&[u8]) -> Vec<Vec<u8>>,
) -> *mut PinyinContext {
    let context = init_at(system, user);
    assert!(!context.is_null());
    let first = first_key_bytes(context, "ni");
    pinyin_fini(context);
    let rows: Vec<(Vec<u8>, Vec<u8>)> = keys(&first)
        .into_iter()
        .map(|key| (key, Vec::new()))
        .collect();
    craft_user_index(&user.join("user_pinyin_index.bin"), &rows);
    write_user_conf(user, system);
    let context = init_at(system, user);
    assert!(!context.is_null());
    context
}

/// The user bigram of a profile on the mini fixture replaced by `rows`
/// under a conforming `user.conf`; the rest of the profile stays.
fn replace_user_bigram(user: &Path, rows: &[(Vec<u8>, Vec<u8>)]) {
    craft_user_bigram(&user.join("user_bigram.db"), rows);
    write_user_conf(user, &system_dir());
}

// ── Keys and parsing ──────────────────────────────────────────────────────

/// The luoma and secondary-zhuyin renderers answer false on the zero key
/// of a column and write NULL to the out-parameter (register row 41).
#[test]
fn luoma_and_secondary_zhuyin_refuse_the_zero_key() {
    let _slots = crate::test_support::key_slot_guard();
    let user = TempUserDir::new("zero-key-renderers");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_set_options(context, 0x1aa));
    assert_eq!(parse(instance, "xi'"), 3);
    assert!(pinyin_guess_sentence(instance));
    let mut key: *mut ChewingKey = ptr::null_mut();
    assert!(pinyin_get_pinyin_key(instance, 2, &raw mut key));
    for render in [
        crate::keys::pinyin_get_luoma_pinyin_string,
        crate::keys::pinyin_get_secondary_zhuyin_string,
    ] {
        let mut out = UNTOUCHED as usize as *mut c_char;
        assert!(!render(instance, key, &raw mut out));
        assert!(out.is_null());
    }
    close(context, instance);
}

/// Init seeds USE_TONE alone: a toned key parses, an initial does not
/// (no PINYIN_INCOMPLETE).
#[test]
fn init_options_parse_tones_but_not_initials() {
    let user = TempUserDir::new("default-options");
    let (context, _) = open_at(&user.path);
    for (text, parsed) in [("n", 0), ("ni3", 3), ("zzz", 0), ("ni2hao", 6)] {
        let instance = alloc(context);
        assert_eq!(parse(instance, text), parsed, "{text}");
        pinyin_free_instance(instance);
    }
    pinyin_fini(context);
}

/// `is_incomplete` asserts on a toned initial.
#[test]
fn is_incomplete_aborts_on_a_toned_initial() {
    assert_aborts(
        "boundary_tests::is_incomplete_aborts_on_a_toned_initial",
        || {
            let user = TempUserDir::new("toned-incomplete");
            let (context, instance) = open_at(&user.path);
            assert!(pinyin_set_options(context, 0x28));
            let mut key = 0_u16;
            pinyin_parse_full_pinyin(instance, c"n4".as_ptr(), (&raw mut key).cast());
            pinyin_get_pinyin_is_incomplete(instance, (&raw mut key).cast());
        },
    );
}

/// A toned initial aborts the sentence guess.
#[test]
fn guess_sentence_aborts_on_a_toned_initial() {
    assert_aborts(
        "boundary_tests::guess_sentence_aborts_on_a_toned_initial",
        || {
            let user = TempUserDir::new("toned-sentence");
            let (context, instance) = open_at(&user.path);
            assert!(pinyin_set_options(context, 0x28));
            assert_eq!(parse(instance, "n4"), 2);
            pinyin_guess_sentence(instance);
        },
    );
}

/// A toned initial aborts the candidate guess without the prepended rows
/// too.
#[test]
fn guess_candidates_aborts_on_a_toned_initial() {
    assert_aborts(
        "boundary_tests::guess_candidates_aborts_on_a_toned_initial",
        || {
            let user = TempUserDir::new("toned-candidates");
            let (context, instance) = open_at(&user.path);
            assert!(pinyin_set_options(context, 0x28));
            assert_eq!(parse(instance, "n4"), 2);
            // SORT_WITHOUT_SENTENCE_CANDIDATE | SORT_WITHOUT_LONGER_CANDIDATE.
            pinyin_guess_candidates(instance, 0, 0x3);
        },
    );
}

/// `parse_full_pinyin` asserts on an apostrophe after bytes that are not
/// UTF-8 (#1279).
#[test]
fn full_key_parse_aborts_on_an_apostrophe_after_invalid_bytes() {
    assert_aborts(
        "boundary_tests::full_key_parse_aborts_on_an_apostrophe_after_invalid_bytes",
        || {
            let user = TempUserDir::new("full-key-invalid");
            let (_context, instance) = open_at(&user.path);
            let mut key = 0xCDEF_u16;
            pinyin_parse_full_pinyin(instance, c"ni\xff'".as_ptr(), (&raw mut key).cast());
        },
    );
}

// ── Cursor, offsets and sentences ─────────────────────────────────────────

/// `get_sentence` asserts on an index past the n-best rows.
#[test]
fn get_sentence_aborts_past_the_rows() {
    assert_aborts("boundary_tests::get_sentence_aborts_past_the_rows", || {
        let user = TempUserDir::new("sentence-past-rows");
        let (_context, instance) = open_at(&user.path);
        assert_eq!(parse(instance, "nihao"), 5);
        assert!(pinyin_guess_sentence(instance));
        let mut out: *mut c_char = ptr::null_mut();
        pinyin_get_sentence(instance, 5, &raw mut out);
    });
}

/// Before a guess, and after a reset, `get_sentence` answers false and
/// leaves the out-pointer alone.
#[test]
fn get_sentence_before_a_guess_leaves_the_out_pointer() {
    let user = TempUserDir::new("sentence-before-guess");
    let (context, instance) = open_at(&user.path);
    for index in [0, 255] {
        assert_eq!(
            sentence_out(instance, index),
            (false, None),
            "fresh {index}"
        );
    }
    assert_eq!(parse(instance, "nihao"), 5);
    for index in [0, 255] {
        assert_eq!(
            sentence_out(instance, index),
            (false, None),
            "parsed {index}"
        );
    }
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence_out(instance, 0), (true, Some("你好".to_owned())));
    assert!(pinyin_reset(instance));
    assert_eq!(sentence_out(instance, 0), (false, None));
    close(context, instance);
}

/// A guess over a leading separator answers true with no n-best row, so
/// `get_sentence(0)` answers false and leaves the out-pointer alone.
#[test]
fn a_leading_separator_guess_has_no_sentence() {
    let user = TempUserDir::new("separator-sentence");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_set_options(context, SEPARATOR_OPTIONS));
    assert_eq!(parse(instance, "'ni'hao"), 7);
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence_out(instance, 0), (false, None));
    close(context, instance);
}

/// The full-pinyin auxiliary text asserts on a leading apostrophe.
#[test]
fn full_auxiliary_text_aborts_on_a_leading_apostrophe() {
    assert_aborts(
        "boundary_tests::full_auxiliary_text_aborts_on_a_leading_apostrophe",
        || {
            let user = TempUserDir::new("aux-leading-apostrophe");
            let (_context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "'nihao"), 6);
            let mut aux: *mut c_char = ptr::null_mut();
            pinyin_get_full_pinyin_auxiliary_text(instance, 2, &raw mut aux);
        },
    );
}

/// The double-pinyin auxiliary text asserts on a cut more than two bytes
/// into a key.
#[test]
fn double_auxiliary_text_aborts_three_bytes_into_a_key() {
    assert_aborts(
        "boundary_tests::double_auxiliary_text_aborts_three_bytes_into_a_key",
        || {
            let user = TempUserDir::new("aux-double-cut");
            let (_context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "zhong"), 5);
            let mut aux: *mut c_char = ptr::null_mut();
            pinyin_get_double_pinyin_auxiliary_text(instance, 3, &raw mut aux);
        },
    );
}

/// `get_pinyin_key_rest` asserts on an empty matrix.
#[test]
fn key_rest_aborts_on_an_empty_matrix() {
    assert_aborts("boundary_tests::key_rest_aborts_on_an_empty_matrix", || {
        let user = TempUserDir::new("key-rest-empty");
        let (_context, instance) = open_at(&user.path);
        let mut rest = ptr::null_mut();
        pinyin_get_pinyin_key_rest(instance, 0, &raw mut rest);
    });
}

/// The left offset asserts on the zero key after a separator.
#[test]
fn left_offset_aborts_after_a_separator() {
    assert_aborts(
        "boundary_tests::left_offset_aborts_after_a_separator",
        || {
            let user = TempUserDir::new("left-after-separator");
            let (_context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "ni'hao"), 6);
            let mut left = 0_usize;
            pinyin_get_left_pinyin_offset(instance, 3, &raw mut left);
        },
    );
}

/// The right offset asserts on the zero key after a separator.
#[test]
fn right_offset_aborts_after_a_separator() {
    assert_aborts(
        "boundary_tests::right_offset_aborts_after_a_separator",
        || {
            let user = TempUserDir::new("right-after-separator");
            let (_context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "ni'hao"), 6);
            let mut right = 0_usize;
            pinyin_get_right_pinyin_offset(instance, 3, &raw mut right);
        },
    );
}

/// The character offset asserts on the empty leading column of a
/// leading separator (MatrixColumnAssert).
#[test]
fn character_offset_aborts_on_an_empty_leading_column() {
    assert_aborts(
        "boundary_tests::character_offset_aborts_on_an_empty_leading_column",
        || {
            let user = TempUserDir::new("character-leading-separator");
            let (context, instance) = open_at(&user.path);
            assert!(pinyin_set_options(context, SEPARATOR_OPTIONS));
            assert_eq!(parse(instance, "'ni"), 3);
            pinyin_guess_sentence(instance);
            let mut length = 0_usize;
            pinyin_get_character_offset(instance, c"你".as_ptr(), 0, &raw mut length);
        },
    );
}

/// The right offset at the first of two leading separators answers false
/// and leaves the out-parameter alone.
#[test]
fn right_offset_at_a_leading_separator_is_refused() {
    let user = TempUserDir::new("right-leading-separator");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_set_options(context, SEPARATOR_OPTIONS));
    assert_eq!(parse(instance, "''ni"), 4);
    assert!(pinyin_guess_sentence(instance));
    let mut right = UNTOUCHED as usize;
    assert!(!pinyin_get_right_pinyin_offset(instance, 0, &raw mut right));
    assert_eq!(right, UNTOUCHED as usize);
    close(context, instance);
}

// ── Candidates ────────────────────────────────────────────────────────────

/// `choose_candidate` asserts on a predicted prefix row.
#[test]
fn choose_candidate_aborts_on_a_predicted_prefix_row() {
    assert_aborts(
        "boundary_tests::choose_candidate_aborts_on_a_predicted_prefix_row",
        || {
            let user = TempUserDir::new("choose-prefix-row");
            let (_context, instance) = open_at(&user.path);
            assert!(pinyin_guess_predicted_candidates_with_punctuations(
                instance,
                c"你".as_ptr()
            ));
            pinyin_choose_candidate(instance, 0, row_of(instance, PREDICTED_PREFIX));
        },
    );
}

/// `choose_predicted_candidate` asserts on a NORMAL row.
#[test]
fn choose_predicted_candidate_aborts_on_a_normal_row() {
    assert_aborts(
        "boundary_tests::choose_predicted_candidate_aborts_on_a_normal_row",
        || {
            let user = TempUserDir::new("choose-predicted-normal");
            let (context, _) = open_at(&user.path);
            let instance = guessed_nihao(context);
            pinyin_choose_predicted_candidate(instance, row_of(instance, NORMAL));
        },
    );
}

/// `get_candidate_nbest_index` asserts on a NORMAL row.
#[test]
fn nbest_index_aborts_on_a_normal_row() {
    assert_aborts("boundary_tests::nbest_index_aborts_on_a_normal_row", || {
        let user = TempUserDir::new("nbest-normal");
        let (context, _) = open_at(&user.path);
        let instance = guessed_nihao(context);
        let mut index = 0xAB_u8;
        pinyin_get_candidate_nbest_index(instance, row_of(instance, NORMAL), &raw mut index);
    });
}

/// `remove_user_candidate` asserts on a sentence row.
#[test]
fn remove_user_candidate_aborts_on_a_sentence_row() {
    assert_aborts(
        "boundary_tests::remove_user_candidate_aborts_on_a_sentence_row",
        || {
            let user = TempUserDir::new("remove-sentence-row");
            let (context, _) = open_at(&user.path);
            let instance = guessed_nihao(context);
            pinyin_remove_user_candidate(instance, row_of(instance, NBEST));
        },
    );
}

/// `remove_user_candidate` asserts on a NORMAL system token.
#[test]
fn remove_user_candidate_aborts_on_a_system_token() {
    assert_aborts(
        "boundary_tests::remove_user_candidate_aborts_on_a_system_token",
        || {
            let user = TempUserDir::new("remove-system-token");
            let (context, _) = open_at(&user.path);
            let instance = guessed_nihao(context);
            pinyin_remove_user_candidate(instance, row_of(instance, NORMAL));
        },
    );
}

/// Imports and saves 龘龘 (da2'da2) in library 7, then closes the context;
/// answers its token.
fn saved_user_phrase(user: &Path) -> u32 {
    let (context, instance) = open_at(user);
    assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 5)]), [true]);
    assert!(pinyin_save(context));
    let token = lookup(instance, "龘龘")[0];
    assert_eq!(token >> 24, 7);
    close(context, instance);
    token
}

/// The first NORMAL user row of `dada` on a new context over `user`.
fn user_row_of_dada(user: &Path) -> (*mut PinyinInstance, *mut LookupCandidate) {
    let (_context, instance) = open_at(user);
    assert_eq!(parse(instance, "dada"), 4);
    assert!(pinyin_guess_candidates(instance, 0, 0));
    let rows = capture_rows(instance);
    let index = (0..rows.len())
        .find(|&index| {
            let cand = candidate_at(instance, index);
            rows[index].0 == NORMAL && pinyin_is_user_candidate(instance, cand)
        })
        .expect("a NORMAL user row");
    (instance, candidate_at(instance, index))
}

// Removing a real imported candidate must mutate the retained store even
// after the public handle is gone; no pin call is valid in that state.
#[rstest::rstest]
fn removing_a_user_candidate_keeps_the_instance_owner(#[values(false, true)] finalized: bool) {
    let user = TempUserDir::new("remove-retained-owner");
    let token = saved_user_phrase(&user.path);
    let (instance, candidate) = user_row_of_dada(&user.path);
    let context = crate::instance::pinyin_get_context(instance);
    if finalized {
        crate::test_support::finalize_public_context(context, instance);
    }
    assert!(pinyin_remove_user_candidate(instance, candidate));
    // SAFETY: the instance remains live; the removal's exclusive borrow ended.
    let inst = unsafe { crate::state::instance_ref(instance) };
    assert!(inst.core.user.phrase(token).expect("read").is_none());
    crate::instance::pinyin_free_instance(instance);
    if !finalized {
        crate::context::pinyin_fini(context);
    }
}

/// `remove_user_candidate` asserts where the user phrase table is gone
///.
#[test]
fn remove_user_candidate_aborts_without_the_phrase_table() {
    assert_aborts(
        "boundary_tests::remove_user_candidate_aborts_without_the_phrase_table",
        || {
            let user = TempUserDir::new("remove-no-phrase-table");
            saved_user_phrase(&user.path);
            std::fs::rename(
                user.path.join("user_phrase_index.bin"),
                user.path.join("user_phrase_index.bin.gone"),
            )
            .expect("move the phrase table");
            let (instance, cand) = user_row_of_dada(&user.path);
            pinyin_remove_user_candidate(instance, cand);
        },
    );
}

/// `remove_user_candidate` asserts on a residual user gram: a total of 7
/// over one item of 3.
#[test]
fn remove_user_candidate_aborts_on_a_residual_bigram_gram() {
    assert_aborts(
        "boundary_tests::remove_user_candidate_aborts_on_a_residual_bigram_gram",
        || {
            let user = TempUserDir::new("remove-residual-gram");
            let token = saved_user_phrase(&user.path);
            replace_user_bigram(
                &user.path,
                &[(1_u32.to_le_bytes().to_vec(), gram_bytes(7, &[(token, 3)]))],
            );
            let (instance, cand) = user_row_of_dada(&user.path);
            pinyin_remove_user_candidate(instance, cand);
        },
    );
}

/// `choose_candidate` at offset 1 asserts on a list guessed without the
/// sentence row.
#[test]
fn choose_candidate_aborts_at_offset_one_without_a_sentence() {
    assert_aborts(
        "boundary_tests::choose_candidate_aborts_at_offset_one_without_a_sentence",
        || {
            let user = TempUserDir::new("choose-without-sentence");
            let (_context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "nihao"), 5);
            // SORT_WITHOUT_SENTENCE_CANDIDATE.
            assert!(pinyin_guess_candidates(instance, 0, 0x1));
            pinyin_choose_candidate(instance, 1, row_of(instance, NORMAL));
        },
    );
}

/// A punctuation row of a prediction chooses: true, twice.
#[test]
fn a_predicted_punctuation_row_chooses() {
    let user = TempUserDir::new("choose-punctuation");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        c"好".as_ptr()
    ));
    let row = row_of(instance, PREDICTED_PUNCTUATION);
    assert!(pinyin_choose_predicted_candidate(instance, row));
    assert!(pinyin_choose_predicted_candidate(instance, row));
    close(context, instance);
}

/// A predicted bigram row whose successor is a user phrase is not a user
/// candidate: `is_user_candidate` tests the type first.
#[test]
fn a_predicted_bigram_row_is_not_a_user_candidate() {
    let user = TempUserDir::new("predicted-is-user");
    let successor = saved_user_phrase(&user.path);
    let (context, instance) = open_at(&user.path);
    let prefixes = lookup(instance, "你");
    close(context, instance);
    let gram = gram_bytes(20, &[(successor, 20)]);
    let rows: Vec<_> = prefixes
        .iter()
        .map(|token| (token.to_le_bytes().to_vec(), gram.clone()))
        .collect();
    replace_user_bigram(&user.path, &rows);
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        c"你".as_ptr()
    ));
    let index = capture_rows(instance)
        .iter()
        .position(|(kind, _, _)| *kind == PREDICTED_BIGRAM)
        .expect("a predicted bigram row");
    assert_eq!(capture_rows(instance)[index].1, "龘龘");
    let row = candidate_at(instance, index);
    let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
    assert!(pinyin_get_candidate_type(instance, row, &raw mut kind));
    assert_eq!(kind, PREDICTED_BIGRAM);
    assert!(!pinyin_is_user_candidate(instance, row));
    close(context, instance);
}

// ── Facade total and prediction ───────────────────────────────────────────

/// With the facade total wrapped to zero, the candidate guess aborts in
/// the scorer (register row 61).
#[test]
fn guess_candidates_aborts_on_a_zero_facade_total() {
    assert_aborts(
        "boundary_tests::guess_candidates_aborts_on_a_zero_facade_total",
        || {
            let user = TempUserDir::new("facade-total-zero");
            let (_context, instance) = open_at(&user.path);
            wrap_facade_total_to_zero(instance);
            assert_eq!(parse(instance, "nihao"), 5);
            pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT);
        },
    );
}

/// A guess that ranks no word row never reaches the scorer: true with no
/// row on a zero facade total, mid-key (`sentence_rows_only`) and at the
/// reserved end slot, where the search loop does not run
/// (`pinyin.cpp:2230`).
#[test]
fn a_guess_with_no_word_row_ranks_nothing_on_a_zero_total() {
    let user = TempUserDir::new("facade-total-zero-no-rank");
    let (context, _) = open_at(&user.path);
    let logs = crate::test_support::LogCapture::new();
    for (offset, sort) in [(3, 3), (5, 2)] {
        let instance = alloc(context);
        assert_eq!(parse(instance, "nihao"), 5);
        wrap_facade_total_to_zero(instance);
        assert!(pinyin_guess_candidates(instance, offset, sort), "{offset}");
        assert_eq!(n_candidate(instance), 0, "{offset}");
        pinyin_free_instance(instance);
    }
    assert!(logs.take().is_empty());
    pinyin_fini(context);
}

/// Predicted bigram rows on a zero facade total abort in the scorer
/// (register row 61).
#[test]
fn predicted_bigram_rows_abort_on_a_zero_facade_total() {
    assert_aborts(
        "boundary_tests::predicted_bigram_rows_abort_on_a_zero_facade_total",
        || {
            let user = TempUserDir::new("row58-total-zero");
            let (context, instance) = open_at(&user.path);
            let (ni, hao) = (lookup(instance, "你"), lookup(instance, "好")[0]);
            close(context, instance);
            let gram = gram_bytes(20, &[(hao, 20)]);
            plant_bigram(
                &user.path,
                ni.iter()
                    .map(|token| (token.to_le_bytes().to_vec(), gram.clone()))
                    .collect(),
            );
            let (_context, instance) = open_at(&user.path);
            wrap_facade_total_to_zero(instance);
            pinyin_guess_predicted_candidates(instance, c"你".as_ptr());
        },
    );
}

/// The prediction walk aborts on a gram with no item and a total left
/// over (`ngram.cpp:70`).
#[test]
fn prediction_aborts_on_an_item_less_gram_with_a_total() {
    assert_aborts(
        "boundary_tests::prediction_aborts_on_an_item_less_gram_with_a_total",
        || {
            let user = TempUserDir::new("predict-empty-gram");
            let (context, instance) = open_at(&user.path);
            let ni = lookup(instance, "你");
            close(context, instance);
            plant_bigram(
                &user.path,
                ni.iter()
                    .map(|token| (token.to_le_bytes().to_vec(), vec![7, 0, 0, 0]))
                    .collect(),
            );
            let (_context, instance) = open_at(&user.path);
            pinyin_guess_predicted_candidates(instance, c"你".as_ptr());
        },
    );
}

/// The prediction walk reads only the grams of the prefixes it reaches: a
/// short gram under another phrase is never read.
#[test]
fn prediction_reads_only_the_prefixes_it_reaches() {
    let user = TempUserDir::new("predict-unrelated-gram");
    let (context, instance) = open_at(&user.path);
    let ni = lookup(instance, "你");
    close(context, instance);
    plant_bigram(
        &user.path,
        ni.iter()
            .map(|token| (token.to_le_bytes().to_vec(), vec![7, 0, 0]))
            .collect(),
    );
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_guess_predicted_candidates(instance, c"中".as_ptr()));
    close(context, instance);
}

/// Choosing a predicted prefix row whose unigram add would pass the
/// `guint32` limit answers false and leaves the count.
#[test]
fn a_prefix_choose_past_the_unigram_limit_is_refused() {
    let user = TempUserDir::new("row58-prefix-overflow");
    let (context, instance) = open_at(&user.path);
    // PINYIN_INCOMPLETE | DYNAMIC_ADJUST | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN.
    assert!(pinyin_set_options(context, 0x38a));
    assert_eq!(
        import_batch(
            context,
            7,
            &[("我", "wo3", 1), ("我甲甲", "wo3'jia3'jia3", 1_431_655_631)]
        ),
        [true, true]
    );
    let token = lookup(instance, "我甲甲")[0];
    assert_eq!(unigram(instance, token), (true, 4_294_966_893));
    assert!(pinyin_guess_predicted_candidates(instance, c"我".as_ptr()));
    let row = row_with(instance, PREDICTED_PREFIX, "甲甲");
    assert!(!pinyin_choose_predicted_candidate(instance, row));
    assert_eq!(unigram(instance, token), (true, 4_294_966_893));
    close(context, instance);
}

/// The facade total of a fresh instance on `context`.
fn facade_total(context: *mut PinyinContext) -> u64 {
    let instance = alloc(context);
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let total = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    pinyin_free_instance(instance);
    total
}

/// A context on a private system under `scratch` whose `gbk_char.bin`
/// (library 2) is a checksum-valid, in-bounds chunk that
/// `SubPhraseIndex::load` refuses at its second separator
/// (`phrase_index.cpp:355`). Its local total is T, the facade total of the
/// other libraries, which the facade does not add (`:242-246`). Answers the
/// context and T.
fn layout_failed_gbk_context(scratch: &Path) -> (*mut PinyinContext, u64) {
    let system = private_system(scratch);
    let user = scratch.join("user");
    std::fs::create_dir(&user).expect("user directory");
    let library = system.join("gbk_char.bin");
    let write = |total: u32| {
        let mut payload = [0_u8; 32];
        for (at, value) in [(0, total), (4, 17), (8, 20), (12, 24)] {
            payload[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        payload[16] = b'#';
        payload[19] = b'!';
        let chunk = oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame");
        std::fs::remove_file(&library).expect("unlink gbk_char.bin");
        std::fs::write(&library, chunk).expect("write gbk_char.bin");
    };
    write(0);
    let context = init_at(&system, &user);
    assert!(!context.is_null());
    let total = facade_total(context);
    pinyin_fini(context);
    assert!(total > 0, "the other libraries hold a total");
    write(u32::try_from(total).expect("a guint32 total"));
    let context = init_at(&system, &user);
    assert!(!context.is_null());
    assert_eq!(
        facade_total(context),
        total,
        "the refused library adds nothing"
    );
    (context, total)
}

/// The import iterator's constructor does not compact
/// (`pinyin.cpp:506-512`). An unload of the layout-failed library 2 while
/// the iterator is open takes its local total T out of the facade total T
/// (`pinyin.cpp:473`, `phrase_index.cpp:265`), and the scorer aborts on the
/// zero total (`pinyin.cpp:1859`, register row 61).
#[test]
fn an_unload_inside_an_open_import_zeroes_the_facade_total() {
    assert_aborts(
        "boundary_tests::an_unload_inside_an_open_import_zeroes_the_facade_total",
        || {
            let scratch = tempfile::tempdir().expect("scratch");
            let (context, _) = layout_failed_gbk_context(scratch.path());
            let iter = pinyin_begin_add_phrases(context, 1);
            assert!(!iter.is_null());
            assert!(pinyin_unload_phrase_library(context, 2));
            let instance = alloc(context);
            assert_eq!(parse(instance, "nihao"), 5);
            pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT);
        },
    );
}

/// An unload inside the open import takes library 2's local total T out
/// of the facade total T (`phrase_index.cpp:261-269`). The end's compact
/// has no sub-index of library 2 left to rebuild (`phrase_index.cpp:651-652`),
/// so the total stays zero and the scorer aborts (`pinyin.cpp:1859`,
/// register row 61).
#[test]
fn an_import_that_ends_after_the_unload_keeps_the_zero_total() {
    assert_aborts(
        "boundary_tests::an_import_that_ends_after_the_unload_keeps_the_zero_total",
        || {
            let scratch = tempfile::tempdir().expect("scratch");
            let (context, _) = layout_failed_gbk_context(scratch.path());
            let iter = pinyin_begin_add_phrases(context, 1);
            assert!(!iter.is_null());
            assert!(pinyin_unload_phrase_library(context, 2));
            pinyin_end_add_phrases(iter);
            let instance = alloc(context);
            assert_eq!(parse(instance, "nihao"), 5);
            pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT);
        },
    );
}

/// Control: `end_add_phrases` compacts first (`pinyin.cpp:657`), so the
/// library's local total is zero at the unload and the facade total stays
/// T.
#[test]
fn an_unload_after_the_import_ends_keeps_the_facade_total() {
    let scratch = tempfile::tempdir().expect("scratch");
    let (context, total) = layout_failed_gbk_context(scratch.path());
    let iter = pinyin_begin_add_phrases(context, 1);
    assert!(!iter.is_null());
    pinyin_end_add_phrases(iter);
    assert!(pinyin_unload_phrase_library(context, 2));
    assert_eq!(facade_total(context), total);
    let instance = alloc(context);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(pinyin_guess_candidates(
        instance,
        0,
        crate::test_support::DEFAULT_SORT
    ));
    assert!(n_candidate(instance) > 0);
    close(context, instance);
}

/// `mask_out` maps library 2 again: the failed load takes the old local
/// total T out of the facade total (`phrase_index.cpp:243-245`), and the
/// closing compact makes the reread local total zero (`pinyin.cpp:1294`).
/// The unload then takes out nothing, and the facade total stays T - T.
#[test]
fn an_unload_after_mask_out_takes_out_the_compacted_total() {
    let scratch = tempfile::tempdir().expect("scratch");
    let (context, _) = layout_failed_gbk_context(scratch.path());
    assert!(pinyin_mask_out(context, 0xFFFF_FFFF, 0));
    assert!(pinyin_unload_phrase_library(context, 2));
    assert_eq!(facade_total(context), 0);
    pinyin_fini(context);
}

/// The end of an import into the layout-failed library 2 rebuilds its
/// sub-index from its one item (`pinyin.cpp:657`, `phrase_index.cpp:648-675`):
/// count 9 times the import's factor 3 (`pinyin.cpp:522`, `:603-605`), so
/// the local total is 27. An increment that brings it to `u32::MAX` does not
/// overflow it (`phrase_index.cpp:170-171`) and is accepted.
#[test]
fn an_increment_after_an_import_into_a_failed_layout_sees_the_item_sum() {
    let scratch = tempfile::tempdir().expect("scratch");
    let (context, _) = layout_failed_gbk_context(scratch.path());
    assert_eq!(import_batch(context, 2, &[("龘龘", "da2'da2", 9)]), [true]);
    let instance = alloc(context);
    let token = lookup(instance, "龘龘")
        .into_iter()
        .find(|token| token >> 24 == 2)
        .expect("the imported token");
    assert!(pinyin_token_add_unigram_frequency(
        instance,
        token,
        u32::MAX - 27
    ));
    close(context, instance);
}

// ── Libraries and iterators ───────────────────────────────────────────────

/// `load_phrase_library(0)` asserts on the reserved library.
#[test]
fn load_of_the_reserved_library_aborts() {
    assert_aborts(
        "boundary_tests::load_of_the_reserved_library_aborts",
        || {
            let user = TempUserDir::new("load-library-0");
            let (context, _) = open_at(&user.path);
            pinyin_load_phrase_library(context, 0);
        },
    );
}

/// `get_next` on a fresh bigram export iterator of an empty profile
/// asserts.
#[test]
fn bigram_get_next_aborts_on_an_empty_export() {
    assert_aborts(
        "boundary_tests::bigram_get_next_aborts_on_an_empty_export",
        || {
            let user = TempUserDir::new("bigram-get-next");
            let (context, _) = open_at(&user.path);
            let iter = pinyin_begin_get_bigram_phrases(context);
            assert!(!pinyin_bigram_iterator_has_next_phrase(iter));
            let (mut phrase, mut pinyin, mut count) = (ptr::null_mut(), ptr::null_mut(), 0);
            pinyin_bigram_iterator_get_next_phrase(
                iter,
                &raw mut phrase,
                &raw mut pinyin,
                &raw mut count,
            );
        },
    );
}

/// Pinyin unloads only the GBK library on a live context: libraries 1, 15
/// and 7 answer false, the user library keeps its items, and loading 7
/// answers false.
#[test]
fn pinyin_unloads_only_the_gbk_library() {
    let user = TempUserDir::new("unload-gate");
    let (context, instance) = open_at(&user.path);
    assert_eq!(import_batch(context, 7, &[("龘龘", "da2'da2", 5)]), [true]);
    let token = lookup(instance, "龘龘")[0];
    for library in [1, 15, 7] {
        assert!(
            !crate::config::pinyin_unload_phrase_library(context, library),
            "unload {library}"
        );
    }
    let mut count = UNTOUCHED;
    assert!(pinyin_token_get_n_pronunciation(
        instance,
        token,
        &raw mut count
    ));
    assert_eq!(count, 1);
    assert!(!pinyin_load_phrase_library(context, 7));
    close(context, instance);
}

/// `token_get_n_pronunciation` of a token in a library with no sub-index
/// answers false and writes 0.
#[test]
fn n_pronunciation_without_a_sub_index_writes_zero() {
    let user = TempUserDir::new("count-no-sub-index");
    let (context, instance) = open_at(&user.path);
    let mut count = UNTOUCHED;
    assert!(!pinyin_token_get_n_pronunciation(
        instance,
        0x0f00_0001,
        &raw mut count
    ));
    assert_eq!(count, 0);
    close(context, instance);
}

// ── User index keys ───────────────────────────────────────────────────────

/// The first key of `ni` and the 17-key run of it.
fn ni_and_its_overlong_run(first: &[u8]) -> Vec<Vec<u8>> {
    vec![first.to_vec(), first.repeat(17)]
}

/// The candidate guess asserts on an over-long user index key it reaches
/// through the parsed prefix.
#[test]
fn guess_candidates_aborts_on_an_overlong_index_key() {
    assert_aborts(
        "boundary_tests::guess_candidates_aborts_on_an_overlong_index_key",
        || {
            let user = TempUserDir::new("overlong-guess");
            let context =
                overlong_index_context(&system_dir(), &user.path, ni_and_its_overlong_run);
            let instance = alloc(context);
            assert_eq!(parse(instance, "ni"), 2);
            pinyin_guess_candidates(instance, 0, 0);
        },
    );
}

/// After a guess over another key, a reparse onto the over-long key's
/// prefix does not reach it: only an explicit guess gates on these keys.
#[test]
fn a_reparse_after_a_guess_ignores_an_overlong_index_key() {
    let user = TempUserDir::new("overlong-reparse");
    let context = overlong_index_context(&system_dir(), &user.path, ni_and_its_overlong_run);
    let instance = alloc(context);
    assert_eq!(parse(instance, "hao"), 3);
    assert!(pinyin_guess_candidates(instance, 0, 0));
    assert_eq!(parse(instance, "ni"), 2);
    close(context, instance);
}

/// Without its prefix row, the over-long key is never walked to: the
/// guess answers true (`walk_suggestions`' key_exists gate).
#[test]
fn an_overlong_index_key_without_its_prefix_is_not_walked() {
    let user = TempUserDir::new("overlong-no-prefix");
    let context = overlong_index_context(&system_dir(), &user.path, |first| vec![first.repeat(17)]);
    let instance = alloc(context);
    assert_eq!(parse(instance, "ni"), 2);
    assert!(pinyin_guess_candidates(instance, 0, 0));
    close(context, instance);
}

/// `mask_out` detects the over-long index key before it reloads any
/// library: the abort comes first even when a later library is gone.
#[test]
fn mask_out_aborts_on_an_overlong_index_key_before_the_reload() {
    assert_aborts(
        "boundary_tests::mask_out_aborts_on_an_overlong_index_key_before_the_reload",
        || {
            let scratch = tempfile::tempdir().expect("scratch");
            let system = private_system(scratch.path());
            let user = TempUserDir::new("overlong-mask-unmappable");
            let context = overlong_index_context(&system, &user.path, |_| {
                vec![vec![0, 1], [0, 1].repeat(17)]
            });
            std::fs::remove_file(system.join("merged.bin")).expect("unlink merged.bin");
            pinyin_mask_out(context, 0xFFFF_FFFF, 0);
        },
    );
}

/// `mask_out` asserts on an empty user index key.
#[test]
fn mask_out_aborts_on_an_empty_index_key() {
    assert_aborts(
        "boundary_tests::mask_out_aborts_on_an_empty_index_key",
        || {
            let user = TempUserDir::new("mask-empty-key");
            let context =
                overlong_index_context(&system_dir(), &user.path, |_| vec![Vec::new(), vec![0, 1]]);
            pinyin_mask_out(context, 0xFFFF_FFFF, 0);
        },
    );
}

/// An import aborts where the user phrase table maps the phrase to two
/// tokens of one library.
#[test]
fn import_aborts_on_two_tokens_of_one_library() {
    assert_aborts(
        "boundary_tests::import_aborts_on_two_tokens_of_one_library",
        || {
            let user = TempUserDir::new("import-duplicate-token");
            let (context, instance) = open_at(&user.path);
            close(context, instance);
            set_phrase_table_row(&user.path, "你好", &[0x0700_0001, 0x0700_0002]);
            let (context, _) = open_at(&user.path);
            import_batch(context, 7, &[("你好", "ni'hao", 5)]);
        },
    );
}

// ── User bigram under mask_out and export ─────────────────────────────────

/// A context on a profile whose user bigram holds exactly `rows`.
fn crafted_bigram_context(user: &Path, rows: Vec<(Vec<u8>, Vec<u8>)>) -> *mut PinyinContext {
    plant_bigram(user, rows);
    let context = init_at(&system_dir(), user);
    assert!(!context.is_null());
    context
}

/// `mask_out` asserts on a user bigram key that is not a token (the
/// facade's `gram_mask_fault_over`).
#[test]
fn mask_out_aborts_on_a_non_token_bigram_key() {
    assert_aborts(
        "boundary_tests::mask_out_aborts_on_a_non_token_bigram_key",
        || {
            let user = TempUserDir::new("mask-non-token-key");
            let context = crafted_bigram_context(&user.path, vec![(vec![0, 1], vec![7, 0, 0, 0])]);
            pinyin_mask_out(context, 0xFFFF_FFFF, 0);
        },
    );
}

/// `mask_out` asserts on a gram whose total outlives its items.
#[test]
fn mask_out_aborts_on_a_residual_bigram_gram() {
    assert_aborts(
        "boundary_tests::mask_out_aborts_on_a_residual_bigram_gram",
        || {
            let user = TempUserDir::new("mask-residual-gram");
            let value = [&[7_u8, 0, 0, 0][..], &[0; 8]].concat();
            let context = crafted_bigram_context(&user.path, vec![(vec![1, 0, 0, 0], value)]);
            pinyin_mask_out(context, 0xFFFF_FFFF, 0);
        },
    );
}

/// `begin_get_bigram_phrases` asserts on a user bigram key that is not a
/// token.
#[test]
fn bigram_export_aborts_on_a_non_token_key() {
    assert_aborts(
        "boundary_tests::bigram_export_aborts_on_a_non_token_key",
        || {
            let user = TempUserDir::new("export-non-token-key");
            let context = crafted_bigram_context(&user.path, vec![(vec![0, 1], vec![7, 0, 0, 0])]);
            pinyin_begin_get_bigram_phrases(context);
        },
    );
}

// ── Libraries at init, reload and save: stderr ────────────────────────────

/// The newline the C stdio of the host writes for `\n` on stderr: the
/// UCRT's text mode writes `\r\n`.
const NL: &str = if cfg!(windows) { "\r\n" } else { "\n" };

/// Trains the top sentence of `nihao` once on a new instance.
fn train_once(context: *mut PinyinContext) {
    let instance = alloc(context);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(pinyin_guess_sentence(instance));
    assert!(crate::candidates::pinyin_train(instance, 0));
    pinyin_free_instance(instance);
}

/// A private system next to the child's user directory, and a context on
/// both.
fn private_context(user: &Path) -> (std::path::PathBuf, *mut PinyinContext) {
    let system = private_system(user.parent().expect("the child's scratch"));
    let context = init_at(&system, user);
    assert!(!context.is_null());
    (system, context)
}

/// An absent library at init prints `mmap … failed!` and the init still
/// answers a context.
#[test]
fn init_reports_an_absent_library_and_succeeds() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::init_reports_an_absent_library_and_succeeds",
        |user| {
            let system = private_system(user.parent().expect("the child's scratch"));
            std::fs::remove_file(system.join("merged.bin")).expect("unlink merged.bin");
            assert!(!init_at(&system, user).is_null());
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    let line = format!("mmap {} failed!{NL}", system.join("merged.bin").display());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains(&line), "{stderr}");
}

/// A reload of an empty library answers true, prints `mmap … failed!` and
/// leaves the library unloaded.
#[test]
fn reload_of_an_empty_library_reports_and_answers_true() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::reload_of_an_empty_library_reports_and_answers_true",
        |user| {
            let (system, context) = private_context(user);
            assert!(crate::config::pinyin_unload_phrase_library(context, 2));
            let library = system.join("gbk_char.bin");
            std::fs::remove_file(&library).expect("unlink gbk_char.bin");
            std::fs::write(&library, b"").expect("an empty library");
            assert!(pinyin_load_phrase_library(context, 2));
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    let line = format!("mmap {} failed!{NL}", system.join("gbk_char.bin").display());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains(&line), "{stderr}");
}

/// A save whose system library is gone prints `mmap … failed!` and
/// answers false (register row 76's outcome).
#[test]
fn save_reports_an_absent_system_library_and_answers_false() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::save_reports_an_absent_system_library_and_answers_false",
        |user| {
            let (system, context) = private_context(user);
            train_once(context);
            std::fs::remove_file(system.join("gb_char.bin")).expect("unlink gb_char.bin");
            assert!(!pinyin_save(context));
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    let line = format!("mmap {} failed!{NL}", system.join("gb_char.bin").display());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains(&line), "{stderr}");
}

/// A save cut by a four-byte file-size limit aborts after the chunk's
/// length word: the staged log holds four bytes and the
/// marker keeps the raised counter.
// Unix only: RLIMIT_FSIZE and SIGXFSZ.
#[cfg(unix)]
#[test]
fn save_aborts_where_the_chunk_header_write_fails() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::save_aborts_where_the_chunk_header_write_fails",
        |user| {
            let context = init_at(&system_dir(), user);
            assert!(!context.is_null());
            train_once(context);
            let limit = libc::rlimit {
                rlim_cur: 4,
                rlim_max: 4,
            };
            // SAFETY: ignores SIGXFSZ and lowers this child's own file-size
            // limit; the child ends in the save.
            unsafe {
                libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
                assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &raw const limit), 0);
            }
            pinyin_save(context);
        },
    );
    let staged = std::fs::metadata(run.user_dir.join("gb_char.dbin.tmp")).expect("staged log");
    assert_eq!(staged.len(), 4);
    let conf = std::fs::read_to_string(run.user_dir.join("user.conf")).expect("user.conf");
    assert!(conf.ends_with("open counter:1\n"), "{conf}");
}

/// Init refuses a NULL system directory, a missing one and one without
/// table.conf: each answers NULL after the two `open`/`load` lines.
#[test]
fn init_refuses_a_system_directory_without_table_conf() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::init_refuses_a_system_directory_without_table_conf",
        |user| {
            let user_c = cstr(user.to_str().expect("UTF-8 path"));
            assert!(pinyin_init(ptr::null(), user_c.as_ptr()).is_null());
            assert!(init_at(Path::new("/nonexistent-system-dir"), user).is_null());
            let system = private_system(user.parent().expect("the child's scratch"));
            std::fs::remove_file(system.join("table.conf")).expect("unlink table.conf");
            assert!(init_at(&system, user).is_null());
        },
    );
    // GLib joins with the separator that last occurs in its arguments: `/`
    // after the literal directory, the platform's own after a native path.
    let conf = run
        .user_dir
        .parent()
        .expect("scratch")
        .join("system")
        .join("table.conf");
    let missing = |conf: &str| format!("open {conf} failed.{NL}load {conf} failed!{NL}");
    let expected = format!(
        "open  failed.{NL}load  failed!{NL}{}{}",
        missing("/nonexistent-system-dir/table.conf"),
        missing(&conf.display().to_string())
    );
    assert_eq!(String::from_utf8_lossy(&run.stderr), expected);
}

/// A NULL user directory prints the empty-path open line at init and the
/// two write lines at fini, and nothing else.
#[test]
fn a_null_user_directory_reports_its_empty_path() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::a_null_user_directory_reports_its_empty_path",
        |_| {
            let system = cstr(system_dir().to_str().expect("UTF-8 path"));
            let context = pinyin_init(system.as_ptr(), ptr::null());
            assert!(!context.is_null());
            pinyin_fini(context);
        },
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        format!("open  failed.write  failed.{NL}write  failed.{NL}")
    );
}

// ── Journals and configuration files ──────────────────────────────────────

/// Two null-token MODIFY_HEADER records.
fn two_headers() -> Vec<u8> {
    [
        crate::test_support::modify_header(0, 1, 1),
        crate::test_support::modify_header(0, 1, 2),
    ]
    .concat()
}

fn write_log(user: &Path, name: &str, payload: &[u8]) {
    std::fs::write(
        user.join(name),
        oxpinyin_data::chunk_format::build_memory_chunk(payload).expect("frame"),
    )
    .expect("write the log");
}

/// Every file of `dir` by name, with its bytes.
fn files_of(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .expect("read the directory")
        .map(|entry| {
            let entry = entry.expect("an entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, std::fs::read(entry.path()).expect("read a file"))
        })
        .collect()
}

/// `mask_out` aborts on a log of two headers after a save, before it
/// writes any file: stderr is empty and the profile is byte-identical.
#[test]
fn mask_out_aborts_on_two_log_headers_and_leaves_the_files() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::mask_out_aborts_on_two_log_headers_and_leaves_the_files",
        |user| {
            write_user_conf(user, &system_dir());
            let context = init_at(&system_dir(), user);
            assert!(!context.is_null());
            assert_eq!(import_batch(context, 7, &[("你好", "ni3'hao3", 1)]), [true]);
            assert!(pinyin_save(context));
            write_log(user, "gb_char.dbin", &two_headers());
            let before = user.parent().expect("scratch").join("before");
            std::fs::create_dir(&before).expect("the snapshot directory");
            for (name, bytes) in files_of(user) {
                std::fs::write(before.join(name), bytes).expect("snapshot a file");
            }
            pinyin_mask_out(context, 0, 0);
        },
    );
    assert!(
        run.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let before = run.user_dir.parent().expect("scratch").join("before");
    assert_eq!(files_of(&run.user_dir), files_of(&before));
}

/// Init accepts a log of two headers.
#[test]
fn init_accepts_two_log_headers() {
    let user = TempUserDir::new("init-two-headers");
    write_user_conf(&user.path, &system_dir());
    write_log(&user.path, "gb_char.dbin", &two_headers());
    let context = init_at(&system_dir(), &user.path);
    assert!(!context.is_null());
    pinyin_fini(context);
}

/// `mask_out` does not read the log of an unloaded library: true.
#[test]
fn mask_out_skips_the_log_of_an_unloaded_library() {
    let user = TempUserDir::new("mask-unloaded-log");
    write_user_conf(&user.path, &system_dir());
    let context = init_at(&system_dir(), &user.path);
    assert!(!context.is_null());
    write_log(&user.path, "gbk_char.dbin", &two_headers());
    assert!(crate::config::pinyin_unload_phrase_library(context, 2));
    assert!(pinyin_mask_out(context, 0, 0));
    pinyin_fini(context);
}

/// A private system whose table.conf has `from` replaced by `to`.
fn conf_system(parent: &Path, from: &str, to: &str) -> std::path::PathBuf {
    let system = private_system(parent);
    let path = system.join("table.conf");
    let text = std::fs::read_to_string(system_dir().join("table.conf")).expect("table.conf");
    assert!(text.contains(from), "table.conf holds {from}");
    std::fs::remove_file(&path).expect("unlink the linked table.conf");
    std::fs::write(&path, text.replace(from, to)).expect("write table.conf");
    system
}

/// An unknown database format in table.conf aborts the init before it
/// writes the user directory.
#[test]
fn init_aborts_on_an_unknown_table_conf_database_format() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::init_aborts_on_an_unknown_table_conf_database_format",
        |user| {
            let format = format!("database format:{}", oxpinyin_data::DEFAULT_STORE_DB_FORMAT);
            let system = conf_system(
                user.parent().expect("scratch"),
                &format,
                "database format:LMDB",
            );
            init_at(&system, user);
        },
    );
    assert!(files_of(&run.user_dir).is_empty());
}

/// An unknown database format in user.conf aborts the init and leaves
/// user.conf as it was.
#[test]
fn init_aborts_on_an_unknown_user_conf_database_format() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::init_aborts_on_an_unknown_user_conf_database_format",
        |user| {
            write_user_conf(user, &system_dir());
            let path = user.join("user.conf");
            let text = std::fs::read_to_string(&path).expect("user.conf");
            let format = format!("database format:{}", oxpinyin_data::DEFAULT_STORE_DB_FORMAT);
            std::fs::write(&path, text.replace(&format, "database format:LMDB"))
                .expect("write user.conf");
            init_at(&system_dir(), user);
        },
    );
    let conf = std::fs::read_to_string(run.user_dir.join("user.conf")).expect("user.conf");
    assert!(conf.contains("database format:LMDB\n"), "{conf}");
    assert!(conf.ends_with("open counter:0\n"), "{conf}");
}

/// An absolute USER_FILE name stays under the user directory:
/// `g_build_filename(user, "/…/nest/u.bin")` is `<user>/…/nest/u.bin`.
// Unix only: the table.conf row spells a POSIX absolute path.
#[cfg(unix)]
#[test]
fn an_absolute_user_file_lands_under_the_user_directory() {
    let scratch = tempfile::tempdir().expect("scratch");
    let user = scratch.path().join("user");
    let nested = format!("{}/nest", user.display());
    let beneath = format!("{}{nested}", user.display());
    std::fs::create_dir_all(&nested).expect("the nested directory");
    std::fs::create_dir_all(&beneath).expect("the user-rooted directory");
    let system = conf_system(
        scratch.path(),
        "user.bin USER_FILE",
        &format!("{nested}/u.bin USER_FILE"),
    );
    let context = init_at(&system, &user);
    assert!(!context.is_null());
    assert_eq!(import_batch(context, 7, &[("你好", "ni3'hao3", 1)]), [true]);
    assert!(pinyin_save(context));
    pinyin_fini(context);
    assert!(Path::new(&beneath).join("u.bin").is_file());
    assert!(!Path::new(&nested).join("u.bin").exists());
}

// ── User store and training ───────────────────────────────────────────────

/// Training over an unpartitioned sentence-start gram whose items are not
/// sorted answers true where the pin aborts
/// (docs/findings/1297-gram-partition-training.md).
#[test]
fn training_over_an_unsorted_start_gram_answers_true() {
    let user = TempUserDir::new("train-unsorted-gram");
    let (context, instance) = open_at(&user.path);
    let ni = lookup(instance, "你")[0];
    close(context, instance);
    let items = [
        (ni - 4, 1),
        (ni - 3, 1),
        (ni - 2, 1),
        (ni + 1, 1),
        (ni - 1, 1),
    ];
    plant_bigram(
        &user.path,
        vec![(1_u32.to_le_bytes().to_vec(), gram_bytes(5, &items))],
    );
    let (context, instance) = open_at(&user.path);
    let logs = crate::test_support::LogCapture::new();
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(pinyin_guess_sentence(instance));
    for (offset, text) in [(0, "你"), (2, "好")] {
        assert!(pinyin_guess_candidates(
            instance,
            offset,
            crate::test_support::DEFAULT_SORT
        ));
        assert!(pinyin_choose_candidate(instance, offset, row_with(instance, NORMAL, text)) > 0);
    }
    assert!(pinyin_guess_sentence(instance));
    assert!(crate::candidates::pinyin_train(instance, 0));
    assert!(logs.take().is_empty());
    close(context, instance);
}

/// The keys of reading `nth` of `token` as little-endian bytes.
fn reading_bytes(instance: *mut PinyinInstance, token: u32, nth: u32) -> Vec<u8> {
    // SAFETY: a fresh GArray of two-byte elements, freed below.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    assert!(crate::dict::pinyin_token_get_nth_pronunciation(
        instance,
        token,
        nth,
        keys.cast()
    ));
    // SAFETY: a live GArray of two-byte elements.
    let bytes = unsafe {
        let view = &*keys;
        std::slice::from_raw_parts(view.data.cast::<u8>(), view.len as usize * 2).to_vec()
    };
    // SAFETY: allocated above, freed once.
    unsafe { glib_sys::g_array_free(keys, 1) };
    bytes
}

/// A pinyin import parses its reading under CORRECT_ALL | USE_TONE: `egn`
/// becomes the key 0x0500.
#[test]
fn an_import_reading_parses_with_the_corrections() {
    let user = TempUserDir::new("import-egn");
    let (context, instance) = open_at(&user.path);
    assert_eq!(import_batch(context, 7, &[("嗯", "egn", 5)]), [true]);
    let token = lookup(instance, "嗯")[0];
    let mut count = UNTOUCHED;
    assert!(pinyin_token_get_n_pronunciation(
        instance,
        token,
        &raw mut count
    ));
    assert_eq!(count, 1);
    assert_eq!(reading_bytes(instance, token, 0), [0x00, 0x05]);
    close(context, instance);
}

/// A stored user key word is read back as it is stored: 0x8000.
#[test]
fn a_stored_user_key_word_is_read_back_verbatim() {
    let user = TempUserDir::new("stored-key-8000");
    let (context, instance) = open_at(&user.path);
    assert_eq!(import_batch(context, 7, &[("你", "ni3", 5)]), [true]);
    let token = lookup(instance, "你")
        .into_iter()
        .find(|token| token >> 24 == 7)
        .expect("a user token");
    assert!(pinyin_save(context));
    close(context, instance);
    let path = user.path.join("user.bin");
    let mut payload = std::fs::read(&path).expect("user.bin")[8..].to_vec();
    let text = 0x4f60_u32.to_le_bytes();
    let at = payload
        .windows(4)
        .position(|window| window == text)
        .expect("the imported item")
        + 4;
    payload[at..at + 2].copy_from_slice(&0x8000_u16.to_le_bytes());
    std::fs::write(
        &path,
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");
    let (context, instance) = open_at(&user.path);
    let (mut length, mut phrase) = (UNTOUCHED, ptr::null_mut());
    assert!(crate::dict::pinyin_token_get_phrase(
        instance,
        token,
        &raw mut length,
        &raw mut phrase
    ));
    assert_eq!(
        (length, crate::ffi::take_owned_cstr(phrase).as_str()),
        (1, "你")
    );
    assert_eq!(reading_bytes(instance, token, 0), [0x00, 0x80]);
    close(context, instance);
}

/// The total of library 1 in the mini fixture's gb_char.bin.
fn library_one_total() -> u32 {
    let file = std::fs::read(system_dir().join("gb_char.bin")).expect("gb_char.bin");
    u32::from_le_bytes(file[8..12].try_into().expect("a word"))
}

/// A predicted prefix choose of a library-1 row answers false where the
/// library total cannot take the unigram delta: the sub-index overflow
/// (`phrase_index.cpp:78`).
#[test]
fn a_prefix_choose_past_the_library_total_is_refused() {
    let user = TempUserDir::new("predicted-library-overflow");
    write_user_conf(&user.path, &system_dir());
    let total = library_one_total();
    write_log(
        &user.path,
        "gb_char.dbin",
        &crate::test_support::modify_header(0, total, u32::MAX - 483),
    );
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_guess_predicted_candidates(instance, c"你".as_ptr()));
    let row = row_of(instance, PREDICTED_PREFIX);
    assert!(pinyin_choose_predicted_candidate(instance, row));
    assert!(!pinyin_choose_predicted_candidate(instance, row));
    close(context, instance);
}

/// Library 1's export of `phrase`: each row's reading and count.
fn export_rows_of(context: *mut PinyinContext, library: u32, phrase: &str) -> Vec<(String, i32)> {
    crate::e2e_tests::export_of(context, library)
        .into_iter()
        .filter(|(text, _, _)| text == phrase)
        .map(|(_, reading, count)| (reading, count))
        .collect()
}

/// A system item's reading count is its `guint32` bit pattern in the
/// export, and training adds to it with wrap: ni3 at 2^32 - 76 exports
/// -76, then -7 (#790).
#[test]
fn a_system_reading_count_exports_its_bit_pattern() {
    let user = TempUserDir::new("export-prefix-overflow");
    let (context, instance) = open_at(&user.path);
    let token = lookup(instance, "你")[0];
    assert_eq!(token >> 24, 1);
    let key = |text: &str| {
        let mut key = 0_u16;
        let text = cstr(text);
        assert!(pinyin_parse_full_pinyin(
            instance,
            text.as_ptr(),
            (&raw mut key).cast()
        ));
        key
    };
    let readings = [
        (key("ni3"), 0xFFFF_FFB4_u32),
        (key("ni2"), 0),
        (key("na4"), 0),
    ];
    close(context, instance);
    // A MODIFY of the item: its readings replaced, its unigram kept.
    let file = std::fs::read(system_dir().join("gb_char.bin")).expect("gb_char.bin");
    let payload = &file[8..];
    let word = |at: usize| u32::from_le_bytes(payload[at..at + 4].try_into().expect("a word"));
    let slot = (word(4) + (token & 0x00FF_FFFF) * 4) as usize;
    let offset = (word(8) + word(slot)) as usize;
    let old = &payload[offset..offset + 10 + usize::from(payload[offset + 1]) * 6];
    let mut new = vec![1, 3];
    new.extend(&old[2..10]);
    for (key, count) in readings {
        new.extend(key.to_le_bytes());
        new.extend(count.to_le_bytes());
    }
    let mut record = 3_u32.to_le_bytes().to_vec();
    record.extend((token & 0x00FF_FFFF).to_le_bytes());
    record.extend(u16::try_from(old.len()).expect("a length").to_le_bytes());
    record.extend(u16::try_from(new.len()).expect("a length").to_le_bytes());
    record.extend(old);
    record.extend(&new);
    write_user_conf(&user.path, &system_dir());
    let total = word(0);
    write_log(
        &user.path,
        "gb_char.dbin",
        &[crate::test_support::modify_header(0, total, total), record].concat(),
    );
    let (context, instance) = open_at(&user.path);
    let before = [
        ("ni3".to_owned(), -76),
        ("ni2".to_owned(), -1),
        ("na4".to_owned(), -1),
    ];
    assert_eq!(export_rows_of(context, 1, "你"), before);
    assert_eq!(parse(instance, "ni"), 2);
    assert!(pinyin_guess_candidates(
        instance,
        0,
        crate::test_support::DEFAULT_SORT
    ));
    assert!(pinyin_choose_candidate(instance, 0, row_with(instance, NORMAL, "你")) > 0);
    assert!(pinyin_guess_sentence(instance));
    assert!(crate::candidates::pinyin_train(instance, 0));
    let after = [
        ("ni3".to_owned(), -7),
        ("ni2".to_owned(), -1),
        ("na4".to_owned(), -1),
    ];
    assert_eq!(export_rows_of(context, 1, "你"), after);
    close(context, instance);
}

/// `remember_user_input` keeps the toned keys of the parse even after the
/// options that made them are cleared.
#[test]
fn remember_keeps_the_parse_time_tones() {
    let user = TempUserDir::new("remember-cleared-tone");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_set_options(context, 0x20));
    assert_eq!(parse(instance, "ni3hao3"), 7);
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_set_options(context, 0));
    assert!(crate::user_data::pinyin_remember_user_input(
        instance,
        c"你好".as_ptr(),
        3
    ));
    assert_eq!(
        crate::e2e_tests::export_of(context, 7),
        [("你好".to_owned(), "ni3'hao3".to_owned(), 3)]
    );
    close(context, instance);
}

/// Hex of each `(key, value)` row.
fn hex_rows(rows: Vec<(Vec<u8>, Vec<u8>)>) -> Vec<(String, String)> {
    let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    rows.iter()
        .map(|(key, value)| (hex(key), hex(value)))
        .collect()
}

/// The saved user index trees hold the pin's records: each reading key
/// with its prefix marker, and each phrase key with its token.
#[test]
fn saved_user_indexes_hold_the_pins_records() {
    let user = TempUserDir::new("saved-index-format");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        import_batch(
            context,
            7,
            &[
                ("龘龘", "da2'da2", 100),
                ("龘龘", "da4'da4", 100),
                ("靐靐", "bing4'bing4", 100)
            ]
        ),
        [true, true, true]
    );
    assert!(pinyin_save(context));
    close(context, instance);
    let rows = |name: &str| crate::test_support::store_rows(&user.path.join(name), false);
    let pairs = |rows: &[(&str, &str)]| -> Vec<(String, String)> {
        rows.iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    };
    assert_eq!(
        hex_rows(rows("user_pinyin_index.bin")),
        pairs(&[
            ("0100", ""),
            ("01000100", "0200000781488148"),
            ("0400", ""),
            ("04000400", "0100000784208420"),
            ("8108", ""),
            ("81088108", "0200000781488148"),
            ("8400", ""),
            ("84008400", "0100000784208420"),
        ])
    );
    assert_eq!(
        hex_rows(rows("user_phrase_index.bin")),
        pairs(&[
            ("50970000", ""),
            ("5097000050970000", "02000007"),
            ("989f0000", ""),
            ("989f0000989f0000", "01000007"),
        ])
    );
    let (context, instance) = open_at(&user.path);
    assert_eq!(lookup(instance, "龘龘"), [0x0700_0001]);
    assert_eq!(lookup(instance, "靐靐"), [0x0700_0002]);
    close(context, instance);
}

/// `mask_out` over an unmappable library answers true, prints the one
/// `mmap … failed!` line and changes no file and no item.
#[test]
fn mask_out_over_an_unmappable_library_changes_nothing() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::mask_out_over_an_unmappable_library_changes_nothing",
        |user| {
            write_user_conf(user, &system_dir());
            let (system, context) = private_context(user);
            let instance = alloc(context);
            assert_eq!(import_batch(context, 1, &[("龘龘", "da2'da2", 1)]), [true]);
            assert!(pinyin_save(context));
            assert_eq!(
                import_batch(context, 7, &[("龘龘龘", "da2'da2'da2", 1)]),
                [true]
            );
            assert!(pinyin_token_add_unigram_frequency(instance, 0x0100_1225, 7));
            let items = [lookup(instance, "龘龘"), lookup(instance, "龘龘龘")];
            let count = unigram(instance, 0x0100_1225);
            let files = files_of(user);
            let library = system.join("gb_char.bin");
            let original = std::fs::read(&library).expect("gb_char.bin");
            std::fs::remove_file(&library).expect("unlink gb_char.bin");
            assert!(pinyin_mask_out(context, 0, 0));
            assert_eq!(files_of(user), files);
            assert_eq!(unigram(instance, 0x0100_1225), count);
            assert_eq!(
                [lookup(instance, "龘龘"), lookup(instance, "龘龘龘")],
                items
            );
            std::fs::write(&library, original).expect("restore gb_char.bin");
            assert!(pinyin_save(context));
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        format!("mmap {} failed!{NL}", system.join("gb_char.bin").display())
    );
}

/// A user gram for 你 and for 中, each `[0xFFFFFFFF, (好, 0xFFFFFFFF)]`:
/// the bigram export count wraps to -2.
#[test]
fn the_bigram_export_count_wraps() {
    let user = TempUserDir::new("d18-export-ffffffff");
    let (context, instance) = open_at(&user.path);
    let [ni, hao, zhong] = ["你", "好", "中"].map(|text| lookup(instance, text)[0]);
    close(context, instance);
    let gram = gram_bytes(0xFFFF_FFFF, &[(hao, 0xFFFF_FFFF)]);
    plant_bigram(
        &user.path,
        vec![
            (ni.to_le_bytes().to_vec(), gram.clone()),
            (zhong.to_le_bytes().to_vec(), gram),
            (1_u32.to_le_bytes().to_vec(), vec![0; 4]),
        ],
    );
    let (context, instance) = open_at(&user.path);
    let iter = pinyin_begin_get_bigram_phrases(context);
    let mut rows = Vec::new();
    while pinyin_bigram_iterator_has_next_phrase(iter) {
        let (mut phrase, mut pinyin, mut count) = (ptr::null_mut(), ptr::null_mut(), 0);
        pinyin_bigram_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut pinyin,
            &raw mut count,
        );
        rows.push((
            crate::ffi::take_owned_cstr(phrase),
            crate::ffi::take_owned_cstr(pinyin),
            count,
        ));
    }
    crate::iterators::pinyin_end_get_bigram_phrases(iter);
    rows.sort();
    assert_eq!(
        rows,
        [
            ("中好".to_owned(), "zhong'hao".to_owned(), -2),
            ("你好".to_owned(), "ni'hao".to_owned(), -2),
        ]
    );
    close(context, instance);
}

// ── Keyless guesses and prediction prefixes ───────────────────────────────

/// A keyless parse (one separator) refuses both sentence guesses before
/// they clear the old result: the old sentence stays, the candidate list
/// is empty, and a fresh instance has no sentence (:183).
#[test]
fn keyless_sentence_guesses_keep_the_old_sentence() {
    let user = TempUserDir::new("zero-key-guess");
    let (context, _) = open_at(&user.path);
    let guesses: [fn(*mut PinyinInstance) -> bool; 2] = [
        |instance| pinyin_guess_sentence(instance),
        |instance| crate::sentence::pinyin_guess_sentence_with_prefix(instance, c"你".as_ptr()),
    ];
    for guess in guesses {
        let instance = alloc(context);
        assert_eq!(parse(instance, "ni3hao3"), 7);
        assert!(pinyin_guess_sentence(instance));
        let sentence = sentence_out(instance, 0);
        assert!(sentence.0);
        assert_eq!(parse(instance, "'"), 1);
        assert!(!guess(instance));
        assert_eq!(sentence_out(instance, 0), sentence);
        assert_eq!(n_candidate(instance), 0);
        pinyin_free_instance(instance);

        let fresh = alloc(context);
        assert_eq!(parse(fresh, "'"), 1);
        assert!(!guess(fresh));
        assert_eq!(sentence_out(fresh, 0), (false, None));
        pinyin_free_instance(fresh);
    }
    pinyin_fini(context);
}

/// A private system whose library 1 holds 甲?乙 (the middle word `word`)
/// at 100, 丙 at 1000, 戌 at 10 and 甲?乙丙; its bigram ranks 戌 after
/// 甲?乙.
fn prefix_system(parent: &Path, word: u32) -> std::path::PathBuf {
    let prefix = [0x7532, word, 0x4e59];
    let phrase = [0x7532, word, 0x4e59, 0x4e19];
    let system = crate::test_support::items_system(
        &system_dir(),
        parent,
        &[
            (&prefix, &[0x1084; 3], 100),
            (&[0x4e19], &[0x1081], 1000),
            (&[0x620c], &[0x1081], 10),
            (&phrase, &[0x1084, 0x1084, 0x1084, 0x1081], 100),
        ],
        2000,
    );
    craft_hash(
        &system.join("bigram.db"),
        &[(
            0x0100_0001_u32.to_le_bytes().to_vec(),
            gram_bytes(1000, &[(0x0100_0003, 1000)]),
        )],
    );
    system
}

/// GLib's historical UTF-8 form of `words`, up to six bytes per word.
fn glib_utf8(words: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    for &word in words {
        let (first, length): (u32, usize) = match word {
            0..0x80 => (0, 1),
            0x80..0x800 => (0xc0, 2),
            0x800..0x1_0000 => (0xe0, 3),
            0x1_0000..0x20_0000 => (0xf0, 4),
            0x20_0000..0x400_0000 => (0xf8, 5),
            _ => (0xfc, 6),
        };
        let mut rest = word;
        let mut tail = Vec::new();
        for _ in 1..length {
            tail.push(0x80 | u8::try_from(rest & 0x3f).expect("six bits"));
            rest >>= 6;
        }
        out.push(u8::try_from(first | rest).expect("a lead byte"));
        out.extend(tail.iter().rev());
    }
    out
}

/// A NULL prefix to `guess_sentence_with_prefix` is no prefix: true, and the
/// sentence is the unigram choice 丙 (docs/findings/upstream-divergences.md,
/// "NULL prediction prefix").
#[test]
fn a_null_sentence_prefix_is_no_prefix() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = prefix_system(scratch.path(), 0x4e01);
    let user = TempUserDir::new("prefix-null");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "ba1"), 3);
    let valid = std::ffi::CString::new(glib_utf8(&[0x7532, 0x4e01, 0x4e59])).expect("no NUL");
    assert!(crate::sentence::pinyin_guess_sentence_with_prefix(
        instance,
        valid.as_ptr()
    ));
    assert!(crate::sentence::pinyin_guess_sentence_with_prefix(
        instance,
        ptr::null()
    ));
    assert_eq!(sentence_out(instance, 0), (true, Some("丙".to_owned())));
    close(context, instance);
}

/// The sentence prefix is converted with `prefix_text`, which accepts a
/// word above Unicode: the prefix 甲·乙 finds its bigram and the sentence
/// is 戌.
#[test]
fn a_sentence_prefix_above_unicode_is_converted() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = prefix_system(scratch.path(), 0x11_0000);
    let user = TempUserDir::new("prefix-above-unicode");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "ba1"), 3);
    assert!(pinyin_guess_sentence(instance));
    let prefix = std::ffi::CString::new(glib_utf8(&[0x7532, 0x11_0000, 0x4e59])).expect("no NUL");
    assert!(crate::sentence::pinyin_guess_sentence_with_prefix(
        instance,
        prefix.as_ptr()
    ));
    assert_eq!(sentence_out(instance, 0), (true, Some("戌".to_owned())));
    close(context, instance);
}

/// The punctuation prediction on a candidate's own borrowed string answers true
/// with no row and leaves the sentence (docs/findings/upstream-divergences.md,
/// "Aliased prediction prefix use-after-free").
#[test]
fn a_borrowed_punctuation_prefix_answers_true_with_no_row() {
    let user = TempUserDir::new("punct-alias");
    let (context, instance) = open_at(&user.path);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(pinyin_guess_sentence(instance));
    let sentence = sentence_out(instance, 0);
    assert!(pinyin_guess_candidates(
        instance,
        0,
        crate::test_support::DEFAULT_SORT
    ));
    let mut text: *const c_char = ptr::null();
    assert!(crate::candidates::pinyin_get_candidate_string(
        instance,
        candidate_at(instance, 0),
        &raw mut text
    ));
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance, text
    ));
    assert_eq!(n_candidate(instance), 0);
    assert_eq!(sentence_out(instance, 0), sentence);
    close(context, instance);
}

// ── Auxiliary text ────────────────────────────────────────────────────────

type Render = extern "C" fn(*mut PinyinInstance, usize, *mut *mut c_char) -> bool;
const FULL_AUX: Render = pinyin_get_full_pinyin_auxiliary_text;
const DOUBLE_AUX: Render = pinyin_get_double_pinyin_auxiliary_text;
const CHEWING_AUX: Render = crate::text::pinyin_get_chewing_auxiliary_text;

fn aux(instance: *mut PinyinInstance, render: Render, cursor: usize) -> Option<String> {
    let mut out = UNTOUCHED as usize as *mut c_char;
    let ret = render(instance, cursor, &raw mut out);
    assert!(
        out as usize != UNTOUCHED as usize,
        "the renderer writes its out-parameter"
    );
    ret.then(|| crate::ffi::take_owned_cstr(out))
}

/// The double-pinyin renderer cuts at two bytes into a key with a tone
/// after the cut; the full and chewing renderers read the same matrix.
#[test]
fn auxiliary_text_cuts_before_a_tone() {
    let user = TempUserDir::new("aux-tones");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        crate::parse::pinyin_parse_more_double_pinyins(instance, c"ni3hk4".as_ptr()),
        6
    );
    for (render, cursor, text) in [
        (DOUBLE_AUX, 2, "ni|3 hao4 "),
        (DOUBLE_AUX, 4, "ni3 h|ao4 "),
        (DOUBLE_AUX, 5, "ni3 hao|4 "),
        (FULL_AUX, 5, "ni3 ha|o4 "),
        (CHEWING_AUX, 2, "ㄋㄧ|ˇ ㄏㄠˋ "),
        (CHEWING_AUX, 5, "ㄋㄧˇ ㄏㄠ|ˋ "),
    ] {
        assert_eq!(
            aux(instance, render, cursor).as_deref(),
            Some(text),
            "{cursor}"
        );
    }
    close(context, instance);
}

/// The getter reads the keys made by the last parse. Setting options only
/// stores the context word (074a2219 pinyin.cpp:1299-1306, 1497-1524,
/// 3381-3438). A candidate guess may apply those options to the Rust
/// session without changing the parse that the getter should display.
#[rstest::rstest]
fn full_auxiliary_text_uses_the_parse_options(#[values(false, true)] after_guess: bool) {
    let user = TempUserDir::new("aux-parse-options");
    let (context, instance) = open_at(&user.path);
    let incomplete = oxpinyin_core::PINYIN_INCOMPLETE;
    assert!(pinyin_set_options(
        context,
        incomplete | oxpinyin_core::USE_TONE
    ));
    assert_eq!(parse(instance, "ni3hao3"), 7);
    assert_eq!(aux(instance, FULL_AUX, 7).as_deref(), Some("ni3 hao3 |"));

    assert!(pinyin_set_options(context, incomplete));
    if after_guess {
        assert!(pinyin_guess_candidates(
            instance,
            0,
            crate::test_support::DEFAULT_SORT
        ));
    }
    assert_eq!(aux(instance, FULL_AUX, 7).as_deref(), Some("ni3 hao3 |"));

    assert_eq!(parse(instance, "ni3hao3"), 2);
    assert_eq!(aux(instance, FULL_AUX, 7).as_deref(), Some("ni |"));
    close(context, instance);
}

/// The chewing renderer clamps a cursor past the input to the end.
#[test]
fn chewing_auxiliary_text_clamps_a_cursor_past_the_input() {
    let user = TempUserDir::new("aux-chewing-overrun");
    let (context, _) = open_at(&user.path);
    let full = alloc(context);
    assert_eq!(parse(full, "nihao"), 5);
    let chewing = alloc(context);
    assert_eq!(
        crate::parse::pinyin_parse_more_chewings(chewing, c"su3cl3".as_ptr()),
        6
    );
    for (instance, cursors, text) in [
        (full, [6, 12, usize::MAX], "ㄋㄧ ㄏㄠ |"),
        (chewing, [7, 13, usize::MAX], "ㄋㄧˇ ㄏㄠˇ |"),
    ] {
        for cursor in cursors {
            assert_eq!(aux(instance, CHEWING_AUX, cursor).as_deref(), Some(text));
        }
        pinyin_free_instance(instance);
    }
    pinyin_fini(context);
}

/// Under USE_DIVIDED_TABLE the renderers show the parsed keys, not the
/// option alternates.
#[test]
fn auxiliary_text_shows_the_parsed_keys_under_the_divided_table() {
    let user = TempUserDir::new("aux-divided");
    let (context, _) = open_at(&user.path);
    assert!(pinyin_set_options(context, 0xA0));
    for (input, expected) in [
        (
            "xianan",
            [
                (FULL_AUX, 3, "xia |nan "),
                (CHEWING_AUX, 3, "ㄒㄧㄚ |ㄋㄢ "),
                (DOUBLE_AUX, 2, "xia| nan "),
            ],
        ),
        (
            "xian",
            [
                (FULL_AUX, 2, "xi|an "),
                (CHEWING_AUX, 2, "ㄒㄧ|ㄢ "),
                (DOUBLE_AUX, 2, "xian| "),
            ],
        ),
    ] {
        let instance = alloc(context);
        assert_eq!(parse(instance, input), input.len());
        for (render, cursor, text) in expected {
            assert_eq!(
                aux(instance, render, cursor).as_deref(),
                Some(text),
                "{input} {cursor}"
            );
        }
        pinyin_free_instance(instance);
    }
    pinyin_fini(context);
}

/// With no parse each renderer answers false and writes an empty string
///.
#[test]
fn auxiliary_text_without_a_parse_is_an_empty_string() {
    let user = TempUserDir::new("aux-no-parse");
    let (context, instance) = open_at(&user.path);
    for render in [FULL_AUX, DOUBLE_AUX, CHEWING_AUX] {
        let mut out = UNTOUCHED as usize as *mut c_char;
        assert!(!render(instance, 0, &raw mut out));
        assert!(!out.is_null() && out as usize != UNTOUCHED as usize);
        assert_eq!(crate::ffi::take_owned_cstr(out), "");
    }
    close(context, instance);
}

// ── C strings, GArrays and the chewing keyboard ───────────────────────────

/// GLib's `G_LOG_LEVEL_CRITICAL`.
const CRITICAL: glib_sys::GLogLevelFlags = glib_sys::G_LOG_LEVEL_CRITICAL;

/// NULL string arguments after defined prior work (#1279,
/// docs/findings/upstream-divergences.md, "NULL string conversion after defined
/// prior work"): the segment and parse entries answer as for an empty string,
/// the others refuse.
#[test]
fn null_string_arguments_after_prior_work() {
    let user = TempUserDir::new("null-strings");
    let (context, instance) = open_at(&user.path);
    let logs = crate::test_support::LogCapture::new();

    assert!(crate::phrase::pinyin_phrase_segment(
        instance,
        c"你好".as_ptr()
    ));
    logs.take();
    assert!(crate::phrase::pinyin_phrase_segment(instance, ptr::null()));
    let mut count = UNTOUCHED;
    assert!(crate::phrase::pinyin_get_n_phrase(instance, &raw mut count));
    assert_eq!(count, 0);
    assert_eq!(logs.take(), [(Some("GLib".to_owned()), CRITICAL)]);

    assert_eq!(parse(instance, "nihao"), 5);
    assert_eq!(pinyin_parse_more_full_pinyins(instance, ptr::null()), 0);
    assert_eq!(crate::parse::pinyin_get_parsed_input_length(instance), 0);

    let mut key = 0xCDEF_u16;
    assert!(!pinyin_parse_full_pinyin(
        instance,
        ptr::null(),
        (&raw mut key).cast()
    ));
    assert_eq!(key, 0xCDEF);

    let iter = crate::iterators::pinyin_begin_add_phrases(context, 7);
    assert!(!crate::iterators::pinyin_iterator_add_phrase(
        iter,
        ptr::null(),
        c"da1'da1'da1".as_ptr(),
        5
    ));
    crate::iterators::pinyin_end_add_phrases(iter);

    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "da1da1da1"), 9);
    assert!(!crate::user_data::pinyin_remember_user_input(
        instance,
        ptr::null(),
        5
    ));
    close(context, instance);
}

/// Invalid bytes in pinyin input: an import reading keeps its parsed
/// prefix, an import text is refused, and the double and chewing parsers
/// stop at the byte.
#[test]
fn invalid_input_bytes_stop_each_pinyin_parser() {
    let user = TempUserDir::new("invalid-bytes");
    let (context, instance) = open_at(&user.path);
    let iter = crate::iterators::pinyin_begin_add_phrases(context, 7);
    let add = |text: &[u8], reading: &[u8]| {
        let text = std::ffi::CString::new(text).expect("no NUL");
        let reading = std::ffi::CString::new(reading).expect("no NUL");
        crate::iterators::pinyin_iterator_add_phrase(iter, text.as_ptr(), reading.as_ptr(), 1)
    };
    assert!(add("你好".as_bytes(), b"ni'ha\xffo"));
    assert!(add("世界".as_bytes(), b"shi'jie\xff"));
    assert!(!add(b"\xe4\xbd\xa0\xe5\xa5\xbd\xff", b"ni'hao"));
    crate::iterators::pinyin_end_add_phrases(iter);
    assert_eq!(
        crate::e2e_tests::export_of(context, 7),
        [
            ("你好".to_owned(), "ni'ha".to_owned(), 1),
            ("世界".to_owned(), "shi'jie".to_owned(), 1),
        ]
    );
    let consumed = |parser: extern "C" fn(*mut PinyinInstance, *const c_char) -> usize,
                    text: &[u8]| {
        let fresh = alloc(context);
        let text = std::ffi::CString::new(text).expect("no NUL");
        let parsed = parser(fresh, text.as_ptr());
        pinyin_free_instance(fresh);
        parsed
    };
    let double = crate::parse::pinyin_parse_more_double_pinyins;
    assert_eq!(consumed(double, b"ni\xffhk"), 2);
    assert_eq!(consumed(double, b"nihk\xff"), 4);
    assert_eq!(consumed(double, b"\xffnihk"), 0);
    let chewings = crate::parse::pinyin_parse_more_chewings;
    assert_eq!(consumed(chewings, b"su\xff3"), 2);
    assert_eq!(consumed(chewings, b"su3\xff"), 3);
    assert_eq!(consumed(chewings, b"\xffsu3"), 0);
    close(context, instance);
}

std::thread_local! {
    static CLEARED: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
}

extern "C" fn record_cleared(element: glib_sys::gpointer) {
    // SAFETY: GLib hands the clear function one live four-byte element.
    let value = unsafe { element.cast::<u32>().read_unaligned() };
    CLEARED.with(|cleared| cleared.borrow_mut().push(value));
}

fn take_cleared() -> Vec<u32> {
    CLEARED.with(|cleared| std::mem::take(&mut *cleared.borrow_mut()))
}

/// The elements of a zero-terminated GArray of `u32`, its terminator
/// checked.
fn terminated_u32(array: *mut glib_sys::GArray) -> Vec<u32> {
    // SAFETY: a live zero-terminated GArray of four-byte elements.
    unsafe {
        let view = &*array;
        let data = view.data.cast::<u32>();
        assert_eq!(
            data.add(view.len as usize).read_unaligned(),
            0,
            "the terminator"
        );
        (0..view.len as usize)
            .map(|at| data.add(at).read_unaligned())
            .collect()
    }
}

/// `lookup_tokens` clears the caller's GArray through its clear function
/// before it appends, and on a refusal leaves it cleared and empty.
#[test]
fn lookup_tokens_resets_the_callers_array() {
    let user = TempUserDir::new("garray-lifecycle");
    let (context, instance) = open_at(&user.path);
    take_cleared();
    // SAFETY: a fresh zero-terminated, cleared GArray of four-byte elements
    // with a clear function that reads one element; freed below.
    let array = unsafe {
        let array = glib_sys::g_array_new(1, 1, 4);
        glib_sys::g_array_set_clear_func(array, Some(record_cleared));
        let seed: u32 = 0x1234;
        glib_sys::g_array_append_vals(array, (&raw const seed).cast(), 1);
        array
    };
    assert!(crate::dict::pinyin_lookup_tokens(
        instance,
        c"你好".as_ptr(),
        array.cast()
    ));
    assert_eq!(take_cleared(), [0x1234]);
    let found = terminated_u32(array);
    assert_eq!(found, [0x0100_6205]);
    assert!(!crate::dict::pinyin_lookup_tokens(
        instance,
        c"not-a-phrase".as_ptr(),
        array.cast()
    ));
    assert_eq!(take_cleared(), found);
    assert!(terminated_u32(array).is_empty());
    // SAFETY: the array is freed once, with its data.
    unsafe { glib_sys::g_array_free(array, 1) };
    close(context, instance);
}

/// `token_get_nth_pronunciation` into a NULL array logs the GArray
/// criticals the pin's calls make: set_size and append_vals on a hit,
/// set_size alone on an unknown token.
#[test]
fn nth_pronunciation_into_a_null_array_logs_the_glib_criticals() {
    let user = TempUserDir::new("garray-null-pronunciation");
    let user_token = saved_user_phrase(&user.path);
    let (context, instance) = open_at(&user.path);
    let token = lookup(instance, "你好")[0];
    let logs = crate::test_support::LogCapture::new();
    let glib = (Some("GLib".to_owned()), CRITICAL);
    for token in [token, user_token] {
        assert!(crate::dict::pinyin_token_get_nth_pronunciation(
            instance,
            token,
            0,
            ptr::null_mut()
        ));
        let records = logs.take_with_messages();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].0, glib);
        assert!(records[0].1.contains("g_array_set_size"));
        assert_eq!(records[1].0, glib);
        assert!(records[1].1.contains("g_array_append_vals"));
    }
    assert!(!crate::dict::pinyin_token_get_nth_pronunciation(
        instance,
        0xFFFF_FFFF,
        0,
        ptr::null_mut()
    ));
    let records = logs.take_with_messages();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, glib);
    assert!(records[0].1.contains("g_array_set_size"));
    close(context, instance);
}

/// The symbols `in_chewing_keyboard` answers for `key`, freed.
fn keyboard(instance: *mut PinyinInstance, key: u8) -> (bool, Option<Vec<String>>) {
    let mut symbols: *mut *mut c_char = UNTOUCHED as usize as *mut *mut c_char;
    let ret = crate::parse::pinyin_in_chewing_keyboard(
        instance,
        c_char::from_ne_bytes([key]),
        &raw mut symbols,
    );
    if symbols.is_null() {
        return (ret, None);
    }
    assert!(
        symbols as usize != UNTOUCHED as usize,
        "the out-parameter is written"
    );
    let mut texts = Vec::new();
    // SAFETY: a NULL-terminated vector of GLib strings the caller owns.
    unsafe {
        let mut at = symbols;
        while !(*at).is_null() {
            texts.push(std::ffi::CStr::from_ptr(*at).to_string_lossy().into_owned());
            at = at.add(1);
        }
        glib_sys::g_strfreev(symbols);
    }
    (ret, Some(texts))
}

/// The pinyin wrapper of `in_chewing_keyboard`: a key of the
/// scheme answers its symbol, a tone key needs USE_TONE, and other keys
/// answer false with NULL.
#[test]
fn the_pinyin_chewing_keyboard_wrapper_answers_per_key() {
    let user = TempUserDir::new("chewing-keyboard");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_set_options(context, 0));
    assert!(crate::config::pinyin_set_zhuyin_scheme(context, 1));
    assert_eq!(
        keyboard(instance, b's'),
        (true, Some(vec!["ㄋ".to_owned()]))
    );
    assert_eq!(keyboard(instance, b'3'), (false, None));
    assert_eq!(keyboard(instance, b'!'), (false, None));
    assert!(pinyin_set_options(context, 0x60));
    assert_eq!(keyboard(instance, b'3'), (true, Some(vec!["ˇ".to_owned()])));
    close(context, instance);
}

// ── Stored words, counts and multi-library phrases ────────────────────────

/// Hex of the NUL-terminated `text`, freed with `g_free`.
fn take_hex(text: *mut c_char) -> String {
    assert!(!text.is_null());
    // SAFETY: a NUL-terminated GLib string the caller owns.
    let bytes = unsafe { std::ffi::CStr::from_ptr(text) }
        .to_bytes()
        .to_vec();
    // SAFETY: freed once.
    unsafe { glib_sys::g_free(text.cast()) };
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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

/// A library-1 journal ADD whose text is replaced with a surrogate word is
/// loaded as stored: the token's text keeps it, the unigram is the import's
/// count, and a save writes the journal as loaded.
#[test]
fn a_journal_word_no_import_writes_is_kept() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = phrase_text_system(scratch.path(), 0x4e01);
    let user = TempUserDir::new("stored-words-surrogate");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert_eq!(
        import_batch(context, 1, &[("甲戊乙", "da1'da1'da1", 1000)]),
        [true]
    );
    let token = lookup(instance, "甲戊乙")
        .into_iter()
        .find(|token| token >> 24 == 1)
        .expect("the imported token");
    assert!(pinyin_save(context));
    close(context, instance);
    let path = user.path.join("gb_char.dbin");
    let original: Vec<u8> = [0x7532_u32, 0x620a, 0x4e59]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let replacement: Vec<u8> = [0x7532_u32, 0xd800, 0x4e59]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let mut payload = std::fs::read(&path).expect("gb_char.dbin")[8..].to_vec();
    let at = payload
        .windows(original.len())
        .position(|window| window == original.as_slice())
        .expect("the imported item");
    payload.splice(at..at + original.len(), replacement);
    write_log(&user.path, "gb_char.dbin", &payload);
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    let (mut length, mut text) = (UNTOUCHED, ptr::null_mut());
    assert!(crate::dict::pinyin_token_get_phrase(
        instance,
        token,
        &raw mut length,
        &raw mut text
    ));
    assert_eq!((length, take_hex(text).as_str()), (3, "e794b2eda080e4b999"));
    assert_eq!(unigram(instance, token), (true, 3000));
    // Ending an import batch arms the save without changing an item.
    assert!(import_batch(context, 7, &[]).is_empty());
    assert!(pinyin_save(context));
    let native: String = std::fs::read(&path)
        .expect("gb_char.dbin")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        native,
        "380000006f1285e404000000000000000400c8000000800c000001000000030000001c000301b80b0000\
         3275000000d80000594e0000841084108410e8030000"
    );
    close(context, instance);
}

/// A stored phrase word above Unicode reaches the sentence, the training
/// and the export through `phrase_text`.
#[test]
fn a_system_phrase_above_unicode_is_chosen_and_trained() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = phrase_text_system(scratch.path(), 0x11_0000);
    let user = TempUserDir::new("phrase-text-above-unicode");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "da1da1da1ba1"), 12);
    assert!(pinyin_guess_candidates(
        instance,
        0,
        crate::test_support::DEFAULT_SORT
    ));
    assert_eq!(
        pinyin_choose_candidate(instance, 0, row_of(instance, NORMAL)),
        9
    );
    assert!(pinyin_guess_sentence(instance));
    let mut sentence: *mut c_char = ptr::null_mut();
    assert!(pinyin_get_sentence(instance, 0, &raw mut sentence));
    assert_eq!(take_hex(sentence), "e794b2f4908080e4b999e4b899");
    assert!(crate::candidates::pinyin_train(instance, 0));
    assert_eq!(unigram(instance, 0x0100_0001), (true, 583));
    assert_eq!(unigram(instance, 0x0100_0002), (true, 583));
    let iter = crate::iterators::pinyin_begin_get_phrases(context, 1);
    let mut rows = Vec::new();
    while crate::iterators::pinyin_iterator_has_next_phrase(iter) {
        let (mut phrase, mut reading, mut count) = (ptr::null_mut(), ptr::null_mut(), 0);
        assert!(crate::iterators::pinyin_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut reading,
            &raw mut count
        ));
        rows.push((take_hex(phrase), take_hex(reading), count));
    }
    crate::iterators::pinyin_end_get_phrases(iter);
    let row = |text: &str, reading: &str| (text.to_owned(), reading.to_owned(), 79);
    assert_eq!(
        rows,
        [
            row("e794b2f4908080e4b999", "6461312764613127646131"),
            row("e4b899", "626131"),
        ]
    );
    close(context, instance);
}

/// A negative remember count is stored as its bit pattern: `i32::MIN`
/// exports as itself.
#[test]
fn a_negative_remember_count_is_stored_as_its_bit_pattern() {
    let user = TempUserDir::new("remember-count-min");
    let (context, instance) = open_at(&user.path);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(crate::user_data::pinyin_remember_user_input(
        instance,
        c"你好".as_ptr(),
        i32::MIN
    ));
    assert_eq!(
        crate::e2e_tests::export_of(context, 7),
        [("你好".to_owned(), "ni'hao".to_owned(), i32::MIN)]
    );
    close(context, instance);
}

/// Imports `phrase` under `reading` into libraries 1 to 5, so the phrase
/// table search holds five tokens.
fn hold_in_five_libraries(context: *mut PinyinContext, phrase: &str, reading: &str) {
    for library in 1..=5 {
        assert_eq!(
            import_batch(context, library, &[(phrase, reading, 100_000)]),
            [true],
            "library {library}"
        );
    }
}

/// An import into an unused library index aborts on the five-token search
/// before the sub-index refusal.
#[test]
fn an_import_aborts_on_five_tokens_before_the_library_refusal() {
    assert_aborts(
        "boundary_tests::an_import_aborts_on_five_tokens_before_the_library_refusal",
        || {
            let user = TempUserDir::new("combined-unused-index");
            let (context, _) = open_at(&user.path);
            hold_in_five_libraries(context, "你好", "ni3'hao3");
            import_batch(context, 8, &[("你好", "ni3'hao3", 100_000)]);
        },
    );
}

/// A reading of the wrong length is refused before the five-token search.
#[test]
fn an_import_refuses_a_short_reading_before_the_token_count() {
    let user = TempUserDir::new("combined-invalid-reading");
    let (context, instance) = open_at(&user.path);
    hold_in_five_libraries(context, "你好", "ni3'hao3");
    assert_eq!(
        import_batch(context, 6, &[("你好", "ni3", 100_000)]),
        [false]
    );
    close(context, instance);
}

/// The prefix guess aborts on a prefix of five tokens (`compute_prefixes`).
#[test]
fn the_prefix_guess_aborts_on_five_tokens() {
    assert_aborts(
        "boundary_tests::the_prefix_guess_aborts_on_five_tokens",
        || {
            let user = TempUserDir::new("combined-prefix-guess");
            let (context, instance) = open_at(&user.path);
            hold_in_five_libraries(context, "你好", "ni3'hao3");
            crate::sentence::pinyin_guess_sentence_with_prefix(instance, c"你好".as_ptr());
        },
    );
}

/// The prediction aborts on a prefix of five tokens.
#[test]
fn the_prediction_aborts_on_five_tokens() {
    assert_aborts(
        "boundary_tests::the_prediction_aborts_on_five_tokens",
        || {
            let user = TempUserDir::new("combined-prediction");
            let (context, instance) = open_at(&user.path);
            hold_in_five_libraries(context, "你好", "ni3'hao3");
            pinyin_guess_predicted_candidates(instance, c"你好".as_ptr());
        },
    );
}

/// The character offset aborts on a phrase of five tokens.
#[test]
fn the_character_offset_aborts_on_five_tokens() {
    assert_aborts(
        "boundary_tests::the_character_offset_aborts_on_five_tokens",
        || {
            let user = TempUserDir::new("combined-character-offset");
            let (context, instance) = open_at(&user.path);
            hold_in_five_libraries(context, "你", "ni3");
            assert_eq!(parse(instance, "ni"), 2);
            let mut length = 0_usize;
            pinyin_get_character_offset(instance, c"你".as_ptr(), 0, &raw mut length);
        },
    );
}

/// `remember_user_input` aborts on a phrase of five tokens.
#[test]
fn remember_aborts_on_five_tokens() {
    assert_aborts("boundary_tests::remember_aborts_on_five_tokens", || {
        let user = TempUserDir::new("combined-remember");
        let (context, instance) = open_at(&user.path);
        hold_in_five_libraries(context, "你", "ni3");
        assert_eq!(parse(instance, "ni"), 2);
        crate::user_data::pinyin_remember_user_input(instance, c"你".as_ptr(), 1);
    });
}

/// A private mini system whose punct.bin maps each phrase's tokens to its
/// value; `lookup` reads the tokens from the fixture first.
fn punct_system(parent: &Path, rows: &[(&str, Vec<u8>)]) -> std::path::PathBuf {
    let user = TempUserDir::new("punct-tokens");
    let (context, instance) = open_at(&user.path);
    let mut table = Vec::new();
    for (text, value) in rows {
        for token in lookup(instance, text) {
            table.push((token.to_le_bytes().to_vec(), value.clone()));
        }
    }
    close(context, instance);
    let system = private_system(parent);
    craft_tree(&system.join("punct.bin"), &table);
    system
}

fn punct_words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// The punctuation rows of the current list, as hex of their bytes.
fn punct_texts(instance: *mut PinyinInstance) -> Vec<String> {
    capture_rows(instance)
        .iter()
        .enumerate()
        .filter(|(_, (kind, _, _))| *kind == PREDICTED_PUNCTUATION)
        .map(|(index, _)| {
            let mut text: *const c_char = ptr::null();
            assert!(crate::candidates::pinyin_get_candidate_string(
                instance,
                candidate_at(instance, index),
                &raw mut text
            ));
            // SAFETY: a true answer points at the row's borrowed string.
            let bytes = unsafe { std::ffi::CStr::from_ptr(text) }.to_bytes();
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        })
        .collect()
}

/// Punctuations of non-scalar words dedup by their bytes, across the
/// prefixes.
#[test]
fn non_scalar_punctuations_dedup_by_their_bytes() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = punct_system(
        scratch.path(),
        &[
            ("好", punct_words(&[0xD800, 0, 0x11_0000, 0])),
            ("你好", punct_words(&[0x11_0000, 0, 0xD800, 0])),
        ],
    );
    let user = TempUserDir::new("punct-non-scalar");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        c"你好".as_ptr()
    ));
    let mut puncts = punct_texts(instance);
    puncts.sort();
    assert_eq!(puncts, ["eda080", "f4908080"]);
    close(context, instance);
}

/// The punctuation dedup ignores the existing candidates: a punctuation
/// equal to the first prediction is listed, and the prediction stays.
#[test]
fn the_punctuation_dedup_ignores_the_candidates() {
    let user = TempUserDir::new("punct-candidate-probe");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_guess_predicted_candidates(instance, c"你".as_ptr()));
    let first = capture_rows(instance)[0].1.clone();
    close(context, instance);
    let mut words: Vec<u32> = first.chars().map(u32::from).collect();
    words.extend([0, 0xFF0C, 0]);
    let scratch = tempfile::tempdir().expect("scratch");
    let system = punct_system(scratch.path(), &[("你", punct_words(&words))]);
    let user = TempUserDir::new("punct-candidate");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        c"你".as_ptr()
    ));
    let first_hex: String = first.bytes().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(punct_texts(instance), [first_hex, "efbc8c".to_owned()]);
    assert!(
        capture_rows(instance)
            .iter()
            .any(|(kind, text, _)| *kind == PREDICTED_PREFIX && *text == first)
    );
    close(context, instance);
}

/// Training over a user gram whose total is at the `guint32` limit keeps
/// the gram's counts and still raises the unigram by the trained delta.
#[test]
fn training_past_the_gram_total_limit_keeps_the_gram() {
    const COUNT: u32 = 0xFFFF_FFFF - 22_080 + 1;
    let user = TempUserDir::new("d18-train-total-overflow");
    let (context, instance) = open_at(&user.path);
    let [ni, hao, zhong] = ["你", "好", "中"].map(|text| lookup(instance, text)[0]);
    close(context, instance);
    let gram = gram_bytes(COUNT, &[(hao, COUNT)]);
    plant_bigram(
        &user.path,
        vec![
            (ni.to_le_bytes().to_vec(), gram.clone()),
            (zhong.to_le_bytes().to_vec(), gram),
            (1_u32.to_le_bytes().to_vec(), vec![0; 4]),
        ],
    );
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_set_options(context, 1 << 5));
    let before = unigram(instance, hao);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(pinyin_guess_sentence(instance));
    for (offset, text) in [(0, "你"), (2, "好")] {
        assert!(pinyin_guess_candidates(
            instance,
            offset,
            crate::test_support::DEFAULT_SORT
        ));
        assert!(pinyin_choose_candidate(instance, offset, row_with(instance, NORMAL, text)) > 0);
    }
    assert!(pinyin_guess_sentence(instance));
    assert!(crate::candidates::pinyin_train(instance, 0));
    let after = unigram(instance, hao);
    assert_eq!(after, (true, before.1 + 154_560));
    assert!(pinyin_save(context));
    close(context, instance);
    let stored = crate::test_support::store_rows(&user.path.join("user_bigram.db"), true)
        .into_iter()
        .find(|(key, _)| *key == ni.to_le_bytes())
        .expect("the 你 gram")
        .1;
    assert_eq!(stored, gram_bytes(COUNT, &[(hao, COUNT)]));
    let (context, instance) = open_at(&user.path);
    assert_eq!(unigram(instance, hao), after);
    close(context, instance);
}

// ── Library masks, separators and save timing ─────────────────────────────

/// The GBK-library tokens of the current candidate list.
fn gbk_tokens(instance: *mut PinyinInstance) -> Vec<u32> {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { crate::state::instance_ref(instance) };
    inst.state
        .candidates
        .iter()
        .filter_map(|cand| cand.token.get().map(oxpinyin_core::PhraseToken::value))
        .filter(|token| token >> 24 == 2)
        .collect()
}

/// An unload of the GBK library hides its rows from the next candidate
/// guess on the same parse, with no reparse.
#[test]
fn an_unload_hides_its_rows_without_a_reparse() {
    let user = TempUserDir::new("row54-unload");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_set_options(context, 1 << 5));
    assert_eq!(parse(instance, "ni"), 2);
    assert!(pinyin_guess_candidates(
        instance,
        0,
        crate::test_support::DEFAULT_SORT
    ));
    assert!(!gbk_tokens(instance).is_empty());
    assert!(crate::config::pinyin_unload_phrase_library(context, 2));
    assert!(pinyin_guess_candidates(
        instance,
        0,
        crate::test_support::DEFAULT_SORT
    ));
    assert!(gbk_tokens(instance).is_empty());
    assert!(n_candidate(instance) > 0);
    close(context, instance);
}

fn ucs4(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| u32::from(ch).to_le_bytes())
        .collect()
}

/// User phrase-table rows whose token names a library that is never
/// loaded (15) are filtered from the lookup and the prediction.
#[test]
fn rows_of_a_never_loaded_library_are_filtered() {
    let user = TempUserDir::new("never-loaded-library");
    let value = ((15_u32 << 24) | 1).to_le_bytes().to_vec();
    craft_user_index(
        &user.path.join("user_phrase_index.bin"),
        &[
            (ucs4("你"), Vec::new()),
            ([ucs4("你"), ucs4("龘")].concat(), value.clone()),
            (ucs4("龘龘"), value),
        ],
    );
    write_user_conf(&user.path, &system_dir());
    let (context, instance) = open_at(&user.path);
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    assert!(!crate::dict::pinyin_lookup_tokens(
        instance,
        c"龘龘".as_ptr(),
        array.cast()
    ));
    // SAFETY: a live GArray, freed once.
    unsafe {
        assert_eq!((*array).len, 0);
        glib_sys::g_array_free(array, 1);
    }
    assert!(pinyin_guess_predicted_candidates(instance, c"你".as_ptr()));
    assert!(
        !capture_rows(instance)
            .iter()
            .any(|(_, text, _)| text.contains('龘'))
    );
    close(context, instance);
}

/// The prediction walks the user phrase table by raw key bytes and takes
/// the row's text from the live item: a key 你 + U+110000 lists 龘龘's
/// suffix 龘.
#[test]
fn a_raw_user_key_predicts_its_live_item() {
    let user = TempUserDir::new("raw-key-prediction");
    let token = saved_user_phrase(&user.path);
    craft_user_index(
        &user.path.join("user_phrase_index.bin"),
        &[
            (ucs4("你"), Vec::new()),
            (
                [ucs4("你"), 0x0011_0000_u32.to_le_bytes().to_vec()].concat(),
                token.to_le_bytes().to_vec(),
            ),
        ],
    );
    write_user_conf(&user.path, &system_dir());
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_guess_predicted_candidates(instance, c"你".as_ptr()));
    assert!(
        capture_rows(instance)
            .iter()
            .any(|(kind, text, _)| *kind == PREDICTED_PREFIX && text == "龘")
    );
    close(context, instance);
}

/// An import reading parses past long separator runs, between its keys and
/// after them, to its two keys.
#[test]
fn an_import_reading_parses_past_long_separator_runs() {
    let user = TempUserDir::new("import-separator-runs");
    let (context, instance) = open_at(&user.path);
    let iter = crate::iterators::pinyin_begin_add_phrases(context, 7);
    for reading in [
        [&b"ni3"[..], &[b'\''; 40_000], b"hao3"].concat(),
        [&b"ni3'hao3"[..], &[b'\''; 70_000]].concat(),
    ] {
        let reading = std::ffi::CString::new(reading).expect("no NUL");
        assert!(crate::iterators::pinyin_iterator_add_phrase(
            iter,
            c"你好".as_ptr(),
            reading.as_ptr(),
            1
        ));
    }
    crate::iterators::pinyin_end_add_phrases(iter);
    assert_eq!(
        crate::e2e_tests::export_of(context, 7),
        [("你好".to_owned(), "ni3'hao3".to_owned(), 2)]
    );
    close(context, instance);
}

/// A user-gram mask reaches the file only at the next armed save: an
/// unarmed save keeps the masked gram on disk, and the save after an
/// import removes it (#677).
#[test]
fn a_masked_user_gram_leaves_the_file_at_the_next_armed_save() {
    let user = TempUserDir::new("issue677");
    let key = 0x0700_0001_u32.to_le_bytes().to_vec();
    let gram = gram_bytes(100, &[(0x0100_0001, 100)]);
    let context = crafted_bigram_context(&user.path, vec![(key.clone(), gram.clone())]);
    let stored = || {
        crate::test_support::store_rows(&user.path.join("user_bigram.db"), true)
            .into_iter()
            .find(|(found, _)| *found == key)
            .map(|(_, value)| value)
    };
    assert_eq!(
        import_batch(context, 7, &[("达达", "da2'da2", 100)]),
        [true]
    );
    assert!(pinyin_save(context));
    assert!(pinyin_mask_out(context, 0x0F00_0000, 0x0700_0000));
    assert!(!pinyin_save(context));
    assert_eq!(stored(), Some(gram));
    assert_eq!(
        import_batch(context, 7, &[("你你", "ni3'ni3", 100)]),
        [true]
    );
    assert!(pinyin_save(context));
    assert_eq!(stored(), None);
    pinyin_fini(context);
}

// ── Native DBM copies ──────────────────────────────────────────────────────

/// Copies the regular files of `user` to `before`, next to it.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
fn keep_before(user: &Path) {
    let before = user.parent().expect("the child's scratch").join("before");
    std::fs::create_dir(&before).expect("the before directory");
    for (name, bytes) in files_of(user) {
        std::fs::write(before.join(name), bytes).expect("keep a file");
    }
}

/// Two whole user-bigram rows: two predecessors, each with an empty gram.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
fn two_bigram_rows() -> Vec<(Vec<u8>, Vec<u8>)> {
    vec![
        (0x0100_0001_u32.to_le_bytes().to_vec(), vec![0; 4]),
        (0x0100_0002_u32.to_le_bytes().to_vec(), vec![0; 4]),
    ]
}

/// The pin's `copy_tkrzwdb` and `copy_bdb` assert on a source walk that
/// fails (`tkrzwdb_utils.h:72`, `bdb_utils.h:67`): init stops in the copy
/// after one record, with the raised open counter already written
/// (`pinyin.cpp:172-218` runs before the loads) and the profile unchanged.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn init_aborts_where_a_copy_fails_a_source_read() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::init_aborts_where_a_copy_fails_a_source_read",
        |user| {
            plant_bigram(user, two_bigram_rows());
            keep_before(user);
            oxpinyin_store::copy_faults::fail_source_read_after(1);
            init_at(&system_dir(), user);
        },
    );
    let before = files_of(&run.user_dir.parent().expect("scratch").join("before"));
    let after = files_of(&run.user_dir);
    let conf = String::from_utf8_lossy(&after["user.conf"]).into_owned();
    assert!(conf.ends_with("open counter:2\n"), "{conf}");
    assert_eq!(after["user_bigram.db"], before["user_bigram.db"]);
}

/// The same stop in `PhraseLargeTable3::load_db`'s copy
/// (`phrase_large_table3_tkrzwdb.cpp:95`, `phrase_large_table3_bdb.cpp:141`):
/// a profile whose only rows are two user phrase-index rows stops after the
/// first, with the profile unchanged.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn init_aborts_where_a_user_index_copy_fails_a_source_read() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::init_aborts_where_a_user_index_copy_fails_a_source_read",
        |user| {
            set_phrase_table_row(user, "你好", &[0x0100_0001]);
            set_phrase_table_row(user, "再见", &[0x0100_0002]);
            keep_before(user);
            oxpinyin_store::copy_faults::fail_source_read_after(1);
            init_at(&system_dir(), user);
        },
    );
    let before = files_of(&run.user_dir.parent().expect("scratch").join("before"));
    let after = files_of(&run.user_dir);
    assert_eq!(
        after["user_phrase_index.bin"],
        before["user_phrase_index.bin"]
    );
}

/// The control: a user bigram that does not open is the pin's soft
/// `return false` (`ngram_tkrzwdb.cpp:55-56`, `ngram_bdb.cpp:67-70`), so no
/// copy runs and init answers a context, though every source read fails.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn init_survives_a_user_bigram_that_does_not_open() {
    crate::test_support::run_in_child(
        "boundary_tests::init_survives_a_user_bigram_that_does_not_open",
        |user| {
            plant_bigram(user, two_bigram_rows());
            let bigram = user.join("user_bigram.db");
            std::fs::remove_file(&bigram).expect("unlink the user bigram");
            std::fs::create_dir(&bigram).expect("a directory in its place");
            oxpinyin_store::copy_faults::fail_source_read_after(0);
            assert!(!init_at(&system_dir(), user).is_null());
        },
    );
}

/// `copy_tkrzwdb` and `copy_bdb` return `false` without an assert when no
/// source iterator is made (`tkrzwdb_utils.h:52-54`, `bdb_utils.h:49-52`),
/// and `load_db` ignores the answer (`ngram_tkrzwdb.cpp:58`,
/// `chewing_large_table2_tkrzwdb.cpp:104`): init answers a context, though
/// every source read of a copy would fail.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn init_survives_a_copy_whose_source_iterator_is_not_made() {
    crate::test_support::run_in_child(
        "boundary_tests::init_survives_a_copy_whose_source_iterator_is_not_made",
        |user| {
            plant_bigram(user, two_bigram_rows());
            oxpinyin_store::copy_faults::fail_iterator_creation_after(0);
            oxpinyin_store::copy_faults::fail_source_read_after(0);
            assert!(!init_at(&system_dir(), user).is_null());
        },
    );
}

/// The pin's copies assert on a failed destination write
/// (`tkrzwdb_utils.h:65`, `bdb_utils.h:61`): the save stops in
/// `_write_files` after one record, before `_rename_files` and
/// `mark_version` (`pinyin.cpp:1141-1143`), so no saved file is replaced.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn save_aborts_where_a_copy_fails_a_destination_write() {
    let run = crate::test_support::assert_aborts_in_dir(
        "boundary_tests::save_aborts_where_a_copy_fails_a_destination_write",
        |user| {
            plant_bigram(user, two_bigram_rows());
            let context = init_at(&system_dir(), user);
            assert!(!context.is_null());
            // `pinyin_save` saves a modified context only (`pinyin.cpp:1136`).
            train_once(context);
            assert!(pinyin_save(context));
            keep_before(user);
            train_once(context);
            // The first copy writes one record and fails the next.
            oxpinyin_store::copy_faults::fail_destination_write_after(1);
            pinyin_save(context);
        },
    );
    let before = files_of(&run.user_dir.parent().expect("scratch").join("before"));
    let after = files_of(&run.user_dir);
    for (name, bytes) in &before {
        assert_eq!(after.get(name), Some(bytes), "{name} was replaced");
    }
    assert!(
        after.keys().any(|name| name.ends_with(".tmp")),
        "the copy left no staged file: {:?}",
        after.keys()
    );
}
