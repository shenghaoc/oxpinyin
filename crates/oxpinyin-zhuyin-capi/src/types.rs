//! Opaque handle types and C-ABI enums matching `zhuyin.h`.

use std::os::raw::{c_char, c_uint};

// ── Opaque handle types ──────────────────────────────────────────────
//
// Each crosses the FFI boundary as `*mut T` only — never by value and never
// dereferenced on the Rust side. The concrete backing allocation is the
// crate-private `CapiContext`/`CapiInstance` (see `state.rs`): constructors
// `Box::into_raw` those and cast to these marker types; the matching
// destructor casts back and `Box::from_raw`s.
//
// Unit structs mark incomplete C handles; only their pointers cross the ABI.

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
/// [`oxpinyin_core::ChewingKey`].
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

    /// Packs the core elements into the ABI word.
    #[must_use]
    pub(crate) const fn from_core(key: oxpinyin_core::ChewingKey) -> Self {
        Self {
            packed: key.to_packed(),
        }
    }

    /// Decodes the word for the renderers.
    #[must_use]
    pub(crate) const fn to_core(self) -> oxpinyin_core::ChewingKey {
        oxpinyin_core::ChewingKey::from_packed(self.packed)
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

/// Opaque import iterator.
pub struct ImportIterator;

pub use glib_sys::GArray;

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
#[expect(non_camel_case_types, reason = "upstream C identifier")]
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

// ── Type aliases matching glib/zhuyin.h scalar types ─────────────────

/// `pinyin_option_t` — bitmask of pinyin table flags (also `zhuyin_option_t`,
/// a C typedef in `zhuyin.h`).
pub type PinyinOptionT = u32;

/// `phrase_token_t` — `guint32`.
pub type PhraseTokenT = u32;

/// `guint` — `GLib` unsigned int (= `c_uint`).
pub type GUint = c_uint;

/// `gchar` — `GLib` char (= `c_char`).
pub type GChar = c_char;

/// `null_token` = 0 (`novel_types.h:121`, tag 2.11.91).
#[expect(non_upper_case_globals, reason = "upstream C identifier")]
pub const null_token: u32 = 0;
