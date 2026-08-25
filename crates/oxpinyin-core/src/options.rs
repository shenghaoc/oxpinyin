//! Parser/decoder option bits.
//!
//! The bit values are the `PinyinTableFlag`, `PinyinAmbiguity2` and
//! `PinyinCorrection2` enumerators from libpinyin `pinyin_custom2.h`. The
//! core crate is dependency-free, so these are raw `u32` constants rather
//! than the `#[repr(C)]` C-ABI enums in `oxpinyin-capi` (which exist for
//! cbindgen).

/// `PINYIN_INCOMPLETE = 1U << 3` (`pinyin_custom2.h:34`).
pub const PINYIN_INCOMPLETE: u32 = 1 << 3;

/// `ZHUYIN_INCOMPLETE = 1U << 4` (`pinyin_custom2.h:35`).
pub const ZHUYIN_INCOMPLETE: u32 = 1 << 4;

/// `USE_TONE = 1U << 5` (`pinyin_custom2.h:36`).
pub const USE_TONE: u32 = 1 << 5;

/// `FORCE_TONE = 1U << 6` (`pinyin_custom2.h:37`). Rejected by the
/// `pinyin_custom2.h` header mirror in `oxpinyin-capi` (the bit arrives
/// only through the raw `pinyin_set_options` word); the parser honours it
/// nested inside `USE_TONE` exactly like the pin
/// (`pinyin_parser2.cpp:176-190`), and nowhere else — the double-pinyin
/// and zhuyin shapes are a different law, unmeasured here
/// (`docs/findings/upstream-divergences.md`).
pub const FORCE_TONE: u32 = 1 << 6;

/// `DYNAMIC_ADJUST = 1U << 9` (`pinyin_custom2.h:40`).
pub const DYNAMIC_ADJUST: u32 = 1 << 9;

/// `PINYIN_AMB_C_CH = 1U << 10` (`pinyin_custom2.h:50`).
pub const PINYIN_AMB_C_CH: u32 = 1 << 10;
/// `PINYIN_AMB_S_SH = 1U << 11` (`pinyin_custom2.h:51`).
pub const PINYIN_AMB_S_SH: u32 = 1 << 11;
/// `PINYIN_AMB_Z_ZH = 1U << 12` (`pinyin_custom2.h:52`).
pub const PINYIN_AMB_Z_ZH: u32 = 1 << 12;
/// `PINYIN_AMB_F_H = 1U << 13` (`pinyin_custom2.h:53`).
pub const PINYIN_AMB_F_H: u32 = 1 << 13;
/// `PINYIN_AMB_G_K = 1U << 14` (`pinyin_custom2.h:54`).
pub const PINYIN_AMB_G_K: u32 = 1 << 14;
/// `PINYIN_AMB_L_N = 1U << 15` (`pinyin_custom2.h:55`).
pub const PINYIN_AMB_L_N: u32 = 1 << 15;
/// `PINYIN_AMB_L_R = 1U << 16` (`pinyin_custom2.h:56`).
pub const PINYIN_AMB_L_R: u32 = 1 << 16;
/// `PINYIN_AMB_AN_ANG = 1U << 17` (`pinyin_custom2.h:57`).
pub const PINYIN_AMB_AN_ANG: u32 = 1 << 17;
/// `PINYIN_AMB_EN_ENG = 1U << 18` (`pinyin_custom2.h:58`).
pub const PINYIN_AMB_EN_ENG: u32 = 1 << 18;
/// `PINYIN_AMB_IN_ING = 1U << 19` (`pinyin_custom2.h:59`).
pub const PINYIN_AMB_IN_ING: u32 = 1 << 19;
/// `PINYIN_AMB_ALL = 0x3FFU << 10` (`pinyin_custom2.h:60`).
pub const PINYIN_AMB_ALL: u32 = 0x3ff << 10;

/// `PINYIN_CORRECT_GN_NG = 1U << 21` (`pinyin_custom2.h:71`).
pub const PINYIN_CORRECT_GN_NG: u32 = 1 << 21;
/// `PINYIN_CORRECT_MG_NG = 1U << 22` (`pinyin_custom2.h:72`).
pub const PINYIN_CORRECT_MG_NG: u32 = 1 << 22;
/// `PINYIN_CORRECT_IOU_IU = 1U << 23` (`pinyin_custom2.h:73`).
pub const PINYIN_CORRECT_IOU_IU: u32 = 1 << 23;
/// `PINYIN_CORRECT_UEI_UI = 1U << 24` (`pinyin_custom2.h:74`).
pub const PINYIN_CORRECT_UEI_UI: u32 = 1 << 24;
/// `PINYIN_CORRECT_UEN_UN = 1U << 25` (`pinyin_custom2.h:75`).
pub const PINYIN_CORRECT_UEN_UN: u32 = 1 << 25;
/// `PINYIN_CORRECT_UE_VE = 1U << 26` (`pinyin_custom2.h:76`).
pub const PINYIN_CORRECT_UE_VE: u32 = 1 << 26;
/// `PINYIN_CORRECT_V_U = 1U << 27` (`pinyin_custom2.h:77`).
pub const PINYIN_CORRECT_V_U: u32 = 1 << 27;
/// `PINYIN_CORRECT_ON_ONG = 1U << 28` (`pinyin_custom2.h:78`).
pub const PINYIN_CORRECT_ON_ONG: u32 = 1 << 28;
/// `PINYIN_CORRECT_ALL = 0xFFU << 21` (`pinyin_custom2.h:79`).
pub const PINYIN_CORRECT_ALL: u32 = 0xff << 21;

/// `ZHUYIN_CORRECT_HSU = 1U << 29` (`pinyin_custom2.h:89`).
pub const ZHUYIN_CORRECT_HSU: u32 = 1 << 29;
/// `ZHUYIN_CORRECT_ETEN26 = 1U << 30` (`pinyin_custom2.h:90`).
pub const ZHUYIN_CORRECT_ETEN26: u32 = 1 << 30;
/// `ZHUYIN_CORRECT_SHUFFLE = 1U << 31` (`pinyin_custom2.h:91`).
pub const ZHUYIN_CORRECT_SHUFFLE: u32 = 1 << 31;
/// `ZHUYIN_CORRECT_ALL = 0x7U << 29` (`pinyin_custom2.h:92`).
pub const ZHUYIN_CORRECT_ALL: u32 = 0x7 << 29;

/// A bit set of parser/decoder options.
///
/// The default (`OptionBits(0)`) is the pre-option parser: no correction or
/// ambiguity aliases and no incomplete initial keys. Callers that reproduce
/// the pinned parity profile set [`PINYIN_INCOMPLETE`] explicitly.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct OptionBits(u32);

impl OptionBits {
    /// Wraps raw option bits.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Returns the raw option word.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Whether `bit` is set.
    #[must_use]
    pub const fn contains(self, bit: u32) -> bool {
        self.0 & bit != 0
    }

    /// Whether `PINYIN_INCOMPLETE` is set.
    #[must_use]
    pub const fn has_incomplete(self) -> bool {
        self.contains(PINYIN_INCOMPLETE)
    }

    /// Whether `DYNAMIC_ADJUST` is set.
    #[must_use]
    pub const fn has_dynamic_adjust(self) -> bool {
        self.contains(DYNAMIC_ADJUST)
    }

    /// Whether `FORCE_TONE` is set. Dead without `USE_TONE` — the pin
    /// nests the force-tone rejection inside the tone scan
    /// (`pinyin_parser2.cpp:176-190`), so a word like `0x1ca` (the parity
    /// word plus `FORCE_TONE`, no `USE_TONE`) is inert.
    #[must_use]
    pub const fn has_force_tone(self) -> bool {
        self.contains(FORCE_TONE)
    }

    /// Sets or clears `bit` and returns the updated set.
    #[must_use]
    pub const fn with(self, bit: u32, enabled: bool) -> Self {
        Self(if enabled { self.0 | bit } else { self.0 & !bit })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_values_match_the_frozen_header() {
        assert_eq!(PINYIN_INCOMPLETE, 0x8);
        assert_eq!(ZHUYIN_INCOMPLETE, 0x10);
        assert_eq!(USE_TONE, 0x20);
        assert_eq!(FORCE_TONE, 0x40);
        assert_eq!(DYNAMIC_ADJUST, 0x200);
        assert_eq!(PINYIN_AMB_ALL, 0x3ff << 10);
        assert_eq!(PINYIN_CORRECT_ALL, 0xff << 21);
        assert_eq!(ZHUYIN_CORRECT_SHUFFLE, 0x8000_0000);
        assert_eq!(ZHUYIN_CORRECT_ALL, 0x7 << 29);
        assert_eq!(
            PINYIN_INCOMPLETE | (1 << 4) | PINYIN_CORRECT_ALL | (1 << 7) | (1 << 8),
            0x1fe0_0198
        );
    }

    #[test]
    fn set_and_clear_is_a_bit_operation() {
        let options = OptionBits::default().with(PINYIN_INCOMPLETE, true);
        assert!(options.has_incomplete());
        assert!(!options.has_dynamic_adjust());
        assert_eq!(
            options.with(PINYIN_INCOMPLETE, false),
            OptionBits::default()
        );
    }
}
