//! The pin's parser driver (`tests/storage/test_parser2.cpp:37-161`).

use std::ffi::{CString, OsString};

use oxpinyin_core::{
    DoublePinyinParser, PINYIN_CORRECT_ALL, PINYIN_INCOMPLETE, USE_RESPLIT_TABLE, USE_TONE,
    ZHUYIN_INCOMPLETE, ZhuyinParser, ZhuyinScheme,
    tool::{Key, Parse, parse_direct},
};
use oxpinyin_input::tool::{self, OptionArg, OptionEntry, OptionValue};

enum Parser {
    Full,
    Double,
    Zhuyin(ZhuyinScheme),
    Direct(bool),
}

impl Parser {
    fn parse(&self, input: &[u8], options: u32) -> Parse {
        match *self {
            Self::Full => oxpinyin_engine::tool::parse_full(input, options),
            Self::Double => {
                let parsed = DoublePinyinParser::new().parse_with_options(input, options);
                Parse {
                    consumed: parsed.consumed(),
                    keys: parsed
                        .keys()
                        .iter()
                        .map(|key| {
                            Key::from_syllable(key.key(), key.tone(), key.start()..key.end())
                        })
                        .collect(),
                }
            }
            Self::Zhuyin(scheme) => {
                let parsed = ZhuyinParser::with_scheme(scheme).parse_with_options(input, options);
                Parse {
                    consumed: parsed.consumed(),
                    keys: parsed
                        .keys()
                        .iter()
                        .map(|key| {
                            Key::from_syllable(key.key(), key.tone(), key.start()..key.end())
                        })
                        .collect(),
                }
            }
            Self::Direct(zhuyin) => parse_direct(input, options, zhuyin),
        }
    }
}

/// GLib options, one parse batch per input line and the pin's printed result.
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let entries = [
        OptionEntry {
            long_name: c"parser",
            short_name: b'p',
            flags: 0,
            arg: OptionArg::String(Some(c"")),
            description: Some(c"parser"),
            arg_description: Some(c"fullpinyin doublepinyin zhuyin pinyindirect zhuyindirect"),
        },
        OptionEntry {
            long_name: c"incomplete",
            short_name: b'i',
            flags: 0,
            arg: OptionArg::Flag(false),
            description: Some(c"incomplete pinyin"),
            arg_description: None,
        },
        OptionEntry {
            long_name: c"scheme",
            short_name: b's',
            flags: 0,
            arg: OptionArg::String(Some(c"")),
            description: Some(c"scheme"),
            arg_description: Some(c"standard hsu dachen26"),
        },
    ];
    let parsed = tool::parse_options(c"- test pinyin parser", &entries, args)
        .unwrap_or_else(|_| tool::exit(tool::EINVAL));
    let string = |index| {
        if let OptionValue::String(Some(value)) = &parsed.values[index] {
            value.as_slice()
        } else {
            b""
        }
    };
    let parser = match string(0) {
        b"doublepinyin" => Parser::Double,
        b"zhuyin" => match string(2) {
            b"standard" => Parser::Zhuyin(ZhuyinScheme::Standard),
            b"hsu" => Parser::Zhuyin(ZhuyinScheme::Hsu),
            b"dachen26" => Parser::Zhuyin(ZhuyinScheme::DachenCp26),
            _ => Parser::Full,
        },
        b"pinyindirect" => Parser::Direct(false),
        b"zhuyindirect" => Parser::Direct(true),
        _ => Parser::Full,
    };
    let mut options = PINYIN_CORRECT_ALL | USE_TONE | USE_RESPLIT_TABLE;
    if matches!(parsed.values[1], OptionValue::Flag(true)) {
        options |= PINYIN_INCOMPLETE | ZHUYIN_INCOMPLETE;
    }
    let mut input = tool::LineReader::stdin();
    while input.getline() {
        if input.line() == b"quit" {
            break;
        }
        let result = parser.parse(input.line(), options);
        tool::parser::print_counts(result.consumed, result.keys.len());
        // A single Key carries both arrays' entries: the pin's assertion
        // at :132 is an invariant of this representation.
        for key in result.keys {
            // The frozen chewing inventory and tone digits contain no NUL.
            if let Ok(pinyin) = CString::new(key.key.pinyin_string()) {
                tool::parser::print_key(&pinyin, key.span.start, key.span.end);
            }
        }
        tool::parser::print_newline();
    }
}
