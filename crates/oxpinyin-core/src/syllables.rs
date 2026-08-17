//! Frozen complete full-pinyin syllable inventory.

use crate::options::{
    OptionBits, PINYIN_AMB_S_SH, PINYIN_AMB_Z_ZH, PINYIN_CORRECT_GN_NG, PINYIN_CORRECT_IOU_IU,
    PINYIN_CORRECT_MG_NG, PINYIN_CORRECT_ON_ONG, PINYIN_CORRECT_UE_VE, PINYIN_CORRECT_UEI_UI,
    PINYIN_CORRECT_UEN_UN, PINYIN_CORRECT_V_U,
};

/// Number of complete syllables in [`FULL_PINYIN_SYLLABLES`].
pub const FULL_PINYIN_SYLLABLE_COUNT: usize = 405;

/// Complete untuned full-pinyin syllables in pinned upstream numeric-ID order.
///
/// This is the active `PINYIN_DICT` key set from libpinyin `2.11.91`; partial
/// initials, correction aliases, tones, and commented entries are excluded.
pub static FULL_PINYIN_SYLLABLES: [&str; FULL_PINYIN_SYLLABLE_COUNT] = [
    "a", "ai", "an", "ang", "ao", "ba", "bai", "ban", "bang", "bao", "bei", "ben", "beng", "bi",
    "bian", "biao", "bie", "bin", "bing", "bo", "bu", "ca", "cai", "can", "cang", "cao", "ce",
    "cen", "ceng", "ci", "cong", "cou", "cu", "cuan", "cui", "cun", "cuo", "cha", "chai", "chan",
    "chang", "chao", "che", "chen", "cheng", "chi", "chong", "chou", "chu", "chuai", "chuan",
    "chuang", "chui", "chun", "chuo", "da", "dai", "dan", "dang", "dao", "de", "dei", "deng", "di",
    "dia", "dian", "diao", "die", "ding", "diu", "dong", "dou", "du", "duan", "dui", "dun", "duo",
    "e", "ei", "en", "er", "fa", "fan", "fang", "fei", "fen", "feng", "fo", "fou", "fu", "ga",
    "gai", "gan", "gang", "gao", "ge", "gei", "gen", "geng", "gong", "gou", "gu", "gua", "guai",
    "guan", "guang", "gui", "gun", "guo", "ha", "hai", "han", "hang", "hao", "he", "hei", "hen",
    "heng", "hong", "hou", "hu", "hua", "huai", "huan", "huang", "hui", "hun", "huo", "ji", "jia",
    "jian", "jiang", "jiao", "jie", "jin", "jing", "jiong", "jiu", "ju", "juan", "jue", "jun",
    "ka", "kai", "kan", "kang", "kao", "ke", "ken", "keng", "kong", "kou", "ku", "kua", "kuai",
    "kuan", "kuang", "kui", "kun", "kuo", "la", "lai", "lan", "lang", "lao", "le", "lei", "leng",
    "li", "lia", "lian", "liang", "liao", "lie", "lin", "ling", "liu", "lo", "long", "lou", "lu",
    "luan", "lun", "luo", "lv", "lve", "ma", "mai", "man", "mang", "mao", "me", "mei", "men",
    "meng", "mi", "mian", "miao", "mie", "min", "ming", "miu", "mo", "mou", "mu", "na", "nai",
    "nan", "nang", "nao", "ne", "nei", "nen", "neng", "ni", "nian", "niang", "niao", "nie", "nin",
    "ning", "niu", "ng", "nong", "nou", "nu", "nuan", "nuo", "nv", "nve", "o", "ou", "pa", "pai",
    "pan", "pang", "pao", "pei", "pen", "peng", "pi", "pian", "piao", "pie", "pin", "ping", "po",
    "pou", "pu", "qi", "qia", "qian", "qiang", "qiao", "qie", "qin", "qing", "qiong", "qiu", "qu",
    "quan", "que", "qun", "ran", "rang", "rao", "re", "ren", "reng", "ri", "rong", "rou", "ru",
    "ruan", "rui", "run", "ruo", "sa", "sai", "san", "sang", "sao", "se", "sen", "seng", "si",
    "song", "sou", "su", "suan", "sui", "sun", "suo", "sha", "shai", "shan", "shang", "shao",
    "she", "shei", "shen", "sheng", "shi", "shou", "shu", "shua", "shuai", "shuan", "shuang",
    "shui", "shun", "shuo", "ta", "tai", "tan", "tang", "tao", "te", "teng", "ti", "tian", "tiao",
    "tie", "ting", "tong", "tou", "tu", "tuan", "tui", "tun", "tuo", "wa", "wai", "wan", "wang",
    "wei", "wen", "weng", "wo", "wu", "xi", "xia", "xian", "xiang", "xiao", "xie", "xin", "xing",
    "xiong", "xiu", "xu", "xuan", "xue", "xun", "ya", "yan", "yang", "yao", "ye", "yi", "yin",
    "ying", "yo", "yong", "you", "yu", "yuan", "yue", "yun", "za", "zai", "zan", "zang", "zao",
    "ze", "zei", "zen", "zeng", "zi", "zong", "zou", "zu", "zuan", "zui", "zun", "zuo", "zha",
    "zhai", "zhan", "zhang", "zhao", "zhe", "zhen", "zheng", "zhi", "zhong", "zhou", "zhu", "zhua",
    "zhuai", "zhuan", "zhuang", "zhui", "zhun", "zhuo",
];

/// Longest complete syllable, in bytes.
///
/// A property of [`FULL_PINYIN_SYLLABLES`], asserted by
/// `frozen_inventory_has_expected_shape`. Both the parser and the segment
/// graph bound their match length by it.
///
/// **Not constant across stages.** Six is the Stage 1 inventory's answer
/// (`chuang`, `shuang`, `zhuang`). A Stage 2 inventory — fuzzy spellings,
/// correction aliases, a second input scheme — may raise it, and the graph's
/// edge count is `O(n × MAX_SYLLABLE_LEN × kinds)`, so raising it costs
/// linearly. Read it; do not hard-code 6. The same warning applies to
/// [`crate::SYLLABLE_KEY_COUNT`], whose ids Stage 2 extends rather than
/// renumbers.
pub const MAX_SYLLABLE_LEN: usize = 6;

/// Number of initial-only keys in [`INCOMPLETE_PINYIN_KEYS`].
pub const INCOMPLETE_PINYIN_KEY_COUNT: usize = 23;

/// Initial-only pinyin keys, in ascending byte order.
///
/// An initial-only key is a non-empty proper prefix of a complete syllable
/// that contains no vowel byte (`a`, `e`, `i`, `o`, `u`, `v`) and is not itself
/// a complete syllable. Applied to [`FULL_PINYIN_SYLLABLES`] that rule yields
/// exactly these 23 keys, which `incomplete_keys_follow_the_frozen_rule`
/// asserts.
///
/// These are the keys the pinned oracle admits at any cursor position when
/// `PINYIN_INCOMPLETE` is set. `docs/findings/segment-graph.md` records the
/// measured evidence and makes them graph edges.
pub static INCOMPLETE_PINYIN_KEYS: [&str; INCOMPLETE_PINYIN_KEY_COUNT] = [
    "b", "c", "ch", "d", "f", "g", "h", "j", "k", "l", "m", "n", "p", "q", "r", "s", "sh", "t",
    "w", "x", "y", "z", "zh",
];

/// Number of option-only canonical syllables.
///
/// These are `content_table` spellings reachable only through a correction
/// alias, never through raw input: `pinyin_index` has no `eng` or `nun`
/// entry (`docs/findings/option-bits.md`).
pub const OPTION_ONLY_COMPLETE_SYLLABLE_COUNT: usize = 2;

/// Canonical option-only complete syllables, in ascending byte order.
pub static OPTION_ONLY_COMPLETE_SYLLABLES: [&str; OPTION_ONLY_COMPLETE_SYLLABLE_COUNT] =
    ["eng", "nun"];

/// A canonical complete syllable spelling, whether it is in the frozen
/// untuned inventory or an option-only correction target.
#[must_use]
pub fn canonical_complete_syllable(text: &str) -> Option<&'static str> {
    FULL_PINYIN_SYLLABLES
        .iter()
        .copied()
        .find(|syllable| *syllable == text)
        .or_else(|| {
            OPTION_ONLY_COMPLETE_SYLLABLES
                .iter()
                .copied()
                .find(|syllable| *syllable == text)
        })
}

/// The canonical spelling for an option-gated spelling alias, if enabled.
///
/// This is the parser-table half of the option bits: correction aliases and
/// the two parser-table ambiguity aliases (`sua`→`shua`, `zua`→`zhua`).
/// Matrix-level fuzzy substitutions live on [`crate::SyllableKey`] instead.
#[must_use]
pub fn option_alias_canonical(text: &str, options: OptionBits) -> Option<&'static str> {
    correction_canonical(text, options).or_else(|| ambiguity_canonical(text, options))
}

fn correction_canonical(text: &str, options: OptionBits) -> Option<&'static str> {
    let bytes = text.as_bytes();

    if options.contains(PINYIN_CORRECT_GN_NG) && text.ends_with("gn") {
        return checked_canonical(text, 2, "ng");
    }
    if options.contains(PINYIN_CORRECT_MG_NG) && text.ends_with("mg") {
        return checked_canonical(text, 2, "ng");
    }
    if options.contains(PINYIN_CORRECT_IOU_IU) && text.ends_with("iou") {
        return checked_canonical(text, 3, "iu");
    }
    if options.contains(PINYIN_CORRECT_UEI_UI) && text.ends_with("uei") {
        return checked_canonical(text, 3, "ui");
    }
    if options.contains(PINYIN_CORRECT_UEN_UN) && text.ends_with("uen") {
        return checked_canonical(text, 3, "un");
    }
    if options.contains(PINYIN_CORRECT_UE_VE) && text.ends_with("ue") {
        return checked_canonical(text, 2, "ve");
    }
    if options.contains(PINYIN_CORRECT_V_U)
        && bytes.len() >= 2
        && matches!(bytes[0], b'j' | b'q' | b'x' | b'y')
        && bytes[1] == b'v'
    {
        let mut candidate = String::with_capacity(text.len());
        candidate.push(char::from(bytes[0]));
        candidate.push('u');
        candidate.push_str(&text[2..]);
        return canonical_complete_syllable(&candidate);
    }
    if options.contains(PINYIN_CORRECT_ON_ONG) && text.ends_with("on") && text.len() > 2 {
        let mut candidate = String::with_capacity(text.len() + 1);
        candidate.push_str(text);
        candidate.push('g');
        return canonical_complete_syllable(&candidate);
    }

    None
}

fn ambiguity_canonical(text: &str, options: OptionBits) -> Option<&'static str> {
    match text {
        "sua" if options.contains(PINYIN_AMB_S_SH) => Some("shua"),
        "zua" if options.contains(PINYIN_AMB_Z_ZH) => Some("zhua"),
        _ => None,
    }
}

fn checked_canonical(text: &str, suffix_len: usize, suffix: &str) -> Option<&'static str> {
    // Bare `gn`/`mg`/`ue`/… are not in `pinyin_index`; only the 80/80/…
    // stemmed rows are. An empty stem would invent `ng`/`ve`.
    let base_len = text.len().checked_sub(suffix_len)?;
    if base_len == 0 {
        return None;
    }
    let mut candidate = String::with_capacity(base_len + suffix.len());
    candidate.push_str(&text[..base_len]);
    candidate.push_str(suffix);
    canonical_complete_syllable(&candidate)
}

/// Longest initial-only key that prefixes `text`.
///
/// `None` for a vowel-initial spelling. `ng` returns `Some("n")` because
/// `"n"` is an inventory prefix; callers that need the chewing `m_initial`
/// (zero-initial `ng`) should use [`phonetic_initial`].
#[must_use]
pub fn syllable_initial(text: &str) -> Option<&'static str> {
    INCOMPLETE_PINYIN_KEYS
        .iter()
        .filter(|key| text.starts_with(**key))
        .max_by_key(|key| key.len())
        .copied()
}

/// Phonetic initial used for incomplete-key expansion.
///
/// Matches `ChewingKey.m_initial`: `zh`/`ch`/`sh` stay two-byte initials,
/// and `ng` is zero-initial (`pinyin_parser_table.h:4211`).
#[must_use]
pub fn phonetic_initial(text: &str) -> Option<&'static str> {
    if text == "ng" {
        return None;
    }
    syllable_initial(text)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        FULL_PINYIN_SYLLABLE_COUNT, FULL_PINYIN_SYLLABLES, INCOMPLETE_PINYIN_KEY_COUNT,
        INCOMPLETE_PINYIN_KEYS, MAX_SYLLABLE_LEN,
    };

    /// Whether `byte` is a vowel for the initial-only key rule.
    ///
    /// `v` counts as a vowel because it spells `ü` in `lv`, `lve`, `nv` and
    /// `nve`, so `lv` is not an initial.
    const fn is_vowel(byte: u8) -> bool {
        matches!(byte, b'a' | b'e' | b'i' | b'o' | b'u' | b'v')
    }

    #[test]
    fn frozen_inventory_has_expected_shape() {
        assert_eq!(FULL_PINYIN_SYLLABLES.len(), FULL_PINYIN_SYLLABLE_COUNT);

        let mut seen = HashSet::with_capacity(FULL_PINYIN_SYLLABLE_COUNT);
        for syllable in FULL_PINYIN_SYLLABLES {
            assert!(!syllable.is_empty(), "syllables are non-empty");
            assert!(
                syllable.bytes().all(|byte| byte.is_ascii_lowercase()),
                "{syllable:?} is not lowercase ASCII"
            );
            assert!(seen.insert(syllable), "duplicate syllable {syllable:?}");
        }

        assert_eq!(
            FULL_PINYIN_SYLLABLES
                .iter()
                .map(|syllable| syllable.len())
                .max(),
            Some(MAX_SYLLABLE_LEN)
        );

        for required in ["ng", "lv", "lve", "nv", "nve", "zhuan"] {
            assert!(FULL_PINYIN_SYLLABLES.contains(&required));
        }
        for excluded in ["b", "zh", "den", "kei", "lue", "nue", "tei", "eng"] {
            assert!(!FULL_PINYIN_SYLLABLES.contains(&excluded));
        }
    }

    #[test]
    fn incomplete_keys_follow_the_frozen_rule() {
        let complete: HashSet<&str> = FULL_PINYIN_SYLLABLES.into_iter().collect();
        let mut derived: Vec<&str> = Vec::new();

        for syllable in FULL_PINYIN_SYLLABLES {
            for length in 1..syllable.len() {
                let prefix = &syllable[..length];
                if prefix.bytes().any(is_vowel)
                    || complete.contains(prefix)
                    || derived.contains(&prefix)
                {
                    continue;
                }
                derived.push(prefix);
            }
        }
        derived.sort_unstable();

        assert_eq!(derived, INCOMPLETE_PINYIN_KEYS);
        assert_eq!(INCOMPLETE_PINYIN_KEYS.len(), INCOMPLETE_PINYIN_KEY_COUNT);
    }

    #[test]
    fn phonetic_initial_follows_m_initial() {
        use super::{phonetic_initial, syllable_initial};

        assert_eq!(syllable_initial("ng"), Some("n"));
        assert_eq!(phonetic_initial("ng"), None);
        assert_eq!(phonetic_initial("na"), Some("n"));
        assert_eq!(phonetic_initial("zhong"), Some("zh"));
        assert_eq!(phonetic_initial("za"), Some("z"));
        assert_eq!(phonetic_initial("an"), None);
        assert_eq!(phonetic_initial("er"), None);
    }

    #[test]
    fn incomplete_keys_are_sorted_and_disjoint_from_complete_keys() {
        let mut previous = "";
        for key in INCOMPLETE_PINYIN_KEYS {
            assert!(previous < key, "{key:?} is out of ascending order");
            assert!(!FULL_PINYIN_SYLLABLES.contains(&key));
            previous = key;
        }
    }

    #[test]
    fn correction_aliases_resolve_to_their_canonical_syllable() {
        use super::{
            OptionBits, PINYIN_CORRECT_GN_NG, PINYIN_CORRECT_IOU_IU, PINYIN_CORRECT_MG_NG,
            PINYIN_CORRECT_ON_ONG, PINYIN_CORRECT_UE_VE, PINYIN_CORRECT_UEI_UI,
            PINYIN_CORRECT_UEN_UN, PINYIN_CORRECT_V_U, option_alias_canonical,
        };

        let cases = [
            ("agn", "ang", PINYIN_CORRECT_GN_NG),
            ("amg", "ang", PINYIN_CORRECT_MG_NG),
            ("diou", "diu", PINYIN_CORRECT_IOU_IU),
            ("duei", "dui", PINYIN_CORRECT_UEI_UI),
            ("duen", "dun", PINYIN_CORRECT_UEN_UN),
            ("lue", "lve", PINYIN_CORRECT_UE_VE),
            ("jv", "ju", PINYIN_CORRECT_V_U),
        ];
        for (alias, canonical, bit) in cases {
            assert_eq!(
                option_alias_canonical(alias, OptionBits::from_bits(bit)),
                Some(canonical)
            );
            assert_eq!(option_alias_canonical(alias, OptionBits::default()), None);
        }
        assert_eq!(
            option_alias_canonical("zon", OptionBits::from_bits(1 << 28)),
            Some("zong")
        );
        // Bare gn/mg/on are not in pinyin_index (80/80/17 stemmed rows).
        assert_eq!(
            option_alias_canonical("gn", OptionBits::from_bits(PINYIN_CORRECT_GN_NG)),
            None
        );
        assert_eq!(
            option_alias_canonical("mg", OptionBits::from_bits(PINYIN_CORRECT_MG_NG)),
            None
        );
        assert_eq!(
            option_alias_canonical("on", OptionBits::from_bits(PINYIN_CORRECT_ON_ONG)),
            None
        );
    }
}
