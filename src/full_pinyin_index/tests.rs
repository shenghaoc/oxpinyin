use super::{
    LUOMA_PINYIN_COUNT, SECONDARY_ZHUYIN_COUNT, luoma_pinyin_index, secondary_zhuyin_index,
};
use crate::OptionBits;
use crate::full_pinyin_index::parse_full_pinyin_index_with_options;

#[test]
fn indexes_match_the_pinned_shapes() {
    for (index, count) in [
        (luoma_pinyin_index(), LUOMA_PINYIN_COUNT),
        (secondary_zhuyin_index(), SECONDARY_ZHUYIN_COUNT),
    ] {
        assert_eq!(index.entries().len(), count);
        assert_eq!(count, 406);
        for pair in index.entries().collect::<Vec<_>>().windows(2) {
            assert!(pair[0].0 < pair[1].0, "index not strictly sorted");
        }
        // Every target is a canonical key: the 440-id inventory
        // covers both indexes with nothing to spare or add.
        for (spelling, canonical) in index.entries() {
            assert!(
                crate::vocab::SyllableKey::from_canonical_text(canonical).is_some(),
                "{spelling} -> {canonical} leaves the inventory"
            );
        }
    }

    // The pinned remaps: shared spellings mean different syllables
    // per scheme.
    let luoma = |spelling: &str| {
        luoma_pinyin_index()
            .entries()
            .find(|row| row.0 == spelling)
            .map(|row| row.1)
    };
    let secondary = |spelling: &str| {
        secondary_zhuyin_index()
            .entries()
            .find(|row| row.0 == spelling)
            .map(|row| row.1)
    };
    assert_eq!(luoma("chi"), Some("qi"));
    assert_eq!(luoma("si"), Some("xi"));
    assert_eq!(secondary("chi"), Some("qi"));
    assert_eq!(secondary("ju"), Some("zhu"));
    assert_eq!(secondary("liu"), Some("lv"));
    // And scheme-specific spellings absent from pinyin_index.
    assert_eq!(luoma("chyueh"), Some("que"));
    assert_eq!(secondary("au"), Some("ao"));
}

#[test]
fn luoma_pinyin_index_matches_original_rows() {
    assert_eq!(
        super::LUOMA_PINYIN_COUNT,
        super::reference::LUOMA_PINYIN_COUNT
    );
    assert!(
        super::luoma_pinyin_index()
            .entries()
            .eq(super::reference::LUOMA_PINYIN_INDEX)
    );
}

#[test]
fn secondary_zhuyin_index_matches_original_rows() {
    assert_eq!(
        super::SECONDARY_ZHUYIN_COUNT,
        super::reference::SECONDARY_ZHUYIN_COUNT
    );
    assert!(
        super::secondary_zhuyin_index()
            .entries()
            .eq(super::reference::SECONDARY_ZHUYIN_INDEX)
    );
}

/// An index parse's keys as `'`-joined canonical spellings.
fn canonical(parse: &crate::parser::FullPinyinIndexParse) -> String {
    parse
        .keys()
        .iter()
        .map(|key| key.canonical())
        .collect::<Vec<_>>()
        .join("'")
}

#[test]
fn luoma_parses_through_the_dp() {
    let parse = |input: &str| {
        parse_full_pinyin_index_with_options(
            input.as_bytes(),
            OptionBits::empty(),
            luoma_pinyin_index(),
        )
    };

    // Longest coverage wins; the canonical spelling is what the
    // segment maps to, spans stay raw.
    let nihao = parse("nihao");
    assert_eq!(nihao.consumed(), 5);
    assert_eq!(canonical(&nihao), "ni'hao");

    let chiang = parse("chiang");
    assert_eq!(chiang.consumed(), 6);
    assert_eq!(canonical(&chiang), "qiang");
    assert_eq!(chiang.keys()[0].start(), 0);
    assert_eq!(chiang.keys()[0].end(), 6);

    // Apostrophes separate keys and are consumed.
    let split = parse("ni'hao");
    assert_eq!(split.consumed(), 6);
    assert_eq!(canonical(&split), "ni'hao");

    // Junk stops the parse: parsed length covers only the prefix
    // that resolves.
    let junk = parse("chiangQ");
    assert_eq!(junk.consumed(), 6);

    // A spelling only pinyin_index knows parses nothing here; the
    // luoma spelling of the ㄓ series targets the initial-only row
    // ("jhih" -> zh), and the full zhi syllable has no spelling at
    // all under this scheme.
    assert_eq!(parse("zhi").consumed(), 0);
    assert_eq!(canonical(&parse("jhih")), "zh");
}

#[test]
fn index_parse_handles_tone_digits() {
    let parse = |input: &str, use_tone| {
        parse_full_pinyin_index_with_options(
            input.as_bytes(),
            if use_tone {
                OptionBits::USE_TONE
            } else {
                OptionBits::empty()
            },
            secondary_zhuyin_index(),
        )
    };

    // Under USE_TONE the trailing digit rides the key (secondary
    // spells zhu as "ju").
    let toned = parse("ju4", true);
    assert_eq!(toned.consumed(), 3);
    assert_eq!(canonical(&toned), "zhu");
    assert_eq!(toned.keys()[0].tone(), 4);
    assert_eq!(toned.keys()[0].end(), 3);

    // Without USE_TONE the digit is junk: only the "ju" spelling
    // consumes.
    let untoned = parse("ju4", false);
    assert_eq!(untoned.consumed(), 2);
    assert_eq!(canonical(&untoned), "zhu");

    // A tone digit alone parses nothing.
    assert_eq!(parse("3", true).consumed(), 0);
}

/// Each key is bounded by the next separator of its own run
/// (`pinyin_parser2.cpp:253-269` at 074a2219): a key never spans a
/// separator, every separator is consumed, and a later run is bounded by
/// its own end, not by an earlier run's.
#[test]
fn index_parse_bounds_each_key_by_its_runs_next_separator() {
    let parse = |input: &str| {
        parse_full_pinyin_index_with_options(
            input.as_bytes(),
            OptionBits::empty(),
            luoma_pinyin_index(),
        )
    };
    let spans = |input: &str| {
        parse(input)
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect::<Vec<_>>()
    };

    // Three runs, a doubled separator and a trailing one.
    let runs = "ni'hao''chiang'";
    assert_eq!(spans(runs), [(0, 2), (3, 6), (8, 14)]);
    assert_eq!(parse(runs).consumed(), runs.len());
    assert_eq!(canonical(&parse(runs)), "ni'hao'qiang");
    // A short run after a long one: `ni` ends at its own separator.
    assert_eq!(spans("chiang'ni'hao"), [(0, 6), (7, 9), (10, 13)]);

    // A long input with no separator parses as its unit does, repeated.
    let unit = parse("nihao");
    let long = "nihao".repeat(200);
    let parsed = parse(&long);
    assert_eq!(parsed.consumed(), long.len());
    assert_eq!(parsed.keys().len(), 200 * unit.keys().len());
    assert_eq!(canonical(&parsed), vec![canonical(&unit); 200].join("'"));
}

#[test]
fn dp_prefers_longest_coverage_then_fewest_keys() {
    // "xian" style ambiguity through the secondary index: "shian"
    // covers 5 bytes as one key; "shi"->xi + "an" also covers 5 as
    // two keys — fewest keys wins, so the single long key parses.
    let parse = |input: &str| {
        parse_full_pinyin_index_with_options(
            input.as_bytes(),
            OptionBits::empty(),
            secondary_zhuyin_index(),
        )
    };
    assert_eq!(canonical(&parse("shian")), "xian");

    // An initial-only target parses with no option bit: these
    // indexes have no gated rows (secondary spells ㄘ-only "tsz").
    assert_eq!(canonical(&parse("tsz")), "c");
}

#[test]
fn force_tone_is_nested_under_use_tone_for_both_indexes() {
    for (index, input) in [
        (luoma_pinyin_index(), "shih"),
        (secondary_zhuyin_index(), "tsz"),
    ] {
        for options in [
            OptionBits::empty(),
            OptionBits::FORCE_TONE,
            OptionBits::USE_TONE,
            OptionBits::USE_TONE | OptionBits::FORCE_TONE,
        ] {
            let plain = parse_full_pinyin_index_with_options(input.as_bytes(), options, index);
            assert_eq!(
                plain.consumed(),
                if options == OptionBits::USE_TONE | OptionBits::FORCE_TONE {
                    0
                } else {
                    input.len()
                }
            );
            let toned = format!("{input}4");
            let parsed = parse_full_pinyin_index_with_options(toned.as_bytes(), options, index);
            assert_eq!(
                parsed.consumed(),
                if options.contains(OptionBits::USE_TONE) {
                    toned.len()
                } else {
                    input.len()
                }
            );
            assert_eq!(
                parsed.keys()[0].tone(),
                if options.contains(OptionBits::USE_TONE) {
                    4
                } else {
                    0
                }
            );
        }
    }
}
