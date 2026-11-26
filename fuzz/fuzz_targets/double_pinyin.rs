//! Every compiled double-pinyin scheme over arbitrary byte input: parse
//! must be total and deterministic.

use oxpinyin::OptionBits;
use oxpinyin::parser::{DoublePinyinParser, DoublePinyinScheme};

/// Inputs per child: AFL++'s suggested start
/// (`instrumentation/README.persistent_mode.md`), which bounds a child's
/// lifetime. `AFL_FUZZER_LOOPCOUNT` overrides it.
const LOOP_COUNT: usize = 1000;

fn main() {
    oxpinyin_fuzz::fuzz(LOOP_COUNT, &[], run);
}

/// Runs one input.
fn run(data: &[u8]) {
    let allow_incomplete = data.first().is_some_and(|byte| byte & 1 == 1);
    let mut options = OptionBits::empty();
    options.set(OptionBits::PINYIN_INCOMPLETE, allow_incomplete);
    let schemes = [
        DoublePinyinScheme::Zrm,
        DoublePinyinScheme::Ms,
        DoublePinyinScheme::Ziguang,
        DoublePinyinScheme::Abc,
        DoublePinyinScheme::Pyjj,
        DoublePinyinScheme::Xhe,
        DoublePinyinScheme::Customized,
    ];
    for scheme in schemes {
        let parser = DoublePinyinParser::with_scheme(scheme);
        let first = parser.parse_with_options(data, options);
        let second = parser.parse_with_options(data, options);
        assert_eq!(first, second, "parse must be deterministic");
        assert_eq!(
            first.consumed(),
            second.consumed(),
            "consumed length must be deterministic"
        );
    }
}
