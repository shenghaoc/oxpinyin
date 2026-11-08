//! A token's unigram around an import and a constrained training (#808). Generated from the frozen pin observations.

use crate::bodies::case_helpers::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};

// token-unigram-user: MATCH.
#[test]
fn token_unigram_user() {
    run(
        "case_helpers::token_unigram_user",
        Expect::RESULT,
        b::token_unigram_user,
        UserUnigram {
            imported: true,
            token: "0x7000001".into(),
            before: (true, 27),
            add: true,
            after: (true, 34),
        },
        Some(NO_LOGS),
    );
}

// token-unigram-trained: MATCH.
// One constrained train adds 69 * 7 = 483 (phonetic_lookup.h:874-929).
#[test]
fn token_unigram_trained() {
    run(
        "case_helpers::token_unigram_trained",
        Expect::RESULT,
        b::token_unigram_trained,
        include!("fixtures/case_helpers/6004bfe4cd650615.rs"),
        Some(NO_LOGS),
    );
}
