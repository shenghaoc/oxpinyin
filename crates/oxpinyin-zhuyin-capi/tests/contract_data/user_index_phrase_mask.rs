//! The user index tables under mask_out and removal: keys left empty, the mask walk's tails, tone-variant readings of one token and predictions over faulty grams (#784, #785, #814, #1418). Generated from the frozen pin observations.

use crate::bodies::user_index_phrase_mask as b;
use crate::support::{Expect, run_j};
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
