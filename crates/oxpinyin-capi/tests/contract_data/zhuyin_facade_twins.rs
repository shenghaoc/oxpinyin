//! The zhuyin twins of the out-parameter and library-unload contracts (#542). Generated from the frozen pin observations.

use crate::bodies::zhuyin_facade_twins as b;
use crate::support::{Expect, run_j};

// user-library-token-unload-pinyin: MATCH.
#[test]
fn user_library_token_unload_pinyin() {
    run_j(
        "zhuyin_facade_twins::user_library_token_unload_pinyin",
        Expect::RESULT,
        |k| b::case(k, "user-library-token-unload-pinyin"),
        raw!(include!("fixtures/zhuyin_facade_twins/a75ea637a1a91169.rs")),
        None,
    );
}
