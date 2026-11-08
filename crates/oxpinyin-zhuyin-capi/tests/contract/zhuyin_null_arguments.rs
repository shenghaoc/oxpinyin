//! NULL arguments the zhuyin pin dereferences (#563, register row 41). Generated from the frozen pin observations.

use crate::bodies::zhuyin_null_arguments::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};

// zhuyin-null-instance-parse-chewing: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1048-1049.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_instance_parse_chewing() {
    run(
        "zhuyin_null_arguments::zhuyin_null_instance_parse_chewing",
        Expect::stderr(b""),
        b::parse_chewing_null_instance,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-instance-in-chewing-keyboard: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1089-1090.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_instance_in_chewing_keyboard() {
    run(
        "zhuyin_null_arguments::zhuyin_null_instance_in_chewing_keyboard",
        Expect::stderr(b""),
        b::in_chewing_null_instance,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-instance-get-n-phrase: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:2229.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_instance_get_n_phrase() {
    run(
        "zhuyin_null_arguments::zhuyin_null_instance_get_n_phrase",
        Expect::stderr(b""),
        b::n_phrase_null_instance,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-instance-get-phrase-token: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:2236, :2240.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_instance_get_phrase_token() {
    run(
        "zhuyin_null_arguments::zhuyin_null_instance_get_phrase_token",
        Expect::stderr(b""),
        b::phrase_token_null_instance,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}
