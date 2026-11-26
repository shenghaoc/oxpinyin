use super::{DoublePinyinParser, DoublePinyinScheme, ZhuyinParse, ZhuyinParser, ZhuyinScheme};
use crate::full_pinyin_index::{luoma_pinyin_index, secondary_zhuyin_index};
use crate::options::OptionBits;
use crate::parser::{FullPinyinScheme, parse_zhuyin_direct};

fn toned(parser: &ZhuyinParser, input: &[u8]) -> ZhuyinParse {
    parser.parse_with_options(input, OptionBits::USE_TONE)
}

fn double_full_pinyin(parse: &crate::parser::DoublePinyinParse) -> String {
    parse
        .keys()
        .iter()
        .map(|item| item.key().text())
        .collect::<Vec<_>>()
        .join("'")
}

fn key_for_symbol(scheme: ZhuyinScheme, symbol: &str) -> u8 {
    scheme
        .simple_tables()
        .expect("simple keyboard")
        .symbols
        .iter()
        .find(|(_, candidate)| *candidate == symbol)
        .expect("symbol is in the keyboard")
        .0
}

fn keystrokes(scheme: ZhuyinScheme, symbols: &[&str]) -> Vec<u8> {
    symbols
        .iter()
        .map(|symbol| key_for_symbol(scheme, symbol))
        .collect()
}

fn keystrokes_with_tone(scheme: ZhuyinScheme, symbols: &[&str], tone: u8) -> Vec<u8> {
    let mut keys = keystrokes(scheme, symbols);
    let tone_key = scheme
        .simple_tables()
        .expect("simple keyboard")
        .tones
        .iter()
        .find(|(_, candidate)| *candidate == tone)
        .expect("tone is in the keyboard")
        .0;
    keys.push(tone_key);
    keys
}

// Pin zhuyin_parser2.cpp:43-100 at 074a2219 gates incomplete and correction rows independently.
#[test]
fn index_flags_gate_through_the_option_word() {
    use crate::zhuyin_map::{eten26_zhuyin_index, hsu_zhuyin_index, zhuyin_pinyin_map};

    for (index, input, options, expected) in [
        (
            zhuyin_pinyin_map(),
            "ㄅㄝㄧ",
            OptionBits::ZHUYIN_CORRECT_SHUFFLE,
            Some("bie"),
        ),
        (zhuyin_pinyin_map(), "ㄅㄝㄧ", OptionBits::empty(), None),
        (
            zhuyin_pinyin_map(),
            "ㄅ",
            OptionBits::ZHUYIN_INCOMPLETE,
            Some("b"),
        ),
        (zhuyin_pinyin_map(), "ㄅ", OptionBits::empty(), None),
        (
            hsu_zhuyin_index(),
            "ㄍㄧ",
            OptionBits::ZHUYIN_CORRECT_HSU,
            Some("ji"),
        ),
        (
            eten26_zhuyin_index(),
            "ㄍㄧ",
            OptionBits::ZHUYIN_CORRECT_ETEN26,
            Some("qi"),
        ),
    ] {
        assert_eq!(
            super::search_zhuyin_index(index, input, options).map(|key| key.text()),
            expected,
            "{input:?}, {options:?}"
        );
    }
}

// Pin zhuyin_parser2.cpp:162-210,216-268 at 074a2219 combines the keyboard correction with tone.
#[test]
fn shuffled_spellings_parse_to_their_canonical_syllable() {
    let parsed = toned(
        &ZhuyinParser::new(),
        &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄅ", "ㄝ", "ㄧ"], 4),
    );
    assert_eq!(parsed.full_pinyin(), "bie");
    assert_eq!(parsed.keys()[0].tone(), 4);
}

// Pin chewing_key.cpp:38-45 and zhuyin_parser2.cpp:255-257 at 074a2219 apply the matched key's tone mask.
#[test]
fn recovered_rows_parse_with_their_upstream_tone_masks() {
    let scheme = ZhuyinScheme::Standard;
    let parser = ZhuyinParser::new();
    for (input, consumed, pinyin) in [
        (keystrokes(scheme, &["ㄉ", "ㄣ"]), None, Some("den")),
        (
            keystrokes_with_tone(scheme, &["ㄉ", "ㄣ"], 4),
            Some(3),
            Some("den"),
        ),
        (
            keystrokes_with_tone(scheme, &["ㄉ", "ㄣ"], 1),
            Some(0),
            None,
        ),
        (
            keystrokes_with_tone(scheme, &["ㄥ"], 1),
            Some(2),
            Some("eng"),
        ),
        (keystrokes(scheme, &["ㄈ", "ㄜ"]), Some(0), None),
    ] {
        let parsed = toned(&parser, &input);
        if let Some(consumed) = consumed {
            assert_eq!(parsed.consumed(), consumed, "{input:?}");
        }
        if let Some(pinyin) = pinyin {
            assert_eq!(parsed.full_pinyin(), pinyin, "{input:?}");
        }
    }
}

// Pin zhuyin_parser2.cpp:270-332 at 074a2219 selects each frozen simple keyboard's symbol and tone tables.
#[test]
fn every_simple_keyboard_parses_the_same_symbols_through_its_own_keys() {
    for scheme in [
        ZhuyinScheme::Standard,
        ZhuyinScheme::Ibm,
        ZhuyinScheme::Ginyieh,
        ZhuyinScheme::Eten,
    ] {
        let parser = ZhuyinParser::with_scheme(scheme);
        let parsed = toned(&parser, &keystrokes_with_tone(scheme, &["ㄋ", "ㄧ"], 2));
        assert_eq!(parsed.consumed(), 3, "{scheme:?}");
        assert_eq!(parsed.full_pinyin(), "ni", "{scheme:?}");

        let tables = scheme.simple_tables().expect("simple keyboard");
        for &(key, symbol) in tables.symbols {
            assert!(parser.in_scheme(key, false), "{scheme:?}, {key:?}");
            assert_eq!(
                parser.symbols_for(key, false),
                [symbol],
                "{scheme:?}, {key:?}"
            );
        }
        for &(key, _) in tables.tones {
            assert!(!parser.in_scheme(key, false), "{scheme:?}, {key:?}");
            assert!(parser.in_scheme(key, true), "{scheme:?}, {key:?}");
        }
    }
    let parser = ZhuyinParser::new();
    assert!(!parser.in_scheme(b'Q', true));
    assert!(parser.symbols_for(b'Q', true).is_empty());
}

fn discrete_key_for(scheme: ZhuyinScheme, slot: usize, symbol: &str) -> u8 {
    let tables = scheme.discrete_tables().expect("discrete keyboard");
    let ordered: &[&[(u8, &'static str)]] = match slot {
        0 => &[tables.initials, tables.middles, tables.finals],
        1 => &[tables.middles, tables.finals],
        _ => &[tables.finals],
    };
    for table in ordered {
        let mut last_key = None;
        for &(key, candidate) in *table {
            if last_key == Some(key) {
                continue;
            }
            last_key = Some(key);
            if candidate == symbol {
                return key;
            }
        }
    }
    panic!("no {scheme:?} key derives {symbol:?} at slot {slot}");
}

fn discrete_keystrokes(scheme: ZhuyinScheme, symbols: &[&str]) -> Vec<u8> {
    symbols
        .iter()
        .enumerate()
        .map(|(slot, symbol)| discrete_key_for(scheme, slot, symbol))
        .collect()
}

// Pin zhuyin_parser2.cpp:335-405,474-490 at 074a2219 selects the positional keyboard and its index.
#[test]
fn discrete_keyboards_parse_through_positional_probes() {
    for scheme in [
        ZhuyinScheme::Hsu,
        ZhuyinScheme::Eten26,
        ZhuyinScheme::HsuDvorak,
    ] {
        let parser = ZhuyinParser::with_scheme(scheme);
        let parsed = toned(&parser, &discrete_keystrokes(scheme, &["ㄋ", "ㄧ"]));
        assert_eq!(parsed.consumed(), 2, "{scheme:?}");
        assert_eq!(parsed.full_pinyin(), "ni", "{scheme:?}");
    }

    let scheme = ZhuyinScheme::Hsu;
    let mut input = discrete_keystrokes(scheme, &["ㄋ", "ㄧ"]);
    let tone_key = scheme
        .discrete_tables()
        .expect("discrete keyboard")
        .tones
        .iter()
        .find(|(_, tone)| *tone == 2)
        .expect("tone is in the keyboard")
        .0;
    input.push(tone_key);
    assert_eq!(
        toned(&ZhuyinParser::with_scheme(scheme), &input).consumed(),
        3
    );
}

// Pin zhuyin_parser2.cpp:474-490 at 074a2219 binds different correction indexes to HSU and ETEN26.
#[test]
fn hsu_and_eten26_select_their_correction_index() {
    for (scheme, expected) in [(ZhuyinScheme::Hsu, "ji"), (ZhuyinScheme::Eten26, "qi")] {
        let parsed = toned(
            &ZhuyinParser::with_scheme(scheme),
            &discrete_keystrokes(scheme, &["ㄍ", "ㄧ"]),
        );
        assert_eq!(parsed.full_pinyin(), expected, "{scheme:?}");
    }
}

// Pin zhuyin_parser2.cpp:335-405,496-545 at 074a2219 displays every mapping but parses the first.
#[test]
fn dual_mapped_keys_report_every_symbol_but_parse_the_first() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::Hsu);
    for (key, use_tone, expected) in [
        (b'c', false, &["ㄒ", "ㄕ"][..]),
        (b'e', false, &["ㄧ", "ㄝ"][..]),
        (b'd', true, &["ㄉ", super::tone_symbol(2)][..]),
        (b'Q', true, &[][..]),
    ] {
        assert_eq!(parser.symbols_for(key, use_tone), expected, "{key:?}");
    }
    assert!(parser.in_scheme(b'd', false));
    assert!(!parser.in_scheme(b'Q', true));
    assert_eq!(toned(&parser, b"c").full_pinyin(), "shi");
}

// Pin zhuyin_parser2.cpp:556-716,718-762 at 074a2219 cycles repeated keys and greedily appends syllables.
#[test]
fn cp26_repeat_counts_cycle_dual_rows() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::DachenCp26);
    for (input, expected) in [
        ("t", "zhi"),
        ("tt", "chi"),
        ("i", "o"),
        ("ii", "ai"),
        ("u", "yi"),
        ("uu", "a"),
        ("uuu", "ya"),
        ("m", "yu"),
        ("mm", "ou"),
        ("g", "shi"),
        ("qqu", "pi"),
        ("quu", "ba"),
        ("amm", "mou"),
        ("vub", "xie"),
        ("sug", "ni'shi"),
    ] {
        assert_eq!(
            toned(&parser, input.as_bytes()).full_pinyin(),
            expected,
            "{input:?}"
        );
    }
    assert_eq!(toned(&parser, b"qmm").consumed(), 0);
}

// Pin zhuyin_parser2.cpp:594-609 at 074a2219 probes and removes the last byte's tone before its symbol.
#[test]
fn cp26_tone_probe_wins_the_last_byte() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::DachenCp26);
    let parsed = toned(&parser, b"eke");
    assert_eq!(parsed.full_pinyin(), "ge");
    assert_eq!(parsed.keys()[0].tone(), 2);
    assert_eq!(toned(&parser, b"d").consumed(), 0);
}

// Pin zhuyin_parser2.cpp:796-844 at 074a2219 orders slot mappings, the i-key extra, and the tone mark.
#[test]
fn cp26_display_reports_the_special_and_dual_rows() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::DachenCp26);
    for (key, use_tone, expected) in [
        (b'i', false, &["ㄛ", "ㄞ", "ㄧㄚ"][..]),
        (b't', false, &["ㄓ", "ㄔ"][..]),
        (b'e', true, &["ㄍ", super::tone_symbol(2)][..]),
        (b'u', false, &["ㄧ", "ㄚ"][..]),
    ] {
        assert_eq!(parser.symbols_for(key, use_tone), expected, "{key:?}");
    }
    assert!(!parser.in_scheme(b'Q', true));
}

// Pin pinyin_parser2.cpp:546-570 at 074a2219 appends ordered spans covering the consumed prefix.
#[test]
fn double_pinyin_produces_aligned_key_spans() {
    let input = b"uiaaaa";
    let parse = DoublePinyinParser::new().parse_with_options(input, OptionBits::empty());
    let mut cursor = 0;
    for key in parse.keys() {
        assert!(key.start() >= cursor);
        assert!(key.end() > key.start());
        cursor = key.end();
    }
    assert!(cursor <= input.len());
    assert!(parse.consumed() >= cursor);
}

// Pin zhuyin_parser2.cpp:176-180,216-268 at 074a2219 passes forced-tone refusals into the batch walk.
#[test]
fn parse_with_options_honours_force_tone_like_the_batch_parser() {
    let parser = ZhuyinParser::new();
    let forced = OptionBits::USE_TONE | OptionBits::FORCE_TONE;
    for (input, options, consumed) in [
        ("su3", forced, 3),
        ("li", forced, 0),
        ("li", OptionBits::USE_TONE, 2),
    ] {
        assert_eq!(
            parser
                .parse_with_options(input.as_bytes(), options)
                .consumed(),
            consumed,
            "{input:?}, {options:?}"
        );
    }
}

// Pin pinyin_parser2.cpp:383-401 at 074a2219 selects Hanyu, Luoma, or Secondary Zhuyin.
#[test]
fn the_full_pinyin_scheme_slot_enumerates_its_three_romanizations() {
    for (scheme, expected) in [
        (FullPinyinScheme::Hanyu, None),
        (FullPinyinScheme::Luoma, Some(luoma_pinyin_index())),
        (
            FullPinyinScheme::SecondaryZhuyin,
            Some(secondary_zhuyin_index()),
        ),
    ] {
        assert_eq!(scheme.index(), expected, "{scheme:?}");
    }
}

// Pin zhuyin_parser2.cpp:859-959 at 074a2219 parses whole segments and consumes following separator runs.
#[test]
fn zhuyin_direct_parse_is_the_pins_bopomofo_split() {
    let options = OptionBits::USE_TONE | OptionBits::FORCE_TONE;
    for (input, expected) in [
        ("ㄘㄜˋ ㄘㄜˋ", &[("ce", 4), ("ce", 4)][..]),
        ("ㄘㄜ'ㄘㄜ", &[("ce", 1), ("ce", 1)][..]),
        ("ㄑㄧㄥ ㄕㄥ˙", &[("qing", 1), ("sheng", 5)][..]),
        ("ㄋㄧˇ  ㄏㄠˇ ", &[("ni", 3), ("hao", 3)][..]),
        ("ce", &[][..]),
        ("ㄑ", &[][..]),
        ("ㄘ ㄑ", &[("ci", 1)][..]),
        ("ㄘㄜˋ xx ㄘㄜˋ", &[("ce", 4)][..]),
        ("", &[][..]),
    ] {
        let parsed: Vec<_> = parse_zhuyin_direct(input, options)
            .into_iter()
            .map(|(key, tone)| (key.text(), tone))
            .collect();
        assert_eq!(parsed, expected, "{input:?}");
    }
}

// Pin pinyin_parser2.cpp:405-530 at 074a2219 specifies whole-key length, tone, initial, and yunmu probes.
#[test]
fn double_one_key_law() {
    let parser = DoublePinyinParser::new();
    let use_tone = OptionBits::USE_TONE;
    let force_tone = OptionBits::FORCE_TONE;
    for (input, options, expected) in [
        ("ni", OptionBits::empty(), Some(("ni", 0))),
        ("ni3", OptionBits::empty(), None),
        ("ni3", use_tone, Some(("ni", 3))),
        ("ni", force_tone, None),
        ("ni3", use_tone | force_tone, Some(("ni", 3))),
        ("ni9", use_tone, None),
        ("niha", OptionBits::empty(), None),
        ("z", OptionBits::empty(), None),
        ("z", OptionBits::PINYIN_INCOMPLETE, Some(("z", 0))),
        ("oo", OptionBits::empty(), Some(("o", 0))),
    ] {
        let parsed = parser.parse_one_key(options, input.as_bytes());
        assert_eq!(
            parsed.map(|key| (key.pinyin_spelling(), key.tone)),
            expected,
            "{input:?}, {options:?}"
        );
    }
}

// Pin zhuyin_parser2.cpp:162-210 at 074a2219 gates whole-key tones and incomplete rows; Rust guards empty input.
#[test]
fn chewing_one_key_law_on_standard() {
    let parser = ZhuyinParser::new();
    let use_tone = OptionBits::USE_TONE;
    let forced = use_tone | OptionBits::FORCE_TONE;
    for (input, options, expected) in [
        ("18", OptionBits::empty(), Some(("ba", 0))),
        ("18", use_tone, Some(("ba", 0))),
        ("183", use_tone, Some(("ba", 3))),
        ("18", forced, None),
        ("183", forced, Some(("ba", 3))),
        ("1", OptionBits::empty(), None),
        ("1", OptionBits::ZHUYIN_INCOMPLETE, Some(("b", 0))),
        ("", use_tone, None),
    ] {
        let parsed = parser.parse_one_key(options, input.as_bytes());
        assert_eq!(
            parsed.map(|key| (key.pinyin_spelling(), key.tone)),
            expected,
            "{input:?}, {options:?}"
        );
    }
}

// Pin pinyin_parser2.cpp:531-574 at 074a2219 retries shorter keys and stops when every probe refuses.
#[test]
fn batch_parse_honours_use_tone_and_force_tone() {
    let parser = DoublePinyinParser::new();
    let use_tone = OptionBits::USE_TONE;
    let force_tone = OptionBits::FORCE_TONE;
    for (input, options, consumed, tone) in [
        ("ni3", use_tone, 3, Some(3)),
        ("ni3", OptionBits::empty(), 2, Some(0)),
        ("nix", use_tone, 2, None),
        ("ni0", use_tone, 2, None),
        ("ni", force_tone, 0, None),
        ("ni3", force_tone, 0, None),
        ("ni3", use_tone | force_tone, 3, Some(3)),
        ("ni3ha4", use_tone | force_tone, 6, None),
    ] {
        let parsed = parser.parse_with_options(input.as_bytes(), options);
        assert_eq!(parsed.consumed(), consumed, "{input:?}, {options:?}");
        if let Some(tone) = tone {
            assert_eq!(parsed.keys()[0].tone(), tone, "{input:?}, {options:?}");
        }
        if input == "ni3" && options == use_tone {
            assert_eq!(parsed.keys().len(), 1);
            assert_eq!(double_full_pinyin(&parsed), "ni");
        }
    }
}

// Pin zhuyin_parser2.cpp:246-257 and chewing_key.cpp:38-45 at 074a2219 stop after an invalid selected key.
#[test]
fn illegal_first_tone_on_ni_stops_without_retrying_a_shorter_key() {
    let parser = ZhuyinParser::new();
    let use_tone = OptionBits::USE_TONE;
    for (input, options, consumed, expected) in [
        ("su ", use_tone, 0, Some("")),
        ("sux6", use_tone, 2, Some("ni")),
        ("1", use_tone, 0, None),
        ("1", use_tone | OptionBits::ZHUYIN_INCOMPLETE, 0, None),
    ] {
        let parsed = parser.parse_with_options(input.as_bytes(), options);
        assert_eq!(parsed.consumed(), consumed, "{input:?}, {options:?}");
        if let Some(expected) = expected {
            assert_eq!(parsed.full_pinyin(), expected, "{input:?}, {options:?}");
        }
    }
}

// Pin pinyin_parser2.cpp:578-614 at 074a2219 clears fallback before an out-of-enum scheme dispatch.
#[test]
fn with_fallback_suppressed_clears_only_the_fallback() {
    let parser = DoublePinyinParser::with_scheme(DoublePinyinScheme::Zrm);
    for (suppressed, input, consumed) in [
        (false, "aa", 2),
        (false, "ni", 2),
        (true, "aa", 0),
        (true, "ni", 2),
    ] {
        let parser = if suppressed {
            parser.with_fallback_suppressed()
        } else {
            parser
        };
        assert_eq!(
            parser
                .parse_with_options(input.as_bytes(), OptionBits::empty())
                .consumed(),
            consumed,
            "{input:?}, suppressed={suppressed}"
        );
    }
}

const FIXTURE: &str = include_str!("../../fixtures/w4/oracle-zhuyin-parse.txt");

const EXPECTED_PIN_REF: &str = "libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99\
+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155\
+dbm-tkrzw";

const CAPTURE_SCHEMA: &str = "pinyin-capture-v1";

struct CaptureLine {
    case: String,
    input: String,
    flags: OptionBits,
    scheme: u8,
    parse_return: usize,
    parsed_input_length: usize,
    segments: String,
    zhuyin: String,
    remainder: String,
}

/// The capture format's escape decoder (\\, \t, \n, \r, \xNN).
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('x') => {
                let mut hex = String::new();
                for _ in 0..2 {
                    if let Some(digit) = chars.next() {
                        hex.push(digit);
                    }
                }
                let byte = u8::from_str_radix(&hex, 16).unwrap_or(b'?');
                out.push(byte as char);
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn fixture_lines() -> Vec<CaptureLine> {
    let mut lines = Vec::new();
    for raw in FIXTURE.lines() {
        let mut fields = raw.split('\t');
        let mut next = || fields.next().unwrap_or("-");
        assert_eq!(next(), format!("schema={CAPTURE_SCHEMA}"), "schema drift");
        let pin = next().strip_prefix("pin_ref=").expect("pin_ref field");
        assert_eq!(
            pin, EXPECTED_PIN_REF,
            "the fixture's pin must be the recorded pin"
        );
        assert_eq!(next(), "family=ZH", "the fixture must hold only ZH lines");
        let case = next().strip_prefix("case=").expect("case field").to_owned();
        let _api_sequence = next(); // recorded provenance, not replayed
        let input = unescape(next().strip_prefix("input=").expect("input field"));
        let flags = u32::from_str_radix(next().strip_prefix("flags=0x").expect("flags field"), 16)
            .map(OptionBits::from_bits_retain)
            .expect("hex flags");
        let scheme: u8 = next()
            .strip_prefix("scheme=")
            .expect("scheme field")
            .parse()
            .expect("scheme number");
        let parse_return: usize = next()
            .strip_prefix("parse_return=")
            .expect("parse_return field")
            .parse()
            .expect("parse_return number");
        let parsed_input_length: usize = next()
            .strip_prefix("parsed_input_length=")
            .expect("parsed_input_length field")
            .parse()
            .expect("parsed_input_length number");
        let segments = unescape(next().strip_prefix("segments=").expect("segments field"));
        let zhuyin = unescape(next().strip_prefix("zhuyin=").expect("zhuyin field"));
        let remainder = unescape(next().strip_prefix("remainder=").expect("remainder field"));
        lines.push(CaptureLine {
            case,
            input,
            flags,
            scheme,
            parse_return,
            parsed_input_length,
            segments,
            zhuyin,
            remainder,
        });
    }
    lines
}

fn scheme_for(value: u8) -> ZhuyinScheme {
    match ZhuyinScheme::try_from(i32::from(value)) {
        Ok(ZhuyinScheme::StandardDvorak) | Err(_) => {
            panic!("scheme {value} is outside the parseable keyboards")
        }
        Ok(scheme) => scheme,
    }
}

// Frozen oracle-zhuyin-parse.txt at 074a2219 holds the consumed prefix, key spans, and renderings.
#[test]
fn zhuyin_parse_matches_the_frozen_oracle_pin() {
    for line in fixture_lines() {
        let parser = ZhuyinParser::with_scheme(scheme_for(line.scheme));
        let parsed = parser.parse_with_options(
            line.input.as_bytes(),
            line.flags & (OptionBits::USE_TONE | OptionBits::ZHUYIN_INCOMPLETE),
        );

        assert_eq!(
            parsed.consumed(),
            line.parse_return,
            "{}: parse_return must match the oracle",
            line.case
        );
        assert_eq!(
            parsed.consumed(),
            line.parsed_input_length,
            "{}: the oracle's two length reads agree; the replay must too",
            line.case
        );

        let segments: Vec<String> = parsed
            .keys()
            .iter()
            .map(|key| {
                format!(
                    "{}@{}:{}",
                    key.key()
                        .chewing_key()
                        .with_tone(key.tone())
                        .pinyin_string(),
                    key.start(),
                    key.end()
                )
            })
            .collect();
        let expected = if line.segments == "-" {
            String::new()
        } else {
            line.segments.clone()
        };
        let actual = if segments.is_empty() {
            String::new()
        } else {
            segments.join(",")
        };
        assert_eq!(
            actual, expected,
            "{}: segments (pinyin@begin:end) must match the oracle",
            line.case
        );

        let zhuyin: Vec<String> = parsed
            .keys()
            .iter()
            .map(crate::parser::ZhuyinKey::display)
            .collect();
        let expected_zhuyin = if line.zhuyin == "-" {
            String::new()
        } else {
            line.zhuyin.clone()
        };
        let actual_zhuyin = if zhuyin.is_empty() {
            String::new()
        } else {
            zhuyin.join(",")
        };
        assert_eq!(
            actual_zhuyin, expected_zhuyin,
            "{}: zhuyin renderings must match the oracle",
            line.case
        );

        assert_eq!(
            &line.input[parsed.consumed()..],
            line.remainder,
            "{}: the unparsed remainder must match the oracle",
            line.case
        );
    }
}
