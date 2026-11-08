//! Facade totals and counts: the n-best denominator wraps with imports, stored headers, unigram adds and library loads (#1175, #798, #801). Generated from the frozen pin observations.

use crate::bodies::facade_totals_counts as b;
use crate::support::{Expect, run_j};

// import-facade-total-below-wrap: MATCH.
#[test]
fn import_facade_total_below_wrap() {
    run_j(
        "facade_totals_counts::import_facade_total_below_wrap",
        Expect::RESULT,
        |k| b::case(k, "import-facade-total-below-wrap"),
        raw!(include!(
            "fixtures/facade_totals_counts/493b2d77e3fcf4e3.rs"
        )),
        None,
    );
}

// import-wrapped-facade-total-guess: MATCH.
#[test]
fn import_wrapped_facade_total_guess() {
    run_j(
        "facade_totals_counts::import_wrapped_facade_total_guess",
        Expect::RESULT,
        |k| b::case(k, "import-wrapped-facade-total-guess"),
        raw!(include!(
            "fixtures/facade_totals_counts/bf13ed037c93017c.rs"
        )),
        None,
    );
}

// import-wrapped-facade-total-segment: MATCH.
#[test]
fn import_wrapped_facade_total_segment() {
    run_j(
        "facade_totals_counts::import_wrapped_facade_total_segment",
        Expect::RESULT,
        |k| b::case(k, "import-wrapped-facade-total-segment"),
        raw!(include!(
            "fixtures/facade_totals_counts/9ca404f11aea9222.rs"
        )),
        None,
    );
}

// longer-choices-numerator-guess: MATCH.
#[test]
fn longer_choices_numerator_guess() {
    run_j(
        "facade_totals_counts::longer_choices_numerator_guess",
        Expect::RESULT,
        |k| b::case(k, "longer-choices-numerator-guess"),
        raw!(include!(
            "fixtures/facade_totals_counts/49541c9abaa111b6.rs"
        )),
        None,
    );
}

// stored-header-facade-total: MATCH.
#[test]
fn stored_header_facade_total() {
    run_j(
        "facade_totals_counts::stored_header_facade_total",
        Expect::RESULT,
        |k| b::case(k, "stored-header-facade-total"),
        raw!(include!(
            "fixtures/facade_totals_counts/2182d5766cd6ca55.rs"
        )),
        None,
    );
}

// stored-header-facade-total-small: MATCH.
#[test]
fn stored_header_facade_total_small() {
    run_j(
        "facade_totals_counts::stored_header_facade_total_small",
        Expect::RESULT,
        |k| b::case(k, "stored-header-facade-total-small"),
        raw!(include!(
            "fixtures/facade_totals_counts/481aab965e0d1ba4.rs"
        )),
        None,
    );
}

// system-add-reload-facade-total: MATCH.
#[test]
fn system_add_reload_facade_total() {
    run_j(
        "facade_totals_counts::system_add_reload_facade_total",
        Expect::RESULT,
        |k| b::case(k, "system-add-reload-facade-total"),
        raw!(include!(
            "fixtures/facade_totals_counts/2e44330507f87afd.rs"
        )),
        None,
    );
}

// system-add-reload-facade-total-small: MATCH.
#[test]
fn system_add_reload_facade_total_small() {
    run_j(
        "facade_totals_counts::system_add_reload_facade_total_small",
        Expect::RESULT,
        |k| b::case(k, "system-add-reload-facade-total-small"),
        raw!(include!(
            "fixtures/facade_totals_counts/9a492d11b8b91ce3.rs"
        )),
        None,
    );
}

// token-add-unigram-denominator-candidates: MATCH.
#[test]
fn token_add_unigram_denominator_candidates() {
    run_j(
        "facade_totals_counts::token_add_unigram_denominator_candidates",
        Expect::RESULT,
        |k| b::case(k, "token-add-unigram-denominator-candidates"),
        raw!(include!(
            "fixtures/facade_totals_counts/c4b55dabe86548c4.rs"
        )),
        None,
    );
}

// token-add-unigram-numerator-guess: MATCH.
#[test]
fn token_add_unigram_numerator_guess() {
    run_j(
        "facade_totals_counts::token_add_unigram_numerator_guess",
        Expect::RESULT,
        |k| b::case(k, "token-add-unigram-numerator-guess"),
        raw!(include!(
            "fixtures/facade_totals_counts/3c76b90ba011f1b3.rs"
        )),
        None,
    );
}

// unload-drops-accepted-bump: MATCH.
#[test]
fn unload_drops_accepted_bump() {
    run_j(
        "facade_totals_counts::unload_drops_accepted_bump",
        Expect::RESULT,
        |k| b::case(k, "unload-drops-accepted-bump"),
        raw!(include!(
            "fixtures/facade_totals_counts/9777f9c218740c34.rs"
        )),
        None,
    );
}
