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
//! scheme parser produces ordered, contiguous, non-overlapping spans, and
//! the canonical spellings they emit are real entries of the frozen
//! inventories. The alternate-romanization index parsers (`pinyindirect`/
//! `zhuyindirect` upstream) are exercised through
//! `parse_full_pinyin_index` over the pinned Luoma and Secondary Zhuyin
//! indexes. Scheme-setter rejection contracts are covered black-box in
//! `crates/oxpinyin-capi/tests/abi/contract.rs`; full-pinyin parse
//! invariants live in `tests/parser_acceptance.rs`.

use oxpinyin_core::{
    DoublePinyinParser, DoublePinyinScheme, FullPinyinParser, FullPinyinScheme, InputParser,
    LUOMA_PINYIN_INDEX, SECONDARY_ZHUYIN_INDEX, SyllableKey, ZhuyinParser, ZhuyinScheme,
    parse_full_pinyin_index,
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

/// Every zhuyin keyboard except the `StandardDvorak` abort slot.
const ZHUYIN_SCHEMES: &[ZhuyinScheme] = &[
    ZhuyinScheme::Standard,
    ZhuyinScheme::Hsu,
    ZhuyinScheme::Ibm,
    ZhuyinScheme::Ginyieh,
    ZhuyinScheme::Eten,
    ZhuyinScheme::Eten26,
    ZhuyinScheme::HsuDvorak,
    ZhuyinScheme::DachenCp26,
];

fn parser_accepts_spellings(parser: &FullPinyinParser, joined: &str) -> bool {
    joined
        .split('\'')
        .all(|spelling| spelling.is_empty() || SyllableKey::from_text(spelling).is_some())
        && !InputParser::parse(parser, joined.as_bytes())
            .unwrap()
            .is_empty()
}

/// The `test_parser2` hard invariant: ordered, contiguous, non-overlapping
/// spans that together cover the parsed prefix of the input.
#[test]
fn every_double_pinyin_scheme_produces_aligned_key_spans() {
    let input = b"uiaaaa"; // six keystrokes; layout-independent length
    for scheme in DOUBLE_SCHEMES {
        let mut parser = DoublePinyinParser::new();
        assert!(parser.set_scheme(*scheme), "{scheme:?} has a table");
        let parse = parser.parse(input, false);
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
        let full = parser.parse(b"uikjkuk", false).full_pinyin();
        assert!(
            parser_accepts_spellings(&FullPinyinParser, &full),
            "{scheme:?}: full-pinyin projection {full:?} is not parsable"
        );
    }
}

#[test]
fn the_zrm_fallback_table_resolves_two_key_spellings() {
    // Upstream ZRM carries a two-key fallback ("aa" -> a); the contract
    // tests exercise it through the ABI, this exercises the parser seam.
    let mut parser = DoublePinyinParser::new();
    assert!(parser.set_scheme(DoublePinyinScheme::Zrm));
    let parse = parser.parse(b"aa", false);
    assert_eq!(parse.consumed(), 2, "both keystrokes consumed");
    assert_eq!(parse.keys().len(), 1, "the fallback folds two keys to one");
    assert_eq!(parse.keys()[0].key().text(), "a");
    assert_eq!(parse.keys()[0].start(), 0);
    assert_eq!(parse.keys()[0].end(), 2);
}

#[test]
fn no_two_double_pinyin_layouts_read_the_keyboard_alike() {
    // The same keystrokes mean different syllables under different
    // layouts. Sweep every two-keystroke input: the six published tables
    // must produce pairwise distinct readings somewhere on that domain —
    // the parser must genuinely consult the active table.
    let inputs: Vec<[u8; 2]> = (b'a'..=b'z')
        .flat_map(|a| (b'a'..=b'z').map(move |b| [a, b]))
        .collect();
    let readings: Vec<Vec<String>> = DOUBLE_SCHEMES
        .iter()
        .map(|scheme| {
            let mut parser = DoublePinyinParser::new();
            parser.set_scheme(*scheme);
            inputs
                .iter()
                .map(|input| parser.parse(input, false).full_pinyin())
                .collect()
        })
        .collect();

    for (i, left) in DOUBLE_SCHEMES.iter().enumerate() {
        for (j, right) in DOUBLE_SCHEMES.iter().enumerate().skip(i + 1) {
            assert_ne!(
                readings[i], readings[j],
                "{left:?} and {right:?} read the keyboard identically"
            );
        }
    }
}

#[test]
fn the_customized_slot_is_rejected_at_the_parser_seam() {
    // Upstream aborts (`pinyin_parser2.cpp:611-612`); oxpinyin reports
    // false and keeps the live scheme.
    let mut parser = DoublePinyinParser::new();
    assert!(!parser.set_scheme(DoublePinyinScheme::Customized));
    assert_eq!(parser.scheme(), DoublePinyinScheme::Ms, "MS stays live");
    assert_eq!(parser.parse(b"ni", false).consumed(), 2);
}

#[test]
fn every_zhuyin_keyboard_parses_the_dachen_spelling_with_aligned_spans() {
    // `ma` in the standard (dachen) layout is ㄇㄚ; every implemented
    // keyboard must map *some* keystrokes to valid keys with the same
    // span discipline as the double-pinyin parsers.
    for scheme in ZHUYIN_SCHEMES {
        let mut parser = ZhuyinParser::new();
        assert!(parser.set_scheme(*scheme), "{scheme:?} is implemented");
        let parse = parser.parse(b"ma", false, false);
        assert!(
            !parse.keys().is_empty() || parse.consumed() == 0,
            "{scheme:?}: empty parse must consume nothing"
        );
        let mut cursor = 0;
        for key in parse.keys() {
            assert!(key.start() >= cursor, "{scheme:?}: span regression");
            assert!(key.end() > key.start(), "{scheme:?}: empty span");
            assert!(
                SyllableKey::from_text(key.key().text()).is_some(),
                "{scheme:?}: resolved syllable {:?} is frozen inventory",
                key.key().text()
            );
            cursor = key.end();
        }
        assert_eq!(
            parse.consumed(),
            cursor,
            "{scheme:?}: consumed length must equal the span coverage"
        );
    }
}

#[test]
fn the_standard_keyboard_reads_the_dachen_mapping() {
    // Dachen: v=ㄒ u=ㄧ 0=ㄢ resolves to the single key xian; the tone
    // digit is consumed under USE_TONE. Pinned by the same expectation as
    // the black-box exact-scheme ABI tests.
    let parser = ZhuyinParser::new();
    let parse = parser.parse(b"vu0", true, false);
    assert_eq!(parse.consumed(), 3);
    assert_eq!(parse.keys().len(), 1);
    assert_eq!(parse.full_pinyin(), "xian");
    assert_eq!(parse.keys()[0].tone(), 0);
}

#[test]
fn incomplete_zhuyin_keystrokes_consume_zero_either_way() {
    // Pinned parity with the oracle: the zhuyin index's incomplete tier
    // matches under `ZHUYIN_INCOMPLETE` but the post-match validity mask
    // still rejects it, so an initial-only keystroke consumes nothing with
    // or without the option — the same outcome upstream produces.
    let parser = ZhuyinParser::new();
    assert_eq!(parser.parse(b"v", true, false).consumed(), 0);
    assert_eq!(parser.parse(b"v", true, true).consumed(), 0);
    assert_eq!(parser.parse(b"1", true, false).consumed(), 0);
    assert_eq!(parser.parse(b"1", true, true).consumed(), 0);
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
    use oxpinyin_core::{FORCE_TONE, USE_TONE};
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
fn hsu_is_a_discrete_keyboard_that_differs_from_dachen() {
    // The HSU layout is a distinct mapping table: at minimum it must not
    // reproduce the dachen reading of the same keystrokes, and its
    // correction bit is forced during parse (upstream
    // `ZhuyinDiscreteParser2`).
    let mut hsu = ZhuyinParser::new();
    assert!(hsu.set_scheme(ZhuyinScheme::Hsu));
    let hsu_parse = hsu.parse(b"hk", false, false);
    let standard = ZhuyinParser::new().parse(b"hk", false, false);
    assert_ne!(
        hsu_parse.full_pinyin(),
        standard.full_pinyin(),
        "HSU must remap dachen keystrokes"
    );
    for key in hsu_parse.keys() {
        assert!(key.start() < key.end());
    }
}

#[test]
fn the_romanization_indexes_parse_with_aligned_spans() {
    // Upstream `pinyindirect`/`zhuyindirect` parsers: the alternate
    // romanizations resolve through a pinned spelling index. `jhih` is a
    // four-letter Luoma spelling (the ABI contract suite pins its parse
    // length); here the invariant is alignment and canonical validity.
    // `jhih` is a four-letter Luoma spelling (the ABI contract suite pins
    // its parse length); the secondary table's own head entry `ai` is the
    // secondary sample. Each must parse as aligned keys resolving to
    // frozen canonical syllables.
    for (name, index, input) in [
        ("luoma", &LUOMA_PINYIN_INDEX[..], &b"jhih"[..]),
        ("secondary", &SECONDARY_ZHUYIN_INDEX[..], &b"ai"[..]),
    ] {
        let parse = parse_full_pinyin_index(input, false, index);
        assert_eq!(
            parse.consumed(),
            input.len(),
            "{name}: the whole spelling parses"
        );
        assert!(!parse.keys().is_empty(), "{name}: at least one key");
        let mut cursor = 0;
        for key in parse.keys() {
            assert!(key.start() >= cursor, "{name}: span regression");
            assert!(key.end() > key.start(), "{name}: empty span");
            assert!(
                SyllableKey::from_text(key.canonical()).is_some(),
                "{name}: canonical {:?} must be a frozen syllable",
                key.canonical()
            );
            cursor = key.end();
        }
        assert_eq!(
            cursor,
            input.len(),
            "{name}: spans cover the consumed input"
        );
    }
}

#[test]
fn the_full_pinyin_scheme_slot_enumerates_its_three_romanizations() {
    // FullPinyinScheme::index mirrors the parser selection the ABI makes
    // for LUOMA/SECONDARY; the Hanyu default uses the ordinary parser and
    // has no index mapping through this seam.
    assert!(FullPinyinScheme::Hanyu.index().is_none());
    assert_eq!(
        FullPinyinScheme::Luoma.index(),
        Some(&LUOMA_PINYIN_INDEX[..])
    );
    assert_eq!(
        FullPinyinScheme::SecondaryZhuyin.index(),
        Some(&SECONDARY_ZHUYIN_INDEX[..])
    );
}
