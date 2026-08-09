//! Core types for the pinyin data layer.
//!
//! These mirror the libpinyin data structures used by the oracle.
//! Interpretation has been verified against the oracle's on-disk
//! format; values that need FFI cross-check are marked.

/// A 6-byte key used to index into `pinyin_index.redb` and `phrase_index.redb`.
///
/// The encoding from pinyin syllables to these keys is handled by
/// the `SyllableEncoder` (to be verified against oracle FFI).
pub type TableKey = [u8; 6];

/// A 4-byte bigram key: `(prev_token: u16, next_token: u16)` in little-endian.
///
/// Used as the key in `bigram.redb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BigramKey {
    /// Previous phrase token index (lower 16 bits of phrase reference).
    pub prev: u16,
    /// Next phrase token index (lower 16 bits of phrase reference).
    pub next: u16,
}

impl BigramKey {
    /// Create a bigram key from two u16 token indices.
    #[inline]
    pub const fn new(prev: u16, next: u16) -> Self {
        Self { prev, next }
    }

    /// Serialize to a 4-byte little-endian key.
    #[inline]
    pub fn to_bytes(self) -> [u8; 4] {
        let mut buf = [0u8; 4];
        buf[0..2].copy_from_slice(&self.prev.to_le_bytes());
        buf[2..4].copy_from_slice(&self.next.to_le_bytes());
        buf
    }

    /// Deserialize from a 4-byte little-endian key.
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 4 {
            return None;
        }
        Some(Self {
            prev: u16::from_le_bytes([bytes[0], bytes[1]]),
            next: u16::from_le_bytes([bytes[2], bytes[3]]),
        })
    }
}

/// A single bigram frequency entry: the frequency count and the packed
/// next-token reference.
///
/// Each entry in a bigram value is 8 bytes:
/// - bytes 0..4: `frequency` (u32 LE)
/// - bytes 4..8: `packed_next` (u32 LE, encoding flags + phrase reference)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigramEntry {
    /// Raw frequency count from the oracle data.
    pub frequency: u32,
    /// Packed next-token reference (flags in upper bits, phrase offset in lower).
    pub packed_next: u32,
}

impl BigramEntry {
    /// Parse from an 8-byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 8 {
            return None;
        }
        Some(Self {
            frequency: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            packed_next: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        })
    }
}

// Re-export the canonical vocabulary types from pinyin-core.
// The data crate is a consumer of the core vocabulary, not a duplicate
// definition. This ensures Session bounds in pinyin-engine (which name
// pinyin_core::vocab::PhraseToken) match the types used here.
pub use pinyin_core::{PhraseEntry, PhraseToken, SyllableKey};
