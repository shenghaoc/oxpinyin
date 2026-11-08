//! The user index tables under mask_out and removal: keys left empty, the mask walk's tails, tone-variant readings of one token and predictions over faulty grams (#784, #785, #814, #1418). Generated from the frozen pin observations.

use crate::bodies::user_index_phrase_mask as b;
use crate::support::{Expect, run_j};

// issue784-index-keys-mask-lookup-zhuyin: MATCH.
#[test]
fn issue784_index_keys_mask_lookup_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/f48dc5d416137f9a.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/7572e4ad7d429f1e.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_lookup_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-lookup-zhuyin"),
        expected,
        None,
    );
}

// issue784-index-keys-mask-middle-zhuyin: MATCH.
#[test]
fn issue784_index_keys_mask_middle_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/04f8861470c4f911.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/19ea5adeedf3a59a.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_middle_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-middle-zhuyin"),
        expected,
        None,
    );
}

// issue784-index-keys-mask-reopen-lookup-zhuyin: MATCH.
#[test]
fn issue784_index_keys_mask_reopen_lookup_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/8b4f7bcc04ee93d9.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/1960420d5b6ffcc1.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_reopen_lookup_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-reopen-lookup-zhuyin"),
        expected,
        None,
    );
}

// issue784-index-keys-mask-reopen-zhuyin: MATCH.
#[test]
fn issue784_index_keys_mask_reopen_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/71ac11cb1f18c4dd.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/3dbbbf9e748bbfeb.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_reopen_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-reopen-zhuyin"),
        expected,
        None,
    );
}

// issue784-index-keys-mask-save-zhuyin: MATCH.
#[test]
fn issue784_index_keys_mask_save_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/54edde5c32e62236.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/ca323be18982e19a.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_save_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-save-zhuyin"),
        expected,
        None,
    );
}

// issue784-index-keys-no-mask-zhuyin: MATCH.
#[test]
fn issue784_index_keys_no_mask_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/7982d0c65eaf0138.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/6e920ac7eec74bf3.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_no_mask_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-no-mask-zhuyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-add-zhuyin: MATCH.
#[test]
fn issue785_pinyin_tails_add_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/11790d147b1af48e.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/302a58d520837799.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_add_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-add-zhuyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-hit-zhuyin: MATCH.
#[test]
fn issue785_pinyin_tails_hit_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/b0375445afc94cdc.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/9ec9abc40669e6a4.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_hit_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-hit-zhuyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-kept-entry-zhuyin: MATCH.
#[test]
fn issue785_pinyin_tails_kept_entry_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/6a71240f4ef93417.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/7e77af90d9891ff8.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_kept_entry_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-kept-entry-zhuyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-lookup-zhuyin: MATCH.
#[test]
fn issue785_pinyin_tails_lookup_zhuyin() {
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_lookup_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-lookup-zhuyin"),
        raw!(include!(
            "fixtures/user_index_phrase_mask/a4c4ed8d54f39c7a.rs"
        )),
        None,
    );
}

// issue785-pinyin-tails-reopen-zhuyin: MATCH.
#[test]
fn issue785_pinyin_tails_reopen_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/f8f418633f2297ad.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/14338cf35f680a18.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_reopen_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-reopen-zhuyin"),
        expected,
        None,
    );
}
