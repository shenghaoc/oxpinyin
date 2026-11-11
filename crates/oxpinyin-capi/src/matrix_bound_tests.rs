//! Register row 66: the pin's matrix has `parsed_len + 1` columns (none while
//! the parse placed no key), and every read past them dies in
//! `PhoneticTable::get_column_size`'s assert (`phonetic_key_matrix.h:103`);
//! chewing keys packed from caller memory die in `get_table_index` and the
//! string renderers (`chewing_key.cpp:28-42`, register row 4).

use std::ptr;

use crate::context::pinyin_fini;
use crate::cursor::{
    pinyin_get_left_pinyin_offset, pinyin_get_pinyin_offset, pinyin_get_pinyin_string,
    pinyin_get_pinyin_strings, pinyin_get_right_pinyin_offset,
};
use crate::instance::pinyin_free_instance;
use crate::keys::pinyin_parse_full_pinyin;
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::sentence::{pinyin_get_character_offset, pinyin_guess_candidates};
use crate::test_support::{DEFAULT_SORT, TempUserDir, assert_aborts, cstr, open};
use crate::types::{ChewingKey, PinyinContext, PinyinInstance};

/// A context, its instance and the user directory they sit in, released in
/// that order when the fixture drops. A death-test child aborts without
/// dropping it; its scratch root removes the directory.
struct Fixture {
    context: *mut PinyinContext,
    instance: *mut PinyinInstance,
    _user_dir: TempUserDir,
}

impl Fixture {
    /// A fresh instance with nothing parsed.
    fn new(tag: &str) -> Self {
        let user_dir = TempUserDir::new(tag);
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        Self {
            context,
            instance,
            _user_dir: user_dir,
        }
    }

    /// An instance over `text`, which parses to `parsed` bytes.
    fn parsed(tag: &str, text: &str, parsed: usize) -> Self {
        let fixture = Self::new(tag);
        assert_eq!(
            pinyin_parse_more_full_pinyins(fixture.instance, cstr(text).as_ptr()),
            parsed
        );
        fixture
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        pinyin_free_instance(self.instance);
        pinyin_fini(self.context);
    }
}

#[test]
fn a_cursor_over_a_keyless_parse_aborts() {
    // `'''` consumes 3 bytes and places no key: `fill_matrix` leaves the
    // matrix empty and the cursor loop's first `get_column_size(1)` asserts.
    assert_aborts(
        "matrix_bound_tests::a_cursor_over_a_keyless_parse_aborts",
        || {
            let fixture = Fixture::parsed("keyless-cursor", "'''", 3);
            let instance = fixture.instance;
            let mut offset = 0;
            pinyin_get_pinyin_offset(instance, 1, &raw mut offset);
        },
    );
}

#[test]
fn cursor_zero_over_a_keyless_parse_reads_no_column() {
    let fixture = Fixture::parsed("keyless-zero", "'''", 3);
    let instance = fixture.instance;
    let mut offset = usize::MAX;
    assert!(pinyin_get_pinyin_offset(instance, 0, &raw mut offset));
    assert_eq!(offset, 0);
}

#[test]
fn guess_candidates_past_an_early_stopped_parse_aborts() {
    // `ni,'hao` parses `ni` (2 bytes): 3 columns, so offset 4 reads column 3.
    assert_aborts(
        "matrix_bound_tests::guess_candidates_past_an_early_stopped_parse_aborts",
        || {
            let fixture = Fixture::parsed("guess-past", "ni,'hao", 2);
            let instance = fixture.instance;
            pinyin_guess_candidates(instance, 4, DEFAULT_SORT);
        },
    );
}

#[test]
fn guess_candidates_at_the_reserved_slot_and_one_past_it_answer() {
    let fixture = Fixture::parsed("guess-slot", "ni,'hao", 2);
    let instance = fixture.instance;
    // Columns 0..=2 exist; offset 3 reads column 2.
    for offset in [2, 3] {
        let _ = pinyin_guess_candidates(instance, offset, DEFAULT_SORT);
    }
}

#[test]
fn the_left_move_past_an_early_stopped_parse_aborts() {
    assert_aborts(
        "matrix_bound_tests::the_left_move_past_an_early_stopped_parse_aborts",
        || {
            let fixture = Fixture::parsed("left-past", "ni,'hao", 2);
            let instance = fixture.instance;
            let mut left = 0;
            pinyin_get_left_pinyin_offset(instance, 4, &raw mut left);
        },
    );
}

#[test]
fn the_right_move_from_past_the_last_column_aborts() {
    assert_aborts(
        "matrix_bound_tests::the_right_move_from_past_the_last_column_aborts",
        || {
            let fixture = Fixture::parsed("right-past", "ni,'hao", 2);
            let instance = fixture.instance;
            let mut right = 0;
            pinyin_get_right_pinyin_offset(instance, 3, &raw mut right);
        },
    );
}

#[test]
fn the_character_offset_past_the_last_column_aborts() {
    assert_aborts(
        "matrix_bound_tests::the_character_offset_past_the_last_column_aborts",
        || {
            let fixture = Fixture::parsed("char-past", "ni,'hao", 2);
            let instance = fixture.instance;
            let mut length = 0;
            pinyin_get_character_offset(instance, cstr("你").as_ptr(), 3, &raw mut length);
        },
    );
}

#[test]
fn the_character_offset_after_a_separator_aborts() {
    // The pin's _check_offset rejects a cursor immediately after a lone
    // zero key (074a2219 pinyin.cpp:3203-3204).
    //
    // The facade aborts without a message, and on the column, range and
    // token-count asserts too. The child therefore writes the walk's answer
    // for the same instance, phrase and offset to fd 2 before the call.
    let stderr = assert_aborts(
        "matrix_bound_tests::the_character_offset_after_a_separator_aborts",
        || {
            use std::io::Write as _;
            let fixture = Fixture::parsed("char-after-separator", "ni'hao", 6);
            // SAFETY: the fixture's instance is live and this thread alone
            // uses it.
            let walk = unsafe { crate::state::instance_ref(fixture.instance) }
                .core
                .character_offset(&"你好".into(), 3);
            // Not `eprintln!`: libtest captures it and the abort drops it.
            writeln!(std::io::stderr(), "walk: {walk:?}").expect("write to stderr");
            let mut length = 0;
            pinyin_get_character_offset(
                fixture.instance,
                cstr("你好").as_ptr(),
                3,
                &raw mut length,
            );
        },
    );
    // The init's `open …/user.conf failed.` precedes it without a newline.
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(
        stderr.ends_with("walk: Err(ZeroKeyOffsetCheck { offset: 3 })\n"),
        "{stderr}"
    );
}

#[test]
fn a_null_phrase_past_the_last_column_aborts() {
    // Result: a NULL phrase past the last column aborts at
    // `assert(offset < matrix.size())` (`pinyin.cpp:3205`) rather than
    // answering `false`.
    assert_aborts(
        "matrix_bound_tests::a_null_phrase_past_the_last_column_aborts",
        || {
            let fixture = Fixture::parsed("char-null-phrase", "ni,'hao", 2);
            let mut length = 0;
            pinyin_get_character_offset(fixture.instance, ptr::null(), 3, &raw mut length);
        },
    );
}

#[test]
fn a_null_phrase_within_the_columns_answers_false() {
    let fixture = Fixture::parsed("char-null-within", "ni,'hao", 2);
    let mut length = 7;
    assert!(!pinyin_get_character_offset(
        fixture.instance,
        ptr::null(),
        2,
        &raw mut length
    ));
    assert_eq!(length, 7);
}

/// A key whose initial is outside the table dimensions (24 or more).
fn out_of_dims_initial() -> ChewingKey {
    ChewingKey { packed: 24 }
}

#[test]
fn a_key_outside_the_table_dimensions_aborts_every_renderer() {
    assert_aborts(
        "matrix_bound_tests::a_key_outside_the_table_dimensions_aborts_every_renderer",
        || {
            let fixture = Fixture::new("key-dims");
            let instance = fixture.instance;
            let mut key = out_of_dims_initial();
            let mut text = ptr::null_mut();
            pinyin_get_pinyin_string(instance, &raw mut key, &raw mut text);
        },
    );
}

#[test]
fn the_strings_getter_aborts_on_a_key_outside_the_table_dimensions() {
    assert_aborts(
        "matrix_bound_tests::the_strings_getter_aborts_on_a_key_outside_the_table_dimensions",
        || {
            let fixture = Fixture::new("key-dims-strings");
            let instance = fixture.instance;
            let mut key = ChewingKey { packed: 18 << 7 };
            pinyin_get_pinyin_strings(instance, &raw mut key, ptr::null_mut(), ptr::null_mut());
        },
    );
}

#[test]
fn a_tone_outside_the_table_dimensions_aborts_the_tone_renderers() {
    assert_aborts(
        "matrix_bound_tests::a_tone_outside_the_table_dimensions_aborts_the_tone_renderers",
        || {
            let fixture = Fixture::new("key-tone");
            let instance = fixture.instance;
            let mut key = ChewingKey { packed: 0 };
            assert!(pinyin_parse_full_pinyin(
                instance,
                cstr("ni").as_ptr(),
                &raw mut key
            ));
            key.packed |= 6 << 12;
            let mut text = ptr::null_mut();
            pinyin_get_pinyin_string(instance, &raw mut key, &raw mut text);
        },
    );
}

#[test]
fn the_shengmu_yunmu_getter_does_not_read_the_tone() {
    // `get_shengmu_string` and `get_yunmu_string` assert no tone.
    let fixture = Fixture::new("key-tone-strings");
    let instance = fixture.instance;
    let mut key = ChewingKey { packed: 0 };
    assert!(pinyin_parse_full_pinyin(
        instance,
        cstr("ni").as_ptr(),
        &raw mut key
    ));
    key.packed |= 6 << 12;
    assert!(pinyin_get_pinyin_strings(
        instance,
        &raw mut key,
        ptr::null_mut(),
        ptr::null_mut()
    ));
}
