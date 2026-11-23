use crate::parser::{ZhuyinParser, ZhuyinScheme};

use super::*;

// Rust contract: scheme dispatch accepts the header discriminants.
#[test]
fn scheme_dispatch_covers_the_header_discriminants_and_nothing_else() {
    for value in 0..=4 {
        assert_eq!(full_scheme(value).is_some(), (1..=3).contains(&value));
    }
    for value in 0..=10 {
        assert_eq!(zhuyin_scheme(value).is_some(), (1..=9).contains(&value));
    }
    assert_eq!(zhuyin_scheme(7), Some(ZhuyinScheme::StandardDvorak));
    for value in 0..=7 {
        assert_eq!(double_scheme(value).is_some(), (1..=6).contains(&value));
    }
}

// Rust contract: exact_input joins keyboard keys and preserves their spans and tones.
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
use crate::facade::PinyinCorrections;

use crate::facade::test_fixture::{pinyin_instance, zhuyin_instance};

// 074a2219 phonetic_key_matrix.cpp:30–84 places keys at their input columns.
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

// 074a2219 phonetic_key_matrix.cpp:30–84 represents a separator by a zero key.
#[test]
fn full_pinyin_skips_a_consumed_separator_column() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("ni'hao"), 6);
    let hao = inst
        .bind(&context.runtime)
        .key_at(2)
        .expect("the walk lands on `hao`");
    assert_eq!((hao.text, hao.begin, hao.end), ("hao", 3, 6));
}

// Rust contract: one-key and keyboard queries dispatch through the live scheme.
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
    assert!(
        !zhuyin
            .bind(&zhuyin_context.runtime)
            .in_keyboard(b's')
            .is_empty()
    );
    assert!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_one_chewing("su3", ChewingCorrections::FromOptions)
            .is_some()
    );
    assert!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_one_chewing("", ChewingCorrections::FromOptions)
            .is_none()
    );
}

mod native {
    use crate::OptionBits;
    #[cfg(feature = "tkrzw")]
    use crate::facade::test_fixture::native_fixture::*;
    use crate::facade::test_fixture::*;
    use crate::facade::{ChewingCorrections, PINYIN_DEFAULT_OPTION_WORD};
    #[cfg(feature = "tkrzw")]
    use crate::user::UserConfLaw;

    // 074a2219 phonetic_key_matrix.cpp:30–84 consumes separator columns.
    #[test]
    fn an_apostrophe_only_parse_matches_the_pin() {
        let (context, mut instance) = pinyin_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("'"), 1);
        assert_eq!(instance.read(&context.runtime).parsed_input_length(), 1);
    }

    // Rust contract: parsed_input_length stores the parse result until reparse or reset.
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
            crate::engine::Selection::Refused
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

    // Rust contract: the chewing correction policy masks only correction bits.
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
                    .parse_chewing_more(text, ChewingCorrections::Masked),
                expected
            );
        }
    }

    fn texts(
        context: &crate::facade::ContextCore,
        instance: &mut crate::facade::InstanceCore,
    ) -> Vec<String> {
        native_window(context, instance, 0)
            .iter()
            .map(|row| row.text().as_str().unwrap().to_owned())
            .collect()
    }

    // Rust contract: the chewing path builds an exact-key session.
    #[test]
    fn zhuyin_keys_are_not_resegmented_by_the_pinyin_inventory() {
        let mut context = open_context(PINYIN_DEFAULT_OPTION_WORD);
        let mut instance = context.alloc_instance();
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_chewing_more("vu0", ChewingCorrections::Masked),
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

    // Rust contract: an exact chewing key keeps its original segmentation.
    #[test]
    fn zhuyin_only_spellings_do_not_fall_back_to_shorter_pinyin_keys() {
        let (context, mut instance) = pinyin_instance();
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_chewing_more("2p", ChewingCorrections::Masked),
            2
        );
        assert!(
            texts(&context, &mut instance)
                .iter()
                .all(|row| row != "的" && row != "得")
        );
    }

    // Rust contract: parse_full_more forwards live options to the parser and split matrix.
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

    // Rust contract: exact Bopomofo keys drive sentence lookup.
    #[test]
    fn the_zhuyin_pipeline_guesses_a_sentence_from_bopomofo_keys() {
        let (context, mut instance) = pinyin_instance();
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_chewing_more("vu0", ChewingCorrections::Masked),
            3
        );
        assert!(guess(&context, &mut instance));
        assert!(!sentence(&context, &instance, 0).unwrap().is_empty());
    }

    // Register row 122: the full-pinyin guess uses its parse-time matrix.
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
}
