//! punct.bin decoding, punctuation order and duplicates, and aliased punctuation prefixes (#1144, #928, #1272). Generated from the frozen pin observations.

use crate::bodies::punctuation as b;
use crate::support::{Expect, run_j};

// punct-alias/borrowed-candidates: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn punct_alias_borrowed_candidates() {
    run_j(
        "punctuation::punct_alias_borrowed_candidates",
        Expect::RESULT,
        |k| b::case(k, "punct-alias/borrowed-candidates"),
        raw!(M(&[
            ("candidates", I(0)),
            ("kinds", L(&[])),
            ("ret", B(true)),
            ("sentences kept", B(true))
        ])),
        Some(&["candidates", "kinds", "ret", "sentences kept"]),
    );
}

// punct-alias/borrowed-predicted: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn punct_alias_borrowed_predicted() {
    run_j(
        "punctuation::punct_alias_borrowed_predicted",
        Expect::RESULT,
        |k| b::case(k, "punct-alias/borrowed-predicted"),
        raw!(M(&[
            ("candidates", I(0)),
            ("kinds", L(&[])),
            ("ret", B(true)),
            ("sentences kept", B(true))
        ])),
        Some(&["candidates", "kinds", "ret", "sentences kept"]),
    );
}

// punct-alias/owned-candidates: MATCH.
#[test]
fn punct_alias_owned_candidates() {
    run_j(
        "punctuation::punct_alias_owned_candidates",
        Expect::RESULT,
        |k| b::case(k, "punct-alias/owned-candidates"),
        raw!(M(&[
            ("candidates", I(4)),
            ("kinds", L(&[I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("ret", B(true)),
            ("sentences kept", B(true))
        ])),
        None,
    );
}

// punct-alias/owned-predicted: MATCH.
#[test]
fn punct_alias_owned_predicted() {
    run_j(
        "punctuation::punct_alias_owned_predicted",
        Expect::RESULT,
        |k| b::case(k, "punct-alias/owned-predicted"),
        raw!(M(&[
            ("candidates", I(1)),
            ("kinds", L(&[I(5)])),
            ("logs", L(&[])),
            ("ret", B(true)),
            ("sentences kept", B(true))
        ])),
        None,
    );
}

// punct-decode/above-unicode: MATCH.
#[test]
fn punct_decode_above_unicode() {
    run_j(
        "punctuation::punct_decode_above_unicode",
        Expect::RESULT,
        |k| b::case(k, "punct-decode/above-unicode"),
        raw!(include!("fixtures/punctuation/f1922cb7a467a1ec.rs")),
        None,
    );
}

// punct-decode/empty: MATCH.
#[test]
fn punct_decode_empty() {
    run_j(
        "punctuation::punct_decode_empty",
        Expect::RESULT,
        |k| b::case(k, "punct-decode/empty"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[S(""), S("efbc8c")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-decode/empty-leading: MATCH.
#[test]
fn punct_decode_empty_leading() {
    run_j(
        "punctuation::punct_decode_empty_leading",
        Expect::RESULT,
        |k| b::case(k, "punct-decode/empty-leading"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[S(""), S("e38082")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-decode/misaligned-short: POLICY-ACCEPTED (1297-punct-words.md).
#[test]
fn punct_decode_misaligned_short() {
    run_j(
        "punctuation::punct_decode_misaligned_short",
        Expect::stderr(b""),
        |k| b::case(k, "punct-decode/misaligned-short"),
        raw!(M(&[("logs", L(&[])), ("puncts", L(&[])), ("ret", B(true))])),
        None,
    );
}

// punct-decode/misaligned-terminated: POLICY-ACCEPTED (1297-punct-words.md).
#[test]
fn punct_decode_misaligned_terminated() {
    run_j(
        "punctuation::punct_decode_misaligned_terminated",
        Expect::stderr(b""),
        |k| b::case(k, "punct-decode/misaligned-terminated"),
        raw!(M(&[
            ("logs", L(&[])),
            ("puncts", L(&[S("efbc8c")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-decode/misaligned-unterminated: MATCH.
#[test]
fn punct_decode_misaligned_unterminated() {
    run_j(
        "punctuation::punct_decode_misaligned_unterminated",
        Expect::RESULT,
        |k| b::case(k, "punct-decode/misaligned-unterminated"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[S("efbc8c")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-decode/surrogate: MATCH.
#[test]
fn punct_decode_surrogate() {
    run_j(
        "punctuation::punct_decode_surrogate",
        Expect::RESULT,
        |k| b::case(k, "punct-decode/surrogate"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[S("eda080"), S("efbc8cedbfbf")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-decode/unterminated: MATCH.
#[test]
fn punct_decode_unterminated() {
    run_j(
        "punctuation::punct_decode_unterminated",
        Expect::RESULT,
        |k| b::case(k, "punct-decode/unterminated"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[S("efbc8c"), S("e38082e38081")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-decode/unterminated-zero: MATCH.
#[test]
fn punct_decode_unterminated_zero() {
    run_j(
        "punctuation::punct_decode_unterminated_zero",
        Expect::RESULT,
        |k| b::case(k, "punct-decode/unterminated-zero"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[S("efbc8c"), S("")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-decode/valid: MATCH.
#[test]
fn punct_decode_valid() {
    run_j(
        "punctuation::punct_decode_valid",
        Expect::RESULT,
        |k| b::case(k, "punct-decode/valid"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[S("efbc8c"), S("e38082")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-decode/zero-length: MATCH.
#[test]
fn punct_decode_zero_length() {
    run_j(
        "punctuation::punct_decode_zero_length",
        Expect::RESULT,
        |k| b::case(k, "punct-decode/zero-length"),
        raw!(M(&[
            ("kinds", L(&[I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-dedup/across-prefixes: MATCH.
#[test]
fn punct_dedup_across_prefixes() {
    run_j(
        "punctuation::punct_dedup_across_prefixes",
        Expect::RESULT,
        |k| b::case(k, "punct-dedup/across-prefixes"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(4)),
            ("puncts", L(&[S("efbc8c"), S("efbc81"), S("e38082")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-dedup/candidate: MATCH.
#[test]
fn punct_dedup_candidate() {
    run_j(
        "punctuation::punct_dedup_candidate",
        Expect::RESULT,
        |k| b::case(k, "punct-dedup/candidate"),
        raw!(include!("fixtures/punctuation/d8c755422515793a.rs")),
        None,
    );
}

// punct-dedup/distinct: MATCH.
#[test]
fn punct_dedup_distinct() {
    run_j(
        "punctuation::punct_dedup_distinct",
        Expect::RESULT,
        |k| b::case(k, "punct-dedup/distinct"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[S("efbc8c"), S("e38082"), S("efbc81")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-dedup/empty: MATCH.
#[test]
fn punct_dedup_empty() {
    run_j(
        "punctuation::punct_dedup_empty",
        Expect::RESULT,
        |k| b::case(k, "punct-dedup/empty"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[S(""), S("efbc8c")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-dedup/empty-forms: MATCH.
#[test]
fn punct_dedup_empty_forms() {
    run_j(
        "punctuation::punct_dedup_empty_forms",
        Expect::RESULT,
        |k| b::case(k, "punct-dedup/empty-forms"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[S(""), S("e38082")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-dedup/non-scalar: MATCH.
#[test]
fn punct_dedup_non_scalar() {
    run_j(
        "punctuation::punct_dedup_non_scalar",
        Expect::RESULT,
        |k| b::case(k, "punct-dedup/non-scalar"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(4)),
            ("puncts", L(&[S("eda080"), S("f4908080")])),
            ("ret", B(true))
        ])),
        None,
    );
}

// punct-dedup/within: MATCH.
#[test]
fn punct_dedup_within() {
    run_j(
        "punctuation::punct_dedup_within",
        Expect::RESULT,
        |k| b::case(k, "punct-dedup/within"),
        raw!(M(&[
            ("kinds", L(&[I(8), I(8), I(5), I(5)])),
            ("logs", L(&[])),
            ("others", I(168)),
            ("puncts", L(&[S("efbc8c"), S("e38082")])),
            ("ret", B(true))
        ])),
        None,
    );
}
