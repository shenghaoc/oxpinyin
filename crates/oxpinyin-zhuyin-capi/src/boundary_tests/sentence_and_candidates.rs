//! Sentence and candidate guesses, choosing a candidate and the sentence
//! step prices.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// `'ni` leaves column 0 empty under every full-pinyin scheme: the Luoma
/// and secondary-zhuyin index parses run the same parser
/// (`FullPinyinParser2::set_scheme`, `pinyin_parser2.cpp:383-401`), and
/// `fill_matrix` fills no zero key before the first key
/// (`phonetic_key_matrix.cpp:52-79`). The beam never leaves column 0
/// (`phonetic_lookup.h:753-771`), so the guess has no sentence.
#[rstest::rstest]
fn a_leading_separator_guess_has_no_sentence(#[values(1, 2, 3)] scheme: i32) {
    let (context, spare) = open();
    unsafe { zhuyin_free_instance(spare) };
    assert!(unsafe { crate::config::zhuyin_set_full_pinyin_scheme(context, scheme) });
    let instance = full_pinyin(context, SEPARATOR_OPTIONS, "'ni");
    assert_eq!(unsafe { zhuyin_get_parsed_input_length(instance) }, 3);
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert_eq!(sentence(instance), (false, None));
    unsafe { zhuyin_free_instance(instance) };
    unsafe { zhuyin_fini(context) };
}

/// A step prices a reading with the query's tone (`phonetic_key_matrix.cpp:
/// 562,575`, `phrase_index.h:136-164` at 074a2219): a nonzero query tone
/// matches an equal or zero stored tone only (`pinyin_phrase3.h:96-105`).
/// The second import of 甲 adds a reading and no index row
/// (`pinyin.cpp:563-582`). Under the default `USE_TONE` (`zhuyin.cpp:273`),
/// ㄅㄚˋ prices 甲 at 2/100, so 乙 (unigram 3) outranks 甲 (unigram 6).
#[test]
fn a_step_prices_readings_with_the_query_tone() {
    let user = TempUserDir::new("toned-step");
    let (context, instance) = open_with_user(&user.path);
    for (text, reading, count) in [("甲", "ㄅㄚˋ", 2), ("甲", "ㄅㄚˇ", 98), ("乙", "ㄅㄚˋ", 1)]
    {
        assert!(import(context, 1, text, reading.as_bytes(), count));
    }
    unsafe { zhuyin_parse_more_chewings(instance, cstr("184").as_ptr()) };
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert_eq!(sentence(instance), (true, Some("乙".to_owned())));
    close(context, instance);
}

/// A step prices a user item from its own readings
/// (`phonetic_lookup.h:646-656`, `phrase_index.h:136-164` at 074a2219).
/// Library 6's 甲 holds ㄅㄚˋ 3 and ㄉㄚˋ 9, so ㄅㄚˋ prices it at 3/12 and
/// 乙 (unigram 6, one reading) outranks 甲 (unigram 9).
#[test]
fn a_step_prices_a_user_item_from_its_readings() {
    let user = TempUserDir::new("user-item-step");
    let (context, instance) = open_with_user(&user.path);
    for (text, reading, count) in [("甲", "ㄅㄚˋ", 3), ("甲", "ㄉㄚˋ", 9), ("乙", "ㄅㄚˋ", 2)]
    {
        assert!(import(context, 6, text, reading.as_bytes(), count));
    }
    unsafe { zhuyin_parse_more_chewings(instance, cstr("184").as_ptr()) };
    assert!(unsafe { zhuyin_guess_sentence(instance) });
    assert_eq!(sentence(instance), (true, Some("乙".to_owned())));
    close(context, instance);
}
