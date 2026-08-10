//! Smoke test for the phrase-export and token-query seam.
//!
//! Pins the observable shape of the public export ABI this crate wraps:
//! library sizes, a known tuple, and the token mapping round-trip. The
//! full table verification lives in `export_verification.rs`.

#![cfg(feature = "oracle-ffi")]

use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};

#[test]
fn export_and_token_seam_round_trip() {
    let prefix = OraclePrefix::locate().expect("oracle prefix is present");
    let mut oracle = Oracle::open_with_temp_user_dir(prefix).expect("context opens");

    // Library 1 (gb_char) carries the bulk of the model.
    let gb = oracle.export_phrases(1).expect("gb_char exports");
    assert!(
        gb.len() > 90_000,
        "gb_char export unexpectedly small: {}",
        gb.len()
    );

    let nihao = gb
        .iter()
        .find(|tuple| tuple.phrase == "你好" && tuple.pinyin == "ni'hao")
        .expect("你好 | ni'hao is exported from gb_char");
    assert!(nihao.count > 0, "你好 carries a positive count");

    // The oracle's own text → token → text mapping closes the loop.
    let mut session = oracle.session(OracleFlags::DEFAULT).expect("session");
    let tokens = session.lookup_tokens("你好").expect("lookup_tokens");
    assert!(!tokens.is_empty(), "你好 resolves to at least one token");
    let gb_token = tokens
        .iter()
        .copied()
        .find(|token| token >> 24 == 1)
        .expect("你好 has a gb_char token");
    let text = session
        .token_text(gb_token)
        .expect("token_text")
        .expect("gb_char token resolves");
    assert_eq!(text, "你好");

    // A token the model cannot hold resolves to nothing rather than failing.
    let none = session.token_text(0x00ff_ffff).expect("token_text on junk");
    assert!(none.is_none(), "junk token resolves to no phrase");
}
