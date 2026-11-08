//! Null user directories and transient user-library imports: cwd and TMPDIR left untouched (#642, #1175). Generated from the frozen pin observations.

use crate::bodies::null_user_library as b;
use crate::support::{Expect, run_j};

// null-user-session-pinyin: MATCH.
#[test]
fn null_user_session_pinyin() {
    run_j(
        "null_user_library::null_user_session_pinyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open  failed.write  failed.\r\nopen  failed.write  failed.\r\nwrite  failed.\r\nwrite  failed.\r\n"
        } else {
            b"open  failed.write  failed.\nopen  failed.write  failed.\nwrite  failed.\nwrite  failed.\n"
        }),
        |k| b::case(k, "null-user-session-pinyin"),
        raw!(include!("fixtures/null_user_library/ae4639b31ebf41b7.rs")),
        None,
    );
}

// user-library-1-null-pinyin: MATCH.
#[test]
fn user_library_1_null_pinyin() {
    run_j(
        "null_user_library::user_library_1_null_pinyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open  failed.write  failed.\r\nwrite  failed.\r\n"
        } else {
            b"open  failed.write  failed.\nwrite  failed.\n"
        }),
        |k| b::case(k, "user-library-1-null-pinyin"),
        raw!(include!("fixtures/null_user_library/06efe3bf9318dc2a.rs")),
        None,
    );
}

// user-library-1-ordinary-pinyin: MATCH.
#[test]
fn user_library_1_ordinary_pinyin() {
    run_j(
        "null_user_library::user_library_1_ordinary_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "user-library-1-ordinary-pinyin"),
        raw!(include!("fixtures/null_user_library/06efe3bf9318dc2a.rs")),
        None,
    );
}

// user-library-2-null-pinyin: MATCH.
#[test]
fn user_library_2_null_pinyin() {
    run_j(
        "null_user_library::user_library_2_null_pinyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open  failed.write  failed.\r\nwrite  failed.\r\n"
        } else {
            b"open  failed.write  failed.\nwrite  failed.\n"
        }),
        |k| b::case(k, "user-library-2-null-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(33570045)]))
        ])),
        None,
    );
}

// user-library-2-ordinary-pinyin: MATCH.
#[test]
fn user_library_2_ordinary_pinyin() {
    run_j(
        "null_user_library::user_library_2_ordinary_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "user-library-2-ordinary-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(33570045)]))
        ])),
        None,
    );
}

// user-library-3-null-pinyin: MATCH.
#[test]
fn user_library_3_null_pinyin() {
    run_j(
        "null_user_library::user_library_3_null_pinyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open  failed.write  failed.\r\nwrite  failed.\r\n"
        } else {
            b"open  failed.write  failed.\nwrite  failed.\n"
        }),
        |k| b::case(k, "user-library-3-null-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(50359794)]))
        ])),
        None,
    );
}

// user-library-3-ordinary-pinyin: MATCH.
#[test]
fn user_library_3_ordinary_pinyin() {
    run_j(
        "null_user_library::user_library_3_ordinary_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "user-library-3-ordinary-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(50359794)]))
        ])),
        None,
    );
}

// user-library-4-null-pinyin: MATCH.
#[test]
fn user_library_4_null_pinyin() {
    run_j(
        "null_user_library::user_library_4_null_pinyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open  failed.write  failed.\r\nwrite  failed.\r\n"
        } else {
            b"open  failed.write  failed.\nwrite  failed.\n"
        }),
        |k| b::case(k, "user-library-4-null-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(67109866)]))
        ])),
        None,
    );
}

// user-library-4-ordinary-pinyin: MATCH.
#[test]
fn user_library_4_ordinary_pinyin() {
    run_j(
        "null_user_library::user_library_4_ordinary_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "user-library-4-ordinary-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(67109866)]))
        ])),
        None,
    );
}

// user-library-5-null-pinyin: MATCH.
#[test]
fn user_library_5_null_pinyin() {
    run_j(
        "null_user_library::user_library_5_null_pinyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open  failed.write  failed.\r\nwrite  failed.\r\n"
        } else {
            b"open  failed.write  failed.\nwrite  failed.\n"
        }),
        |k| b::case(k, "user-library-5-null-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(83886081)]))
        ])),
        None,
    );
}

// user-library-5-ordinary-pinyin: MATCH.
#[test]
fn user_library_5_ordinary_pinyin() {
    run_j(
        "null_user_library::user_library_5_ordinary_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "user-library-5-ordinary-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(83886081)]))
        ])),
        None,
    );
}

// user-library-6-null-pinyin: MATCH.
#[test]
fn user_library_6_null_pinyin() {
    run_j(
        "null_user_library::user_library_6_null_pinyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open  failed.write  failed.\r\nwrite  failed.\r\n"
        } else {
            b"open  failed.write  failed.\nwrite  failed.\n"
        }),
        |k| b::case(k, "user-library-6-null-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(100663297)]))
        ])),
        None,
    );
}

// user-library-6-ordinary-pinyin: MATCH.
#[test]
fn user_library_6_ordinary_pinyin() {
    run_j(
        "null_user_library::user_library_6_ordinary_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "user-library-6-ordinary-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(100663297)]))
        ])),
        None,
    );
}

// user-library-7-null-pinyin: MATCH.
#[test]
fn user_library_7_null_pinyin() {
    run_j(
        "null_user_library::user_library_7_null_pinyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open  failed.write  failed.\r\nwrite  failed.\r\n"
        } else {
            b"open  failed.write  failed.\nwrite  failed.\n"
        }),
        |k| b::case(k, "user-library-7-null-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(117440513)]))
        ])),
        None,
    );
}

// user-library-7-ordinary-pinyin: MATCH.
#[test]
fn user_library_7_ordinary_pinyin() {
    run_j(
        "null_user_library::user_library_7_ordinary_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "user-library-7-ordinary-pinyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            (
                "export",
                L(&[L(&[B(true), S("你好"), S("ni3'hao3"), I(100000)])])
            ),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(117440513)]))
        ])),
        None,
    );
}
