use super::{DoublePinyinParser, DoublePinyinScheme, ZhuyinParse, ZhuyinParser, ZhuyinScheme};
use crate::full_pinyin_index::{luoma_pinyin_index, secondary_zhuyin_index};
use crate::options::OptionBits;
use crate::parser::{FullPinyinScheme, ZhuyinKey, parse_zhuyin_direct};
use crate::vocab::SyllableKey;

/// `parser` over `input` under `USE_TONE`, the word most cases here set.
fn toned(parser: &ZhuyinParser, input: &[u8]) -> ZhuyinParse {
    parser.parse_with_options(input, OptionBits::USE_TONE)
}

fn double_full_pinyin(parse: &crate::parser::DoublePinyinParse) -> String {
    parse
        .keys()
        .iter()
        .map(|item| item.key().text())
        .collect::<Vec<_>>()
        .join("'")
}

#[test]
fn double_two_keys_use_fallback_when_the_first_yunmu_is_missing() {
    use super::{MS_SHENG, MS_YUN, SchemeTables, YunmuTable, parse_two_keys};

    const YUNMU: YunmuTable = {
        let mut table = MS_YUN;
        // Index 14 is `o`, counted from `a`.
        table[14] = [None, Some("o")];
        table
    };
    let tables = SchemeTables {
        shengmu: &MS_SHENG,
        yunmu: &YUNMU,
        fallback: Some(&[("oo", "ou")]),
    };

    // Pin pinyin_parser2.cpp:478-479: a NULL first slot goes to fallback.
    let key = parse_two_keys(b"oo", tables).expect("oo resolves through fallback");
    assert_eq!(key.text(), "ou");
}

/// The key one zhuyin symbol maps to on `scheme`'s keyboard, derived
/// from the symbol table rather than hand-authored (the same rule the
/// frozen chewing layout cases apply).
fn key_for_symbol(scheme: ZhuyinScheme, symbol: &str) -> u8 {
    let tables = scheme.simple_tables().expect("Simple keyboard");
    for (key, candidate) in tables.symbols {
        if *candidate == symbol {
            return *key;
        }
    }
    panic!("no {scheme:?} key spells {symbol:?}");
}

fn keystrokes(scheme: ZhuyinScheme, symbols: &[&str]) -> Vec<u8> {
    symbols
        .iter()
        .map(|symbol| key_for_symbol(scheme, symbol))
        .collect()
}

/// The key spelling tone `tone` (1..5) on `scheme`'s keyboard,
/// derived from the tone table like [`key_for_symbol`].
fn key_for_tone(scheme: ZhuyinScheme, tone: u8) -> u8 {
    let tables = scheme.simple_tables().expect("Simple keyboard");
    for (key, candidate) in tables.tones {
        if *candidate == tone {
            return *key;
        }
    }
    panic!("no {scheme:?} key spells tone {tone}");
}

fn keystrokes_with_tone(scheme: ZhuyinScheme, symbols: &[&str], tone: u8) -> Vec<u8> {
    let mut keys = keystrokes(scheme, symbols);
    keys.push(key_for_tone(scheme, tone));
    keys
}

#[test]
fn index_matches_the_pinned_row_counts_and_shuffle_law() {
    assert_eq!(
        crate::zhuyin_map::reference::ZHUYIN_PINYIN_MAP
            .iter()
            .copied()
            .len(),
        1493
    );

    // Sorted and unique: the binary-search invariant of the pinned
    // table (strcmp order == UTF-8 byte order).
    for pair in crate::zhuyin_map::reference::ZHUYIN_PINYIN_MAP.windows(2) {
        assert!(pair[0].0 < pair[1].0, "rows not strictly sorted");
    }

    let incomplete: Vec<_> = crate::zhuyin_map::reference::ZHUYIN_PINYIN_MAP
        .iter()
        .copied()
        .filter(|row| OptionBits::from_bits_retain(row.3).contains(OptionBits::ZHUYIN_INCOMPLETE))
        .collect();
    let shuffle: Vec<_> = crate::zhuyin_map::reference::ZHUYIN_PINYIN_MAP
        .iter()
        .copied()
        .filter(|row| {
            OptionBits::from_bits_retain(row.3).contains(OptionBits::ZHUYIN_CORRECT_SHUFFLE)
        })
        .collect();
    let plain: Vec<_> = crate::zhuyin_map::reference::ZHUYIN_PINYIN_MAP
        .iter()
        .copied()
        .filter(|row| row.3 == 0)
        .collect();
    assert_eq!(incomplete.len(), 14);
    assert_eq!(shuffle.len(), 1062);
    assert_eq!(plain.len(), 417);

    // The 417 plain rows are 23 single-symbol + 227 two-symbol +
    // 167 three-symbol spellings, and every multi-symbol plain row
    // contributes all non-canonical symbol permutations as shuffle
    // rows: 227 × 1 + 167 × 5 = 1062.
    let plain_by_len = |symbols: usize| {
        plain
            .iter()
            .filter(|row| row.0.chars().count() == symbols)
            .count()
    };
    assert_eq!(plain_by_len(1), 23);
    assert_eq!(plain_by_len(2), 227);
    assert_eq!(plain_by_len(3), 167);

    let mut expected: std::collections::HashSet<(String, &'static str)> = Default::default();
    for (spelling, _canonical, pinyin, _) in plain.iter().copied() {
        let symbols: Vec<&str> = spelling
            .char_indices()
            .map(|(i, ch)| &spelling[i..i + ch.len_utf8()])
            .collect();
        if symbols.len() < 2 {
            continue;
        }
        permute(&symbols, &mut |permutation: &[&str]| {
            if permutation != symbols.as_slice() {
                expected.insert((permutation.concat(), pinyin));
            }
        });
    }
    assert_eq!(expected.len(), 1062);
    for (spelling, _canonical, pinyin, flags) in shuffle.iter().copied() {
        assert_eq!(
            OptionBits::from_bits_retain(flags),
            OptionBits::ZHUYIN_CORRECT_SHUFFLE
        );
        assert!(
            expected.contains(&(spelling.to_owned(), pinyin)),
            "unexpected shuffle row {spelling:?}"
        );
    }
}

/// Heap's algorithm over the symbol slice, in any order — the test
/// only needs the full permutation set, not a canonical order.
fn permute<'a>(symbols: &[&'a str], mut emit: impl FnMut(&[&'a str])) {
    fn go<'a, F: FnMut(&[&'a str])>(symbols: &mut [&'a str], k: usize, emit: &mut F) {
        if k <= 1 {
            emit(symbols);
            return;
        }
        go(symbols, k - 1, emit);
        for i in 0..k - 1 {
            if k.is_multiple_of(2) {
                symbols.swap(i, k - 1);
            } else {
                symbols.swap(0, k - 1);
            }
            go(symbols, k - 1, emit);
        }
    }
    let mut owned = symbols.to_vec();
    let len = owned.len();
    go(&mut owned, len, &mut emit);
}

#[test]
fn index_flags_gate_through_the_option_word() {
    use super::search_zhuyin_index;
    use crate::zhuyin_map::zhuyin_pinyin_map;

    // Shuffle rows: only under the parser-owned correction bit.
    let bie = crate::vocab::SyllableKey::from_text("bie").expect("bie");
    assert_eq!(
        search_zhuyin_index(
            zhuyin_pinyin_map(),
            "ㄅㄝㄧ",
            OptionBits::ZHUYIN_CORRECT_SHUFFLE
        ),
        Some(bie)
    );
    assert_eq!(
        search_zhuyin_index(zhuyin_pinyin_map(), "ㄅㄝㄧ", OptionBits::empty()),
        None
    );

    // Incomplete rows: only under the caller's option bit. All 14 are
    // parse-dead after the validity mask, but the index gate itself is
    // what this pins (both outcomes at this pin consume 0).
    let b = crate::vocab::SyllableKey::from_text("b").expect("b");
    assert_eq!(
        search_zhuyin_index(zhuyin_pinyin_map(), "ㄅ", OptionBits::ZHUYIN_INCOMPLETE),
        Some(b)
    );
    assert_eq!(
        search_zhuyin_index(zhuyin_pinyin_map(), "ㄅ", OptionBits::empty()),
        None
    );
}

#[test]
fn shuffled_spellings_parse_to_their_canonical_syllable() {
    let parser = ZhuyinParser::new();
    // The SPEC's own example: ㄅㄝㄧ (shuffled ㄅㄧㄝ) is bie, and it
    // displays as the canonical spelling, like upstream's aux text.
    let shuffled = toned(
        &parser,
        &keystrokes(ZhuyinScheme::Standard, &["ㄅ", "ㄝ", "ㄧ"]),
    );
    assert_eq!(shuffled.consumed(), 3);
    assert_eq!(shuffled.full_pinyin(), "bie");
    assert_eq!(shuffled.keys()[0].display(), "ㄅㄧㄝ");

    // Every permutation of a three-symbol spelling parses.
    let canonical = ["ㄅ", "ㄧ", "ㄝ"];
    permute(&canonical, &mut |permutation: &[&str]| {
        let parsed = toned(&parser, &keystrokes(ZhuyinScheme::Standard, permutation));
        assert_eq!(
            parsed.full_pinyin(),
            "bie",
            "permutation {permutation:?} must parse as bie"
        );
    });

    // A two-symbol swap: ㄧㄅ (shuffled ㄅㄧ) is bi.
    let swapped = toned(&parser, &keystrokes(ZhuyinScheme::Standard, &["ㄧ", "ㄅ"]));
    assert_eq!(swapped.full_pinyin(), "bi");

    // Shuffle composes with tone.
    let toned = toned(
        &parser,
        &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄅ", "ㄝ", "ㄧ"], 4),
    );
    assert_eq!(toned.full_pinyin(), "bie");
    assert_eq!(toned.keys()[0].tone(), 4);
}

#[test]
fn recovered_rows_parse_with_their_upstream_tone_masks() {
    let parser = ZhuyinParser::new();

    // ㄉㄣ (den): tones 0 and 4 valid; tone 1 is mask-invalid, so the
    // post-match break stops the whole parse at the two symbol keys.
    assert_eq!(
        toned(&parser, &keystrokes(ZhuyinScheme::Standard, &["ㄉ", "ㄣ"])).full_pinyin(),
        "den"
    );
    let den4 = toned(
        &parser,
        &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄉ", "ㄣ"], 4),
    );
    assert_eq!(den4.consumed(), 3);
    assert_eq!(den4.full_pinyin(), "den");
    let den1 = toned(
        &parser,
        &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄉ", "ㄣ"], 1),
    );
    assert_eq!(den1.consumed(), 0, "tone 1 is mask-invalid for den");

    // ㄥ (eng): tones 0 and 1 valid — first tone legal, unlike ㄋㄧ.
    let eng = toned(&parser, b"/");
    assert_eq!(eng.full_pinyin(), "eng");
    let eng1 = toned(
        &parser,
        &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄥ"], 1),
    );
    assert_eq!(eng1.consumed(), 2, "tone 1 is mask-valid for eng");
    assert_eq!(eng1.full_pinyin(), "eng");

    // ㄓㄟ (zhei): tones 0 and 4 valid.
    assert_eq!(toned(&parser, b"5o").full_pinyin(), "zhei");
    assert_eq!(
        toned(
            &parser,
            &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄓ", "ㄟ"], 4)
        )
        .consumed(),
        3
    );
    assert_eq!(
        toned(
            &parser,
            &keystrokes_with_tone(ZhuyinScheme::Standard, &["ㄓ", "ㄟ"], 1)
        )
        .consumed(),
        0
    );

    // The five dead rows (mask 0): matched, then rejected by the
    // post-match validity break — consumed 0 on the whole input.
    for input in [
        keystrokes(ZhuyinScheme::Standard, &["ㄈ", "ㄜ"]), // fe
        keystrokes(ZhuyinScheme::Standard, &["ㄉ", "ㄧ", "ㄣ"]), // din
        keystrokes(ZhuyinScheme::Standard, &["ㄎ", "ㄟ"]), // kei
        keystrokes(ZhuyinScheme::Standard, &["ㄌ", "ㄣ"]), // len
        keystrokes(ZhuyinScheme::Standard, &["ㄖ", "ㄨ", "ㄚ"]), // rua
    ] {
        let parsed = toned(&parser, &input);
        assert_eq!(parsed.consumed(), 0, "{input:?} is a dead row at the pin");
    }
}

#[test]
fn valid_zhuyin_tones_carry_the_extracted_masks() {
    use super::VALID_ZHUYIN_TONES;
    use crate::vocab::{SYLLABLE_KEY_COUNT, SyllableKey};
    assert_eq!(VALID_ZHUYIN_TONES.len(), SYLLABLE_KEY_COUNT);

    let mask = |spelling: &str| {
        VALID_ZHUYIN_TONES[SyllableKey::from_canonical_text(spelling)
            .unwrap_or_else(|| panic!("{spelling:?} is not a canonical key"))
            .index()]
    };
    // The twelve recovered rows and the incomplete tier, from the
    // pinned valid_zhuyin_table extraction.
    assert_eq!(mask("eng"), 0x03);
    assert_eq!(mask("nun"), 0x15);
    assert_eq!(mask("chua"), 0x0b);
    assert_eq!(mask("den"), 0x11);
    assert_eq!(mask("zhei"), 0x11);
    assert_eq!(mask("nia"), 0x05);
    assert_eq!(mask("yai"), 0x05);
    assert_eq!(mask("fe"), 0x00);
    assert_eq!(mask("din"), 0x00);
    assert_eq!(mask("kei"), 0x00);
    assert_eq!(mask("len"), 0x00);
    assert_eq!(mask("rua"), 0x00);
    // The 14 zhuyin incomplete rows are ㄅ..ㄒ; every initial-only key
    // is parse-dead in zhuyin at the pin.
    for key in crate::syllables::INCOMPLETE_PINYIN_KEYS {
        assert_eq!(mask(key), 0x00, "initial-only key {key:?}");
    }
}

#[test]
fn every_simple_keyboard_parses_the_same_symbols_through_its_own_keys() {
    for scheme in [
        ZhuyinScheme::Standard,
        ZhuyinScheme::Ibm,
        ZhuyinScheme::Ginyieh,
        ZhuyinScheme::Eten,
    ] {
        let parser = ZhuyinParser::with_scheme(scheme);

        // Tone-bearing and tone-rejection through this keyboard's keys.
        let ni2 = toned(&parser, &keystrokes_with_tone(scheme, &["ㄋ", "ㄧ"], 2));
        assert_eq!((scheme, ni2.consumed()), (scheme, 3));
        assert_eq!((scheme, ni2.full_pinyin().as_str()), (scheme, "ni"));
        let ni1 = toned(&parser, &keystrokes_with_tone(scheme, &["ㄋ", "ㄧ"], 1));
        assert_eq!((scheme, ni1.consumed()), (scheme, 0));

        // Keyboard membership mirrors the tables: every symbol key
        // reports its symbol, every tone key its mark, others nothing.
        let tables = scheme.simple_tables().expect("Simple keyboard");
        for &(key, symbol) in tables.symbols {
            assert!(
                parser.in_scheme(key, false),
                "{scheme:?} symbol key {key:?}"
            );
            assert_eq!(parser.symbols_for(key, false), vec![symbol.to_owned()]);
        }
        for &(key, _tone) in tables.tones {
            assert!(!parser.in_scheme(key, false));
            assert!(parser.in_scheme(key, true));
        }
        assert!(!parser.in_scheme(b'Q', true));
        assert!(parser.symbols_for(b'Q', true).is_empty());
    }
}

#[test]
fn keyboards_switch_tables_and_stick() {
    let mut parser = ZhuyinParser::new();
    for scheme in [
        ZhuyinScheme::Ibm,
        ZhuyinScheme::Ginyieh,
        ZhuyinScheme::Eten,
        ZhuyinScheme::Hsu,
        ZhuyinScheme::Eten26,
        ZhuyinScheme::HsuDvorak,
        ZhuyinScheme::DachenCp26,
        ZhuyinScheme::Standard,
    ] {
        assert!(parser.set_scheme(scheme));
        assert_eq!(parser.scheme(), scheme);
    }
    // The dvorak abort slot keeps the current scheme.
    assert!(!parser.set_scheme(ZhuyinScheme::StandardDvorak));
    assert_eq!(parser.scheme(), ZhuyinScheme::Standard);
}

#[test]
fn contract_slots_reject_without_mutation() {
    // The #109 contract locks, core side: CUSTOMIZED (30) and the
    // dvorak fallthrough slot (7) reject and keep the live scheme —
    // including a fallback-bearing scheme's fallback behaviour.
    let mut parser = DoublePinyinParser::with_scheme(DoublePinyinScheme::Zrm);
    assert!(
        parser
            .parse_with_options(b"aa", OptionBits::empty())
            .consumed()
            == 2,
        "ZRM fallback row"
    );
    assert!(!parser.set_scheme(DoublePinyinScheme::Customized));
    assert_eq!(parser.scheme(), DoublePinyinScheme::Zrm);
    assert_eq!(
        parser
            .parse_with_options(b"aa", OptionBits::empty())
            .consumed(),
        2
    );

    let mut zhuyin = ZhuyinParser::with_scheme(ZhuyinScheme::DachenCp26);
    assert_eq!(toned(&zhuyin, b"t").full_pinyin(), "zhi");
    assert!(!zhuyin.set_scheme(ZhuyinScheme::StandardDvorak));
    assert_eq!(zhuyin.scheme(), ZhuyinScheme::DachenCp26);
    assert_eq!(toned(&zhuyin, b"t").full_pinyin(), "zhi");
}

#[test]
fn customized_slot_keeps_the_default_double_pinyin_scheme() {
    let mut parser = DoublePinyinParser::new();
    assert_eq!(parser.scheme(), DoublePinyinScheme::Ms);
    assert!(!parser.set_scheme(DoublePinyinScheme::Customized));
    assert_eq!(parser.scheme(), DoublePinyinScheme::Ms);
}

#[test]
fn apostrophe_keys_follow_the_pinned_tables() {
    // The two keyboards whose tables use the escaped-quote key.
    assert_eq!(key_for_symbol(ZhuyinScheme::Ginyieh, "ㄥ"), b'\'');
    assert_eq!(key_for_symbol(ZhuyinScheme::Eten, "ㄘ"), b'\'');
}

/// The key that makes a positional probe read `symbol` at `slot`:
/// the parser takes each table's **first** matching row per key, so
/// a dual key's second row never derives a parse (same rule the
/// frozen chewing layout cases apply for the Discrete keyboards).
fn discrete_key_for(scheme: ZhuyinScheme, slot: usize, symbol: &str) -> u8 {
    let tables = scheme.discrete_tables().expect("discrete keyboard");
    let ordered: &[&[(u8, &'static str)]] = match slot {
        0 => &[tables.initials, tables.middles, tables.finals],
        1 => &[tables.middles, tables.finals],
        _ => &[tables.finals],
    };
    for table in ordered {
        let mut last_key: Option<u8> = None;
        for &(key, candidate) in *table {
            if last_key == Some(key) {
                continue;
            }
            last_key = Some(key);
            if candidate == symbol {
                return key;
            }
        }
    }
    panic!("no {scheme:?} key derives {symbol:?} at slot {slot}");
}

fn discrete_keystrokes(scheme: ZhuyinScheme, symbols: &[&str]) -> Vec<u8> {
    symbols
        .iter()
        .enumerate()
        .map(|(slot, symbol)| discrete_key_for(scheme, slot, symbol))
        .collect()
}

fn discrete_keystrokes_with_tone(scheme: ZhuyinScheme, symbols: &[&str], tone: u8) -> Vec<u8> {
    let mut keys = discrete_keystrokes(scheme, symbols);
    let tables = scheme.discrete_tables().expect("discrete keyboard");
    let tone_key = tables
        .tones
        .iter()
        .find(|(_, value)| *value == tone)
        .map_or_else(
            || panic!("no {scheme:?} key spells tone {tone}"),
            |(key, _)| *key,
        );
    keys.push(tone_key);
    keys
}

#[test]
fn discrete_keyboards_parse_through_positional_probes() {
    for scheme in [
        ZhuyinScheme::Hsu,
        ZhuyinScheme::Eten26,
        ZhuyinScheme::HsuDvorak,
    ] {
        let parser = ZhuyinParser::with_scheme(scheme);

        // Canonical control: ㄋㄧ through the n/e keys, tone-bearing
        // and tone-rejection.
        let ni = toned(&parser, &discrete_keystrokes(scheme, &["ㄋ", "ㄧ"]));
        assert_eq!((scheme, ni.consumed()), (scheme, 2));
        assert_eq!((scheme, ni.full_pinyin().as_str()), (scheme, "ni"));
        let ni2 = toned(
            &parser,
            &discrete_keystrokes_with_tone(scheme, &["ㄋ", "ㄧ"], 2),
        );
        assert_eq!((scheme, ni2.consumed()), (scheme, 3));
        let ni1 = toned(
            &parser,
            &discrete_keystrokes_with_tone(scheme, &["ㄋ", "ㄧ"], 1),
        );
        assert_eq!((scheme, ni1.consumed()), (scheme, 0));
    }
}

#[test]
fn hsu_remap_rows_parse_with_canonical_display() {
    let hsu = ZhuyinParser::with_scheme(ZhuyinScheme::Hsu);

    // The keyboard's correction rows: ㄍㄧ→ji (typed g,e), ㄐㄨㄥ→
    // zhong (j,x,l), ㄑ→chi (q), ㄒ→shi (c), ㄇ→an (m). Display
    // renders each row's canonical spelling.
    let ji = toned(&hsu, &discrete_keystrokes(hsu.scheme(), &["ㄍ", "ㄧ"]));
    assert_eq!(ji.full_pinyin(), "ji");
    assert_eq!(ji.keys()[0].display(), "ㄐㄧ");

    let zhong = toned(
        &hsu,
        &discrete_keystrokes(hsu.scheme(), &["ㄐ", "ㄨ", "ㄥ"]),
    );
    assert_eq!(zhong.full_pinyin(), "zhong");
    assert_eq!(zhong.keys()[0].display(), "ㄓㄨㄥ");

    let chi = toned(&hsu, &discrete_keystrokes(hsu.scheme(), &["ㄑ"]));
    assert_eq!(chi.full_pinyin(), "chi");
    let shi = toned(&hsu, &discrete_keystrokes(hsu.scheme(), &["ㄒ"]));
    assert_eq!(shi.full_pinyin(), "shi");
    let an = toned(&hsu, &discrete_keystrokes(hsu.scheme(), &["ㄇ"]));
    assert_eq!(an.full_pinyin(), "an");

    // The palatal plain rows stay reachable through the same keys
    // plus a middle: c,e types ㄒㄧ = xi.
    let xi = toned(&hsu, &discrete_keystrokes(hsu.scheme(), &["ㄒ", "ㄧ"]));
    assert_eq!(xi.full_pinyin(), "xi");
}

#[test]
fn eten26_remap_rows_parse_with_canonical_display() {
    let eten26 = ZhuyinParser::with_scheme(ZhuyinScheme::Eten26);

    // ETEN26 remaps ㄍㄧ to the qi series (HSU remaps it to ji) and
    // has its own vowel stand-ins: ㄆ→ou, ㄊ→ang, ㄌ→eng, ㄏ→er.
    let qi = toned(
        &eten26,
        &discrete_keystrokes(eten26.scheme(), &["ㄍ", "ㄧ"]),
    );
    assert_eq!(qi.full_pinyin(), "qi");
    assert_eq!(qi.keys()[0].display(), "ㄑㄧ");

    for (symbols, pinyin) in [
        (vec!["ㄆ"], "ou"),
        (vec!["ㄊ"], "ang"),
        (vec!["ㄌ"], "eng"),
        (vec!["ㄏ"], "er"),
        (vec!["ㄒ", "ㄨ"], "shu"),
    ] {
        let parsed = toned(&eten26, &discrete_keystrokes(eten26.scheme(), &symbols));
        assert_eq!(
            parsed.full_pinyin(),
            pinyin,
            "{symbols:?} on ETEN26 must parse as {pinyin}"
        );
    }
}

#[test]
fn discrete_indexes_gate_their_correction_rows() {
    use super::search_zhuyin_index;
    use crate::zhuyin_map::{eten26_zhuyin_index, hsu_zhuyin_index};

    // ㄍㄧ is ji under the HSU bit and qi under the ETEN26 bit — the
    // same keystrokes mean different syllables per keyboard.
    let ji = crate::vocab::SyllableKey::from_text("ji").expect("ji");
    let qi = crate::vocab::SyllableKey::from_text("qi").expect("qi");
    assert_eq!(
        search_zhuyin_index(hsu_zhuyin_index(), "ㄍㄧ", OptionBits::ZHUYIN_CORRECT_HSU),
        Some(ji)
    );
    assert_eq!(
        search_zhuyin_index(
            eten26_zhuyin_index(),
            "ㄍㄧ",
            OptionBits::ZHUYIN_CORRECT_ETEN26
        ),
        Some(qi)
    );

    // The HSU incomplete set is 5 rows (ㄅㄆㄈㄉㄊ), ETEN26's is 6
    // (ㄅㄈㄉㄍㄎㄑ) — both smaller than the global 14.
    let hsu_incomplete = crate::zhuyin_map::reference::HSU_ZHUYIN_INDEX
        .iter()
        .copied()
        .filter(|row| OptionBits::from_bits_retain(row.3).contains(OptionBits::ZHUYIN_INCOMPLETE))
        .count();
    let eten26_incomplete = crate::zhuyin_map::reference::ETEN26_ZHUYIN_INDEX
        .iter()
        .copied()
        .filter(|row| OptionBits::from_bits_retain(row.3).contains(OptionBits::ZHUYIN_INCOMPLETE))
        .count();
    assert_eq!(hsu_incomplete, 5);
    assert_eq!(eten26_incomplete, 6);
}

#[test]
fn dual_mapped_keys_report_every_symbol_but_parse_the_first() {
    use super::tone_symbol;

    let hsu = ZhuyinParser::with_scheme(ZhuyinScheme::Hsu);
    // 'c' is dual in the initials (ㄒ/ㄕ): display reports both.
    assert_eq!(
        hsu.symbols_for(b'c', false),
        vec!["ㄒ".to_owned(), "ㄕ".to_owned()]
    );
    assert_eq!(
        hsu.symbols_for(b'j', false),
        vec!["ㄐ".to_owned(), "ㄓ".to_owned()]
    );
    assert_eq!(
        hsu.symbols_for(b'v', false),
        vec!["ㄑ".to_owned(), "ㄔ".to_owned()]
    );
    // 'e' is a middle (ㄧ) and a final (ㄝ): both, in slot order.
    assert_eq!(
        hsu.symbols_for(b'e', false),
        vec!["ㄧ".to_owned(), "ㄝ".to_owned()]
    );
    // 'd' is an initial (ㄉ) and tone 2: symbol plus tone mark.
    assert_eq!(
        hsu.symbols_for(b'd', true),
        vec!["ㄉ".to_owned(), tone_symbol(2).to_owned()]
    );
    assert!(hsu.in_scheme(b'd', false));
    assert!(!hsu.in_scheme(b'Q', true));

    // Parsing takes the first row: 'c' alone is ㄒ → the HSU remap
    // row makes it shi, never si-from-ㄕ.
    let c_alone = toned(&hsu, b"c");
    assert_eq!(c_alone.full_pinyin(), "shi");
}

#[test]
fn hsu_dvorak_behaves_like_hsu_at_this_pin() {
    // chewing_hsu_dvorak_* is byte-identical to chewing_hsu_* at
    // 2.11.91 and shares hsu_zhuyin_index: same inputs, same parses.
    let hsu = ZhuyinParser::with_scheme(ZhuyinScheme::Hsu);
    let dvorak = ZhuyinParser::with_scheme(ZhuyinScheme::HsuDvorak);
    for input in [
        &discrete_keystrokes(ZhuyinScheme::Hsu, &["ㄋ", "ㄧ"])[..],
        b"ge",
        b"c",
        b"jxl",
        b"m",
    ] {
        assert_eq!(
            toned(&hsu, input),
            toned(&dvorak, input),
            "{input:?} must parse identically on HSU and HSU_DVORAK"
        );
    }
}

#[test]
fn discrete_incomplete_rows_consume_zero() {
    // HSU admits ㄅ under ZHUYIN_INCOMPLETE; the all-zero mask stops
    // the parse either way (the derivation uses HSU's own keys).
    let hsu = ZhuyinParser::with_scheme(ZhuyinScheme::Hsu);
    let b_key = discrete_key_for(ZhuyinScheme::Hsu, 0, "ㄅ");
    assert_eq!(toned(&hsu, &[b_key]).consumed(), 0);
    assert_eq!(
        hsu.parse_with_options(
            &[b_key],
            OptionBits::USE_TONE | OptionBits::ZHUYIN_INCOMPLETE
        )
        .consumed(),
        0
    );
}

#[test]
fn cp26_repeat_counts_cycle_dual_rows() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::DachenCp26);

    // Initials: single tap is the first row, the run cycles —
    // t→ㄓ (zhi), tt→ㄔ (chi); q/t/w are the dual initial keys.
    assert_eq!(toned(&parser, b"t").full_pinyin(), "zhi");
    assert_eq!(toned(&parser, b"tt").full_pinyin(), "chi");
    assert_eq!(toned(&parser, b"ttt").full_pinyin(), "zhi");

    // The same tap-count law on a dual final: i→ㄛ (o), ii→ㄞ (ai).
    assert_eq!(toned(&parser, b"i").full_pinyin(), "o");
    assert_eq!(toned(&parser, b"ii").full_pinyin(), "ai");

    // Single taps that are complete plain rows.
    assert_eq!(toned(&parser, b"g").full_pinyin(), "shi");
    assert_eq!(toned(&parser, b"u").full_pinyin(), "yi");
    assert_eq!(toned(&parser, b"m").full_pinyin(), "yu");
}

#[test]
fn cp26_middle_specials_cycle_three_and_two_ways() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::DachenCp26);

    // u cycles (count-1) % 3: ㄧ, ㄚ, ㄧㄚ.
    assert_eq!(toned(&parser, b"u").full_pinyin(), "yi");
    assert_eq!(toned(&parser, b"uu").full_pinyin(), "a");
    assert_eq!(toned(&parser, b"uuu").full_pinyin(), "ya");
    assert_eq!(toned(&parser, b"uuuu").full_pinyin(), "yi");

    // m cycles (count-1) % 2: ㄩ, ㄡ.
    assert_eq!(toned(&parser, b"m").full_pinyin(), "yu");
    assert_eq!(toned(&parser, b"mm").full_pinyin(), "ou");

    // With an initial, the specials still apply: quu is ㄅㄚ (ba),
    // amm is ㄇㄡ (mou — ㄅㄡ is not a Mandarin syllable, so the
    // m-special needs a compatible initial); after a consumed
    // middle the finals table is probed plainly, so vum is ㄒㄧㄡ
    // (xiu) and vub is ㄒㄧㄝ (xie).
    assert_eq!(toned(&parser, b"qu").full_pinyin(), "bi");
    assert_eq!(toned(&parser, b"qqu").full_pinyin(), "pi");
    assert_eq!(toned(&parser, b"quu").full_pinyin(), "ba");
    assert_eq!(toned(&parser, b"amm").full_pinyin(), "mou");
    assert_eq!(toned(&parser, b"qmm").consumed(), 0);
    assert_eq!(toned(&parser, b"vub").full_pinyin(), "xie");
    assert_eq!(toned(&parser, b"vum").full_pinyin(), "xiu");
}

#[test]
fn cp26_tone_probe_wins_the_last_byte() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::DachenCp26);

    // e/d/r/y are initials AND tones: the tone probe strips the last
    // byte first — eke is ㄍㄜ plus tone 2, dkd is ㄎㄜ plus tone 4,
    // and a bare "ee"/"dd" strips to the lone incomplete initials
    // ㄍ/ㄎ and consumes 0 (upstream identically).
    let ge2 = toned(&parser, b"eke");
    assert_eq!(ge2.full_pinyin(), "ge");
    assert_eq!(ge2.keys()[0].tone(), 2);
    let ke4 = toned(&parser, b"dkd");
    assert_eq!(ke4.full_pinyin(), "ke");
    assert_eq!(ke4.keys()[0].tone(), 4);
    assert_eq!(toned(&parser, b"ee").consumed(), 0);
    assert_eq!(toned(&parser, b"dd").consumed(), 0);

    // Tone-bearing and tone-rejection on ㄋㄧ (s, u, e / space).
    let ni2 = toned(&parser, b"sue");
    assert_eq!(ni2.full_pinyin(), "ni");
    assert_eq!(ni2.keys()[0].tone(), 2);
    assert_eq!(toned(&parser, b"su ").consumed(), 0);

    // A lone tone-or-initial key under USE_TONE strips to nothing.
    assert_eq!(toned(&parser, b"d").consumed(), 0);
}

#[test]
fn cp26_greedy_boundary_and_incomplete_row() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::DachenCp26);

    // Multi-syllable greedy: ni then shi.
    let sug = toned(&parser, b"sug");
    assert_eq!(sug.consumed(), 3);
    assert_eq!(sug.full_pinyin(), "ni'shi");

    // ㄅ alone is the global index's incomplete row: matched under
    // the option, stopped by the all-zero mask — consumed 0 either
    // way, and CP26 owns no correction bit to change that.
    assert_eq!(toned(&parser, b"q").consumed(), 0);
    assert_eq!(
        parser
            .parse_with_options(b"q", OptionBits::USE_TONE | OptionBits::ZHUYIN_INCOMPLETE)
            .consumed(),
        0
    );

    // Empty input.
    assert_eq!(toned(&parser, b"").consumed(), 0);
}

#[test]
fn cp26_display_reports_the_special_and_dual_rows() {
    use super::tone_symbol;
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::DachenCp26);

    // 'i' reports both final rows plus the display-only ㄧㄚ — the
    // string parse can only produce through the u triple-tap.
    assert_eq!(
        parser.symbols_for(b'i', false),
        vec!["ㄛ".to_owned(), "ㄞ".to_owned(), "ㄧㄚ".to_owned()]
    );
    // 't' reports both initial rows; 'e' its initial plus tone mark.
    assert_eq!(
        parser.symbols_for(b't', false),
        vec!["ㄓ".to_owned(), "ㄔ".to_owned()]
    );
    assert_eq!(
        parser.symbols_for(b'e', true),
        vec!["ㄍ".to_owned(), tone_symbol(2).to_owned()]
    );
    // 'u' spans middle and final tables.
    assert_eq!(
        parser.symbols_for(b'u', false),
        vec!["ㄧ".to_owned(), "ㄚ".to_owned()]
    );
    assert!(!parser.in_scheme(b'Q', true));
}

// Behavioural port of `libpinyin/tests/storage/test_parser2.cpp`.
//
// The upstream driver enumerates every parser variant (`fullpinyin`,
// `doublepinyin`, `zhuyin` × `standard`/`hsu`/`dachen26`,
// `pinyindirect`, `zhuyindirect`), parses sample lines, and protects
// exactly one hard invariant: the parse produces aligned key/key-rest
// streams (`keys->len == key_rests->len`), i.e. every key carries a
// well-formed span of the raw input. The rest is diagnostic output.
//
// The same invariant, lifted to oxpinyin's public parser seam: every
// double-pinyin layout produces ordered, contiguous, non-overlapping spans,
// and its full-pinyin projection parses as frozen syllables. The zhuyin
// keyboards are checked against the frozen pin by the zhuyin parse parity
// tests below, and the alternate-romanization index parsers in
// `src/full_pinyin_index/tests.rs`. Scheme-setter rejection contracts
// are covered black-box in `capi/tests/it/contract.rs`;
// full-pinyin selected splits are checked in `src/graph/tests.rs`.

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
        let parse = parser.parse_with_options(input, OptionBits::empty());
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
        let full = double_full_pinyin(&parser.parse_with_options(b"uikjkuk", OptionBits::empty()));
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
    let forced = parser.parse_with_options(b"su3", OptionBits::USE_TONE | OptionBits::FORCE_TONE);
    assert_eq!(forced.consumed(), 3);
    assert_eq!(forced.keys().len(), 1);

    // Toneless syllables are rejected: a complete syllable with no tone
    // consumes 0 under FORCE_TONE, exactly as the pin does.
    for syllables in [&b"li"[..], &b"ju"[..]] {
        let untoned =
            parser.parse_with_options(syllables, OptionBits::USE_TONE | OptionBits::FORCE_TONE);
        assert_eq!(
            untoned.consumed(),
            0,
            "{} must consume 0 under FORCE_TONE",
            std::str::from_utf8(syllables).unwrap()
        );
    }

    // The same toneless syllables DO parse under USE_TONE alone (the pin
    // without FORCE_TONE accepts them) — verifying FORCE_TONE is the gate.
    let li = toned(&parser, b"li");
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
    let options = OptionBits::USE_TONE | OptionBits::FORCE_TONE;
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
    let keys: Vec<(&str, u8)> =
        parse_zhuyin_direct(&input, OptionBits::USE_TONE | OptionBits::FORCE_TONE)
            .into_iter()
            .map(|(key, tone)| (key.text(), tone))
            .collect();
    assert_eq!(keys, [("ni", 3), ("hao", 3)]);
}

/// `DoublePinyinParser2::parse_one_key` on the MS tables: a two-key
/// spelling resolves to its syllable with tone zero, a three-key
/// input carries the tone only under `USE_TONE`, and `FORCE_TONE`
/// demands exactly three bytes.
#[test]
fn double_one_key_law() {
    let parser = DoublePinyinParser::with_scheme(DoublePinyinScheme::Ms);

    let key = parser
        .parse_one_key(OptionBits::empty(), b"ni")
        .expect("ni parses");
    assert_eq!(key.pinyin_string(), "ni");
    assert_eq!(key.tone, 0);
    assert_eq!(key.shengmu_string(), "n");
    assert_eq!(key.yunmu_string(), "i");

    // Without USE_TONE the three-byte form is refused.
    assert_eq!(parser.parse_one_key(OptionBits::empty(), b"ni3"), None);
    let toned = parser
        .parse_one_key(OptionBits::USE_TONE, b"ni3")
        .expect("toned");
    assert_eq!(toned.pinyin_string(), "ni3");

    // FORCE_TONE: two bytes rejected; a three-byte input already
    // carries its tone digit, so it passes with the key toned (the
    // pin's zero-tone check at `pinyin_parser2.cpp:448` is dead —
    // the digit parse above already refused every non-tone byte).
    assert_eq!(parser.parse_one_key(OptionBits::FORCE_TONE, b"ni"), None);
    assert!(
        parser
            .parse_one_key(OptionBits::USE_TONE | OptionBits::FORCE_TONE, b"ni3")
            .is_some()
    );
    // A non-tone third byte is refused with or without FORCE_TONE.
    assert_eq!(parser.parse_one_key(OptionBits::USE_TONE, b"ni9"), None);
    assert_eq!(
        parser.parse_one_key(OptionBits::USE_TONE | OptionBits::FORCE_TONE, b"ni9"),
        None
    );

    // A four-byte input is refused outright.
    assert_eq!(parser.parse_one_key(OptionBits::empty(), b"niha"), None);
}

/// The double one-key incomplete probe under `PINYIN_INCOMPLETE`
/// (`z` → the initial-only `z` key on MS), refused without the bit.
#[test]
fn double_one_key_incomplete_needs_its_bit() {
    let parser = DoublePinyinParser::with_scheme(DoublePinyinScheme::Ms);
    assert_eq!(parser.parse_one_key(OptionBits::empty(), b"z"), None);
    let key = parser
        .parse_one_key(OptionBits::PINYIN_INCOMPLETE, b"z")
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
    let key = parser
        .parse_one_key(OptionBits::empty(), b"oo")
        .expect("oo parses");
    assert_eq!(key.pinyin_string(), "o");
}

/// `ZhuyinSimpleParser2::parse_one_key` on STANDARD: `18` is ㄅㄚ
/// (ba), the trailing `3` is tone 3 under `USE_TONE`, and
/// `FORCE_TONE` rejects the toneless form while accepting the toned
/// one.
#[test]
fn chewing_one_key_law_on_standard() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::Standard);

    let key = parser
        .parse_one_key(OptionBits::empty(), b"18")
        .expect("ba parses");
    assert_eq!(key.pinyin_string(), "ba");
    assert_eq!(key.shengmu_string(), "b");
    assert_eq!(key.yunmu_string(), "a");
    assert_eq!(key.zhuyin_string(), "ㄅㄚ");

    // `USE_TONE` alone does not demand a tone: `8` is not a tone
    // key on STANDARD, nothing is stripped, and the match carries
    // tone zero. `FORCE_TONE` is what rejects it.
    let toneless = parser
        .parse_one_key(OptionBits::USE_TONE, b"18")
        .expect("ba");
    assert_eq!(toneless.pinyin_string(), "ba");
    assert_eq!(toneless.tone, 0);
    let toned = parser
        .parse_one_key(OptionBits::USE_TONE, b"183")
        .expect("ba3");
    assert_eq!(toned.pinyin_string(), "ba3");
    assert_eq!(toned.zhuyin_string(), "ㄅㄚˇ");

    assert_eq!(
        parser.parse_one_key(OptionBits::USE_TONE | OptionBits::FORCE_TONE, b"18"),
        None
    );
    assert!(
        parser
            .parse_one_key(OptionBits::USE_TONE | OptionBits::FORCE_TONE, b"183")
            .is_some()
    );
}

/// The chewing incomplete row gate: ㄅ alone resolves to the
/// initial-only `b` key only under `ZHUYIN_INCOMPLETE`.
#[test]
fn chewing_one_key_incomplete_needs_its_bit() {
    let parser = ZhuyinParser::with_scheme(ZhuyinScheme::Standard);
    assert_eq!(parser.parse_one_key(OptionBits::empty(), b"1"), None);
    let key = parser
        .parse_one_key(OptionBits::ZHUYIN_INCOMPLETE, b"1")
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
    assert_eq!(standard.parse_one_key(OptionBits::USE_TONE, b""), None);
    assert_eq!(standard.parse_one_key(OptionBits::empty(), b""), None);
    let dvorak = ZhuyinParser::with_scheme(ZhuyinScheme::StandardDvorak);
    assert_eq!(dvorak.parse_one_key(OptionBits::empty(), b"18"), None);
}

#[test]
fn ms_parses_common_spellings() {
    let parser = DoublePinyinParser::new();
    let parsed = parser.parse_with_options(b"nihao", OptionBits::PINYIN_INCOMPLETE);
    assert_eq!(parsed.consumed(), 4);
    assert_eq!(double_full_pinyin(&parsed), "ni'ha");
}

/// The batch law that `docs/findings/double-pinyin-spec.md` (Tone) states:
/// `USE_TONE` lets a three-byte key carry its `1`..=`5` digit as the
/// tone, and `FORCE_TONE` requires an exactly-three-byte key — so
/// `FORCE_TONE` without `USE_TONE` consumes nothing at all.
#[test]
fn batch_parse_honours_use_tone_and_force_tone() {
    let parser = DoublePinyinParser::new();

    // USE_TONE: the three-byte key rides its tone.
    let toned = parser.parse_with_options(b"ni3", OptionBits::USE_TONE);
    assert_eq!(toned.consumed(), 3);
    assert_eq!(toned.keys().len(), 1);
    assert_eq!(toned.keys()[0].key().text(), "ni");
    assert_eq!(toned.keys()[0].tone(), 3);

    // Without USE_TONE the three-byte key is refused and the walk
    // retries length two.
    let toneless = parser.parse_with_options(b"ni3", OptionBits::empty());
    assert_eq!(toneless.consumed(), 2);
    assert_eq!(toneless.keys()[0].tone(), 0);

    // A non-tone third byte is refused even under USE_TONE.
    assert_eq!(
        parser
            .parse_with_options(b"nix", OptionBits::USE_TONE)
            .consumed(),
        2
    );
    assert_eq!(
        parser
            .parse_with_options(b"ni0", OptionBits::USE_TONE)
            .consumed(),
        2
    );

    // FORCE_TONE: a two-byte key is refused.
    assert_eq!(
        parser
            .parse_with_options(b"ni", OptionBits::FORCE_TONE)
            .consumed(),
        0
    );

    // FORCE_TONE without USE_TONE: the three-byte key passes the
    // length gate but the tone parse refuses it — nothing consumed.
    assert_eq!(
        parser
            .parse_with_options(b"ni3", OptionBits::FORCE_TONE)
            .consumed(),
        0
    );

    // USE_TONE | FORCE_TONE: only an exactly-three-byte toned key.
    let forced = parser.parse_with_options(b"ni3", OptionBits::USE_TONE | OptionBits::FORCE_TONE);
    assert_eq!(forced.consumed(), 3);
    assert_eq!(forced.keys()[0].tone(), 3);
    assert_eq!(
        parser
            .parse_with_options(b"ni3ha4", OptionBits::USE_TONE | OptionBits::FORCE_TONE)
            .consumed(),
        6
    );
}

#[test]
fn illegal_first_tone_on_ni_stops_without_retrying_a_shorter_key() {
    let parser = ZhuyinParser::new();
    let rejected = toned(&parser, b"su ");
    assert_eq!(rejected.consumed(), 0);
    assert!(rejected.keys().is_empty());

    let legal = toned(&parser, b"su6");
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

    let untoned = toned(&parser, b"su");
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
    let parsed = toned(&parser, b"sux6");
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
    assert_eq!(toned(&parser, b"6").consumed(), 0);
    assert_eq!(toned(&parser, b" ").consumed(), 0);
}

#[test]
fn incomplete_keys_consume_zero_either_way() {
    let parser = ZhuyinParser::new();
    // With the option off the row is gated at the index; with it on
    // the row matches and the post-match validity mask rejects it.
    // Both consume 0 at this pin — same outcome as upstream.
    assert_eq!(toned(&parser, b"1").consumed(), 0);
    assert_eq!(
        parser
            .parse_with_options(b"1", OptionBits::USE_TONE | OptionBits::ZHUYIN_INCOMPLETE)
            .consumed(),
        0
    );
    assert_eq!(toned(&parser, b"x").consumed(), 0);
    assert_eq!(
        parser
            .parse_with_options(b"x", OptionBits::USE_TONE | OptionBits::ZHUYIN_INCOMPLETE)
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
    assert_eq!(
        zrm.parse_with_options(b"aa", OptionBits::empty())
            .consumed(),
        2
    );
    // ZRM "ni" (n+i) goes through shengmu/yunmu tables.
    assert_eq!(
        zrm.parse_with_options(b"ni", OptionBits::empty())
            .consumed(),
        2
    );

    let suppressed = zrm.with_fallback_suppressed();
    // Fallback is gone — "aa" no longer resolves.
    assert_eq!(
        suppressed
            .parse_with_options(b"aa", OptionBits::empty())
            .consumed(),
        0
    );
    // Shengmu/yunmu are intact — "ni" still works.
    assert_eq!(
        suppressed
            .parse_with_options(b"ni", OptionBits::empty())
            .consumed(),
        2
    );

    // set_scheme clears the suppressed flag.
    let mut parser = suppressed;
    assert!(parser.set_scheme(DoublePinyinScheme::Zrm));
    assert_eq!(
        parser
            .parse_with_options(b"aa", OptionBits::empty())
            .consumed(),
        2
    );
}

// The frozen zhuyin parse pin and its capture fixture
// (`docs/testing/upstream-test-coverage.md`, "Interactive modes", the `test_zhuyin` row).
//
// `fixtures/w4/oracle-zhuyin-parse.txt` was captured from the pinned
// oracle (the recorded capture harness family ZH; pin ref below) by
// driving `pinyin_parse_more_chewings` over authored keystroke lines in
// the shape of upstream's `tests/test_zhuyin.cpp` — stdin-line driven,
// no committed corpus of its own — across every parseable keyboard and
// the three option words that reach the parser. This test replays the
// same battery through oxpinyin's public parser seam and demands
// byte-for-byte agreement, so zhuyin parity is a frozen pin that runs
// on every `cargo test`, not only where the oracle is built.
//
// The replay seam is the one the pinyin facade forwards to
// (`parse_chewing_more`, `ChewingCorrections::Masked`):
// `ZhuyinParser::parse_with_options` with the `USE_TONE` and
// `ZHUYIN_INCOMPLETE` bits read off the captured option word.
// Per key the fixture carries both renderings the oracle
// hands out: the pinyin string (`_ChewingKey::get_pinyin_string`'s law:
// canonical spelling plus the tone digit for a non-zero tone) and the
// zhuyin string (`get_zhuyin_string`'s law: tone marks on tones 2..5).
//
// When the pin moves, regenerate the fixture with
// the recorded capture harness and update `EXPECTED_PIN_REF` — a
// change here is a re-freeze, not a silent drift.

const FIXTURE: &str = include_str!("../../fixtures/w4/oracle-zhuyin-parse.txt");

const EXPECTED_PIN_REF: &str = "libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99\
+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155\
+dbm-tkrzw";

/// The capture schema every line must carry.
const CAPTURE_SCHEMA: &str = "pinyin-capture-v1";

/// One parsed fixture line.
struct CaptureLine {
    case: String,
    input: String,
    flags: OptionBits,
    scheme: u8,
    parse_return: usize,
    parsed_input_length: usize,
    segments: String,
    zhuyin: String,
    remainder: String,
}

/// The capture format's escape decoder (\\, \t, \n, \r, \xNN).
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('x') => {
                let mut hex = String::new();
                for _ in 0..2 {
                    if let Some(digit) = chars.next() {
                        hex.push(digit);
                    }
                }
                let byte = u8::from_str_radix(&hex, 16).unwrap_or(b'?');
                out.push(byte as char);
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn fixture_lines() -> Vec<CaptureLine> {
    let mut lines = Vec::new();
    for raw in FIXTURE.lines() {
        let mut fields = raw.split('\t');
        let mut next = || fields.next().unwrap_or("-");
        assert_eq!(next(), format!("schema={CAPTURE_SCHEMA}"), "schema drift");
        let pin = next().strip_prefix("pin_ref=").expect("pin_ref field");
        assert_eq!(
            pin, EXPECTED_PIN_REF,
            "the fixture's pin must be the recorded pin"
        );
        assert_eq!(next(), "family=ZH", "the fixture must hold only ZH lines");
        let case = next().strip_prefix("case=").expect("case field").to_owned();
        let _api_sequence = next(); // recorded provenance, not replayed
        let input = unescape(next().strip_prefix("input=").expect("input field"));
        let flags = u32::from_str_radix(next().strip_prefix("flags=0x").expect("flags field"), 16)
            .map(OptionBits::from_bits_retain)
            .expect("hex flags");
        let scheme: u8 = next()
            .strip_prefix("scheme=")
            .expect("scheme field")
            .parse()
            .expect("scheme number");
        let parse_return: usize = next()
            .strip_prefix("parse_return=")
            .expect("parse_return field")
            .parse()
            .expect("parse_return number");
        let parsed_input_length: usize = next()
            .strip_prefix("parsed_input_length=")
            .expect("parsed_input_length field")
            .parse()
            .expect("parsed_input_length number");
        let segments = unescape(next().strip_prefix("segments=").expect("segments field"));
        let zhuyin = unescape(next().strip_prefix("zhuyin=").expect("zhuyin field"));
        let remainder = unescape(next().strip_prefix("remainder=").expect("remainder field"));
        lines.push(CaptureLine {
            case,
            input,
            flags,
            scheme,
            parse_return,
            parsed_input_length,
            segments,
            zhuyin,
            remainder,
        });
    }
    assert!(
        lines.len() >= 20,
        "the battery must stay broad ({}) — a shrunken fixture is a re-freeze",
        lines.len()
    );
    lines
}

/// The scheme discriminants the facade's `zhuyin_scheme` mapping accepts,
/// mirrored here so the fixture drives the same keyboards. 7
/// (`StandardDvorak`) is upstream's setter abort and never appears.
fn scheme_for(value: u8) -> ZhuyinScheme {
    match ZhuyinScheme::try_from(i32::from(value)) {
        Ok(ZhuyinScheme::StandardDvorak) | Err(_) => {
            panic!("scheme {value} is outside the parseable keyboards")
        }
        Ok(scheme) => scheme,
    }
}

/// `get_pinyin_string`'s law: the canonical spelling with the tone digit
/// appended for a non-zero tone.
fn pinyin_string(key: &ZhuyinKey) -> String {
    let base = key.key().text();
    match key.tone() {
        0 => base.to_owned(),
        tone => format!("{base}{tone}"),
    }
}

/// The whole battery: parse through the public seam, demand the oracle's
/// consumed length, spans, both renderings, and the unparsed remainder.
#[test]
fn zhuyin_parse_matches_the_frozen_oracle_pin() {
    for line in fixture_lines() {
        let parser = ZhuyinParser::with_scheme(scheme_for(line.scheme));
        let parsed = parser.parse_with_options(
            line.input.as_bytes(),
            line.flags & (OptionBits::USE_TONE | OptionBits::ZHUYIN_INCOMPLETE),
        );

        assert_eq!(
            parsed.consumed(),
            line.parse_return,
            "{}: parse_return must match the oracle",
            line.case
        );
        assert_eq!(
            parsed.consumed(),
            line.parsed_input_length,
            "{}: the oracle's two length reads agree; the replay must too",
            line.case
        );

        let segments: Vec<String> = parsed
            .keys()
            .iter()
            .map(|key| format!("{}@{}:{}", pinyin_string(key), key.start(), key.end()))
            .collect();
        let expected = if line.segments == "-" {
            String::new()
        } else {
            line.segments.clone()
        };
        let actual = if segments.is_empty() {
            String::new()
        } else {
            segments.join(",")
        };
        assert_eq!(
            actual, expected,
            "{}: segments (pinyin@begin:end) must match the oracle",
            line.case
        );

        let zhuyin: Vec<String> = parsed
            .keys()
            .iter()
            .map(crate::parser::ZhuyinKey::display)
            .collect();
        let expected_zhuyin = if line.zhuyin == "-" {
            String::new()
        } else {
            line.zhuyin.clone()
        };
        let actual_zhuyin = if zhuyin.is_empty() {
            String::new()
        } else {
            zhuyin.join(",")
        };
        assert_eq!(
            actual_zhuyin, expected_zhuyin,
            "{}: zhuyin renderings must match the oracle",
            line.case
        );

        assert_eq!(
            &line.input[parsed.consumed()..],
            line.remainder,
            "{}: the unparsed remainder must match the oracle",
            line.case
        );
    }
}

/// The battery must still cover every parseable keyboard and both option
/// bits — a fixture narrowed to the standard layout would silently shrink
/// the pin.
#[test]
fn the_battery_covers_every_keyboard_and_option_word() {
    let lines = fixture_lines();
    for scheme in [1_u8, 2, 3, 4, 5, 6, 8, 9] {
        assert!(
            lines.iter().any(|line| line.scheme == scheme),
            "scheme {scheme} has no case in the fixture"
        );
    }
    assert!(
        lines
            .iter()
            .any(|line| line.flags.contains(OptionBits::USE_TONE)),
        "no case exercises USE_TONE"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.flags.contains(OptionBits::ZHUYIN_INCOMPLETE)),
        "no case exercises ZHUYIN_INCOMPLETE"
    );
    assert!(
        lines.iter().any(|line| !line
            .flags
            .intersects(OptionBits::USE_TONE | OptionBits::ZHUYIN_INCOMPLETE)),
        "no case exercises the plain option word"
    );
}
