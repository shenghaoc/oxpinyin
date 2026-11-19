#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    TempUserDir, ZhuyinContext, ZhuyinInstance, assert_aborts_after_setup, cstr,
    lookup_candidate_type_t, open_with_user, zhuyin_choose_candidate, zhuyin_clear_constraint,
    zhuyin_fini, zhuyin_free_instance, zhuyin_get_candidate, zhuyin_get_candidate_string,
    zhuyin_get_candidate_type, zhuyin_get_n_candidate, zhuyin_guess_candidates_after_cursor,
    zhuyin_guess_sentence, zhuyin_parse_more_chewings, zhuyin_parse_more_full_pinyins,
    zhuyin_set_options,
};

/// `text` parsed as full pinyin under `scheme` (1 Hanyu, 2 Luoma, 3
/// secondary zhuyin). The full-pinyin export fills the zhuyin matrix
/// (`zhuyin.cpp:1017-1043`). Default FORCE_TONE refuses this untoned input
/// (`pinyin_parser2.cpp:185-189`). The Luoma and secondary-zhuyin index
/// parses run the same parser over another index
/// (`FullPinyinParser2::set_scheme`, `pinyin_parser2.cpp:383-401`), so the
/// `'` holds the same zero-key column (`phonetic_key_matrix.cpp:52-79`).
/// Secondary zhuyin spells 好 `hau`.
fn scheme_separator(scheme: i32, text: &str) -> Fixture {
    let fixture = Fixture::new("full-pinyin-separator");
    assert!(unsafe { crate::support::zhuyin_set_full_pinyin_scheme(fixture.context, scheme) });
    let input = cstr(text);
    assert_eq!(
        unsafe { zhuyin_parse_more_full_pinyins(fixture.instance, input.as_ptr()) },
        0
    );
    assert!(unsafe { zhuyin_set_options(fixture.context, oxpinyin::OptionBits::USE_TONE.bits()) });
    assert_eq!(
        unsafe { zhuyin_parse_more_full_pinyins(fixture.instance, input.as_ptr()) },
        6
    );
    fixture
}

fn guess_after_the_separator_aborts(name: &str, scheme: i32, text: &str) {
    assert_aborts_after_setup(
        name,
        || scheme_separator(scheme, text),
        |fixture| {
            unsafe { zhuyin_guess_candidates_after_cursor(fixture.instance, 3) };
        },
    );
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { zhuyin_free_instance(self.instance) };
        unsafe { zhuyin_fini(self.context) };
    }
}

/// `zhuyin-separator/guess-3` at 074a2219: the preceding column is the
/// separator's zero key, so `_check_offset` asserts (`zhuyin.cpp:1440-1458`).
#[test]
fn a_full_pinyin_guess_after_the_separator_aborts() {
    guess_after_the_separator_aborts(
        "parse_matrix::a_full_pinyin_guess_after_the_separator_aborts",
        1,
        "ni'hao",
    );
}

/// `zhuyin-separator/guess-2-clear-{2,3}` at 074a2219: the row starts at 2
/// (`zhuyin.cpp:1498-1509`); choose answers its end 6 (`:1649-1653`) and
/// either clear reaches [2, 6) (`:1669-1675`, `phonetic_lookup.cpp:88-115`).
#[rstest::rstest]
fn a_full_pinyin_separator_forcing_clears_at_either_column(
    #[values(2, 3)] probe: usize,
    #[values((1, "ni'hao"), (2, "ni'hao"), (3, "ni'hau"))] scheme: (i32, &str),
) {
    let fixture = scheme_separator(scheme.0, scheme.1);
    let instance = fixture.instance;
    let row = after_cursor_row(instance, 2, "好");
    assert_eq!(unsafe { zhuyin_choose_candidate(instance, 2, row) }, 6);
    assert!(unsafe { zhuyin_clear_constraint(instance, probe) });
    assert!(!unsafe { zhuyin_clear_constraint(instance, probe) });
}

/// The after-cursor row at `cursor` whose string is `text`.
fn after_cursor_row(
    instance: *mut ZhuyinInstance,
    cursor: usize,
    wanted: &str,
) -> *mut crate::support::LookupCandidate {
    assert!(unsafe { zhuyin_guess_candidates_after_cursor(instance, cursor) });
    let mut count = 0;
    assert!(unsafe { zhuyin_get_n_candidate(instance, &raw mut count) });
    (0..count)
        .find_map(|index| {
            let mut row = std::ptr::null_mut();
            assert!(unsafe { zhuyin_get_candidate(instance, index, &raw mut row) });
            let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
            assert!(unsafe { zhuyin_get_candidate_type(instance, row, &raw mut kind) });
            let mut text = std::ptr::null();
            assert!(unsafe { zhuyin_get_candidate_string(instance, row, &raw mut text) });
            assert!(!text.is_null());
            // SAFETY: the getter returns this live row's NUL-terminated string.
            let text = unsafe { std::ffi::CStr::from_ptr(text) };
            (kind == lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR
                && text.to_bytes() == wanted.as_bytes())
            .then_some(row)
        })
        .expect("the after-cursor row")
}

/// A reparse leaves the constraint array as the last validation sized it,
/// indexed by that parse's matrix columns (074a2219 `zhuyin.cpp:1058-1081`
/// never touches `m_constraints`). The clear indexes that array with the
/// caller's offset (`zhuyin.cpp:1669-1676`, `phonetic_lookup.cpp:88-118`):
/// after 你好 is forced over [0, 6) of `su3cl3` (7 columns) and the input is
/// reparsed as `su3`, an offset past the array refuses, column 6 is free,
/// and column 4 still reaches the run's `NOSEARCH` cell.
#[rstest::rstest]
#[case::past_the_array(usize::MAX, false)]
#[case::past_the_run(6, false)]
#[case::inside_the_run(4, true)]
fn a_clear_after_a_reparse_reads_the_last_validated_columns(
    #[case] offset: usize,
    #[case] cleared: bool,
) {
    let fixture = Fixture::new("chewing-reparse");
    let instance = fixture.instance;
    let parse = |text: &str| {
        let input = cstr(text);
        assert_eq!(
            unsafe { zhuyin_parse_more_chewings(instance, input.as_ptr()) },
            text.len()
        );
    };
    parse("su3cl3");
    let row = after_cursor_row(instance, 0, "你好");
    assert_eq!(unsafe { zhuyin_choose_candidate(instance, 0, row) }, 6);
    parse("su3");
    assert_eq!(
        unsafe { zhuyin_clear_constraint(instance, offset) },
        cleared
    );
}

/// A forcing that survives validation keeps its raw columns
/// (074a2219 `phonetic_lookup.cpp:120-163`): 郝 forced over [3, 6) of
/// `su3cl3` (ㄋㄧˇㄏㄠˇ) still spells ㄏㄠˇ over [3, 6) of `5;3cl3`
/// (ㄓㄤˇㄏㄠˇ), so the guess keeps it, the sentence holds it
/// (`zhuyin.cpp:895-910`) and a clear at 3 reaches its `ONESTEP` cell
/// (`zhuyin.cpp:1669-1676`, `phonetic_lookup.cpp:88-118`).
#[test]
fn a_forcing_keeps_its_columns_when_an_earlier_key_respells() {
    let fixture = Fixture::new("chewing-respell");
    let instance = fixture.instance;
    let parse = |text: &str| {
        let input = cstr(text);
        assert_eq!(
            unsafe { zhuyin_parse_more_chewings(instance, input.as_ptr()) },
            text.len()
        );
    };
    parse("su3cl3");
    let row = after_cursor_row(instance, 3, "郝");
    assert_eq!(unsafe { zhuyin_choose_candidate(instance, 3, row) }, 6);
    parse("5;3cl3");
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    let (found, text) = unsafe { crate::support::sentence(instance) };
    assert!(found);
    let text = text.expect("a sentence");
    assert!(text.ends_with('郝'), "{text}");
    assert!(unsafe { zhuyin_clear_constraint(instance, 3) });
}
