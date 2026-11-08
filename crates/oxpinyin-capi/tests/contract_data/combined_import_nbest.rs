//! Searches that hold the same phrase in more than four libraries, and n-best ties between equally counted user phrases (#525, #757, register row 74). Generated from the frozen pin observations.

use crate::bodies::combined_import_nbest as b;
use crate::support::{Expect, run_aborting, run_j};

// character-offset-five-libraries-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn character_offset_five_libraries_pinyin() {
    run_aborting(
        "combined_import_nbest::character_offset_five_libraries_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "character-offset-five-libraries-pinyin");
        },
    );
}

// character-offset-four-libraries-pinyin: MATCH.
#[test]
fn character_offset_four_libraries_pinyin() {
    run_j(
        "combined_import_nbest::character_offset_four_libraries_pinyin",
        Expect::RESULT,
        |k| b::case(k, "character-offset-four-libraries-pinyin"),
        raw!(M(&[("length", I(0)), ("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// combined-library-fifth-add-pinyin: MATCH.
#[test]
fn combined_library_fifth_add_pinyin() {
    run_j(
        "combined_import_nbest::combined_library_fifth_add_pinyin",
        Expect::RESULT,
        |k| b::case(k, "combined-library-fifth-add-pinyin"),
        raw!(M(&[
            ("added", L(&[B(true), B(true), B(true), B(true), B(true)])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// combined-library-invalid-reading-pinyin: MATCH.
#[test]
fn combined_library_invalid_reading_pinyin() {
    run_j(
        "combined_import_nbest::combined_library_invalid_reading_pinyin",
        Expect::RESULT,
        |k| b::case(k, "combined-library-invalid-reading-pinyin"),
        raw!(M(&[
            ("added", L(&[B(true), B(true), B(true), B(true), B(true)])),
            ("logs", L(&[])),
            ("ret", B(false))
        ])),
        None,
    );
}

// combined-library-sixth-add-key-past-parse-bound-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn combined_library_sixth_add_key_past_parse_bound_pinyin() {
    run_aborting(
        "combined_import_nbest::combined_library_sixth_add_key_past_parse_bound_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "combined-library-sixth-add-key-past-parse-bound-pinyin");
        },
    );
}

// combined-library-sixth-add-long-reading-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn combined_library_sixth_add_long_reading_pinyin() {
    run_aborting(
        "combined_import_nbest::combined_library_sixth_add_long_reading_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "combined-library-sixth-add-long-reading-pinyin");
        },
    );
}

// combined-library-sixth-add-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn combined_library_sixth_add_pinyin() {
    run_aborting(
        "combined_import_nbest::combined_library_sixth_add_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "combined-library-sixth-add-pinyin");
        },
    );
}

// combined-library-unused-index-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn combined_library_unused_index_pinyin() {
    run_aborting(
        "combined_import_nbest::combined_library_unused_index_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "combined-library-unused-index-pinyin");
        },
    );
}

// nbest-user-table-order-pinyin: POLICY-ACCEPTED (trellis-adjusted-cost-s1-s2.md).
#[test]
fn nbest_user_table_order_pinyin() {
    run_j(
        "combined_import_nbest::nbest_user_table_order_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "nbest-user-table-order-pinyin"),
        raw!(include!(
            "fixtures/combined_import_nbest/c79963f197400676.rs"
        )),
        None,
    );
}

// predicted-candidates-five-libraries-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn predicted_candidates_five_libraries_pinyin() {
    run_aborting(
        "combined_import_nbest::predicted_candidates_five_libraries_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "predicted-candidates-five-libraries-pinyin");
        },
    );
}

// prefix-guess-five-libraries-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn prefix_guess_five_libraries_pinyin() {
    run_aborting(
        "combined_import_nbest::prefix_guess_five_libraries_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "prefix-guess-five-libraries-pinyin");
        },
    );
}

// prefix-guess-four-libraries-pinyin: MATCH.
#[test]
fn prefix_guess_four_libraries_pinyin() {
    run_j(
        "combined_import_nbest::prefix_guess_four_libraries_pinyin",
        Expect::RESULT,
        |k| b::case(k, "prefix-guess-four-libraries-pinyin"),
        raw!(M(&[("logs", L(&[])), ("ret", B(false))])),
        None,
    );
}

// remember-user-input-five-libraries-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn remember_user_input_five_libraries_pinyin() {
    run_aborting(
        "combined_import_nbest::remember_user_input_five_libraries_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "remember-user-input-five-libraries-pinyin");
        },
    );
}

// remember-user-input-five-libraries-unparsed-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn remember_user_input_five_libraries_unparsed_pinyin() {
    run_aborting(
        "combined_import_nbest::remember_user_input_five_libraries_unparsed_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "remember-user-input-five-libraries-unparsed-pinyin");
        },
    );
}
