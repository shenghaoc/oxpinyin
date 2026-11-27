#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::iterators::{
    zhuyin_begin_add_phrases, zhuyin_end_add_phrases, zhuyin_iterator_add_phrase,
};

use crate::test_support::{TempUserDir, close, cstr, open_with_user, with_store};

use oxpinyin::facade::USER_DICTIONARY;

/// libzhuyin's import parses the reading with `ZhuyinDirectParser2` under
/// `USE_TONE | FORCE_TONE` (`zhuyin.cpp:515-523` at 074a2219). The parser splits on `' '` and
/// `'\''` and skips any run of them (`storage/zhuyin_parser2.cpp:914-960`), so an apostrophe and
/// a trailing space change nothing. A leading separator leaves an empty first key, which
/// `parse_one_key` refuses (`:866-867`), and an initial alone parses no key: the phrase's
/// length then differs from the key count, and the add answers false (`zhuyin.cpp:525`). Pin
/// Pin observations `zhuyin-import/run`,
/// `zhuyin-import/leading` and `zhuyin-import/initial` (the run's new library-7 token stored
/// `ㄋㄧˇ ㄏㄠˇ`; the others stored nothing).
#[test]
fn the_import_parser_skips_separator_runs_and_refuses_a_leading_one() {
    for (phrase, reading, added) in [
        ("你好", "ㄋㄧˇ' ㄏㄠˇ ", true),
        ("你好", " ㄋㄧˇ ㄏㄠˇ", false),
        ("你", "ㄋ", false),
    ] {
        let user_dir = TempUserDir::new("import-separators");
        let (context, instance) = open_with_user(&user_dir.path);
        let iter = unsafe { zhuyin_begin_add_phrases(context, USER_DICTIONARY) };
        assert!(!iter.is_null());
        assert_eq!(
            unsafe {
                zhuyin_iterator_add_phrase(iter, cstr(phrase).as_ptr(), cstr(reading).as_ptr(), 5)
            },
            added,
            "{reading:?}"
        );
        unsafe { zhuyin_end_add_phrases(iter) };
        // SAFETY: `instance` is live and the store borrow ends before close.
        let phrases = unsafe { with_store(instance, |store| store.phrases()) };
        let mut rows = phrases.iter().filter(|row| {
            oxpinyin::vocab::phrase_index_library_index(row.token()) == USER_DICTIONARY
        });
        if added {
            let row = rows.next().expect("stored phrase");
            assert_eq!(
                row.words(),
                phrase.chars().map(u32::from).collect::<Vec<_>>()
            );
            let [reading] = row.pronunciations() else {
                panic!("expected one stored reading");
            };
            let keys: Vec<_> = reading
                .keys()
                .iter()
                .map(|&key| oxpinyin::user::key_pinyin_string(key).expect("stored key"))
                .collect();
            assert_eq!(keys, ["ni3", "hao3"]);
            assert_eq!(reading.count(), 5);
        }
        assert!(rows.next().is_none(), "{reading:?}");
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        unsafe { close(context, instance) };
    }
}
