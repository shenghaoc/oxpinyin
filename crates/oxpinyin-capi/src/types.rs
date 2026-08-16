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

/// `PinyinKey` alias (`libpinyin/src/pinyin.h:1093`, tag 2.11.91):
/// compatibility alias for [`ChewingKey`].
pub type PinyinKey = ChewingKey;

/// `PinyinKeyPos` alias (`libpinyin/src/pinyin.h:1094`, tag 2.11.91):
/// compatibility alias for [`ChewingKeyRest`].
pub type PinyinKeyPos = ChewingKeyRest;

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

/// `PinyinTableFlag` constants the fork compiles against
/// (`libpinyin/src/storage/pinyin_custom2.h:31-41`, tag 2.11.91).
///
/// Only the fork-referenced enumerators are carried; upstream's `IS_PINYIN`,
/// `IS_ZHUYIN`, and `FORCE_TONE` are intentionally absent from this header.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PinyinTableFlag {
    /// `PINYIN_INCOMPLETE = 1U << 3` (`pinyin_custom2.h:34`).
    PINYIN_INCOMPLETE = 1 << 3,
    /// `ZHUYIN_INCOMPLETE = 1U << 4` (`pinyin_custom2.h:35`).
    ZHUYIN_INCOMPLETE = 1 << 4,
    /// `USE_TONE = 1U << 5` (`pinyin_custom2.h:36`).
    USE_TONE = 1 << 5,
    /// `USE_DIVIDED_TABLE = 1U << 7` (`pinyin_custom2.h:38`).
    USE_DIVIDED_TABLE = 1 << 7,
    /// `USE_RESPLIT_TABLE = 1U << 8` (`pinyin_custom2.h:39`).
    USE_RESPLIT_TABLE = 1 << 8,
    /// `DYNAMIC_ADJUST = 1U << 9` (`pinyin_custom2.h:40`).
    DYNAMIC_ADJUST = 1 << 9,
}

/// `PinyinAmbiguity2` fuzzy-pinyin bits the fork compiles against
/// (`libpinyin/src/storage/pinyin_custom2.h:49-61`, tag 2.11.91).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PinyinAmbiguity2 {
    /// `PINYIN_AMB_C_CH = 1U << 10` (`pinyin_custom2.h:50`).
    PINYIN_AMB_C_CH = 1 << 10,
    /// `PINYIN_AMB_S_SH = 1U << 11` (`pinyin_custom2.h:51`).
    PINYIN_AMB_S_SH = 1 << 11,
    /// `PINYIN_AMB_Z_ZH = 1U << 12` (`pinyin_custom2.h:52`).
    PINYIN_AMB_Z_ZH = 1 << 12,
    /// `PINYIN_AMB_F_H = 1U << 13` (`pinyin_custom2.h:53`).
    PINYIN_AMB_F_H = 1 << 13,
    /// `PINYIN_AMB_G_K = 1U << 14` (`pinyin_custom2.h:54`).
    PINYIN_AMB_G_K = 1 << 14,
    /// `PINYIN_AMB_L_N = 1U << 15` (`pinyin_custom2.h:55`).
    PINYIN_AMB_L_N = 1 << 15,
    /// `PINYIN_AMB_L_R = 1U << 16` (`pinyin_custom2.h:56`).
    PINYIN_AMB_L_R = 1 << 16,
    /// `PINYIN_AMB_AN_ANG = 1U << 17` (`pinyin_custom2.h:57`).
    PINYIN_AMB_AN_ANG = 1 << 17,
    /// `PINYIN_AMB_EN_ENG = 1U << 18` (`pinyin_custom2.h:58`).
    PINYIN_AMB_EN_ENG = 1 << 18,
    /// `PINYIN_AMB_IN_ING = 1U << 19` (`pinyin_custom2.h:59`).
    PINYIN_AMB_IN_ING = 1 << 19,
    /// `PINYIN_AMB_ALL = 0x3FFU << 10` (`pinyin_custom2.h:60`).
    PINYIN_AMB_ALL = 0x3ff << 10,
}

/// `PinyinCorrection2` correct-pinyin bits the fork compiles against
/// (`libpinyin/src/storage/pinyin_custom2.h:70-80`, tag 2.11.91).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PinyinCorrection2 {
    /// `PINYIN_CORRECT_GN_NG = 1U << 21` (`pinyin_custom2.h:71`).
    PINYIN_CORRECT_GN_NG = 1 << 21,
    /// `PINYIN_CORRECT_MG_NG = 1U << 22` (`pinyin_custom2.h:72`).
    PINYIN_CORRECT_MG_NG = 1 << 22,
    /// `PINYIN_CORRECT_IOU_IU = 1U << 23` (`pinyin_custom2.h:73`).
    PINYIN_CORRECT_IOU_IU = 1 << 23,
    /// `PINYIN_CORRECT_UEI_UI = 1U << 24` (`pinyin_custom2.h:74`).
    PINYIN_CORRECT_UEI_UI = 1 << 24,
    /// `PINYIN_CORRECT_UEN_UN = 1U << 25` (`pinyin_custom2.h:75`).
    PINYIN_CORRECT_UEN_UN = 1 << 25,
    /// `PINYIN_CORRECT_UE_VE = 1U << 26` (`pinyin_custom2.h:76`).
    PINYIN_CORRECT_UE_VE = 1 << 26,
    /// `PINYIN_CORRECT_V_U = 1U << 27` (`pinyin_custom2.h:77`).
    PINYIN_CORRECT_V_U = 1 << 27,
    /// `PINYIN_CORRECT_ON_ONG = 1U << 28` (`pinyin_custom2.h:78`).
    PINYIN_CORRECT_ON_ONG = 1 << 28,
    /// `PINYIN_CORRECT_ALL = 0xFFU << 21` (`pinyin_custom2.h:79`).
    PINYIN_CORRECT_ALL = 0xff << 21,
}

/// `PHRASE_INDEX_LIBRARIES` ids the fork compiles against
/// (`libpinyin/src/include/novel_types.h:151-162`, tag 2.11.91).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PhraseIndexLibraries {
    /// `ADDON_DICTIONARY = 5` (`novel_types.h:159`).
    ADDON_DICTIONARY = 5,
    /// `NETWORK_DICTIONARY = 6` (`novel_types.h:160`).
    NETWORK_DICTIONARY = 6,
    /// `USER_DICTIONARY = 7` (`novel_types.h:161`).
    USER_DICTIONARY = 7,
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

/// `null_token` = 0 (`novel_types.h:121`, tag 2.11.91).
#[allow(non_upper_case_globals)]
pub const null_token: u32 = 0;
