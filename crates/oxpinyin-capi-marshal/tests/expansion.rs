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

use oxpinyin_core::PhraseText;

/// Stands for a facade's sentence allocator: the phrases joined into a
/// copy the test can reclaim, or NULL for no phrase, as a sentence whose
/// every conversion failed answers.
fn test_sentence_text(phrases: &[PhraseText]) -> *mut c_char {
    if phrases.is_empty() {
        return ptr::null_mut();
    }
    let joined: String = phrases.iter().filter_map(PhraseText::as_str).collect();
    std::ffi::CString::new(joined).map_or(ptr::null_mut(), std::ffi::CString::into_raw)
}

oxpinyin_capi_marshal::write_owned_sentence!(test_sentence_text);

#[test]
fn sentence_null_out_param_answers_true() {
    assert!(write_owned_sentence(
        &[PhraseText::from("你")],
        ptr::null_mut()
    ));
}

#[test]
fn sentence_null_allocation_still_answers_true() {
    // The pin's `convert_to_utf8` returns true even when its result is NULL.
    let mut out = std::ptr::dangling_mut::<c_char>();
    assert!(write_owned_sentence(&[], &mut out));
    assert!(out.is_null());
}

#[test]
fn sentence_phrases_write_through() {
    let phrases = [
        PhraseText::from("你"),
        PhraseText::from("好"),
        PhraseText::from("x"),
    ];
    let mut out = ptr::null_mut();
    assert!(write_owned_sentence(&phrases, &mut out));
    assert!(!out.is_null());
    // SAFETY: the fixture returned CString::into_raw ownership to `out`.
    let cstr = unsafe { std::ffi::CString::from_raw(out) };
    assert_eq!(cstr.to_bytes(), "你好x".as_bytes());
}
