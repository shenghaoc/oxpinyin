//! Every compiled double-pinyin scheme over arbitrary byte input: parse
//! must be total and deterministic. Also pins the setter contract shared
//! with the C ABI (contract_tests.rs): `Customized` is rejected and leaves
//! the effective scheme unchanged.

use oxpinyin::OptionBits;
use oxpinyin::parser::{DoublePinyinParser, DoublePinyinScheme};

fn main() {
    oxpinyin_fuzz::run_once(run);
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

    // Setter contract: Customized is rejected with state preserved.
    let mut parser = DoublePinyinParser::new();
    let before = parser.scheme();
    assert!(!parser.set_scheme(DoublePinyinScheme::Customized));
    assert_eq!(parser.scheme(), before);
}
