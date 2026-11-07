//! Caller-owned GArrays (#924) and the phrase-segment critical (#1297). Generated from the frozen pin observations.

use crate::bodies::glib_arrays::{self as b, *};
use crate::support::{Expect, FATAL_CRITICALS, Log, NO_LOGS, run};

// glib-924-array-lifecycle-pinyin: MATCH.
// GLib garray.c keeps the caller's metadata and runs its clear callback on a shrink.
#[test]
fn glib_924_array_lifecycle_pinyin() {
    run(
        "glib_arrays::glib_924_array_lifecycle_pinyin",
        Expect::RESULT,
        b::array_lifecycle,
        include!("fixtures/glib_arrays/7d9a90ce3d3995ff.rs"),
        Some(NO_LOGS),
    );
}

// glib-924-segment-critical-pinyin: POLICY-ACCEPTED.
// Policy: docs/findings/1297-segment-critical.md. The pin logs [None, 8] "... assertion
// 'num_of_chars == ucs4_len' failed" (pinyin.cpp:1450-1454); oxpinyin logs nothing.
#[test]
fn glib_924_segment_critical_pinyin() {
    run(
        "glib_arrays::glib_924_segment_critical_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::segment_critical,
        Segment {
            ret: false,
            retained: vec![0x1006205, 0],
            messages: vec![],
        },
        Some(NO_LOGS),
    );
}

// glib-924-segment-fatal-critical-pinyin: POLICY-ACCEPTED.
// Policy: docs/findings/1297-segment-critical.md. Under G_DEBUG=fatal-criticals the pin dies
// of SIGTRAP at that critical (pinyin.cpp:1450-1454); oxpinyin logs nothing and returns false.
#[test]
fn glib_924_segment_fatal_critical_pinyin() {
    run(
        "glib_arrays::glib_924_segment_fatal_critical_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_env(FATAL_CRITICALS),
        b::segment_critical,
        Segment {
            ret: false,
            retained: vec![0x1006205, 0],
            messages: vec![],
        },
        Some(NO_LOGS),
    );
}

// glib-924-null-array-lookup-pinyin: MATCH.
// GLib's criticals for a NULL array (garray.c:611, :778); the facade ignores the NULL returns
// (pinyin.cpp:2805-2818, phrase_large_table3.h:77-97).
#[test]
fn glib_924_null_array_lookup_pinyin() {
    run(
        "glib_arrays::glib_924_null_array_lookup_pinyin",
        Expect::RESULT,
        |k| b::null_array(k, false),
        include!("fixtures/glib_arrays/81787019cbd2092f.rs"),
        Some(&[
            Log(Some("GLib"), 8),
            Log(Some("GLib"), 8),
            Log(Some("GLib"), 8),
            Log(Some("GLib"), 8),
            Log(Some("GLib"), 8),
            Log(Some("GLib"), 8),
            Log(Some("GLib"), 8),
            Log(Some("GLib"), 8),
        ]),
    );
}

// glib-924-null-array-pronunciation-pinyin: MATCH.
#[test]
fn glib_924_null_array_pronunciation_pinyin() {
    run(
        "glib_arrays::glib_924_null_array_pronunciation_pinyin",
        Expect::RESULT,
        |k| b::null_array(k, true),
        include!("fixtures/glib_arrays/2f2cbcb3845eefd9.rs"),
        Some(&[Log(Some("GLib"), 8)]),
    );
}
