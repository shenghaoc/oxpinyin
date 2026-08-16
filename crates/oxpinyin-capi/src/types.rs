//! Opaque handle types and C-ABI enums matching `pinyin.h`.
//!
//! Many items here are not yet referenced from Rust code but exist to
//! match the C ABI surface and appear in the generated header.
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_uint};

// ── Opaque handle types ──────────────────────────────────────────────
//
// Each crosses the FFI boundary as `*mut T` only — never by value and never
// dereferenced on the Rust side. The concrete backing allocation is the
// crate-private `CapiContext`/`CapiInstance` (see `context.rs`/`instance.rs`):
// constructors `Box::into_raw` those and cast to these marker types; the
// matching destructor casts back and `Box::from_raw`s.
//
// These are declared as unit structs (no `#[repr(C)]`, no fields) so cbindgen
// emits `pinyin.h`-style *incomplete* handles (`typedef struct pinyin_context_t
// pinyin_context_t;`) rather than a complete `[u8; 0]` body.

/// Opaque pinyin context (one per input mode).
pub struct PinyinContext;

/// Opaque pinyin instance (one per active editor).
pub struct PinyinInstance;

/// Opaque lookup candidate (instance-borrowed, transient).
pub struct LookupCandidate;

/// Opaque chewing key.
pub struct ChewingKey;

/// Opaque chewing key rest (position span).
pub struct ChewingKeyRest;

/// Opaque import iterator.
pub struct ImportIterator;

/// Opaque export iterator.
pub struct ExportIterator;

/// Opaque bigram export iterator.
pub struct BigramExportIterator;

// ── Enums ────────────────────────────────────────────────────────────

/// `lookup_candidate_type_t` from `pinyin.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum lookup_candidate_type_t {
    /// Best sentence-level match.
    NBEST_MATCH_CANDIDATE = 1,
    /// Normal word candidate.
    NORMAL_CANDIDATE = 2,
    /// Zombie candidate.
    ZOMBIE_CANDIDATE = 3,
    /// Predicted bigram candidate.
    PREDICTED_BIGRAM_CANDIDATE = 4,
    /// Predicted prefix candidate.
    PREDICTED_PREFIX_CANDIDATE = 5,
    /// Addon dictionary candidate.
    ADDON_CANDIDATE = 6,
    /// Longer candidate.
    LONGER_CANDIDATE = 7,
    /// Predicted punctuation candidate.
    PREDICTED_PUNCTUATION_CANDIDATE = 8,
}

/// `sort_option_t` flag bits from `pinyin.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum sort_option_t {
    /// Exclude sentence candidate.
    SORT_WITHOUT_SENTENCE_CANDIDATE = 0x1,
    /// Exclude longer candidates.
    SORT_WITHOUT_LONGER_CANDIDATE = 0x2,
    /// Sort by phrase length.
    SORT_BY_PHRASE_LENGTH = 0x4,
    /// Sort by pinyin length.
    SORT_BY_PINYIN_LENGTH = 0x8,
    /// Sort by frequency.
    SORT_BY_FREQUENCY = 0x10,
}

/// `DoublePinyinScheme` from `pinyin_custom2.h`.
///
/// FFI parameters carrying a scheme take `c_int`, not this enum: callers
/// may pass any `int`, and forming a `#[repr(C)]` enum from a discriminant
/// that is not a variant is UB. This enum exists only so cbindgen emits the
/// named constants and values.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum DoublePinyinScheme {
    /// Ziran码 scheme.
    DOUBLE_PINYIN_ZRM = 1,
    /// Microsoft scheme.
    DOUBLE_PINYIN_MS = 2,
    /// Ziguang scheme.
    DOUBLE_PINYIN_ZIGUANG = 3,
    /// ABC scheme.
    DOUBLE_PINYIN_ABC = 4,
    /// PYJJ scheme.
    DOUBLE_PINYIN_PYJJ = 5,
    /// Xiaohe scheme.
    DOUBLE_PINYIN_XHE = 6,
    /// User's keyboard.
    DOUBLE_PINYIN_CUSTOMIZED = 30,
}

/// `ZhuyinScheme` from `pinyin_custom2.h`.
///
/// FFI parameters carrying a scheme take `c_int`, not this enum: callers
/// may pass any `int`, and forming a `#[repr(C)]` enum from a discriminant
/// that is not a variant is UB. This enum exists only so cbindgen emits the
/// named constants and values.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum ZhuyinScheme {
    /// Standard layout.
    ZHUYIN_STANDARD = 1,
    /// Hsu layout.
    ZHUYIN_HSU = 2,
    /// IBM layout.
    ZHUYIN_IBM = 3,
    /// GinYieh layout.
    ZHUYIN_GINYIEH = 4,
    /// Eten layout.
    ZHUYIN_ETEN = 5,
    /// Eten26 layout.
    ZHUYIN_ETEN26 = 6,
    /// Standard Dvorak layout.
    ZHUYIN_STANDARD_DVORAK = 7,
    /// Hsu Dvorak layout.
    ZHUYIN_HSU_DVORAK = 8,
    /// Dachen CP26 layout.
    ZHUYIN_DACHEN_CP26 = 9,
}

// ── Type aliases matching glib/pinyin.h scalar types ─────────────────

/// `pinyin_option_t` — bitmask of pinyin table flags.
pub type PinyinOptionT = u32;

/// `phrase_token_t` — `guint32`.
pub type PhraseTokenT = u32;

/// `guint` — GLib unsigned int (= `c_uint`).
pub type GUint = c_uint;

/// `gint` — GLib signed int (= `c_int`).
pub type GInt = c_int;

/// `gchar` — GLib char (= `c_char`).
pub type GChar = c_char;
