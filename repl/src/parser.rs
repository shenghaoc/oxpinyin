//! The pin's parser driver (`tests/storage/test_parser2.cpp:37-161`).

use std::ffi::{CString, OsString};

use oxpinyin::__internal::parser::{DoublePinyinParser, ZhuyinParser};
use oxpinyin::OptionBits;
use oxpinyin::facade::ZhuyinScheme;
use oxpinyin_tool_support::input::{self as tool, OptionArg, OptionEntry};
use oxpinyin_tool_support::parser::{Key, Parse, parse_direct};

enum Parser {
    Full,
    Double,
    Zhuyin(ZhuyinScheme),
    Direct(oxpinyin::facade::Law),
}

impl Parser {
    fn parse(&self, input: &[u8], options: OptionBits) -> Parse {
        match *self {
            Self::Full => parse_full(input, options),
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
            Self::Direct(law) => parse_direct(input, options, law),
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
    let parsed = tool::parse_options_or_exit(c"- test pinyin parser", &entries, args);
    let string = |index| {
        parsed
            .string(index)
            .map_or(&b""[..], std::ffi::CStr::to_bytes)
    };
    let parser = match string(0) {
        b"doublepinyin" => Parser::Double,
        b"zhuyin" => match string(2) {
            b"standard" => Parser::Zhuyin(ZhuyinScheme::Standard),
            b"hsu" => Parser::Zhuyin(ZhuyinScheme::Hsu),
            b"dachen26" => Parser::Zhuyin(ZhuyinScheme::DachenCp26),
            _ => Parser::Full,
        },
        b"pinyindirect" => Parser::Direct(oxpinyin::facade::Law::Pinyin),
        b"zhuyindirect" => Parser::Direct(oxpinyin::facade::Law::Zhuyin),
        _ => Parser::Full,
    };
    let mut options =
        OptionBits::PINYIN_CORRECT_ALL | OptionBits::USE_TONE | OptionBits::USE_RESPLIT_TABLE;
    if parsed.flag(1) {
        options |= OptionBits::PINYIN_INCOMPLETE | OptionBits::ZHUYIN_INCOMPLETE;
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

/// Reuse the C API's graph, fewest-key selection and trailing-apostrophe
/// computation (`Session::full_parsed_len`). The standalone parser does not
/// perform the later scan-matrix resplit/divide steps: the pin disables its
/// parser post-processing at `pinyin_parser2.cpp:377-379` (074a2219).
#[must_use]
pub(crate) fn parse_full(input: &[u8], options: OptionBits) -> Parse {
    let Some((input, graph)) = oxpinyin_tool_support::lookup::full_graph(input, options) else {
        return Parse::default();
    };
    let path = graph.fewest_keys(options);
    oxpinyin_tool_support::lookup::selected_parse(&input, &path)
}

#[cfg(test)]
mod tests;
