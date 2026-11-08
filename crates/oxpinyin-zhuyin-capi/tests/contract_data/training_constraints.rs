//! Training after stale forcings and shrinking re-parses, and forcings over abbreviations past the expansion limit (#1175, #525, #825). Generated from the frozen pin observations.

use crate::bodies::training_constraints as b;
use crate::support::{Expect, run_aborting, run_j};

// abort-zhuyin-train-after-reparse-empty-end-column: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_train_after_reparse_empty_end_column() {
    run_aborting(
        "training_constraints::abort_zhuyin_train_after_reparse_empty_end_column",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-train-after-reparse-empty-end-column");
        },
    );
}

// abort-zhuyin-train-after-reparse-empty-start-column: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_train_after_reparse_empty_start_column() {
    run_aborting(
        "training_constraints::abort_zhuyin_train_after_reparse_empty_start_column",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-train-after-reparse-empty-start-column");
        },
    );
}

// abort-zhuyin-train-after-reparse-past-matrix-end: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_train_after_reparse_past_matrix_end() {
    run_aborting(
        "training_constraints::abort_zhuyin_train_after_reparse_past_matrix_end",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-train-after-reparse-past-matrix-end");
        },
    );
}

// abort-zhuyin-train-stale-forcing: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_train_stale_forcing() {
    run_aborting(
        "training_constraints::abort_zhuyin_train_stale_forcing",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-train-stale-forcing");
        },
    );
}

// zhuyin-train-after-reparse-and-choose-control: MATCH.
#[test]
fn zhuyin_train_after_reparse_and_choose_control() {
    run_j(
        "training_constraints::zhuyin_train_after_reparse_and_choose_control",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-train-after-reparse-and-choose-control"),
        raw!(M(&[
            ("first choose", I(5)),
            ("guess", B(true)),
            ("logs", L(&[])),
            ("parse", I(7)),
            ("reparse", I(4)),
            ("second choose", I(2)),
            ("train", B(true))
        ])),
        None,
    );
}

// zhuyin-train-after-reparse-internal-zero-key: MATCH.
#[test]
fn zhuyin_train_after_reparse_internal_zero_key() {
    run_j(
        "training_constraints::zhuyin_train_after_reparse_internal_zero_key",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-train-after-reparse-internal-zero-key"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}
