//! Compare both headers with oxpinyin constants and the pin's keyboard scheme values.

use super::bindings::{pinyin, zhuyin};
use oxpinyin::{OptionBits, vocab};

macro_rules! check {
    ($name:ident, $value:expr) => {{
        let expected = $value;
        assert_eq!(pinyin::$name, expected, "pinyin.h: {}", stringify!($name));
        assert_eq!(zhuyin::$name, expected, "zhuyin.h: {}", stringify!($name));
    }};
}

macro_rules! check_options {
    ($($flag:ident),+ $(,)?) => {
        $(check!($flag, OptionBits::$flag.bits());)+
    };
}

#[test]
fn header_constants_match_rust_and_pinned_schemes() {
    check_options!(
        PINYIN_INCOMPLETE,
        ZHUYIN_INCOMPLETE,
        USE_TONE,
        USE_DIVIDED_TABLE,
        USE_RESPLIT_TABLE,
        DYNAMIC_ADJUST,
        PINYIN_AMB_C_CH,
        PINYIN_AMB_S_SH,
        PINYIN_AMB_Z_ZH,
        PINYIN_AMB_F_H,
        PINYIN_AMB_G_K,
        PINYIN_AMB_L_N,
        PINYIN_AMB_L_R,
        PINYIN_AMB_AN_ANG,
        PINYIN_AMB_EN_ENG,
        PINYIN_AMB_IN_ING,
        PINYIN_AMB_ALL,
        PINYIN_CORRECT_GN_NG,
        PINYIN_CORRECT_MG_NG,
        PINYIN_CORRECT_IOU_IU,
        PINYIN_CORRECT_UEI_UI,
        PINYIN_CORRECT_UEN_UN,
        PINYIN_CORRECT_UE_VE,
        PINYIN_CORRECT_V_U,
        PINYIN_CORRECT_ON_ONG,
        PINYIN_CORRECT_ALL,
    );
    // src/storage/pinyin_custom2.h:110,116,123,132 at 074a2219.
    check!(DOUBLE_PINYIN_DEFAULT, 2);
    check!(DOUBLE_PINYIN_MS, 2);
    check!(ZHUYIN_DEFAULT, 1);
    check!(ZHUYIN_STANDARD, 1);
    check!(PHRASE_MASK, vocab::PHRASE_MASK);
    check!(PHRASE_INDEX_LIBRARY_MASK, vocab::PHRASE_INDEX_LIBRARY_MASK);
    check!(ADDON_DICTIONARY, u32::from(vocab::ADDON_DICTIONARY));
    check!(NETWORK_DICTIONARY, u32::from(vocab::NETWORK_DICTIONARY));
    check!(USER_DICTIONARY, u32::from(vocab::USER_DICTIONARY));
    check!(null_token, vocab::NULL_TOKEN);
    check!(
        OXPINYIN_USER_TOKEN,
        vocab::phrase_index_make_token(vocab::USER_DICTIONARY, vocab::NULL_TOKEN)
    );
}
