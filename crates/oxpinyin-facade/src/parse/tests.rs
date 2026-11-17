use oxpinyin_core::{ZhuyinParser, ZhuyinScheme};

use super::*;

#[test]
fn den_matrix_keeps_key_and_fuzzy_deng() {
    let parse = ZhuyinParser::with_scheme(ZhuyinScheme::Standard)
        .parse_with_options(b"2p", OptionBits::USE_TONE);
    let keys = crate::cursor::ModeKeys::Zhuyin(parse.keys())
        .iter()
        .collect::<Vec<_>>();
    let keys = crate::cursor::matrix_keys_of(&keys);
    let matrix =
        oxpinyin_core::engine::parse_time_keys(&keys, 2, OptionBits::PINYIN_AMB_EN_ENG, false);
    assert_eq!(
        matrix
            .iter()
            .map(|k| k.key().pinyin_spelling())
            .collect::<Vec<_>>(),
        ["den", "deng"]
    );
}

#[test]
fn scheme_dispatch_covers_the_header_discriminants_and_nothing_else() {
    assert!((1..=3).all(|v| full_scheme(v).is_some()));
    assert!(full_scheme(0).is_none() && full_scheme(4).is_none());
    // Zhuyin: total over 1..=9 including the STANDARD_DVORAK slot (7).
    assert!((1..=9).all(|v| zhuyin_scheme(v).is_some()));
    assert_eq!(zhuyin_scheme(7), Some(ZhuyinScheme::StandardDvorak));
    assert!(zhuyin_scheme(0).is_none() && zhuyin_scheme(10).is_none());
    assert!((1..=6).all(|v| double_scheme(v).is_some()));
    assert!(double_scheme(0).is_none() && double_scheme(7).is_none());
    assert!(double_scheme(-1).is_none());
}

#[test]
fn exact_input_joins_keys_with_apostrophes_and_spans_each_one() {
    let parse = ZhuyinParser::with_scheme(ZhuyinScheme::Standard)
        .parse_with_options(b"su3cl3", OptionBits::USE_TONE);
    let (text, segments) = exact_input(parse.keys());
    assert_eq!(text, "ni'hao");
    assert_eq!(segments.len(), 2);
    assert_eq!((segments[0].start(), segments[0].end()), (0, 2));
    assert_eq!((segments[1].start(), segments[1].end()), (3, 6));
    assert_eq!(segments[0].tone(), 3);
    assert_eq!(segments[1].tone(), 3);
    let no_keys: &[ZhuyinKey] = &[];
    let (empty, none) = exact_input(no_keys);
    assert!(empty.is_empty() && none.is_empty());
}

// From the facade-orchestration laws (see instance/tests.rs).
use crate::PinyinCorrections;

use crate::test_fixture::{pinyin_instance, zhuyin_instance};

#[test]
fn full_pinyin_parse_consumes_and_keys_sit_at_their_syllable_starts() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    assert_eq!(inst.bind(&context.runtime).parsed_len, 5);
    assert!(inst.bind(&context.runtime).session.is_composing());
    let ni = inst
        .bind(&context.runtime)
        .key_at(0)
        .expect("a key starts at 0");
    assert_eq!((ni.text, ni.begin, ni.end), ("ni", 0, 2));
    let hao = inst
        .bind(&context.runtime)
        .key_at(2)
        .expect("a key starts at 2");
    assert_eq!((hao.text, hao.begin, hao.end), ("hao", 2, 5));
    assert!(
        inst.bind(&context.runtime).key_at(1).is_none(),
        "a mid-syllable column is empty"
    );
    assert!(
        inst.bind(&context.runtime).key_at(5).is_none(),
        "the reserved slot answers nothing"
    );
}

#[test]
fn full_pinyin_skips_a_consumed_separator_column() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("ni'hao"), 6);
    // Offset 2 is the apostrophe: a lone zero-key column the pin walks past.
    let hao = inst
        .bind(&context.runtime)
        .key_at(2)
        .expect("the walk lands on `hao`");
    assert_eq!((hao.text, hao.begin, hao.end), ("hao", 3, 6));
}

#[test]
fn one_key_probes_follow_the_live_schemes() {
    let (context, mut inst) = pinyin_instance();
    assert!(
        inst.bind(&context.runtime)
            .parse_one_full_pinyin("ni", PinyinCorrections::FromOptions)
            .is_some()
    );
    assert!(
        inst.bind(&context.runtime)
            .parse_one_full_pinyin("xyz", PinyinCorrections::FromOptions)
            .is_none()
    );
    let (zhuyin_context, mut zhuyin) = zhuyin_instance();
    // Standard keyboard: `s` is ㄋ; `q` is not a chewing symbol.
    assert!(
        !zhuyin
            .bind(&zhuyin_context.runtime)
            .in_keyboard(b's')
            .is_empty()
    );
    assert!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_one_chewing("su3")
            .is_some()
    );
    assert!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_one_chewing("")
            .is_none()
    );
}

mod native {
    #[cfg(feature = "tkrzw")]
    use crate::test_fixture::native_fixture::*;
    use crate::test_fixture::*;
    use crate::{PINYIN_DEFAULT_OPTION_WORD, ToneForwarding};
    use oxpinyin_core::OptionBits;
    #[cfg(feature = "tkrzw")]
    use oxpinyin_user::UserConfLaw;

    #[rstest::rstest]
    #[case::one("'", 1, 1)]
    #[case::two("''", 2, 2)]
    #[case::three("'''", 3, 3)]
    fn an_apostrophe_only_parse_matches_the_pin(
        #[case] input: &str,
        #[case] consumed: usize,
        #[case] parsed_length: usize,
    ) {
        let (context, mut instance) = pinyin_instance();
        assert_eq!(
            instance.bind(&context.runtime).parse_full_more(input),
            consumed
        );
        assert_eq!(
            instance.read(&context.runtime).parsed_input_length(),
            parsed_length
        );
    }

    #[test]
    fn parsed_input_length_stores_the_parse_result_and_clears_on_reset() {
        let (context, mut instance) = pinyin_instance();
        assert_eq!(instance.read(&context.runtime).parsed_input_length(), 0);
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
        assert_eq!(instance.read(&context.runtime).parsed_input_length(), 5);
        let first = native_window(&context, &mut instance, 0)
            .first()
            .unwrap()
            .text()
            .as_str()
            .unwrap()
            .to_owned();
        let row = published(&context, &mut instance, 0, &first);
        assert_ne!(
            choose(&context, &mut instance, &row),
            oxpinyin_core::engine::Selection::Refused
        );
        assert_eq!(instance.read(&context.runtime).parsed_input_length(), 5);
        let consumed = instance.bind(&context.runtime).parse_full_more("nihaoXYZ");
        assert_eq!(
            instance.read(&context.runtime).parsed_input_length(),
            consumed
        );
        instance.bind(&context.runtime).full_reset();
        assert_eq!(instance.read(&context.runtime).parsed_input_length(), 0);
        assert_eq!(instance.bind(&context.runtime).parse_full_more(""), 0);
        assert_eq!(instance.read(&context.runtime).parsed_input_length(), 0);
    }

    #[test]
    fn the_seam_strips_only_the_zhuyin_corrections() {
        let mut context = open_context(PINYIN_DEFAULT_OPTION_WORD);
        let mut instance = context.alloc_instance();
        for (word, text, expected) in [
            (0x18a | 0x20, "su", 2),
            (0x18a | 0x20, "su3", 3),
            (0x18a | 0x40, "su", 2),
        ] {
            context.set_options(OptionBits::from_bits_retain(word));
            assert_eq!(
                instance
                    .bind(&context.runtime)
                    .parse_chewing_more(text, ToneForwarding::PinFacade),
                expected
            );
        }
    }

    fn texts(context: &crate::ContextCore, instance: &mut crate::InstanceCore) -> Vec<String> {
        native_window(context, instance, 0)
            .iter()
            .map(|row| row.text().as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn zhuyin_keys_are_not_resegmented_by_the_pinyin_inventory() {
        let mut context = open_context(PINYIN_DEFAULT_OPTION_WORD);
        let mut instance = context.alloc_instance();
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_chewing_more("vu0", ToneForwarding::PinFacade),
            3
        );
        let zhuyin = texts(&context, &mut instance);
        for text in ["现", "先"] {
            assert!(zhuyin.iter().any(|row| row == text));
        }
        for text in ["西安", "锡安", "西岸"] {
            assert!(!zhuyin.iter().any(|row| row == text));
        }
        context.set_options(OptionBits::from_bits_retain(0x188));
        assert_eq!(instance.bind(&context.runtime).parse_full_more("xian"), 4);
        assert!(
            texts(&context, &mut instance)
                .iter()
                .any(|row| row == "西安")
        );
    }

    #[test]
    fn zhuyin_only_spellings_do_not_fall_back_to_shorter_pinyin_keys() {
        let (context, mut instance) = pinyin_instance();
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_chewing_more("2p", ToneForwarding::PinFacade),
            2
        );
        assert!(
            texts(&context, &mut instance)
                .iter()
                .all(|row| row != "的" && row != "得")
        );
    }

    #[test]
    fn option_gating_jv_zon_and_divided_table() {
        let mut context = open_context(PINYIN_DEFAULT_OPTION_WORD);
        let mut instance = context.alloc_instance();
        context.set_options(OptionBits::empty());
        for text in ["jv", "zon"] {
            assert_eq!(instance.bind(&context.runtime).parse_full_more(text), 0);
            assert!(!instance.read(&context.runtime).mode_has_keys());
            assert!(texts(&context, &mut instance).is_empty());
            instance.bind(&context.runtime).full_reset();
        }
        for (options, offered) in [(0x8, false), (0x88, true)] {
            context.set_options(OptionBits::from_bits_retain(options));
            assert_eq!(instance.bind(&context.runtime).parse_full_more("xian"), 4);
            assert_eq!(
                texts(&context, &mut instance)
                    .iter()
                    .any(|row| row == "西安"),
                offered
            );
            instance.bind(&context.runtime).full_reset();
        }
    }

    #[test]
    fn the_zhuyin_pipeline_guesses_a_sentence_from_bopomofo_keys() {
        let (context, mut instance) = pinyin_instance();
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_chewing_more("vu0", ToneForwarding::PinFacade),
            3
        );
        assert!(guess(&context, &mut instance));
        assert!(!sentence(&context, &instance, 0).unwrap().is_empty());
    }

    #[cfg(feature = "tkrzw")]
    #[test]
    fn the_resplit_xian_window_lists_single_syllables_only() {
        let user = TempUserDir::new("resplit-xian");
        let mut context = native_context(
            &contract_data::contract_data_dir(),
            &user.path,
            UserConfLaw::Zhuyin,
        );
        context.set_options(OptionBits::from_bits_retain(0x18a));
        let mut instance = native_instance(&context, UserConfLaw::Zhuyin);
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_full_more_zhuyin("xian"),
            4
        );
        let rows = texts(&context, &mut instance);
        assert_eq!(rows[..3], ["县", "见", "线"]);
        for split in ["西安", "西岸", "锡安"] {
            assert!(!rows.iter().any(|row| row == split));
        }
    }

    use super::parser_rows;
    parser_rows::gated_parser_rows! {
        fn gated_parser_rows_keep_consumed_and_stored_lengths(
            #[case] input: &str, #[case] options: u32, #[case] consumed: usize,
            #[case] _keys: &[(u16, u16, u16, &str)], #[case] _auxiliary: &str,
        ) {
            let mut context = open_context(PINYIN_DEFAULT_OPTION_WORD);
            context.set_options(OptionBits::from_bits_retain(options));
            let mut instance = context.alloc_instance();
            assert_eq!(instance.bind(&context.runtime).parse_full_more(input), consumed);
            assert_eq!(instance.read(&context.runtime).parsed_input_length(), consumed);
        }
    }
}

#[path = "../../../oxpinyin-core/tests/common/parser_rows.rs"]
mod parser_rows;
