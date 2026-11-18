//! Sentence guesses, `pinyin_get_sentence`, sentence prefixes, n-best rows
//! and the sentence step prices.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{PARITY, init_fixture_at};

use crate::support::{
    DEFAULT_SORT, PinyinInstance, TempUserDir, assert_aborts, capture_rows, cstr, open,
    pinyin_alloc_instance, pinyin_choose_candidate, pinyin_fini, pinyin_free_instance,
    pinyin_get_candidate_nbest_index, pinyin_get_sentence, pinyin_guess_candidates,
    pinyin_guess_sentence, pinyin_parse_more_full_pinyins, pinyin_reset, pinyin_set_options,
    system_dir, *,
};

use oxpinyin::user::SENTENCE_START;

use std::ffi::c_char;

use std::ptr;

/// GLib's historical UTF-8 form of `words`, up to six bytes per word.
fn glib_utf8(words: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    for &word in words {
        let (first, length): (u32, usize) = match word {
            0..0x80 => (0, 1),
            0x80..0x800 => (0xc0, 2),
            0x800..0x1_0000 => (0xe0, 3),
            0x1_0000..0x20_0000 => (0xf0, 4),
            0x20_0000..0x400_0000 => (0xf8, 5),
            _ => (0xfc, 6),
        };
        let mut rest = word;
        let mut tail = Vec::new();
        for _ in 1..length {
            tail.push(0x80 | u8::try_from(rest & 0x3f).expect("six bits"));
            rest >>= 6;
        }
        out.push(u8::try_from(first | rest).expect("a lead byte"));
        out.extend(tail.iter().rev());
    }
    out
}

/// A toned initial aborts the sentence guess.
#[test]
fn guess_sentence_aborts_on_a_toned_initial() {
    assert_aborts("sentence::guess_sentence_aborts_on_a_toned_initial", || {
        let user = TempUserDir::new("toned-sentence");
        let (context, instance) = open_at(&user.path);
        assert!(unsafe { pinyin_set_options(context, 0x28) });
        assert_eq!(parse(instance, "n4"), 2);
        unsafe { pinyin_guess_sentence(instance) };
    });
}

/// `get_sentence` asserts on an index past the n-best rows.
#[test]
fn get_sentence_aborts_past_the_rows() {
    assert_aborts("sentence::get_sentence_aborts_past_the_rows", || {
        let user = TempUserDir::new("sentence-past-rows");
        let (_context, instance) = open_at(&user.path);
        assert_eq!(parse(instance, "nihao"), 5);
        assert!(unsafe { pinyin_guess_sentence(instance) });
        let mut out: *mut c_char = ptr::null_mut();
        unsafe { pinyin_get_sentence(instance, 5, &raw mut out) };
    });
}

/// A guess over a leading separator answers true with no n-best row, so
/// `get_sentence(0)` answers false and leaves the out-pointer alone.
#[test]
fn a_leading_separator_guess_has_no_sentence() {
    let user = TempUserDir::new("separator-sentence");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_set_options(context, SEPARATOR_OPTIONS) });
    assert_eq!(parse(instance, "'ni'hao"), 7);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence_out(instance, 0), (false, None));
    close(context, instance);
}

/// A keyless parse (one separator) refuses both sentence guesses before
/// they clear the old result: the old sentence stays, the candidate list
/// is empty, and a fresh instance has no sentence (:183).
#[test]
fn keyless_sentence_guesses_keep_the_old_sentence() {
    let user = TempUserDir::new("zero-key-guess");
    let (context, _) = open_at(&user.path);
    let guesses: [fn(*mut PinyinInstance) -> bool; 2] = [
        |instance| unsafe { pinyin_guess_sentence(instance) },
        |instance| unsafe {
            crate::support::pinyin_guess_sentence_with_prefix(instance, c"你".as_ptr())
        },
    ];
    for guess in guesses {
        let instance = alloc(context);
        assert_eq!(parse(instance, "ni3hao3"), 7);
        assert!(unsafe { pinyin_guess_sentence(instance) });
        let sentence = sentence_out(instance, 0);
        assert!(sentence.0);
        assert_eq!(parse(instance, "'"), 1);
        assert!(!guess(instance));
        assert_eq!(sentence_out(instance, 0), sentence);
        assert_eq!(n_candidate(instance), 0);
        unsafe { pinyin_free_instance(instance) };

        let fresh = alloc(context);
        assert_eq!(parse(fresh, "'"), 1);
        assert!(!guess(fresh));
        assert_eq!(sentence_out(fresh, 0), (false, None));
        unsafe { pinyin_free_instance(fresh) };
    }
    unsafe { pinyin_fini(context) };
}

/// A NULL prefix to `guess_sentence_with_prefix` is no prefix: true, and the
/// sentence is the unigram choice 丙 (docs/findings/upstream-divergences.md,
/// "NULL prediction prefix").
#[test]
fn a_null_sentence_prefix_is_no_prefix() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = prefix_system(scratch.path(), 0x4e01);
    let user = TempUserDir::new("prefix-null");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    assert_eq!(parse(instance, "ba1"), 3);
    let valid = std::ffi::CString::new(glib_utf8(&[0x7532, 0x4e01, 0x4e59])).expect("no NUL");
    assert!(unsafe { crate::support::pinyin_guess_sentence_with_prefix(instance, valid.as_ptr()) });
    assert!(unsafe { crate::support::pinyin_guess_sentence_with_prefix(instance, ptr::null()) });
    assert_eq!(sentence_out(instance, 0), (true, Some("丙".to_owned())));
    close(context, instance);
}

/// The sentence prefix is converted with `prefix_text`, which accepts a
/// word above Unicode: the prefix 甲·乙 finds its bigram and the sentence
/// is 戌.
#[test]
fn a_sentence_prefix_above_unicode_is_converted() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = prefix_system(scratch.path(), 0x11_0000);
    let user = TempUserDir::new("prefix-above-unicode");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    assert_eq!(parse(instance, "ba1"), 3);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    let prefix = std::ffi::CString::new(glib_utf8(&[0x7532, 0x11_0000, 0x4e59])).expect("no NUL");
    assert!(unsafe {
        crate::support::pinyin_guess_sentence_with_prefix(instance, prefix.as_ptr())
    });
    assert_eq!(sentence_out(instance, 0), (true, Some("戌".to_owned())));
    close(context, instance);
}

/// An n-best row whose phrases all start with a zero word has the string
/// `""` at the pin (`lookup/lookup.cpp:39-68`): `pinyin_get_sentence`
/// answers true with it, and the NBEST row outranks the NORMAL row of the
/// same string in the dedup (`pinyin.cpp:2116-2125`, `:2295-2296`).
#[test]
fn an_nbest_row_with_empty_text_is_a_sentence_and_outranks_its_phrase() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0, 0x4e19], &[0x1081; 2], 100)],
        2000,
    );
    let user = TempUserDir::new("nbest-empty-text");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    assert_eq!(parse(instance, "ba1ba1"), 6);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence_out(instance, 0), (true, Some(String::new())));
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    assert_eq!(capture_rows(instance), [(NBEST, String::new(), Some(0))]);
    close(context, instance);
}

/// `convert_to_utf8` leaves out a phrase whose `g_ucs4_to_utf8` is NULL, a
/// word at or above 0x80000000 (`lookup/lookup.cpp:58-65`), so the row
/// [A, 丙] reads `丙` and `pinyin_get_sentence` answers true. A's own NORMAL
/// row has a NULL string, which the candidate dedup compares (rows 80 and
/// 106, class (b)), so the test lists no candidates.
#[test]
fn an_nbest_row_leaves_out_a_phrase_that_does_not_convert() {
    let scratch = tempfile::tempdir().expect("scratch");
    let an = oxpinyin::ChewingKey::from_pinyin("an")
        .expect("frozen syllable")
        .with_tone(1)
        .to_packed();
    let system = crate::support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0x8000_0000], &[0x1081], 100), (&[0x4e19], &[an], 100)],
        2000,
    );
    let user = TempUserDir::new("nbest-unconvertible");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    assert_eq!(parse(instance, "ba1an1"), 6);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence_out(instance, 0), (true, Some("丙".to_owned())));
    close(context, instance);
}

/// A failed conversion for the row's only phrase leaves the sentence NULL
/// and still answers true (074a2219 `lookup/lookup.cpp:27-70`,
/// `pinyin.cpp:1464-1482`).
#[test]
fn an_nbest_row_with_no_converted_phrase_answers_true_with_null() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0x8000_0000], &[0x1081], 100)],
        2000,
    );
    let user = TempUserDir::new("nbest-null-text");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(unsafe { pinyin_set_options(context, oxpinyin::OptionBits::USE_TONE.bits()) });
    assert_eq!(parse(instance, "ba1"), 3);
    assert!(unsafe { pinyin_guess_sentence(instance) });

    let mut out = UNTOUCHED as usize as *mut c_char;
    assert!(unsafe { pinyin_get_sentence(instance, 0, &raw mut out) });
    assert!(out.is_null());
    close(context, instance);
}

/// NBEST choose reaches diff_result without validation (pinyin.cpp:2515-2520).
/// An empty-matrix guess shrinks the store but keeps the decoded results
/// (phonetic_lookup.cpp:120-138; phonetic_lookup.h:746-754). The differing
/// write then returns zero and check_result aborts (phonetic_lookup.cpp:203).
#[test]
fn a_kept_differing_sentence_aborts_after_an_empty_matrix_guess() {
    assert_aborts(
        "sentence::a_kept_differing_sentence_aborts_after_an_empty_matrix_guess",
        || {
            let user = TempUserDir::new("empty-matrix-diff-write");
            let (context, instance) = open_at(&user.path);
            assert_eq!(parse(instance, "nihao"), 5);
            assert!(unsafe { pinyin_guess_sentence(instance) });
            assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
            let rows = capture_rows(instance);
            let index = rows
                .iter()
                .enumerate()
                .find_map(|(index, (kind, _, _))| {
                    if *kind != NBEST {
                        return None;
                    }
                    let row = candidate_at(instance, index);
                    let mut rank = 0;
                    assert!(unsafe {
                        pinyin_get_candidate_nbest_index(instance, row, &raw mut rank)
                    });
                    (rank > 0).then_some(index)
                })
                .expect("a differing n-best row");
            let row = candidate_at(instance, index);
            assert_eq!(parse(instance, ""), 0);
            assert!(!unsafe { pinyin_guess_sentence(instance) });
            unsafe { pinyin_choose_candidate(instance, 0, row) };
            close(context, instance);
        },
    );
}

fn parity_instance(
    tag: &str,
) -> (
    *mut crate::support::PinyinContext,
    *mut crate::support::PinyinInstance,
    TempUserDir,
) {
    let user_dir = TempUserDir::new(tag);
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(unsafe { pinyin_set_options(context, PARITY) });
    (context, instance, user_dir)
}

fn character_offset(
    instance: *mut crate::support::PinyinInstance,
    phrase: &str,
    offset: usize,
) -> (bool, usize) {
    let phrase = cstr(phrase);
    let mut length = usize::MAX;
    let ok = unsafe {
        crate::support::pinyin_get_character_offset(
            instance,
            phrase.as_ptr(),
            offset,
            &raw mut length,
        )
    };
    (ok, length)
}

/// Issue #356's reproducer: the pinyin string itself as the phrase.
/// Both pinned oracles answer `false` at offsets 0-2 (the
/// one-character phrase-table search finds no token for `n`, `i` or
/// `'`), with the out-param untouched. At 3 and 4 they abort
/// (`_check_offset` on the separator's zero key, then the range
/// assert), and so does the facade: `abort-get-character-offset-*` of
/// the recorded contract diff harness hold those shapes.
#[test]
fn a_non_hanzi_phrase_answers_false_before_the_separator() {
    let (context, instance, _user_dir) = parity_instance("char-offset-non-hanzi");
    let input = cstr("ni'");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        3
    );

    for offset in 0..=2 {
        assert_eq!(
            character_offset(instance, "ni'", offset),
            (false, usize::MAX),
            "offset {offset}"
        );
    }

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// The consumer's shape (`PYPPinyinEditor.cc:290`): the guessed
/// sentence as the phrase, the lookup offset from the cursor. `nihao`
/// against 你好 counts one character per key whose raw end is at or
/// before the offset: 0 at the start, 1 past `ni`, 2 past `hao`.
///
/// `你`@5 walks past the phrase's one token: pin behaviour reproduced.
/// `cached_tokens` is zero-terminated (`g_array_new(TRUE, TRUE, …)`,
/// `pinyin.cpp:3219`), so the pin's unchecked read at index == length
/// (`:3167-3168`) lands on the zero terminator. Token 0 finds no item,
/// and the pin answers `false` with the length the walk reached
/// (`*plength = length`, `:3238`).
#[test]
fn counts_the_sentence_characters_up_to_the_offset() {
    let (context, instance, _user_dir) = parity_instance("char-offset-nihao");
    let input = cstr("nihao");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        5
    );

    for (offset, expected) in [(0, 0), (2, 1), (5, 2)] {
        assert_eq!(
            character_offset(instance, "你好", offset),
            (true, expected),
            "offset {offset}"
        );
    }
    // A phrase whose characters do not pronounce the keys: no path.
    // The walk's own `false` stores the length it reached, 0
    // (`*plength = length`, `pinyin.cpp:3238`).
    assert_eq!(character_offset(instance, "好你", 2), (false, 0));
    // A walk beyond the phrase's tokens: the pin's miss at the terminator.
    assert_eq!(character_offset(instance, "你", 5), (false, 0));
    // An empty phrase and a NULL phrase are the pin's `false`.
    assert_eq!(character_offset(instance, "", 0), (false, usize::MAX));
    let mut length = usize::MAX;
    assert!(!unsafe {
        crate::support::pinyin_get_character_offset(instance, std::ptr::null(), 0, &raw mut length)
    });
    assert_eq!(length, usize::MAX);

    let separated = cstr("ni'hao");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, separated.as_ptr()) },
        6
    );
    // `pinyin.cpp:3158-3163` steps over the separator and tail zero keys.
    assert_eq!(character_offset(instance, "你好", 6), (true, 2));

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// No parse yet: the pin's `0 == matrix.size()` false.
#[test]
fn an_empty_matrix_answers_false() {
    let (context, instance, _user_dir) = parity_instance("char-offset-empty");
    assert_eq!(character_offset(instance, "你", 0), (false, usize::MAX));
    for (input, consumed) in [("!", 0), ("'", 1)] {
        let input = cstr(input);
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
            consumed
        );
        assert_eq!(character_offset(instance, "你", 0), (false, usize::MAX));
    }
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// Parsing reads no gram. With DYNAMIC_ADJUST, a guess at offset zero
/// merges the sentence-start gram and aborts on its short total (074a2219
/// pinyin.cpp:1497-1520,1720-1723,2200-2215; ngram.cpp:48-51,303-304;
/// memory_chunk.h:390; register row 85).
#[test]
fn a_short_sentence_start_gram_aborts_the_guess_after_parsing() {
    use std::io::Write as _;

    const PARSED: &str = "row 85: full pinyin parse returned";
    let stderr = crate::support::assert_aborts(
        "sentence::a_short_sentence_start_gram_aborts_the_guess_after_parsing",
        || {
            let user = TempUserDir::new("short-start-gram");
            crate::support::plant_bigram(
                &user.path,
                vec![(SENTENCE_START.to_le_bytes().to_vec(), vec![7, 0, 0])],
            );
            let context = init_fixture_at(&user.path);
            assert!(!context.is_null());
            assert!(unsafe {
                pinyin_set_options(context, oxpinyin::OptionBits::DYNAMIC_ADJUST.bits())
            });
            let instance = unsafe { pinyin_alloc_instance(context) };
            assert!(!instance.is_null());
            assert_eq!(
                unsafe { pinyin_parse_more_full_pinyins(instance, c"nihao".as_ptr()) },
                5
            );
            writeln!(std::io::stderr().lock(), "{PARSED}").expect("write parse marker");
            let _ = unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) };
        },
    );
    assert!(
        String::from_utf8_lossy(&stderr)
            .lines()
            .any(|line| line == PARSED),
        "the child aborted before parsing returned: {}",
        String::from_utf8_lossy(&stderr)
    );
}

/// Before a guess, and after a reset, `get_sentence` answers false and
/// leaves the out-pointer alone.
#[test]
fn get_sentence_before_a_guess_leaves_the_out_pointer() {
    let user = TempUserDir::new("sentence-before-guess");
    let (context, instance) = open_at(&user.path);
    for index in [0, 255] {
        assert_eq!(
            sentence_out(instance, index),
            (false, None),
            "fresh {index}"
        );
    }
    assert_eq!(parse(instance, "nihao"), 5);
    for index in [0, 255] {
        assert_eq!(
            sentence_out(instance, index),
            (false, None),
            "parsed {index}"
        );
    }
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert_eq!(sentence_out(instance, 0), (true, Some("你好".to_owned())));
    assert!(unsafe { pinyin_reset(instance) });
    assert_eq!(sentence_out(instance, 0), (false, None));
    close(context, instance);
}
