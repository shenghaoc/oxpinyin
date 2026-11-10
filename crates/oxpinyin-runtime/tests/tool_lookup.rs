//! Real-model witnesses migrated from the nonshipping upstream lookup driver.
//! The manifest requires the `tools` feature for this target.

use oxpinyin_core::{PINYIN_CORRECT_ALL, PINYIN_INCOMPLETE, USE_TONE};
use oxpinyin_data::table_info::TableConf;
use oxpinyin_engine::tool::prepare_lookup;
use oxpinyin_runtime::tool::Lookup;
use std::path::PathBuf;

/// 074a2219 tests/lookup/test_pinyin_lookup.cpp:84-111 passes the explicit
/// parsed length through fill_matrix into n-best lookup. The frozen six- and
/// seven-byte cases retain that terminal column after g_strndup zero padding.
/// In `ni'\0hao` the key ends at 2 and the parsed length is 3: a bound taken
/// from the key span shortens the row, and without the physical separator
/// column no row reaches the bound.
#[test]
fn embedded_nul_lookup_keeps_the_original_terminal_column() {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT);
    let conf_path = oxpinyin_input::c_path(&data.join("table.conf")).unwrap();
    let conf = TableConf::load(&conf_path).unwrap();
    let lookup = Lookup::open(&data, &conf, true, true).unwrap_or_else(|_| panic!("W3 lookup"));
    for (input, key_end, consumed) in [
        (b"ni\0hao".as_slice(), 6, 6),
        (b"ni\0'hao".as_slice(), 7, 7),
        (b"ni'\0hao".as_slice(), 2, 3),
    ] {
        for options in [
            PINYIN_INCOMPLETE,
            USE_TONE | PINYIN_CORRECT_ALL | PINYIN_INCOMPLETE,
        ] {
            let prepared = prepare_lookup(input, options).unwrap();
            assert_eq!(prepared.parsed.consumed, consumed);
            assert_eq!(prepared.parsed.keys.len(), 1);
            assert_eq!(prepared.parsed.keys[0].key.pinyin_string(), "ni");
            assert_eq!(prepared.parsed.keys[0].span, 0..key_end);
            let rows = lookup.phonetic(&prepared).unwrap();
            assert_eq!(rows.len(), 3);
            for (row, (token, text)) in
                rows.iter()
                    .zip([(16_781_861, "你"), (16_781_858, "尼"), (16_781_884, "泥")])
            {
                assert_eq!(row.span, consumed);
                assert_eq!(row.tokens, [(0, token)]);
                assert_eq!(row.text.as_str(), Some(text));
            }
        }
    }
}
