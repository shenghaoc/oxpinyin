//! user.conf integer overflow and state preservation, and over-long chewing index keys (inventory row 56, #920, #525). Generated from the frozen pin observations.

use crate::bodies::user_conf_overlong_index as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};

// abort-guess-candidates-overlong-incomplete-index-key: the pin's abort, reproduced.
#[test]
fn abort_guess_candidates_overlong_incomplete_index_key() {
    run_aborting(
        "user_conf_overlong_index::abort_guess_candidates_overlong_incomplete_index_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-candidates-overlong-incomplete-index-key");
        },
    );
}

// abort-guess-candidates-overlong-index-key: the pin's abort, reproduced.
#[test]
fn abort_guess_candidates_overlong_index_key() {
    run_aborting(
        "user_conf_overlong_index::abort_guess_candidates_overlong_index_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-candidates-overlong-index-key");
        },
    );
}

// abort-guess-candidates-overlong-odd-index-key: the pin's abort, reproduced.
#[test]
fn abort_guess_candidates_overlong_odd_index_key() {
    run_aborting(
        "user_conf_overlong_index::abort_guess_candidates_overlong_odd_index_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-candidates-overlong-odd-index-key");
        },
    );
}

// abort-guess-candidates-overlong-odd-system-index-key: the pin's abort, reproduced.
#[test]
fn abort_guess_candidates_overlong_odd_system_index_key() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "user_conf_overlong_index::abort_guess_candidates_overlong_odd_system_index_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[("user.conf", UserFile::Conf(USER_CONF))]),
        |k| {
            b::case(k, "abort-guess-candidates-overlong-odd-system-index-key");
        },
    );
}

// abort-guess-candidates-overlong-system-index-key: the pin's abort, reproduced.
#[test]
fn abort_guess_candidates_overlong_system_index_key() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "user_conf_overlong_index::abort_guess_candidates_overlong_system_index_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[("user.conf", UserFile::Conf(USER_CONF))]),
        |k| {
            b::case(k, "abort-guess-candidates-overlong-system-index-key");
        },
    );
}

// abort-mask-out-empty-index-key: the pin's abort, reproduced.
#[test]
fn abort_mask_out_empty_index_key() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "user_conf_overlong_index::abort_mask_out_empty_index_key",
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
            b::case(k, "abort-mask-out-empty-index-key");
        },
    );
}

// abort-mask-out-one-byte-index-key: the pin's abort, reproduced.
#[test]
fn abort_mask_out_one_byte_index_key() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "user_conf_overlong_index::abort_mask_out_one_byte_index_key",
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
            b::case(k, "abort-mask-out-one-byte-index-key");
        },
    );
}

// abort-mask-out-overlong-index-key: the pin's abort, reproduced.
#[test]
fn abort_mask_out_overlong_index_key() {
    run_aborting(
        "user_conf_overlong_index::abort_mask_out_overlong_index_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-mask-out-overlong-index-key");
        },
    );
}

// abort-mask-out-overlong-index-key-later-unmappable: the pin's abort, reproduced.
#[test]
fn abort_mask_out_overlong_index_key_later_unmappable() {
    run_aborting(
        "user_conf_overlong_index::abort_mask_out_overlong_index_key_later_unmappable",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-mask-out-overlong-index-key-later-unmappable");
        },
    );
}

// abort-mask-out-overlong-odd-index-key: the pin's abort, reproduced.
#[test]
fn abort_mask_out_overlong_odd_index_key() {
    run_aborting(
        "user_conf_overlong_index::abort_mask_out_overlong_odd_index_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-mask-out-overlong-odd-index-key");
        },
    );
}

// guess-after-overlong-index-key-without-prefix-answers-true: MATCH.
#[test]
fn guess_after_overlong_index_key_without_prefix_answers_true() {
    run_j(
        "user_conf_overlong_index::guess_after_overlong_index_key_without_prefix_answers_true",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "guess-after-overlong-index-key-without-prefix-answers-true",
            )
        },
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// reparse-after-guess-ignores-overlong-index-key: MATCH.
#[test]
fn reparse_after_guess_ignores_overlong_index_key() {
    run_j(
        "user_conf_overlong_index::reparse_after_guess_ignores_overlong_index_key",
        Expect::RESULT,
        |k| b::case(k, "reparse-after-guess-ignores-overlong-index-key"),
        raw!(M(&[
            ("guess_hao", B(true)),
            ("logs", L(&[])),
            ("reparse_ni", I(2))
        ])),
        None,
    );
}

// user-conf-920-counter-missing: MATCH.
#[test]
fn user_conf_920_counter_missing() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/5afbc6a9142c9ea3.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/d7ef62ea7e9e6f59.rs"
    ));
    run_j(
        "user_conf_overlong_index::user_conf_920_counter_missing",
        Expect::RESULT,
        |k| b::case(k, "user-conf-920-counter-missing"),
        expected,
        None,
    );
}

// user-conf-920-counter-overflow-conforms: MATCH.
#[test]
fn user_conf_920_counter_overflow_conforms() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/5afbc6a9142c9ea3.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/d7ef62ea7e9e6f59.rs"
    ));
    run_j(
        "user_conf_overlong_index::user_conf_920_counter_overflow_conforms",
        Expect::RESULT,
        |k| b::case(k, "user-conf-920-counter-overflow-conforms"),
        expected,
        None,
    );
}

// user-conf-920-counter-overflow-nonconforms: MATCH.
#[test]
fn user_conf_920_counter_overflow_nonconforms() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/fef2cc48114d116a.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/c8223e19b142103a.rs"
    ));
    run_j(
        "user_conf_overlong_index::user_conf_920_counter_overflow_nonconforms",
        Expect::RESULT,
        |k| b::case(k, "user-conf-920-counter-overflow-nonconforms"),
        expected,
        None,
    );
}

// user-conf-920-database-missing-at-eof: MATCH.
#[test]
fn user_conf_920_database_missing_at_eof() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/fef2cc48114d116a.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/c8223e19b142103a.rs"
    ));
    run_j(
        "user_conf_overlong_index::user_conf_920_database_missing_at_eof",
        Expect::RESULT,
        |k| b::case(k, "user-conf-920-database-missing-at-eof"),
        expected,
        None,
    );
}

// user-conf-920-negative-overflow-nonconforms: MATCH.
#[test]
fn user_conf_920_negative_overflow_nonconforms() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/fef2cc48114d116a.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/c8223e19b142103a.rs"
    ));
    run_j(
        "user_conf_overlong_index::user_conf_920_negative_overflow_nonconforms",
        Expect::RESULT,
        |k| b::case(k, "user-conf-920-negative-overflow-nonconforms"),
        expected,
        None,
    );
}

// user-conf-920-nul-database-token: MATCH.
#[test]
fn user_conf_920_nul_database_token() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/5afbc6a9142c9ea3.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/d7ef62ea7e9e6f59.rs"
    ));
    run_j(
        "user_conf_overlong_index::user_conf_920_nul_database_token",
        Expect::RESULT,
        |k| b::case(k, "user-conf-920-nul-database-token"),
        expected,
        None,
    );
}

// user-conf-920-version-overflow-conforms: MATCH.
#[test]
fn user_conf_920_version_overflow_conforms() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/5afbc6a9142c9ea3.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/d7ef62ea7e9e6f59.rs"
    ));
    run_j(
        "user_conf_overlong_index::user_conf_920_version_overflow_conforms",
        Expect::RESULT,
        |k| b::case(k, "user-conf-920-version-overflow-conforms"),
        expected,
        None,
    );
}

// user-conf-920-version-overflow-nonconforms: MATCH.
#[test]
fn user_conf_920_version_overflow_nonconforms() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/fef2cc48114d116a.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/user_conf_overlong_index/c8223e19b142103a.rs"
    ));
    run_j(
        "user_conf_overlong_index::user_conf_920_version_overflow_nonconforms",
        Expect::RESULT,
        |k| b::case(k, "user-conf-920-version-overflow-nonconforms"),
        expected,
        None,
    );
}

// user-conf-marker-unwritable: MATCH.
#[test]
fn user_conf_marker_unwritable() {
    run_j(
        "user_conf_overlong_index::user_conf_marker_unwritable",
        // The UCRT's fopen of a directory fails with EACCES (Microsoft Learn, errno constants), so the user.conf load writes its open %s failed. line first; text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.write SCRATCH\\user-X\\user.conf failed.\r\nwrite SCRATCH\\user-X\\user.conf failed.\r\n"
        } else {
            b"write SCRATCH/user-X/user.conf failed.\nwrite SCRATCH/user-X/user.conf failed.\n"
        }),
        |k| b::case(k, "user-conf-marker-unwritable"),
        raw!(include!(
            "fixtures/user_conf_overlong_index/4fbaa91fbb2ff07a.rs"
        )),
        None,
    );
}
