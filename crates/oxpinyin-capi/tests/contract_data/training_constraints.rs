//! Training after stale forcings and shrinking re-parses, and forcings over abbreviations past the expansion limit (#1175, #525, #825). Generated from the frozen pin observations.

use crate::bodies::training_constraints as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};

// abort-train-after-reparse-empty-end-column: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_train_after_reparse_empty_end_column() {
    run_aborting(
        "training_constraints::abort_train_after_reparse_empty_end_column",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-train-after-reparse-empty-end-column");
        },
    );
}

// abort-train-after-reparse-empty-start-column: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_train_after_reparse_empty_start_column() {
    run_aborting(
        "training_constraints::abort_train_after_reparse_empty_start_column",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-train-after-reparse-empty-start-column");
        },
    );
}

// abort-train-after-reparse-past-matrix-end: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_train_after_reparse_past_matrix_end() {
    run_aborting(
        "training_constraints::abort_train_after_reparse_past_matrix_end",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-train-after-reparse-past-matrix-end");
        },
    );
}

// abort-train-index-past-results: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_train_index_past_results() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "training_constraints::abort_train_index_past_results",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[("user.conf", UserFile::Conf(USER_CONF))]),
        |k| {
            b::case(k, "abort-train-index-past-results");
        },
    );
}

// abort-train-stale-forcing: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_train_stale_forcing() {
    run_aborting(
        "training_constraints::abort_train_stale_forcing",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-train-stale-forcing");
        },
    );
}

// constraint-stale-item-drops-nihao: MATCH.
#[test]
fn constraint_stale_item_drops_nihao() {
    run_j(
        "training_constraints::constraint_stale_item_drops_nihao",
        Expect::RESULT,
        |k| b::case(k, "constraint-stale-item-drops-nihao"),
        raw!(include!(
            "fixtures/training_constraints/6967192d1353f730.rs"
        )),
        None,
    );
}

// constraint-stale-item-keeps-nini: MATCH.
#[test]
fn constraint_stale_item_keeps_nini() {
    run_j(
        "training_constraints::constraint_stale_item_keeps_nini",
        Expect::RESULT,
        |k| b::case(k, "constraint-stale-item-keeps-nini"),
        raw!(include!(
            "fixtures/training_constraints/31f3a31288936d20.rs"
        )),
        None,
    );
}

// constraint-survives-reconcile-juezhhrm: MATCH.
#[test]
fn constraint_survives_reconcile_juezhhrm() {
    run_j(
        "training_constraints::constraint_survives_reconcile_juezhhrm",
        Expect::RESULT,
        |k| b::case(k, "constraint-survives-reconcile-juezhhrm"),
        raw!(include!(
            "fixtures/training_constraints/5f4c832b059c88fe.rs"
        )),
        None,
    );
}

// constraint-survives-reconcile-juezhonghuarm: MATCH.
#[test]
fn constraint_survives_reconcile_juezhonghuarm() {
    run_j(
        "training_constraints::constraint_survives_reconcile_juezhonghuarm",
        Expect::RESULT,
        |k| b::case(k, "constraint-survives-reconcile-juezhonghuarm"),
        raw!(include!(
            "fixtures/training_constraints/c5e87b42caa708d6.rs"
        )),
        None,
    );
}

// constraint-survives-zhhrenmin: MATCH.
#[test]
fn constraint_survives_zhhrenmin() {
    run_j(
        "training_constraints::constraint_survives_zhhrenmin",
        Expect::RESULT,
        |k| b::case(k, "constraint-survives-zhhrenmin"),
        raw!(include!(
            "fixtures/training_constraints/292ee64dd64326f7.rs"
        )),
        None,
    );
}

// constraint-survives-zhhrm: MATCH.
#[test]
fn constraint_survives_zhhrm() {
    run_j(
        "training_constraints::constraint_survives_zhhrm",
        Expect::RESULT,
        |k| b::case(k, "constraint-survives-zhhrm"),
        raw!(include!(
            "fixtures/training_constraints/ab8ab57e5a9a75ba.rs"
        )),
        None,
    );
}

// constraint-survives-zhhrmghg: MATCH.
#[test]
fn constraint_survives_zhhrmghg() {
    run_j(
        "training_constraints::constraint_survives_zhhrmghg",
        Expect::RESULT,
        |k| b::case(k, "constraint-survives-zhhrmghg"),
        raw!(include!(
            "fixtures/training_constraints/91383531900251cf.rs"
        )),
        None,
    );
}

// constraint-survives-zhonghrm: MATCH.
#[test]
fn constraint_survives_zhonghrm() {
    run_j(
        "training_constraints::constraint_survives_zhonghrm",
        Expect::RESULT,
        |k| b::case(k, "constraint-survives-zhonghrm"),
        raw!(include!(
            "fixtures/training_constraints/42899d1e3b24df9c.rs"
        )),
        None,
    );
}

// constraint-survives-zhonghuarenmin: MATCH.
#[test]
fn constraint_survives_zhonghuarenmin() {
    run_j(
        "training_constraints::constraint_survives_zhonghuarenmin",
        Expect::RESULT,
        |k| b::case(k, "constraint-survives-zhonghuarenmin"),
        raw!(include!(
            "fixtures/training_constraints/15326c91e9359d7c.rs"
        )),
        None,
    );
}

// constraint-survives-zhonghuarm: MATCH.
#[test]
fn constraint_survives_zhonghuarm() {
    run_j(
        "training_constraints::constraint_survives_zhonghuarm",
        Expect::RESULT,
        |k| b::case(k, "constraint-survives-zhonghuarm"),
        raw!(include!(
            "fixtures/training_constraints/b318d0ad466df11d.rs"
        )),
        None,
    );
}

// train-after-choose-lookup-control: MATCH.
#[test]
fn train_after_choose_lookup_control() {
    run_j(
        "training_constraints::train_after_choose_lookup_control",
        Expect::RESULT,
        |k| b::case(k, "train-after-choose-lookup-control"),
        raw!(M(&[
            ("logs", L(&[])),
            ("no-reparse", B(true)),
            ("same-length-reparse", B(true))
        ])),
        None,
    );
}

// train-after-reparse-and-choose-control: MATCH.
#[test]
fn train_after_reparse_and_choose_control() {
    run_j(
        "training_constraints::train_after_reparse_and_choose_control",
        Expect::RESULT,
        |k| b::case(k, "train-after-reparse-and-choose-control"),
        raw!(M(&[
            ("first choose", I(5)),
            ("guess", B(true)),
            ("logs", L(&[])),
            ("parse", I(7)),
            ("reparse", I(4)),
            ("row 0", L(&[B(true), S("你好吗")])),
            ("second choose", I(2)),
            ("train", B(true))
        ])),
        None,
    );
}

// train-after-reparse-internal-zero-key: MATCH.
#[test]
fn train_after_reparse_internal_zero_key() {
    run_j(
        "training_constraints::train_after_reparse_internal_zero_key",
        Expect::RESULT,
        |k| b::case(k, "train-after-reparse-internal-zero-key"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// train-forcing-after-lookup: MATCH.
#[test]
fn train_forcing_after_lookup() {
    run_j(
        "training_constraints::train_forcing_after_lookup",
        Expect::RESULT,
        |k| b::case(k, "train-forcing-after-lookup"),
        raw!(M(&[
            ("again", B(true)),
            ("chosen", I(2)),
            ("logs", L(&[])),
            ("train", B(true))
        ])),
        None,
    );
}
