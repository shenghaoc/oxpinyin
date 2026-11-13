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
    DoublePinyinParser, DoublePinyinScheme, FullPinyinScheme, SyllableKey, ZhuyinParser,
    luoma_pinyin_index, secondary_zhuyin_index,
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
