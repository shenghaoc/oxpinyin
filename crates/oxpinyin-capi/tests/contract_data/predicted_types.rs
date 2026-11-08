//! Predicted-candidate type selection (#540, #542). Generated from the frozen pin observations.

use crate::bodies::predicted_types as b;
use crate::support::{Expect, NO_LOGS, run};

// choose-predicted-types: MATCH.
// A punctuation row has no frequency and answers true (pinyin.cpp:2603-2605); a prefix
// row trains its unigram and answers true (:2608-2617).
#[test]
fn choose_predicted_types() {
    run(
        "predicted_types::choose_predicted_types",
        Expect::RESULT,
        b::choose_predicted_types,
        include!("fixtures/predicted_types/b4636d5742858fe8.rs"),
        Some(NO_LOGS),
    );
}
