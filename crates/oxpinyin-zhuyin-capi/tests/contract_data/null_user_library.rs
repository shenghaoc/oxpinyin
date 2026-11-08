//! Null user directories and transient user-library imports: cwd and TMPDIR left untouched (#642, #1175). Generated from the frozen pin observations.

use crate::bodies::null_user_library as b;
use crate::support::{Expect, run_j};

// null-user-session-zhuyin: MATCH.
#[test]
fn null_user_session_zhuyin() {
    run_j(
        "null_user_library::null_user_session_zhuyin",
        Expect::stderr(b"open  failed.open  failed."),
        |k| b::case(k, "null-user-session-zhuyin"),
        raw!(include!("fixtures/null_user_library/5cf5d8950ed42918.rs")),
        None,
    );
}

// user-library-1-null-zhuyin: MATCH.
#[test]
fn user_library_1_null_zhuyin() {
    run_j(
        "null_user_library::user_library_1_null_zhuyin",
        Expect::stderr(b"open  failed."),
        |k| b::case(k, "user-library-1-null-zhuyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            ("export", S("NO EXPORT API")),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(16802309)]))
        ])),
        None,
    );
}

// user-library-1-ordinary-zhuyin: MATCH.
#[test]
fn user_library_1_ordinary_zhuyin() {
    run_j(
        "null_user_library::user_library_1_ordinary_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "user-library-1-ordinary-zhuyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            ("export", S("NO EXPORT API")),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(16802309)]))
        ])),
        None,
    );
}

// user-library-2-null-zhuyin: MATCH.
#[test]
fn user_library_2_null_zhuyin() {
    run_j(
        "null_user_library::user_library_2_null_zhuyin",
        Expect::stderr(b"open  failed."),
        |k| b::case(k, "user-library-2-null-zhuyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            ("export", S("NO EXPORT API")),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(33570045)]))
        ])),
        None,
    );
}

// user-library-2-ordinary-zhuyin: MATCH.
#[test]
fn user_library_2_ordinary_zhuyin() {
    run_j(
        "null_user_library::user_library_2_ordinary_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "user-library-2-ordinary-zhuyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            ("export", S("NO EXPORT API")),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(33570045)]))
        ])),
        None,
    );
}

// user-library-3-null-zhuyin: MATCH.
#[test]
fn user_library_7_null_zhuyin() {
    run_j(
        "null_user_library::user_library_7_null_zhuyin",
        Expect::stderr(b"open  failed."),
        |k| b::case(k, "user-library-7-null-zhuyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            ("export", S("NO EXPORT API")),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(117440513)]))
        ])),
        None,
    );
}

// user-library-7-ordinary-zhuyin: MATCH.
#[test]
fn user_library_7_ordinary_zhuyin() {
    run_j(
        "null_user_library::user_library_7_ordinary_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "user-library-7-ordinary-zhuyin"),
        raw!(M(&[
            ("add", B(true)),
            ("begin", B(true)),
            ("export", S("NO EXPORT API")),
            ("init", B(true)),
            ("logs", L(&[])),
            ("tokens", L(&[I(117440513)]))
        ])),
        None,
    );
}
