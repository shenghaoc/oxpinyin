#![no_main]

use libfuzzer_sys::fuzz_target;
use oxpinyin_core::{FullPinyinParser, OptionBits, ParseError, MAX_PARSE_RESULTS};

fuzz_target!(|data: &[u8]| {
    let parser = FullPinyinParser;
    let first = parser.parse_with_options(data, OptionBits::default());
    let second = parser.parse_with_options(data, OptionBits::default());

    assert_eq!(first, second, "parser output must be deterministic");

    if let Err(ParseError::TooManyAlternatives { limit }) = first {
        assert_eq!(limit, MAX_PARSE_RESULTS);
    }
});
