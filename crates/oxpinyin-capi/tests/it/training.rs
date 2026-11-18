//! Training and remember: the grams, the counts and the user indexes they
//! write.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{
    TempUserDir, cstr, lookup, pinyin_guess_sentence, pinyin_parse_full_pinyin, pinyin_set_options,
    system_dir, *,
};

/// `remember_user_input` keeps the toned keys of the parse even after the
/// options that made them are cleared.
#[test]
fn remember_keeps_the_parse_time_tones() {
    let user = TempUserDir::new("remember-cleared-tone");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_set_options(context, 0x20) });
    assert_eq!(parse(instance, "ni3hao3"), 7);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_set_options(context, 0) });
    assert!(unsafe { crate::support::pinyin_remember_user_input(instance, c"你好".as_ptr(), 3) });
    assert_eq!(
        crate::support::export_of(context, 7),
        [("你好".to_owned(), "ni3'hao3".to_owned(), 3)]
    );
    close(context, instance);
}

/// Remember stores the matrix's own key, with or without a syllable-table
/// row. Under `USE_TONE | PINYIN_AMB_EN_ENG`, `shun3` puts the final
/// alternate (sh, u, eng, tone 3) in the matrix with no table check
/// (`phonetic_key_matrix.cpp:285-306` at 074a2219). 甲's only reading is
/// that key, so the one matching path carries it to `_add_phrase`
/// (`pinyin.cpp:3594-3606`, `:3647-3663`), which answers true and stores
/// it. The nth-pronunciation getter returns the stored word
/// (`pinyin.cpp:2801-2818`). The key has no `content_table` row:
/// `get_pinyin_string` renders the empty row-zero spelling and the tone digit
/// (`chewing_key.cpp:47-57`, `pinyin_parser_table.h:3977`).
#[test]
fn remember_stores_a_fuzzy_final_alternate_with_no_table_row() {
    let key_user = TempUserDir::new("remember-alternate-keys");
    let (key_context, key_instance) = open_at(&key_user.path);
    let key = |text: &str| {
        let mut key = 0_u16;
        let text = cstr(text);
        assert!(unsafe {
            pinyin_parse_full_pinyin(key_instance, text.as_ptr(), (&raw mut key).cast())
        });
        key
    };
    // The final field is bits 7 to 11 of the packed key.
    let alternate = (key("shun3") & !0x0f80) | (key("sheng") & 0x0f80);
    close(key_context, key_instance);

    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::support::items_system(
        &system_dir(),
        scratch.path(),
        &[(&[0x7532], &[alternate], 100)],
        1000,
    );
    let user = TempUserDir::new("remember-alternate");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(unsafe {
        pinyin_set_options(
            context,
            (oxpinyin_core::OptionBits::USE_TONE | oxpinyin_core::OptionBits::PINYIN_AMB_EN_ENG)
                .bits(),
        )
    });
    assert_eq!(parse(instance, "shun3"), 5);
    let phrase = cstr("甲");
    assert!(unsafe { crate::support::pinyin_remember_user_input(instance, phrase.as_ptr(), 5) });
    let token = lookup(instance, "甲")
        .into_iter()
        .find(|&token| oxpinyin_core::user::is_user_token(token))
        .expect("the remembered library-7 token");
    assert_eq!(reading_bytes(instance, token, 0), alternate.to_le_bytes());
    assert_eq!(export_rows_of(context, 7, "甲"), [("3".to_owned(), 5)]);
    close(context, instance);
}

/// A negative remember count is stored as its bit pattern: `i32::MIN`
/// exports as itself.
#[test]
fn a_negative_remember_count_is_stored_as_its_bit_pattern() {
    let user = TempUserDir::new("remember-count-min");
    let (context, instance) = open_at(&user.path);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe {
        crate::support::pinyin_remember_user_input(instance, c"你好".as_ptr(), i32::MIN)
    });
    assert_eq!(
        crate::support::export_of(context, 7),
        [("你好".to_owned(), "ni'hao".to_owned(), i32::MIN)]
    );
    close(context, instance);
}
