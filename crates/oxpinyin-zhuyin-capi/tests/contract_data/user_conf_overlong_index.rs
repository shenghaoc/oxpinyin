//! user.conf integer overflow and state preservation, and over-long chewing index keys (inventory row 56, #920, #525). Generated from the frozen pin observations.

use crate::bodies::user_conf_overlong_index as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};

// abort-zhuyin-mask-out-empty-index-key: the pin's abort, reproduced.
#[test]
fn abort_zhuyin_mask_out_empty_index_key() {
    // The pin aborts before it rewrites user.conf: the file is the case's own LF text on every host.
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:0|";
    run_aborting(
        "user_conf_overlong_index::abort_zhuyin_mask_out_empty_index_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[
            ("user.conf", UserFile::Conf(USER_CONF)),
            ("user_pinyin_index.bin", UserFile::Size(529408)),
        ]),
        |k| {
            b::case(k, "abort-zhuyin-mask-out-empty-index-key");
        },
    );
}

// abort-zhuyin-mask-out-overlong-index-key: the pin's abort, reproduced.
#[test]
fn abort_zhuyin_mask_out_overlong_index_key() {
    run_aborting(
        "user_conf_overlong_index::abort_zhuyin_mask_out_overlong_index_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-mask-out-overlong-index-key");
        },
    );
}

// abort-zhuyin-mask-out-overlong-index-key-later-unmappable: the pin's abort, reproduced.
#[test]
fn abort_zhuyin_mask_out_overlong_index_key_later_unmappable() {
    run_aborting(
        "user_conf_overlong_index::abort_zhuyin_mask_out_overlong_index_key_later_unmappable",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(
                k,
                "abort-zhuyin-mask-out-overlong-index-key-later-unmappable",
            );
        },
    );
}

// user-conf-920-counter-overflow-nonconforms-zhuyin: MATCH.
#[test]
fn user_conf_920_counter_overflow_nonconforms_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings"). The case then rewrites it line by line, splitting on LF and joining with LF, so the pin's CR stays on the lines it keeps, and the zhuyin pin's reopen leaves the file as written.
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/99f8c8ff0e076272.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/d4450b1c065220a1.rs"
    ));
    run_j(
        "user_conf_overlong_index::user_conf_920_counter_overflow_nonconforms_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "user-conf-920-counter-overflow-nonconforms-zhuyin"),
        expected,
        None,
    );
}

// user-conf-920-version-overflow-conforms-zhuyin: MATCH.
#[test]
fn user_conf_920_version_overflow_conforms_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings"). The case then rewrites it line by line, splitting on LF and joining with LF, so the pin's CR stays on the lines it keeps, and the zhuyin pin's reopen leaves the file as written.
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/938ba2b023993765.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/48be3121ba32fec5.rs"
    ));
    run_j(
        "user_conf_overlong_index::user_conf_920_version_overflow_conforms_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "user-conf-920-version-overflow-conforms-zhuyin"),
        expected,
        None,
    );
}

// user-conf-920-version-overflow-nonconforms-zhuyin: MATCH.
#[test]
fn user_conf_920_version_overflow_nonconforms_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings"). The case then rewrites it line by line, splitting on LF and joining with LF, so the pin's CR stays on the lines it keeps, and the zhuyin pin's reopen leaves the file as written.
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/2dadd692b6486109.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/d9ab6cfb7cbfd7b8.rs"
    ));
    run_j(
        "user_conf_overlong_index::user_conf_920_version_overflow_nonconforms_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "user-conf-920-version-overflow-nonconforms-zhuyin"),
        expected,
        None,
    );
}

// user-conf-marker-unwritable-zhuyin: MATCH.
#[test]
fn user_conf_marker_unwritable_zhuyin() {
    run_j(
        "user_conf_overlong_index::user_conf_marker_unwritable_zhuyin",
        // The UCRT's fopen of a directory fails with EACCES (Microsoft Learn, errno constants), so the user.conf load writes its open %s failed. line first; text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.write SCRATCH\\user-X\\user.conf failed.\r\n"
        } else {
            b"write SCRATCH/user-X/user.conf failed.\n"
        }),
        |k| b::case(k, "user-conf-marker-unwritable-zhuyin"),
        raw!(include!(
            "fixtures/user_conf_overlong_index/bec9b1b8c5521c3f.rs"
        )),
        None,
    );
}

// zhuyin-guess-candidates-overlong-index-key: MATCH.
#[test]
fn zhuyin_guess_candidates_overlong_index_key() {
    run_j(
        "user_conf_overlong_index::zhuyin_guess_candidates_overlong_index_key",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-guess-candidates-overlong-index-key"),
        raw!(include!(
            "fixtures/user_conf_overlong_index/f850983ccae2a463.rs"
        )),
        None,
    );
}
