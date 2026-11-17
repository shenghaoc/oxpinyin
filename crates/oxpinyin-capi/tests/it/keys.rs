//! The single-key parse + `ChewingKey` display surface (`src/keys.rs`),
//! driven black-box through the re-exported symbols against the `fixtures/w3`
//! mini tables.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::{PARITY, take};

use crate::support::{
    ChewingKey, ChewingKeyRest, PinyinInstance, TempUserDir, assert_aborts, cstr, key_slot_guard,
    open, pinyin_parse_more_full_pinyins, pinyin_set_options,
};

use pinyin_capi::{
    PinyinContext, pinyin_fini, pinyin_free_instance, pinyin_get_context,
    pinyin_get_luoma_pinyin_string, pinyin_get_pinyin_is_incomplete, pinyin_get_pinyin_string,
    pinyin_get_pinyin_strings, pinyin_get_secondary_zhuyin_string, pinyin_get_zhuyin_string,
    pinyin_parse_chewing, pinyin_parse_double_pinyin, pinyin_parse_full_pinyin,
    pinyin_set_double_pinyin_scheme, pinyin_set_zhuyin_scheme, pinyin_unload_addon_phrase_library,
};

use std::ptr;

/// The renderers allocate through GLib; `take_owned_cstr` frees with
/// `g_free`, so a leak or a mismatched free would surface here rather
/// than in a consumer.
fn take_rendered(p: *mut crate::support::GChar) -> String {
    assert!(!p.is_null(), "renderer answered true with a NULL out-param");
    crate::support::take_owned_cstr(p)
}

/// S7: the key at offset 0 of a `nihao` parse under full-pinyin scheme 2 (luoma) and
/// scheme 3 (secondary zhuyin), both under 0x18a after a sentence guess. The key is `ni`,
/// packed 43, and both renderers spell it from its table index (`pinyin.cpp:2729-2749` at
/// 074a2219, `storage/chewing_key.cpp:91-116`). Pin capture of 2026-11-07, runs 1 and 2
/// identical, observations `scheme-key/luoma/nihao` and `scheme-key/secondary-zhuyin/nihao`.
#[test]
fn a_scheme_parse_keys_render_as_luoma_and_secondary_zhuyin() {
    let _slots = key_slot_guard();
    for scheme in [2, 3] {
        let user_dir = TempUserDir::new("scheme-key");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert!(unsafe { pinyin_set_options(context, PARITY) });
        assert!(unsafe { crate::support::pinyin_set_full_pinyin_scheme(context, scheme) });
        let input = cstr("nihao");
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
            5
        );
        assert!(unsafe { crate::support::pinyin_guess_sentence(instance) });
        let mut key: *mut ChewingKey = ptr::null_mut();
        assert!(unsafe { crate::support::pinyin_get_pinyin_key(instance, 0, &raw mut key) });
        // SAFETY: a true answer points `key` at the instance's key slot.
        assert_eq!(unsafe { (*key).packed }, 43, "scheme {scheme}");
        let mut rendered: *mut crate::support::GChar = ptr::null_mut();
        assert!(unsafe {
            crate::support::pinyin_get_luoma_pinyin_string(instance, key, &raw mut rendered)
        });
        assert_eq!(take_rendered(rendered), "ni", "scheme {scheme}");
        assert!(unsafe {
            crate::support::pinyin_get_secondary_zhuyin_string(instance, key, &raw mut rendered)
        });
        assert_eq!(take_rendered(rendered), "ni", "scheme {scheme}");
        unsafe { crate::support::pinyin_free_instance(instance) };
        unsafe { crate::support::pinyin_fini(context) };
    }
}

/// The expectation table established from the pin: `nihao` parses as
/// `ni|hao`, so keys start at byte 0 and byte 2 and every other offset
/// answers `false` — offsets 1/3/4 are empty mid-syllable columns and 5
/// is the reserved slot (`offset >= matrix.size() - 1`).
#[test]
fn nihao_preedit_family_matches_the_pin_expectation_table() {
    let _slots = key_slot_guard();
    let user_dir = TempUserDir::new("preedit-nihao");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(unsafe { pinyin_set_options(context, PARITY) });
    let input = cstr("nihao");
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, input.as_ptr()) },
        5
    );

    let expected: [(usize, &str, u16, u16, &str, &str, &str); 2] = [
        (0, "ni", 0, 2, "n", "i", "ㄋㄧ"),
        (2, "hao", 2, 5, "h", "ao", "ㄏㄠ"),
    ];

    for (offset, pinyin, begin, end, shengmu, yunmu, zhuyin) in expected {
        let mut key: *mut ChewingKey = ptr::null_mut();
        assert!(
            unsafe { crate::support::pinyin_get_pinyin_key(instance, offset, &raw mut key) },
            "offset {offset} starts a key"
        );
        assert!(!key.is_null());

        let mut rest: *mut ChewingKeyRest = ptr::null_mut();
        assert!(unsafe {
            crate::support::pinyin_get_pinyin_key_rest(instance, offset, &raw mut rest)
        });
        let (mut b, mut e) = (0_u16, 0_u16);
        assert!(unsafe {
            crate::support::pinyin_get_pinyin_key_rest_positions(
                instance, rest, &raw mut b, &raw mut e,
            )
        });
        assert_eq!((b, e), (begin, end), "raw span at offset {offset}");

        let mut len = 0_u16;
        assert!(unsafe {
            crate::support::pinyin_get_pinyin_key_rest_length(instance, rest, &raw mut len)
        });
        assert_eq!(len, end - begin, "rest length at offset {offset}");

        let mut s: *mut crate::support::GChar = ptr::null_mut();
        assert!(unsafe { crate::support::pinyin_get_pinyin_string(instance, key, &raw mut s) });
        assert_eq!(take_rendered(s), pinyin);

        let (mut sm, mut ym): (*mut crate::support::GChar, *mut crate::support::GChar) =
            (ptr::null_mut(), ptr::null_mut());
        assert!(unsafe {
            crate::support::pinyin_get_pinyin_strings(instance, key, &raw mut sm, &raw mut ym)
        });
        assert_eq!(
            (take_rendered(sm), take_rendered(ym)),
            (shengmu.to_owned(), yunmu.to_owned())
        );

        let mut z: *mut crate::support::GChar = ptr::null_mut();
        assert!(unsafe { crate::support::pinyin_get_zhuyin_string(instance, key, &raw mut z) });
        assert_eq!(take_rendered(z), zhuyin);
    }

    for offset in [1_usize, 3, 4, 5] {
        let mut key: *mut ChewingKey = ptr::null_mut();
        assert!(
            !unsafe { crate::support::pinyin_get_pinyin_key(instance, offset, &raw mut key) },
            "offset {offset} starts no key"
        );
        assert!(key.is_null(), "a false answer nulls the out-param");
        let mut rest: *mut ChewingKeyRest = ptr::null_mut();
        assert!(!unsafe {
            crate::support::pinyin_get_pinyin_key_rest(instance, offset, &raw mut rest)
        });
    }

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// Past byte 65535 the key-rest positions narrow into the pin's
/// `guint16` fields mod 2^16 (maintainer ruling 2026-11-01 UTC,
/// `docs/findings/upstream-divergences.md#key-positions-past-byte-65535`). `ni3` then `ni`
/// repeated puts a key at 65535..65537, which crosses byte 65536, and
/// one at 65537..65539, wholly past it; both lengths stay 2.
#[test]
fn key_rest_positions_past_byte_65535_wrap() {
    use crate::support::pinyin_parse_more_double_pinyins;
    use crate::support::pinyin_set_double_pinyin_scheme;

    let _slots = key_slot_guard();
    let user_dir = TempUserDir::new("key-rest-wrap-double");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    assert!(unsafe { pinyin_set_double_pinyin_scheme(context, 2) });
    let input = format!("ni3{}", "ni".repeat(32_769));
    assert_eq!(
        unsafe { pinyin_parse_more_double_pinyins(instance, cstr(&input).as_ptr()) },
        65_541
    );

    for (offset, begin, end) in [(65_535_usize, 65_535_u16, 1_u16), (65_537, 1, 3)] {
        let mut rest: *mut ChewingKeyRest = ptr::null_mut();
        assert!(unsafe {
            crate::support::pinyin_get_pinyin_key_rest(instance, offset, &raw mut rest)
        });
        let (mut b, mut e) = (0_u16, 0_u16);
        assert!(unsafe {
            crate::support::pinyin_get_pinyin_key_rest_positions(
                instance, rest, &raw mut b, &raw mut e,
            )
        });
        assert_eq!((b, e), (begin, end), "offset {offset}");
        let mut length = 0_u16;
        assert!(unsafe {
            crate::support::pinyin_get_pinyin_key_rest_length(instance, rest, &raw mut length)
        });
        assert_eq!(length, 2, "offset {offset}");
    }

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// `PINYIN_INCOMPLETE` (`pinyin_custom2.h:34`) — the fixture word: `Fixture::new`
/// sets it explicitly, because `pinyin_init` itself seeds `USE_TONE`.
const PINYIN_INCOMPLETE: u32 = 1 << 3;

/// `USE_TONE` (`pinyin_custom2.h:36`).
const USE_TONE: u32 = 1 << 5;

/// `FORCE_TONE` (`pinyin_custom2.h:37`). The enum in `pinyin.h` carries
/// only the fork-referenced bits, so tests pass the raw word.
const FORCE_TONE: u32 = 1 << 6;

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

/// The full-pinyin one-key surface: `ni` parses and renders, the
/// whole-input probe refuses `nihao`, and a failed parse leaves the ZEROED
/// key (upstream zeroes `*onekey` before the probe). An apostrophe is the
/// pin's assert (`pinyin_parser2.cpp:170`), held by
/// `parse_keys::full_key_parse_aborts_on_an_apostrophe_after_invalid_bytes`
/// in `src/boundary_tests/parse_keys.rs`.
#[test]
fn full_pinyin_one_key_surface() {
    let fixture = Fixture::new("keys-full");

    let mut key = ChewingKey::ZERO;
    assert!(unsafe {
        pinyin_parse_full_pinyin(fixture.instance, cstr("ni").as_ptr(), &raw mut key)
    });
    assert_eq!(key.packed, 43, "initial n(11) | medial i(1) packing");

    let mut rendered: *mut pinyin_capi::GChar = std::ptr::null_mut();
    assert!(unsafe { pinyin_get_pinyin_string(fixture.instance, &raw mut key, &raw mut rendered) });
    assert_eq!(take(rendered).as_deref(), Some("ni"));
    assert!(unsafe { pinyin_get_zhuyin_string(fixture.instance, &raw mut key, &raw mut rendered) });
    assert_eq!(take(rendered).as_deref(), Some("ㄋㄧ"));
    assert!(unsafe {
        pinyin_get_luoma_pinyin_string(fixture.instance, &raw mut key, &raw mut rendered)
    });
    assert_eq!(take(rendered).as_deref(), Some("ni"));
    assert!(unsafe {
        pinyin_get_secondary_zhuyin_string(fixture.instance, &raw mut key, &raw mut rendered)
    });
    assert_eq!(
        take(rendered).as_deref(),
        Some("ni"),
        "secondary zhuyin for ni is ASCII"
    );

    // The whole input is the probe: no forward maximum match.
    let mut key = ChewingKey::ZERO;
    assert!(!unsafe {
        pinyin_parse_full_pinyin(fixture.instance, cstr("nihao").as_ptr(), &raw mut key)
    });
    assert_eq!(key.packed, 0, "failed full-pinyin parse zeroes the key");

    // Initial-only keys parse and render; is_incomplete answers true.
    assert!(unsafe {
        pinyin_parse_full_pinyin(fixture.instance, cstr("b").as_ptr(), &raw mut key)
    });
    assert!(unsafe { pinyin_get_pinyin_is_incomplete(fixture.instance, &raw mut key) });
    assert!(unsafe { pinyin_get_pinyin_string(fixture.instance, &raw mut key, &raw mut rendered) });
    assert_eq!(take(rendered).as_deref(), Some("b"));
    // Luoma/secondary for an initial-only key are the pin's "None" rows.
    assert!(unsafe {
        pinyin_get_luoma_pinyin_string(fixture.instance, &raw mut key, &raw mut rendered)
    });
    assert_eq!(take(rendered).as_deref(), Some("None"));
}

/// The double-pinyin one-key surface (MS default): `ni` resolves, tone
/// rides a three-byte input only under `USE_TONE`, `FORCE_TONE` demands
/// three bytes, and a failed parse leaves the caller's key UNTOUCHED.
#[test]
fn double_pinyin_one_key_surface() {
    let fixture = Fixture::new("keys-double");
    assert!(unsafe { pinyin_set_double_pinyin_scheme(fixture.context, 2) });

    let mut key = ChewingKey { packed: 0xBEEF };
    assert!(unsafe {
        pinyin_parse_double_pinyin(fixture.instance, cstr("ni").as_ptr(), &raw mut key)
    });
    let mut rendered: *mut pinyin_capi::GChar = std::ptr::null_mut();
    assert!(unsafe { pinyin_get_pinyin_string(fixture.instance, &raw mut key, &raw mut rendered) });
    assert_eq!(take(rendered).as_deref(), Some("ni"));

    // Without USE_TONE the three-byte form is refused and the key keeps
    // its prior bytes (the pin writes only on success).
    assert!(!unsafe {
        pinyin_parse_double_pinyin(fixture.instance, cstr("ni3").as_ptr(), &raw mut key)
    });
    assert_eq!(key.packed, 43, "prior key untouched on failure");

    fixture.set_options(USE_TONE | PINYIN_INCOMPLETE);
    assert!(unsafe {
        pinyin_parse_double_pinyin(fixture.instance, cstr("ni3").as_ptr(), &raw mut key)
    });
    assert!(unsafe { pinyin_get_pinyin_string(fixture.instance, &raw mut key, &raw mut rendered) });
    assert_eq!(take(rendered).as_deref(), Some("ni3"));

    // FORCE_TONE demands exactly three bytes.
    fixture.set_options(USE_TONE | FORCE_TONE | PINYIN_INCOMPLETE);
    assert!(!unsafe {
        pinyin_parse_double_pinyin(fixture.instance, cstr("ni").as_ptr(), &raw mut key)
    });
    assert!(unsafe {
        pinyin_parse_double_pinyin(fixture.instance, cstr("ni3").as_ptr(), &raw mut key)
    });

    // The incomplete single-key probe under PINYIN_INCOMPLETE.
    fixture.set_options(PINYIN_INCOMPLETE);
    assert!(unsafe {
        pinyin_parse_double_pinyin(fixture.instance, cstr("z").as_ptr(), &raw mut key)
    });
    assert!(unsafe { pinyin_get_pinyin_string(fixture.instance, &raw mut key, &raw mut rendered) });
    assert_eq!(take(rendered).as_deref(), Some("z"));
}

/// The chewing one-key surface (STANDARD keyboard): `18` is ㄅㄚ; the
/// `ZHUYIN_CORRECT_ALL` strip keeps caller corrections away from the
/// chewing parser; the incomplete ㄅ resolves only under
/// `ZHUYIN_INCOMPLETE` (bit 0x10 of the option word).
#[test]
fn chewing_one_key_surface() {
    let fixture = Fixture::new("keys-chewing");
    assert!(unsafe { pinyin_set_zhuyin_scheme(fixture.context, 1) });

    let mut key = ChewingKey::ZERO;
    assert!(unsafe { pinyin_parse_chewing(fixture.instance, cstr("18").as_ptr(), &raw mut key) });
    let mut rendered: *mut pinyin_capi::GChar = std::ptr::null_mut();
    assert!(unsafe { pinyin_get_pinyin_string(fixture.instance, &raw mut key, &raw mut rendered) });
    assert_eq!(take(rendered).as_deref(), Some("ba"));
    assert!(unsafe { pinyin_get_zhuyin_string(fixture.instance, &raw mut key, &raw mut rendered) });
    assert_eq!(take(rendered).as_deref(), Some("ㄅㄚ"));
    assert!(!unsafe { pinyin_get_pinyin_is_incomplete(fixture.instance, &raw mut key) });

    // Tone: `3` is a tone key on STANDARD under USE_TONE.
    fixture.set_options(USE_TONE);
    assert!(unsafe { pinyin_parse_chewing(fixture.instance, cstr("183").as_ptr(), &raw mut key) });
    assert!(unsafe { pinyin_get_pinyin_string(fixture.instance, &raw mut key, &raw mut rendered) });
    assert_eq!(take(rendered).as_deref(), Some("ba3"));

    // Incomplete ㄅ: refused without the bit, the initial-only `b` with
    // it (ZHUYIN_INCOMPLETE = 0x10).
    fixture.set_options(0);
    assert!(!unsafe { pinyin_parse_chewing(fixture.instance, cstr("1").as_ptr(), &raw mut key) });
    fixture.set_options(1 << 4);
    assert!(unsafe { pinyin_parse_chewing(fixture.instance, cstr("1").as_ptr(), &raw mut key) });
    assert!(unsafe { pinyin_get_pinyin_is_incomplete(fixture.instance, &raw mut key) });
}

/// `pinyin_get_pinyin_strings`: shengmu/yunmu render separately, NULL
/// out-params skip, and the zero-key guard leaves the out-params
/// UNTOUCHED (the pin's asymmetric contract).
#[test]
fn pinyin_strings_contract() {
    let fixture = Fixture::new("keys-strings");
    let mut key = ChewingKey::ZERO;
    assert!(unsafe {
        pinyin_parse_full_pinyin(fixture.instance, cstr("ba").as_ptr(), &raw mut key)
    });

    let mut shengmu: *mut pinyin_capi::GChar = std::ptr::null_mut();
    let mut yunmu: *mut pinyin_capi::GChar = std::ptr::null_mut();
    assert!(unsafe {
        pinyin_get_pinyin_strings(
            fixture.instance,
            &raw mut key,
            &raw mut shengmu,
            &raw mut yunmu,
        )
    });
    assert_eq!(take(shengmu).as_deref(), Some("b"));
    assert_eq!(take(yunmu).as_deref(), Some("a"));

    // NULL out-params are tolerated on success.
    shengmu = std::ptr::null_mut();
    assert!(unsafe {
        pinyin_get_pinyin_strings(
            fixture.instance,
            &raw mut key,
            &raw mut shengmu,
            std::ptr::null_mut(),
        )
    });
    assert_eq!(take(shengmu).as_deref(), Some("b"));

    // The zero key fails without touching the out-params.
    let mut key = ChewingKey::ZERO;
    let sentinel: *mut pinyin_capi::GChar = std::ptr::dangling_mut::<pinyin_capi::GChar>();
    let mut sentinel = sentinel;
    let kept = sentinel;
    assert!(!unsafe {
        pinyin_get_pinyin_strings(
            fixture.instance,
            &raw mut key,
            &raw mut sentinel,
            std::ptr::null_mut(),
        )
    });
    assert_eq!(sentinel, kept, "out-param untouched on the failing guard");

    // The four string getters NULL the out-param on the same guard.
    let mut rendered: *mut pinyin_capi::GChar = std::ptr::dangling_mut::<pinyin_capi::GChar>();
    assert!(!unsafe {
        pinyin_get_pinyin_string(fixture.instance, &raw mut key, &raw mut rendered)
    });
    assert!(rendered.is_null());
    assert!(!unsafe {
        pinyin_get_zhuyin_string(fixture.instance, &raw mut key, &raw mut rendered)
    });
    assert!(rendered.is_null());
}

/// A hand-crafted toned initial-only key: the pin asserts in
/// `pinyin_get_pinyin_is_incomplete`'s true branch (`pinyin.cpp:2769`),
/// and the facade aborts the process too (register row 4).
#[test]
fn toned_initial_only_key_aborts() {
    assert_aborts("keys::toned_initial_only_key_aborts", || {
        let fixture = Fixture::new("keys-toned-incomplete");
        // middle 0, final 0, tone 3.
        let mut key = ChewingKey { packed: 3 << 12 };
        unsafe { pinyin_get_pinyin_is_incomplete(fixture.instance, &raw mut key) };
    });
}

/// The untoned initial and the zero key keep the pin's `true`.
#[test]
fn untoned_initial_only_key_is_incomplete() {
    let fixture = Fixture::new("keys-untoned-incomplete");
    let mut untoned = ChewingKey { packed: 0 };
    assert!(unsafe { pinyin_get_pinyin_is_incomplete(fixture.instance, &raw mut untoned) });
}

/// `pinyin_get_context` hands back the allocating context handle; null
/// instances answer null.
#[test]
fn get_context_returns_the_handle() {
    let (context, instance, _user_dir) = open_with_dir("keys-context");
    assert_eq!(unsafe { pinyin_get_context(instance) }, context);
    assert!(unsafe { pinyin_get_context(std::ptr::null_mut()) }.is_null());
    unsafe { pinyin_free_instance(instance) };
    unsafe { pinyin_fini(context) };
}

/// The addon unload contract: any in-range index answers `true` loaded or
/// not. An index of 16 or more is the pin's assert (`pinyin.cpp:499`), held
/// by `null_arguments::null_context_unload_addon_phrase_library_aborts_on_index_16`
/// in `tests/it/null_arguments.rs`.
#[test]
fn unload_addon_contract() {
    let fixture = Fixture::new("keys-unload");
    // Never-loaded in-range indexes still unload "successfully".
    assert!(unsafe { pinyin_unload_addon_phrase_library(fixture.context, 5) });
    assert!(unsafe { pinyin_unload_addon_phrase_library(fixture.context, 5) });
    assert!(unsafe { pinyin_unload_addon_phrase_library(fixture.context, 0) });
}
