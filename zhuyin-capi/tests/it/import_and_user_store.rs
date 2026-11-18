//! Import, mask-out and the user index, bigram and file rows.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

#[cfg(unix)]
use crate::support::directory_reads_as_a_chunk_here;
#[cfg(unix)]
use crate::support::map_a_directory_library;

use crate::support::init_at;

use crate::support::{
    TempUserDir, assert_aborts, close, craft_user_index, cstr, import, init_on, open_with_user,
    plant_bigram, system_dir, tokens_of, ucs4_key, write_user_conf, zhuyin_fini,
    zhuyin_guess_candidates_after_cursor, zhuyin_guess_candidates_before_cursor, zhuyin_mask_out,
    zhuyin_parse_more_chewings, zhuyin_save, *,
};

use std::path::Path;

/// Opens and closes `user_dir` once, so that the profile exists.
fn first_init(user_dir: &Path) {
    let context = init_on(&system_dir(), Some(user_dir));
    assert!(!context.is_null());
    unsafe { zhuyin_fini(context) };
}

/// A user pinyin index whose crafted keys are the first key and its 17-fold
/// repeat: the user index rows of a profile reopened under a conforming
/// `user.conf`.
fn overlong_index_profile(user_dir: &Path) {
    first_init(user_dir);
    let first = vec![0_u8, 1];
    craft_user_index(
        &user_dir.join("user_pinyin_index.bin"),
        &[(first.clone(), Vec::new()), (first.repeat(17), Vec::new())],
    );
    write_user_conf(user_dir, &system_dir());
}

/// Replaces the user phrase index with `你好 -> value`, reopens and imports
/// 你好 into library 7: the pin asserts at `zhuyin.cpp:440` or `:454`.
fn add_after_index(user_dir: &Path, value: &[u8]) {
    craft_user_index(
        &user_dir.join("user_phrase_index.bin"),
        &[(ucs4_key("你好"), value.to_vec())],
    );
    write_user_conf(user_dir, &system_dir());
    let (context, _instance) = open_with_user(user_dir);
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    unsafe { import(context, 7, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 1) };
}

/// libzhuyin's import parses chewing (`zhuyin.cpp:515-523`): ㄉㄣ, a reading
/// no typed input spells, stores key 0x1484 and survives a reopen.
#[test]
fn an_imported_den_reading_stores_its_key() {
    let user_dir = TempUserDir::new("den");
    let (context, instance) = open_with_user(&user_dir.path);
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(unsafe { import(context, 7, "扽", "ㄉㄣ".as_bytes(), 5) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let tokens = unsafe { user_tokens(instance, "扽") };
    assert_eq!(tokens, [0x0700_0001]);
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { nth_pronunciation(instance, tokens[0], 0) },
        (true, 1, "8414".to_owned())
    );
    assert!(unsafe { zhuyin_save(context) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    let (context, instance) = open_with_user(&user_dir.path);
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { nth_pronunciation(instance, tokens[0], 0) },
        (true, 1, "8414".to_owned())
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// mask_out walks the user index and asserts on a key longer than 16 words
/// (register row 84).
#[test]
fn mask_out_over_an_overlong_user_index_key_aborts() {
    assert_aborts(
        "import_and_user_store::mask_out_over_an_overlong_user_index_key_aborts",
        || {
            let user_dir = TempUserDir::new("overlong-mask");
            overlong_index_profile(&user_dir.path);
            let (context, _instance) = open_with_user(&user_dir.path);
            unsafe { zhuyin_mask_out(context, 0xFFFF_FFFF, 0) };
        },
    );
}

/// The cursor guesses never read the overlong user index key: both answer
/// true, with no abort.
#[test]
fn cursor_guesses_ignore_an_overlong_user_index_key() {
    let user_dir = TempUserDir::new("overlong-guess");
    overlong_index_profile(&user_dir.path);
    let (context, instance) = open_with_user(&user_dir.path);
    assert_eq!(
        unsafe { zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()) },
        6
    );
    assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, 0) });
    assert!(unsafe { zhuyin_guess_candidates_before_cursor(instance, 6) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// Two library-7 tokens for one phrase key (register row 93).
#[test]
fn adding_a_phrase_with_two_library_tokens_aborts() {
    assert_aborts(
        "import_and_user_store::adding_a_phrase_with_two_library_tokens_aborts",
        || {
            let user_dir = TempUserDir::new("duplicate-token");
            first_init(&user_dir.path);
            let value = [0x0700_0001_u32.to_le_bytes(), 0x0700_0002_u32.to_le_bytes()].concat();
            add_after_index(&user_dir.path, &value);
        },
    );
}

/// A user bigram key that is not a four-byte token aborts the mask walk.
#[test]
fn mask_out_over_a_non_token_bigram_key_aborts() {
    assert_aborts(
        "import_and_user_store::mask_out_over_a_non_token_bigram_key_aborts",
        || {
            let user_dir = TempUserDir::new("bigram-key");
            plant_bigram(&user_dir.path, vec![(vec![0, 1], vec![7, 0, 0, 0])]);
            let (context, _instance) = open_with_user(&user_dir.path);
            unsafe { zhuyin_mask_out(context, 0xFFFF_FFFF, 0) };
        },
    );
}

/// A user gram whose item survives the mask with a zero token aborts the walk.
#[test]
fn mask_out_over_a_residual_bigram_gram_aborts() {
    assert_aborts(
        "import_and_user_store::mask_out_over_a_residual_bigram_gram_aborts",
        || {
            let user_dir = TempUserDir::new("bigram-residual");
            let value = [&[7_u8, 0, 0, 0][..], &[0; 8]].concat();
            plant_bigram(&user_dir.path, vec![(vec![1, 0, 0, 0], value)]);
            let (context, _instance) = open_with_user(&user_dir.path);
            unsafe { zhuyin_mask_out(context, 0xFFFF_FFFF, 0) };
        },
    );
}

/// A mask that refuses a partial user gram leaves the import working.
#[test]
fn an_import_after_a_refused_bigram_mask_still_adds() {
    let user_dir = TempUserDir::new("partial-mask");
    plant_bigram(
        &user_dir.path,
        vec![(vec![1, 0, 0, 0], vec![0, 0, 0, 0, 1])],
    );
    let (context, instance) = open_with_user(&user_dir.path);
    let reading = "ㄋㄧˇ ㄋㄧˇ ㄋㄧˇ ㄋㄧˇ".as_bytes();
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(unsafe { import(context, 1, "你你你你", reading, 5) });
    assert!(!unsafe { zhuyin_mask_out(context, 0xFFFF_FFFF, 0) });
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(unsafe { import(context, 1, "你你你你", reading, 4) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The import reads a chewing reading directly: an invalid byte in a later
/// segment keeps the earlier key and adds, and one in the only or the last
/// whole segment refuses the reading.
#[test]
fn the_import_reads_a_chewing_reading_up_to_an_invalid_byte() {
    let user_dir = TempUserDir::new("direct-reading");
    let (context, instance) = open_with_user(&user_dir.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let before = unsafe { tokens_of(instance, "甲") };
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(unsafe { import(context, 7, "甲", &["ㄉㄚ ".as_bytes(), b"\xff"].concat(), 5) });
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { tokens_of(instance, "甲") },
        [before, vec![0x0700_0001]].concat()
    );
    let whole = "ㄋㄧˇ ㄏㄠˇ".as_bytes();
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(!unsafe { import(context, 7, "你好", &[whole, b"\xff"].concat(), 1) });
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(!unsafe {
        import(
            context,
            7,
            "你好",
            &["ㄋㄧˇ".as_bytes(), b"\xff", "ㄏㄠˇ".as_bytes()].concat(),
            1,
        )
    });
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    assert!(unsafe { import(context, 7, "你好", whole, 1) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// Register row 97: `mask_out` reloads library 1 and `_mask_out_records`
/// appends a kept ADD with both chunk pointers (`phrase_index_logger.h:245`).
#[test]
fn mask_out_aborts_on_a_surviving_add() {
    crate::support::assert_aborts(
        "import_and_user_store::mask_out_aborts_on_a_surviving_add",
        || {
            let user_dir = TempUserDir::new("mask-surviving-add");
            crate::support::plant_log(&user_dir.path, "gb_char.dbin", &[]);
            let context = init_at(&user_dir.path);
            assert!(!context.is_null());
            let payload = [crate::support::logs::header(), crate::support::logs::add()].concat();
            crate::support::plant_log(&user_dir.path, "gb_char.dbin", &payload);
            let _ = unsafe { crate::support::zhuyin_mask_out(context, 0xffff_ffff, 0x0100_0001) };
        },
    );
}

/// Register row 85: `zhuyin_mask_out` loads token 1's gram, which the mask
/// leaves alone, and `SingleGram::mask_out` reads its total from three bytes
/// (`ngram.cpp:80`, `memory_chunk.h:390`).
#[test]
fn mask_out_aborts_on_a_short_bigram_value() {
    crate::support::assert_aborts(
        "import_and_user_store::mask_out_aborts_on_a_short_bigram_value",
        || {
            let user_dir = TempUserDir::new("mask-short-gram");
            crate::support::plant_bigram(
                &user_dir.path,
                vec![(1_u32.to_le_bytes().to_vec(), vec![7, 0, 0])],
            );
            let context = init_at(&user_dir.path);
            assert!(!context.is_null());
            let _ = unsafe { crate::support::zhuyin_mask_out(context, 0xffff_ffff, 0) };
        },
    );
}

/// Register row 96 at mask-out, after the user tables are masked.
// Unix only, for the reason `oxpinyin`'s `save_stops_at_the_directory_with_the_pins_files` gives.
#[cfg(unix)]
#[test]
fn mask_out_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::support::assert_aborts(
            "import_and_user_store::mask_out_aborts_on_a_directory_library",
            || {
                let _ = map_a_directory_library("mask");
            },
        );
    } else {
        assert!(map_a_directory_library("mask"));
    }
}
