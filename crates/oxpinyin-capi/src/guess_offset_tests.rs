//! The dbff264 lookup-offset law through the C ABI.
//!
//! ibus-libpinyin ≥ 1.16.1 passes `pinyin_guess_candidates` the raw begin
//! of the next key rest, which can sit one position past the zero-key `'`
//! separator run (ibus-libpinyin issue #570). The guess normalizes the
//! caller offset back across the run, refuses — never aborts — when the
//! run leads the input, and a choose at the caller offset still advances.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use std::ptr;

use crate::candidates::{pinyin_choose_candidate, pinyin_clear_constraint, pinyin_get_candidate};

use crate::config::pinyin_set_full_pinyin_scheme;

use crate::parse::pinyin_parse_more_full_pinyins;

use crate::sentence::pinyin_guess_candidates;

use crate::sentence::pinyin_guess_sentence;

use crate::state::instance_ref;

use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, open};

use crate::types::{LookupCandidate, PinyinInstance};

/// The current snapshot's candidate texts, in list order.
fn texts(instance: *mut PinyinInstance) -> Vec<String> {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_ref(instance) };
    inst.state
        .candidates
        .iter()
        .map(|c| c.text_str().expect("UTF-8 candidate").to_owned())
        .collect()
}

/// The snapshot position of the first candidate with `text`.
fn position_of(instance: *mut PinyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_ref(instance) };
    inst.state
        .candidates
        .iter()
        .position(|c| c.text_str() == Ok(text))
        .unwrap_or_else(|| panic!("{text:?} is offered"))
}

/// Borrowed pointer to the snapshot candidate at `index`.
fn candidate_at(instance: *mut PinyinInstance, index: usize) -> *mut LookupCandidate {
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(
        unsafe {
            pinyin_get_candidate(
                instance,
                u32::try_from(index).expect("small candidate index"),
                &raw mut cand,
            )
        },
        "candidate {index} exists"
    );
    assert!(!cand.is_null());
    cand
}

fn parse(instance: *mut PinyinInstance, input: &str) -> usize {
    let text = cstr(input);
    unsafe { pinyin_parse_more_full_pinyins(instance, text.as_ptr()) }
}

#[test]
fn a_guess_at_a_mid_syllable_offset_answers_the_empty_column() {
    // The pin's empty matrix column: no key of ni|hao starts at bytes
    // 1/3/4, so `pinyin_guess_candidates` there answers true — the parse
    // is non-empty, the empty-matrix refusal does not fire — with no
    // phrase rows (the suffix re-parse the engine once served must stay
    // gone), then the stored n-best row alone once a sentence lookup
    // ran. The syllable start at byte 2 keeps its window throughout
    // (measured against the pin: nihao@1/3/4 → true n=0 fresh, true n=1
    // 你好 after `pinyin_guess_sentence`; nihao@2 → the hao window).
    let user_dir = TempUserDir::new("guess-mid-syllable");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    assert_eq!(parse(instance, "nihao"), 5);
    for offset in [1usize, 3, 4] {
        assert!(
            unsafe { pinyin_guess_candidates(instance, offset, DEFAULT_SORT) },
            "offset {offset} answers true: the parse is non-empty"
        );
        assert_eq!(
            texts(instance),
            Vec::<String>::new(),
            "offset {offset}: no suffix re-parse rows at the empty column"
        );
    }
    assert!(unsafe { pinyin_guess_candidates(instance, 2, DEFAULT_SORT) });
    assert!(
        texts(instance).iter().any(|text| text == "\u{597d}"),
        "offset 2 keeps the hao window"
    );

    assert!(unsafe { pinyin_guess_sentence(instance) });
    for offset in [1usize, 3, 4] {
        assert!(
            unsafe { pinyin_guess_candidates(instance, offset, DEFAULT_SORT) },
            "offset {offset} still answers true after the sentence lookup"
        );
        // Only the n-best sentence rows appear at the empty column (no
        // suffix re-parse), 你好 first. The mini fixture, now read with
        // the chunk items' real counts, offers 你好 and the near-tie 你号
        // as rows 0 and 1 — the gfloat-trellis difference
        // (`compatibility-policy.md`, row 11) tips that tie at fixture scale; the
        // full-data nbest differential is identical.
        let rows = texts(instance);
        assert_eq!(rows.first().map(String::as_str), Some("\u{4f60}\u{597d}"));
        assert!(
            rows.iter().all(|text| text.chars().count() == 2),
            "offset {offset}: only two-character n-best rows: {rows:?}"
        );
    }

    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// A guess's start is every row's `m_begin` (`pinyin.cpp:2226-2252`), and
/// a choose forces from it (`add_constraint(m_begin, m_end, token)` and
/// the second validation, `:2582-2585`). In `ni'hao` the `'` at byte 2 is
/// a zero-key column of its own, so a guess at 3 forces 好 over `[3, 6)`
/// and a guess at 2 over `[2, 6)`; `pinyin_clear_constraint` answers false
/// for a free cell (`:2643-2649`, `phonetic_lookup.cpp:88-115`). The Luoma
/// and secondary-zhuyin index parses run the same parser over another
/// index (`FullPinyinParser2::set_scheme`, `pinyin_parser2.cpp:383-401`),
/// so `fill_matrix` gives their `'` the same column
/// (`phonetic_key_matrix.cpp:52-79`). Secondary zhuyin spells 好 `hau`.
#[rstest::rstest]
#[case::hanyu_guess_at_the_key(1, "ni'hao", 3, [false, true])]
#[case::hanyu_guess_at_the_separator(1, "ni'hao", 2, [true, true])]
#[case::luoma_guess_at_the_key(2, "ni'hao", 3, [false, true])]
#[case::luoma_guess_at_the_separator(2, "ni'hao", 2, [true, true])]
#[case::secondary_zhuyin_guess_at_the_key(3, "ni'hau", 3, [false, true])]
#[case::secondary_zhuyin_guess_at_the_separator(3, "ni'hau", 2, [true, true])]
fn a_guess_start_is_the_forcings_start_across_a_separator(
    #[case] scheme: std::ffi::c_int,
    #[case] input: &str,
    #[case] guess: usize,
    #[case] cleared: [bool; 2],
) {
    for (probe, expected) in [2, 3].into_iter().zip(cleared) {
        let user_dir = TempUserDir::new("separator-start");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert!(unsafe { pinyin_set_full_pinyin_scheme(context, scheme) });
        assert_eq!(parse(instance, input), 6);
        assert!(unsafe { pinyin_guess_candidates(instance, guess, DEFAULT_SORT) });
        let hao = candidate_at(instance, position_of(instance, "\u{597d}"));
        assert_eq!(unsafe { pinyin_choose_candidate(instance, guess, hao) }, 6);
        assert_eq!(
            unsafe { pinyin_clear_constraint(instance, probe) },
            expected,
            "scheme {scheme}, guess at {guess}, clear at {probe}"
        );
        unsafe { crate::instance::pinyin_free_instance(instance) };
        unsafe { crate::context::pinyin_fini(context) };
    }
}

/// `USE_DIVIDED_TABLE` (`pinyin.h`, bit 7).
const DIVIDED: u32 = 1 << 7;

/// The candidate texts a guess at `offset` answers for `input` under
/// `scheme`, with the divided table on and 安 imported under `an`.
fn divided_window(scheme: std::ffi::c_int, input: &str, offset: usize) -> Vec<String> {
    use crate::iterators::{
        pinyin_begin_add_phrases, pinyin_end_add_phrases, pinyin_iterator_add_phrase,
    };
    let user_dir = TempUserDir::new("divided-window");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let import = unsafe { pinyin_begin_add_phrases(context, 7) };
    assert!(unsafe {
        pinyin_iterator_add_phrase(import, cstr("\u{5b89}").as_ptr(), cstr("an").as_ptr(), 1)
    });
    unsafe { pinyin_end_add_phrases(import) };
    assert!(unsafe { crate::config::pinyin_set_options(context, 0x8 | DIVIDED) });
    assert!(unsafe { pinyin_set_full_pinyin_scheme(context, scheme) });
    assert_eq!(parse(instance, input), 4);
    assert!(unsafe { pinyin_guess_candidates(instance, offset, DEFAULT_SORT) });
    let window = texts(instance);
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
    window
}

/// The divided table gives Luoma `sian` (xian, `[0, 4)`) an `an` key at
/// column 2 (`inner_split_step`, `phonetic_key_matrix.cpp:169-228`; the
/// column is `index + strlen("xi")`, `:206`). A guess there searches that
/// column (`pinyin.cpp:2224-2262`): column 1 is empty, so `_check_offset`
/// (`:2163-2181`) passes. The window is Hanyu `xian`'s at 2, not the
/// empty list of a column inside a key.
#[test]
fn a_guess_at_a_divided_column_inside_a_luoma_key_searches_it() {
    let hanyu = divided_window(1, "xian", 2);
    assert!(
        hanyu.iter().any(|text| text == "\u{5b89}"),
        "the Hanyu divided column offers 安: {hanyu:?}"
    );
    assert_eq!(divided_window(2, "sian", 2), hanyu);
}
