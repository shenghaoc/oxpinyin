//! Import, mask-out and the user index, bigram and file rows.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

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
            assert!(unsafe { zhuyin_unload_phrase_library(context, 7) });
            assert!(unsafe { zhuyin_load_phrase_library(context, 7) });
            import(context, 7, "好你", reading, 50);
        },
    );
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
            assert!(unsafe { zhuyin_save(context) });
            let token = user_tokens(instance, "你們")[0];
            close(context, instance);
            add_after_index(&user_dir.path, &token.to_le_bytes());
        },
    );
}

/// Whether the phrase segmenter matched the one-character `龘`, and the token
/// it stored (`null_token` after a failed match).
fn segmented_token(instance: *mut ZhuyinInstance) -> (bool, u32) {
    let matched = unsafe { zhuyin_phrase_segment(instance, c"龘".as_ptr()) };
    let mut count = UNTOUCHED;
    assert!(unsafe { zhuyin_get_n_phrase(instance, &raw mut count) });
    assert_eq!(count, 1);
    let mut token = UNTOUCHED;
    assert!(unsafe { zhuyin_get_phrase_token(instance, 0, &raw mut token) });
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
    assert!(unsafe { zhuyin_save(context) });
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
