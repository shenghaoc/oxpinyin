//! Import, mask-out and the user index, bigram and file rows.

use super::*;

/// Opens and closes `user_dir` once, so that the profile exists.
fn first_init(user_dir: &Path) {
    let context = init_on(&system_dir(), Some(user_dir));
    assert!(!context.is_null());
    zhuyin_fini(context);
}

/// A user phrase added again after its library reloads unsaved aborts.
#[test]
fn adding_a_user_phrase_again_after_a_reload_aborts() {
    assert_aborts(
        "boundary_tests::import_and_user_store::adding_a_user_phrase_again_after_a_reload_aborts",
        || {
            let user_dir = TempUserDir::new("reload-reuse");
            let (context, _instance) = open_with_user(&user_dir.path);
            let reading = "ㄏㄠˇ ㄋㄧˇ".as_bytes();
            assert!(import(context, 7, "好你", reading, 50));
            assert!(zhuyin_unload_phrase_library(context, 7));
            assert!(zhuyin_load_phrase_library(context, 7));
            import(context, 7, "好你", reading, 50);
        },
    );
}

/// libzhuyin's import parses chewing (`zhuyin.cpp:515-523`): ㄉㄣ, a reading
/// no typed input spells, stores key 0x1484 and survives a reopen.
#[test]
fn an_imported_den_reading_stores_its_key() {
    let user_dir = TempUserDir::new("den");
    let (context, instance) = open_with_user(&user_dir.path);
    assert!(import(context, 7, "扽", "ㄉㄣ".as_bytes(), 5));
    let tokens = user_tokens(instance, "扽");
    assert_eq!(tokens, [0x0700_0001]);
    assert_eq!(
        nth_pronunciation(instance, tokens[0], 0),
        (true, 1, "8414".to_owned())
    );
    assert!(zhuyin_save(context));
    close(context, instance);
    let (context, instance) = open_with_user(&user_dir.path);
    assert_eq!(
        nth_pronunciation(instance, tokens[0], 0),
        (true, 1, "8414".to_owned())
    );
    close(context, instance);
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

/// mask_out walks the user index and asserts on a key longer than 16 words
/// (register row 84).
#[test]
fn mask_out_over_an_overlong_user_index_key_aborts() {
    assert_aborts(
        "boundary_tests::import_and_user_store::mask_out_over_an_overlong_user_index_key_aborts",
        || {
            let user_dir = TempUserDir::new("overlong-mask");
            overlong_index_profile(&user_dir.path);
            let (context, _instance) = open_with_user(&user_dir.path);
            zhuyin_mask_out(context, 0xFFFF_FFFF, 0);
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
        zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()),
        6
    );
    assert!(zhuyin_guess_candidates_after_cursor(instance, 0));
    assert!(zhuyin_guess_candidates_before_cursor(instance, 6));
    close(context, instance);
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
    import(context, 7, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 1);
}

/// Two library-7 tokens for one phrase key (register row 93).
#[test]
fn adding_a_phrase_with_two_library_tokens_aborts() {
    assert_aborts(
        "boundary_tests::import_and_user_store::adding_a_phrase_with_two_library_tokens_aborts",
        || {
            let user_dir = TempUserDir::new("duplicate-token");
            first_init(&user_dir.path);
            let value = [0x0700_0001_u32.to_le_bytes(), 0x0700_0002_u32.to_le_bytes()].concat();
            add_after_index(&user_dir.path, &value);
        },
    );
}

/// The phrase key points at the token of another phrase of the same length
/// (register row 104).
#[test]
fn adding_a_phrase_whose_index_token_has_other_text_aborts() {
    assert_aborts(
        "boundary_tests::import_and_user_store::adding_a_phrase_whose_index_token_has_other_text_aborts",
        || {
            let user_dir = TempUserDir::new("text-mismatch");
            let (context, instance) = open_with_user(&user_dir.path);
            assert!(import(context, 7, "你們", "ㄋㄧˇ ㄇㄣˊ".as_bytes(), 1));
            assert!(zhuyin_save(context));
            let token = user_tokens(instance, "你們")[0];
            close(context, instance);
            add_after_index(&user_dir.path, &token.to_le_bytes());
        },
    );
}

/// A user bigram key that is not a four-byte token aborts the mask walk.
#[test]
fn mask_out_over_a_non_token_bigram_key_aborts() {
    assert_aborts(
        "boundary_tests::import_and_user_store::mask_out_over_a_non_token_bigram_key_aborts",
        || {
            let user_dir = TempUserDir::new("bigram-key");
            plant_bigram(&user_dir.path, vec![(vec![0, 1], vec![7, 0, 0, 0])]);
            let (context, _instance) = open_with_user(&user_dir.path);
            zhuyin_mask_out(context, 0xFFFF_FFFF, 0);
        },
    );
}

/// A user gram whose item survives the mask with a zero token aborts the walk.
#[test]
fn mask_out_over_a_residual_bigram_gram_aborts() {
    assert_aborts(
        "boundary_tests::import_and_user_store::mask_out_over_a_residual_bigram_gram_aborts",
        || {
            let user_dir = TempUserDir::new("bigram-residual");
            let value = [&[7_u8, 0, 0, 0][..], &[0; 8]].concat();
            plant_bigram(&user_dir.path, vec![(vec![1, 0, 0, 0], value)]);
            let (context, _instance) = open_with_user(&user_dir.path);
            zhuyin_mask_out(context, 0xFFFF_FFFF, 0);
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
    assert!(import(context, 1, "你你你你", reading, 5));
    assert!(!zhuyin_mask_out(context, 0xFFFF_FFFF, 0));
    assert!(import(context, 1, "你你你你", reading, 4));
    close(context, instance);
}

/// The import reads a chewing reading directly: an invalid byte in a later
/// segment keeps the earlier key and adds, and one in the only or the last
/// whole segment refuses the reading.
#[test]
fn the_import_reads_a_chewing_reading_up_to_an_invalid_byte() {
    let user_dir = TempUserDir::new("direct-reading");
    let (context, instance) = open_with_user(&user_dir.path);
    let before = tokens_of(instance, "甲");
    assert!(import(
        context,
        7,
        "甲",
        &["ㄉㄚ ".as_bytes(), b"\xff"].concat(),
        5
    ));
    assert_eq!(
        tokens_of(instance, "甲"),
        [before, vec![0x0700_0001]].concat()
    );
    let whole = "ㄋㄧˇ ㄏㄠˇ".as_bytes();
    assert!(!import(context, 7, "你好", &[whole, b"\xff"].concat(), 1));
    assert!(!import(
        context,
        7,
        "你好",
        &["ㄋㄧˇ".as_bytes(), b"\xff", "ㄏㄠˇ".as_bytes()].concat(),
        1
    ));
    assert!(import(context, 7, "你好", whole, 1));
    close(context, instance);
}

/// Whether the phrase segmenter matched the one-character `龘`, and the token
/// it stored (`null_token` after a failed match).
fn segmented_token(instance: *mut ZhuyinInstance) -> (bool, u32) {
    let matched = zhuyin_phrase_segment(instance, c"龘".as_ptr());
    let mut count = UNTOUCHED;
    assert!(zhuyin_get_n_phrase(instance, &raw mut count));
    assert_eq!(count, 1);
    let mut token = UNTOUCHED;
    assert!(zhuyin_get_phrase_token(instance, 0, &raw mut token));
    (matched, token)
}

/// Every pin scoring step reads the token's phrase item first and skips the
/// token when the read fails (`phrase_lookup.cpp:310,329`,
/// `phonetic_lookup.h:646,674`). A phrase-table token of library 7 whose item
/// is gone is skipped by the segmenter however strong its sentence-start
/// bigram; the same bigram admits the readable item with a zero unigram. The
/// stale phase has no sentence witness: the n-best result is the same with
/// and without the gate, so the model layer covers the n-best costs.
#[test]
fn a_user_file_token_without_its_item_is_skipped_and_a_zero_item_is_not() {
    let user = TempUserDir::new("stale-user-file-token");
    let (context, instance) = open_with_user(&user.path);
    assert!(import(context, 7, "龘", "ㄋㄧˇ".as_bytes(), 0));
    let [token] = user_tokens(instance, "龘")[..] else {
        panic!("one user token for 龘");
    };
    assert!(zhuyin_save(context));
    close(context, instance);
    let item_file = std::fs::read(user.path.join("user.bin")).expect("user.bin");
    // The sentence-start gram admits the token with all of its mass.
    let gram = [
        100_u32.to_le_bytes(),
        token.to_le_bytes(),
        100_u32.to_le_bytes(),
    ]
    .concat();
    let plant = || {
        plant_bigram(
            &user.path,
            vec![(1_u32.to_le_bytes().to_vec(), gram.clone())],
        )
    };

    // The item is gone: the phrase table and the pinyin index still name it.
    let empty = oxpinyin_data::chunk_write::build_chunk(&[]).expect("an empty library");
    std::fs::write(
        user.path.join("user.bin"),
        oxpinyin_data::chunk_format::build_memory_chunk(
            &empty[oxpinyin_data::chunk_format::CHUNK_HEADER_SIZE..],
        )
        .expect("frame"),
    )
    .expect("write user.bin");
    plant();
    let (context, instance) = open_with_user(&user.path);
    assert_eq!(user_tokens(instance, "龘"), [token], "the stale phrase row");
    assert_eq!(segmented_token(instance), (false, crate::types::null_token));
    close(context, instance);

    // The item is back with a zero unigram: the bigram branch admits it.
    std::fs::write(user.path.join("user.bin"), &item_file).expect("restore user.bin");
    plant();
    let (context, instance) = open_with_user(&user.path);
    assert_eq!(segmented_token(instance), (true, token));
    close(context, instance);
}
