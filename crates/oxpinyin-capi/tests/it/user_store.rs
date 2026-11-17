//! The user store: mask-out, user index and bigram rows, journal logs and
//! user files.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

#[cfg(unix)]
use crate::support::directory_reads_as_a_chunk_here;
#[cfg(unix)]
use crate::support::remap_a_directory_library;

use crate::support::init_fixture_at;

use crate::support::{
    ChewingKey, PinyinContext, TempUserDir, assert_aborts, craft_user_index, cstr, import_batch,
    open, pinyin_begin_get_bigram_phrases, pinyin_fini, pinyin_free_instance,
    pinyin_get_pinyin_key, pinyin_guess_candidates, pinyin_mask_out,
    pinyin_parse_more_full_pinyins, pinyin_remember_user_input, pinyin_save, plant_bigram,
    private_system, system_dir, write_user_conf, *,
};

use std::path::Path;

use std::ptr;

/// The first key of `text` as parsed by a scratch instance: its two bytes.
fn first_key_bytes(context: *mut PinyinContext, text: &str) -> Vec<u8> {
    let _slots = crate::support::key_slot_guard();
    let instance = alloc(context);
    assert!(parse(instance, text) > 0);
    let mut key: *mut ChewingKey = ptr::null_mut();
    assert!(unsafe { pinyin_get_pinyin_key(instance, 0, &raw mut key) });
    // SAFETY: a true answer points at a live two-byte ChewingKey, read
    // before the instance is freed.
    let bytes = unsafe { std::slice::from_raw_parts(key.cast::<u8>(), 2) }.to_vec();
    unsafe { pinyin_free_instance(instance) };
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
    unsafe { pinyin_fini(context) };
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

/// The first key of `ni` and the 17-key run of it.
fn ni_and_its_overlong_run(first: &[u8]) -> Vec<Vec<u8>> {
    vec![first.to_vec(), first.repeat(17)]
}

/// A context on a profile whose user bigram holds exactly `rows`.
fn crafted_bigram_context(user: &Path, rows: Vec<(Vec<u8>, Vec<u8>)>) -> *mut PinyinContext {
    plant_bigram(user, rows);
    let context = init_at(&system_dir(), user);
    assert!(!context.is_null());
    context
}

/// Two null-token MODIFY_HEADER records.
fn two_headers() -> Vec<u8> {
    [
        crate::support::modify_header(0, 1, 1),
        crate::support::modify_header(0, 1, 2),
    ]
    .concat()
}

/// The candidate guess asserts on an over-long user index key it reaches
/// through the parsed prefix.
#[test]
fn guess_candidates_aborts_on_an_overlong_index_key() {
    assert_aborts(
        "user_store::guess_candidates_aborts_on_an_overlong_index_key",
        || {
            let user = TempUserDir::new("overlong-guess");
            let context =
                overlong_index_context(&system_dir(), &user.path, ni_and_its_overlong_run);
            let instance = alloc(context);
            assert_eq!(parse(instance, "ni"), 2);
            unsafe { pinyin_guess_candidates(instance, 0, 0) };
        },
    );
}

/// `mask_out` asserts on a user bigram key that is not a token (the
/// facade's `gram_mask_fault_over`).
#[test]
fn mask_out_aborts_on_a_non_token_bigram_key() {
    assert_aborts(
        "user_store::mask_out_aborts_on_a_non_token_bigram_key",
        || {
            let user = TempUserDir::new("mask-non-token-key");
            let context = crafted_bigram_context(&user.path, vec![(vec![0, 1], vec![7, 0, 0, 0])]);
            unsafe { pinyin_mask_out(context, 0xFFFF_FFFF, 0) };
        },
    );
}

/// `mask_out` asserts on a gram whose total outlives its items.
#[test]
fn mask_out_aborts_on_a_residual_bigram_gram() {
    assert_aborts(
        "user_store::mask_out_aborts_on_a_residual_bigram_gram",
        || {
            let user = TempUserDir::new("mask-residual-gram");
            let value = [&[7_u8, 0, 0, 0][..], &[0; 8]].concat();
            let context = crafted_bigram_context(&user.path, vec![(vec![1, 0, 0, 0], value)]);
            unsafe { pinyin_mask_out(context, 0xFFFF_FFFF, 0) };
        },
    );
}

/// `begin_get_bigram_phrases` asserts on a user bigram key that is not a
/// token.
#[test]
fn bigram_export_aborts_on_a_non_token_key() {
    assert_aborts(
        "user_store::bigram_export_aborts_on_a_non_token_key",
        || {
            let user = TempUserDir::new("export-non-token-key");
            let context = crafted_bigram_context(&user.path, vec![(vec![0, 1], vec![7, 0, 0, 0])]);
            unsafe { pinyin_begin_get_bigram_phrases(context) };
        },
    );
}

/// `mask_out` aborts on a log of two headers after a save, before it
/// writes any file: stderr is empty and the profile is byte-identical.
#[test]
fn mask_out_aborts_on_two_log_headers_and_leaves_the_files() {
    let run = crate::support::assert_aborts_in_dir(
        "user_store::mask_out_aborts_on_two_log_headers_and_leaves_the_files",
        |user| {
            write_user_conf(user, &system_dir());
            let context = init_at(&system_dir(), user);
            assert!(!context.is_null());
            assert_eq!(import_batch(context, 7, &[("你好", "ni3'hao3", 1)]), [true]);
            assert!(unsafe { pinyin_save(context) });
            write_log(user, "gb_char.dbin", &two_headers());
            let before = user.parent().expect("scratch").join("before");
            std::fs::create_dir(&before).expect("the snapshot directory");
            for (name, bytes) in files_of(user) {
                std::fs::write(before.join(name), bytes).expect("snapshot a file");
            }
            unsafe { pinyin_mask_out(context, 0, 0) };
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

/// Register row 97: `mask_out` reloads library 1 and `_mask_out_records`
/// appends a kept ADD with both chunk pointers (`phrase_index_logger.h:245`).
#[test]
fn mask_out_aborts_on_a_surviving_add() {
    crate::support::assert_aborts("user_store::mask_out_aborts_on_a_surviving_add", || {
        let user_dir = TempUserDir::new("mask-surviving-add");
        let context = init_fixture_at(&user_dir.path);
        assert!(!context.is_null());
        let payload = [crate::support::logs::header(), crate::support::logs::add()].concat();
        crate::support::plant_log(&user_dir.path, "gb_char.dbin", &payload);
        let _ = unsafe { pinyin_mask_out(context, 0xffff_ffff, 0x0100_0001) };
    });
}

/// Register row 85: `pinyin_mask_out` loads token 1's gram, which the mask
/// leaves alone, and `SingleGram::mask_out` reads its total from three bytes
/// (`ngram.cpp:80`, `memory_chunk.h:390`).
#[test]
fn mask_out_aborts_on_a_short_bigram_value() {
    crate::support::assert_aborts(
        "user_store::mask_out_aborts_on_a_short_bigram_value",
        || {
            let user_dir = TempUserDir::new("mask-short-gram");
            crate::support::plant_bigram(
                &user_dir.path,
                vec![(1_u32.to_le_bytes().to_vec(), vec![7, 0, 0])],
            );
            let context = init_fixture_at(&user_dir.path);
            assert!(!context.is_null());
            let _ = unsafe { pinyin_mask_out(context, 0xffff_ffff, 0) };
        },
    );
}

/// Row 98 (class (b)) keeps its refusal: a partial item tail under a key the
/// mask leaves alone answers `false` (`ngram.cpp:76-86`).
#[test]
fn mask_out_refuses_a_partial_bigram_tail() {
    let user_dir = TempUserDir::new("mask-partial-tail");
    let mut value = 7_u32.to_le_bytes().to_vec();
    value.extend_from_slice(&[9, 0, 0, 0, 7, 0, 0, 0, 1]);
    crate::support::plant_bigram(&user_dir.path, vec![(1_u32.to_le_bytes().to_vec(), value)]);
    let context = init_fixture_at(&user_dir.path);
    assert!(!context.is_null());
    assert!(!unsafe { pinyin_mask_out(context, 0xffff_ffff, 0) });
    unsafe { crate::support::pinyin_fini(context) };
}

/// Register row 93 through `pinyin_remember_user_input`: the pin's
/// `_remember_phrase_recur` calls `_add_phrase` for USER_DICTIONARY
/// (`pinyin.cpp:3702`), so a phrase table whose row for the phrase holds two
/// library-7 tokens trips `assert(PHRASE_INDEX_LIBRARY_INDEX(token) != index)`
/// (`:554`) there as it does on the import path.
#[test]
fn remember_user_input_aborts_on_two_same_library_tokens() {
    crate::support::assert_aborts(
        "user_store::remember_user_input_aborts_on_two_same_library_tokens",
        || {
            let user_dir = TempUserDir::new("remember-two-tokens");
            crate::support::set_phrase_table_row(
                &user_dir.path,
                "你好",
                &[0x0700_0001, 0x0700_0002],
            );
            let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
            assert_eq!(
                unsafe { pinyin_parse_more_full_pinyins(instance, cstr("nihao").as_ptr()) },
                5
            );
            let _ = unsafe { pinyin_remember_user_input(instance, cstr("你好").as_ptr(), -1) };
        },
    );
}

/// `_remember_phrase_recur` computes `end = matrix.size() - 1`, which wraps
/// for an empty matrix, and then asserts in `get_column_size(0)`
/// (`phonetic_key_matrix.h:102`; register row 66). A fresh instance holds
/// one.
#[test]
fn remember_user_input_aborts_on_a_fresh_instance() {
    crate::support::assert_aborts(
        "user_store::remember_user_input_aborts_on_a_fresh_instance",
        || {
            let user_dir = TempUserDir::new("remember-fresh");
            let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
            let _ = unsafe { pinyin_remember_user_input(instance, cstr("你好").as_ptr(), -1) };
        },
    );
}

/// As above, for a parse that consumed bytes but placed no key (three
/// apostrophes): `fill_matrix` leaves the matrix empty.
#[test]
fn remember_user_input_aborts_on_a_keyless_parse() {
    crate::support::assert_aborts(
        "user_store::remember_user_input_aborts_on_a_keyless_parse",
        || {
            let user_dir = TempUserDir::new("remember-keyless");
            let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
            assert_eq!(
                unsafe { pinyin_parse_more_full_pinyins(instance, cstr("'''").as_ptr()) },
                3
            );
            let _ = unsafe { pinyin_remember_user_input(instance, cstr("你好").as_ptr(), -1) };
        },
    );
}

/// The empty-matrix abort follows the token computation: a character with
/// no token refuses first (`pinyin.cpp:3685-3695`).
#[test]
fn remember_user_input_refuses_an_unknown_character_before_the_empty_matrix() {
    let user_dir = TempUserDir::new("remember-unknown-empty");
    let (_context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(!unsafe { pinyin_remember_user_input(instance, cstr("😀").as_ptr(), -1) });
}

/// Register row 96 at mask-out, after the user tables are masked.
// Unix only, for the reason `init_aborts_on_a_directory_library` gives.
#[cfg(unix)]
#[test]
fn mask_out_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::support::assert_aborts("user_store::mask_out_aborts_on_a_directory_library", || {
            let _ = remap_a_directory_library("mask");
        });
    } else {
        assert!(remap_a_directory_library("mask"));
    }
}

/// `mask_out` detects the over-long index key before it reloads any
/// library: the abort comes first even when a later library is gone.
#[test]
fn mask_out_aborts_on_an_overlong_index_key_before_the_reload() {
    assert_aborts(
        "user_store::mask_out_aborts_on_an_overlong_index_key_before_the_reload",
        || {
            let scratch = tempfile::tempdir().expect("scratch");
            let system = private_system(scratch.path());
            let user = TempUserDir::new("overlong-mask-unmappable");
            let context = overlong_index_context(&system, &user.path, |_| {
                vec![vec![0, 1], [0, 1].repeat(17)]
            });
            std::fs::remove_file(system.join("merged.bin")).expect("unlink merged.bin");
            unsafe { pinyin_mask_out(context, 0xFFFF_FFFF, 0) };
        },
    );
}
