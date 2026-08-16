//! Scheme parsers for non-full-pinyin input surfaces.
//!
//! Double pinyin and Zhuyin are source-embedded keyboard schemes. The
//! double-pinyin tables below are direct ports of the generated upstream
//! header `src/storage/double_pinyin_table.h` at libpinyin `2.11.91`
//! (`0c5e80e1200f84fab185d1c5bde458b770a0636c`), cited per table in
//! `docs/findings/double-pinyin-spec.md`. The STANDARD Zhuyin keyboard and
//! tone tables come from `src/storage/zhuyin_table.h`; the Zhuyin-to-pinyin
//! map is generated from `scripts2/pyzymap.py` and lives in
//! [`crate::zhuyin_map`].

use crate::SyllableKey;
use crate::zhuyin_map::ZHUYIN_PINYIN_MAP;

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

    /// The tone-less Zhuyin spelling.
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

fn standard_symbol(byte: u8) -> Option<&'static str> {
    match byte {
        b',' => Some("ㄝ"),
        b'-' => Some("ㄦ"),
        b'.' => Some("ㄡ"),
        b'/' => Some("ㄥ"),
        b'0' => Some("ㄢ"),
        b'1' => Some("ㄅ"),
        b'2' => Some("ㄉ"),
        b'5' => Some("ㄓ"),
        b'8' => Some("ㄚ"),
        b'9' => Some("ㄞ"),
        b';' => Some("ㄤ"),
        b'a' => Some("ㄇ"),
        b'b' => Some("ㄖ"),
        b'c' => Some("ㄏ"),
        b'd' => Some("ㄎ"),
        b'e' => Some("ㄍ"),
        b'f' => Some("ㄑ"),
        b'g' => Some("ㄕ"),
        b'h' => Some("ㄘ"),
        b'i' => Some("ㄛ"),
        b'j' => Some("ㄨ"),
        b'k' => Some("ㄜ"),
        b'l' => Some("ㄠ"),
        b'm' => Some("ㄩ"),
        b'n' => Some("ㄙ"),
        b'o' => Some("ㄟ"),
        b'p' => Some("ㄣ"),
        b'q' => Some("ㄆ"),
        b'r' => Some("ㄐ"),
        b's' => Some("ㄋ"),
        b't' => Some("ㄔ"),
        b'u' => Some("ㄧ"),
        b'v' => Some("ㄒ"),
        b'w' => Some("ㄊ"),
        b'x' => Some("ㄌ"),
        b'y' => Some("ㄗ"),
        b'z' => Some("ㄈ"),
        _ => None,
    }
}

fn standard_tone(byte: u8) -> Option<u8> {
    match byte {
        b' ' => Some(1),
        b'3' => Some(3),
        b'4' => Some(4),
        b'6' => Some(2),
        b'7' => Some(5),
        _ => None,
    }
}

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

fn lookup_zhuyin(zhuyin: &str) -> Option<SyllableKey> {
    ZHUYIN_PINYIN_MAP
        .iter()
        .find_map(|(spelling, pinyin)| (*spelling == zhuyin).then_some(*pinyin))
        .and_then(SyllableKey::from_text)
}

/// Stateless parser for one Zhuyin keyboard.
///
/// The first W13 pass implements STANDARD only. Unsupported schemes parse
/// nothing and their setters report `false`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZhuyinParser {
    scheme: ZhuyinScheme,
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

    /// Selects a scheme. Only STANDARD is implemented in the first pass.
    pub fn set_scheme(&mut self, scheme: ZhuyinScheme) -> bool {
        if scheme != ZhuyinScheme::Standard {
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
        self.scheme == ZhuyinScheme::Standard
            && (standard_symbol(key).is_some() || (use_tone && standard_tone(key).is_some()))
    }

    /// The Zhuyin symbol(s) mapped by one keystroke.
    ///
    /// STANDARD has at most one symbol per key; a tone key returns its tone
    /// mark, matching `pinyin_in_chewing_keyboard`.
    #[must_use]
    pub fn symbols_for(&self, key: u8, use_tone: bool) -> Vec<String> {
        if self.scheme != ZhuyinScheme::Standard {
            return Vec::new();
        }
        if let Some(symbol) = standard_symbol(key) {
            return vec![symbol.to_owned()];
        }
        if use_tone && let Some(tone) = standard_tone(key) {
            return vec![tone_symbol(tone).to_owned()];
        }
        Vec::new()
    }

    /// Greedily parses `input`, mirroring `ZhuyinSimpleParser2::parse`
    /// (`src/storage/zhuyin_parser2.cpp:216-268`).
    #[must_use]
    pub fn parse(&self, input: &[u8], use_tone: bool) -> ZhuyinParse {
        if self.scheme != ZhuyinScheme::Standard {
            return ZhuyinParse::default();
        }

        let maximum_len = input
            .iter()
            .take_while(|&&byte| {
                standard_symbol(byte).is_some() || (use_tone && standard_tone(byte).is_some())
            })
            .count();

        let mut keys = Vec::new();
        let mut parsed_len = 0;
        while parsed_len < maximum_len {
            let remaining = &input[parsed_len..maximum_len];
            let try_len = remaining.len().min(4);
            let mut matched = None;
            for len in (1..=try_len).rev() {
                if let Some((key, zhuyin, tone)) =
                    parse_one_zhuyin_key(&input[parsed_len..parsed_len + len], use_tone)
                {
                    matched = Some((key, zhuyin, tone, len));
                    break;
                }
            }

            let Some((key, zhuyin, tone, len)) = matched else {
                break;
            };
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

fn parse_one_zhuyin_key(input: &[u8], use_tone: bool) -> Option<(SyllableKey, String, u8)> {
    let mut symbol_len = input.len();
    let mut tone = 0;
    if use_tone
        && symbol_len > 0
        && let Some(value) = standard_tone(input[symbol_len - 1])
    {
        tone = value;
        symbol_len -= 1;
    }
    if symbol_len == 0 {
        return None;
    }

    let mut zhuyin = String::new();
    for byte in &input[..symbol_len] {
        let symbol = standard_symbol(*byte)?;
        zhuyin.push_str(symbol);
    }
    let key = lookup_zhuyin(&zhuyin)?;
    Some((key, zhuyin, tone))
}

#[cfg(test)]
mod tests {
    use super::{DoublePinyinParser, DoublePinyinScheme};

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
}
