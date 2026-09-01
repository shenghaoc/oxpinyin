#![no_main]
//! Hostile fixture text through the fixture dictionary/language-model
//! parsers — the ingress of audit F-1/F-2 (`docs/safety/oxpinyin-audit.md`):
//! counts near `u64::MAX` must saturate (not wrap) and parsing must be
//! total and deterministic on every byte sequence.

use libfuzzer_sys::fuzz_target;
use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

fuzz_target!(|data: &[u8]| {
    // Any prefix is the vocab half; the remainder is the bigram half.
    let cut = data.len() / 2;
    let (Ok(vocab), Ok(bigrams)) = (
        std::str::from_utf8(&data[..cut]),
        std::str::from_utf8(&data[cut..]),
    ) else {
        return;
    };

    let dict_a = FixtureDictionary::parse(vocab);
    let dict_b = FixtureDictionary::parse(vocab);
    assert_eq!(
        dict_a.is_ok(),
        dict_b.is_ok(),
        "parse must be deterministic"
    );

    let model_a = FixtureLanguageModel::parse(vocab, bigrams);
    let model_b = FixtureLanguageModel::parse(vocab, bigrams);
    assert_eq!(
        model_a.is_ok(),
        model_b.is_ok(),
        "model parse must be deterministic"
    );
});
