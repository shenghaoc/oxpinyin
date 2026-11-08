//! Token reuse and addon import: the next token an import takes after mask_out frees the top of a library's range, and a promoted addon phrase's rows (#762, #913). Generated from the frozen pin observations.

use crate::bodies::token_reuse_addon_import as b;
use crate::support::{Expect, run_j};

// issue762-addon-import-mask-highest-zhuyin: MATCH.
#[test]
fn issue762_addon_import_mask_highest_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/44456e7792a2ad07.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/3cf08baa6fc2af28.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_addon_import_mask_highest_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-addon-import-mask-highest-zhuyin"),
        expected,
        None,
    );
}

// issue762-addon-import-mask-middle-zhuyin: MATCH.
#[test]
fn issue762_addon_import_mask_middle_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/2636ee2a53c31d8c.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/c86e43c0b06de5fa.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_addon_import_mask_middle_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-addon-import-mask-middle-zhuyin"),
        expected,
        None,
    );
}

// issue762-addon-import-no-mask-zhuyin: MATCH.
#[test]
fn issue762_addon_import_no_mask_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/c06aa181182ef9e8.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/0221a365679549b9.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_addon_import_no_mask_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-addon-import-no-mask-zhuyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-all-zhuyin: MATCH.
#[test]
fn issue762_token_reuse_mask_all_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/14233a86a5c7f1e4.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/e061fd7acc57ecf6.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_all_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-all-zhuyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-highest-zhuyin: MATCH.
#[test]
fn issue762_token_reuse_mask_highest_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/35e6441822a0da9f.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/48fa0fe4b723f2a0.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_highest_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-highest-zhuyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-import-mask-zhuyin: MATCH.
#[test]
fn issue762_token_reuse_mask_import_mask_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/b187616431b16c27.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/a9cebf42a6bbd650.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_import_mask_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-import-mask-zhuyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-middle-zhuyin: MATCH.
#[test]
fn issue762_token_reuse_mask_middle_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/c1e2a58b4d4c171f.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/665a163dbeff388b.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_middle_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-middle-zhuyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-reopen-middle-zhuyin: MATCH.
#[test]
fn issue762_token_reuse_mask_reopen_middle_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/55b66f24e2bf86b0.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/ac06c05b3a8c1e9a.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_reopen_middle_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-reopen-middle-zhuyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-reopen-zhuyin: MATCH.
#[test]
fn issue762_token_reuse_mask_reopen_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/b9bf2794190a5e24.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/9a764301f2f54ba9.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_reopen_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-reopen-zhuyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-twice-zhuyin: MATCH.
#[test]
fn issue762_token_reuse_mask_twice_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/d3412b9116c99e61.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/28a7285f49fe75ac.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_twice_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-twice-zhuyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-mask-unsaved-reopen-zhuyin: MATCH.
#[test]
fn issue762_token_reuse_mask_unsaved_reopen_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/1a6415340c94d65a.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/1f52125c14642605.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_mask_unsaved_reopen_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-mask-unsaved-reopen-zhuyin"),
        expected,
        None,
    );
}

// issue762-token-reuse-no-mask-zhuyin: MATCH.
#[test]
fn issue762_token_reuse_no_mask_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/b5ec45c6848287b4.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/token_reuse_addon_import/e84622bae7179679.rs"
    ));
    run_j(
        "token_reuse_addon_import::issue762_token_reuse_no_mask_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue762-token-reuse-no-mask-zhuyin"),
        expected,
        None,
    );
}
