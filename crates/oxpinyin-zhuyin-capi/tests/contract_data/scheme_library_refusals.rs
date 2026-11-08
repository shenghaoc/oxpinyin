//! Out-of-range scheme and library refusals, their in-range neighbours and empty iterator reads (#525, #681). Generated from the frozen pin observations.

use crate::bodies::scheme_library_refusals as b;
use crate::support::{Expect, run_aborting};

// abort-zhuyin-chewing-scheme-0: the pin's abort, reproduced.
#[test]
fn abort_zhuyin_chewing_scheme_0() {
    run_aborting(
        "scheme_library_refusals::abort_zhuyin_chewing_scheme_0",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-chewing-scheme-0");
        },
    );
}

// abort-zhuyin-chewing-scheme-10: the pin's abort, reproduced.
#[test]
fn abort_zhuyin_chewing_scheme_10() {
    run_aborting(
        "scheme_library_refusals::abort_zhuyin_chewing_scheme_10",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-chewing-scheme-10");
        },
    );
}

// abort-zhuyin-chewing-scheme-7: the pin's abort, reproduced.
#[test]
fn abort_zhuyin_chewing_scheme_7() {
    run_aborting(
        "scheme_library_refusals::abort_zhuyin_chewing_scheme_7",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-chewing-scheme-7");
        },
    );
}

// abort-zhuyin-chewing-scheme-minus-1: the pin's abort, reproduced.
#[test]
fn abort_zhuyin_chewing_scheme_minus_1() {
    run_aborting(
        "scheme_library_refusals::abort_zhuyin_chewing_scheme_minus_1",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-chewing-scheme-minus-1");
        },
    );
}

// abort-zhuyin-full-pinyin-scheme-0: the pin's abort, reproduced.
#[test]
fn abort_zhuyin_full_pinyin_scheme_0() {
    run_aborting(
        "scheme_library_refusals::abort_zhuyin_full_pinyin_scheme_0",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-full-pinyin-scheme-0");
        },
    );
}

// abort-zhuyin-full-pinyin-scheme-4: the pin's abort, reproduced.
#[test]
fn abort_zhuyin_full_pinyin_scheme_4() {
    run_aborting(
        "scheme_library_refusals::abort_zhuyin_full_pinyin_scheme_4",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-full-pinyin-scheme-4");
        },
    );
}

// abort-zhuyin-load-phrase-library-0: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_load_phrase_library_0() {
    run_aborting(
        "scheme_library_refusals::abort_zhuyin_load_phrase_library_0",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-load-phrase-library-0");
        },
    );
}

// abort-zhuyin-load-phrase-library-8: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_load_phrase_library_8() {
    run_aborting(
        "scheme_library_refusals::abort_zhuyin_load_phrase_library_8",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-load-phrase-library-8");
        },
    );
}

// abort-zhuyin-unload-phrase-library-16: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_unload_phrase_library_16() {
    run_aborting(
        "scheme_library_refusals::abort_zhuyin_unload_phrase_library_16",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-unload-phrase-library-16");
        },
    );
}

// abort-zhuyin-unload-phrase-library-255: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_unload_phrase_library_255() {
    run_aborting(
        "scheme_library_refusals::abort_zhuyin_unload_phrase_library_255",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-unload-phrase-library-255");
        },
    );
}
