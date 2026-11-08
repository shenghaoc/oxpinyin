//! The import iterator's phrase-table asserts (#525, register rows 93 and 104).

use crate::facade;
use crate::fixtures::{craft_tree, ucs4_key, write_user_conf};
use crate::readers::{c, tokens_of};
use crate::support::Kit;

/// The reading of 你好 on this facade.
fn nihao() -> &'static str {
    if facade::ZHUYIN {
        "ㄋㄧˇ ㄏㄠˇ"
    } else {
        "ni'hao"
    }
}

/// Replaces the user phrase index with `你好 -> value`, reopens and imports 你好
/// into library 7: the pin asserts at `pinyin.cpp:554`, `:568` or `:571`
/// (`zhuyin.cpp:440`, `:454`, `:457`).
fn add_after_index(k: &mut Kit, value: &[u8], reading: &str) {
    craft_tree(
        &k.user.join("user_phrase_index.bin"),
        &[(ucs4_key("你好"), value.to_vec())],
    );
    write_user_conf(k);
    let ctx = k.init_default();
    assert!(!ctx.is_null(), "the reopen failed");
    let it = facade::begin_add_phrases(ctx, 7);
    facade::iterator_add_phrase(it, c("你好").as_ptr(), c(reading).as_ptr(), 1);
}

/// Imports and saves `text`, answering its new library-7 token.
fn persist_phrase(k: &mut Kit, text: &str, reading: &str) -> u32 {
    let ctx = k.ctx();
    let it = facade::begin_add_phrases(ctx, 7);
    assert!(
        facade::iterator_add_phrase(it, c(text).as_ptr(), c(reading).as_ptr(), 1),
        "the setup phrase did not add"
    );
    facade::end_add_phrases(it);
    assert!(facade::save(ctx));
    let token = tokens_of(k, text)[0];
    assert_eq!(token >> 24, 7, "the setup phrase is not new to library 7");
    facade::free_instance(k.inst());
    facade::fini(ctx);
    k.forget();
    token
}

/// Opens and closes the context once, so the profile exists.
fn first_init(k: &mut Kit) {
    let ctx = k.init_default();
    assert!(!ctx.is_null(), "the first init failed");
    facade::fini(ctx);
}

/// Two library-7 tokens for one phrase key.
pub fn duplicate_library_token(k: &mut Kit) {
    first_init(k);
    let value = [0x0700_0001_u32.to_le_bytes(), 0x0700_0002_u32.to_le_bytes()].concat();
    add_after_index(k, &value, nihao());
}

/// The token of a longer phrase.
pub fn index_length_mismatch(k: &mut Kit) {
    let (text, reading) = if facade::ZHUYIN {
        ("你好嗷", "ㄋㄧˇ ㄏㄠˇ ㄠˊ")
    } else {
        ("你好嗷", "ni'hao'ao")
    };
    let token = persist_phrase(k, text, reading);
    add_after_index(k, &token.to_le_bytes(), nihao());
}

/// A token with no phrase item.
pub fn index_item_missing(k: &mut Kit) {
    first_init(k);
    add_after_index(k, &0x0700_0001_u32.to_le_bytes(), nihao());
}

/// The token of another phrase of the same length.
pub fn index_text_mismatch(k: &mut Kit) {
    let (text, reading) = if facade::ZHUYIN {
        ("你們", "ㄋㄧˇ ㄇㄣˊ")
    } else {
        ("你們", "ni'men")
    };
    let token = persist_phrase(k, text, reading);
    add_after_index(k, &token.to_le_bytes(), nihao());
}
