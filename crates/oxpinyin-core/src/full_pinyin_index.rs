//! Full-pinyin scheme indexes: the pinned `luoma_pinyin_index` and
//! `secondary_zhuyin_index`.
//!
//! Extracted from libpinyin `2.11.91`
//! (`0c5e80e1200f84fab185d1c5bde458b770a0636c`)
//! `src/storage/pinyin_parser_table.h:674/1083`, joined to canonical
//! spellings through `content_table`. Both indexes are plain
//! `IS_PINYIN` spelling→content maps — no correction aliases, no
//! incomplete rows, no ambiguities — and both match their
//! `content_table` spelling columns exactly. Source-embedded tables,
//! copyable under the project source policy; not model-archive data.

/// Number of entries in [`LUOMA_PINYIN_INDEX`].
pub const LUOMA_PINYIN_COUNT: usize = 406;

/// `(input spelling, canonical pinyin)` pairs from `luoma_pinyin_index[]`
/// (`pinyin_parser_table.h:674`, equal to the `m_luoma_pinyin_str`
/// column of `content_table`), sorted by spelling — the
/// binary-search invariant of the pinned table.
pub static LUOMA_PINYIN_INDEX: [(&str, &str); LUOMA_PINYIN_COUNT] = [
    ("a", "a"),
    ("ai", "ai"),
    ("an", "an"),
    ("ang", "ang"),
    ("ao", "ao"),
    ("ba", "ba"),
    ("bai", "bai"),
    ("ban", "ban"),
    ("bang", "bang"),
    ("bao", "bao"),
    ("bei", "bei"),
    ("ben", "ben"),
    ("beng", "beng"),
    ("bi", "bi"),
    ("bian", "bian"),
    ("biao", "biao"),
    ("bieh", "bie"),
    ("bin", "bin"),
    ("bing", "bing"),
    ("bo", "bo"),
    ("bu", "bu"),
    ("cha", "cha"),
    ("chai", "chai"),
    ("chan", "chan"),
    ("chang", "chang"),
    ("chao", "chao"),
    ("che", "che"),
    ("chen", "chen"),
    ("cheng", "cheng"),
    ("chi", "qi"),
    ("chia", "qia"),
    ("chian", "qian"),
    ("chiang", "qiang"),
    ("chiao", "qiao"),
    ("chieh", "qie"),
    ("chih", "ch"),
    ("chin", "qin"),
    ("ching", "qing"),
    ("chiou", "qiu"),
    ("chong", "chong"),
    ("chou", "chou"),
    ("chu", "chu"),
    ("chuai", "chuai"),
    ("chuan", "chuan"),
    ("chuang", "chuang"),
    ("chuei", "chui"),
    ("chun", "chun"),
    ("chuo", "chuo"),
    ("chyong", "qiong"),
    ("chyu", "qu"),
    ("chyuan", "quan"),
    ("chyueh", "que"),
    ("chyun", "qun"),
    ("da", "da"),
    ("dai", "dai"),
    ("dan", "dan"),
    ("dang", "dang"),
    ("dao", "dao"),
    ("de", "de"),
    ("dei", "dei"),
    ("deng", "deng"),
    ("di", "di"),
    ("dian", "dian"),
    ("diao", "diao"),
    ("dieh", "die"),
    ("ding", "ding"),
    ("diou", "diu"),
    ("dong", "dong"),
    ("dou", "dou"),
    ("du", "du"),
    ("duan", "duan"),
    ("duei", "dui"),
    ("dun", "dun"),
    ("duo", "duo"),
    ("e", "e"),
    ("ei", "ei"),
    ("en", "en"),
    ("eng", "eng"),
    ("er", "er"),
    ("fa", "fa"),
    ("fan", "fan"),
    ("fang", "fang"),
    ("fei", "fei"),
    ("fen", "fen"),
    ("fo", "fo"),
    ("fou", "fou"),
    ("fu", "fu"),
    ("ga", "ga"),
    ("gai", "gai"),
    ("gan", "gan"),
    ("gang", "gang"),
    ("gao", "gao"),
    ("ge", "ge"),
    ("gei", "gei"),
    ("gen", "gen"),
    ("geng", "geng"),
    ("gong", "gong"),
    ("gou", "gou"),
    ("gu", "gu"),
    ("gua", "gua"),
    ("guai", "guai"),
    ("guan", "guan"),
    ("guang", "guang"),
    ("guei", "gui"),
    ("gun", "gun"),
    ("guo", "guo"),
    ("ha", "ha"),
    ("hai", "hai"),
    ("han", "han"),
    ("hang", "hang"),
    ("hao", "hao"),
    ("he", "he"),
    ("hei", "hei"),
    ("hen", "hen"),
    ("heng", "heng"),
    ("hong", "hong"),
    ("hou", "hou"),
    ("hu", "hu"),
    ("hua", "hua"),
    ("huai", "huai"),
    ("huan", "huan"),
    ("huang", "huang"),
    ("huei", "hui"),
    ("hun", "hun"),
    ("huo", "huo"),
    ("jha", "zha"),
    ("jhai", "zhai"),
    ("jhan", "zhan"),
    ("jhang", "zhang"),
    ("jhao", "zhao"),
    ("jhe", "zhe"),
    ("jhei", "zhei"),
    ("jhen", "zhen"),
    ("jheng", "zheng"),
    ("jhih", "zh"),
    ("jhong", "zhong"),
    ("jhou", "zhou"),
    ("jhu", "zhu"),
    ("jhua", "zhua"),
    ("jhuai", "zhuai"),
    ("jhuan", "zhuan"),
    ("jhuang", "zhuang"),
    ("jhuei", "zhui"),
    ("jhun", "zhun"),
    ("jhuo", "zhuo"),
    ("ji", "ji"),
    ("jia", "jia"),
    ("jian", "jian"),
    ("jiang", "jiang"),
    ("jiao", "jiao"),
    ("jieh", "jie"),
    ("jin", "jin"),
    ("jing", "jing"),
    ("jiou", "jiu"),
    ("jyong", "jiong"),
    ("jyu", "ju"),
    ("jyuan", "juan"),
    ("jyueh", "jue"),
    ("jyun", "jun"),
    ("ka", "ka"),
    ("kai", "kai"),
    ("kan", "kan"),
    ("kang", "kang"),
    ("kao", "kao"),
    ("ke", "ke"),
    ("ken", "ken"),
    ("keng", "keng"),
    ("kong", "kong"),
    ("kou", "kou"),
    ("ku", "ku"),
    ("kua", "kua"),
    ("kuai", "kuai"),
    ("kuan", "kuan"),
    ("kuang", "kuang"),
    ("kuei", "kui"),
    ("kun", "kun"),
    ("kuo", "kuo"),
    ("la", "la"),
    ("lai", "lai"),
    ("lan", "lan"),
    ("lang", "lang"),
    ("lao", "lao"),
    ("le", "le"),
    ("lei", "lei"),
    ("leng", "leng"),
    ("li", "li"),
    ("lia", "lia"),
    ("lian", "lian"),
    ("liang", "liang"),
    ("liao", "liao"),
    ("lieh", "lie"),
    ("lin", "lin"),
    ("ling", "ling"),
    ("liou", "liu"),
    ("lo", "lo"),
    ("long", "long"),
    ("lou", "lou"),
    ("lu", "lu"),
    ("luan", "luan"),
    ("lun", "lun"),
    ("luo", "luo"),
    ("lyu", "lv"),
    ("lyueh", "lve"),
    ("ma", "ma"),
    ("mai", "mai"),
    ("man", "man"),
    ("mang", "mang"),
    ("mao", "mao"),
    ("me", "me"),
    ("mei", "mei"),
    ("men", "men"),
    ("meng", "meng"),
    ("mi", "mi"),
    ("mian", "mian"),
    ("miao", "miao"),
    ("mieh", "mie"),
    ("min", "min"),
    ("ming", "ming"),
    ("miou", "miu"),
    ("mo", "mo"),
    ("mou", "mou"),
    ("mu", "mu"),
    ("na", "na"),
    ("nai", "nai"),
    ("nan", "nan"),
    ("nang", "nang"),
    ("nao", "nao"),
    ("ne", "ne"),
    ("nei", "nei"),
    ("nen", "nen"),
    ("neng", "neng"),
    ("ni", "ni"),
    ("nian", "nian"),
    ("niang", "niang"),
    ("niao", "niao"),
    ("nieh", "nie"),
    ("nin", "nin"),
    ("ning", "ning"),
    ("niou", "niu"),
    ("nong", "nong"),
    ("nou", "nou"),
    ("nu", "nu"),
    ("nuan", "nuan"),
    ("nun", "nun"),
    ("nuo", "nuo"),
    ("nyu", "nv"),
    ("nyueh", "nve"),
    ("o", "o"),
    ("ou", "ou"),
    ("pa", "pa"),
    ("pai", "pai"),
    ("pan", "pan"),
    ("pang", "pang"),
    ("pao", "pao"),
    ("pei", "pei"),
    ("pen", "pen"),
    ("peng", "peng"),
    ("pi", "pi"),
    ("pian", "pian"),
    ("piao", "piao"),
    ("pieh", "pie"),
    ("pin", "pin"),
    ("ping", "ping"),
    ("po", "po"),
    ("pou", "pou"),
    ("pu", "pu"),
    ("ran", "ran"),
    ("rang", "rang"),
    ("rao", "rao"),
    ("re", "re"),
    ("ren", "ren"),
    ("reng", "reng"),
    ("rih", "r"),
    ("rong", "rong"),
    ("rou", "rou"),
    ("ru", "ru"),
    ("ruan", "ruan"),
    ("ruei", "rui"),
    ("run", "run"),
    ("ruo", "ruo"),
    ("sa", "sa"),
    ("sai", "sai"),
    ("san", "san"),
    ("sang", "sang"),
    ("sao", "sao"),
    ("se", "se"),
    ("sen", "sen"),
    ("seng", "seng"),
    ("sha", "sha"),
    ("shai", "shai"),
    ("shan", "shan"),
    ("shang", "shang"),
    ("shao", "shao"),
    ("she", "she"),
    ("shei", "shei"),
    ("shen", "shen"),
    ("sheng", "sheng"),
    ("shih", "sh"),
    ("shou", "shou"),
    ("shu", "shu"),
    ("shua", "shua"),
    ("shuai", "shuai"),
    ("shuan", "shuan"),
    ("shuang", "shuang"),
    ("shuei", "shui"),
    ("shun", "shun"),
    ("shuo", "shuo"),
    ("si", "xi"),
    ("sia", "xia"),
    ("sian", "xian"),
    ("siang", "xiang"),
    ("siao", "xiao"),
    ("sieh", "xie"),
    ("sih", "s"),
    ("sin", "xin"),
    ("sing", "xing"),
    ("siou", "xiu"),
    ("song", "song"),
    ("sou", "sou"),
    ("su", "su"),
    ("suan", "suan"),
    ("suei", "sui"),
    ("sun", "sun"),
    ("suo", "suo"),
    ("syong", "xiong"),
    ("syu", "xu"),
    ("syuan", "xuan"),
    ("syueh", "xue"),
    ("syun", "xun"),
    ("ta", "ta"),
    ("tai", "tai"),
    ("tan", "tan"),
    ("tang", "tang"),
    ("tao", "tao"),
    ("te", "te"),
    ("teng", "teng"),
    ("ti", "ti"),
    ("tian", "tian"),
    ("tiao", "tiao"),
    ("tieh", "tie"),
    ("ting", "ting"),
    ("tong", "tong"),
    ("tou", "tou"),
    ("tsa", "ca"),
    ("tsai", "cai"),
    ("tsan", "can"),
    ("tsang", "cang"),
    ("tsao", "cao"),
    ("tse", "ce"),
    ("tsen", "cen"),
    ("tseng", "ceng"),
    ("tsih", "c"),
    ("tsong", "cong"),
    ("tsou", "cou"),
    ("tsu", "cu"),
    ("tsuan", "cuan"),
    ("tsuei", "cui"),
    ("tsun", "cun"),
    ("tsuo", "cuo"),
    ("tu", "tu"),
    ("tuan", "tuan"),
    ("tuei", "tui"),
    ("tun", "tun"),
    ("tuo", "tuo"),
    ("wa", "wa"),
    ("wai", "wai"),
    ("wan", "wan"),
    ("wang", "wang"),
    ("wei", "wei"),
    ("wo", "wo"),
    ("wong", "weng"),
    ("wu", "wu"),
    ("wun", "wen"),
    ("ya", "ya"),
    ("yai", "yai"),
    ("yan", "yan"),
    ("yang", "yang"),
    ("yao", "yao"),
    ("yeh", "ye"),
    ("yi", "yi"),
    ("yin", "yin"),
    ("ying", "ying"),
    ("yo", "yo"),
    ("yong", "yong"),
    ("you", "you"),
    ("yu", "yu"),
    ("yuan", "yuan"),
    ("yueh", "yue"),
    ("yun", "yun"),
    ("za", "za"),
    ("zai", "zai"),
    ("zan", "zan"),
    ("zang", "zang"),
    ("zao", "zao"),
    ("ze", "ze"),
    ("zei", "zei"),
    ("zen", "zen"),
    ("zeng", "zeng"),
    ("zih", "z"),
    ("zong", "zong"),
    ("zou", "zou"),
    ("zu", "zu"),
    ("zuan", "zuan"),
    ("zuei", "zui"),
    ("zun", "zun"),
    ("zuo", "zuo"),
];

/// Number of entries in [`SECONDARY_ZHUYIN_INDEX`].
pub const SECONDARY_ZHUYIN_COUNT: usize = 406;

/// `(input spelling, canonical pinyin)` pairs from `secondary_zhuyin_index[]`
/// (`pinyin_parser_table.h:1083`, equal to the `m_secondary_zhuyin_str`
/// column of `content_table`), sorted by spelling — the
/// binary-search invariant of the pinned table.
pub static SECONDARY_ZHUYIN_INDEX: [(&str, &str); SECONDARY_ZHUYIN_COUNT] = [
    ("a", "a"),
    ("ai", "ai"),
    ("an", "an"),
    ("ang", "ang"),
    ("au", "ao"),
    ("ba", "ba"),
    ("bai", "bai"),
    ("ban", "ban"),
    ("bang", "bang"),
    ("bau", "bao"),
    ("bei", "bei"),
    ("ben", "ben"),
    ("beng", "beng"),
    ("bi", "bi"),
    ("bian", "bian"),
    ("biau", "biao"),
    ("bie", "bie"),
    ("bin", "bin"),
    ("bing", "bing"),
    ("bo", "bo"),
    ("bu", "bu"),
    ("cha", "cha"),
    ("chai", "chai"),
    ("chan", "chan"),
    ("chang", "chang"),
    ("chau", "chao"),
    ("che", "che"),
    ("chen", "chen"),
    ("cheng", "cheng"),
    ("chi", "qi"),
    ("chia", "qia"),
    ("chian", "qian"),
    ("chiang", "qiang"),
    ("chiau", "qiao"),
    ("chie", "qie"),
    ("chin", "qin"),
    ("ching", "qing"),
    ("chiou", "qiu"),
    ("chiu", "qu"),
    ("chiuan", "quan"),
    ("chiue", "que"),
    ("chiun", "qun"),
    ("chiung", "qiong"),
    ("chou", "chou"),
    ("chr", "ch"),
    ("chu", "chu"),
    ("chuai", "chuai"),
    ("chuan", "chuan"),
    ("chuang", "chuang"),
    ("chuei", "chui"),
    ("chuen", "chun"),
    ("chung", "chong"),
    ("chuo", "chuo"),
    ("da", "da"),
    ("dai", "dai"),
    ("dan", "dan"),
    ("dang", "dang"),
    ("dau", "dao"),
    ("de", "de"),
    ("dei", "dei"),
    ("deng", "deng"),
    ("di", "di"),
    ("dian", "dian"),
    ("diau", "diao"),
    ("die", "die"),
    ("ding", "ding"),
    ("diou", "diu"),
    ("dou", "dou"),
    ("du", "du"),
    ("duan", "duan"),
    ("duei", "dui"),
    ("duen", "dun"),
    ("dung", "dong"),
    ("duo", "duo"),
    ("e", "e"),
    ("ei", "ei"),
    ("en", "en"),
    ("eng", "eng"),
    ("er", "er"),
    ("fa", "fa"),
    ("fan", "fan"),
    ("fang", "fang"),
    ("fei", "fei"),
    ("fen", "fen"),
    ("fo", "fo"),
    ("fou", "fou"),
    ("fu", "fu"),
    ("ga", "ga"),
    ("gai", "gai"),
    ("gan", "gan"),
    ("gang", "gang"),
    ("gau", "gao"),
    ("ge", "ge"),
    ("gei", "gei"),
    ("gen", "gen"),
    ("geng", "geng"),
    ("gou", "gou"),
    ("gu", "gu"),
    ("gua", "gua"),
    ("guai", "guai"),
    ("guan", "guan"),
    ("guang", "guang"),
    ("guei", "gui"),
    ("guen", "gun"),
    ("gung", "gong"),
    ("guo", "guo"),
    ("ha", "ha"),
    ("hai", "hai"),
    ("han", "han"),
    ("hang", "hang"),
    ("hau", "hao"),
    ("he", "he"),
    ("hei", "hei"),
    ("hen", "hen"),
    ("heng", "heng"),
    ("hou", "hou"),
    ("hu", "hu"),
    ("hua", "hua"),
    ("huai", "huai"),
    ("huan", "huan"),
    ("huang", "huang"),
    ("huei", "hui"),
    ("huen", "hun"),
    ("hung", "hong"),
    ("huo", "huo"),
    ("ja", "zha"),
    ("jai", "zhai"),
    ("jan", "zhan"),
    ("jang", "zhang"),
    ("jau", "zhao"),
    ("je", "zhe"),
    ("jei", "zhei"),
    ("jen", "zhen"),
    ("jeng", "zheng"),
    ("ji", "ji"),
    ("jia", "jia"),
    ("jian", "jian"),
    ("jiang", "jiang"),
    ("jiau", "jiao"),
    ("jie", "jie"),
    ("jin", "jin"),
    ("jing", "jing"),
    ("jiou", "jiu"),
    ("jiu", "ju"),
    ("jiuan", "juan"),
    ("jiue", "jue"),
    ("jiun", "jun"),
    ("jiung", "jiong"),
    ("jou", "zhou"),
    ("jr", "zh"),
    ("ju", "zhu"),
    ("jua", "zhua"),
    ("juai", "zhuai"),
    ("juan", "zhuan"),
    ("juang", "zhuang"),
    ("juei", "zhui"),
    ("juen", "zhun"),
    ("jung", "zhong"),
    ("juo", "zhuo"),
    ("ka", "ka"),
    ("kai", "kai"),
    ("kan", "kan"),
    ("kang", "kang"),
    ("kau", "kao"),
    ("ke", "ke"),
    ("ken", "ken"),
    ("keng", "keng"),
    ("kou", "kou"),
    ("ku", "ku"),
    ("kua", "kua"),
    ("kuai", "kuai"),
    ("kuan", "kuan"),
    ("kuang", "kuang"),
    ("kuei", "kui"),
    ("kuen", "kun"),
    ("kung", "kong"),
    ("kuo", "kuo"),
    ("la", "la"),
    ("lai", "lai"),
    ("lan", "lan"),
    ("lang", "lang"),
    ("lau", "lao"),
    ("le", "le"),
    ("lei", "lei"),
    ("leng", "leng"),
    ("li", "li"),
    ("lia", "lia"),
    ("lian", "lian"),
    ("liang", "liang"),
    ("liau", "liao"),
    ("lie", "lie"),
    ("lin", "lin"),
    ("ling", "ling"),
    ("liou", "liu"),
    ("liu", "lv"),
    ("liue", "lve"),
    ("lo", "lo"),
    ("lou", "lou"),
    ("lu", "lu"),
    ("luan", "luan"),
    ("luen", "lun"),
    ("lung", "long"),
    ("luo", "luo"),
    ("ma", "ma"),
    ("mai", "mai"),
    ("man", "man"),
    ("mang", "mang"),
    ("mau", "mao"),
    ("me", "me"),
    ("mei", "mei"),
    ("men", "men"),
    ("meng", "meng"),
    ("mi", "mi"),
    ("mian", "mian"),
    ("miau", "miao"),
    ("mie", "mie"),
    ("min", "min"),
    ("ming", "ming"),
    ("miou", "miu"),
    ("mo", "mo"),
    ("mou", "mou"),
    ("mu", "mu"),
    ("na", "na"),
    ("nai", "nai"),
    ("nan", "nan"),
    ("nang", "nang"),
    ("nau", "nao"),
    ("ne", "ne"),
    ("nei", "nei"),
    ("nen", "nen"),
    ("neng", "neng"),
    ("ni", "ni"),
    ("nian", "nian"),
    ("niang", "niang"),
    ("niau", "niao"),
    ("nie", "nie"),
    ("nin", "nin"),
    ("ning", "ning"),
    ("niou", "niu"),
    ("niu", "nv"),
    ("niue", "nve"),
    ("nou", "nou"),
    ("nu", "nu"),
    ("nuan", "nuan"),
    ("nuen", "nun"),
    ("nung", "nong"),
    ("nuo", "nuo"),
    ("o", "o"),
    ("ou", "ou"),
    ("pa", "pa"),
    ("pai", "pai"),
    ("pan", "pan"),
    ("pang", "pang"),
    ("pau", "pao"),
    ("pei", "pei"),
    ("pen", "pen"),
    ("peng", "peng"),
    ("pi", "pi"),
    ("pian", "pian"),
    ("piau", "piao"),
    ("pie", "pie"),
    ("pin", "pin"),
    ("ping", "ping"),
    ("po", "po"),
    ("pou", "pou"),
    ("pu", "pu"),
    ("r", "r"),
    ("ran", "ran"),
    ("rang", "rang"),
    ("rau", "rao"),
    ("re", "re"),
    ("ren", "ren"),
    ("reng", "reng"),
    ("rou", "rou"),
    ("ru", "ru"),
    ("ruan", "ruan"),
    ("ruei", "rui"),
    ("ruen", "run"),
    ("rung", "rong"),
    ("ruo", "ruo"),
    ("sa", "sa"),
    ("sai", "sai"),
    ("san", "san"),
    ("sang", "sang"),
    ("sau", "sao"),
    ("se", "se"),
    ("sen", "sen"),
    ("seng", "seng"),
    ("sha", "sha"),
    ("shai", "shai"),
    ("shan", "shan"),
    ("shang", "shang"),
    ("shau", "shao"),
    ("she", "she"),
    ("shei", "shei"),
    ("shen", "shen"),
    ("sheng", "sheng"),
    ("shi", "xi"),
    ("shia", "xia"),
    ("shian", "xian"),
    ("shiang", "xiang"),
    ("shiau", "xiao"),
    ("shie", "xie"),
    ("shin", "xin"),
    ("shing", "xing"),
    ("shiou", "xiu"),
    ("shiu", "xu"),
    ("shiuan", "xuan"),
    ("shiue", "xue"),
    ("shiun", "xun"),
    ("shiung", "xiong"),
    ("shou", "shou"),
    ("shr", "sh"),
    ("shu", "shu"),
    ("shua", "shua"),
    ("shuai", "shuai"),
    ("shuan", "shuan"),
    ("shuang", "shuang"),
    ("shuei", "shui"),
    ("shuen", "shun"),
    ("shuo", "shuo"),
    ("sou", "sou"),
    ("su", "su"),
    ("suan", "suan"),
    ("suei", "sui"),
    ("suen", "sun"),
    ("sung", "song"),
    ("suo", "suo"),
    ("sz", "s"),
    ("ta", "ta"),
    ("tai", "tai"),
    ("tan", "tan"),
    ("tang", "tang"),
    ("tau", "tao"),
    ("te", "te"),
    ("teng", "teng"),
    ("ti", "ti"),
    ("tian", "tian"),
    ("tiau", "tiao"),
    ("tie", "tie"),
    ("ting", "ting"),
    ("tou", "tou"),
    ("tsa", "ca"),
    ("tsai", "cai"),
    ("tsan", "can"),
    ("tsang", "cang"),
    ("tsau", "cao"),
    ("tse", "ce"),
    ("tsen", "cen"),
    ("tseng", "ceng"),
    ("tsou", "cou"),
    ("tsu", "cu"),
    ("tsuan", "cuan"),
    ("tsuei", "cui"),
    ("tsun", "cun"),
    ("tsung", "cong"),
    ("tsuo", "cuo"),
    ("tsz", "c"),
    ("tu", "tu"),
    ("tuan", "tuan"),
    ("tuei", "tui"),
    ("tuen", "tun"),
    ("tung", "tong"),
    ("tuo", "tuo"),
    ("tz", "z"),
    ("tza", "za"),
    ("tzai", "zai"),
    ("tzan", "zan"),
    ("tzang", "zang"),
    ("tzau", "zao"),
    ("tze", "ze"),
    ("tzei", "zei"),
    ("tzen", "zen"),
    ("tzeng", "zeng"),
    ("tzou", "zou"),
    ("tzu", "zu"),
    ("tzuan", "zuan"),
    ("tzuei", "zui"),
    ("tzuen", "zun"),
    ("tzung", "zong"),
    ("tzuo", "zuo"),
    ("wa", "wa"),
    ("wai", "wai"),
    ("wan", "wan"),
    ("wang", "wang"),
    ("wei", "wei"),
    ("wen", "wen"),
    ("weng", "weng"),
    ("wo", "wo"),
    ("wu", "wu"),
    ("ya", "ya"),
    ("yai", "yai"),
    ("yan", "yan"),
    ("yang", "yang"),
    ("yau", "yao"),
    ("ye", "ye"),
    ("yi", "yi"),
    ("yin", "yin"),
    ("ying", "ying"),
    ("yo", "yo"),
    ("you", "you"),
    ("yu", "yu"),
    ("yuan", "yuan"),
    ("yue", "yue"),
    ("yun", "yun"),
    ("yung", "yong"),
];

/// Longest spelling in either index, in bytes.
///
/// The pinned parser's lookahead is `max_full_pinyin_length = 7`
/// including the tone digit (`pinyin_parser2.cpp:82`); the longest
/// index spelling is 6, so one tone digit fits inside the lookahead.
pub const MAX_INDEX_SPELLING_LEN: usize = 6;

/// Lookahead bound of the pinned full-pinyin DP, tone digit included
/// (`max_full_pinyin_length`, `pinyin_parser2.cpp:82`).
const MAX_LOOKAHEAD: usize = 7;

/// One parsed key: the canonical spelling the input segment maps to,
/// its raw byte span (tone digit included), and the parsed tone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FullPinyinIndexKey {
    canonical: &'static str,
    start: usize,
    end: usize,
    tone: u8,
}

impl FullPinyinIndexKey {
    /// The canonical full-pinyin spelling this segment resolves to.
    #[must_use]
    pub const fn canonical(&self) -> &'static str {
        self.canonical
    }

    /// Inclusive byte offset of the segment's first input byte.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// Exclusive byte offset one past the segment's last input byte
    /// (tone digit included).
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }

    /// Tone number (1..5), or 0 for zero tone.
    #[must_use]
    pub const fn tone(&self) -> u8 {
        self.tone
    }
}

/// Result of one full-pinyin index parse.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FullPinyinIndexParse {
    keys: Vec<FullPinyinIndexKey>,
    consumed: usize,
}

impl FullPinyinIndexParse {
    /// The parsed keys in input order.
    #[must_use]
    pub fn keys(&self) -> &[FullPinyinIndexKey] {
        &self.keys
    }

    /// Number of original input bytes consumed.
    #[must_use]
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    /// The keys as a `'`-joined canonical full-pinyin string.
    #[must_use]
    pub fn full_pinyin(&self) -> String {
        self.keys
            .iter()
            .map(|key| key.canonical)
            .collect::<Vec<_>>()
            .join("'")
    }
}

/// One dynamic-programming step of the pinned full-pinyin parse.
#[derive(Clone, Copy, Debug)]
struct Step {
    /// Whether any path reaches this step.
    reached: bool,
    /// Previous step of the best path (the DP's `m_last_step`).
    last: usize,
    /// Keys on the best path (apostrophes excluded, like upstream).
    num_keys: usize,
    /// Input bytes covered by the best path.
    parsed_len: usize,
    /// The last key the best path appended, `None` for apostrophe hops.
    key: Option<FullPinyinIndexKey>,
}

/// `FullPinyinParser2::parse_one_key` over an index
/// (`pinyin_parser2.cpp:155-205`): under `use_tone` a trailing digit
/// 1..5 is the tone and is consumed with the match; the whole substring
/// must resolve through the index (plain rows only — these indexes have
/// no option-gated rows).
fn parse_one_index_key(
    input: &[u8],
    use_tone: bool,
    index: &[(&'static str, &'static str)],
) -> Option<FullPinyinIndexKey> {
    let mut parsed_len = input.len();
    let mut tone = 0;
    if use_tone && parsed_len > 0 && matches!(input[parsed_len - 1], b'1'..=b'5') {
        tone = input[parsed_len - 1] - b'0';
        parsed_len -= 1;
    }
    if parsed_len == 0 {
        return None;
    }

    let spelling = core::str::from_utf8(input.get(..parsed_len)?).ok()?;
    let position = index.partition_point(|row| row.0 < spelling);
    let canonical = index.get(position).filter(|row| row.0 == spelling)?;

    let mut end = parsed_len;
    if use_tone && tone != 0 {
        end += 1;
    }
    Some(FullPinyinIndexKey {
        canonical: canonical.1,
        start: 0,
        end,
        tone,
    })
}

/// `FullPinyinParser2::parse` + `final_step` over an index
/// (`pinyin_parser2.cpp:222-381`): the pinned dynamic program.
/// Apostrophes hop between segments consuming one byte and no key; every
/// other position extends up to `max_full_pinyin_length = 7` bytes ahead
/// (stopping at the next apostrophe); steps relax by longest covered
/// input first, then fewest keys, then least distance (always zero
/// here), with the first equal value kept — which makes the earliest
/// start, i.e. the longest spelling, win ties. The result backtracks
/// the longest covered prefix from the beginning of the input.
#[must_use]
pub fn parse_full_pinyin_index(
    input: &[u8],
    use_tone: bool,
    index: &[(&'static str, &'static str)],
) -> FullPinyinIndexParse {
    let mut steps = vec![
        Step {
            reached: false,
            last: 0,
            num_keys: 0,
            parsed_len: 0,
            key: None,
        };
        input.len() + 1
    ];
    steps[0].reached = true;

    for position in 0..input.len() {
        if input[position] == b'\'' {
            // Propagate the current step past the separator.
            let previous = steps[position];
            if previous.reached {
                let next = &mut steps[position + 1];
                let value = Step {
                    reached: true,
                    last: position,
                    num_keys: previous.num_keys,
                    parsed_len: previous.parsed_len + 1,
                    key: None,
                };
                if !next.reached
                    || value.parsed_len > next.parsed_len
                    || (value.parsed_len == next.parsed_len && value.num_keys < next.num_keys)
                {
                    *next = value;
                }
            }
            continue;
        }

        let next_sep = position
            + input[position..]
                .iter()
                .position(|&byte| byte == b'\'')
                .unwrap_or(input.len() - position);
        let try_end = (position + MAX_LOOKAHEAD).min(next_sep);
        for end in (position + 1)..=try_end {
            let Some(key) = parse_one_index_key(&input[position..end], use_tone, index) else {
                continue;
            };
            let previous = steps[position];
            if !previous.reached {
                continue;
            }
            let value = Step {
                reached: true,
                last: position,
                num_keys: previous.num_keys + 1,
                parsed_len: previous.parsed_len + (end - position),
                key: Some(FullPinyinIndexKey {
                    canonical: key.canonical,
                    start: position,
                    end,
                    tone: key.tone,
                }),
            };
            let next = &mut steps[end];
            if !next.reached
                || value.parsed_len > next.parsed_len
                || (value.parsed_len == next.parsed_len && value.num_keys < next.num_keys)
            {
                *next = value;
            }
        }
    }

    // final_step: the longest covered prefix from the input's start.
    let mut best = 0;
    for position in (0..=input.len()).rev() {
        if steps[position].reached && steps[position].parsed_len == position {
            best = position;
            break;
        }
    }

    let mut keys = Vec::new();
    let mut cursor = best;
    while cursor != 0 {
        let step = steps[cursor];
        if let Some(key) = step.key {
            keys.push(key);
        }
        cursor = step.last;
    }
    keys.reverse();

    FullPinyinIndexParse {
        keys,
        consumed: best,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LUOMA_PINYIN_COUNT, LUOMA_PINYIN_INDEX, SECONDARY_ZHUYIN_COUNT, SECONDARY_ZHUYIN_INDEX,
        parse_full_pinyin_index,
    };

    #[test]
    fn indexes_match_the_pinned_shapes() {
        for (index, count) in [
            (&LUOMA_PINYIN_INDEX[..], LUOMA_PINYIN_COUNT),
            (&SECONDARY_ZHUYIN_INDEX[..], SECONDARY_ZHUYIN_COUNT),
        ] {
            assert_eq!(index.len(), count);
            assert_eq!(count, 406);
            for pair in index.windows(2) {
                assert!(pair[0].0 < pair[1].0, "index not strictly sorted");
            }
            // Every target is a canonical key: the 440-id inventory
            // covers both indexes with nothing to spare or add.
            for &(spelling, canonical) in index {
                assert!(
                    crate::SyllableKey::from_canonical_text(canonical).is_some(),
                    "{spelling} -> {canonical} leaves the inventory"
                );
            }
        }

        // The pinned remaps: shared spellings mean different syllables
        // per scheme.
        let luoma = |spelling: &str| {
            LUOMA_PINYIN_INDEX
                .iter()
                .find(|row| row.0 == spelling)
                .map(|row| row.1)
        };
        let secondary = |spelling: &str| {
            SECONDARY_ZHUYIN_INDEX
                .iter()
                .find(|row| row.0 == spelling)
                .map(|row| row.1)
        };
        assert_eq!(luoma("chi"), Some("qi"));
        assert_eq!(luoma("si"), Some("xi"));
        assert_eq!(secondary("chi"), Some("qi"));
        assert_eq!(secondary("ju"), Some("zhu"));
        assert_eq!(secondary("liu"), Some("lv"));
        // And scheme-specific spellings absent from pinyin_index.
        assert_eq!(luoma("chyueh"), Some("que"));
        assert_eq!(secondary("au"), Some("ao"));
    }

    #[test]
    fn luoma_parses_through_the_dp() {
        let parse =
            |input: &str| parse_full_pinyin_index(input.as_bytes(), false, &LUOMA_PINYIN_INDEX);

        // Longest coverage wins; the canonical spelling is what the
        // segment maps to, spans stay raw.
        let nihao = parse("nihao");
        assert_eq!(nihao.consumed(), 5);
        assert_eq!(nihao.full_pinyin(), "ni'hao");

        let chiang = parse("chiang");
        assert_eq!(chiang.consumed(), 6);
        assert_eq!(chiang.full_pinyin(), "qiang");
        assert_eq!(chiang.keys()[0].start(), 0);
        assert_eq!(chiang.keys()[0].end(), 6);

        // Apostrophes separate keys and are consumed.
        let split = parse("ni'hao");
        assert_eq!(split.consumed(), 6);
        assert_eq!(split.full_pinyin(), "ni'hao");

        // Junk stops the parse: parsed length covers only the prefix
        // that resolves.
        let junk = parse("chiangQ");
        assert_eq!(junk.consumed(), 6);

        // A spelling only pinyin_index knows parses nothing here; the
        // luoma spelling of the ㄓ series targets the initial-only row
        // ("jhih" -> zh), and the full zhi syllable has no spelling at
        // all under this scheme.
        assert_eq!(parse("zhi").consumed(), 0);
        assert_eq!(parse("jhih").full_pinyin(), "zh");
    }

    #[test]
    fn index_parse_handles_tone_digits() {
        let parse = |input: &str, use_tone| {
            parse_full_pinyin_index(input.as_bytes(), use_tone, &SECONDARY_ZHUYIN_INDEX)
        };

        // Under USE_TONE the trailing digit rides the key (secondary
        // spells zhu as "ju").
        let toned = parse("ju4", true);
        assert_eq!(toned.consumed(), 3);
        assert_eq!(toned.full_pinyin(), "zhu");
        assert_eq!(toned.keys()[0].tone(), 4);
        assert_eq!(toned.keys()[0].end(), 3);

        // Without USE_TONE the digit is junk: only the "ju" spelling
        // consumes.
        let untoned = parse("ju4", false);
        assert_eq!(untoned.consumed(), 2);
        assert_eq!(untoned.full_pinyin(), "zhu");

        // A tone digit alone parses nothing.
        assert_eq!(parse("3", true).consumed(), 0);
    }

    #[test]
    fn dp_prefers_longest_coverage_then_fewest_keys() {
        // "xian" style ambiguity through the secondary index: "shian"
        // covers 5 bytes as one key; "shi"->xi + "an" also covers 5 as
        // two keys — fewest keys wins, so the single long key parses.
        let parse =
            |input: &str| parse_full_pinyin_index(input.as_bytes(), false, &SECONDARY_ZHUYIN_INDEX);
        assert_eq!(parse("shian").full_pinyin(), "xian");

        // An initial-only target parses with no option bit: these
        // indexes have no gated rows (secondary spells ㄘ-only "tsz").
        assert_eq!(parse("tsz").full_pinyin(), "c");
    }
}
