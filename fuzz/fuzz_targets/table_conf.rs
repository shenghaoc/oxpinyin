#![no_main]
//! Hostile text through the `table.conf` λ parser — the one system file
//! read as text at `pinyin_init`. `parse_table_conf_lambda` must answer
//! on every input, never panic on odd decimals, and be deterministic;
//! the decoder falls back to the pinned λ on `None`, so a rejected line
//! is a legitimate answer, not a failure.

use libfuzzer_sys::fuzz_target;
use oxpinyin_data::parse_table_conf_lambda;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let first = parse_table_conf_lambda(text);
    let second = parse_table_conf_lambda(text);
    assert_eq!(first, second, "parse must be deterministic");
    if let Some(lambda) = first {
        // A parsed λ is a rational in [0, 1]; the accessors are total.
        let value = lambda.as_f64();
        assert!((0.0..=1.0).contains(&value), "λ out of range: {value}");
    }
});
