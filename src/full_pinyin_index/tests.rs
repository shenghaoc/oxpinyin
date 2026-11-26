use super::{luoma_pinyin_index, parse_full_pinyin_index_with_options, secondary_zhuyin_index};
use crate::OptionBits;

// Frozen pinyin_parser_table.h:674-1490 at 074a2219 supplies both spelling indexes.
#[test]
fn luoma_pinyin_index_matches_original_rows() {
    for (index, rows) in [
        (
            luoma_pinyin_index(),
            super::reference::LUOMA_PINYIN_INDEX.as_slice(),
        ),
        (
            secondary_zhuyin_index(),
            super::reference::SECONDARY_ZHUYIN_INDEX.as_slice(),
        ),
    ] {
        for &(input, expected) in rows {
            assert_eq!(index.canonical(input), Some(expected), "{input:?}");
        }
    }
}

fn canonical(parse: &crate::parser::FullPinyinIndexParse) -> String {
    parse
        .keys()
        .iter()
        .map(|key| key.canonical())
        .collect::<Vec<_>>()
        .join("'")
}

// Pin pinyin_parser2.cpp:217-310,346-380 at 074a2219 selects the longest reachable prefix.
#[test]
fn luoma_parses_through_the_dp() {
    for (input, consumed, expected) in [
        ("nihao", 5, Some("ni'hao")),
        ("chiang", 6, Some("qiang")),
        ("chiangQ", 6, None),
        ("zhi", 0, None),
    ] {
        let parsed = parse_full_pinyin_index_with_options(
            input.as_bytes(),
            OptionBits::empty(),
            luoma_pinyin_index(),
        );
        assert_eq!(parsed.consumed(), consumed, "{input:?}");
        if let Some(expected) = expected {
            assert_eq!(canonical(&parsed), expected, "{input:?}");
        }
        if input == "chiang" {
            assert_eq!((parsed.keys()[0].start(), parsed.keys()[0].end()), (0, 6));
        }
    }
}

// Pin pinyin_parser2.cpp:233,237-269 at 074a2219 bounds each key at its run's next separator.
#[test]
fn index_parse_bounds_each_key_by_its_runs_next_separator() {
    for (input, expected) in [
        ("ni'hao''chiang'", [(0, 2), (3, 6), (8, 14)]),
        ("chiang'ni'hao", [(0, 6), (7, 9), (10, 13)]),
    ] {
        let parsed = parse_full_pinyin_index_with_options(
            input.as_bytes(),
            OptionBits::empty(),
            luoma_pinyin_index(),
        );
        let spans: Vec<_> = parsed
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect();
        assert_eq!(spans, expected, "{input:?}");
        if input == "ni'hao''chiang'" {
            assert_eq!(parsed.consumed(), input.len());
            assert_eq!(canonical(&parsed), "ni'hao'qiang");
        }
    }
}

// Pin pinyin_parser2.cpp:291-310 at 074a2219 prefers fewer keys; alternate-index rows have no gates.
#[test]
fn dp_prefers_longest_coverage_then_fewest_keys() {
    for (input, expected) in [("shian", "xian"), ("tsz", "c")] {
        let parsed = parse_full_pinyin_index_with_options(
            input.as_bytes(),
            OptionBits::empty(),
            secondary_zhuyin_index(),
        );
        assert_eq!(canonical(&parsed), expected, "{input:?}");
    }
}

// Pin pinyin_parser2.cpp:176-190,205-210 at 074a2219 reads FORCE_TONE only under USE_TONE.
#[test]
fn force_tone_is_nested_under_use_tone() {
    let use_tone = OptionBits::USE_TONE;
    let force_tone = OptionBits::FORCE_TONE;
    for (input, options, consumed, tone) in [
        ("tsz", OptionBits::empty(), 3, None),
        ("tsz4", OptionBits::empty(), 3, Some(0)),
        ("tsz", force_tone, 3, None),
        ("tsz4", force_tone, 3, Some(0)),
        ("tsz", use_tone, 3, None),
        ("tsz4", use_tone, 4, Some(4)),
        ("tsz", use_tone | force_tone, 0, None),
        ("tsz4", use_tone | force_tone, 4, Some(4)),
        ("3", use_tone, 0, None),
    ] {
        let parsed = parse_full_pinyin_index_with_options(
            input.as_bytes(),
            options,
            secondary_zhuyin_index(),
        );
        assert_eq!(parsed.consumed(), consumed, "{input:?}, {options:?}");
        if let Some(tone) = tone {
            assert_eq!(parsed.keys()[0].tone(), tone, "{input:?}, {options:?}");
        }
    }
}
