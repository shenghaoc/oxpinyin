//! Integration tests for the two `macro_rules!` macros exported by
//! `oxpinyin-capi-marshal`.
//!
// The macro bodies stamp `unsafe` blocks (pointer casts, `Box::from_raw`),
// and the tests themselves call `unsafe` functions / reclaim pointers.
// The library crate forbids unsafe_code; the workspace denies it.
// Integration tests are separate compilation units, so this allow is scoped
// to the test binary only.
#![allow(unsafe_code)]
//!
//! These tests live in `tests/` (a separate compilation unit) because the
//! macro bodies stamp `unsafe` blocks that are type-checked in the
//! *expanding* crate. The library crate itself is `#![forbid(unsafe_code)]`,
//! so the expansion cannot happen there.

use std::os::raw::c_char;
use std::ptr;

// ── Fixture types for `opaque_handle_casts!` ───────────────────────────

/// Zero-sized opaque marker, like the real `PinyinContext`.
#[repr(C)]
struct TestContext;

/// The backing data behind `TestContext *`.
struct ContextBacking {
    locale: String,
    count: u32,
}

/// Zero-sized opaque marker, like the real `PinyinInstance`.
#[repr(C)]
struct TestInstance;

/// The backing data behind `TestInstance *`.
struct InstanceBacking {
    cursor: usize,
}

/// Zero-sized opaque marker, like the real `LookupCandidate`.
#[repr(C)]
struct TestCandidate;

/// The backing data behind `TestCandidate *`.
struct CandidateBacking {
    text: String,
    score: f64,
}

// Stamp the helpers.
oxpinyin_capi_marshal::opaque_handle_casts! {
    vis: pub,
    context: TestContext => ContextBacking,
    instance: TestInstance => InstanceBacking,
    candidate: TestCandidate => CandidateBacking,
}

// ── opaque_handle_casts: context helpers ───────────────────────────────

#[test]
fn context_round_trip() {
    let ptr = box_context(ContextBacking {
        locale: "zh_CN".into(),
        count: 42,
    });
    assert!(!ptr.is_null());

    // SAFETY: `ptr` was just produced by `box_context`.
    let r = unsafe { context_ref(ptr) };
    assert_eq!(r.locale, "zh_CN");
    assert_eq!(r.count, 42);

    // Reclaim to avoid a leak.
    // SAFETY: `ptr` was produced by `box_context` and has not been freed.
    let _ = unsafe { Box::from_raw(ptr.cast::<ContextBacking>()) };
}

#[test]
fn context_mut_modification() {
    let ptr = box_context(ContextBacking {
        locale: "en_US".into(),
        count: 0,
    });

    // SAFETY: `ptr` was just produced by `box_context`.
    let m = unsafe { context_mut(ptr) };
    m.count = 99;
    m.locale = "zh_TW".into();

    // SAFETY: same pointer, no other references alive.
    let r = unsafe { context_ref(ptr) };
    assert_eq!(r.count, 99);
    assert_eq!(r.locale, "zh_TW");

    // Reclaim.
    let _ = unsafe { Box::from_raw(ptr.cast::<ContextBacking>()) };
}

// ── opaque_handle_casts: instance helpers ──────────────────────────────

#[test]
fn instance_round_trip() {
    let ptr = box_instance(InstanceBacking { cursor: 7 });
    assert!(!ptr.is_null());

    // SAFETY: `ptr` was just produced by `box_instance`.
    let r = unsafe { instance_ref(ptr) };
    assert_eq!(r.cursor, 7);

    let _ = unsafe { Box::from_raw(ptr.cast::<InstanceBacking>()) };
}

#[test]
fn instance_mut_modification() {
    let ptr = box_instance(InstanceBacking { cursor: 0 });

    // SAFETY: `ptr` was just produced by `box_instance`.
    let m = unsafe { instance_mut(ptr) };
    m.cursor = 123;

    let r = unsafe { instance_ref(ptr) };
    assert_eq!(r.cursor, 123);

    let _ = unsafe { Box::from_raw(ptr.cast::<InstanceBacking>()) };
}

// ── opaque_handle_casts: candidate helpers ─────────────────────────────

#[test]
fn candidate_ptr_ref_round_trip() {
    let backing = CandidateBacking {
        text: "hello".into(),
        score: 0.95,
    };

    let ptr = candidate_ptr(&backing);
    assert!(!ptr.is_null());

    // SAFETY: `ptr` points to the live `backing` on the stack.
    let r = unsafe { candidate_ref(ptr) };
    assert_eq!(r.text, "hello");
    assert!((r.score - 0.95).abs() < f64::EPSILON);
}

#[test]
fn candidate_ptr_into_vec() {
    // Reproduces the real usage pattern: candidates live in a Vec, and
    // `candidate_ptr` hands out pointers into it.
    let candidates = [
        CandidateBacking {
            text: "a".into(),
            score: 1.0,
        },
        CandidateBacking {
            text: "b".into(),
            score: 2.0,
        },
        CandidateBacking {
            text: "c".into(),
            score: 3.0,
        },
    ];

    for (i, cand) in candidates.iter().enumerate() {
        let ptr = candidate_ptr(cand);
        // SAFETY: `ptr` points into the live `candidates` vec.
        let r = unsafe { candidate_ref(ptr) };
        assert_eq!(r.text, ["a", "b", "c"][i]);
    }
}

// ── Fixture for `write_owned_sentence!` ────────────────────────────────

/// A test stand-in for `crate::ffi::owned_cstr`: allocates a `CString`
/// copy via `Box` (not libc `malloc`, since the test frees with
/// `CString::from_raw` rather than `g_free`). Returns null on an interior
/// NUL byte, mirroring the real function's contract.
fn test_owned_cstr(s: &str) -> *mut c_char {
    match std::ffi::CString::new(s) {
        Ok(cstr) => {
            // Leak into a raw pointer the caller owns.
            cstr.into_raw()
        }
        Err(_) => ptr::null_mut(),
    }
}

// Stamp the helper.
oxpinyin_capi_marshal::write_owned_sentence!(test_owned_cstr);

// ── write_owned_sentence tests ─────────────────────────────────────────

#[test]
fn sentence_empty_text_returns_false_and_nulls_out() {
    let mut out: *mut c_char = std::ptr::dangling_mut::<c_char>();
    let ok = write_owned_sentence("", &mut out);
    assert!(!ok);
    assert!(out.is_null(), "out-param must be nulled on empty text");
}

#[test]
fn sentence_empty_text_null_out_param() {
    // Null sentence pointer with empty text: must not crash.
    let ok = write_owned_sentence("", ptr::null_mut());
    assert!(!ok);
}

#[test]
fn sentence_valid_text_writes_through() {
    let mut out: *mut c_char = ptr::null_mut();
    let ok = write_owned_sentence("hello", &mut out);
    assert!(ok);
    assert!(!out.is_null());

    // Read back and verify.
    // SAFETY: `out` was produced by `test_owned_cstr` (a `CString::into_raw`).
    let cstr = unsafe { std::ffi::CString::from_raw(out) };
    assert_eq!(cstr.to_str().unwrap(), "hello");
}

#[test]
fn sentence_valid_text_null_out_param() {
    // Null sentence pointer with valid text: returns true, no crash.
    let ok = write_owned_sentence("hello", ptr::null_mut());
    assert!(ok);
}

#[test]
fn sentence_interior_nul_returns_false() {
    let mut out: *mut c_char = std::ptr::dangling_mut::<c_char>();
    let ok = write_owned_sentence("hel\0lo", &mut out);
    assert!(!ok);
    assert!(out.is_null(), "out-param must be nulled on interior NUL");
}

#[test]
fn sentence_unicode_text() {
    let mut out: *mut c_char = ptr::null_mut();
    let ok = write_owned_sentence("nihao", &mut out);
    assert!(ok);

    // SAFETY: produced by `CString::into_raw`.
    let cstr = unsafe { std::ffi::CString::from_raw(out) };
    assert_eq!(cstr.to_str().unwrap(), "nihao");
}

#[test]
fn sentence_cjk_text() {
    let mut out: *mut c_char = ptr::null_mut();
    let ok = write_owned_sentence("\u{4f60}\u{597d}", &mut out);
    assert!(ok);
    assert!(!out.is_null());

    let cstr = unsafe { std::ffi::CString::from_raw(out) };
    assert_eq!(cstr.to_str().unwrap(), "\u{4f60}\u{597d}");
}

#[test]
fn sentence_single_byte_text() {
    let mut out: *mut c_char = ptr::null_mut();
    let ok = write_owned_sentence("x", &mut out);
    assert!(ok);

    let cstr = unsafe { std::ffi::CString::from_raw(out) };
    assert_eq!(cstr.to_str().unwrap(), "x");
}
