#![no_main]

use libfuzzer_sys::fuzz_target;
use pinyin_core::{FullPinyinParser, InputParser, ParseError, MAX_PARSE_RESULTS};

fuzz_target!(|data: &[u8]| {
    let parser = FullPinyinParser;
    let first = parser.parse(data);
    let second = parser.parse(data);

    assert_eq!(first, second, "parser output must be deterministic");

    if let Err(ParseError::TooManyAlternatives { limit }) = first {
        assert_eq!(limit, MAX_PARSE_RESULTS);
    }
});
