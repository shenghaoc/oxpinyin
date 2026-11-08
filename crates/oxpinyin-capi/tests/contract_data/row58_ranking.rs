//! Predicted-candidate ranking under seeded bigrams, PREFIX choices, large counts, wrapped totals and lambda edits (#1175, #697, #712). Generated from the frozen pin observations.

use crate::bodies::row58_ranking as b;
use crate::support::{Expect, run_aborting, run_j};

// row58-bigram-choices-off: MATCH.
#[test]
fn row58_bigram_choices_off() {
    run_j(
        "row58_ranking::row58_bigram_choices_off",
        Expect::RESULT,
        |k| b::case(k, "row58-bigram-choices-off"),
        raw!(include!("fixtures/row58_ranking/73af2a816626e17a.rs")),
        None,
    );
}

// row58-bigram-choices-on: MATCH.
#[test]
fn row58_bigram_choices_on() {
    run_j(
        "row58_ranking::row58_bigram_choices_on",
        Expect::RESULT,
        |k| b::case(k, "row58-bigram-choices-on"),
        raw!(include!("fixtures/row58_ranking/73af2a816626e17a.rs")),
        None,
    );
}

// row58-bigram-scoring-off: MATCH.
#[test]
fn row58_bigram_scoring_off() {
    run_j(
        "row58_ranking::row58_bigram_scoring_off",
        Expect::RESULT,
        |k| b::case(k, "row58-bigram-scoring-off"),
        raw!(include!("fixtures/row58_ranking/7f9891f973418b3b.rs")),
        None,
    );
}

// row58-bigram-scoring-on: MATCH.
#[test]
fn row58_bigram_scoring_on() {
    run_j(
        "row58_ranking::row58_bigram_scoring_on",
        Expect::RESULT,
        |k| b::case(k, "row58-bigram-scoring-on"),
        raw!(include!("fixtures/row58_ranking/7f9891f973418b3b.rs")),
        None,
    );
}

// row58-conversions-on: MATCH.
#[test]
fn row58_conversions_on() {
    run_j(
        "row58_ranking::row58_conversions_on",
        Expect::RESULT,
        |k| b::case(k, "row58-conversions-on"),
        raw!(include!("fixtures/row58_ranking/b40be006f22a64a2.rs")),
        None,
    );
}

// row58-lambda-rounding-on: MATCH.
#[test]
fn row58_lambda_rounding_on() {
    run_j(
        "row58_ranking::row58_lambda_rounding_on",
        Expect::RESULT,
        |k| b::case(k, "row58-lambda-rounding-on"),
        raw!(include!("fixtures/row58_ranking/314900fb2647fe98.rs")),
        None,
    );
}

// row58-lambda-ties-on: MATCH.
#[test]
fn row58_lambda_ties_on() {
    run_j(
        "row58_ranking::row58_lambda_ties_on",
        Expect::RESULT,
        |k| b::case(k, "row58-lambda-ties-on"),
        raw!(include!("fixtures/row58_ranking/d6c3c7afe9528f54.rs")),
        None,
    );
}

// row58-prefix-live-21-on: MATCH.
#[test]
fn row58_prefix_live_21_on() {
    run_j(
        "row58_ranking::row58_prefix_live_21_on",
        Expect::RESULT,
        |k| b::case(k, "row58-prefix-live-21-on"),
        raw!(include!("fixtures/row58_ranking/2c2fbb42885a62ec.rs")),
        None,
    );
}

// row58-prefix-no-bigram-on: MATCH.
#[test]
fn row58_prefix_no_bigram_on() {
    run_j(
        "row58_ranking::row58_prefix_no_bigram_on",
        Expect::RESULT,
        |k| b::case(k, "row58-prefix-no-bigram-on"),
        raw!(include!("fixtures/row58_ranking/4b6086dfa9231fbc.rs")),
        None,
    );
}

// row58-prefix-overflow-on: MATCH.
#[test]
fn row58_prefix_overflow_on() {
    run_j(
        "row58_ranking::row58_prefix_overflow_on",
        Expect::RESULT,
        |k| b::case(k, "row58-prefix-overflow-on"),
        raw!(include!("fixtures/row58_ranking/e2c0df461ddf3b9f.rs")),
        None,
    );
}

// row58-prefix-zero-total-on: MATCH.
#[test]
fn row58_prefix_zero_total_on() {
    run_j(
        "row58_ranking::row58_prefix_zero_total_on",
        Expect::RESULT,
        |k| b::case(k, "row58-prefix-zero-total-on"),
        raw!(include!("fixtures/row58_ranking/5379686af1e455ed.rs")),
        None,
    );
}

// row58-signed-duplicate-on: MATCH.
#[test]
fn row58_signed_duplicate_on() {
    run_j(
        "row58_ranking::row58_signed_duplicate_on",
        Expect::RESULT,
        |k| b::case(k, "row58-signed-duplicate-on"),
        raw!(include!("fixtures/row58_ranking/89020871f850b8eb.rs")),
        None,
    );
}

// row58-signed-score-on: MATCH.
#[test]
fn row58_signed_score_on() {
    run_j(
        "row58_ranking::row58_signed_score_on",
        Expect::RESULT,
        |k| b::case(k, "row58-signed-score-on"),
        raw!(include!("fixtures/row58_ranking/e1dd6e265b618b37.rs")),
        None,
    );
}

// row58-system-existing-on: MATCH.
#[test]
fn row58_system_existing_on() {
    run_j(
        "row58_ranking::row58_system_existing_on",
        Expect::RESULT,
        |k| b::case(k, "row58-system-existing-on"),
        raw!(include!("fixtures/row58_ranking/a72651a2e6585892.rs")),
        None,
    );
}

// row58-system-import-1-on: MATCH.
#[test]
fn row58_system_import_1_on() {
    run_j(
        "row58_ranking::row58_system_import_1_on",
        Expect::RESULT,
        |k| b::case(k, "row58-system-import-1-on"),
        raw!(include!("fixtures/row58_ranking/b292f2370e12a687.rs")),
        None,
    );
}

// row58-system-import-2-on: MATCH.
#[test]
fn row58_system_import_2_on() {
    run_j(
        "row58_ranking::row58_system_import_2_on",
        Expect::RESULT,
        |k| b::case(k, "row58-system-import-2-on"),
        raw!(include!("fixtures/row58_ranking/f3e4732759b61a35.rs")),
        None,
    );
}

// row58-system-import-3-on: MATCH.
#[test]
fn row58_system_import_3_on() {
    run_j(
        "row58_ranking::row58_system_import_3_on",
        Expect::RESULT,
        |k| b::case(k, "row58-system-import-3-on"),
        raw!(include!("fixtures/row58_ranking/93d8789574c09459.rs")),
        None,
    );
}

// row58-system-import-4-on: MATCH.
#[test]
fn row58_system_import_4_on() {
    run_j(
        "row58_ranking::row58_system_import_4_on",
        Expect::RESULT,
        |k| b::case(k, "row58-system-import-4-on"),
        raw!(include!("fixtures/row58_ranking/72c0a98c9ad6e628.rs")),
        None,
    );
}

// row58-total-zero-on: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn row58_total_zero_on() {
    run_aborting(
        "row58_ranking::row58_total_zero_on",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "row58-total-zero-on");
        },
    );
}

// row58-total-zero-punct-on: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn row58_total_zero_punct_on() {
    run_aborting(
        "row58_ranking::row58_total_zero_punct_on",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "row58-total-zero-punct-on");
        },
    );
}

// row58-unsigned-score-off: POLICY-ACCEPTED (arm64-pin-portability-1137.md).
#[test]
fn row58_unsigned_score_off() {
    run_j(
        "row58_ranking::row58_unsigned_score_off",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "row58-unsigned-score-off"),
        raw!(include!("fixtures/row58_ranking/11bb72b11d141ddd.rs")),
        None,
    );
}

// row58-unsigned-score-on: POLICY-ACCEPTED (arm64-pin-portability-1137.md).
#[test]
fn row58_unsigned_score_on() {
    run_j(
        "row58_ranking::row58_unsigned_score_on",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "row58-unsigned-score-on"),
        raw!(include!("fixtures/row58_ranking/11bb72b11d141ddd.rs")),
        None,
    );
}
