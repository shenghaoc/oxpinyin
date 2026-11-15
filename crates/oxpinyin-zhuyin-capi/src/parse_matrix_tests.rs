//! The zhuyin twins of `oxpinyin-capi`'s parse-matrix tests: the parsed modes
//! keep the matrix the pin built at the parse, and the offset and key
//! functions read it. The pin's zhuyin functions read every item of a column
//! in the character offset (`zhuyin.cpp:2117`) and the left offset (`:2009`),
//! and the first item of the column in the other getters
//! (`:1902`, `:1924`, `:1961`, `:2043-2053`). Register rows 19 and 66. The
//! pinyin twins hold the first-item reads, which go through the shared
//! facade code.

use crate::candidates::{
    zhuyin_choose_candidate, zhuyin_clear_constraint, zhuyin_get_candidate,
    zhuyin_get_candidate_string, zhuyin_get_candidate_type, zhuyin_get_n_candidate,
};
use crate::config::{zhuyin_set_chewing_scheme, zhuyin_set_options};
use crate::context::zhuyin_fini;
use crate::instance::zhuyin_free_instance;
use crate::iterators::{
    zhuyin_begin_add_phrases, zhuyin_end_add_phrases, zhuyin_iterator_add_phrase,
};
use crate::parse::{zhuyin_parse_more_chewings, zhuyin_parse_more_full_pinyins};
use crate::sentence::{zhuyin_get_character_offset, zhuyin_guess_candidates_after_cursor};
use crate::test_support::{TempUserDir, assert_aborts_after_setup, cstr, open_with_user};
use crate::types::{ZhuyinContext, ZhuyinInstance, lookup_candidate_type_t};

const L_N: u32 = 1 << 15;

/// The full-pinyin export fills the zhuyin matrix (`zhuyin.cpp:1017-1043`).
/// Default FORCE_TONE refuses this untoned input (`pinyin_parser2.cpp:185-189`).
fn full_pinyin_separator() -> Fixture {
    let fixture = Fixture::new("full-pinyin-separator");
    let input = cstr("ni'hao");
    assert_eq!(
        zhuyin_parse_more_full_pinyins(fixture.instance, input.as_ptr()),
        0
    );
    assert!(zhuyin_set_options(
        fixture.context,
        oxpinyin_core::OptionBits::USE_TONE.bits()
    ));
    assert_eq!(
        zhuyin_parse_more_full_pinyins(fixture.instance, input.as_ptr()),
        6
    );
    fixture
}

/// `zhuyin-separator/guess-3` at 074a2219: the preceding column is the
/// separator's zero key, so `_check_offset` asserts (`zhuyin.cpp:1440-1458`).
#[test]
fn a_full_pinyin_guess_after_the_separator_aborts() {
    assert_aborts_after_setup(
        "parse_matrix_tests::a_full_pinyin_guess_after_the_separator_aborts",
        full_pinyin_separator,
        |fixture| {
            zhuyin_guess_candidates_after_cursor(fixture.instance, 3);
        },
    );
}

/// `zhuyin-separator/guess-2-clear-{2,3}` at 074a2219: the row starts at 2
/// (`zhuyin.cpp:1498-1509`); choose answers its end 6 (`:1649-1653`) and
/// either clear reaches [2, 6) (`:1669-1675`, `phonetic_lookup.cpp:88-115`).
#[rstest::rstest]
fn a_full_pinyin_separator_forcing_clears_at_either_column(#[values(2, 3)] probe: usize) {
    let fixture = full_pinyin_separator();
    let instance = fixture.instance;
    assert!(zhuyin_guess_candidates_after_cursor(instance, 2));
    let mut count = 0;
    assert!(zhuyin_get_n_candidate(instance, &raw mut count));
    let row = (0..count)
        .find_map(|index| {
            let mut row = std::ptr::null_mut();
            assert!(zhuyin_get_candidate(instance, index, &raw mut row));
            let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
            assert!(zhuyin_get_candidate_type(instance, row, &raw mut kind));
            let mut text = std::ptr::null();
            assert!(zhuyin_get_candidate_string(instance, row, &raw mut text));
            assert!(!text.is_null());
            // SAFETY: the getter returns this live row's NUL-terminated string.
            let text = unsafe { std::ffi::CStr::from_ptr(text) };
            (kind == lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR
                && text.to_bytes() == "好".as_bytes())
            .then_some(row)
        })
        .expect("after-cursor 好");
    assert_eq!(zhuyin_choose_candidate(instance, 2, row), 6);
    assert!(zhuyin_clear_constraint(instance, probe));
    assert!(!zhuyin_clear_constraint(instance, probe));
}

/// A context and instance that release themselves.
struct Fixture {
    context: *mut ZhuyinContext,
    instance: *mut ZhuyinInstance,
    _user_dir: TempUserDir,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let user_dir = TempUserDir::new(tag);
        let (context, instance) = open_with_user(&user_dir.path);
        Self {
            context,
            instance,
            _user_dir: user_dir,
        }
    }

    /// 弄 imported with the one reading ㄌㄨㄥ (`long`).
    fn with_long_only(tag: &str) -> Self {
        let fixture = Self::new(tag);
        let import = zhuyin_begin_add_phrases(fixture.context, 5);
        assert!(zhuyin_iterator_add_phrase(
            import,
            cstr("弄").as_ptr(),
            cstr("ㄌㄨㄥ").as_ptr(),
            1
        ));
        zhuyin_end_add_phrases(import);
        fixture
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        zhuyin_free_instance(self.instance);
        zhuyin_fini(self.context);
    }
}

#[test]
fn the_character_offset_reads_the_chewing_fuzzy_alternative() {
    let fixture = Fixture::with_long_only("z-char-offset-fuzzy");
    assert!(zhuyin_set_options(fixture.context, 0x8 | L_N));
    assert!(zhuyin_set_chewing_scheme(fixture.context, 1));
    assert_eq!(
        zhuyin_parse_more_chewings(fixture.instance, cstr("sj/").as_ptr()),
        3
    );
    let mut length = usize::MAX;
    assert!(zhuyin_get_character_offset(
        fixture.instance,
        cstr("弄").as_ptr(),
        2,
        &raw mut length
    ));
    // Offset 2, not the parsed length 3: at 3 the pin's walk reaches the zero
    // key and fails (`zhuyin.cpp:2108-2128`; see `zhuyin_get_character_offset`).
    // The key's end lies past offset 2, so no character is consumed yet; the
    // walk answers true only because `long` pronounces 弄.
    assert_eq!(length, 0);
}
