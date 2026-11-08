//! Token reuse and addon import: the next token an import takes after mask_out frees the top of a library's range, and a promoted addon phrase's rows (#762, #913). Generated from the frozen pin observations.

use crate::bodies::token_reuse_addon_import as b;
use crate::support::{Expect, run_j};

// addon-promoted-listing-by-frequency: MATCH.
#[test]
fn addon_promoted_listing_by_frequency() {
    run_j(
        "token_reuse_addon_import::addon_promoted_listing_by_frequency",
        Expect::RESULT,
        |k| b::case(k, "addon-promoted-listing-by-frequency"),
        raw!(include!(
            "fixtures/token_reuse_addon_import/d52a1e8629f5f005.rs"
        )),
        None,
    );
}

// addon-promoted-listing-unsorted: MATCH.
#[test]
fn addon_promoted_listing_unsorted() {
    run_j(
        "token_reuse_addon_import::addon_promoted_listing_unsorted",
        Expect::RESULT,
        |k| b::case(k, "addon-promoted-listing-unsorted"),
        raw!(include!(
            "fixtures/token_reuse_addon_import/fa091dec730352f5.rs"
        )),
        None,
    );
}

// issue762-addon-import-mask-highest-pinyin: MATCH.
#[test]
fn issue762_addon_import_mask_highest_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/7e9764a7c152e1bd.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/b77950bd81b2a1fc.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_addon_import_mask_highest_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-addon-import-mask-highest-pinyin"),
        expected,
        None,
    );
}

// issue762-addon-import-mask-middle-pinyin: MATCH.
#[test]
fn issue762_addon_import_mask_middle_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/0106807b48393f9f.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/79ea5c94fcaef633.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_addon_import_mask_middle_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-addon-import-mask-middle-pinyin"),
        expected,
        None,
    );
}

// issue762-addon-import-no-mask-pinyin: MATCH.
#[test]
fn issue762_addon_import_no_mask_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/cfcb283b9a8caf89.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/2e6521330a8edf2d.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_addon_import_no_mask_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-addon-import-no-mask-pinyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-all-pinyin: MATCH.
#[test]
fn issue762_token_reuse_mask_all_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/56700a5b6cc31c64.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/b91d575120c144a9.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_all_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-all-pinyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-highest-pinyin: MATCH.
#[test]
fn issue762_token_reuse_mask_highest_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/b4d0eca69b761893.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/aaf0e7682b456047.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_highest_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-highest-pinyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-import-mask-pinyin: MATCH.
#[test]
fn issue762_token_reuse_mask_import_mask_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/61c91c7e8b55d1d0.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/54e91452244f4bed.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_import_mask_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-import-mask-pinyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-middle-pinyin: MATCH.
#[test]
fn issue762_token_reuse_mask_middle_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/574d80380bf7389d.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/9c913274e0025995.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_middle_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-middle-pinyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-reopen-middle-pinyin: MATCH.
#[test]
fn issue762_token_reuse_mask_reopen_middle_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/7b2143e6cbefeafa.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/9ddbf9e0d9ecd57e.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_reopen_middle_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-reopen-middle-pinyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-reopen-pinyin: MATCH.
#[test]
fn issue762_token_reuse_mask_reopen_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/8e6c17d63082a692.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/1af262f98f495aae.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_reopen_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-reopen-pinyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-twice-pinyin: MATCH.
#[test]
fn issue762_token_reuse_mask_twice_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/bae5a1baa31edf21.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/27ae776ede127242.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_twice_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-twice-pinyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-unsaved-reopen-pinyin: MATCH.
#[test]
fn issue762_token_reuse_mask_unsaved_reopen_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/c07714f57cffc8c5.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/dc117077973ca8c5.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_unsaved_reopen_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-unsaved-reopen-pinyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-no-mask-pinyin: MATCH.
#[test]
fn issue762_token_reuse_no_mask_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/1471a17b1354883f.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/091da69bc7c0b2e8.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_no_mask_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-no-mask-pinyin"),
        expected,
        None,
    );
}
