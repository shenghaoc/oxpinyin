#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::Fixture;

use crate::support::{
    ChewingKey, DEFAULT_SORT, assert_aborts, cstr, pinyin_get_character_offset,
    pinyin_get_left_pinyin_offset, pinyin_get_pinyin_offset, pinyin_get_pinyin_string,
    pinyin_get_pinyin_strings, pinyin_get_right_pinyin_offset, pinyin_guess_candidates,
    pinyin_parse_full_pinyin,
};

use std::ptr;

/// A key whose initial is outside the table dimensions (24 or more).
fn out_of_dims_initial() -> ChewingKey {
    ChewingKey { packed: 24 }
}

#[test]
fn a_cursor_over_a_keyless_parse_aborts() {
    // `'''` consumes 3 bytes and places no key: `fill_matrix` leaves the
    // matrix empty and the cursor loop's first `get_column_size(1)` asserts.
    assert_aborts(
        "matrix_bounds::a_cursor_over_a_keyless_parse_aborts",
        || {
            let fixture = Fixture::parsed("keyless-cursor", "'''", 3);
            let instance = fixture.instance();
            let mut offset = 0;
            unsafe { pinyin_get_pinyin_offset(instance, 1, &raw mut offset) };
        },
    );
}

#[test]
fn cursor_zero_over_a_keyless_parse_reads_no_column() {
    let fixture = Fixture::parsed("keyless-zero", "'''", 3);
    let instance = fixture.instance();
    let mut offset = usize::MAX;
    assert!(unsafe { pinyin_get_pinyin_offset(instance, 0, &raw mut offset) });
    assert_eq!(offset, 0);
}

#[test]
fn guess_candidates_past_an_early_stopped_parse_aborts() {
    // `ni,'hao` parses `ni` (2 bytes): 3 columns, so offset 4 reads column 3.
    assert_aborts(
        "matrix_bounds::guess_candidates_past_an_early_stopped_parse_aborts",
        || {
            let fixture = Fixture::parsed("guess-past", "ni,'hao", 2);
            let instance = fixture.instance();
            unsafe { pinyin_guess_candidates(instance, 4, DEFAULT_SORT) };
        },
    );
}

#[test]
fn guess_candidates_at_the_reserved_slot_and_one_past_it_answer() {
    let fixture = Fixture::parsed("guess-slot", "ni,'hao", 2);
    let instance = fixture.instance();
    // Columns 0..=2 exist; offset 3 reads column 2.
    for offset in [2, 3] {
        let _ = unsafe { pinyin_guess_candidates(instance, offset, DEFAULT_SORT) };
    }
}

#[test]
fn the_left_move_past_an_early_stopped_parse_aborts() {
    assert_aborts(
        "matrix_bounds::the_left_move_past_an_early_stopped_parse_aborts",
        || {
            let fixture = Fixture::parsed("left-past", "ni,'hao", 2);
            let instance = fixture.instance();
            let mut left = 0;
            unsafe { pinyin_get_left_pinyin_offset(instance, 4, &raw mut left) };
        },
    );
}

#[test]
fn the_right_move_from_past_the_last_column_aborts() {
    assert_aborts(
        "matrix_bounds::the_right_move_from_past_the_last_column_aborts",
        || {
            let fixture = Fixture::parsed("right-past", "ni,'hao", 2);
            let instance = fixture.instance();
            let mut right = 0;
            unsafe { pinyin_get_right_pinyin_offset(instance, 3, &raw mut right) };
        },
    );
}

#[test]
fn the_character_offset_past_the_last_column_aborts() {
    assert_aborts(
        "matrix_bounds::the_character_offset_past_the_last_column_aborts",
        || {
            let fixture = Fixture::parsed("char-past", "ni,'hao", 2);
            let instance = fixture.instance();
            let mut length = 0;
            unsafe {
                pinyin_get_character_offset(instance, cstr("你").as_ptr(), 3, &raw mut length)
            };
        },
    );
}

#[test]
fn a_null_phrase_past_the_last_column_aborts() {
    // Result: a NULL phrase past the last column aborts at
    // `assert(offset < matrix.size())` (`pinyin.cpp:3203`) rather than
    // answering `false`.
    assert_aborts(
        "matrix_bounds::a_null_phrase_past_the_last_column_aborts",
        || {
            let fixture = Fixture::parsed("char-null-phrase", "ni,'hao", 2);
            let mut length = 0;
            unsafe {
                pinyin_get_character_offset(fixture.instance(), ptr::null(), 3, &raw mut length)
            };
        },
    );
}

#[test]
fn a_null_phrase_within_the_columns_answers_false() {
    let fixture = Fixture::parsed("char-null-within", "ni,'hao", 2);
    let mut length = 7;
    assert!(!unsafe {
        pinyin_get_character_offset(fixture.instance(), ptr::null(), 2, &raw mut length)
    });
    assert_eq!(length, 7);
}

#[test]
fn a_key_outside_the_table_dimensions_aborts_every_renderer() {
    assert_aborts(
        "matrix_bounds::a_key_outside_the_table_dimensions_aborts_every_renderer",
        || {
            let fixture = Fixture::new("key-dims");
            let instance = fixture.instance();
            let mut key = out_of_dims_initial();
            let mut text = ptr::null_mut();
            unsafe { pinyin_get_pinyin_string(instance, &raw mut key, &raw mut text) };
        },
    );
}

#[test]
fn the_strings_getter_aborts_on_a_key_outside_the_table_dimensions() {
    assert_aborts(
        "matrix_bounds::the_strings_getter_aborts_on_a_key_outside_the_table_dimensions",
        || {
            let fixture = Fixture::new("key-dims-strings");
            let instance = fixture.instance();
            let mut key = ChewingKey { packed: 18 << 7 };
            unsafe {
                pinyin_get_pinyin_strings(instance, &raw mut key, ptr::null_mut(), ptr::null_mut())
            };
        },
    );
}

#[test]
fn a_tone_outside_the_table_dimensions_aborts_the_tone_renderers() {
    assert_aborts(
        "matrix_bounds::a_tone_outside_the_table_dimensions_aborts_the_tone_renderers",
        || {
            let fixture = Fixture::new("key-tone");
            let instance = fixture.instance();
            let mut key = ChewingKey { packed: 0 };
            assert!(unsafe {
                pinyin_parse_full_pinyin(instance, cstr("ni").as_ptr(), &raw mut key)
            });
            key.packed |= 6 << 12;
            let mut text = ptr::null_mut();
            unsafe { pinyin_get_pinyin_string(instance, &raw mut key, &raw mut text) };
        },
    );
}

#[test]
fn the_shengmu_yunmu_getter_does_not_read_the_tone() {
    // `get_shengmu_string` and `get_yunmu_string` assert no tone.
    let fixture = Fixture::new("key-tone-strings");
    let instance = fixture.instance();
    let mut key = ChewingKey { packed: 0 };
    assert!(unsafe { pinyin_parse_full_pinyin(instance, cstr("ni").as_ptr(), &raw mut key) });
    key.packed |= 6 << 12;
    assert!(unsafe {
        pinyin_get_pinyin_strings(instance, &raw mut key, ptr::null_mut(), ptr::null_mut())
    });
}
