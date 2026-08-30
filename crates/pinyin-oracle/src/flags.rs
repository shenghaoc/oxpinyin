//! Parser option bits accepted by the oracle.
//!
//! Values are frozen in `docs/testing/oracle-ffi-seam.md`, derived from the
//! `PinyinTableFlag` and `PinyinAmbiguity2` enumerations declared by the pinned
//! public headers. The [`CAPTURE_DEFAULT`] word equals the `flags` field
//! recorded in every `fixtures/foundation/f-a.txt` record, which cross-checks
//! these constants against frozen capture output.
//!
//! This module is pure, so the protocol rule that `DYNAMIC_ADJUST` is *rejected*
//! rather than masked stays under test with no oracle present.

use crate::OracleError;

/// `IS_PINYIN` — the base full-pinyin bit. F-C's baseline word.
pub const IS_PINYIN: u32 = 1 << 1;

/// `IS_ZHUYIN`.
pub const IS_ZHUYIN: u32 = 1 << 2;

/// `PINYIN_INCOMPLETE` — admit partial (initial-only) tails.
pub const PINYIN_INCOMPLETE: u32 = 1 << 3;

/// `ZHUYIN_INCOMPLETE`.
pub const ZHUYIN_INCOMPLETE: u32 = 1 << 4;

/// `USE_TONE`.
pub const USE_TONE: u32 = 1 << 5;

/// `FORCE_TONE`.
pub const FORCE_TONE: u32 = 1 << 6;

/// `USE_DIVIDED_TABLE`.
pub const USE_DIVIDED_TABLE: u32 = 1 << 7;

/// `USE_RESPLIT_TABLE`.
pub const USE_RESPLIT_TABLE: u32 = 1 << 8;

/// `DYNAMIC_ADJUST` — forbidden by the parity protocol.
///
/// The capture protocol in `docs/testing/capture-fixtures.md` rejects this bit
/// outright. Masking it silently would make a run look protocol-clean while
/// learning was active, so [`OracleFlags::new`] returns an error instead.
pub const DYNAMIC_ADJUST: u32 = 1 << 9;

/// Every pinyin ambiguity bit (`PINYIN_AMB_ALL`).
pub const PINYIN_AMB_ALL: u32 = 0x3ff << 10;

/// Sort order used for candidate enumeration.
///
/// `SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY`, the value
/// `tools/capture/capture.c` passes to `pinyin_guess_candidates`.
pub const SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY: u32 = 0x1e;

/// The F-A capture profile: `0x18a`.
///
/// `IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE`.
pub const CAPTURE_DEFAULT: u32 =
    IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE;

/// The F-C baseline profile: `IS_PINYIN` alone.
pub const CAPTURE_BASELINE: u32 = IS_PINYIN;

/// A protocol-legal option word.
///
/// Constructing this type is the proof that `DYNAMIC_ADJUST` is clear.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OracleFlags(u32);

impl OracleFlags {
    /// The F-A capture profile.
    pub const DEFAULT: Self = Self(CAPTURE_DEFAULT);

    /// The F-C baseline profile.
    pub const BASELINE: Self = Self(CAPTURE_BASELINE);

    /// Wraps `bits` after checking the protocol.
    ///
    /// # Errors
    ///
    /// Returns [`OracleError::DynamicAdjustRejected`] when `bits` sets
    /// [`DYNAMIC_ADJUST`].
    pub const fn new(bits: u32) -> Result<Self, OracleError> {
        if bits & DYNAMIC_ADJUST != 0 {
            return Err(OracleError::DynamicAdjustRejected { flags: bits });
        }
        Ok(Self(bits))
    }

    /// The raw option word.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Whether every bit in `mask` is set.
    #[must_use]
    pub const fn contains(self, mask: u32) -> bool {
        self.0 & mask == mask
    }
}

impl core::fmt::Display for OracleFlags {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{:#010x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CAPTURE_DEFAULT, DYNAMIC_ADJUST, IS_PINYIN, OracleFlags, PINYIN_INCOMPLETE,
        SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY, USE_DIVIDED_TABLE,
        USE_RESPLIT_TABLE,
    };
    use crate::OracleError;

    #[test]
    fn capture_default_equals_the_frozen_f_a_flag_word() {
        // Every record in fixtures/foundation/f-a.txt carries flags=0x0000018a.
        assert_eq!(CAPTURE_DEFAULT, 0x0000_018a);
        assert_eq!(OracleFlags::DEFAULT.to_string(), "0x0000018a");
    }

    #[test]
    fn individual_bit_values_match_the_frozen_seam() {
        assert_eq!(IS_PINYIN, 0x002);
        assert_eq!(PINYIN_INCOMPLETE, 0x008);
        assert_eq!(USE_DIVIDED_TABLE, 0x080);
        assert_eq!(USE_RESPLIT_TABLE, 0x100);
        assert_eq!(DYNAMIC_ADJUST, 0x200);
        assert_eq!(SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY, 0x1e);
    }

    #[test]
    fn dynamic_adjust_is_rejected_never_masked() {
        let requested = CAPTURE_DEFAULT | DYNAMIC_ADJUST;
        match OracleFlags::new(requested) {
            Err(OracleError::DynamicAdjustRejected { flags }) => assert_eq!(flags, requested),
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn protocol_legal_words_round_trip() {
        for bits in [0, IS_PINYIN, CAPTURE_DEFAULT, 0x3ff << 10] {
            let flags = OracleFlags::new(bits).expect("no DYNAMIC_ADJUST bit");
            assert_eq!(flags.bits(), bits);
        }
    }

    #[test]
    fn contains_requires_every_bit_of_the_mask() {
        assert!(OracleFlags::DEFAULT.contains(IS_PINYIN));
        assert!(OracleFlags::DEFAULT.contains(IS_PINYIN | USE_RESPLIT_TABLE));
        assert!(!OracleFlags::BASELINE.contains(PINYIN_INCOMPLETE));
    }
}
