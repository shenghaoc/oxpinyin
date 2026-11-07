//! The parsed modes (index-parsed full pinyin, double pinyin, chewing) keep
//! the matrix the pin built at the parse, and the cursor, key and character
//! offset functions read it: the pin reads every item of a column in
//! `_get_char_offset_recur` (`pinyin.cpp:3138-3191`) and
//! `pinyin_get_left_pinyin_offset` (`:3031-3060`), and the first item of
//! columns the divided step fills in the other key and offset functions
//! (`:2930-2964`, `:3010-3028`, `:3063-3095`). Register rows 19 and 66.

use std::ptr;

use crate::config::{
    pinyin_set_double_pinyin_scheme, pinyin_set_full_pinyin_scheme, pinyin_set_zhuyin_scheme,
};
use crate::context::pinyin_fini;
use crate::cursor::{
    pinyin_get_left_pinyin_offset, pinyin_get_pinyin_key, pinyin_get_pinyin_key_rest,
    pinyin_get_pinyin_key_rest_positions, pinyin_get_pinyin_offset, pinyin_get_right_pinyin_offset,
};
use crate::instance::pinyin_free_instance;
use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_end_add_phrases, pinyin_iterator_add_phrase,
};
use crate::parse::{
    pinyin_parse_more_chewings, pinyin_parse_more_double_pinyins, pinyin_parse_more_full_pinyins,
};
use crate::pinyin_set_options;
use crate::sentence::pinyin_get_character_offset;
use crate::test_support::{TempUserDir, cstr, open};
use crate::types::{ChewingKey, ChewingKeyRest, PinyinContext, PinyinInstance};

const L_N: u32 = 1 << 15;
const DIVIDED: u32 = 1 << 7;

/// A context and instance that release themselves.
struct Fixture {
    context: *mut PinyinContext,
    instance: *mut PinyinInstance,
    _user_dir: TempUserDir,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let user_dir = TempUserDir::new(tag);
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        Self {
            context,
            instance,
            _user_dir: user_dir,
        }
    }

    /// 弄 imported with the one reading `long`, so only a fuzzy alternative
    /// of `nong` pronounces it.
    fn with_long_only(tag: &str) -> Self {
        let fixture = Self::new(tag);
        let import = pinyin_begin_add_phrases(fixture.context, 7);
        assert!(pinyin_iterator_add_phrase(
            import,
            cstr("弄").as_ptr(),
            cstr("long").as_ptr(),
            1
        ));
        pinyin_end_add_phrases(import);
        fixture
    }

    /// `sian` (Luoma for `xian`) through the Luoma index parse with the divided table on: the
    /// pin's matrix also holds `xi` at column 0 and `an` at column 2.
    fn luoma_xian(tag: &str) -> Self {
        let fixture = Self::new(tag);
        assert!(pinyin_set_options(fixture.context, 0x8 | DIVIDED));
        assert!(pinyin_set_full_pinyin_scheme(fixture.context, 2));
        assert_eq!(
            pinyin_parse_more_full_pinyins(fixture.instance, cstr("sian").as_ptr()),
            4
        );
        fixture
    }

    fn character_offset(&self, offset: usize) -> (bool, usize) {
        let mut length = usize::MAX;
        let ok = pinyin_get_character_offset(
            self.instance,
            cstr("弄").as_ptr(),
            offset,
            &raw mut length,
        );
        (ok, length)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        pinyin_free_instance(self.instance);
        pinyin_fini(self.context);
    }
}

#[test]
fn the_character_offset_reads_the_double_pinyin_fuzzy_alternative() {
    let fixture = Fixture::with_long_only("char-offset-double-fuzzy");
    assert!(pinyin_set_options(fixture.context, 0x8 | L_N));
    assert!(pinyin_set_double_pinyin_scheme(fixture.context, 2));
    assert_eq!(
        pinyin_parse_more_double_pinyins(fixture.instance, cstr("ns").as_ptr()),
        2
    );
    assert_eq!(fixture.character_offset(2), (true, 1));
}

#[test]
fn the_character_offset_reads_the_chewing_fuzzy_alternative() {
    let fixture = Fixture::with_long_only("char-offset-chewing-fuzzy");
    assert!(pinyin_set_options(fixture.context, 0x8 | L_N));
    assert!(pinyin_set_zhuyin_scheme(fixture.context, 1));
    assert_eq!(
        pinyin_parse_more_chewings(fixture.instance, cstr("sj/").as_ptr()),
        3
    );
    assert_eq!(fixture.character_offset(3), (true, 1));
}

#[test]
fn the_left_offset_reads_the_divided_column() {
    // The divided `an` ends at 4 from column 2, so the pin's scan stops there.
    let fixture = Fixture::luoma_xian("left-divided");
    let mut left = usize::MAX;
    assert!(pinyin_get_left_pinyin_offset(
        fixture.instance,
        4,
        &raw mut left
    ));
    assert_eq!(left, 2);
}

#[test]
fn the_right_offset_reads_the_divided_column() {
    let fixture = Fixture::luoma_xian("right-divided");
    let mut right = usize::MAX;
    assert!(pinyin_get_right_pinyin_offset(
        fixture.instance,
        2,
        &raw mut right
    ));
    assert_eq!(right, 4);
}

#[test]
fn the_cursor_offset_stops_at_the_divided_column() {
    let fixture = Fixture::luoma_xian("offset-divided");
    let mut offset = usize::MAX;
    assert!(pinyin_get_pinyin_offset(
        fixture.instance,
        2,
        &raw mut offset
    ));
    assert_eq!(offset, 2);
}

#[test]
fn the_key_getter_reads_the_divided_column() {
    let fixture = Fixture::luoma_xian("key-divided");
    let mut key: *mut ChewingKey = ptr::null_mut();
    assert!(pinyin_get_pinyin_key(fixture.instance, 2, &raw mut key));
    assert!(!key.is_null());
}

#[test]
fn the_key_rest_getter_reads_the_divided_column() {
    let fixture = Fixture::luoma_xian("key-rest-divided");
    let mut rest: *mut ChewingKeyRest = ptr::null_mut();
    assert!(pinyin_get_pinyin_key_rest(
        fixture.instance,
        2,
        &raw mut rest
    ));
    let (mut begin, mut end) = (0_u16, 0_u16);
    assert!(pinyin_get_pinyin_key_rest_positions(
        fixture.instance,
        rest,
        &raw mut begin,
        &raw mut end
    ));
    assert_eq!((begin, end), (2, 4));
}
