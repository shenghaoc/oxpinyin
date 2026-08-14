//! Opaque handle types and C-ABI enums matching `pinyin.h`.
//!
//! Many items here are not yet referenced from Rust code but exist to
//! match the C ABI surface and appear in the generated header.
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_uint};

// ── Opaque handle types ──────────────────────────────────────────────
//
// Each crosses the FFI boundary as `*mut T`.
// Constructor: `Box::into_raw(Box::new(..))`.
// Destructor:  `drop(unsafe { Box::from_raw(ptr) })`.

/// Opaque pinyin context (one per input mode).
#[repr(C)]
pub struct PinyinContext {
    _opaque: [u8; 0],
}

/// Opaque pinyin instance (one per active editor).
#[repr(C)]
pub struct PinyinInstance {
    _opaque: [u8; 0],
}

/// Opaque lookup candidate (instance-borrowed, transient).
#[repr(C)]
pub struct LookupCandidate {
    _opaque: [u8; 0],
}

/// Opaque chewing key.
#[repr(C)]
pub struct ChewingKey {
    _opaque: [u8; 0],
}

/// Opaque chewing key rest (position span).
#[repr(C)]
pub struct ChewingKeyRest {
    _opaque: [u8; 0],
}

/// Opaque import iterator.
#[repr(C)]
pub struct ImportIterator {
    _opaque: [u8; 0],
}

/// Opaque export iterator.
#[repr(C)]
pub struct ExportIterator {
    _opaque: [u8; 0],
}

/// Opaque bigram export iterator.
#[repr(C)]
pub struct BigramExportIterator {
    _opaque: [u8; 0],
}

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
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoublePinyinScheme {
    /// Ziran码 scheme.
    Zrm = 1,
    /// Microsoft scheme.
    Ms = 2,
    /// Ziguang scheme.
    Ziguang = 3,
    /// ABC scheme.
    Abc = 4,
    /// PYJJ scheme.
    Pyjj = 5,
    /// Xiaohe scheme.
    Xhe = 6,
}

/// `ZhuyinScheme` from `pinyin_custom2.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZhuyinScheme {
    /// Standard layout.
    Standard = 1,
    /// Hsu layout.
    Hsu = 2,
    /// IBM layout.
    Ibm = 3,
    /// GinYieh layout.
    Ginyieh = 4,
    /// Eten layout.
    Eten = 5,
    /// Eten26 layout.
    Eten26 = 6,
    /// Standard Dvorak layout.
    StandardDvorak = 7,
    /// Hsu Dvorak layout.
    HsuDvorak = 8,
    /// Dachen CP26 layout.
    DachenCp26 = 9,
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
