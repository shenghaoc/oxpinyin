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

// abort-guess-predicted-short-bigram-value: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_predicted_short_bigram_value() {
    run_aborting(
        "user_index_phrase_mask::abort_guess_predicted_short_bigram_value",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-predicted-short-bigram-value");
        },
    );
}

// guess-predicted-unrelated-empty-bigram-gram: MATCH.
#[test]
fn guess_predicted_unrelated_empty_bigram_gram() {
    run_j(
        "user_index_phrase_mask::guess_predicted_unrelated_empty_bigram_gram",
        Expect::RESULT,
        |k| b::case(k, "guess-predicted-unrelated-empty-bigram-gram"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
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

// issue784-index-keys-mask-all-pinyin: MATCH.
#[test]
fn issue784_index_keys_mask_all_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/fa4e3c8476cd634d.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/6190057eef574684.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_all_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-all-pinyin"),
        expected,
        None,
    );
}

// issue784-index-keys-mask-highest-pinyin: MATCH.
#[test]
fn issue784_index_keys_mask_highest_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/dc4882e559f026b6.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/913970dd05e0e1f5.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_highest_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-highest-pinyin"),
        expected,
        None,
    );
}

// issue784-index-keys-mask-lookup-pinyin: MATCH.
#[test]
fn issue784_index_keys_mask_lookup_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/46728e1169c9b122.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/78ccde129cc68623.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_lookup_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-lookup-pinyin"),
        expected,
        None,
    );
}

// issue784-index-keys-mask-middle-highest-pinyin: MATCH.
#[test]
fn issue784_index_keys_mask_middle_highest_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/af825ac72f2c1a06.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/291871d53dae91f7.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_middle_highest_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-middle-highest-pinyin"),
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

// issue784-index-keys-mask-reopen-pinyin: MATCH.
#[test]
fn issue784_index_keys_mask_reopen_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/670b7a4d447b6d6c.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/32ca41ede340b307.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_reopen_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-reopen-pinyin"),
        expected,
        None,
    );
}

// issue784-index-keys-mask-save-pinyin: MATCH.
#[test]
fn issue784_index_keys_mask_save_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/f061b4b7e0877e5e.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/9fc5caa81cf2f2e9.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_save_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-save-pinyin"),
        expected,
        None,
    );
}

// issue784-index-keys-mask-unsaved-pinyin: MATCH.
#[test]
fn issue784_index_keys_mask_unsaved_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/a73b03d88f3ce6a4.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/30427d5847523035.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_mask_unsaved_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-mask-unsaved-pinyin"),
        expected,
        None,
    );
}

// issue784-index-keys-no-mask-pinyin: MATCH.
#[test]
fn issue784_index_keys_no_mask_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/e72e5ae416790878.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/cebf8764562b305b.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_no_mask_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-no-mask-pinyin"),
        expected,
        None,
    );
}

// issue784-index-keys-remove-lookup: MATCH.
#[test]
fn issue784_index_keys_remove_lookup() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/77d28340885fec06.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/10ecc588de4489aa.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_remove_lookup",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-remove-lookup"),
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

// issue784-index-keys-remove-reopen: MATCH.
#[test]
fn issue784_index_keys_remove_reopen() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/941689be53f8e4ff.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/1988b7fefbe16505.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_remove_reopen",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-remove-reopen"),
        expected,
        None,
    );
}

// issue784-index-keys-remove-reopen-lookup: MATCH.
#[test]
fn issue784_index_keys_remove_reopen_lookup() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/c2ba720e85b8f92b.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/1182799c71de49cb.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_remove_reopen_lookup",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-remove-reopen-lookup"),
        expected,
        None,
    );
}

// issue784-index-keys-remove-save: MATCH.
#[test]
fn issue784_index_keys_remove_save() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/216750f0893371cc.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/eba166d45d88f6b6.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_remove_save",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-remove-save"),
        expected,
        None,
    );
}

// issue784-index-keys-remove-unsaved: MATCH.
#[test]
fn issue784_index_keys_remove_unsaved() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/d9e3812488d2ed60.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/1231ff2ce721aaf3.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue784_index_keys_remove_unsaved",
        Expect::RESULT,
        |k| b::case(k, "issue784-index-keys-remove-unsaved"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-add-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_add_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/eae190f6f539af83.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/20b41125391744a9.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_add_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-add-pinyin"),
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

// issue785-pinyin-tails-hit-order-reverse-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_hit_order_reverse_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/55b925e0b0a5244d.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/59fbb1f8de047a0b.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_hit_order_reverse_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-hit-order-reverse-pinyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-hit-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_hit_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/9a880d245e5dfb67.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/3f2e604ec617ac23.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_hit_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-hit-pinyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-hit-shrink-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_hit_shrink_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/671d1fe8ae18908b.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/15d5a6a8e33d9664.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_hit_shrink_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-hit-shrink-pinyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-hit-tailed-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_hit_tailed_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/6a7619c6c68b0472.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/09e08a33cd674ac2.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_hit_tailed_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-hit-tailed-pinyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-lookup-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_lookup_pinyin() {
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_lookup_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-lookup-pinyin"),
        raw!(include!(
            "fixtures/user_index_phrase_mask/0557ac845c182f4d.rs"
        )),
        None,
    );
}

// issue785-pinyin-tails-markers-hit-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_markers_hit_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/3b75d9ca58104dea.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/5f35bc616627dce3.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_markers_hit_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-markers-hit-pinyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-markers-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_markers_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/b9d89c7816c0a967.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/ac0287a791a0e29f.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_markers_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-markers-pinyin"),
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

// issue785-pinyin-tails-mask-none-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_mask_none_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/fff59edb11e3cb16.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/b2e4ff9263c6d0fa.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_mask_none_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-mask-none-pinyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-mask-twice-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_mask_twice_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/e70bbf03131c2529.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/b6d1f07006a93ede.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_mask_twice_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-mask-twice-pinyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-remove-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_remove_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/f9130ae8d53e80a3.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/71a6728147d3dc42.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_remove_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-remove-pinyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-reopen-mask-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_reopen_mask_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/e3ea86942ca79260.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/cc2697ae7d9adaab.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_reopen_mask_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-reopen-mask-pinyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-reopen-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_reopen_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/4a90b4ac3e61f7c3.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/521a1b23c41bc45c.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_reopen_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-reopen-pinyin"),
        expected,
        None,
    );
}

// issue785-pinyin-tails-suggestion-pinyin: MATCH.
#[test]
fn issue785_pinyin_tails_suggestion_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/b9574cf4f0cbeb07.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_index_phrase_mask/4a01a93fe38aef03.rs"
    ));
    run_j(
        "user_index_phrase_mask::issue785_pinyin_tails_suggestion_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue785-pinyin-tails-suggestion-pinyin"),
        expected,
        None,
    );
}
