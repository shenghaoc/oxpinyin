//! The zhuyin twins of `oxpinyin-capi`'s matrix-bound death tests: the pin's
//! matrix has `parsed_len + 1` columns (none while the parse placed no key)
//! and a read past them dies in `get_column_size`'s assert
//! (`phonetic_key_matrix.h:103`; register row 66), and a key packed outside
//! the table dimensions dies in `get_table_index` and the renderers
//! (`chewing_key.cpp:28-42`).

use std::ptr;

use crate::cursor::zhuyin_get_zhuyin_offset;
use crate::keys::zhuyin_get_zhuyin_string;
use crate::parse::{zhuyin_parse_more_chewings, zhuyin_parse_more_full_pinyins};
use crate::sentence::zhuyin_get_character_offset;
use crate::test_support::{TempUserDir, assert_aborts, close, cstr, open_with_user};
use crate::types::ChewingKey;

#[test]
fn a_cursor_over_a_keyless_parse_aborts() {
    // `'''` consumes 3 bytes through `zhuyin_parse_more_full_pinyins` and
    // places no key; `zhuyin.cpp:1980-1981` reads `get_column_size(1)`.
    assert_aborts(
        "matrix_bound_tests::a_cursor_over_a_keyless_parse_aborts",
        || {
            let user_dir = TempUserDir::new("keyless-cursor");
            let (_context, instance) = open_with_user(&user_dir.path);
            assert_eq!(
                zhuyin_parse_more_full_pinyins(instance, cstr("'''").as_ptr()),
                3
            );
            let mut offset = 0;
            zhuyin_get_zhuyin_offset(instance, 1, &raw mut offset);
        },
    );
}

#[test]
fn cursor_zero_over_a_keyless_parse_reads_no_column() {
    let user_dir = TempUserDir::new("keyless-zero");
    let (context, instance) = open_with_user(&user_dir.path);
    assert_eq!(
        zhuyin_parse_more_full_pinyins(instance, cstr("'''").as_ptr()),
        3
    );
    let mut offset = usize::MAX;
    assert!(zhuyin_get_zhuyin_offset(instance, 0, &raw mut offset));
    assert_eq!(offset, 0);
    close(context, instance);
}

#[test]
fn a_key_outside_the_table_dimensions_aborts_the_renderer() {
    assert_aborts(
        "matrix_bound_tests::a_key_outside_the_table_dimensions_aborts_the_renderer",
        || {
            let mut key = ChewingKey { packed: 24 };
            let mut text = ptr::null_mut();
            zhuyin_get_zhuyin_string(ptr::null_mut(), &raw mut key, &raw mut text);
        },
    );
}

#[test]
fn a_tone_outside_the_table_dimensions_aborts_the_renderer() {
    assert_aborts(
        "matrix_bound_tests::a_tone_outside_the_table_dimensions_aborts_the_renderer",
        || {
            let ni = oxpinyin_core::ChewingKey::from_pinyin("ni").expect("ni is a key");
            let mut key = ChewingKey {
                packed: ni.to_packed() | (6 << 12),
            };
            let mut text = ptr::null_mut();
            zhuyin_get_zhuyin_string(ptr::null_mut(), &raw mut key, &raw mut text);
        },
    );
}

#[test]
fn a_null_phrase_past_the_last_column_aborts() {
    // Result: a NULL phrase past the last column aborts at
    // `assert(offset < matrix.size())` (`zhuyin.cpp:2158`) rather than
    // answering `false`.
    assert_aborts(
        "matrix_bound_tests::a_null_phrase_past_the_last_column_aborts",
        || {
            let user_dir = TempUserDir::new("char-null-phrase");
            let (_context, instance) = open_with_user(&user_dir.path);
            assert_eq!(
                zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()),
                6
            );
            let mut length = 0;
            zhuyin_get_character_offset(instance, ptr::null(), 7, &raw mut length);
        },
    );
}

#[test]
fn a_null_phrase_within_the_columns_answers_false() {
    let user_dir = TempUserDir::new("char-null-within");
    let (context, instance) = open_with_user(&user_dir.path);
    assert_eq!(
        zhuyin_parse_more_chewings(instance, cstr("su3cl3").as_ptr()),
        6
    );
    let mut length = 7;
    assert!(!zhuyin_get_character_offset(
        instance,
        ptr::null(),
        2,
        &raw mut length
    ));
    assert_eq!(length, 7);
    close(context, instance);
}

// With no keys, fill_matrix leaves the matrix empty (phonetic_key_matrix.cpp:30-38).
// The wrapped size-minus-one check reaches the column assertion
// (zhuyin.cpp:1886-1895; phonetic_key_matrix.h:102-103 at 074a2219).
macro_rules! full_pinyin_keyless_query_aborts {
    ($name:ident, $input:literal, $length:expr) => {
        #[test]
        fn $name() {
            assert_aborts(concat!("matrix_bound_tests::", stringify!($name)), || {
                let user = TempUserDir::new(stringify!($name));
                let (_context, instance) = open_with_user(&user.path);
                assert_eq!(
                    zhuyin_parse_more_full_pinyins(instance, cstr($input).as_ptr()),
                    $length
                );
                assert_eq!(
                    crate::parse::zhuyin_get_parsed_input_length(instance),
                    $length
                );
                let mut key = ptr::null_mut();
                crate::cursor::zhuyin_get_zhuyin_key(instance, 0, &raw mut key);
            });
        }
    };
}

full_pinyin_keyless_query_aborts!(full_pinyin_one_separator_key_aborts, "'", 1);
full_pinyin_keyless_query_aborts!(full_pinyin_two_separators_key_aborts, "''", 2);
full_pinyin_keyless_query_aborts!(full_pinyin_three_separators_key_aborts, "'''", 3);
full_pinyin_keyless_query_aborts!(full_pinyin_ni_separator_key_aborts, "ni'", 0);
full_pinyin_keyless_query_aborts!(full_pinyin_ni_separator_invalid_key_aborts, "ni'!", 0);
full_pinyin_keyless_query_aborts!(full_pinyin_ni_separator_i_key_aborts, "ni'i", 0);
