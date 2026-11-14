//! Behavioural port of `libpinyin/tests/storage/test_parser2.cpp`.
//!
//! The upstream driver enumerates every parser variant (`fullpinyin`,
//! `doublepinyin`, `zhuyin` × `standard`/`hsu`/`dachen26`,
//! `pinyindirect`, `zhuyindirect`), parses sample lines, and protects
//! exactly one hard invariant: the parse produces aligned key/key-rest
//! streams (`keys->len == key_rests->len`), i.e. every key carries a
//! well-formed span of the raw input. The rest is diagnostic output.
//!
//! The same invariant, lifted to oxpinyin's public parser seam: every
//! double-pinyin layout produces ordered, contiguous, non-overlapping spans,
//! and its full-pinyin projection parses as frozen syllables. The zhuyin
//! keyboards are checked against the frozen pin in
//! `tests/zhuyin_parse_parity.rs`, and the alternate-romanization index
//! parsers in `src/full_pinyin_index.rs`. Scheme-setter rejection contracts
//! are covered black-box in `crates/oxpinyin-capi/tests/abi/contract.rs`;
//! full-pinyin selected splits are checked in `tests/graph_paths.rs`.

use oxpinyin_core::{
    DoublePinyinParser, DoublePinyinScheme, FORCE_TONE, FullPinyinScheme, OptionBits,
    PINYIN_INCOMPLETE, SyllableKey, USE_TONE, ZHUYIN_INCOMPLETE, ZhuyinParser, ZhuyinScheme,
    luoma_pinyin_index, parse_full_pinyin_index, parse_full_pinyin_index_with_options,
    parse_zhuyin_direct, secondary_zhuyin_index,
};

/// All double-pinyin layouts with a compiled table (Customized has none).
const DOUBLE_SCHEMES: &[DoublePinyinScheme] = &[
    DoublePinyinScheme::Zrm,
    DoublePinyinScheme::Ms,
    DoublePinyinScheme::Ziguang,
    DoublePinyinScheme::Abc,
    DoublePinyinScheme::Pyjj,
    DoublePinyinScheme::Xhe,
];

fn parser_accepts_spellings(joined: &str) -> bool {
    joined
        .split('\'')
        .all(|spelling| spelling.is_empty() || SyllableKey::from_text(spelling).is_some())
}

/// The `test_parser2` hard invariant: ordered, contiguous, non-overlapping
/// spans that together cover the parsed prefix of the input.
#[test]
fn every_double_pinyin_scheme_produces_aligned_key_spans() {
    let input = b"uiaaaa"; // six keystrokes; layout-independent length
    for scheme in DOUBLE_SCHEMES {
        let mut parser = DoublePinyinParser::new();
        assert!(parser.set_scheme(*scheme), "{scheme:?} has a table");
        let parse = parser.parse_with_options(input, 0);
        let mut cursor = 0;
        for key in parse.keys() {
            assert!(
                key.start() >= cursor,
                "{scheme:?}: key span {start}..{end} overlaps or regresses (cursor {cursor})",
                start = key.start(),
                end = key.end()
            );
            assert!(key.end() > key.start(), "{scheme:?}: empty key span");
            cursor = key.end();
        }
        assert!(
            cursor <= input.len(),
            "{scheme:?}: spans ran past the input"
        );
        assert!(
            parse.consumed() >= cursor,
            "{scheme:?}: consumed {n} but spans only reach {cursor}",
            n = parse.consumed()
        );
    }
}

#[test]
fn double_pinyin_output_is_valid_full_pinyin_for_every_scheme() {
    // The decoder downstream of the scheme seam consumes the parser's
    // full-pinyin projection, so it must re-parse as real syllables for
    // every layout.
    for scheme in DOUBLE_SCHEMES {
        let mut parser = DoublePinyinParser::new();
        parser.set_scheme(*scheme);
        let full = parser.parse_with_options(b"uikjkuk", 0).full_pinyin();
        assert!(
            parser_accepts_spellings(&full),
            "{scheme:?}: full-pinyin projection {full:?} is not parsable"
        );
    }
}

#[test]
fn parse_with_options_honours_force_tone_like_the_batch_parser() {
    // The pin's `zhuyin_parse_more_chewings` passes `context->m_options`
    // (`zhuyin.cpp:1061`), which `zhuyin_init` seeds `USE_TONE | FORCE_TONE`
    // (`zhuyin.cpp:273`). The batch `ZhuyinSimpleParser2::parse_one_key`
    // rejects a toneless syllable under `FORCE_TONE`
    // (`zhuyin_parser2.cpp:176-180`). `parse_with_options` is the additive
    // seam that models that law; the plain 3-arg `parse` intentionally does
    // not (it is the pinyin facade's path, never FORCE_TONE).
    let parser = ZhuyinParser::new();

    // Toned syllables parse fully (tone is required by FORCE_TONE).
    let toned = parser.parse_with_options(b"su3", USE_TONE | FORCE_TONE);
    assert_eq!(toned.consumed(), 3);
    assert_eq!(toned.keys().len(), 1);

    // Toneless syllables are rejected: a complete syllable with no tone
    // consumes 0 under FORCE_TONE, exactly as the pin does.
    for syllables in [&b"li"[..], &b"ju"[..]] {
        let untoned = parser.parse_with_options(syllables, USE_TONE | FORCE_TONE);
        assert_eq!(
            untoned.consumed(),
            0,
            "{} must consume 0 under FORCE_TONE",
            std::str::from_utf8(syllables).unwrap()
        );
    }

    // The same toneless syllables DO parse under USE_TONE alone (the pin
    // without FORCE_TONE accepts them) — verifying FORCE_TONE is the gate.
    let li = parser.parse_with_options(b"li", USE_TONE);
    assert_eq!(li.consumed(), 2);
}

#[test]
fn the_full_pinyin_scheme_slot_enumerates_its_three_romanizations() {
    // FullPinyinScheme::index mirrors the parser selection the ABI makes
    // for LUOMA/SECONDARY; the Hanyu default uses the ordinary parser and
    // has no index mapping through this seam.
    assert!(FullPinyinScheme::Hanyu.index().is_none());
    assert_eq!(FullPinyinScheme::Luoma.index(), Some(luoma_pinyin_index()));
    assert_eq!(
        FullPinyinScheme::SecondaryZhuyin.index(),
        Some(secondary_zhuyin_index())
    );
}

#[test]
fn zhuyin_direct_parse_is_the_pins_bopomofo_split() {
    let options = USE_TONE | FORCE_TONE;
    let text = |keys: Vec<(SyllableKey, u8)>| -> Vec<(&'static str, u8)> {
        keys.into_iter()
            .map(|(key, tone)| (key.text(), tone))
            .collect()
    };
    // Toned, unmarked (tone 1), and the neutral tone.
    assert_eq!(
        text(parse_zhuyin_direct("ㄘㄜˋ ㄘㄜˋ", options)),
        [("ce", 4), ("ce", 4)]
    );
    assert_eq!(
        text(parse_zhuyin_direct("ㄘㄜ'ㄘㄜ", options)),
        [("ce", 1), ("ce", 1)]
    );
    assert_eq!(
        text(parse_zhuyin_direct("ㄑㄧㄥ ㄕㄥ˙", options)),
        [("qing", 1), ("sheng", 5)]
    );
    // Runs of separators are skipped; a trailing one is harmless.
    assert_eq!(
        text(parse_zhuyin_direct("ㄋㄧˇ  ㄏㄠˇ ", options)),
        [("ni", 3), ("hao", 3)]
    );
    // Romanized input and incomplete keys parse nothing; the parse
    // stops at the first failing segment.
    assert!(parse_zhuyin_direct("ce'ce", options).is_empty());
    assert!(parse_zhuyin_direct("ㄑ ㄑ", options).is_empty());
    assert_eq!(text(parse_zhuyin_direct("ㄘ ㄑ", options)), [("ci", 1)]);
    assert_eq!(
        text(parse_zhuyin_direct("ㄘㄜˋ xx ㄘㄜˋ", options)),
        [("ce", 4)]
    );
    assert!(parse_zhuyin_direct("", options).is_empty());
}

/// `ZhuyinDirectParser2::parse` walks the input with `int` indices
/// (`zhuyin_parser2.cpp:914-958`) and has no length bound. A key that
/// starts past byte 65535 still parses.
#[test]
fn zhuyin_direct_parse_has_no_length_bound() {
    let input = format!("ㄋㄧˇ{}ㄏㄠˇ", " ".repeat(70_000));
    let keys: Vec<(&str, u8)> = parse_zhuyin_direct(&input, USE_TONE | FORCE_TONE)
        .into_iter()
        .map(|(key, tone)| (key.text(), tone))
        .collect();
    assert_eq!(keys, [("ni", 3), ("hao", 3)]);
}

#[test]
fn default_scheme_is_ms() {
    assert_eq!(DoublePinyinParser::new().scheme(), DoublePinyinScheme::Ms);
}

/// `DoublePinyinParser2::parse_one_key` on the MS tables: a two-key
/// spelling resolves to its syllable with tone zero, a three-key
/// input carries the tone only under `USE_TONE`, and `FORCE_TONE`
/// demands exactly three bytes.
#[test]
fn double_one_key_law() {
    let parser = DoublePinyinParser::with_scheme(DoublePinyinScheme::Ms);

    let key = parser.parse_one_key(0, b"ni").expect("ni parses");
    assert_eq!(key.pinyin_string(), "ni");
    assert_eq!(key.tone, 0);
    assert_eq!(key.shengmu_string(), "n");
    assert_eq!(key.yunmu_string(), "i");

    // Without USE_TONE the three-byte form is refused.
    assert_eq!(parser.parse_one_key(0, b"ni3"), None);
    let toned = parser.parse_one_key(USE_TONE, b"ni3").expect("toned");
    assert_eq!(toned.pinyin_string(), "ni3");

    // FORCE_TONE: two bytes rejected; a three-byte input already
    // carries its tone digit, so it passes with the key toned (the
    // pin's zero-tone check at `pinyin_parser2.cpp:448` is dead —
    // the digit parse above already refused every non-tone byte).
    assert_eq!(parser.parse_one_key(FORCE_TONE, b"ni"), None);
    assert!(
        parser
            .parse_one_key(USE_TONE | FORCE_TONE, b"ni3")
            .is_some()
    );
    // A non-tone third byte is refused with or without FORCE_TONE.
    assert_eq!(parser.parse_one_key(USE_TONE, b"ni9"), None);
    assert_eq!(parser.parse_one_key(USE_TONE | FORCE_TONE, b"ni9"), None);

    // A four-byte input is refused outright.
    assert_eq!(parser.parse_one_key(0, b"niha"), None);
}

/// The double one-key incomplete probe under `PINYIN_INCOMPLETE`
/// (`z` → the initial-only `z` key on MS), refused without the bit.
#[test]
fn double_one_key_incomplete_needs_its_bit() {
    let parser = DoublePinyinParser::with_scheme(DoublePinyinScheme::Ms);
    assert_eq!(parser.parse_one_key(0, b"z"), None);
    let key = parser
        .parse_one_key(PINYIN_INCOMPLETE, b"z")
        .expect("incomplete z");
    assert_eq!(key.pinyin_string(), "z");
    assert_eq!(key.yunmu_string(), "");
}

/// The double one-key second yunmu candidate: MS `oo` enters through
/// the zero shengmu (`'` → empty) and lands on `o`, the second of
/// `o`'s two yunmu rows (`uo` misses first).
#[test]
fn double_one_key_tries_the_second_yunmu() {
    let parser = DoublePinyinParser::with_scheme(DoublePinyinScheme::Ms);
    let key = parser.parse_one_key(0, b"oo").expect("oo parses");
    assert_eq!(key.pinyin_string(), "o");
}

/// `ZhuyinSimpleParser2::parse_one_key` on STANDARD: `18` is ㄅㄚ
/// (ba), the trailing `3` is tone 3 under `USE_TONE`, and
/// `FORCE_TONE` rejects the toneless form while accepting the toned
/// one.
#[test]
fn chewing_one_key_law_on_standard() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::Standard);

    let key = parser.parse_one_key(0, b"18").expect("ba parses");
    assert_eq!(key.pinyin_string(), "ba");
    assert_eq!(key.shengmu_string(), "b");
    assert_eq!(key.yunmu_string(), "a");
    assert_eq!(key.zhuyin_string(), "ㄅㄚ");

    // `USE_TONE` alone does not demand a tone: `8` is not a tone
    // key on STANDARD, nothing is stripped, and the match carries
    // tone zero. `FORCE_TONE` is what rejects it.
    let toneless = parser.parse_one_key(USE_TONE, b"18").expect("ba");
    assert_eq!(toneless.pinyin_string(), "ba");
    assert_eq!(toneless.tone, 0);
    let toned = parser.parse_one_key(USE_TONE, b"183").expect("ba3");
    assert_eq!(toned.pinyin_string(), "ba3");
    assert_eq!(toned.zhuyin_string(), "ㄅㄚˇ");

    assert_eq!(parser.parse_one_key(USE_TONE | FORCE_TONE, b"18"), None);
    assert!(
        parser
            .parse_one_key(USE_TONE | FORCE_TONE, b"183")
            .is_some()
    );
}

/// The chewing incomplete row gate: ㄅ alone resolves to the
/// initial-only `b` key only under `ZHUYIN_INCOMPLETE`.
#[test]
fn chewing_one_key_incomplete_needs_its_bit() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::Standard);
    assert_eq!(parser.parse_one_key(0, b"1"), None);
    let key = parser
        .parse_one_key(ZHUYIN_INCOMPLETE, b"1")
        .expect("incomplete b");
    assert_eq!(key.pinyin_string(), "b");
}

/// Empty input is refused on every keyboard family (upstream reads
/// `str[-1]` under `USE_TONE` on the Simple law, a read outside the
/// string; this function refuses instead), and the `StandardDvorak` slot parses
/// nothing.
#[test]
fn chewing_one_key_refuses_empty_and_dvorak() {
    let standard = ZhuyinParser::with_scheme(ZhuyinScheme::Standard);
    assert_eq!(standard.parse_one_key(USE_TONE, b""), None);
    assert_eq!(standard.parse_one_key(0, b""), None);
    let dvorak = ZhuyinParser::with_scheme(ZhuyinScheme::StandardDvorak);
    assert_eq!(dvorak.parse_one_key(0, b"18"), None);
}

#[test]
fn ms_parses_common_spellings() {
    let parser = DoublePinyinParser::new();
    let parsed = parser.parse_with_options(b"nihao", PINYIN_INCOMPLETE);
    assert_eq!(parsed.consumed(), 4);
    assert_eq!(parsed.full_pinyin(), "ni'ha");
}

/// The batch law that `docs/findings/double-pinyin-spec.md` (Tone) states:
/// `USE_TONE` lets a three-byte key carry its `1`..=`5` digit as the
/// tone, and `FORCE_TONE` requires an exactly-three-byte key — so
/// `FORCE_TONE` without `USE_TONE` consumes nothing at all.
#[test]
fn batch_parse_honours_use_tone_and_force_tone() {
    let parser = DoublePinyinParser::new();

    // USE_TONE: the three-byte key rides its tone.
    let toned = parser.parse_with_options(b"ni3", USE_TONE);
    assert_eq!(toned.consumed(), 3);
    assert_eq!(toned.keys().len(), 1);
    assert_eq!(toned.keys()[0].key().text(), "ni");
    assert_eq!(toned.keys()[0].tone(), 3);

    // Without USE_TONE the three-byte key is refused and the walk
    // retries length two.
    let toneless = parser.parse_with_options(b"ni3", 0);
    assert_eq!(toneless.consumed(), 2);
    assert_eq!(toneless.keys()[0].tone(), 0);

    // A non-tone third byte is refused even under USE_TONE.
    assert_eq!(parser.parse_with_options(b"nix", USE_TONE).consumed(), 2);
    assert_eq!(parser.parse_with_options(b"ni0", USE_TONE).consumed(), 2);

    // FORCE_TONE: a two-byte key is refused.
    assert_eq!(parser.parse_with_options(b"ni", FORCE_TONE).consumed(), 0);

    // FORCE_TONE without USE_TONE: the three-byte key passes the
    // length gate but the tone parse refuses it — nothing consumed.
    assert_eq!(parser.parse_with_options(b"ni3", FORCE_TONE).consumed(), 0);

    // USE_TONE | FORCE_TONE: only an exactly-three-byte toned key.
    let forced = parser.parse_with_options(b"ni3", USE_TONE | FORCE_TONE);
    assert_eq!(forced.consumed(), 3);
    assert_eq!(forced.keys()[0].tone(), 3);
    assert_eq!(
        parser
            .parse_with_options(b"ni3ha4", USE_TONE | FORCE_TONE)
            .consumed(),
        6
    );
}

#[test]
fn illegal_first_tone_on_ni_stops_without_retrying_a_shorter_key() {
    let parser = ZhuyinParser::new();
    let rejected = parser.parse_with_options(b"su ", USE_TONE);
    assert_eq!(rejected.consumed(), 0);
    assert!(rejected.keys().is_empty());

    let legal = parser.parse_with_options(b"su6", USE_TONE);
    assert_eq!(legal.consumed(), 3);
    assert_eq!(
        legal
            .keys()
            .iter()
            .map(|key| key.key().text())
            .collect::<Vec<_>>(),
        ["ni"]
    );
    assert_eq!(legal.keys()[0].tone(), 2);

    let untoned = parser.parse_with_options(b"su", USE_TONE);
    assert_eq!(untoned.consumed(), 2);
    assert_eq!(
        untoned
            .keys()
            .iter()
            .map(|key| key.key().text())
            .collect::<Vec<_>>(),
        ["ni"]
    );
    assert_eq!(untoned.keys()[0].tone(), 0);
}

#[test]
fn tone_after_an_invalid_syllable_does_not_extend_a_valid_prefix() {
    let parser = ZhuyinParser::new();
    let parsed = parser.parse_with_options(b"sux6", USE_TONE);
    assert_eq!(parsed.consumed(), 2);
    assert_eq!(
        parsed
            .keys()
            .iter()
            .map(|key| key.key().text())
            .collect::<Vec<_>>(),
        ["ni"]
    );
}

#[test]
fn a_tone_key_alone_is_not_a_syllable() {
    let parser = ZhuyinParser::new();
    assert_eq!(parser.parse_with_options(b"6", USE_TONE).consumed(), 0);
    assert_eq!(parser.parse_with_options(b" ", USE_TONE).consumed(), 0);
}

#[test]
fn incomplete_keys_consume_zero_either_way() {
    let parser = ZhuyinParser::new();
    // With the option off the row is gated at the index; with it on
    // the row matches and the post-match validity mask rejects it.
    // Both consume 0 at this pin — same outcome as upstream.
    assert_eq!(parser.parse_with_options(b"1", USE_TONE).consumed(), 0);
    assert_eq!(
        parser
            .parse_with_options(b"1", USE_TONE | ZHUYIN_INCOMPLETE)
            .consumed(),
        0
    );
    assert_eq!(parser.parse_with_options(b"x", USE_TONE).consumed(), 0);
    assert_eq!(
        parser
            .parse_with_options(b"x", USE_TONE | ZHUYIN_INCOMPLETE)
            .consumed(),
        0
    );
}

/// `with_fallback_suppressed` reproduces upstream's half-mutation
/// state: the scheme's shengmu/yunmu tables are active but the
/// fallback table is null.
#[test]
fn with_fallback_suppressed_clears_only_the_fallback() {
    let zrm = DoublePinyinParser::with_scheme(DoublePinyinScheme::Zrm);
    // ZRM "aa" goes through the fallback table.
    assert_eq!(zrm.parse_with_options(b"aa", 0).consumed(), 2);
    // ZRM "ni" (n+i) goes through shengmu/yunmu tables.
    assert_eq!(zrm.parse_with_options(b"ni", 0).consumed(), 2);

    let suppressed = zrm.with_fallback_suppressed();
    // Fallback is gone — "aa" no longer resolves.
    assert_eq!(suppressed.parse_with_options(b"aa", 0).consumed(), 0);
    // Shengmu/yunmu are intact — "ni" still works.
    assert_eq!(suppressed.parse_with_options(b"ni", 0).consumed(), 2);

    // set_scheme clears the suppressed flag.
    let mut parser = suppressed;
    assert!(parser.set_scheme(DoublePinyinScheme::Zrm));
    assert_eq!(parser.parse_with_options(b"aa", 0).consumed(), 2);
}

#[test]
fn luoma_parses_through_the_dp() {
    let parse =
        |input: &str| parse_full_pinyin_index(input.as_bytes(), false, luoma_pinyin_index());

    // Longest coverage wins; the canonical spelling is what the
    // segment maps to, spans stay raw.
    let nihao = parse("nihao");
    assert_eq!(nihao.consumed(), 5);
    assert_eq!(nihao.full_pinyin(), "ni'hao");

    let chiang = parse("chiang");
    assert_eq!(chiang.consumed(), 6);
    assert_eq!(chiang.full_pinyin(), "qiang");
    assert_eq!(chiang.keys()[0].start(), 0);
    assert_eq!(chiang.keys()[0].end(), 6);

    // Apostrophes separate keys and are consumed.
    let split = parse("ni'hao");
    assert_eq!(split.consumed(), 6);
    assert_eq!(split.full_pinyin(), "ni'hao");

    // Junk stops the parse: parsed length covers only the prefix
    // that resolves.
    let junk = parse("chiangQ");
    assert_eq!(junk.consumed(), 6);

    // A spelling only pinyin_index knows parses nothing here; the
    // luoma spelling of the ㄓ series targets the initial-only row
    // ("jhih" -> zh), and the full zhi syllable has no spelling at
    // all under this scheme.
    assert_eq!(parse("zhi").consumed(), 0);
    assert_eq!(parse("jhih").full_pinyin(), "zh");
}

#[test]
fn index_parse_handles_tone_digits() {
    let parse = |input: &str, use_tone| {
        parse_full_pinyin_index(input.as_bytes(), use_tone, secondary_zhuyin_index())
    };

    // Under USE_TONE the trailing digit rides the key (secondary
    // spells zhu as "ju").
    let toned = parse("ju4", true);
    assert_eq!(toned.consumed(), 3);
    assert_eq!(toned.full_pinyin(), "zhu");
    assert_eq!(toned.keys()[0].tone(), 4);
    assert_eq!(toned.keys()[0].end(), 3);

    // Without USE_TONE the digit is junk: only the "ju" spelling
    // consumes.
    let untoned = parse("ju4", false);
    assert_eq!(untoned.consumed(), 2);
    assert_eq!(untoned.full_pinyin(), "zhu");

    // A tone digit alone parses nothing.
    assert_eq!(parse("3", true).consumed(), 0);
}

#[test]
fn dp_prefers_longest_coverage_then_fewest_keys() {
    // "xian" style ambiguity through the secondary index: "shian"
    // covers 5 bytes as one key; "shi"->xi + "an" also covers 5 as
    // two keys — fewest keys wins, so the single long key parses.
    let parse =
        |input: &str| parse_full_pinyin_index(input.as_bytes(), false, secondary_zhuyin_index());
    assert_eq!(parse("shian").full_pinyin(), "xian");

    // An initial-only target parses with no option bit: these
    // indexes have no gated rows (secondary spells ㄘ-only "tsz").
    assert_eq!(parse("tsz").full_pinyin(), "c");
}

#[test]
fn force_tone_is_nested_under_use_tone_for_both_indexes() {
    for (index, input) in [
        (luoma_pinyin_index(), "shih"),
        (secondary_zhuyin_index(), "tsz"),
    ] {
        for bits in [0, FORCE_TONE, USE_TONE, USE_TONE | FORCE_TONE] {
            let options = OptionBits::from_bits(bits);
            let plain = parse_full_pinyin_index_with_options(input.as_bytes(), options, index)
                .expect("infallible parse");
            assert_eq!(
                plain.consumed(),
                if bits == USE_TONE | FORCE_TONE {
                    0
                } else {
                    input.len()
                }
            );
            let toned = format!("{input}4");
            let parsed = parse_full_pinyin_index_with_options(toned.as_bytes(), options, index)
                .expect("infallible parse");
            assert_eq!(
                parsed.consumed(),
                if bits & USE_TONE != 0 {
                    toned.len()
                } else {
                    input.len()
                }
            );
            assert_eq!(
                parsed.keys()[0].tone(),
                if bits & USE_TONE != 0 { 4 } else { 0 }
            );
        }
    }
}
