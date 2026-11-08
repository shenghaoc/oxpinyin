//! Searches that hold the same phrase in more than four libraries, and n-best ties between equally counted user phrases (#525, #757, register row 74). Generated from the frozen pin observations.

use crate::bodies::combined_import_nbest as b;
use crate::support::{Expect, run_aborting, run_j};

// character-offset-five-libraries-zhuyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn character_offset_five_libraries_zhuyin() {
    run_aborting(
        "combined_import_nbest::character_offset_five_libraries_zhuyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "character-offset-five-libraries-zhuyin");
        },
    );
}

// character-offset-four-libraries-zhuyin: MATCH.
#[test]
fn character_offset_four_libraries_zhuyin() {
    run_j(
        "combined_import_nbest::character_offset_four_libraries_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "character-offset-four-libraries-zhuyin"),
        raw!(M(&[("length", I(0)), ("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// combined-library-fifth-add-zhuyin: MATCH.
#[test]
fn combined_library_fifth_add_zhuyin() {
    run_j(
        "combined_import_nbest::combined_library_fifth_add_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "combined-library-fifth-add-zhuyin"),
        raw!(M(&[
            ("added", L(&[B(true), B(true), B(true), B(true), B(true)])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// combined-library-invalid-reading-zhuyin: MATCH.
#[test]
fn combined_library_invalid_reading_zhuyin() {
    run_j(
        "combined_import_nbest::combined_library_invalid_reading_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "combined-library-invalid-reading-zhuyin"),
        raw!(M(&[
            ("added", L(&[B(true), B(true), B(true), B(true), B(true)])),
            ("logs", L(&[])),
            ("ret", B(false))
        ])),
        None,
    );
}

// combined-library-sixth-add-long-reading-zhuyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn combined_library_sixth_add_long_reading_zhuyin() {
    run_aborting(
        "combined_import_nbest::combined_library_sixth_add_long_reading_zhuyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "combined-library-sixth-add-long-reading-zhuyin");
        },
    );
}

// combined-library-sixth-add-one-key-zhuyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn combined_library_sixth_add_one_key_zhuyin() {
    run_aborting(
        "combined_import_nbest::combined_library_sixth_add_one_key_zhuyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "combined-library-sixth-add-one-key-zhuyin");
        },
    );
}

// combined-library-sixth-add-zhuyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn combined_library_sixth_add_zhuyin() {
    run_aborting(
        "combined_import_nbest::combined_library_sixth_add_zhuyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "combined-library-sixth-add-zhuyin");
        },
    );
}

// combined-library-unused-index-zhuyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn combined_library_unused_index_zhuyin() {
    run_aborting(
        "combined_import_nbest::combined_library_unused_index_zhuyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "combined-library-unused-index-zhuyin");
        },
    );
}

// nbest-user-table-order-zhuyin: POLICY-ACCEPTED (1378-zhuyin-selection.md).
#[test]
fn nbest_user_table_order_zhuyin() {
    run_j(
        "combined_import_nbest::nbest_user_table_order_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "nbest-user-table-order-zhuyin"),
        raw!(M(&[
            ("guess", B(true)),
            ("logs", L(&[])),
            ("parsed", I(4)),
            ("sentence", L(&[B(true), S("靐靐")])),
            ("tokens", L(&[S("0x7000001"), S("0x7000002")]))
        ])),
        None,
    );
}

// prefix-guess-five-libraries-zhuyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn prefix_guess_five_libraries_zhuyin() {
    run_aborting(
        "combined_import_nbest::prefix_guess_five_libraries_zhuyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "prefix-guess-five-libraries-zhuyin");
        },
    );
}

// prefix-guess-four-libraries-zhuyin: MATCH.
#[test]
fn prefix_guess_four_libraries_zhuyin() {
    run_j(
        "combined_import_nbest::prefix_guess_four_libraries_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "prefix-guess-four-libraries-zhuyin"),
        raw!(M(&[("logs", L(&[])), ("ret", B(false))])),
        None,
    );
}
