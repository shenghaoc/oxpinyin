//! Opaque handle types and C-ABI enums matching `pinyin.h`.

use std::os::raw::{c_char, c_uint};

// ── Opaque handle types ──────────────────────────────────────────────
//
// Each crosses the FFI boundary as `*mut T` only — never by value and never
// dereferenced on the Rust side. The concrete backing allocation is the
// crate-private `CapiContext`/`CapiInstance` (see `context.rs`/`instance.rs`):
// constructors `Box::into_raw` those and cast to these marker types; the
// matching destructor casts back and `Box::from_raw`s.
//
// Unit structs mark incomplete C handles; only their pointers cross the ABI.

/// Opaque pinyin context (one per input mode).
pub struct PinyinContext;

/// Opaque pinyin instance (one per active editor).
pub struct PinyinInstance;

/// Opaque lookup candidate (instance-borrowed, transient).
pub struct LookupCandidate;

/// One parsed phonetic key, as the C ABI's opaque `ChewingKey`.
///
/// The real library's `_ChewingKey` is a 16-bit packed bitfield of
/// initial/middle/final/tone (`chewing_key.h:41-55`), and it is the SAME
/// type for every scheme — full pinyin, double pinyin and zhuyin all parse
/// into it, and only the renderer differs. Consumers never see the layout:
/// `chewing_key.h` is not installed, and the shipped `pinyin.h` carries
/// only `typedef struct _ChewingKey ChewingKey;`, so a consumer holds an
/// incomplete type.
///
/// The representation is still upstream's packed word (`#[repr(C)]`,
/// size and alignment asserted against the bitfield): a caller holding a
/// vendored `chewing_key.h` allocates exactly these 2 bytes and may copy
/// keys through the pointer, so this must write and read what upstream
/// writes and reads — the D1' contract. The renderers decode the word
/// through [`oxpinyin_core::ChewingKey`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChewingKey {
    /// Upstream's packed bitfield word — byte-identical to a vendored
    /// `chewing_key.h` `_ChewingKey` object.
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

/// One key's raw input span, as the C ABI's opaque `ChewingKeyRest`.
///
/// The pin's `_ChewingKeyRest` is `{ guint16 m_raw_begin; guint16 m_raw_end; }`
/// with `length() = m_raw_end - m_raw_begin` (`chewing_key.h:97-114`), and
/// is opaque to consumers for the same reason as [`ChewingKey`]. The
/// field order and width match that declaration (`#[repr(C)]`, asserted).
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

/// Opaque export iterator.
pub struct ExportIterator;

/// Opaque bigram export iterator.
pub struct BigramExportIterator;

pub use glib_sys::GArray;

// ── Enums ────────────────────────────────────────────────────────────

/// `lookup_candidate_type_t` from `pinyin.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(non_camel_case_types, reason = "upstream C identifier")]
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

impl lookup_candidate_type_t {
    /// The one byte a snapshot row stores the type in: the discriminant.
    pub(crate) const fn tag(self) -> u8 {
        self as u8
    }

    /// The type [`Self::tag`] stored as `tag`. A row stores only tags of
    /// real types; any other byte reads as `NORMAL_CANDIDATE`.
    pub(crate) const fn from_tag(tag: u8) -> Self {
        match tag {
            1 => Self::NBEST_MATCH_CANDIDATE,
            3 => Self::ZOMBIE_CANDIDATE,
            4 => Self::PREDICTED_BIGRAM_CANDIDATE,
            5 => Self::PREDICTED_PREFIX_CANDIDATE,
            6 => Self::ADDON_CANDIDATE,
            7 => Self::LONGER_CANDIDATE,
            8 => Self::PREDICTED_PUNCTUATION_CANDIDATE,
            _ => Self::NORMAL_CANDIDATE,
        }
    }
}

// ── Type aliases matching glib/pinyin.h scalar types ─────────────────

/// `pinyin_option_t` — bitmask of pinyin table flags.
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

// Header phrase-index literals must stay byte-identical to the canonical
// `oxpinyin-user` token layout. `PinyinKey` / `PinyinKeyPos` are C-only
// typedefs in the checked-in `pinyin.h`, so they do not collide
// with `oxpinyin_user::PinyinKey` (`u16`).
const _: () = {
    assert!(oxpinyin_user::USER_DICTIONARY == 7);
    assert!(oxpinyin_user::PHRASE_MASK == 0x00FF_FFFF);
    assert!(oxpinyin_user::PHRASE_INDEX_LIBRARY_MASK == 0x0F00_0000);
};
