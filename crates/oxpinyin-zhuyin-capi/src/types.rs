//! Opaque handle types and C-ABI enums matching `zhuyin.h`.
//!
//! Many items here are not yet referenced from Rust code but exist to
//! match the C ABI surface and appear in the generated header.
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_uint};

// ── Opaque handle types ──────────────────────────────────────────────
//
// Each crosses the FFI boundary as `*mut T` only — never by value and never
// dereferenced on the Rust side. The concrete backing allocation is the
// crate-private `CapiContext`/`CapiInstance` (see `state.rs`): constructors
// `Box::into_raw` those and cast to these marker types; the matching
// destructor casts back and `Box::from_raw`s.
//
// These are declared as unit structs (no `#[repr(C)]`, no fields) so cbindgen
// emits `zhuyin.h`-style *incomplete* handles.

/// Opaque zhuyin context (one per input mode).
pub struct ZhuyinContext;

/// Opaque zhuyin instance (one per active editor).
pub struct ZhuyinInstance;

/// Opaque lookup candidate (instance-borrowed, transient).
pub struct LookupCandidate;

/// One parsed phonetic key, as the C ABI's opaque `ChewingKey`.
///
/// Byte-identical to the pinyin facade's `ChewingKey` (upstream declares one
/// `_ChewingKey` shared by both facades, `zhuyin.h:21`). The representation is
/// upstream's packed 16-bit word; the renderers decode it through
/// [`oxpinyin_chewing::ChewingKey`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChewingKey {
    /// Upstream's packed bitfield word.
    pub packed: u16,
}

const _: () = {
    assert!(size_of::<ChewingKey>() == 2);
    assert!(align_of::<ChewingKey>() == 2);
};

impl ChewingKey {
    /// The zero key (upstream `ChewingKey()`); the renderers answer
    /// `false` on it, the pin's `0 == key->get_table_index()` guard.
    pub const ZERO: Self = Self { packed: 0 };

    /// Packs a canonical spelling (a `content_table` pinyin) and tone
    /// into the ABI word; `None` for a spelling outside the table.
    #[must_use]
    pub(crate) fn from_spelling(text: &str, tone: u8) -> Option<Self> {
        let key = oxpinyin_chewing::ChewingKey::from_pinyin(text)?;
        Some(Self {
            packed: key.with_tone(tone).to_packed(),
        })
    }

    /// Packs the core elements into the ABI word.
    #[must_use]
    pub(crate) const fn from_core(key: oxpinyin_chewing::ChewingKey) -> Self {
        Self {
            packed: key.to_packed(),
        }
    }

    /// Decodes the word for the renderers.
    #[must_use]
    pub(crate) fn to_core(self) -> oxpinyin_chewing::ChewingKey {
        oxpinyin_chewing::ChewingKey::from_packed(self.packed)
    }
}

/// One key's raw input span, as the C ABI's opaque `ChewingKeyRest`
/// (`{ guint16 m_raw_begin; guint16 m_raw_end; }`, `chewing_key.h:97-114`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChewingKeyRest {
    /// `m_raw_begin` — the begin of the raw input.
    pub(crate) begin: u16,
    /// `m_raw_end` — the end of the raw input.
    pub(crate) end: u16,
}

const _: () = {
    assert!(size_of::<ChewingKeyRest>() == 4);
    assert!(align_of::<ChewingKeyRest>() == 2);
};

impl ChewingKeyRest {
    /// `_ChewingKeyRest::length` (`chewing_key.h:111-113`):
    /// `m_raw_end - m_raw_begin`.
    #[must_use]
    pub(crate) fn length(self) -> u16 {
        self.end.wrapping_sub(self.begin)
    }
}

/// Opaque import iterator.
pub struct ImportIterator;

/// glib `GArray` — the caller passes a real glib array (built through
/// `g_array_new`, torn down through `g_array_free`); the library appends
/// into it through glib's `g_array_append_vals`.
#[repr(C)]
pub struct GArray {
    /// The element data buffer, glib-owned.
    pub data: *mut GChar,
    /// The element count.
    pub len: u32,
}

// ── Enums ────────────────────────────────────────────────────────────

/// `lookup_candidate_type_t` from `zhuyin.h:41-45`.
///
/// **This is NOT the pinyin facade's `lookup_candidate_type_t`.** The zhuyin
/// header defines only four enumerators, and they collision with the pinyin
/// eight at discriminants 3 and 4:
///
/// | value | zhuyin (`zhuyin.h:41-45`) | pinyin (`pinyin.h`) |
/// |---|---|---|
/// | 1 | `BEST_MATCH_CANDIDATE` | `NBEST_MATCH_CANDIDATE` |
/// | 2 | `NORMAL_CANDIDATE_AFTER_CURSOR` | `NORMAL_CANDIDATE` |
/// | 3 | `NORMAL_CANDIDATE_BEFORE_CURSOR` | `ZOMBIE_CANDIDATE` |
/// | 4 | `ZOMBIE_CANDIDATE` | `PREDICTED_BIGRAM_CANDIDATE` |
///
/// So the four symbols that touch the candidate type — `zhuyin_guess_candidates_after_cursor`,
/// `zhuyin_guess_candidates_before_cursor`, `zhuyin_choose_candidate`,
/// `zhuyin_get_candidate_type` — must tag/read from THIS enum, never the
/// pinyin one. The name is deliberately not shared so a misuse fails to
/// compile rather than silently assigning the wrong discriminant.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum lookup_candidate_type_t {
    /// `BEST_MATCH_CANDIDATE = 1`.
    BEST_MATCH_CANDIDATE = 1,
    /// `NORMAL_CANDIDATE_AFTER_CURSOR = 2`.
    NORMAL_CANDIDATE_AFTER_CURSOR = 2,
    /// `NORMAL_CANDIDATE_BEFORE_CURSOR = 3`.
    NORMAL_CANDIDATE_BEFORE_CURSOR = 3,
    /// `ZOMBIE_CANDIDATE = 4`.
    ZOMBIE_CANDIDATE = 4,
}

/// `ZhuyinScheme` from `pinyin_custom2.h:122-133`.
///
/// FFI parameters carrying a scheme take `c_int`, not this enum: callers
/// may pass any `int`, and forming a `#[repr(C)]` enum from a discriminant
/// that is not a variant is UB.
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

/// `FullPinyinScheme` discriminants the zhuyin facade's
/// `zhuyin_get_pinyin_string` dispatches on (`zhuyin.cpp:1743-1766`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum FullPinyinScheme {
    /// `FULL_PINYIN_HANYU = 1`.
    FULL_PINYIN_HANYU = 1,
    /// `FULL_PINYIN_LUOMA = 2`.
    FULL_PINYIN_LUOMA = 2,
    /// `FULL_PINYIN_SECONDARY_ZHUYIN = 3`.
    FULL_PINYIN_SECONDARY_ZHUYIN = 3,
}

// ── Type aliases matching glib/zhuyin.h scalar types ─────────────────

/// `pinyin_option_t` — bitmask of pinyin table flags (also `zhuyin_option_t`,
/// a C typedef in `zhuyin.h`).
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
