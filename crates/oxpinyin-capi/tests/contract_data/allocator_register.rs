//! The allocator registers' key-string getters at a zero and a real key (#681). Generated from the frozen pin observations.

use crate::bodies::allocator_register as b;
use crate::support::{Expect, run_j};

// alloc-register-pinyin_get_luoma_pinyin-real-key: MATCH.
#[test]
fn alloc_register_pinyin_get_luoma_pinyin_real_key() {
    run_j(
        "allocator_register::alloc_register_pinyin_get_luoma_pinyin_real_key",
        Expect::RESULT,
        |k| b::case(k, "alloc-register-pinyin_get_luoma_pinyin-real-key"),
        raw!(M(&[
            ("guess_sentence", B(true)),
            ("key_bytes", S("3400")),
            ("key_ret", B(true)),
            ("logs", L(&[])),
            ("out", S("si")),
            ("ret", B(true))
        ])),
        None,
    );
}

// alloc-register-pinyin_get_luoma_pinyin-zero-key: MATCH.
#[test]
fn alloc_register_pinyin_get_luoma_pinyin_zero_key() {
    run_j(
        "allocator_register::alloc_register_pinyin_get_luoma_pinyin_zero_key",
        Expect::RESULT,
        |k| b::case(k, "alloc-register-pinyin_get_luoma_pinyin-zero-key"),
        raw!(M(&[
            ("guess_sentence", B(true)),
            ("key_bytes", S("0000")),
            ("key_ret", B(true)),
            ("logs", L(&[])),
            ("out", Nil),
            ("ret", B(false))
        ])),
        None,
    );
}

// alloc-register-pinyin_get_pinyin-real-key: MATCH.
#[test]
fn alloc_register_pinyin_get_pinyin_real_key() {
    run_j(
        "allocator_register::alloc_register_pinyin_get_pinyin_real_key",
        Expect::RESULT,
        |k| b::case(k, "alloc-register-pinyin_get_pinyin-real-key"),
        raw!(M(&[
            ("guess_sentence", B(true)),
            ("key_bytes", S("3400")),
            ("key_ret", B(true)),
            ("logs", L(&[])),
            ("out", S("xi")),
            ("ret", B(true))
        ])),
        None,
    );
}

// alloc-register-pinyin_get_pinyin-zero-key: MATCH.
#[test]
fn alloc_register_pinyin_get_pinyin_zero_key() {
    run_j(
        "allocator_register::alloc_register_pinyin_get_pinyin_zero_key",
        Expect::RESULT,
        |k| b::case(k, "alloc-register-pinyin_get_pinyin-zero-key"),
        raw!(M(&[
            ("guess_sentence", B(true)),
            ("key_bytes", S("0000")),
            ("key_ret", B(true)),
            ("logs", L(&[])),
            ("out", Nil),
            ("ret", B(false))
        ])),
        None,
    );
}

// alloc-register-pinyin_get_secondary_zhuyin-real-key: MATCH.
#[test]
fn alloc_register_pinyin_get_secondary_zhuyin_real_key() {
    run_j(
        "allocator_register::alloc_register_pinyin_get_secondary_zhuyin_real_key",
        Expect::RESULT,
        |k| b::case(k, "alloc-register-pinyin_get_secondary_zhuyin-real-key"),
        raw!(M(&[
            ("guess_sentence", B(true)),
            ("key_bytes", S("3400")),
            ("key_ret", B(true)),
            ("logs", L(&[])),
            ("out", S("shi")),
            ("ret", B(true))
        ])),
        None,
    );
}

// alloc-register-pinyin_get_secondary_zhuyin-zero-key: MATCH.
#[test]
fn alloc_register_pinyin_get_secondary_zhuyin_zero_key() {
    run_j(
        "allocator_register::alloc_register_pinyin_get_secondary_zhuyin_zero_key",
        Expect::RESULT,
        |k| b::case(k, "alloc-register-pinyin_get_secondary_zhuyin-zero-key"),
        raw!(M(&[
            ("guess_sentence", B(true)),
            ("key_bytes", S("0000")),
            ("key_ret", B(true)),
            ("logs", L(&[])),
            ("out", Nil),
            ("ret", B(false))
        ])),
        None,
    );
}

// alloc-register-pinyin_get_strings-real-key: MATCH.
#[test]
fn alloc_register_pinyin_get_strings_real_key() {
    run_j(
        "allocator_register::alloc_register_pinyin_get_strings_real_key",
        Expect::RESULT,
        |k| b::case(k, "alloc-register-pinyin_get_strings-real-key"),
        raw!(M(&[
            ("guess_sentence", B(true)),
            ("key_bytes", S("3400")),
            ("key_ret", B(true)),
            ("logs", L(&[])),
            ("ret", B(true)),
            ("shengmu", S("x")),
            ("yunmu", S("i"))
        ])),
        None,
    );
}

// alloc-register-pinyin_get_strings-zero-key: MATCH.
#[test]
fn alloc_register_pinyin_get_strings_zero_key() {
    run_j(
        "allocator_register::alloc_register_pinyin_get_strings_zero_key",
        Expect::RESULT,
        |k| b::case(k, "alloc-register-pinyin_get_strings-zero-key"),
        raw!(M(&[
            ("guess_sentence", B(true)),
            ("key_bytes", S("0000")),
            ("key_ret", B(true)),
            ("logs", L(&[])),
            ("ret", B(false)),
            ("shengmu", S("untouched")),
            ("yunmu", S("untouched"))
        ])),
        None,
    );
}

// alloc-register-pinyin_get_zhuyin-real-key: MATCH.
#[test]
fn alloc_register_pinyin_get_zhuyin_real_key() {
    run_j(
        "allocator_register::alloc_register_pinyin_get_zhuyin_real_key",
        Expect::RESULT,
        |k| b::case(k, "alloc-register-pinyin_get_zhuyin-real-key"),
        raw!(M(&[
            ("guess_sentence", B(true)),
            ("key_bytes", S("3400")),
            ("key_ret", B(true)),
            ("logs", L(&[])),
            ("out", S("ㄒㄧ")),
            ("ret", B(true))
        ])),
        None,
    );
}

// alloc-register-pinyin_get_zhuyin-zero-key: MATCH.
#[test]
fn alloc_register_pinyin_get_zhuyin_zero_key() {
    run_j(
        "allocator_register::alloc_register_pinyin_get_zhuyin_zero_key",
        Expect::RESULT,
        |k| b::case(k, "alloc-register-pinyin_get_zhuyin-zero-key"),
        raw!(M(&[
            ("guess_sentence", B(true)),
            ("key_bytes", S("0000")),
            ("key_ret", B(true)),
            ("logs", L(&[])),
            ("out", Nil),
            ("ret", B(false))
        ])),
        None,
    );
}
