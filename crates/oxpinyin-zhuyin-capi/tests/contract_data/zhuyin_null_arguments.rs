//! NULL arguments the zhuyin pin dereferences (#563, register row 41). Generated from the frozen pin observations.

use crate::bodies::zhuyin_null_arguments::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};

// zhuyin-add-phrase-library-16: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:392-398, :475; phrase_index.cpp:611.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_add_phrase_library_16() {
    run(
        "zhuyin_null_arguments::zhuyin_add_phrase_library_16",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::add_phrase_library_16,
        Ret { ret: false },
        None,
    );
}

// zhuyin-null-text-parse-chewing: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1052.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_text_parse_chewing() {
    run(
        "zhuyin_null_arguments::zhuyin_null_text_parse_chewing",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::parse_chewing_null_text,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-key-parse-chewing: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1053-1054.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_key_parse_chewing() {
    run(
        "zhuyin_null_arguments::zhuyin_null_key_parse_chewing",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::parse_chewing_null_key,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-symbols-in-chewing-keyboard: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1093.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_symbols_in_chewing_keyboard() {
    run(
        "zhuyin_null_arguments::zhuyin_null_symbols_in_chewing_keyboard",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::in_chewing_null_symbols,
        Ret { ret: true },
        Some(NO_LOGS),
    );
}

// zhuyin-null-instance-get-pinyin-string: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1746-1747.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_instance_get_pinyin_string() {
    run(
        "zhuyin_null_arguments::zhuyin_null_instance_get_pinyin_string",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::pinyin_string_null_instance,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-key-get-zhuyin-string: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1736.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_key_get_zhuyin_string() {
    run(
        "zhuyin_null_arguments::zhuyin_null_key_get_zhuyin_string",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::zhuyin_string_null_key,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-out-get-zhuyin-string: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1735.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_out_get_zhuyin_string() {
    run(
        "zhuyin_null_arguments::zhuyin_null_out_get_zhuyin_string",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::zhuyin_string_null_out,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-rest-get-zhuyin-key-rest-positions: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1933-1934.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_rest_get_zhuyin_key_rest_positions() {
    run(
        "zhuyin_null_arguments::zhuyin_null_rest_get_zhuyin_key_rest_positions",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::key_rest_positions_null_rest,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-rest-get-zhuyin-key-rest-length: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1945; chewing_key.h:111-112.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_rest_get_zhuyin_key_rest_length() {
    run(
        "zhuyin_null_arguments::zhuyin_null_rest_get_zhuyin_key_rest_length",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::key_rest_length_null_rest,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-length-get-zhuyin-key-rest-length: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1945.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_length_get_zhuyin_key_rest_length() {
    run(
        "zhuyin_null_arguments::zhuyin_null_length_get_zhuyin_key_rest_length",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::key_rest_length_null_length,
        Ret { ret: true },
        Some(NO_LOGS),
    );
}

// zhuyin-null-candidate-get-candidate-type: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1861.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_candidate_get_candidate_type() {
    run(
        "zhuyin_null_arguments::zhuyin_null_candidate_get_candidate_type",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::candidate_type_null_candidate,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-out-get-candidate-type: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1861.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_out_get_candidate_type() {
    run(
        "zhuyin_null_arguments::zhuyin_null_out_get_candidate_type",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::candidate_type_null_out,
        Ret { ret: true },
        Some(NO_LOGS),
    );
}

// zhuyin-null-candidate-get-candidate-string: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1868.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_candidate_get_candidate_string() {
    run(
        "zhuyin_null_arguments::zhuyin_null_candidate_get_candidate_string",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::candidate_string_null_candidate,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}

// zhuyin-null-out-get-candidate-string: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:1868.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_out_get_candidate_string() {
    run(
        "zhuyin_null_arguments::zhuyin_null_out_get_candidate_string",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::candidate_string_null_out,
        Ret { ret: true },
        Some(NO_LOGS),
    );
}

// zhuyin-null-out-get-n-phrase: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:2229.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_out_get_n_phrase() {
    run(
        "zhuyin_null_arguments::zhuyin_null_out_get_n_phrase",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::n_phrase_null_out,
        Ret { ret: true },
        Some(NO_LOGS),
    );
}

// zhuyin-null-out-get-phrase-token: MATCH.
// The pin reads the NULL argument at zhuyin.cpp:2238.
// The pin dies of SIGSEGV after this stderr; oxpinyin answers the declared value (class (b)).
#[test]
fn zhuyin_null_out_get_phrase_token() {
    run(
        "zhuyin_null_arguments::zhuyin_null_out_get_phrase_token",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::phrase_token_null_out,
        Ret { ret: false },
        Some(NO_LOGS),
    );
}
