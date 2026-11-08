//! Stored pronunciation-count headers (#882, #1549). Generated from the frozen pin observations.

use crate::bodies::stored_count_headers::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};

// stored-count-headers-system-zhuyin: MATCH.
// Library 1 planted through the SYSTEM_FILE branch; the probe is phrase_index.cpp:180-205.
#[test]
fn stored_count_headers_system_zhuyin() {
    run(
        "stored_count_headers::stored_count_headers_system_zhuyin",
        Expect::RESULT,
        |k| b::stored_count_headers(k, false),
        include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/stored_count_headers/ca56425e717cde50.rs"
        ),
        Some(NO_LOGS),
    );
}

// stored-count-headers-user-zhuyin: MATCH.
// Library 7 planted as user.bin, the USER_FILE branch.
#[test]
fn stored_count_headers_user_zhuyin() {
    run(
        "stored_count_headers::stored_count_headers_user_zhuyin",
        Expect::RESULT,
        |k| b::stored_count_headers(k, true),
        include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/stored_count_headers/ca56425e717cde50.rs"
        ),
        Some(NO_LOGS),
    );
}

// untouched-truncated-count-zhuyin: MATCH.
// No length check past the header (phrase_index.cpp:203-205).
#[test]
fn untouched_truncated_count_zhuyin() {
    run(
        "stored_count_headers::untouched_truncated_count_zhuyin",
        Expect::RESULT,
        b::untouched_truncated,
        Truncated { count: (true, 17) },
        Some(NO_LOGS),
    );
}
