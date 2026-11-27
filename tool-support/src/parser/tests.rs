use super::*;
use oxpinyin::OptionBits;

#[test]
fn direct_spans_keep_skipped_separators_and_partial_failure() {
    let parsed = parse_direct(
        b"ni3'' hao3  invalid",
        OptionBits::USE_TONE,
        oxpinyin::engine::Law::Pinyin,
    );
    assert_eq!(parsed.consumed, 12);
    assert_eq!(
        parsed
            .keys
            .iter()
            .map(|key| key.span.clone())
            .collect::<Vec<_>>(),
        [0..3, 6..10]
    );
    assert_eq!(parsed.keys[0].key.pinyin_string(), "ni3");
    let corrected = parse_direct(
        b"bagn n",
        OptionBits::USE_TONE | OptionBits::PINYIN_CORRECT_ALL | OptionBits::PINYIN_INCOMPLETE,
        oxpinyin::engine::Law::Pinyin,
    );
    assert_eq!(corrected.consumed, 6);
    assert_eq!(corrected.keys[0].key.pinyin_string(), "bang");
    assert_eq!(corrected.keys[1].key.pinyin_string(), "n");
}

#[test]
fn zhuyin_spans_are_utf8_bytes_from_the_shipping_key_parser() {
    let input = "ㄋㄧˇ' ㄏㄠˇ  bad";
    let parsed = parse_direct(
        input.as_bytes(),
        OptionBits::USE_TONE,
        oxpinyin::engine::Law::Zhuyin,
    );
    assert_eq!(parsed.consumed, 20);
    assert_eq!(
        parsed
            .keys
            .iter()
            .map(|key| key.span.clone())
            .collect::<Vec<_>>(),
        [0..8, 10..18]
    );
    let shipping = oxpinyin::parser::parse_zhuyin_direct(input, OptionBits::USE_TONE);
    assert_eq!(shipping.len(), parsed.keys.len());
    for ((key, tone), with_span) in shipping.iter().zip(parsed.keys) {
        assert_eq!(
            ChewingKey::from_pinyin(key.text())
                .unwrap()
                .with_tone(*tone),
            with_span.key
        );
    }
}

#[test]
fn fuzzy_alternatives_compose_an_initial_bit_then_a_final_bit() {
    use oxpinyin::OptionBits;

    let can = SyllableKey::from_text("can").expect("can");
    let both = OptionBits::PINYIN_AMB_C_CH | OptionBits::PINYIN_AMB_AN_ANG;
    let texts: Vec<&str> = fuzzy_alternatives(can, both)
        .iter()
        .map(|k| k.text())
        .collect();
    assert!(
        texts.contains(&"chan") && texts.contains(&"cang") && texts.contains(&"chang"),
        "can under C_CH+AN_ANG must yield chan, cang, and chained chang, got {texts:?}"
    );

    let lan = SyllableKey::from_text("lan").expect("lan");
    let ln_an = OptionBits::PINYIN_AMB_L_N | OptionBits::PINYIN_AMB_AN_ANG;
    let texts: Vec<&str> = fuzzy_alternatives(lan, ln_an)
        .iter()
        .map(|k| k.text())
        .collect();
    assert!(
        texts.contains(&"nan") && texts.contains(&"lang") && texts.contains(&"nang"),
        "lan under L_N+AN_ANG must yield nan, lang, and chained nang, got {texts:?}"
    );
}

#[test]
fn fuzzy_alternatives_swap_in_and_ing() {
    use oxpinyin::OptionBits;

    let in_ing = OptionBits::PINYIN_AMB_IN_ING;
    for (from, to) in [("jin", "jing"), ("jing", "jin"), ("bin", "bing")] {
        let key = SyllableKey::from_text(from).expect(from);
        let texts: Vec<&str> = fuzzy_alternatives(key, in_ing)
            .iter()
            .map(|k| k.text())
            .collect();
        assert_eq!(texts, [to], "{from} under AMB_IN_ING");
    }
}
