//! Options for parsing input, expanding the syllable matrix and ranking candidates.

// The flag values come from PinyinTableFlag, PinyinAmbiguity2 and PinyinCorrection2 in
// pinyin_custom2.h:31–92.

bitflags::bitflags! {
    /// The parser and decoder option word.
    ///
    /// The default set is empty. [`Self::from_bits_retain`] preserves unnamed bits from a
    /// C option word. See the starting words for the
    /// [`pinyin`](crate::facade::PINYIN_DEFAULT_OPTION_WORD) and
    /// [`Zhuyin`](crate::facade::ZHUYIN_DEFAULT_OPTION_WORD) facades, and
    /// [`crate::engine::SessionSettings`] for the engine defaults.
    #[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
    pub struct OptionBits: u32 {
        // pinyin_custom2.h:34.
        /// Admits initial-only pinyin spellings.
        const PINYIN_INCOMPLETE = 1 << 3;

        // pinyin_custom2.h:35.
        /// Admits initial-only Zhuyin spellings.
        const ZHUYIN_INCOMPLETE = 1 << 4;

        // pinyin_custom2.h:36.
        /// Reads tone digits or tone keys in parsers that support them.
        const USE_TONE = 1 << 5;

        // pinyin_custom2.h:37 (full pinyin: pinyin_parser2.cpp:176–190; double: 403–451; Zhuyin:
        // zhuyin_parser2.cpp:162–179,335–388,595–604).
        /// Requires an explicit tone according to the rules of
        /// [`crate::FullPinyinParser::parse_one_key`],
        /// [`crate::DoublePinyinParser::parse_one_key`] or
        /// [`crate::ZhuyinParser::parse_one_key`].
        const FORCE_TONE = 1 << 6;

        // pinyin_custom2.h:38 and phonetic_key_matrix.cpp:169–172.
        /// Adds divided-syllable alternatives to the lookup matrix.
        const USE_DIVIDED_TABLE = 1 << 7;

        // pinyin_custom2.h:39 and phonetic_key_matrix.cpp:87–90.
        /// Adds alternative splits of adjacent syllables to the lookup matrix.
        const USE_RESPLIT_TABLE = 1 << 8;

        // pinyin_custom2.h:40 and pinyin.cpp:1845–1866.
        /// Includes the previous token’s bigram probability in candidate ranking. A missing gram
        /// or a zero row total contributes zero.
        const DYNAMIC_ADJUST = 1 << 9;

        // pinyin_custom2.h:50, phonetic_key_matrix.cpp:238–306.
        /// Adds matrix alternatives interchanging `c` and `ch`.
        const PINYIN_AMB_C_CH = 1 << 10;
        // pinyin_custom2.h:51, phonetic_key_matrix.cpp:238–306.
        /// Adds matrix alternatives interchanging `s` and `sh` and admits the spelling alias
        /// `sua`.
        const PINYIN_AMB_S_SH = 1 << 11;
        // pinyin_custom2.h:52, phonetic_key_matrix.cpp:238–306.
        /// Adds matrix alternatives interchanging `z` and `zh` and admits the spelling alias
        /// `zua`.
        const PINYIN_AMB_Z_ZH = 1 << 12;
        // pinyin_custom2.h:53, phonetic_key_matrix.cpp:238–306.
        /// Adds matrix alternatives interchanging `f` and `h`.
        const PINYIN_AMB_F_H = 1 << 13;
        // pinyin_custom2.h:54, phonetic_key_matrix.cpp:238–306.
        /// Adds matrix alternatives interchanging `g` and `k`.
        const PINYIN_AMB_G_K = 1 << 14;
        // pinyin_custom2.h:55, phonetic_key_matrix.cpp:238–306.
        /// Adds matrix alternatives interchanging `l` and `n`.
        const PINYIN_AMB_L_N = 1 << 15;
        // pinyin_custom2.h:56, phonetic_key_matrix.cpp:238–306.
        /// Adds matrix alternatives interchanging `l` and `r`.
        const PINYIN_AMB_L_R = 1 << 16;
        // pinyin_custom2.h:57, phonetic_key_matrix.cpp:238–306.
        /// Adds matrix alternatives interchanging `an` and `ang`.
        const PINYIN_AMB_AN_ANG = 1 << 17;
        // pinyin_custom2.h:58, phonetic_key_matrix.cpp:238–306.
        /// Adds matrix alternatives interchanging `en` and `eng`.
        const PINYIN_AMB_EN_ENG = 1 << 18;
        // pinyin_custom2.h:59, phonetic_key_matrix.cpp:238–306.
        /// Adds matrix alternatives interchanging `in` and `ing`.
        const PINYIN_AMB_IN_ING = 1 << 19;
        // pinyin_custom2.h:60.
        /// Mask of all initial and final ambiguity options.
        const PINYIN_AMB_ALL = 0x3ff << 10;

        // pinyin_custom2.h:71, scripts2/options.py:23–32.
        /// Admits indexed spelling aliases with `gn` for `ng`.
        const PINYIN_CORRECT_GN_NG = 1 << 21;
        // pinyin_custom2.h:72, scripts2/options.py:23–32.
        /// Admits indexed spelling aliases with `mg` for `ng`.
        const PINYIN_CORRECT_MG_NG = 1 << 22;
        // pinyin_custom2.h:73, scripts2/options.py:23–32.
        /// Admits indexed spelling aliases with `iou` for `iu`.
        const PINYIN_CORRECT_IOU_IU = 1 << 23;
        // pinyin_custom2.h:74, scripts2/options.py:23–32.
        /// Admits indexed spelling aliases with `uei` for `ui`.
        const PINYIN_CORRECT_UEI_UI = 1 << 24;
        // pinyin_custom2.h:75, scripts2/options.py:23–32.
        /// Admits indexed spelling aliases with `uen` for `un`.
        const PINYIN_CORRECT_UEN_UN = 1 << 25;
        // pinyin_custom2.h:76, scripts2/options.py:23–32.
        /// Admits indexed spelling aliases with `ue` for `ve`.
        const PINYIN_CORRECT_UE_VE = 1 << 26;
        // pinyin_custom2.h:77, scripts2/options.py:23–32.
        /// Admits indexed spelling aliases with `v` for `u`.
        const PINYIN_CORRECT_V_U = 1 << 27;
        // pinyin_custom2.h:78, scripts2/options.py:23–32.
        /// Admits indexed spelling aliases with `on` for `ong`.
        const PINYIN_CORRECT_ON_ONG = 1 << 28;
        // pinyin_custom2.h:79.
        /// Mask of all pinyin spelling corrections.
        const PINYIN_CORRECT_ALL = 0xff << 21;

        // pinyin_custom2.h:89.
        /// Admits Hsu keyboard correction rows.
        const ZHUYIN_CORRECT_HSU = 1 << 29;
        // pinyin_custom2.h:90.
        /// Admits Eten26 keyboard correction rows.
        const ZHUYIN_CORRECT_ETEN26 = 1 << 30;
        // pinyin_custom2.h:91.
        /// Admits shuffled Zhuyin spellings.
        const ZHUYIN_CORRECT_SHUFFLE = 1 << 31;
        // pinyin_custom2.h:92.
        /// Mask of all Zhuyin keyboard corrections.
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
mod tests;
