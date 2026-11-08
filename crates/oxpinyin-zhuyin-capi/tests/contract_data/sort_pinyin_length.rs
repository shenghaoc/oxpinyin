//! GLib merge-sorted sentence tails, candidate and predicted lists, and the wrapping pinyin-length key (#921, #927, #1275). Generated from the frozen pin observations.

use crate::bodies::sort_pinyin_length as b;
use crate::support::{Expect, run_j};

// glib-sort-candidates-zhuyin: MATCH.
#[test]
fn glib_sort_candidates_zhuyin() {
    run_j(
        "sort_pinyin_length::glib_sort_candidates_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "glib-sort-candidates-zhuyin"),
        raw!(include!("fixtures/sort_pinyin_length/4649f491b573f351.rs")),
        None,
    );
}

// glib-sort-tails-zhuyin: MATCH.
#[test]
fn glib_sort_tails_zhuyin() {
    run_j(
        "sort_pinyin_length::glib_sort_tails_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-sort-tails-zhuyin"),
        raw!(include!("fixtures/sort_pinyin_length/c3e312a30db8ab80.rs")),
        None,
    );
}
