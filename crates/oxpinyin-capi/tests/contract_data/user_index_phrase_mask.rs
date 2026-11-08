//! The user index tables under mask_out and removal: keys left empty, the mask walk's tails, tone-variant readings of one token and predictions over faulty grams (#784, #785, #814, #1418). Generated from the frozen pin observations.

use crate::bodies::user_index_phrase_mask as b;
use crate::support::{Expect, run_aborting, run_j};

// abort-guess-predicted-empty-bigram-gram: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_predicted_empty_bigram_gram() {
    run_aborting(
        "user_index_phrase_mask::abort_guess_predicted_empty_bigram_gram",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-predicted-empty-bigram-gram");
        },
    );
}
// guess-predicted-unrelated-short-bigram-value: MATCH.
#[test]
fn guess_predicted_unrelated_short_bigram_value() {
    run_j(
        "user_index_phrase_mask::guess_predicted_unrelated_short_bigram_value",
        Expect::RESULT,
        |k| b::case(k, "guess-predicted-unrelated-short-bigram-value"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// issue1418-tone-variant-readings-pinyin: MATCH.
#[test]
fn issue1418_tone_variant_readings_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/f6a9966d2cccbdd2.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/5e6b11d23dbcf222.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue1418_tone_variant_readings_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue1418-tone-variant-readings-pinyin"),
        expected,
        None,
    );
}
// issue784-index-keys-mask-middle-pinyin: MATCH.
#[test]
fn issue784_index_keys_mask_middle_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/a8db168dcd9f43d8.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/b5d6646eb019c225.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_middle_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-middle-pinyin"),
        expected,
        None,
    );
}

// issue784-index-keys-mask-reopen-lookup-pinyin: MATCH.
#[test]
fn issue784_index_keys_mask_reopen_lookup_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/fe9948d31c926351.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/3934e7a6653165f2.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_reopen_lookup_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-reopen-lookup-pinyin"),
        expected,
        None,
    );
}
// issue784-index-keys-remove-middle: MATCH.
#[test]
fn issue784_index_keys_remove_middle() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/cf8d1c629a2dafd0.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/bc0d5c94907f4645.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_remove_middle",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-remove-middle"),
        expected,
        None,
    );
}
// issue785-pinyin-tails-hit-order-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_hit_order_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/1e850d0f64cb7b67.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/3f313ddf48cdfd2e.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_hit_order_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-hit-order-pinyin"),
        expected,
        None,
    );
}
// issue785-pinyin-tails-mask-all-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_mask_all_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/ecd025fae2f0a7fc.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/53e2ce15daf69f34.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_mask_all_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-mask-all-pinyin"),
        expected,
        None,
    );
}
