//! Scheme parsers for non-full-pinyin input surfaces.
//!
//! Double pinyin and Zhuyin are source-embedded keyboard schemes. The
//! double-pinyin tables below are direct ports of the generated upstream
//! header `src/storage/double_pinyin_table.h` at libpinyin `2.11.91`
//! (`0c5e80e1200f84fab185d1c5bde458b770a0636c`), cited per table in
//! `docs/findings/double-pinyin-spec.md`. The Simple Zhuyin keyboards
//! (STANDARD, IBM, GINYIEH, ETEN) and their tone tables come from
//! `src/storage/zhuyin_table.h`; the Zhuyin index — the pinned 1493-row
//! `zhuyin_index` with its shuffle and incomplete flag gating — lives in
//! [`crate::zhuyin_map`].

use crate::SyllableKey;
use crate::options::{
    PINYIN_AMB_ALL, ZHUYIN_CORRECT_ALL, ZHUYIN_CORRECT_SHUFFLE, ZHUYIN_INCOMPLETE,
};
use crate::zhuyin_map::{VALID_ZHUYIN_TONES, ZHUYIN_PINYIN_MAP};

/// One parsed double-pinyin key: a full-pinyin [`SyllableKey`] and its byte
/// span in the original two-key (or one-key incomplete) input.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DoublePinyinKey {
    key: SyllableKey,
    start: usize,
    end: usize,
}

impl DoublePinyinKey {
    /// The full-pinyin key this double-pinyin spelling resolves to.
    #[must_use]
    pub const fn key(self) -> SyllableKey {
        self.key
    }

    /// Inclusive byte offset of the first input key.
    #[must_use]
    pub const fn start(self) -> usize {
        self.start
    }

    /// Exclusive byte offset one past the last input key.
    #[must_use]
    pub const fn end(self) -> usize {
        self.end
    }
}

/// Result of one greedy double-pinyin parse.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DoublePinyinParse {
    keys: Vec<DoublePinyinKey>,
    consumed: usize,
}

impl DoublePinyinParse {
    /// The parsed keys in input order.
    #[must_use]
    pub fn keys(&self) -> &[DoublePinyinKey] {
        &self.keys
    }

    /// Number of original input bytes consumed.
    #[must_use]
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    /// The parsed keys as a `'`-joined full-pinyin string.
    ///
    /// Used by the compatibility layer to drive the existing decoder
    /// without turning double-pinyin keys back into shorter full-pinyin
    /// segmentations.
    #[must_use]
    pub fn full_pinyin(&self) -> String {
        self.keys
            .iter()
            .map(|item| item.key.text())
            .collect::<Vec<_>>()
            .join("'")
    }
}

/// `DoublePinyinScheme` from `src/storage/pinyin_custom2.h:108-117`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum DoublePinyinScheme {
    /// Ziranma.
    Zrm = 1,
    /// Microsoft.
    #[default]
    Ms = 2,
    /// Ziguang.
    Ziguang = 3,
    /// ABC.
    Abc = 4,
    /// PYJJ.
    Pyjj = 5,
    /// Xiaohe.
    Xhe = 6,
    /// User's keyboard; not a compiled table.
    Customized = 30,
}

/// Shengmu/yunmu tables are indexed by `a..z` then `;` (`charid` 0..26).
const SCHEME_KEY_COUNT: usize = 27;

type ShengmuTable = [Option<&'static str>; SCHEME_KEY_COUNT];
type YunmuTable = [[Option<&'static str>; 2]; SCHEME_KEY_COUNT];
type FallbackTable = &'static [(&'static str, &'static str)];

/// Microsoft shengmu table, `double_pinyin_mspy_sheng`
/// (`src/storage/double_pinyin_table.h:9-37`).
const MS_SHENG: ShengmuTable = [
    None,
    Some("b"),
    Some("c"),
    Some("d"),
    None,
    Some("f"),
    Some("g"),
    Some("h"),
    Some("ch"),
    Some("j"),
    Some("k"),
    Some("l"),
    Some("m"),
    Some("n"),
    Some("'"),
    Some("p"),
    Some("q"),
    Some("r"),
    Some("s"),
    Some("t"),
    Some("sh"),
    Some("zh"),
    Some("w"),
    Some("x"),
    Some("y"),
    Some("z"),
    None,
];

/// Microsoft yunmu table, `double_pinyin_mspy_yun`
/// (`src/storage/double_pinyin_table.h:39-67`).
const MS_YUN: YunmuTable = [
    [Some("a"), None],
    [Some("ou"), None],
    [Some("iao"), None],
    [Some("uang"), Some("iang")],
    [Some("e"), None],
    [Some("en"), None],
    [Some("eng"), Some("ng")],
    [Some("ang"), None],
    [Some("i"), None],
    [Some("an"), None],
    [Some("ao"), None],
    [Some("ai"), None],
    [Some("ian"), None],
    [Some("in"), None],
    [Some("uo"), Some("o")],
    [Some("un"), None],
    [Some("iu"), None],
    [Some("uan"), Some("er")],
    [Some("ong"), Some("iong")],
    [Some("ue"), None],
    [Some("u"), None],
    [Some("ui"), Some("ue")],
    [Some("ia"), Some("ua")],
    [Some("ie"), None],
    [Some("uai"), Some("v")],
    [Some("ei"), None],
    [Some("ing"), None],
];

/// Ziranma shengmu table, `double_pinyin_zrm_sheng`
/// (`src/storage/double_pinyin_table.h:69-97`).
const ZRM_SHENG: ShengmuTable = [
    None,
    Some("b"),
    Some("c"),
    Some("d"),
    None,
    Some("f"),
    Some("g"),
    Some("h"),
    Some("ch"),
    Some("j"),
    Some("k"),
    Some("l"),
    Some("m"),
    Some("n"),
    Some("'"),
    Some("p"),
    Some("q"),
    Some("r"),
    Some("s"),
    Some("t"),
    Some("sh"),
    Some("zh"),
    Some("w"),
    Some("x"),
    Some("y"),
    Some("z"),
    None,
];

/// Ziranma yunmu table, `double_pinyin_zrm_yun`
/// (`src/storage/double_pinyin_table.h:114-142`).
const ZRM_YUN: YunmuTable = [
    [Some("a"), None],
    [Some("ou"), None],
    [Some("iao"), None],
    [Some("uang"), Some("iang")],
    [Some("e"), None],
    [Some("en"), None],
    [Some("eng"), Some("ng")],
    [Some("ang"), None],
    [Some("i"), None],
    [Some("an"), None],
    [Some("ao"), None],
    [Some("ai"), None],
    [Some("ian"), None],
    [Some("in"), None],
    [Some("uo"), Some("o")],
    [Some("un"), None],
    [Some("iu"), None],
    [Some("uan"), Some("er")],
    [Some("ong"), Some("iong")],
    [Some("ue"), None],
    [Some("u"), None],
    [Some("ui"), Some("v")],
    [Some("ia"), Some("ua")],
    [Some("ie"), None],
    [Some("uai"), Some("ing")],
    [Some("ei"), None],
    [None, None],
];

/// Ziranma fallback table, `double_pinyin_zrm_fallback`
/// (`src/storage/double_pinyin_table.h:99-112`).
const ZRM_FALLBACK: FallbackTable = &[
    ("aa", "a"),
    ("ai", "ai"),
    ("an", "an"),
    ("ah", "ang"),
    ("ao", "ao"),
    ("ee", "e"),
    ("ei", "ei"),
    ("en", "en"),
    ("er", "er"),
    ("oo", "o"),
    ("ou", "ou"),
];

/// ABC shengmu table, `double_pinyin_abc_sheng`
/// (`src/storage/double_pinyin_table.h:144-172`).
const ABC_SHENG: ShengmuTable = [
    Some("zh"),
    Some("b"),
    Some("c"),
    Some("d"),
    Some("ch"),
    Some("f"),
    Some("g"),
    Some("h"),
    None,
    Some("j"),
    Some("k"),
    Some("l"),
    Some("m"),
    Some("n"),
    Some("'"),
    Some("p"),
    Some("q"),
    Some("r"),
    Some("s"),
    Some("t"),
    None,
    Some("sh"),
    Some("w"),
    Some("x"),
    Some("y"),
    Some("z"),
    None,
];

/// ABC yunmu table, `double_pinyin_abc_yun`
/// (`src/storage/double_pinyin_table.h:174-202`).
const ABC_YUN: YunmuTable = [
    [Some("a"), None],
    [Some("ou"), None],
    [Some("in"), Some("uai")],
    [Some("ia"), Some("ua")],
    [Some("e"), None],
    [Some("en"), None],
    [Some("eng"), Some("ng")],
    [Some("ang"), None],
    [Some("i"), None],
    [Some("an"), None],
    [Some("ao"), None],
    [Some("ai"), None],
    [Some("ue"), Some("ui")],
    [Some("un"), None],
    [Some("uo"), Some("o")],
    [Some("uan"), None],
    [Some("ei"), None],
    [Some("er"), Some("iu")],
    [Some("ong"), Some("iong")],
    [Some("iang"), Some("uang")],
    [Some("u"), None],
    [Some("v"), Some("ue")],
    [Some("ian"), None],
    [Some("ie"), None],
    [Some("ing"), None],
    [Some("iao"), None],
    [None, None],
];

/// Ziguang shengmu table, `double_pinyin_zgpy_sheng`
/// (`src/storage/double_pinyin_table.h:204-232`).
const ZG_SHENG: ShengmuTable = [
    Some("ch"),
    Some("b"),
    Some("c"),
    Some("d"),
    None,
    Some("f"),
    Some("g"),
    Some("h"),
    Some("sh"),
    Some("j"),
    Some("k"),
    Some("l"),
    Some("m"),
    Some("n"),
    Some("'"),
    Some("p"),
    Some("q"),
    Some("r"),
    Some("s"),
    Some("t"),
    Some("zh"),
    None,
    Some("w"),
    Some("x"),
    Some("y"),
    Some("z"),
    None,
];

/// Ziguang yunmu table, `double_pinyin_zgpy_yun`
/// (`src/storage/double_pinyin_table.h:234-262`).
const ZG_YUN: YunmuTable = [
    [Some("a"), None],
    [Some("iao"), None],
    [None, None],
    [Some("ie"), None],
    [Some("e"), None],
    [Some("ian"), None],
    [Some("iang"), Some("uang")],
    [Some("ong"), Some("iong")],
    [Some("i"), None],
    [Some("er"), Some("iu")],
    [Some("ei"), None],
    [Some("uan"), None],
    [Some("un"), None],
    [Some("ue"), Some("ui")],
    [Some("uo"), Some("o")],
    [Some("ai"), None],
    [Some("ao"), None],
    [Some("an"), None],
    [Some("ang"), None],
    [Some("eng"), Some("ng")],
    [Some("u"), None],
    [Some("v"), None],
    [Some("en"), None],
    [Some("ia"), Some("ua")],
    [Some("in"), Some("uai")],
    [Some("ou"), None],
    [Some("ing"), None],
];

/// PYJJ shengmu table, `double_pinyin_pyjj_sheng`
/// (`src/storage/double_pinyin_table.h:264-292`).
const PYJJ_SHENG: ShengmuTable = [
    Some("'"),
    Some("b"),
    Some("c"),
    Some("d"),
    None,
    Some("f"),
    Some("g"),
    Some("h"),
    Some("sh"),
    Some("j"),
    Some("k"),
    Some("l"),
    Some("m"),
    Some("n"),
    Some("'"),
    Some("p"),
    Some("q"),
    Some("r"),
    Some("s"),
    Some("t"),
    Some("ch"),
    Some("zh"),
    Some("w"),
    Some("x"),
    Some("y"),
    Some("z"),
    None,
];

/// PYJJ yunmu table, `double_pinyin_pyjj_yun`
/// (`src/storage/double_pinyin_table.h:294-322`).
const PYJJ_YUN: YunmuTable = [
    [Some("a"), None],
    [Some("ia"), Some("ua")],
    [Some("uan"), None],
    [Some("ao"), None],
    [Some("e"), None],
    [Some("an"), None],
    [Some("ang"), None],
    [Some("iang"), Some("uang")],
    [Some("i"), None],
    [Some("ian"), None],
    [Some("iao"), None],
    [Some("in"), None],
    [Some("ie"), None],
    [Some("iu"), None],
    [Some("uo"), Some("o")],
    [Some("ou"), None],
    [Some("er"), Some("ing")],
    [Some("en"), None],
    [Some("ai"), None],
    [Some("eng"), Some("ng")],
    [Some("u"), None],
    [Some("v"), Some("ui")],
    [Some("ei"), None],
    [Some("uai"), Some("ue")],
    [Some("ong"), Some("iong")],
    [Some("un"), None],
    [None, None],
];

/// PYJJ fallback table, `double_pinyin_pyjj_fallback`
/// (`src/storage/double_pinyin_table.h:324-337`).
const PYJJ_FALLBACK: FallbackTable = &[
    ("aa", "a"),
    ("as", "ai"),
    ("af", "an"),
    ("ag", "ang"),
    ("ad", "ao"),
    ("ee", "e"),
    ("ew", "ei"),
    ("er", "en"),
    ("eq", "er"),
    ("oo", "o"),
    ("op", "ou"),
];

/// Xiaohe shengmu table, `double_pinyin_xhe_sheng`
/// (`src/storage/double_pinyin_table.h:339-367`).
const XHE_SHENG: ShengmuTable = [
    None,
    Some("b"),
    Some("c"),
    Some("d"),
    None,
    Some("f"),
    Some("g"),
    Some("h"),
    Some("ch"),
    Some("j"),
    Some("k"),
    Some("l"),
    Some("m"),
    Some("n"),
    Some("'"),
    Some("p"),
    Some("q"),
    Some("r"),
    Some("s"),
    Some("t"),
    Some("sh"),
    Some("zh"),
    Some("w"),
    Some("x"),
    Some("y"),
    Some("z"),
    None,
];

/// Xiaohe yunmu table, `double_pinyin_xhe_yun`
/// (`src/storage/double_pinyin_table.h:369-397`).
const XHE_YUN: YunmuTable = [
    [Some("a"), None],
    [Some("in"), None],
    [Some("ao"), None],
    [Some("ai"), None],
    [Some("e"), None],
    [Some("en"), None],
    [Some("eng"), Some("ng")],
    [Some("ang"), None],
    [Some("i"), None],
    [Some("an"), None],
    [Some("uai"), Some("ing")],
    [Some("iang"), Some("uang")],
    [Some("ian"), None],
    [Some("iao"), None],
    [Some("uo"), Some("o")],
    [Some("ie"), None],
    [Some("iu"), None],
    [Some("uan"), Some("er")],
    [Some("ong"), Some("iong")],
    [Some("ue"), None],
    [Some("u"), None],
    [Some("v"), Some("ui")],
    [Some("ei"), None],
    [Some("ia"), Some("ua")],
    [Some("un"), None],
    [Some("ou"), None],
    [None, None],
];

/// Xiaohe fallback table, `double_pinyin_xhe_fallback`
/// (`src/storage/double_pinyin_table.h:399-412`).
const XHE_FALLBACK: FallbackTable = &[
    ("aa", "a"),
    ("ai", "ai"),
    ("an", "an"),
    ("ah", "ang"),
    ("ao", "ao"),
    ("ee", "e"),
    ("ei", "ei"),
    ("en", "en"),
    ("er", "er"),
    ("oo", "o"),
    ("ou", "ou"),
];

/// One double-pinyin scheme's three source tables.
#[derive(Clone, Copy)]
struct SchemeTables {
    shengmu: &'static ShengmuTable,
    yunmu: &'static YunmuTable,
    fallback: Option<FallbackTable>,
}

impl DoublePinyinScheme {
    /// The compiled source tables for this scheme, or `None` for
    /// `Customized`.
    fn tables(self) -> Option<SchemeTables> {
        match self {
            Self::Zrm => Some(SchemeTables {
                shengmu: &ZRM_SHENG,
                yunmu: &ZRM_YUN,
                fallback: Some(ZRM_FALLBACK),
            }),
            Self::Ms => Some(SchemeTables {
                shengmu: &MS_SHENG,
                yunmu: &MS_YUN,
                fallback: None,
            }),
            Self::Ziguang => Some(SchemeTables {
                shengmu: &ZG_SHENG,
                yunmu: &ZG_YUN,
                fallback: None,
            }),
            Self::Abc => Some(SchemeTables {
                shengmu: &ABC_SHENG,
                yunmu: &ABC_YUN,
                fallback: None,
            }),
            Self::Pyjj => Some(SchemeTables {
                shengmu: &PYJJ_SHENG,
                yunmu: &PYJJ_YUN,
                fallback: Some(PYJJ_FALLBACK),
            }),
            Self::Xhe => Some(SchemeTables {
                shengmu: &XHE_SHENG,
                yunmu: &XHE_YUN,
                fallback: Some(XHE_FALLBACK),
            }),
            Self::Customized => None,
        }
    }
}

/// Whether `byte` is a double-pinyin input key.
const fn is_key(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte == b';'
}

fn key_id(byte: u8) -> usize {
    if byte == b';' {
        SCHEME_KEY_COUNT - 1
    } else {
        usize::from(byte - b'a')
    }
}

/// Looks a spelling up in the frozen full-pinyin inventory, applying the two
/// corrections `DoublePinyinParser2::parse_one_key` forces upstream
/// (`PINYIN_CORRECT_UE_VE` and `PINYIN_CORRECT_V_U`,
/// `src/storage/pinyin_parser2.cpp:434-436`).
fn lookup_pinyin(spelling: &str) -> Option<SyllableKey> {
    if let Some(key) = SyllableKey::from_text(spelling) {
        return Some(key);
    }

    if matches!(spelling, "lue" | "nue") {
        let corrected = format!("{}ve", &spelling[..1]);
        return SyllableKey::from_text(&corrected);
    }

    let first = spelling.as_bytes().first().copied()?;
    if matches!(first, b'j' | b'q' | b'x' | b'y') && spelling.contains('v') {
        let corrected = spelling.replace('v', "u");
        return SyllableKey::from_text(&corrected);
    }

    None
}

/// Stateless parser for one double-pinyin scheme.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DoublePinyinParser {
    scheme: DoublePinyinScheme,
}

impl DoublePinyinParser {
    /// A parser for the scheme's default (`DOUBLE_PINYIN_MS`).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            scheme: DoublePinyinScheme::Ms,
        }
    }

    /// A parser for a specific scheme.
    #[must_use]
    pub const fn with_scheme(scheme: DoublePinyinScheme) -> Self {
        Self { scheme }
    }

    /// Selects a scheme. Returns `false` and keeps the current scheme for
    /// `Customized`, mirroring upstream's lack of a compiled table there
    /// (`src/storage/pinyin_parser2.cpp:610-612`) without the abort.
    pub fn set_scheme(&mut self, scheme: DoublePinyinScheme) -> bool {
        if scheme == DoublePinyinScheme::Customized {
            return false;
        }
        self.scheme = scheme;
        true
    }

    /// The scheme in force.
    #[must_use]
    pub const fn scheme(self) -> DoublePinyinScheme {
        self.scheme
    }

    /// Greedily parses `input`, mirroring `DoublePinyinParser2::parse`
    /// (`src/storage/pinyin_parser2.cpp:531-574`).
    ///
    /// `allow_incomplete` is the `PINYIN_INCOMPLETE` option. Tone is
    /// intentionally not represented here yet: without `USE_TONE` upstream
    /// rejects a three-byte key, and the W13 double-pinyin differential
    /// runs in that tone-less profile.
    #[must_use]
    pub fn parse(&self, input: &[u8], allow_incomplete: bool) -> DoublePinyinParse {
        let Some(tables) = self.scheme.tables() else {
            return DoublePinyinParse::default();
        };

        let maximum_len = input
            .iter()
            .take_while(|&&byte| is_key(byte) || (b'1'..=b'5').contains(&byte))
            .count();

        let mut keys = Vec::new();
        let mut parsed_len = 0;
        while parsed_len < maximum_len {
            let remaining = &input[parsed_len..maximum_len];
            let try_len = remaining.len().min(3);
            let mut matched = None;
            for len in (1..=try_len).rev() {
                if let Some(key) = parse_one_key(
                    &input[parsed_len..parsed_len + len],
                    allow_incomplete,
                    tables,
                ) {
                    matched = Some((key, len));
                    break;
                }
            }

            let Some((key, len)) = matched else {
                break;
            };
            keys.push(DoublePinyinKey {
                key,
                start: parsed_len,
                end: parsed_len + len,
            });
            parsed_len += len;
        }

        DoublePinyinParse {
            keys,
            consumed: parsed_len,
        }
    }
}

impl Default for DoublePinyinParser {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_one_key(
    input: &[u8],
    allow_incomplete: bool,
    tables: SchemeTables,
) -> Option<SyllableKey> {
    match input.len() {
        1 => parse_incomplete(input[0], allow_incomplete, tables),
        2 => parse_two_keys(input, tables),
        // Upstream accepts length 3 only with USE_TONE. The tone-less W13
        // profile therefore rejects it and lets the caller retry length 2.
        3 => None,
        _ => None,
    }
}

fn parse_incomplete(byte: u8, allow_incomplete: bool, tables: SchemeTables) -> Option<SyllableKey> {
    if !allow_incomplete || !is_key(byte) {
        return None;
    }
    let sheng = tables.shengmu[key_id(byte)]?;
    if sheng == "'" {
        return None;
    }
    SyllableKey::from_text(sheng)
}

fn parse_two_keys(input: &[u8], tables: SchemeTables) -> Option<SyllableKey> {
    if !is_key(input[0]) || !is_key(input[1]) {
        return None;
    }

    let sheng = match tables.shengmu[key_id(input[0])] {
        None => return parse_fallback(input, tables),
        Some("'") => "",
        Some(value) => value,
    };
    let yun = &tables.yunmu[key_id(input[1])];

    for candidate in yun.iter().flatten() {
        let spelling = format!("{sheng}{candidate}");
        if let Some(key) = lookup_pinyin(&spelling) {
            return Some(key);
        }
    }

    parse_fallback(input, tables)
}

fn parse_fallback(input: &[u8], tables: SchemeTables) -> Option<SyllableKey> {
    let table = tables.fallback?;
    let text = core::str::from_utf8(input).ok()?;
    table
        .iter()
        .find_map(|(keys, yunmu)| (*keys == text).then_some(*yunmu))
        .and_then(lookup_pinyin)
}

/// `ZhuyinScheme` from `src/storage/pinyin_custom2.h:122-133`.
///
/// The first W13 bopomofo PR scopes STANDARD only; the remaining values are
/// declared so the ABI setter can report `false` for them instead of aborting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum ZhuyinScheme {
    /// Standard keyboard.
    #[default]
    Standard = 1,
    /// Hsu keyboard.
    Hsu = 2,
    /// IBM keyboard.
    Ibm = 3,
    /// Gin-Yieh keyboard.
    Ginyieh = 4,
    /// Eten keyboard.
    Eten = 5,
    /// Eten 26 keyboard.
    Eten26 = 6,
    /// Standard Dvorak keyboard.
    StandardDvorak = 7,
    /// Hsu Dvorak keyboard.
    HsuDvorak = 8,
    /// Dachen CP26 keyboard.
    DachenCp26 = 9,
}

/// One parsed Zhuyin key: a tone-less full-pinyin [`SyllableKey`], its
/// original keystroke span, the source Zhuyin spelling, and the parsed tone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZhuyinKey {
    key: SyllableKey,
    start: usize,
    end: usize,
    zhuyin: String,
    tone: u8,
}

impl ZhuyinKey {
    /// The full-pinyin key this Zhuyin spelling resolves to.
    #[must_use]
    pub const fn key(&self) -> SyllableKey {
        self.key
    }

    /// Inclusive byte offset of the first keystroke.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// Exclusive byte offset one past the last keystroke.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }

    /// The tone-less canonical Zhuyin spelling of the matched row —
    /// shuffled inputs render canonically, matching
    /// `_ChewingKey::get_zhuyin_string`.
    #[must_use]
    pub fn zhuyin(&self) -> &str {
        &self.zhuyin
    }

    /// Tone number (1..5), or 0 for zero tone.
    #[must_use]
    pub const fn tone(&self) -> u8 {
        self.tone
    }

    /// Zhuyin spelling plus the tone mark, matching
    /// `_ChewingKey::get_zhuyin_string` (`src/storage/chewing_key.cpp:74-89`):
    /// first and zero tones render the bare spelling; tones 2..5 append their
    /// tone mark.
    #[must_use]
    pub fn display(&self) -> String {
        let mut display = self.zhuyin.clone();
        if (2..=5).contains(&self.tone) {
            display.push_str(tone_symbol(self.tone));
        }
        display
    }
}

/// Result of one greedy Zhuyin parse.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ZhuyinParse {
    keys: Vec<ZhuyinKey>,
    consumed: usize,
}

impl ZhuyinParse {
    /// The parsed keys in input order.
    #[must_use]
    pub fn keys(&self) -> &[ZhuyinKey] {
        &self.keys
    }

    /// Number of original input bytes consumed.
    #[must_use]
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    /// The parsed keys as a `'`-joined tone-less full-pinyin string.
    #[must_use]
    pub fn full_pinyin(&self) -> String {
        self.keys
            .iter()
            .map(|item| item.key.text())
            .collect::<Vec<_>>()
            .join("'")
    }
}

/// One Simple keyboard's source tables: key-to-symbol and key-to-tone
/// rows ported verbatim from the pinned `zhuyin_table.h`. Linear scan by
/// key, exactly like upstream's `search_chewing_symbols` /
/// `search_chewing_tones` ("we only have < 50 items").
#[derive(Clone, Copy)]
struct SimpleTables {
    /// `(key, zhuyin symbol)` rows.
    symbols: &'static [(u8, &'static str)],
    /// `(key, tone number)` rows.
    tones: &'static [(u8, u8)],
}

impl SimpleTables {
    /// The symbol string one key maps to, if it is a symbol key.
    fn symbol(&self, byte: u8) -> Option<&'static str> {
        self.symbols
            .iter()
            .find(|(key, _)| *key == byte)
            .map(|(_, symbol)| *symbol)
    }

    /// The tone one key spells, if it is a tone key.
    fn tone(&self, byte: u8) -> Option<u8> {
        self.tones
            .iter()
            .find(|(key, _)| *key == byte)
            .map(|(_, tone)| *tone)
    }
}

/// Standard keyboard symbol table, `chewing_standard_symbols`
/// (`src/storage/zhuyin_table.h:9`).
const STANDARD_SYMBOLS: &[(u8, &str)] = &[
    (b',', "ㄝ"),
    (b'-', "ㄦ"),
    (b'.', "ㄡ"),
    (b'/', "ㄥ"),
    (b'0', "ㄢ"),
    (b'1', "ㄅ"),
    (b'2', "ㄉ"),
    (b'5', "ㄓ"),
    (b'8', "ㄚ"),
    (b'9', "ㄞ"),
    (b';', "ㄤ"),
    (b'a', "ㄇ"),
    (b'b', "ㄖ"),
    (b'c', "ㄏ"),
    (b'd', "ㄎ"),
    (b'e', "ㄍ"),
    (b'f', "ㄑ"),
    (b'g', "ㄕ"),
    (b'h', "ㄘ"),
    (b'i', "ㄛ"),
    (b'j', "ㄨ"),
    (b'k', "ㄜ"),
    (b'l', "ㄠ"),
    (b'm', "ㄩ"),
    (b'n', "ㄙ"),
    (b'o', "ㄟ"),
    (b'p', "ㄣ"),
    (b'q', "ㄆ"),
    (b'r', "ㄐ"),
    (b's', "ㄋ"),
    (b't', "ㄔ"),
    (b'u', "ㄧ"),
    (b'v', "ㄒ"),
    (b'w', "ㄊ"),
    (b'x', "ㄌ"),
    (b'y', "ㄗ"),
    (b'z', "ㄈ"),
];

/// Standard keyboard tone table, `chewing_standard_tones`
/// (`src/storage/zhuyin_table.h:48`).
const STANDARD_TONES: &[(u8, u8)] = &[(b' ', 1), (b'3', 3), (b'4', 4), (b'6', 2), (b'7', 5)];

/// IBM keyboard symbol table, `chewing_ibm_symbols`
/// (`src/storage/zhuyin_table.h:159`).
const IBM_SYMBOLS: &[(u8, &str)] = &[
    (b'-', "ㄏ"),
    (b'0', "ㄎ"),
    (b'1', "ㄅ"),
    (b'2', "ㄆ"),
    (b'3', "ㄇ"),
    (b'4', "ㄈ"),
    (b'5', "ㄉ"),
    (b'6', "ㄊ"),
    (b'7', "ㄋ"),
    (b'8', "ㄌ"),
    (b'9', "ㄍ"),
    (b';', "ㄠ"),
    (b'a', "ㄧ"),
    (b'b', "ㄥ"),
    (b'c', "ㄣ"),
    (b'd', "ㄩ"),
    (b'e', "ㄒ"),
    (b'f', "ㄚ"),
    (b'g', "ㄛ"),
    (b'h', "ㄜ"),
    (b'i', "ㄗ"),
    (b'j', "ㄝ"),
    (b'k', "ㄞ"),
    (b'l', "ㄟ"),
    (b'n', "ㄦ"),
    (b'o', "ㄘ"),
    (b'p', "ㄙ"),
    (b'q', "ㄐ"),
    (b'r', "ㄓ"),
    (b's', "ㄨ"),
    (b't', "ㄔ"),
    (b'u', "ㄖ"),
    (b'v', "ㄤ"),
    (b'w', "ㄑ"),
    (b'x', "ㄢ"),
    (b'y', "ㄕ"),
    (b'z', "ㄡ"),
];

/// IBM keyboard tone table, `chewing_ibm_tones`
/// (`src/storage/zhuyin_table.h:198`).
const IBM_TONES: &[(u8, u8)] = &[(b' ', 1), (b',', 3), (b'.', 4), (b'/', 5), (b'm', 2)];

/// Gin-Yieh keyboard symbol table, `chewing_ginyieh_symbols`
/// (`src/storage/zhuyin_table.h:59`).
const GINYIEH_SYMBOLS: &[(u8, &str)] = &[
    (b'\'', "ㄥ"),
    (b',', "ㄚ"),
    (b'-', "ㄣ"),
    (b'.', "ㄞ"),
    (b'/', "ㄢ"),
    (b'0', "ㄟ"),
    (b'2', "ㄅ"),
    (b'3', "ㄉ"),
    (b'6', "ㄓ"),
    (b'8', "ㄧ"),
    (b'9', "ㄛ"),
    (b';', "ㄡ"),
    (b'=', "ㄦ"),
    (b'[', "ㄤ"),
    (b'b', "ㄒ"),
    (b'c', "ㄌ"),
    (b'd', "ㄋ"),
    (b'e', "ㄊ"),
    (b'f', "ㄎ"),
    (b'g', "ㄑ"),
    (b'h', "ㄕ"),
    (b'i', "ㄨ"),
    (b'j', "ㄘ"),
    (b'k', "ㄩ"),
    (b'l', "ㄝ"),
    (b'm', "ㄙ"),
    (b'n', "ㄖ"),
    (b'o', "ㄜ"),
    (b'p', "ㄠ"),
    (b'r', "ㄍ"),
    (b's', "ㄇ"),
    (b't', "ㄐ"),
    (b'u', "ㄗ"),
    (b'v', "ㄏ"),
    (b'w', "ㄆ"),
    (b'x', "ㄈ"),
    (b'y', "ㄔ"),
];

/// Gin-Yieh keyboard tone table, `chewing_ginyieh_tones`
/// (`src/storage/zhuyin_table.h:98`).
const GINYIEH_TONES: &[(u8, u8)] = &[(b' ', 1), (b'1', 5), (b'a', 3), (b'q', 2), (b'z', 4)];

/// Eten keyboard symbol table, `chewing_eten_symbols`
/// (`src/storage/zhuyin_table.h:109`).
const ETEN_SYMBOLS: &[(u8, &str)] = &[
    (b'\'', "ㄘ"),
    (b',', "ㄓ"),
    (b'-', "ㄥ"),
    (b'.', "ㄔ"),
    (b'/', "ㄕ"),
    (b'0', "ㄤ"),
    (b'7', "ㄑ"),
    (b'8', "ㄢ"),
    (b'9', "ㄣ"),
    (b';', "ㄗ"),
    (b'=', "ㄦ"),
    (b'a', "ㄚ"),
    (b'b', "ㄅ"),
    (b'c', "ㄒ"),
    (b'd', "ㄉ"),
    (b'e', "ㄧ"),
    (b'f', "ㄈ"),
    (b'g', "ㄐ"),
    (b'h', "ㄏ"),
    (b'i', "ㄞ"),
    (b'j', "ㄖ"),
    (b'k', "ㄎ"),
    (b'l', "ㄌ"),
    (b'm', "ㄇ"),
    (b'n', "ㄋ"),
    (b'o', "ㄛ"),
    (b'p', "ㄆ"),
    (b'q', "ㄟ"),
    (b'r', "ㄜ"),
    (b's', "ㄙ"),
    (b't', "ㄊ"),
    (b'u', "ㄩ"),
    (b'v', "ㄍ"),
    (b'w', "ㄝ"),
    (b'x', "ㄨ"),
    (b'y', "ㄡ"),
    (b'z', "ㄠ"),
];

/// Eten keyboard tone table, `chewing_eten_tones`
/// (`src/storage/zhuyin_table.h:148`).
const ETEN_TONES: &[(u8, u8)] = &[(b' ', 1), (b'1', 5), (b'2', 2), (b'3', 3), (b'4', 4)];

fn tone_symbol(tone: u8) -> &'static str {
    match tone {
        1 => " ",
        2 => "ˊ",
        3 => "ˇ",
        4 => "ˋ",
        5 => "˙",
        _ => "",
    }
}

/// `search_chewing_index` + `check_chewing_options`
/// (`src/storage/zhuyin_parser2.cpp:43-100`): binary search the sorted
/// zhuyin index (at most one hit), gate the row's flags against the
/// parser's option word, and return the key with the row's canonical
/// spelling — what upstream renders through `ChewingKey::
/// get_zhuyin_string`, so a shuffled input displays canonically.
///
/// `options` carries only parser-owned bits — `ZHUYIN_CORRECT_SHUFFLE`
/// (forced by `ZhuyinSimpleParser2::set_scheme`, `:272`) and optionally
/// `ZHUYIN_INCOMPLETE` passed through from the caller's option word
/// (`pinyin.cpp` strips `ZHUYIN_CORRECT_ALL` from caller options at every
/// chewing entry point, so caller corrections can never reach this gate).
fn search_zhuyin_index(zhuyin: &str, options: u32) -> Option<(SyllableKey, &'static str)> {
    let index = ZHUYIN_PINYIN_MAP.partition_point(|row| row.0 < zhuyin);
    let &(spelling, canonical, pinyin, flags) = ZHUYIN_PINYIN_MAP.get(index)?;

    if spelling != zhuyin {
        return None;
    }
    // An incomplete row needs its option bit; a correction-flagged row
    // needs every correction bit present ((flags & options) != flags
    // rejects).
    if flags & ZHUYIN_INCOMPLETE != 0 && options & ZHUYIN_INCOMPLETE == 0 {
        return None;
    }
    let corrections = flags & (ZHUYIN_CORRECT_ALL | PINYIN_AMB_ALL);
    if corrections != 0 && corrections & options != corrections {
        return None;
    }
    SyllableKey::from_canonical_text(pinyin).map(|key| (key, canonical))
}

/// Stateless parser for one Zhuyin keyboard.
///
/// The Simple keyboards (STANDARD, IBM, GINYIEH, ETEN) are implemented
/// table-driven over the shared [`crate::zhuyin_map`] index; Discrete,
/// CP26, and the STANDARD_DVORAK abort slot parse nothing and their
/// setters report `false`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZhuyinParser {
    scheme: ZhuyinScheme,
}

impl ZhuyinScheme {
    /// The compiled Simple tables for this scheme, or `None` for the
    /// Discrete/CP26 keyboards and the dvorak abort slot.
    fn simple_tables(self) -> Option<SimpleTables> {
        match self {
            Self::Standard => Some(SimpleTables {
                symbols: STANDARD_SYMBOLS,
                tones: STANDARD_TONES,
            }),
            Self::Ibm => Some(SimpleTables {
                symbols: IBM_SYMBOLS,
                tones: IBM_TONES,
            }),
            Self::Ginyieh => Some(SimpleTables {
                symbols: GINYIEH_SYMBOLS,
                tones: GINYIEH_TONES,
            }),
            Self::Eten => Some(SimpleTables {
                symbols: ETEN_SYMBOLS,
                tones: ETEN_TONES,
            }),
            _ => None,
        }
    }
}

impl ZhuyinParser {
    /// A parser for `ZHUYIN_DEFAULT = ZHUYIN_STANDARD`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            scheme: ZhuyinScheme::Standard,
        }
    }

    /// A parser for a specific scheme.
    #[must_use]
    pub const fn with_scheme(scheme: ZhuyinScheme) -> Self {
        Self { scheme }
    }

    /// Selects a scheme. The Simple keyboards (STANDARD, IBM, GINYIEH,
    /// ETEN) switch tables the way upstream's `set_scheme` does
    /// (`src/storage/zhuyin_parser2.cpp:271-300`) minus the
    /// STANDARD_DVORAK fallthrough-abort; everything else reports
    /// `false` and keeps the current scheme.
    pub fn set_scheme(&mut self, scheme: ZhuyinScheme) -> bool {
        if scheme.simple_tables().is_none() {
            return false;
        }
        self.scheme = scheme;
        true
    }

    /// The scheme in force.
    #[must_use]
    pub const fn scheme(self) -> ZhuyinScheme {
        self.scheme
    }

    /// Whether `key` is part of the current keyboard.
    #[must_use]
    pub fn in_scheme(&self, key: u8, use_tone: bool) -> bool {
        let Some(tables) = self.scheme.simple_tables() else {
            return false;
        };
        tables.symbol(key).is_some() || (use_tone && tables.tone(key).is_some())
    }

    /// The Zhuyin symbol(s) mapped by one keystroke.
    ///
    /// Simple keyboards have at most one symbol per key; a tone key
    /// returns its tone mark, matching `pinyin_in_chewing_keyboard`.
    #[must_use]
    pub fn symbols_for(&self, key: u8, use_tone: bool) -> Vec<String> {
        let Some(tables) = self.scheme.simple_tables() else {
            return Vec::new();
        };
        if let Some(symbol) = tables.symbol(key) {
            return vec![symbol.to_owned()];
        }
        if use_tone && let Some(tone) = tables.tone(key) {
            return vec![tone_symbol(tone).to_owned()];
        }
        Vec::new()
    }

    /// Greedily parses `input`, mirroring `ZhuyinSimpleParser2::parse`
    /// (`src/storage/zhuyin_parser2.cpp:216-268`), including the post-match
    /// `is_valid_zhuyin` abort (`:256-257`, `_ChewingKey::is_valid_zhuyin`
    /// at `chewing_key.cpp:38-45`).
    ///
    /// `allow_incomplete` is the caller's `ZHUYIN_INCOMPLETE` option; the
    /// parser additionally owns `ZHUYIN_CORRECT_SHUFFLE`, which upstream's
    /// `set_scheme` forces for every Simple keyboard (`:272`) and `parse`
    /// or's into the option word (`:221`).
    #[must_use]
    pub fn parse(&self, input: &[u8], use_tone: bool, allow_incomplete: bool) -> ZhuyinParse {
        let Some(tables) = self.scheme.simple_tables() else {
            return ZhuyinParse::default();
        };

        let options = if allow_incomplete {
            ZHUYIN_CORRECT_SHUFFLE | ZHUYIN_INCOMPLETE
        } else {
            ZHUYIN_CORRECT_SHUFFLE
        };

        let maximum_len = input
            .iter()
            .take_while(|&&byte| {
                tables.symbol(byte).is_some() || (use_tone && tables.tone(byte).is_some())
            })
            .count();

        let mut keys = Vec::new();
        let mut parsed_len = 0;
        while parsed_len < maximum_len {
            let remaining = &input[parsed_len..maximum_len];
            let try_len = remaining.len().min(4);
            let mut matched = None;
            for len in (1..=try_len).rev() {
                if let Some((key, zhuyin, tone)) = parse_one_zhuyin_key(
                    &input[parsed_len..parsed_len + len],
                    use_tone,
                    options,
                    tables,
                ) {
                    matched = Some((key, zhuyin, tone, len));
                    break;
                }
            }

            let Some((key, zhuyin, tone, len)) = matched else {
                break;
            };
            // Same stop as upstream `:256-257`: do not retry a shorter key.
            if !is_valid_zhuyin(key, tone) {
                break;
            }
            keys.push(ZhuyinKey {
                key,
                start: parsed_len,
                end: parsed_len + len,
                zhuyin,
                tone,
            });
            parsed_len += len;
        }

        ZhuyinParse {
            keys,
            consumed: parsed_len,
        }
    }
}

impl Default for ZhuyinParser {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_one_zhuyin_key(
    input: &[u8],
    use_tone: bool,
    options: u32,
    tables: SimpleTables,
) -> Option<(SyllableKey, String, u8)> {
    let mut symbol_len = input.len();
    let mut tone = 0;
    if use_tone
        && symbol_len > 0
        && let Some(value) = tables.tone(input[symbol_len - 1])
    {
        tone = value;
        symbol_len -= 1;
    }
    if symbol_len == 0 {
        return None;
    }

    let mut zhuyin = String::new();
    for byte in &input[..symbol_len] {
        let symbol = tables.symbol(*byte)?;
        zhuyin.push_str(symbol);
    }
    let (key, canonical) = search_zhuyin_index(&zhuyin, options)?;
    // Display carries the canonical spelling (upstream renders the
    // matched key, not the raw keystrokes); the span stays input-based.
    Some((key, canonical.to_owned(), tone))
}

/// `_ChewingKey::is_valid_zhuyin` (`src/storage/chewing_key.cpp:38-45`).
fn is_valid_zhuyin(key: SyllableKey, tone: u8) -> bool {
    let index = key.index();
    let Some(&mask) = VALID_ZHUYIN_TONES.get(index) else {
        return false;
    };
    let Some(bit) = (1u8).checked_shl(u32::from(tone)) else {
        return false;
    };
    mask & bit != 0
}

#[cfg(test)]
mod tests {
    use super::{DoublePinyinParser, DoublePinyinScheme, ZhuyinParser, ZhuyinScheme};

    #[test]
    fn default_scheme_is_ms() {
        assert_eq!(DoublePinyinParser::new().scheme(), DoublePinyinScheme::Ms);
    }

    #[test]
    fn ms_parses_common_spellings() {
        let parser = DoublePinyinParser::new();
        let parsed = parser.parse(b"nihao", true);
        assert_eq!(parsed.consumed(), 4);
        assert_eq!(parsed.full_pinyin(), "ni'ha");
    }

    #[test]
    fn customized_is_rejected_without_replacing_the_scheme() {
        let mut parser = DoublePinyinParser::with_scheme(DoublePinyinScheme::Zrm);
        assert!(!parser.set_scheme(DoublePinyinScheme::Customized));
        assert_eq!(parser.scheme(), DoublePinyinScheme::Zrm);
    }

    #[test]
    fn no_inventory_key_is_invented() {
        for scheme in [
            DoublePinyinScheme::Zrm,
            DoublePinyinScheme::Ms,
            DoublePinyinScheme::Ziguang,
            DoublePinyinScheme::Abc,
            DoublePinyinScheme::Pyjj,
            DoublePinyinScheme::Xhe,
        ] {
            let parser = DoublePinyinParser::with_scheme(scheme);
            for a in b'a'..=b'z' {
                for b in b'a'..=b'z' {
                    let parsed = parser.parse(&[a, b], false);
                    for item in parsed.keys() {
                        assert!(
                            crate::FULL_PINYIN_SYLLABLES.contains(&item.key().text())
                                || crate::INCOMPLETE_PINYIN_KEYS.contains(&item.key().text()),
                            "{} produced {}",
                            scheme as i32,
                            item.key()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn illegal_first_tone_on_ni_stops_without_retrying_a_shorter_key() {
        let parser = ZhuyinParser::new();
        let rejected = parser.parse(b"su ", true, false);
        assert_eq!(rejected.consumed(), 0);
        assert!(rejected.keys().is_empty());

        let legal = parser.parse(b"su6", true, false);
        assert_eq!(legal.consumed(), 3);
        assert_eq!(legal.full_pinyin(), "ni");
        assert_eq!(legal.keys()[0].tone(), 2);

        let untoned = parser.parse(b"su", true, false);
        assert_eq!(untoned.consumed(), 2);
        assert_eq!(untoned.full_pinyin(), "ni");
        assert_eq!(untoned.keys()[0].tone(), 0);
    }

    #[test]
    fn tone_after_an_invalid_syllable_does_not_extend_a_valid_prefix() {
        let parser = ZhuyinParser::new();
        let parsed = parser.parse(b"sux6", true, false);
        assert_eq!(parsed.consumed(), 2);
        assert_eq!(parsed.full_pinyin(), "ni");
    }

    #[test]
    fn a_tone_key_alone_is_not_a_syllable() {
        let parser = ZhuyinParser::new();
        assert_eq!(parser.parse(b"6", true, false).consumed(), 0);
        assert_eq!(parser.parse(b" ", true, false).consumed(), 0);
    }

    /// The key one zhuyin symbol maps to on `scheme`'s keyboard, derived
    /// from the symbol table rather than hand-authored (the same rule the
    /// chewing-diff corpus applies).
    fn key_for_symbol(scheme: ZhuyinScheme, symbol: &str) -> u8 {
        let tables = scheme.simple_tables().expect("Simple keyboard");
        for (key, candidate) in tables.symbols {
            if *candidate == symbol {
                return *key;
            }
        }
        panic!("no {scheme:?} key spells {symbol:?}");
    }

    fn keystrokes(scheme: ZhuyinScheme, symbols: &[&str]) -> Vec<u8> {
        symbols
            .iter()
            .map(|symbol| key_for_symbol(scheme, symbol))
            .collect()
    }

    /// The key spelling tone `tone` (1..5) on `scheme`'s keyboard,
    /// derived from the tone table like [`key_for_symbol`].
    fn key_for_tone(scheme: ZhuyinScheme, tone: u8) -> u8 {
        let tables = scheme.simple_tables().expect("Simple keyboard");
        for (key, candidate) in tables.tones {
            if *candidate == tone {
                return *key;
            }
        }
        panic!("no {scheme:?} key spells tone {tone}");
    }

    fn keystrokes_with_tone(scheme: ZhuyinScheme, symbols: &[&str], tone: u8) -> Vec<u8> {
        let mut keys = keystrokes(scheme, symbols);
        keys.push(key_for_tone(scheme, tone));
        keys
    }

    #[test]
    fn index_matches_the_pinned_row_counts_and_shuffle_law() {
        use super::ZHUYIN_PINYIN_MAP;
        use crate::ZHUYIN_CORRECT_SHUFFLE;
        use crate::ZHUYIN_INCOMPLETE;

        assert_eq!(ZHUYIN_PINYIN_MAP.len(), 1493);

        // Sorted and unique: the binary-search invariant of the pinned
        // table (strcmp order == UTF-8 byte order).
        for pair in ZHUYIN_PINYIN_MAP.windows(2) {
            assert!(pair[0].0 < pair[1].0, "rows not strictly sorted");
        }

        let incomplete: Vec<_> = ZHUYIN_PINYIN_MAP
            .iter()
            .filter(|row| row.3 & ZHUYIN_INCOMPLETE != 0)
            .collect();
        let shuffle: Vec<_> = ZHUYIN_PINYIN_MAP
            .iter()
            .filter(|row| row.3 & ZHUYIN_CORRECT_SHUFFLE != 0)
            .collect();
        let plain: Vec<_> = ZHUYIN_PINYIN_MAP.iter().filter(|row| row.3 == 0).collect();
        assert_eq!(incomplete.len(), 14);
        assert_eq!(shuffle.len(), 1062);
        assert_eq!(plain.len(), 417);

        // The 417 plain rows are 23 single-symbol + 227 two-symbol +
        // 167 three-symbol spellings, and every multi-symbol plain row
        // contributes all non-canonical symbol permutations as shuffle
        // rows: 227 × 1 + 167 × 5 = 1062.
        let plain_by_len = |symbols: usize| {
            plain
                .iter()
                .filter(|row| row.0.chars().count() == symbols)
                .count()
        };
        assert_eq!(plain_by_len(1), 23);
        assert_eq!(plain_by_len(2), 227);
        assert_eq!(plain_by_len(3), 167);

        let mut expected: std::collections::HashSet<(String, &'static str)> = Default::default();
        for &(spelling, _canonical, pinyin, _) in plain.iter().copied() {
            let symbols: Vec<&str> = spelling
                .char_indices()
                .map(|(i, ch)| &spelling[i..i + ch.len_utf8()])
                .collect();
            if symbols.len() < 2 {
                continue;
            }
            permute(&symbols, &mut |permutation: &[&str]| {
                if permutation != symbols.as_slice() {
                    expected.insert((permutation.concat(), pinyin));
                }
            });
        }
        assert_eq!(expected.len(), 1062);
        for (spelling, _canonical, pinyin, flags) in shuffle.iter().map(|row| **row) {
            assert_eq!(flags, ZHUYIN_CORRECT_SHUFFLE);
            assert!(
                expected.contains(&(spelling.to_owned(), pinyin)),
                "unexpected shuffle row {spelling:?}"
            );
        }
    }

    /// Heap's algorithm over the symbol slice, in any order — the test
    /// only needs the full permutation set, not a canonical order.
    fn permute<'a>(symbols: &[&'a str], emit: &mut dyn FnMut(&[&'a str])) {
        fn go<'a>(symbols: &mut [&'a str], k: usize, emit: &mut dyn FnMut(&[&'a str])) {
            if k <= 1 {
                emit(symbols);
                return;
            }
            go(symbols, k - 1, emit);
            for i in 0..k - 1 {
                if k.is_multiple_of(2) {
                    symbols.swap(i, k - 1);
                } else {
                    symbols.swap(0, k - 1);
                }
                go(symbols, k - 1, emit);
            }
        }
        let mut owned = symbols.to_vec();
        let len = owned.len();
        go(&mut owned, len, emit);
    }

    #[test]
    fn index_flags_gate_through_the_option_word() {
        use super::search_zhuyin_index;
        use crate::ZHUYIN_CORRECT_SHUFFLE;
        use crate::ZHUYIN_INCOMPLETE;

        // Shuffle rows: only under the parser-owned correction bit. The
        // hit carries the canonical spelling for display.
        let bie = crate::SyllableKey::from_text("bie").expect("bie");
        assert_eq!(
            search_zhuyin_index("ㄅㄝㄧ", ZHUYIN_CORRECT_SHUFFLE),
            Some((bie, "ㄅㄧㄝ"))
        );
        assert_eq!(search_zhuyin_index("ㄅㄝㄧ", 0), None);

        // Incomplete rows: only under the caller's option bit. All 14 are
        // parse-dead after the validity mask, but the index gate itself is
        // what this pins (both outcomes at this pin consume 0).
        let b = crate::SyllableKey::from_text("b").expect("b");
        assert_eq!(
            search_zhuyin_index("ㄅ", ZHUYIN_INCOMPLETE),
            Some((b, "ㄅ"))
        );
        assert_eq!(search_zhuyin_index("ㄅ", 0), None);
    }

    #[test]
    fn shuffled_spellings_parse_to_their_canonical_syllable() {
        let parser = ZhuyinParser::new();
        // The SPEC's own example: ㄅㄝㄧ (shuffled ㄅㄧㄝ) is bie, and it
        // displays as the canonical spelling, like upstream's aux text.
        let shuffled = parser.parse(
            &keystrokes(ZhuyinScheme::Standard, &["ㄅ", "ㄝ", "ㄧ"]),
            true,
            false,
        );
        assert_eq!(shuffled.consumed(), 3);
        assert_eq!(shuffled.full_pinyin(), "bie");
        assert_eq!(shuffled.keys()[0].zhuyin(), "ㄅㄧㄝ");

        // Every permutation of a three-symbol spelling parses.
        let canonical = ["ㄅ", "ㄧ", "ㄝ"];
        permute(&canonical, &mut |permutation: &[&str]| {
            let parsed = parser.parse(
                &keystrokes(ZhuyinScheme::Standard, permutation),
                true,
                false,
            );
            assert_eq!(
                parsed.full_pinyin(),
                "bie",
                "permutation {permutation:?} must parse as bie"
            );
        });

        // A two-symbol swap: ㄧㄅ (shuffled ㄅㄧ) is bi.
        let swapped = parser.parse(
            &keystrokes(ZhuyinScheme::Standard, &["ㄧ", "ㄅ"]),
            true,
            false,
        );
        assert_eq!(swapped.full_pinyin(), "bi");

        // Shuffle composes with tone.
        let toned = parser.parse(
            &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄅ", "ㄝ", "ㄧ"], 4),
            true,
            false,
        );
        assert_eq!(toned.full_pinyin(), "bie");
        assert_eq!(toned.keys()[0].tone(), 4);
    }

    #[test]
    fn recovered_rows_parse_with_their_upstream_tone_masks() {
        let parser = ZhuyinParser::new();

        // ㄉㄣ (den): tones 0 and 4 valid; tone 1 is mask-invalid, so the
        // post-match break stops the whole parse at the two symbol keys.
        assert_eq!(
            parser
                .parse(
                    &keystrokes(ZhuyinScheme::Standard, &["ㄉ", "ㄣ"]),
                    true,
                    false
                )
                .full_pinyin(),
            "den"
        );
        let den4 = parser.parse(
            &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄉ", "ㄣ"], 4),
            true,
            false,
        );
        assert_eq!(den4.consumed(), 3);
        assert_eq!(den4.full_pinyin(), "den");
        let den1 = parser.parse(
            &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄉ", "ㄣ"], 1),
            true,
            false,
        );
        assert_eq!(den1.consumed(), 0, "tone 1 is mask-invalid for den");

        // ㄥ (eng): tones 0 and 1 valid — first tone legal, unlike ㄋㄧ.
        let eng = parser.parse(b"/", true, false);
        assert_eq!(eng.full_pinyin(), "eng");
        let eng1 = parser.parse(
            &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄥ"], 1),
            true,
            false,
        );
        assert_eq!(eng1.consumed(), 2, "tone 1 is mask-valid for eng");
        assert_eq!(eng1.full_pinyin(), "eng");

        // ㄓㄟ (zhei): tones 0 and 4 valid.
        assert_eq!(parser.parse(b"5o", true, false).full_pinyin(), "zhei");
        assert_eq!(
            parser
                .parse(
                    &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄓ", "ㄟ"], 4),
                    true,
                    false
                )
                .consumed(),
            3
        );
        assert_eq!(
            parser
                .parse(
                    &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄓ", "ㄟ"], 1),
                    true,
                    false
                )
                .consumed(),
            0
        );

        // The five dead rows (mask 0): matched, then rejected by the
        // post-match validity break — consumed 0 on the whole input.
        for input in [
            keystrokes(ZhuyinScheme::Standard, &["ㄈ", "ㄜ"]), // fe
            keystrokes(ZhuyinScheme::Standard, &["ㄉ", "ㄧ", "ㄣ"]), // din
            keystrokes(ZhuyinScheme::Standard, &["ㄎ", "ㄟ"]), // kei
            keystrokes(ZhuyinScheme::Standard, &["ㄌ", "ㄣ"]), // len
            keystrokes(ZhuyinScheme::Standard, &["ㄖ", "ㄨ", "ㄚ"]), // rua
        ] {
            let parsed = parser.parse(&input, true, false);
            assert_eq!(parsed.consumed(), 0, "{input:?} is a dead row at the pin");
        }
    }

    #[test]
    fn valid_zhuyin_tones_carry_the_extracted_masks() {
        use super::VALID_ZHUYIN_TONES;
        use crate::SYLLABLE_KEY_COUNT;
        use crate::SyllableKey;

        assert_eq!(VALID_ZHUYIN_TONES.len(), SYLLABLE_KEY_COUNT);

        let mask = |spelling: &str| {
            VALID_ZHUYIN_TONES[SyllableKey::from_canonical_text(spelling)
                .unwrap_or_else(|| panic!("{spelling:?} is not a canonical key"))
                .index()]
        };
        // The twelve recovered rows and the incomplete tier, from the
        // pinned valid_zhuyin_table extraction.
        assert_eq!(mask("eng"), 0x03);
        assert_eq!(mask("nun"), 0x15);
        assert_eq!(mask("chua"), 0x0b);
        assert_eq!(mask("den"), 0x11);
        assert_eq!(mask("zhei"), 0x11);
        assert_eq!(mask("nia"), 0x05);
        assert_eq!(mask("yai"), 0x05);
        assert_eq!(mask("fe"), 0x00);
        assert_eq!(mask("din"), 0x00);
        assert_eq!(mask("kei"), 0x00);
        assert_eq!(mask("len"), 0x00);
        assert_eq!(mask("rua"), 0x00);
        // The 14 zhuyin incomplete rows are ㄅ..ㄒ; every initial-only key
        // is parse-dead in zhuyin at the pin.
        for key in crate::INCOMPLETE_PINYIN_KEYS {
            assert_eq!(mask(key), 0x00, "initial-only key {key:?}");
        }
    }

    #[test]
    fn incomplete_keys_consume_zero_either_way() {
        let parser = ZhuyinParser::new();
        // With the option off the row is gated at the index; with it on
        // the row matches and the post-match validity mask rejects it.
        // Both consume 0 at this pin — same outcome as upstream.
        assert_eq!(parser.parse(b"1", true, false).consumed(), 0);
        assert_eq!(parser.parse(b"1", true, true).consumed(), 0);
        assert_eq!(parser.parse(b"x", true, false).consumed(), 0);
        assert_eq!(parser.parse(b"x", true, true).consumed(), 0);
    }

    #[test]
    fn simple_keyboards_switch_tables_and_stick() {
        let mut parser = ZhuyinParser::new();
        for scheme in [
            ZhuyinScheme::Ibm,
            ZhuyinScheme::Ginyieh,
            ZhuyinScheme::Eten,
            ZhuyinScheme::Standard,
        ] {
            assert!(parser.set_scheme(scheme));
            assert_eq!(parser.scheme(), scheme);
        }
        // The Discrete/CP26 keyboards and the dvorak abort slot keep the
        // current scheme.
        for rejected in [
            ZhuyinScheme::Hsu,
            ZhuyinScheme::Eten26,
            ZhuyinScheme::StandardDvorak,
            ZhuyinScheme::HsuDvorak,
            ZhuyinScheme::DachenCp26,
        ] {
            assert!(!parser.set_scheme(rejected));
            assert_eq!(parser.scheme(), ZhuyinScheme::Standard);
        }
    }

    #[test]
    fn every_simple_keyboard_parses_the_same_symbols_through_its_own_keys() {
        for scheme in [
            ZhuyinScheme::Standard,
            ZhuyinScheme::Ibm,
            ZhuyinScheme::Ginyieh,
            ZhuyinScheme::Eten,
        ] {
            let parser = ZhuyinParser::with_scheme(scheme);

            // Tone-bearing and tone-rejection through this keyboard's keys.
            let ni2 = parser.parse(&keystrokes_with_tone(scheme, &["ㄋ", "ㄧ"], 2), true, false);
            assert_eq!((scheme, ni2.consumed()), (scheme, 3));
            assert_eq!((scheme, ni2.full_pinyin().as_str()), (scheme, "ni"));
            let ni1 = parser.parse(&keystrokes_with_tone(scheme, &["ㄋ", "ㄧ"], 1), true, false);
            assert_eq!((scheme, ni1.consumed()), (scheme, 0));

            // Keyboard membership mirrors the tables: every symbol key
            // reports its symbol, every tone key its mark, others nothing.
            let tables = scheme.simple_tables().expect("Simple keyboard");
            for &(key, symbol) in tables.symbols {
                assert!(
                    parser.in_scheme(key, false),
                    "{scheme:?} symbol key {key:?}"
                );
                assert_eq!(parser.symbols_for(key, false), vec![symbol.to_owned()]);
            }
            for &(key, _tone) in tables.tones {
                assert!(!parser.in_scheme(key, false));
                assert!(parser.in_scheme(key, true));
            }
            assert!(!parser.in_scheme(b'Q', true));
            assert!(parser.symbols_for(b'Q', true).is_empty());
        }
    }

    #[test]
    fn shuffle_and_recovered_rows_work_on_every_simple_keyboard() {
        for scheme in [
            ZhuyinScheme::Standard,
            ZhuyinScheme::Ibm,
            ZhuyinScheme::Ginyieh,
            ZhuyinScheme::Eten,
        ] {
            let parser = ZhuyinParser::with_scheme(scheme);

            // Shuffled spelling parses to the canonical syllable and
            // displays canonically.
            let shuffled = parser.parse(&keystrokes(scheme, &["ㄅ", "ㄝ", "ㄧ"]), true, false);
            assert_eq!((scheme, shuffled.consumed()), (scheme, 3));
            assert_eq!(shuffled.full_pinyin(), "bie");
            assert_eq!(shuffled.keys()[0].zhuyin(), "ㄅㄧㄝ");

            // One recovered live row: ㄉㄣ accepts tone 4, rejects tone 1.
            let den4 = parser.parse(&keystrokes_with_tone(scheme, &["ㄉ", "ㄣ"], 4), true, false);
            assert_eq!((scheme, den4.consumed()), (scheme, 3));
            assert_eq!(den4.full_pinyin(), "den");
            let den1 = parser.parse(&keystrokes_with_tone(scheme, &["ㄉ", "ㄣ"], 1), true, false);
            assert_eq!((scheme, den1.consumed()), (scheme, 0));

            // One dead row: ㄈㄜ matches, then the validity break consumes
            // nothing — on every keyboard.
            let fe = parser.parse(&keystrokes(scheme, &["ㄈ", "ㄜ"]), true, false);
            assert_eq!((scheme, fe.consumed()), (scheme, 0));
        }
    }

    #[test]
    fn apostrophe_keys_follow_the_pinned_tables() {
        // The two keyboards whose tables use the escaped-quote key.
        assert_eq!(key_for_symbol(ZhuyinScheme::Ginyieh, "ㄥ"), b'\'');
        assert_eq!(key_for_symbol(ZhuyinScheme::Eten, "ㄘ"), b'\'');
    }
}
