//! The import iterator's phrase-table asserts (#525, register rows 93 and 104). Generated from the frozen pin observations.

use crate::bodies::duplicate_phrase_add as b;
use crate::support::{Expect, run_aborting};

// abort-zhuyin-add-phrase-duplicate-library-token: POLICY-ACCEPTED (pin-abort-assert-line.md).
// The pin asserts at zhuyin.cpp:440.
// Reproduced abort; the pin also wrote glibc's assertion line, which oxpinyin does not
// (pin-abort-assert-line.md).
#[test]
fn abort_zhuyin_add_phrase_duplicate_library_token() {
    run_aborting(
        "duplicate_phrase_add::abort_zhuyin_add_phrase_duplicate_library_token",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::duplicate_library_token,
    );
}

// abort-zhuyin-add-phrase-index-length-mismatch: POLICY-ACCEPTED (pin-abort-assert-line.md).
// The pin asserts at zhuyin.cpp:454.
// Reproduced abort; the pin also wrote glibc's assertion line, which oxpinyin does not
// (pin-abort-assert-line.md).
#[test]
fn abort_zhuyin_add_phrase_index_length_mismatch() {
    run_aborting(
        "duplicate_phrase_add::abort_zhuyin_add_phrase_index_length_mismatch",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::index_length_mismatch,
    );
}

// abort-zhuyin-add-phrase-index-item-missing: POLICY-ACCEPTED (pin-abort-assert-line.md).
// The pin asserts at zhuyin.cpp:454.
// Reproduced abort; the pin also wrote glibc's assertion line, which oxpinyin does not
// (pin-abort-assert-line.md).
#[test]
fn abort_zhuyin_add_phrase_index_text_mismatch() {
    run_aborting(
        "duplicate_phrase_add::abort_zhuyin_add_phrase_index_text_mismatch",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::index_text_mismatch,
    );
}
