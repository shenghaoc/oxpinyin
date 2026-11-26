//! The fault classification of the cursor and lookup laws, and the
//! candidate-lookup offset of both facades.

use crate::engine::EngineError;

use super::{CursorFault, CursorLaw};
use crate::facade::ChewingCorrections;
use crate::facade::test_fixture::{open_context, pinyin_instance, zhuyin_instance};

// Rust contract: CursorFault::of classifies the engine fault for the calling law.
#[rstest::rstest]
#[case::out_of_range(EngineError::LookupOffsetOutOfRange { offset: 7, len: 5 }, true, true)]
#[case::zero_key(EngineError::ZeroKeyOffsetCheck, true, true)]
#[case::matrix_column(EngineError::PinAssert, false, true)]
fn each_law_aborts_at_its_own_pin_asserts(
    #[case] error: EngineError,
    #[case] check_offset_asserts: bool,
    #[case] character_offset_asserts: bool,
) {
    let fault = |asserts| {
        if asserts {
            CursorFault::PinAssert
        } else {
            CursorFault::Unreadable
        }
    };
    assert_eq!(
        CursorFault::of(&error, CursorLaw::CheckOffset),
        fault(check_offset_asserts)
    );
    assert_eq!(
        CursorFault::of(&error, CursorLaw::CharacterOffset),
        fault(character_offset_asserts)
    );
}

// 074a2219 pinyin.cpp:2226 and zhuyin.cpp:1441–1456 differ at the reserved column.
#[rstest::rstest]
#[case::pinyin(false, 2, 6, Ok(5))]
#[case::zhuyin(true, 3, 7, Err(CursorFault::PinAssert))]
fn candidate_offset_at_the_reserved_column(
    #[case] zhuyin: bool,
    #[case] interior: usize,
    #[case] columns: usize,
    #[case] reserved: Result<usize, CursorFault>,
) {
    let (context, mut instance) = if zhuyin {
        zhuyin_instance()
    } else {
        pinyin_instance()
    };
    let mut view = instance.bind(&context.runtime);
    let consumed = if zhuyin {
        view.parse_chewing_more("su3cl3", ChewingCorrections::FromOptions)
    } else {
        view.parse_full_more("nihao")
    };
    assert_eq!(consumed, columns - 1);
    let read = instance.read(&context.runtime);
    assert_eq!(read.pin_matrix_columns(), columns);
    assert_eq!(read.abi_candidate_offset(interior, zhuyin), Ok(interior));
    assert_eq!(read.abi_candidate_offset(columns, zhuyin), reserved);
    assert_eq!(
        read.abi_candidate_offset(columns + 1, zhuyin),
        Err(CursorFault::PinAssert)
    );
}

// Rust contract: an empty matrix only admits candidate offset zero.
#[test]
fn an_empty_matrix_asserts_on_a_candidate_offset_past_column_zero() {
    let (context, mut inst) = pinyin_instance();
    let read = inst.bind(&context.runtime);
    assert_eq!(read.pin_matrix_columns(), 0);
    assert_eq!(
        read.abi_candidate_offset(1, false),
        Err(CursorFault::PinAssert)
    );
    assert_eq!(
        read.abi_candidate_offset(1, true),
        Err(CursorFault::PinAssert)
    );
}

// From the facade-orchestration laws (see instance/tests.rs).
// Register row 122 distinguishes a typed separator from a syllable key.
#[test]
fn abi_key_at_a_separator_reads_the_zero_key_only_for_zhuyin() {
    let context = open_context(crate::OptionBits::from_bits_retain(0x18a));
    let mut inst = context.alloc_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("'ni'hao"), 7);
    let mut packed = |zhuyin| {
        inst.bind(&context.runtime)
            .abi_key_at(3, zhuyin)
            .expect("no zero-column fault")
            .expect("a key")
            .0
    };
    assert_eq!(packed(true).to_le_bytes(), [0x00, 0x00]);
    assert_eq!(packed(false).to_le_bytes(), [0x86, 0x02]);
}

// Rust contract: cursor movement follows the active parse spans.
#[test]
fn cursor_laws_step_by_key_in_both_modes() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    assert_eq!(
        inst.bind(&context.runtime)
            .lookup_offset(0)
            .expect("cursor 0"),
        0
    );
    assert_eq!(
        inst.bind(&context.runtime)
            .left_offset(2)
            .expect("left of hao"),
        0
    );
    assert_eq!(
        inst.bind(&context.runtime)
            .right_offset(0)
            .expect("right of ni"),
        Some(2)
    );

    let (zhuyin_context, mut zhuyin) = zhuyin_instance();
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_chewing_more("su3cl3", ChewingCorrections::FromOptions),
        6
    );
    let read = zhuyin.bind(&zhuyin_context.runtime);
    let source = read.span_source().expect("a zhuyin parse is active");
    let spans: Vec<_> = source
        .spans
        .iter()
        .map(|key| (key.begin, key.end))
        .collect();
    assert_eq!(spans, [(0, 3), (3, 6)]);
    assert!(!source.separators, "zhuyin holds no zero-key columns");
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .left_offset(3)
            .expect("left of the second key"),
        0
    );
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .right_offset(0)
            .expect("right of the first key"),
        Some(3)
    );
    let ni = zhuyin
        .bind(&zhuyin_context.runtime)
        .key_at(0)
        .expect("first key");
    assert_eq!((ni.text, ni.tone, ni.begin, ni.end), ("ni", 3, 0, 3));
}

mod native {
    use crate::OptionBits;
    use crate::engine::CandidateKind;
    use crate::facade::test_fixture::{native_fixture::*, *};
    use crate::facade::{ChewingCorrections, ContextCore, InstanceCore};
    use crate::user::UserConfLaw;

    fn luoma_xian() -> (ContextCore, InstanceCore) {
        let mut context = open_context(crate::facade::PINYIN_DEFAULT_OPTION_WORD);
        context.set_options(OptionBits::from_bits_retain(0x88));
        context.live.full_scheme.set(2);
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("sian"), 4);
        (context, instance)
    }

    // 074a2219 pinyin.cpp:3138–3191 and zhuyin.cpp:2117 walk the character spans.
    #[rstest::rstest]
    #[case::double(UserConfLaw::Pinyin, 7, "long", "ns", 2, 1, true)]
    #[case::chewing(UserConfLaw::Pinyin, 7, "long", "sj/", 3, 1, false)]
    #[case::zhuyin(UserConfLaw::Zhuyin, 5, "ㄌㄨㄥ", "sj/", 2, 0, false)]
    fn the_character_offset_reads_the_fuzzy_alternative(
        #[case] law: UserConfLaw,
        #[case] library: u8,
        #[case] reading: &str,
        #[case] input: &str,
        #[case] offset: usize,
        #[case] expected: usize,
        #[case] double: bool,
    ) {
        let user = TempUserDir::new("char-offset-fuzzy");
        let mut context = native_context(&system_dir(), &user.path, law);
        assert_eq!(
            import_native(&mut context, library, &[("弄", reading, 1)]),
            [true]
        );
        context.set_options(OptionBits::from_bits_retain(0x8 | 1 << 15));
        context.live.double_scheme.set(2);
        context.live.zhuyin_scheme.set(1);
        let mut instance = native_instance(&context, law);
        let consumed = if double {
            instance.bind(&context.runtime).parse_double_more(input)
        } else {
            instance.bind(&context.runtime).parse_chewing_more(
                input,
                if law == UserConfLaw::Pinyin {
                    ChewingCorrections::Masked
                } else {
                    ChewingCorrections::FromOptions
                },
            )
        };
        assert_eq!(consumed, input.len());
        assert_eq!(
            instance
                .read(&context.runtime)
                .character_offset(&"弄".into(), offset, law == UserConfLaw::Zhuyin)
                .unwrap(),
            (Some(expected), true)
        );
    }

    // Register row 122 retains divided columns in the original input coordinates.
    #[test]
    fn cursor_getters_read_the_divided_column() {
        let (context, instance) = luoma_xian();
        assert_eq!(
            instance
                .read(&context.runtime)
                .abi_left_offset(4, false)
                .unwrap(),
            2
        );

        assert_eq!(
            instance.read(&context.runtime).right_offset(2).unwrap(),
            Some(4)
        );

        assert_eq!(instance.read(&context.runtime).lookup_offset(2).unwrap(), 2);

        let key = instance
            .read(&context.runtime)
            .abi_key_at(2, false)
            .unwrap()
            .unwrap();
        assert_eq!((key.1, key.2), (2, 4));
    }

    // Register row 122 preserves resplit keys on both parse paths.
    #[rstest::rstest]
    fn a_resplit_key_pair_spells_a_phrase_on_the_session_and_index_paths(
        #[values(1, 2)] scheme: i32,
    ) {
        let user = TempUserDir::new("resplit-scheme");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        assert_eq!(
            import_native(&mut context, 7, &[("方案", "fang'an", 1)]),
            [true]
        );
        context.set_options(OptionBits::from_bits_retain(0x108));
        context.live.full_scheme.set(scheme);
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("fangan"), 6);
        assert!(
            native_window(&context, &mut instance, 0)
                .iter()
                .any(|row| row.text().as_str() == Some("方案"))
        );
    }

    // 074a2219 phonetic_lookup.cpp:88–115 clears the forcing at its stored start.
    #[rstest::rstest]
    #[case::hanyu_guess_at_the_key(1, "ni'hao", 3, [false, true])]
    #[case::luoma_guess_at_the_separator(2, "ni'hao", 2, [true, true])]
    fn a_guess_start_is_the_forcings_start_across_a_separator(
        #[case] scheme: i32,
        #[case] input: &str,
        #[case] offset: usize,
        #[case] cleared: [bool; 2],
    ) {
        for (probe, expected) in [2, 3].into_iter().zip(cleared) {
            let (context, mut instance) = pinyin_instance();
            context.live.full_scheme.set(scheme);
            assert_eq!(instance.bind(&context.runtime).parse_full_more(input), 6);
            let row = published(&context, &mut instance, offset, "好");
            assert_eq!(row.end, 6);
            assert_ne!(
                choose(&context, &mut instance, &row),
                crate::engine::Selection::Refused
            );
            assert_eq!(
                instance
                    .bind(&context.runtime)
                    .clear_abi_constraint(probe, true),
                expected
            );
        }
    }

    fn divided_window(scheme: i32, input: &str) -> Vec<String> {
        let user = TempUserDir::new("divided-window");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        assert_eq!(import_native(&mut context, 7, &[("安", "an", 1)]), [true]);
        context.set_options(OptionBits::from_bits_retain(0x88));
        context.live.full_scheme.set(scheme);
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more(input), 4);
        native_window(&context, &mut instance, 2)
            .iter()
            .map(|row| row.text().as_str().unwrap().to_owned())
            .collect()
    }
    // Register row 122 allows lookup inside a divided Luoma key.
    #[test]
    fn a_guess_at_a_divided_column_inside_a_luoma_key_searches_it() {
        let hanyu = divided_window(1, "xian");
        assert!(hanyu.iter().any(|text| text == "安"));
        assert_eq!(divided_window(2, "sian"), hanyu);
    }

    // Rust contract: double-pinyin lookup distinguishes mid-key input offsets.
    #[test]
    fn a_double_pinyin_lookup_identifies_mid_key_columns() {
        let (context, mut instance) = pinyin_instance();
        context.live.double_scheme.set(2);
        assert_eq!(instance.bind(&context.runtime).parse_double_more("nihk"), 4);
        assert!(guess(&context, &mut instance));
        for offset in [0, 2] {
            assert!(!crate::facade::double_offset_is_mid_key(
                instance
                    .read(&context.runtime)
                    .double_parse
                    .as_ref()
                    .unwrap(),
                offset
            ));
            assert!(
                native_window(&context, &mut instance, offset)
                    .iter()
                    .any(|row| row.kind() == CandidateKind::Phrase)
            );
        }
        for offset in [1, 3] {
            assert!(crate::facade::double_offset_is_mid_key(
                instance
                    .read(&context.runtime)
                    .double_parse
                    .as_ref()
                    .unwrap(),
                offset
            ));
        }
    }
}
