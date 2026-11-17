//! The single-key parse + `ChewingKey` display surface (`src/keys.rs`),
//! driven black-box through the re-exported symbols against the `fixtures/w3`
//! mini tables.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use std::ffi::CStr;

use pinyin_capi::{
    ChewingKey, PinyinContext, PinyinInstance, pinyin_fini, pinyin_free_instance,
    pinyin_get_pinyin_string, pinyin_parse_full_pinyin, pinyin_set_options,
};

use crate::common::{TempUserDir, cstr, open};

/// `PINYIN_INCOMPLETE` (`pinyin_custom2.h:34`) — the fixture word: `Fixture::new`
/// sets it explicitly, because `pinyin_init` itself seeds `USE_TONE`.
const PINYIN_INCOMPLETE: u32 = 1 << 3;

/// `USE_TONE` (`pinyin_custom2.h:36`).
const USE_TONE: u32 = 1 << 5;

/// `FORCE_TONE` (`pinyin_custom2.h:37`). The enum in `pinyin.h` carries
/// only the fork-referenced bits, so tests pass the raw word.
const FORCE_TONE: u32 = 1 << 6;

/// Reads a caller-owned rendered string (`g_free`-releasable) and frees it.
fn take(rendered: *mut pinyin_capi::GChar) -> Option<String> {
    if rendered.is_null() {
        return None;
    }
    // SAFETY: The getters return a NUL-terminated UTF-8 buffer or null.
    let text = Some(
        unsafe { CStr::from_ptr(rendered) }
            .to_str()
            .expect("UTF-8 render")
            .to_owned(),
    );
    // SAFETY: The buffer is a returned string this test owns; GLib
    // allocates with malloc, which libc `free` releases.
    unsafe {
        libc_free(rendered.cast());
    }
    text
}

unsafe extern "C" {
    #[link_name = "free"]
    fn libc_free(ptr: *mut core::ffi::c_void);
}

/// A fresh fixture instance under the default parity word
/// (`PINYIN_INCOMPLETE`).
struct Fixture {
    context: *mut PinyinContext,
    instance: *mut PinyinInstance,
    _user_dir: TempUserDir,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let (context, instance, user_dir) = open_with_dir(tag);
        assert!(unsafe { pinyin_set_options(context, PINYIN_INCOMPLETE) });
        Self {
            context,
            instance,
            _user_dir: user_dir,
        }
    }

    fn set_options(&self, options: u32) {
        assert!(unsafe { pinyin_set_options(self.context, options) });
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { pinyin_free_instance(self.instance) };
        unsafe { pinyin_fini(self.context) };
    }
}

fn open_with_dir(tag: &str) -> (*mut PinyinContext, *mut PinyinInstance, TempUserDir) {
    let user_dir = TempUserDir::new(tag);
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    (context, instance, user_dir)
}

/// The tone law: a trailing `1..=5` is the tone under `USE_TONE`;
/// `FORCE_TONE` (nested inside `USE_TONE`) rejects the toneless form.
#[test]
fn full_pinyin_tone_law() {
    let fixture = Fixture::new("keys-tone");
    let mut key = ChewingKey::ZERO;

    // The fixture word (PINYIN_INCOMPLETE) ignores the digit
    // gate: `ni3` is not a spelling, so it fails.
    assert!(!unsafe {
        pinyin_parse_full_pinyin(fixture.instance, cstr("ni3").as_ptr(), &raw mut key)
    });

    fixture.set_options(USE_TONE);
    assert!(unsafe {
        pinyin_parse_full_pinyin(fixture.instance, cstr("ni3").as_ptr(), &raw mut key)
    });
    let mut rendered: *mut pinyin_capi::GChar = std::ptr::null_mut();
    assert!(unsafe { pinyin_get_pinyin_string(fixture.instance, &raw mut key, &raw mut rendered) });
    assert_eq!(take(rendered).as_deref(), Some("ni3"));

    // USE_TONE alone does not demand a tone: `ni` parses with tone zero.
    assert!(unsafe {
        pinyin_parse_full_pinyin(fixture.instance, cstr("ni").as_ptr(), &raw mut key)
    });

    // FORCE_TONE nested inside USE_TONE: the toneless form is refused.
    fixture.set_options(USE_TONE | FORCE_TONE);
    assert!(!unsafe {
        pinyin_parse_full_pinyin(fixture.instance, cstr("ni").as_ptr(), &raw mut key)
    });
    assert!(unsafe {
        pinyin_parse_full_pinyin(fixture.instance, cstr("ni3").as_ptr(), &raw mut key)
    });

    // FORCE_TONE without USE_TONE is inert (the pin's nesting).
    fixture.set_options(FORCE_TONE);
    assert!(unsafe {
        pinyin_parse_full_pinyin(fixture.instance, cstr("ni").as_ptr(), &raw mut key)
    });
}
