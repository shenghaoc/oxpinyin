use crate::OptionBits;
use crate::parser::{DoublePinyinParser, DoublePinyinScheme, ZhuyinParser, ZhuyinScheme};

use super::*;

fn nihao() -> ZhuyinParse {
    let parse = ZhuyinParser::with_scheme(ZhuyinScheme::Standard)
        .parse_with_options(b"su3cl3", OptionBits::USE_TONE);
    assert_eq!(parse.consumed(), 6);
    assert_eq!(parse.keys().len(), 2);
    assert_eq!(
        parse
            .keys()
            .iter()
            .map(|k| (k.start(), k.end()))
            .collect::<Vec<_>>(),
        [(0, 3), (3, 6)]
    );
    parse
}

// Rust contract: transformed offsets through a key end consume that original key.
#[test]
fn zhuyin_original_offset_answers_the_end_of_the_key_it_falls_in() {
    let parse = nihao();
    for (offset, expected) in [(1, 3), (2, 3), (3, 6), (6, 6), (40, 6)] {
        assert_eq!(zhuyin_original_offset(&parse, offset), expected);
    }
}

// Rust contract: only interior offsets before the consumed end are mid-key.
#[test]
fn zhuyin_mid_key_offsets_are_the_ones_no_key_starts_on() {
    let parse = nihao();
    for (offset, expected) in [
        (0, false),
        (1, true),
        (2, true),
        (3, false),
        (4, true),
        (5, true),
        (6, false),
        (40, false),
    ] {
        assert_eq!(zhuyin_offset_is_mid_key(&parse, offset), expected);
    }
}

// Rust contract: session_column maps the next key to its graph edge at the apostrophe.
#[test]
fn zhuyin_session_offset_is_the_next_key_column() {
    let parse = nihao();
    for (offset, expected) in [(0, 0), (2, 0), (3, 2), (5, 2), (6, 6)] {
        assert_eq!(zhuyin_session_offset(&parse, offset), expected);
    }
}

// Rust contract: lookup direction selects the edge at an original key boundary.
#[test]
fn zhuyin_lookup_offset_is_direction_dependent_at_a_boundary() {
    let parse = nihao();
    let len = "ni'hao".len();
    for (offset, before, expected) in [
        (3, false, 2),
        (3, true, 2),
        (6, false, len),
        (6, true, len),
        (1, true, len),
    ] {
        assert_eq!(
            zhuyin_lookup_session_offset(&parse, len, offset, before),
            expected
        );
    }
}

// Rust contract: double-pinyin key starts select their session matrix columns.
#[test]
fn double_pinyin_key_starts_map_to_session_columns() {
    let parse = DoublePinyinParser::with_scheme(DoublePinyinScheme::Zrm)
        .parse_with_options(b"nihk", OptionBits::empty());
    assert!(!parse.keys().is_empty());
    let mut start = 0_usize;
    for key in parse.keys() {
        assert_eq!(
            double_session_offset(&parse, key.start()),
            start.saturating_sub(1)
        );
        start += key.key().text().len() + 1;
    }
}
