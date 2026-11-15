//! Parser/decoder option bits.
//!
//! The flag values are the `PinyinTableFlag`, `PinyinAmbiguity2` and
//! `PinyinCorrection2` enumerators from libpinyin `pinyin_custom2.h`. The
//! C option word converts with [`OptionBits::from_bits_retain`], so bits
//! without a name pass through unchanged.

bitflags::bitflags! {
    /// A bit set of parser/decoder options.
    ///
    /// The default (empty) set is the pre-option parser: no correction or
    /// ambiguity aliases and no incomplete initial keys. Callers that
    /// reproduce the pinned parity profile set
    /// [`PINYIN_INCOMPLETE`](Self::PINYIN_INCOMPLETE) explicitly.
    #[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
    pub struct OptionBits: u32 {
        /// `PINYIN_INCOMPLETE = 1U << 3` (`pinyin_custom2.h:34`).
        const PINYIN_INCOMPLETE = 1 << 3;

        /// `ZHUYIN_INCOMPLETE = 1U << 4` (`pinyin_custom2.h:35`).
        const ZHUYIN_INCOMPLETE = 1 << 4;

        /// `USE_TONE = 1U << 5` (`pinyin_custom2.h:36`).
        const USE_TONE = 1 << 5;

        /// `FORCE_TONE = 1U << 6` (`pinyin_custom2.h:37`). Rejected by the
        /// `pinyin_custom2.h` header mirror in `oxpinyin-capi`.
        ///
        /// The bit arrives only through the raw `pinyin_set_options` word; the
        /// parser honours it nested inside `USE_TONE` exactly like the pin
        /// (`pinyin_parser2.cpp:176-190`), and nowhere else — the double-pinyin
        /// and zhuyin shapes are a different law, unmeasured here
        /// (`docs/findings/compatibility-policy.md`, rows 16, 24 and 30).
        const FORCE_TONE = 1 << 6;

        /// `USE_DIVIDED_TABLE = 1U << 7` (`pinyin_custom2.h:38`).
        ///
        /// Gates the divided-syllable alternates that `inner_split_step` adds to
        /// the matrix (`phonetic_key_matrix.cpp:169-171`): without this bit the
        /// divided pairs for `jie` → `ji` + `e`, `xian` → `xi` + `an`, etc. are
        /// not in the lookup inventory.
        const USE_DIVIDED_TABLE = 1 << 7;

        /// `USE_RESPLIT_TABLE = 1U << 8` (`pinyin_custom2.h:39`).
        ///
        /// Gates the resplit alternates that `resplit_step` adds to the matrix
        /// (`phonetic_key_matrix.cpp:87-89`): without this bit the resplit pairs
        /// for `fangan` → `fang` + `an`, etc. are not in the lookup inventory.
        const USE_RESPLIT_TABLE = 1 << 8;

        /// `DYNAMIC_ADJUST = 1U << 9` (`pinyin_custom2.h:40`).
        const DYNAMIC_ADJUST = 1 << 9;

        /// `PINYIN_AMB_C_CH = 1U << 10` (`pinyin_custom2.h:50`).
        const PINYIN_AMB_C_CH = 1 << 10;
        /// `PINYIN_AMB_S_SH = 1U << 11` (`pinyin_custom2.h:51`).
        const PINYIN_AMB_S_SH = 1 << 11;
        /// `PINYIN_AMB_Z_ZH = 1U << 12` (`pinyin_custom2.h:52`).
        const PINYIN_AMB_Z_ZH = 1 << 12;
        /// `PINYIN_AMB_F_H = 1U << 13` (`pinyin_custom2.h:53`).
        const PINYIN_AMB_F_H = 1 << 13;
        /// `PINYIN_AMB_G_K = 1U << 14` (`pinyin_custom2.h:54`).
        const PINYIN_AMB_G_K = 1 << 14;
        /// `PINYIN_AMB_L_N = 1U << 15` (`pinyin_custom2.h:55`).
        const PINYIN_AMB_L_N = 1 << 15;
        /// `PINYIN_AMB_L_R = 1U << 16` (`pinyin_custom2.h:56`).
        const PINYIN_AMB_L_R = 1 << 16;
        /// `PINYIN_AMB_AN_ANG = 1U << 17` (`pinyin_custom2.h:57`).
        const PINYIN_AMB_AN_ANG = 1 << 17;
        /// `PINYIN_AMB_EN_ENG = 1U << 18` (`pinyin_custom2.h:58`).
        const PINYIN_AMB_EN_ENG = 1 << 18;
        /// `PINYIN_AMB_IN_ING = 1U << 19` (`pinyin_custom2.h:59`).
        const PINYIN_AMB_IN_ING = 1 << 19;
        /// `PINYIN_AMB_ALL = 0x3FFU << 10` (`pinyin_custom2.h:60`).
        const PINYIN_AMB_ALL = 0x3ff << 10;

        /// `PINYIN_CORRECT_GN_NG = 1U << 21` (`pinyin_custom2.h:71`).
        const PINYIN_CORRECT_GN_NG = 1 << 21;
        /// `PINYIN_CORRECT_MG_NG = 1U << 22` (`pinyin_custom2.h:72`).
        const PINYIN_CORRECT_MG_NG = 1 << 22;
        /// `PINYIN_CORRECT_IOU_IU = 1U << 23` (`pinyin_custom2.h:73`).
        const PINYIN_CORRECT_IOU_IU = 1 << 23;
        /// `PINYIN_CORRECT_UEI_UI = 1U << 24` (`pinyin_custom2.h:74`).
        const PINYIN_CORRECT_UEI_UI = 1 << 24;
        /// `PINYIN_CORRECT_UEN_UN = 1U << 25` (`pinyin_custom2.h:75`).
        const PINYIN_CORRECT_UEN_UN = 1 << 25;
        /// `PINYIN_CORRECT_UE_VE = 1U << 26` (`pinyin_custom2.h:76`).
        const PINYIN_CORRECT_UE_VE = 1 << 26;
        /// `PINYIN_CORRECT_V_U = 1U << 27` (`pinyin_custom2.h:77`).
        const PINYIN_CORRECT_V_U = 1 << 27;
        /// `PINYIN_CORRECT_ON_ONG = 1U << 28` (`pinyin_custom2.h:78`).
        const PINYIN_CORRECT_ON_ONG = 1 << 28;
        /// `PINYIN_CORRECT_ALL = 0xFFU << 21` (`pinyin_custom2.h:79`).
        const PINYIN_CORRECT_ALL = 0xff << 21;

        /// `ZHUYIN_CORRECT_HSU = 1U << 29` (`pinyin_custom2.h:89`).
        const ZHUYIN_CORRECT_HSU = 1 << 29;
        /// `ZHUYIN_CORRECT_ETEN26 = 1U << 30` (`pinyin_custom2.h:90`).
        const ZHUYIN_CORRECT_ETEN26 = 1 << 30;
        /// `ZHUYIN_CORRECT_SHUFFLE = 1U << 31` (`pinyin_custom2.h:91`).
        const ZHUYIN_CORRECT_SHUFFLE = 1 << 31;
        /// `ZHUYIN_CORRECT_ALL = 0x7U << 29` (`pinyin_custom2.h:92`).
        const ZHUYIN_CORRECT_ALL = 0x7 << 29;

        // Every bit is known, so the C word round-trips with its unnamed bits.
        const _ = !0;
    }
}

/// `u32` spellings of the row flags that the generated zhuyin index rows
/// (`table_data/zhuyin_indexes.rs`) name. `just rust-tables` emits the bare
/// names, and its acceptance digests (`tools/scripts2/acceptance.json`)
/// cover those tokens.
pub(crate) mod table_row {
    use super::OptionBits;

    pub(crate) const ZHUYIN_INCOMPLETE: u32 = OptionBits::ZHUYIN_INCOMPLETE.bits();
    pub(crate) const ZHUYIN_CORRECT_ETEN26: u32 = OptionBits::ZHUYIN_CORRECT_ETEN26.bits();
    pub(crate) const ZHUYIN_CORRECT_HSU: u32 = OptionBits::ZHUYIN_CORRECT_HSU.bits();
    pub(crate) const ZHUYIN_CORRECT_SHUFFLE: u32 = OptionBits::ZHUYIN_CORRECT_SHUFFLE.bits();
}

#[cfg(test)]
mod tests {
    use super::OptionBits;

    #[test]
    fn bit_values_match_the_frozen_header() {
        assert_eq!(OptionBits::PINYIN_INCOMPLETE.bits(), 0x8);
        assert_eq!(OptionBits::ZHUYIN_INCOMPLETE.bits(), 0x10);
        assert_eq!(OptionBits::USE_TONE.bits(), 0x20);
        assert_eq!(OptionBits::FORCE_TONE.bits(), 0x40);
        assert_eq!(OptionBits::USE_DIVIDED_TABLE.bits(), 0x080);
        assert_eq!(OptionBits::USE_RESPLIT_TABLE.bits(), 0x100);
        assert_eq!(OptionBits::DYNAMIC_ADJUST.bits(), 0x200);
        assert_eq!(OptionBits::PINYIN_AMB_ALL.bits(), 0x3ff << 10);
        assert_eq!(OptionBits::PINYIN_CORRECT_ALL.bits(), 0xff << 21);
        assert_eq!(OptionBits::ZHUYIN_CORRECT_SHUFFLE.bits(), 0x8000_0000);
        assert_eq!(OptionBits::ZHUYIN_CORRECT_ALL.bits(), 0x7 << 29);
        assert_eq!(
            (OptionBits::PINYIN_INCOMPLETE
                | OptionBits::ZHUYIN_INCOMPLETE
                | OptionBits::PINYIN_CORRECT_ALL
                | OptionBits::USE_DIVIDED_TABLE
                | OptionBits::USE_RESPLIT_TABLE)
                .bits(),
            0x1fe0_0198
        );
    }

    #[test]
    fn set_and_clear_is_a_bit_operation() {
        let mut options = OptionBits::default();
        options.set(OptionBits::PINYIN_INCOMPLETE, true);
        assert!(options.contains(OptionBits::PINYIN_INCOMPLETE));
        assert!(!options.contains(OptionBits::DYNAMIC_ADJUST));
        options.set(OptionBits::PINYIN_INCOMPLETE, false);
        assert_eq!(options, OptionBits::default());
    }

    #[test]
    fn unnamed_bits_round_trip() {
        // Bits 0 to 2 and 20 have no `OptionBits` flag (`pinyin_custom2.h:31-32`
        // names bits 1 and 2 as table-row flags only).
        for word in [0x1, 0x7, 1 << 20, 0x0010_0128, u32::MAX] {
            assert_eq!(OptionBits::from_bits_retain(word).bits(), word);
        }
        let mut options = OptionBits::from_bits_retain(0x5 | (1 << 20));
        options.set(OptionBits::USE_TONE, true);
        options.remove(OptionBits::PINYIN_CORRECT_ALL);
        assert_eq!(options.bits(), 0x5 | (1 << 20) | 0x20);
    }
}
