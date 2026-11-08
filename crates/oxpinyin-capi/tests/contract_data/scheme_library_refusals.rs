//! Out-of-range scheme and library refusals, their in-range neighbours and empty iterator reads (#525, #681). Generated from the frozen pin observations.

use crate::bodies::scheme_library_refusals as b;
use crate::support::{Expect, run_aborting, run_j};

// abort-bigram-iterator-get-next-phrase: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_bigram_iterator_get_next_phrase() {
    run_aborting(
        "scheme_library_refusals::abort_bigram_iterator_get_next_phrase",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-bigram-iterator-get-next-phrase");
        },
    );
}

// abort-iterator-get-next-phrase: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_iterator_get_next_phrase() {
    run_aborting(
        "scheme_library_refusals::abort_iterator_get_next_phrase",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-iterator-get-next-phrase");
        },
    );
}

// abort-load-phrase-library-0: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_load_phrase_library_0() {
    run_aborting(
        "scheme_library_refusals::abort_load_phrase_library_0",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-load-phrase-library-0");
        },
    );
}

// abort-load-phrase-library-8: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_load_phrase_library_8() {
    run_aborting(
        "scheme_library_refusals::abort_load_phrase_library_8",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-load-phrase-library-8");
        },
    );
}

// abort-set-double-pinyin-scheme-30: the pin's abort, reproduced.
#[test]
fn abort_set_double_pinyin_scheme_30() {
    run_aborting(
        "scheme_library_refusals::abort_set_double_pinyin_scheme_30",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-set-double-pinyin-scheme-30");
        },
    );
}

// abort-set-full-pinyin-scheme-0: the pin's abort, reproduced.
#[test]
fn abort_set_full_pinyin_scheme_0() {
    run_aborting(
        "scheme_library_refusals::abort_set_full_pinyin_scheme_0",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-set-full-pinyin-scheme-0");
        },
    );
}

// abort-set-full-pinyin-scheme-4: the pin's abort, reproduced.
#[test]
fn abort_set_full_pinyin_scheme_4() {
    run_aborting(
        "scheme_library_refusals::abort_set_full_pinyin_scheme_4",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-set-full-pinyin-scheme-4");
        },
    );
}

// abort-set-zhuyin-scheme-0: the pin's abort, reproduced.
#[test]
fn abort_set_zhuyin_scheme_0() {
    run_aborting(
        "scheme_library_refusals::abort_set_zhuyin_scheme_0",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-set-zhuyin-scheme-0");
        },
    );
}

// abort-set-zhuyin-scheme-10: the pin's abort, reproduced.
#[test]
fn abort_set_zhuyin_scheme_10() {
    run_aborting(
        "scheme_library_refusals::abort_set_zhuyin_scheme_10",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-set-zhuyin-scheme-10");
        },
    );
}

// abort-set-zhuyin-scheme-7: the pin's abort, reproduced.
#[test]
fn abort_set_zhuyin_scheme_7() {
    run_aborting(
        "scheme_library_refusals::abort_set_zhuyin_scheme_7",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-set-zhuyin-scheme-7");
        },
    );
}

// abort-unload-addon-phrase-library-16: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_unload_addon_phrase_library_16() {
    run_aborting(
        "scheme_library_refusals::abort_unload_addon_phrase_library_16",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-unload-addon-phrase-library-16");
        },
    );
}

// abort-unload-phrase-library-16: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_unload_phrase_library_16() {
    run_aborting(
        "scheme_library_refusals::abort_unload_phrase_library_16",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-unload-phrase-library-16");
        },
    );
}

// scheme-and-library-neighbours: MATCH.
#[test]
fn scheme_and_library_neighbours() {
    run_j(
        "scheme_library_refusals::scheme_and_library_neighbours",
        Expect::RESULT,
        |k| b::case(k, "scheme-and-library-neighbours"),
        raw!(include!(
            "fixtures/scheme_library_refusals/c1bfdf736ec2862b.rs"
        )),
        None,
    );
}
