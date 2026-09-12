//! The frozen zhuyin parse pin — G1's capture fixture
//! (`testing-strategy.md` §2, the `test_zhuyin.cpp` battery).
//!
//! `fixtures/w4/oracle-zhuyin-parse.txt` was captured from the pinned
//! oracle (`tools/capture/run-capture.sh` family ZH; pin ref below) by
//! driving `pinyin_parse_more_chewings` over authored keystroke lines in
//! the shape of upstream's `tests/test_zhuyin.cpp` — stdin-line driven,
//! no committed corpus of its own — across every parseable keyboard and
//! the three option words that reach the parser. This test replays the
//! same battery through oxpinyin's public parser seam and demands
//! byte-for-byte agreement, so zhuyin parity is a frozen pin that runs
//! on every `cargo test`, not only where the oracle is built.
//!
//! The replay seam is the one the pinyin facade forwards to
//! (`parse_chewing_more`, `ToneForwarding::PinFacade`):
//! `ZhuyinParser::parse(input, use_tone, allow_incomplete)` with both
//! bits read off the captured option word — `FORCE_TONE` never crosses
//! that facade. Per key the fixture carries both renderings the oracle
//! hands out: the pinyin string (`_ChewingKey::get_pinyin_string`'s law:
//! canonical spelling plus the tone digit for a non-zero tone) and the
//! zhuyin string (`get_zhuyin_string`'s law: tone marks on tones 2..5).
//!
//! When the pin moves, regenerate the fixture with
//! `tools/capture/run-capture.sh` and update `EXPECTED_PIN_REF` — a
//! change here is a re-freeze, not a silent drift.

use oxpinyin_core::{USE_TONE, ZHUYIN_INCOMPLETE, ZhuyinKey, ZhuyinParser, ZhuyinScheme};

const FIXTURE: &str = include_str!("../../../fixtures/w4/oracle-zhuyin-parse.txt");
const EXPECTED_PIN_REF: &str = "libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99\
+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155\
+dbm-tkrzw";

/// The capture schema every line must carry.
const CAPTURE_SCHEMA: &str = "pinyin-capture-v1";

/// One parsed fixture line.
struct CaptureLine {
    case: String,
    input: String,
    flags: u32,
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
    assert!(
        lines.len() >= 20,
        "the battery must stay broad ({}) — a shrunken fixture is a re-freeze",
        lines.len()
    );
    lines
}

/// The scheme discriminants the facade's `zhuyin_scheme` mapping accepts,
/// mirrored here so the fixture drives the same keyboards. 7
/// (`StandardDvorak`) is upstream's setter abort and never appears.
fn scheme_for(value: u8) -> ZhuyinScheme {
    match value {
        1 => ZhuyinScheme::Standard,
        2 => ZhuyinScheme::Hsu,
        3 => ZhuyinScheme::Ibm,
        4 => ZhuyinScheme::Ginyieh,
        5 => ZhuyinScheme::Eten,
        6 => ZhuyinScheme::Eten26,
        8 => ZhuyinScheme::HsuDvorak,
        9 => ZhuyinScheme::DachenCp26,
        other => panic!("scheme {other} is outside the parseable keyboards"),
    }
}

/// `get_pinyin_string`'s law: the canonical spelling with the tone digit
/// appended for a non-zero tone.
fn pinyin_string(key: &ZhuyinKey) -> String {
    let base = key.key().text();
    match key.tone() {
        0 => base.to_owned(),
        tone => format!("{base}{tone}"),
    }
}

/// The whole battery: parse through the public seam, demand the oracle's
/// consumed length, spans, both renderings, and the unparsed remainder.
#[test]
fn zhuyin_parse_matches_the_frozen_oracle_pin() {
    for line in fixture_lines() {
        let parser = ZhuyinParser::with_scheme(scheme_for(line.scheme));
        let use_tone = line.flags & USE_TONE != 0;
        let allow_incomplete = line.flags & ZHUYIN_INCOMPLETE != 0;
        let parsed = parser.parse(line.input.as_bytes(), use_tone, allow_incomplete);

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
            .map(|key| format!("{}@{}:{}", pinyin_string(key), key.start(), key.end()))
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
            .map(oxpinyin_core::ZhuyinKey::display)
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

/// The battery must still cover every parseable keyboard and both option
/// bits — a fixture narrowed to the standard layout would silently shrink
/// the pin.
#[test]
fn the_battery_covers_every_keyboard_and_option_word() {
    let lines = fixture_lines();
    for scheme in [1_u8, 2, 3, 4, 5, 6, 8, 9] {
        assert!(
            lines.iter().any(|line| line.scheme == scheme),
            "scheme {scheme} has no case in the fixture"
        );
    }
    assert!(
        lines.iter().any(|line| line.flags & USE_TONE != 0),
        "no case exercises USE_TONE"
    );
    assert!(
        lines.iter().any(|line| line.flags & ZHUYIN_INCOMPLETE != 0),
        "no case exercises ZHUYIN_INCOMPLETE"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.flags & (USE_TONE | ZHUYIN_INCOMPLETE) == 0),
        "no case exercises the plain option word"
    );
}
