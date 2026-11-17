//! Integration tests for the six `macro_rules!` macros exported by
//! `oxpinyin-capi-marshal`.
//!
// The macro bodies stamp `unsafe` blocks (pointer casts, `Box::from_raw`),
// and the tests themselves call `unsafe` functions / reclaim pointers.
// The library crate forbids unsafe_code; the workspace denies it.
// Integration tests are separate compilation units, so this expectation is scoped
// to the test binary only.
#![expect(
    unsafe_code,
    reason = "the macros expand unsafe blocks here and the tests reclaim their pointers"
)]
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
    // SAFETY: `ptr` was produced by `box_context`, has not been freed, and
    // no reference to it is used again.
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

    // SAFETY: `ptr` was produced by `box_instance`, has not been freed, and
    // no reference to it is used again.
    let _ = unsafe { Box::from_raw(ptr.cast::<InstanceBacking>()) };
}

#[test]
fn instance_mut_modification() {
    let ptr = box_instance(InstanceBacking { cursor: 0 });

    // SAFETY: `ptr` was just produced by `box_instance`.
    let m = unsafe { instance_mut(ptr) };
    m.cursor = 123;

    // SAFETY: same pointer; `m` is not used again.
    let r = unsafe { instance_ref(ptr) };
    assert_eq!(r.cursor, 123);

    // SAFETY: `ptr` was produced by `box_instance`, has not been freed, and
    // no reference to it is used again.
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

// ── Fixture for `write_owned_sentence!` ────────────────────────────────

use oxpinyin_core::PhraseText;

/// Stands for a facade's sentence allocator: the phrases joined into a
/// copy the test can reclaim, or NULL for no phrase, as a sentence whose
/// every conversion failed answers.
fn test_sentence_text(
    phrases: impl IntoIterator<Item = Option<impl core::borrow::Borrow<PhraseText>>>,
) -> *mut c_char {
    let phrases: Vec<_> = phrases.into_iter().flatten().collect();
    if phrases.is_empty() {
        return ptr::null_mut();
    }
    let joined: String = phrases
        .iter()
        .filter_map(|phrase| phrase.borrow().as_str())
        .collect();
    std::ffi::CString::new(joined).map_or(ptr::null_mut(), std::ffi::CString::into_raw)
}

oxpinyin_capi_marshal::write_owned_sentence!(test_sentence_text);

#[test]
fn sentence_null_out_param_answers_true() {
    assert!(write_owned_sentence(
        [Some(PhraseText::from("你"))],
        ptr::null_mut()
    ));
}

#[test]
fn sentence_null_allocation_still_answers_true() {
    // The pin's `convert_to_utf8` returns true even when its result is NULL.
    let mut out = std::ptr::dangling_mut::<c_char>();
    assert!(write_owned_sentence([None::<PhraseText>], &mut out));
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
    assert!(write_owned_sentence(phrases.iter().map(Some), &mut out));
    assert!(!out.is_null());
    // SAFETY: the fixture returned CString::into_raw ownership to `out`.
    let cstr = unsafe { std::ffi::CString::from_raw(out) };
    assert_eq!(cstr.to_bytes(), "你好x".as_bytes());
}

// The new macro expansions are checked in this separate compilation unit.
mod ffi_output_expansion {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_int, c_void};
    use std::ptr;

    use oxpinyin_core::{KeyText, PhraseText};

    #[allow(non_upper_case_globals)]
    mod glib {
        use super::{c_char, c_int, c_void};
        pub const GTRUE: c_int = 1;
        pub const GFALSE: c_int = 0;
        pub const G_LOG_LEVEL_WARNING: c_int = 1 << 4;
        #[repr(C)]
        pub struct GPtrArray {
            _private: [u8; 0],
        }
        #[link(name = "glib-2.0")]
        unsafe extern "C" {
            pub fn g_strdup(text: *const c_char) -> *mut c_char;
            pub fn g_strdup_printf(format: *const c_char, ...) -> *mut c_char;
            pub fn g_ptr_array_new() -> *mut GPtrArray;
            pub fn g_ptr_array_add(array: *mut GPtrArray, data: *mut c_void);
            pub fn g_ptr_array_free(array: *mut GPtrArray, free_segment: c_int) -> *mut c_void;
            pub fn g_strconcat(first: *const c_char, ...) -> *mut c_char;
            pub fn g_free(ptr: *mut c_void);
            pub fn g_strfreev(ptr: *mut *mut c_char);
            pub fn g_log(domain: *const c_char, level: c_int, format: *const c_char, ...);
        }
    }
    unsafe extern "C" {
        fn malloc(size: usize) -> *mut c_void;
        fn free(ptr: *mut c_void);
    }
    oxpinyin_capi_marshal::ffi_output_helpers! {
        glib: glib, key_text: KeyText, malloc: malloc, free: free, domain: c"libpinyin"
    }

    #[test]
    fn ffi_output_helpers_preserve_ownership_and_bytes() {
        let duplicated = strdup("abc");
        // SAFETY: The macro returned a NUL-terminated GLib string.
        assert_eq!(unsafe { CStr::from_ptr(duplicated) }.to_bytes(), b"abc");
        // SAFETY: `g_free` owns the returned `g_strdup` allocation.
        unsafe { glib::g_free(duplicated.cast()) };
        // SAFETY: GLib accepts a NULL string and returns NULL.
        assert!(unsafe { strdup_raw(ptr::null()) }.is_null());

        let key = key_text(KeyText::Digit("a", 3));
        // SAFETY: The macro returned a NUL-terminated GLib string.
        assert_eq!(unsafe { CStr::from_ptr(key) }.to_bytes(), b"a3");
        // SAFETY: `g_free` owns the returned key string.
        unsafe { glib::g_free(key.cast()) };

        let vector = symbol_vector(&["one", "two"]);
        // SAFETY: `symbol_vector` returns a NULL-terminated GLib vector.
        assert_eq!(unsafe { CStr::from_ptr(*vector.add(1)) }.to_bytes(), b"two");
        // SAFETY: The vector and elements belong to `g_strfreev`.
        unsafe { glib::g_strfreev(vector) };
        assert!(symbol_vector(&[] as &[&str]).is_null());

        let sentence = sentence_text([Some(PhraseText::from("你")), Some(PhraseText::from("好"))]);
        // SAFETY: The macro returned a NUL-terminated GLib string.
        let sentence_bytes = unsafe { CStr::from_ptr(sentence) }.to_bytes();
        assert_eq!(sentence_bytes, "你好".as_bytes());
        // SAFETY: `g_free` releases the final GLib allocation.
        unsafe { glib::g_free(sentence.cast()) };

        let filled = malloc_cstr_with(3, |out| {
            out.copy_from_slice(b"abc");
            3
        });
        // SAFETY: The macro returned a NUL-terminated malloc string.
        assert_eq!(unsafe { CStr::from_ptr(filled) }.to_bytes(), b"abc");
        // SAFETY: `free` pairs with the macro's malloc.
        unsafe { free(filled.cast()) };
        assert!(
            malloc_cstr_with(1, |out| {
                out[0] = 0;
                1
            })
            .is_null()
        );
        log_warning("a\0b"); // Interior NUL is dropped before GLib sees it.
    }
}

mod display_getter_expansion {
    use oxpinyin_core::{ChewingKey as CoreKey, KeyText};
    use std::os::raw::c_char;
    use std::ptr;
    #[derive(Clone, Copy)]
    struct ChewingKey(CoreKey);
    impl ChewingKey {
        fn to_core(self) -> CoreKey {
            self.0
        }
    }
    type GChar = c_char;
    fn key_text(_: KeyText) -> *mut GChar {
        c"rendered".as_ptr().cast_mut()
    }
    oxpinyin_capi_marshal::display_string_getter! { key: ChewingKey, char: GChar }
    #[test]
    fn display_getter_checks_zero_and_writes_rendering() {
        let mut zero = ChewingKey(CoreKey::ZERO);
        let mut out = ptr::dangling_mut::<GChar>();
        assert!(!display_string_getter(&mut zero, &mut out, |_| {
            KeyText::Plain("x")
        }));
        assert!(out.is_null());
        let mut key = ChewingKey(CoreKey::new(1, 0, 0, 0));
        assert!(display_string_getter(&mut key, &mut out, |_| {
            KeyText::Plain("x")
        }));
        assert_eq!(out, c"rendered".as_ptr().cast_mut());
    }
}

mod append_words_expansion {
    use std::os::raw::{c_uint, c_void};
    struct GArray(Vec<u16>);
    /// Copies `length` native-endian packed words from the source bytes.
    ///
    /// # Safety
    /// `keys` must be a live array, and `data` must point to `length * 2`
    /// readable bytes for the duration of this call.
    unsafe fn append(keys: *mut GArray, data: *const c_void, length: c_uint) {
        // SAFETY: The macro test passes a live array exclusively for this call.
        let array = unsafe { &mut *keys };
        // SAFETY: The macro supplies `length` packed words as readable bytes;
        // byte alignment is sufficient for a `u8` slice.
        let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length as usize * 2) };
        array.0.extend(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_ne_bytes),
        );
    }
    oxpinyin_capi_marshal::append_stored_words! { array: GArray, append: append }
    #[test]
    fn append_words_copies_reading_or_zeroes() {
        let mut array = GArray(Vec::new());
        let words = [1_u16, 2];
        let stored: Vec<u8> = words.into_iter().flat_map(u16::to_ne_bytes).collect();
        // SAFETY: The test array is live with two-byte elements.
        unsafe { append_stored_words(&mut array, 2, Some(&stored)) };
        assert_eq!(array.0, [1, 2]);
        // SAFETY: The same array remains live with two-byte elements.
        unsafe { append_stored_words(&mut array, 2, None) };
        assert_eq!(array.0, [1, 2, 0, 0]);
    }
}

mod state_handle_expansion {
    use std::{cell::UnsafeCell, rc::Rc};
    struct ContextMarker;
    struct InstanceMarker;
    #[allow(dead_code)]
    struct CandidateMarker;
    #[allow(dead_code)]
    struct Candidate;
    struct Runtime {
        runtime: u8,
    }
    impl Runtime {
        fn train(&mut self, _: &InstanceCore, index: u8) -> Result<bool, ()> {
            self.runtime = index;
            Ok(true)
        }
    }
    struct CapiContext {
        core: Runtime,
    }
    type ContextOwner = Rc<UnsafeCell<CapiContext>>;
    struct InstanceCore;
    impl InstanceCore {
        fn read<'a>(&'a self, _: &'a u8) -> &'a Self {
            self
        }
        fn bind<'a>(&'a mut self, _: &'a u8) -> &'a mut Self {
            self
        }
    }
    struct CapiInstance<C = InstanceCore, S = u8, O = ContextOwner> {
        owner: O,
        core: C,
        state: S,
    }
    type CapiInstanceRead<'a> = CapiInstance<&'a InstanceCore, &'a u8, &'a ContextOwner>;
    type CapiInstanceMut<'a> = CapiInstance<&'a mut InstanceCore, &'a mut u8, &'a ContextOwner>;
    mod handles {
        use super::*;
        oxpinyin_capi_marshal::opaque_handle_casts! {
            vis: pub(super),
            context: ContextMarker => ContextOwner,
            instance: InstanceMarker => CapiInstance,
            candidate: CandidateMarker => Candidate,
        }
    }
    oxpinyin_capi_marshal::state_handle_helpers! {
        context: ContextMarker, instance: InstanceMarker, instance_binding: instance,
        error: (), context_field: {}
    }
    #[test]
    fn state_handles_retain_context_and_borrow_instance() {
        let context = box_context(CapiContext {
            core: Runtime { runtime: 1 },
        });
        // SAFETY: `context` is a live handle produced above.
        let owner = unsafe { context_owner(context) };
        let instance = handles::box_instance(CapiInstance {
            owner,
            core: InstanceCore,
            state: 7,
        });
        // SAFETY: The context handle is live and no other access overlaps.
        assert_eq!(unsafe { context_ref(context) }.core.runtime, 1);
        // SAFETY: The instance now retains the context allocation through its owner.
        unsafe { drop(Box::from_raw(context.cast::<ContextOwner>())) };
        // SAFETY: The instance handle is live and no other access overlaps.
        assert_eq!(*unsafe { instance_ref(instance) }.state, 7);
        // SAFETY: The instance handle is live and no other access overlaps.
        *unsafe { instance_mut(instance) }.state = 9;
        // SAFETY: The instance handle is live and no other access overlaps.
        assert_eq!(*unsafe { instance_ref(instance) }.state, 9);
        // SAFETY: The instance handle is live and no other access overlaps.
        assert_eq!(unsafe { train_instance(instance, 3) }, Ok(true));
        // SAFETY: The live instance retains the context; no mutation overlaps.
        let view = unsafe { instance_ref(instance) };
        // SAFETY: The instance owner retains the allocation for this read.
        assert_eq!(unsafe { owner_ref(view.owner) }.core.runtime, 3);
        // SAFETY: The instance handle was boxed above and is no longer borrowed.
        unsafe {
            drop(Box::from_raw(instance.cast::<CapiInstance>()));
        }
    }
}
