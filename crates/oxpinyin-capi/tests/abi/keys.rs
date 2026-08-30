//! The single-key parse + `ChewingKey` display surface (`src/keys.rs`),
//! driven black-box through the re-exported symbols against the `fixtures/w3`
//! mini tables.

use std::ffi::CStr;

use pinyin_capi::{
    ChewingKey, PinyinContext, PinyinInstance, pinyin_fini, pinyin_free_instance,
    pinyin_get_context, pinyin_get_luoma_pinyin_string, pinyin_get_pinyin_is_incomplete,
    pinyin_get_pinyin_string, pinyin_get_pinyin_strings, pinyin_get_secondary_zhuyin_string,
    pinyin_get_zhuyin_string, pinyin_parse_chewing, pinyin_parse_double_pinyin,
    pinyin_parse_full_pinyin, pinyin_set_double_pinyin_scheme, pinyin_set_options,
    pinyin_set_zhuyin_scheme, pinyin_unload_addon_phrase_library,
};

use crate::common::{TempUserDir, cstr, open};

/// `PINYIN_INCOMPLETE` (`pinyin_custom2.h:34`) — the fixture default word.
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
    // SAFETY: The buffer came from the capi's libc-malloc `owned_cstr`.
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
        Self {
            context,
            instance,
            _user_dir: user_dir,
        }
    }

    fn set_options(&self, options: u32) {
        assert!(pinyin_set_options(self.context, options));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        pinyin_free_instance(self.instance);
        pinyin_fini(self.context);
    }
}

fn open_with_dir(tag: &str) -> (*mut PinyinContext, *mut PinyinInstance, TempUserDir) {
    let user_dir = TempUserDir::new(tag);
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    (context, instance, user_dir)
}

/// The layout pin behind the checked-in header: the packed two-byte key
/// word and the four-byte raw span.
#[test]
fn abi_layout_is_pinned() {
    assert_eq!(size_of::<ChewingKey>(), 2);
    assert_eq!(align_of::<ChewingKey>(), 2);
    assert_eq!(
        size_of::<pinyin_capi::ChewingKeyRest>(),
        4,
        "m_raw_begin + m_raw_end"
    );
}

/// The full-pinyin one-key surface: `ni` parses and renders, the
/// whole-input probe refuses `nihao` and apostrophes, and a failed parse
/// leaves the ZEROED key (upstream zeroes `*onekey` before the probe).
#[test]
fn full_pinyin_one_key_surface() {
    let fixture = Fixture::new("keys-full");

    let mut key = ChewingKey::ZERO;
    assert!(pinyin_parse_full_pinyin(
        fixture.instance,
        cstr("ni").as_ptr(),
        &mut key
    ));
    assert_eq!(key.packed, 43, "initial n(11) | medial i(1) packing");

    let mut rendered: *mut pinyin_capi::GChar = std::ptr::null_mut();
    assert!(pinyin_get_pinyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("ni"));
    assert!(pinyin_get_zhuyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("ㄋㄧ"));
    assert!(pinyin_get_luoma_pinyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("ni"));
    assert!(pinyin_get_secondary_zhuyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(
        take(rendered).as_deref(),
        Some("ni"),
        "secondary zhuyin for ni is ASCII"
    );

    // The whole input is the probe: no forward maximum match.
    let mut key = ChewingKey::ZERO;
    assert!(!pinyin_parse_full_pinyin(
        fixture.instance,
        cstr("nihao").as_ptr(),
        &mut key
    ));
    assert_eq!(key.packed, 0, "failed full-pinyin parse zeroes the key");

    // Apostrophes are refused (the pin asserts on them).
    assert!(!pinyin_parse_full_pinyin(
        fixture.instance,
        cstr("ni'hao").as_ptr(),
        &mut key
    ));

    // Initial-only keys parse and render; is_incomplete answers true.
    assert!(pinyin_parse_full_pinyin(
        fixture.instance,
        cstr("b").as_ptr(),
        &mut key
    ));
    assert!(pinyin_get_pinyin_is_incomplete(fixture.instance, &mut key));
    assert!(pinyin_get_pinyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("b"));
    // Luoma/secondary for an initial-only key are the pin's "None" rows.
    assert!(pinyin_get_luoma_pinyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("None"));
}

/// The tone law: a trailing `1..=5` is the tone under `USE_TONE`;
/// `FORCE_TONE` (nested inside `USE_TONE`) rejects the toneless form.
#[test]
fn full_pinyin_tone_law() {
    let fixture = Fixture::new("keys-tone");
    let mut key = ChewingKey::ZERO;

    // The default fixture word (PINYIN_INCOMPLETE) ignores the digit
    // gate: `ni3` is not a spelling, so it fails.
    assert!(!pinyin_parse_full_pinyin(
        fixture.instance,
        cstr("ni3").as_ptr(),
        &mut key
    ));

    fixture.set_options(USE_TONE);
    assert!(pinyin_parse_full_pinyin(
        fixture.instance,
        cstr("ni3").as_ptr(),
        &mut key
    ));
    let mut rendered: *mut pinyin_capi::GChar = std::ptr::null_mut();
    assert!(pinyin_get_pinyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("ni3"));

    // USE_TONE alone does not demand a tone: `ni` parses with tone zero.
    assert!(pinyin_parse_full_pinyin(
        fixture.instance,
        cstr("ni").as_ptr(),
        &mut key
    ));

    // FORCE_TONE nested inside USE_TONE: the toneless form is refused.
    fixture.set_options(USE_TONE | FORCE_TONE);
    assert!(!pinyin_parse_full_pinyin(
        fixture.instance,
        cstr("ni").as_ptr(),
        &mut key
    ));
    assert!(pinyin_parse_full_pinyin(
        fixture.instance,
        cstr("ni3").as_ptr(),
        &mut key
    ));

    // FORCE_TONE without USE_TONE is inert (the pin's nesting).
    fixture.set_options(FORCE_TONE);
    assert!(pinyin_parse_full_pinyin(
        fixture.instance,
        cstr("ni").as_ptr(),
        &mut key
    ));
}

/// The double-pinyin one-key surface (MS default): `ni` resolves, tone
/// rides a three-byte input only under `USE_TONE`, `FORCE_TONE` demands
/// three bytes, and a failed parse leaves the caller's key UNTOUCHED.
#[test]
fn double_pinyin_one_key_surface() {
    let fixture = Fixture::new("keys-double");
    assert!(pinyin_set_double_pinyin_scheme(fixture.context, 2));

    let mut key = ChewingKey { packed: 0xBEEF };
    assert!(pinyin_parse_double_pinyin(
        fixture.instance,
        cstr("ni").as_ptr(),
        &mut key
    ));
    let mut rendered: *mut pinyin_capi::GChar = std::ptr::null_mut();
    assert!(pinyin_get_pinyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("ni"));

    // Without USE_TONE the three-byte form is refused and the key keeps
    // its prior bytes (the pin writes only on success).
    assert!(!pinyin_parse_double_pinyin(
        fixture.instance,
        cstr("ni3").as_ptr(),
        &mut key
    ));
    assert_eq!(key.packed, 43, "prior key untouched on failure");

    fixture.set_options(USE_TONE | PINYIN_INCOMPLETE);
    assert!(pinyin_parse_double_pinyin(
        fixture.instance,
        cstr("ni3").as_ptr(),
        &mut key
    ));
    assert!(pinyin_get_pinyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("ni3"));

    // FORCE_TONE demands exactly three bytes.
    fixture.set_options(USE_TONE | FORCE_TONE | PINYIN_INCOMPLETE);
    assert!(!pinyin_parse_double_pinyin(
        fixture.instance,
        cstr("ni").as_ptr(),
        &mut key
    ));
    assert!(pinyin_parse_double_pinyin(
        fixture.instance,
        cstr("ni3").as_ptr(),
        &mut key
    ));

    // The incomplete single-key probe under PINYIN_INCOMPLETE.
    fixture.set_options(PINYIN_INCOMPLETE);
    assert!(pinyin_parse_double_pinyin(
        fixture.instance,
        cstr("z").as_ptr(),
        &mut key
    ));
    assert!(pinyin_get_pinyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("z"));
}

/// The chewing one-key surface (STANDARD keyboard): `18` is ㄅㄚ; the
/// `ZHUYIN_CORRECT_ALL` strip keeps caller corrections away from the
/// chewing parser; the incomplete ㄅ resolves only under
/// `ZHUYIN_INCOMPLETE` (bit 0x10 of the option word).
#[test]
fn chewing_one_key_surface() {
    let fixture = Fixture::new("keys-chewing");
    assert!(pinyin_set_zhuyin_scheme(fixture.context, 1));

    let mut key = ChewingKey::ZERO;
    assert!(pinyin_parse_chewing(
        fixture.instance,
        cstr("18").as_ptr(),
        &mut key
    ));
    let mut rendered: *mut pinyin_capi::GChar = std::ptr::null_mut();
    assert!(pinyin_get_pinyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("ba"));
    assert!(pinyin_get_zhuyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("ㄅㄚ"));
    assert!(!pinyin_get_pinyin_is_incomplete(fixture.instance, &mut key));

    // Tone: `3` is a tone key on STANDARD under USE_TONE.
    fixture.set_options(USE_TONE);
    assert!(pinyin_parse_chewing(
        fixture.instance,
        cstr("183").as_ptr(),
        &mut key
    ));
    assert!(pinyin_get_pinyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert_eq!(take(rendered).as_deref(), Some("ba3"));

    // Incomplete ㄅ: refused without the bit, the initial-only `b` with
    // it (ZHUYIN_INCOMPLETE = 0x10).
    fixture.set_options(0);
    assert!(!pinyin_parse_chewing(
        fixture.instance,
        cstr("1").as_ptr(),
        &mut key
    ));
    fixture.set_options(1 << 4);
    assert!(pinyin_parse_chewing(
        fixture.instance,
        cstr("1").as_ptr(),
        &mut key
    ));
    assert!(pinyin_get_pinyin_is_incomplete(fixture.instance, &mut key));
}

/// `pinyin_get_pinyin_strings`: shengmu/yunmu render separately, NULL
/// out-params skip, and the zero-key guard leaves the out-params
/// UNTOUCHED (the pin's asymmetric contract).
#[test]
fn pinyin_strings_contract() {
    let fixture = Fixture::new("keys-strings");
    let mut key = ChewingKey::ZERO;
    assert!(pinyin_parse_full_pinyin(
        fixture.instance,
        cstr("ba").as_ptr(),
        &mut key
    ));

    let mut shengmu: *mut pinyin_capi::GChar = std::ptr::null_mut();
    let mut yunmu: *mut pinyin_capi::GChar = std::ptr::null_mut();
    assert!(pinyin_get_pinyin_strings(
        fixture.instance,
        &mut key,
        &mut shengmu,
        &mut yunmu
    ));
    assert_eq!(take(shengmu).as_deref(), Some("b"));
    assert_eq!(take(yunmu).as_deref(), Some("a"));

    // NULL out-params are tolerated on success.
    shengmu = std::ptr::null_mut();
    assert!(pinyin_get_pinyin_strings(
        fixture.instance,
        &mut key,
        &mut shengmu,
        std::ptr::null_mut()
    ));
    assert_eq!(take(shengmu).as_deref(), Some("b"));

    // The zero key fails without touching the out-params.
    let mut key = ChewingKey::ZERO;
    let sentinel: *mut pinyin_capi::GChar = std::ptr::dangling_mut::<pinyin_capi::GChar>();
    let mut sentinel = sentinel;
    let kept = sentinel;
    assert!(!pinyin_get_pinyin_strings(
        fixture.instance,
        &mut key,
        &mut sentinel,
        std::ptr::null_mut()
    ));
    assert_eq!(sentinel, kept, "out-param untouched on the failing guard");

    // The four string getters NULL the out-param on the same guard.
    let mut rendered: *mut pinyin_capi::GChar = std::ptr::dangling_mut::<pinyin_capi::GChar>();
    assert!(!pinyin_get_pinyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert!(rendered.is_null());
    assert!(!pinyin_get_zhuyin_string(
        fixture.instance,
        &mut key,
        &mut rendered
    ));
    assert!(rendered.is_null());
}

/// A hand-crafted toned initial-only key: the pin asserts (abort) in
/// `pinyin_get_pinyin_is_incomplete`'s true branch; the no-abort policy
/// answers `true`.
#[test]
fn toned_initial_only_key_is_incomplete_without_aborting() {
    let fixture = Fixture::new("keys-toned-incomplete");
    // middle 0, final 0, tone 3.
    let mut key = ChewingKey { packed: 3 << 12 };
    assert!(pinyin_get_pinyin_is_incomplete(fixture.instance, &mut key));
}

/// `pinyin_get_context` hands back the allocating context handle; null
/// instances answer null.
#[test]
fn get_context_returns_the_handle() {
    let (context, instance, _user_dir) = open_with_dir("keys-context");
    assert_eq!(pinyin_get_context(instance), context);
    assert!(pinyin_get_context(std::ptr::null_mut()).is_null());
    pinyin_free_instance(instance);
    pinyin_fini(context);
}

/// The addon unload contract: out-of-range answers `false` (the pin's
/// assert), any in-range index answers `true` loaded or not.
#[test]
fn unload_addon_contract() {
    let fixture = Fixture::new("keys-unload");
    assert!(!pinyin_unload_addon_phrase_library(fixture.context, 16));
    assert!(!pinyin_unload_addon_phrase_library(
        fixture.context,
        u8::MAX
    ));
    // Never-loaded in-range indexes still unload "successfully".
    assert!(pinyin_unload_addon_phrase_library(fixture.context, 5));
    assert!(pinyin_unload_addon_phrase_library(fixture.context, 5));
    assert!(pinyin_unload_addon_phrase_library(fixture.context, 0));
}
