#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::Fixture;

use crate::support::{
    ChewingKey, DEFAULT_SORT, IBUS_DEFAULT_SORT, assert_aborts, capture_rows, cstr,
    lookup_candidate_type_t, n_candidate, pinyin_get_character_offset,
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

// pinyin.cpp:3074-3075 enters the loop at zero on an empty matrix.
#[test]
fn right_offset_zero_over_a_keyless_parse_aborts() {
    assert_aborts(
        "matrix_bounds::right_offset_zero_over_a_keyless_parse_aborts",
        || {
            let fixture = Fixture::parsed("keyless-right", "'''", 3);
            let mut right = usize::MAX;
            unsafe { pinyin_get_right_pinyin_offset(fixture.instance(), 0, &raw mut right) };
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

/// `ni` parses to a matrix of three columns: the key at 0, an empty column
/// and the reserved slot (`phonetic_key_matrix.cpp:47-62`). At offsets 2 and
/// 3 the span loop does not run (`end = start + 1` is not below the size,
/// `pinyin.cpp:2229-2231`), and `_check_offset` reads column `offset - 1`
/// only (`:2163-2181`), so both answer `true` with no searched row. The
/// LONGER row does not depend on the offset: its search starts at column 0
/// with the parsed key count (`:1874-1883`,
/// `phonetic_key_matrix.cpp:504-529`), so every offset lists the row of
/// offset 0, unless the sort option leaves it out (`pinyin.cpp:2292-2293`).
#[test]
fn guess_candidates_at_the_reserved_slot_and_one_past_it_answer() {
    let fixture = Fixture::parsed("guess-slot", "ni,'hao", 2);
    let instance = fixture.instance();
    let longer = lookup_candidate_type_t::LONGER_CANDIDATE;
    assert!(unsafe { pinyin_guess_candidates(instance, 0, IBUS_DEFAULT_SORT) });
    // SAFETY: the fixture's instance is live; only this thread calls into it.
    let at_zero: Vec<_> = unsafe { capture_rows(instance) }
        .into_iter()
        .filter(|(kind, _, _)| *kind == longer)
        .collect();
    assert_eq!(at_zero.len(), 1, "{at_zero:?}");
    // Columns 0..=2 exist; offset 3 reads column 2.
    for offset in [2, 3] {
        assert!(unsafe { pinyin_guess_candidates(instance, offset, IBUS_DEFAULT_SORT) });
        // SAFETY: the fixture's instance is live; only this thread calls into it.
        assert_eq!(
            unsafe { capture_rows(instance) },
            at_zero,
            "offset {offset}"
        );
        assert!(unsafe { pinyin_guess_candidates(instance, offset, DEFAULT_SORT) });
        // SAFETY: the fixture's instance is live; only this thread calls into it.
        assert_eq!(unsafe { n_candidate(instance) }, 0, "offset {offset}");
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
