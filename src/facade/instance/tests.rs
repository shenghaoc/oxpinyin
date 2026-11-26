//! The facade-orchestration laws both C-ABI facades rely on, driven over
//! the committed `fixtures/w3` mini set: the parse-continuation rule a
//! re-parse applies, the lookup-offset validation per parse mode, the
//! key-at and cursor laws, and the reset shapes.
//!
//! Fixture selection follows the compiled backend. Initialization succeeds
//! even after a failed system attach, so it cannot identify a file format.

use crate::facade::ChewingCorrections;

use crate::facade::test_fixture::{pinyin_instance, zhuyin_instance};

// Rust contract: a new instance has no active composition or results.
#[test]
fn a_fresh_instance_is_empty_and_answers_nothing() {
    let (mut context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parsed_len, 0);
    assert!(!inst.bind(&context.runtime).session.is_composing());
    assert!(inst.bind(&context.runtime).key_at(0).is_none());
    assert!(
        !context.train(&inst, 0).expect("fresh train"),
        "no user store and no selection: train refuses"
    );
}

// Rust contract: offset validation uses the active parse length.
#[test]
fn lookup_offset_validation_follows_the_active_mode() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    assert!(
        inst.bind(&context.runtime)
            .validate_lookup_offset(0)
            .is_ok()
    );
    assert!(
        inst.bind(&context.runtime)
            .validate_lookup_offset(5)
            .is_ok()
    );
    assert!(
        inst.bind(&context.runtime)
            .validate_lookup_offset(6)
            .is_err(),
        "past one-past-end"
    );

    let (zhuyin_context, mut zhuyin) = zhuyin_instance();
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .parse_chewing_more("su3cl3", ChewingCorrections::FromOptions),
        6
    );
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .validate_lookup_offset(3)
            .expect("in range"),
        3
    );
    assert_eq!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .validate_lookup_offset(6)
            .expect("terminal"),
        6
    );
    assert!(
        zhuyin
            .bind(&zhuyin_context.runtime)
            .validate_lookup_offset(7)
            .is_err()
    );
}

// Rust contract: begin_parse keeps related buffers in the composition and restarts on divergence.
#[rstest::rstest]
#[case::related("ni", &["nihao", "niha"], false)]
#[case::divergent("nihao", &["women"], true)]
fn reparse_updates_the_active_composition(
    #[case] initial: &str,
    #[case] edits: &[&str],
    #[case] restarted: bool,
) {
    let (context, mut instance) = pinyin_instance();
    let mut view = instance.bind(&context.runtime);
    assert_eq!(view.parse_full_more(initial), initial.len());
    for &input in edits {
        view.begin_parse(input.as_bytes());
        assert_eq!(view.parse_full_more(input), input.len());
        assert_eq!(view.parsed_len, input.len());
        if restarted {
            assert_eq!(view.session.raw_input(), input);
            assert_eq!(view.session.composition_offset(), 0);
        }
    }
}

// Rust contract: parse reset preserves the session and full reset clears it.
#[test]
fn the_parse_reset_keeps_the_session_and_the_full_reset_clears_it() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    inst.bind(&context.runtime).reset_parse_state();
    assert_eq!(inst.bind(&context.runtime).parsed_len, 0);
    assert!(inst.bind(&context.runtime).session.is_composing());
    assert_eq!(inst.bind(&context.runtime).session.raw_input(), "nihao");
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    inst.bind(&context.runtime).full_reset();
    assert_eq!(inst.bind(&context.runtime).parsed_len, 0);
    assert!(!inst.bind(&context.runtime).session.is_composing());
    assert!(inst.bind(&context.runtime).phrase_result.is_empty());
}

mod native {
    use crate::OptionBits;
    use crate::engine::{CandidateKind, Selection};
    use crate::facade::test_fixture::{native_fixture::*, *};
    use crate::facade::{ChewingCorrections, ContextCore, InstanceCore};
    use crate::user::UserConfLaw;

    // Register row 122 gives a leading separator an empty first column.
    #[rstest::rstest]
    #[case::pinyin_luoma(UserConfLaw::Pinyin, 2)]
    #[case::zhuyin_hanyu(UserConfLaw::Zhuyin, 1)]
    fn a_leading_separator_guess_has_no_sentence(#[case] law: UserConfLaw, #[case] scheme: i32) {
        let user = TempUserDir::new("separator-sentence");
        let mut context = native_context(&system_dir(), &user.path, law);
        context.set_options(OptionBits::from_bits_retain(0x18a));
        context.live.full_scheme.set(scheme);
        let mut instance = native_instance(&context, law);
        let consumed = if law == UserConfLaw::Pinyin {
            instance.bind(&context.runtime).parse_full_more("'ni")
        } else {
            instance
                .bind(&context.runtime)
                .parse_full_more_zhuyin("'ni")
        };
        assert_eq!(consumed, 3);
        assert_eq!(instance.read(&context.runtime).parsed_input_length(), 3);
        assert!(guess(&context, &mut instance));
        assert_eq!(sentence(&context, &instance, 0), None);
    }

    // 074a2219 pinyin.cpp:2582–2588 constrains the selected row span.
    #[test]
    fn a_repeat_choose_of_an_accepted_row_forces_its_own_span_again() {
        let start = |tag| {
            let user = TempUserDir::new(tag);
            let context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
            let mut instance = context.alloc_instance();
            assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
            let row = published(&context, &mut instance, 0, "你");
            assert_eq!(row.end, 2);
            assert_ne!(choose(&context, &mut instance, &row), Selection::Refused);
            (user, context, instance, row)
        };
        let (_once_dir, mut once_context, mut once, _) = start("repeat-choose-once");
        let (_twice_dir, mut twice_context, mut twice, row) = start("repeat-choose-twice");
        assert_ne!(choose(&twice_context, &mut twice, &row), Selection::Refused);
        let selected = |context: &ContextCore, instance: &InstanceCore| {
            instance
                .read(&context.runtime)
                .session
                .selected_tokens()
                .to_vec()
        };
        assert_eq!(selected(&twice_context, &twice), [0x0100_1225]);
        assert_eq!(
            selected(&once_context, &once),
            selected(&twice_context, &twice)
        );
        assert!(guess(&once_context, &mut once));
        assert!(guess(&twice_context, &mut twice));
        assert_eq!(sentence(&once_context, &once, 0).as_deref(), Some("你好"));
        assert_eq!(sentence(&twice_context, &twice, 0).as_deref(), Some("你好"));
        let tokens: Vec<u32> = ["你", "好", "号"]
            .into_iter()
            .flat_map(|text| tokens(&twice_context, text))
            .collect();
        let counts = |context: &ContextCore| {
            tokens
                .iter()
                .map(|&token| unigram(context, token))
                .collect::<Vec<_>>()
        };
        let before = counts(&once_context);
        assert_eq!(before, counts(&twice_context));
        assert!(once_context.train(&once, 0).unwrap());
        assert!(twice_context.train(&twice, 0).unwrap());
        assert_ne!(counts(&once_context), before);
        assert_eq!(counts(&once_context), counts(&twice_context));
    }

    // 074a2219 pinyin.cpp:2582–2588 retains a forced phrase for the next lookup.
    #[test]
    fn a_new_sentence_lookup_trains_a_forcing() {
        let user = TempUserDir::new("train-restep");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let mut instance = context.alloc_instance();
        assert_eq!(
            instance.bind(&context.runtime).parse_full_more("zhongguo"),
            8
        );
        assert!(guess(&context, &mut instance));
        let window = native_window(&context, &mut instance, 0);
        let text = window
            .iter()
            .rfind(|row| row.kind() == CandidateKind::Phrase && row.token().is_some())
            .unwrap()
            .text()
            .as_str()
            .unwrap()
            .to_owned();
        let row = published(&context, &mut instance, 0, &text);
        assert_ne!(choose(&context, &mut instance, &row), Selection::Refused);
        assert!(guess(&context, &mut instance));
        assert!(context.train(&instance, 0).unwrap());
    }

    // Rust contract: a committed composition reparses against its active store.
    #[test]
    fn a_selection_committed_composition_reparses_with_its_store() {
        let (context, mut instance) = pinyin_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
        let row = published(&context, &mut instance, 0, "你好");
        assert_eq!(row.end, 5);
        assert_eq!(choose(&context, &mut instance, &row), Selection::Completed);
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_full_more("nihaoshijie"),
            11
        );
        assert!(
            instance
                .bind(&context.runtime)
                .clear_abi_constraint(0, true)
        );
        assert!(
            !instance
                .bind(&context.runtime)
                .clear_abi_constraint(0, true)
        );
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_full_more("nihaoshijie"),
            11
        );
        assert!(
            native_window(&context, &mut instance, 0)
                .iter()
                .any(|row| row.text().as_str() == Some("你"))
        );
    }

    // 074a2219 pinyin.cpp:1497–1525 leaves the previous guess on reparse.
    #[test]
    fn a_reparse_keeps_the_stale_nbest_result_until_the_next_guess() {
        let (context, mut instance) = pinyin_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
        assert!(guess(&context, &mut instance));
        assert_eq!(sentence(&context, &instance, 0).as_deref(), Some("你好"));
        assert_eq!(
            instance.bind(&context.runtime).parse_full_more("zhongguo"),
            8
        );
        assert_eq!(sentence(&context, &instance, 0).as_deref(), Some("你好"));
        let window = native_window(&context, &mut instance, 0);
        let row = window.first().unwrap();
        assert_eq!(
            (row.kind(), row.text().as_str(), row.nbest_row()),
            (CandidateKind::Sentence, Some("你好"), Some(0))
        );
        assert!(guess(&context, &mut instance));
        assert_eq!(sentence(&context, &instance, 0).as_deref(), Some("中国"));
    }

    // Rust contract: reparse uses the typed buffer even with an overlong stored key.
    #[test]
    fn a_reparse_after_a_guess_ignores_an_overlong_index_key() {
        let user = TempUserDir::new("overlong-reparse");
        let first = crate::parser::ChewingKey::from_pinyin("ni")
            .unwrap()
            .to_packed()
            .to_le_bytes();
        craft_user_index(
            &user.path.join("user_pinyin_index.bin"),
            &[(first.to_vec(), vec![]), (first.repeat(17), vec![])],
        );
        write_user_conf(&user.path, &system_dir());
        let context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("hao"), 3);
        native_window(&context, &mut instance, 0);
        assert_eq!(instance.bind(&context.runtime).parse_full_more("ni"), 2);
    }

    // Rust contract: phrase getters enumerate the segmented result.
    #[test]
    fn a_sentence_is_handed_out_phrase_by_phrase() {
        let (context, mut instance) = pinyin_instance();
        assert!(
            instance
                .bind(&context.runtime)
                .parse_full_more("nihaonihao")
                > 0
        );
        assert!(guess(&context, &mut instance));
        let view = instance.read(&context.runtime);
        let phrases = view
            .sentence_phrases(0)
            .unwrap()
            .map(|part| part.text())
            .collect::<Option<Vec<_>>>()
            .unwrap();
        assert!(phrases.len() > 1);
        let joined: String = phrases.iter().filter_map(|part| part.as_str()).collect();
        assert_eq!(
            Some(joined.as_str()),
            view.session.sentence_text(0).unwrap().as_str()
        );
        assert!(!joined.is_empty());
    }

    // Rust contract: the parsed length and matrix length describe the stored parse.
    #[test]
    fn the_length_getters_answer_the_stored_parse_length() {
        let mut context = open_context(crate::facade::PINYIN_DEFAULT_OPTION_WORD);
        let mut instance = context.alloc_instance();
        let lengths = |context: &ContextCore, instance: &InstanceCore| {
            let view = instance.read(&context.runtime);
            (view.pin_parsed_len(), view.pin_matrix_columns())
        };
        context.set_options(OptionBits::PINYIN_INCOMPLETE);
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nih"), 3);
        assert_eq!(lengths(&context, &instance), (3, 4));
        context.set_options(OptionBits::empty());
        native_window(&context, &mut instance, 0);
        assert_eq!(lengths(&context, &instance), (3, 4));
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nih"), 2);
        assert_eq!(lengths(&context, &instance), (2, 3));
        instance.bind(&context.runtime).full_reset();
        assert_eq!(lengths(&context, &instance), (0, 0));
    }

    // Rust contract: a guess reads options captured in the parse matrix.
    #[test]
    fn a_guess_after_an_option_change_reads_the_parse_time_matrix() {
        let mut context = open_context(crate::facade::PINYIN_DEFAULT_OPTION_WORD);
        let mut instance = context.alloc_instance();
        context.set_options(OptionBits::PINYIN_INCOMPLETE);
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nih"), 3);
        assert!(guess(&context, &mut instance));
        let parsed = sentence(&context, &instance, 0).unwrap();
        assert_eq!(parsed.chars().count(), 2);
        context.set_options(OptionBits::empty());
        native_window(&context, &mut instance, 0);
        assert_eq!(instance.read(&context.runtime).parsed_input_length(), 3);
        assert!(guess(&context, &mut instance));
        assert_eq!(sentence(&context, &instance, 0), Some(parsed));
    }

    // Rust contract: a choice without a forcing leaves the full matrix available.
    #[test]
    fn a_guess_after_a_choice_without_a_forcing_decodes_the_whole_matrix() {
        let (context, mut instance) = pinyin_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nihaov"), 5);
        assert!(guess(&context, &mut instance));
        let decoded = sentence(&context, &instance, 0);
        let window = native_window(&context, &mut instance, 0);
        let rank = window.iter().find_map(|row| row.nbest_row()).unwrap();
        assert_ne!(
            instance
                .bind(&context.runtime)
                .select_nbest_rank(rank)
                .unwrap(),
            Selection::Refused
        );
        assert!(guess(&context, &mut instance));
        assert_eq!(sentence(&context, &instance, 0), decoded);
    }

    // Rust contract: segmentation exposes its success flag and sized result array.
    #[rstest::rstest]
    #[case::empty("", true, 0, &[(0,None)])]
    #[case::unmatched("abcd", false, 4, &[(0,Some(0)),(3,Some(0))])]
    fn segment_result(
        #[case] text: &str,
        #[case] success: bool,
        #[case] count: usize,
        #[case] tokens: &[(usize, Option<u32>)],
    ) {
        let (context, mut instance) = pinyin_instance();
        let mut view = instance.bind(&context.runtime);
        assert_eq!(view.segment_phrases(text).unwrap(), success);
        assert_eq!(view.phrase_count(), count);
        for &(index, token) in tokens {
            assert_eq!(view.phrase_token(index), token);
        }
    }
    // Rust contract: full reset clears the segmentation result.
    #[test]
    fn reset_clears_the_phrase_result() {
        let (context, mut instance) = pinyin_instance();
        let mut view = instance.bind(&context.runtime);
        assert_eq!(view.phrase_count(), 0);
        assert!(view.segment_phrases("你好").unwrap());
        assert_eq!(view.phrase_count(), 2);
        view.full_reset();
        assert_eq!(view.phrase_count(), 0);
        assert_eq!(view.phrase_token(0), None);
    }

    // Rust contract: training requires a current result.
    #[test]
    fn training_requires_a_result_and_reset_starts_new_composition() {
        let user = TempUserDir::new("train-doubling");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Zhuyin);
        let mut instance = native_instance(&context, UserConfLaw::Zhuyin);
        let select = |context: &ContextCore, instance: &mut InstanceCore, index: usize| {
            assert_eq!(
                instance
                    .bind(&context.runtime)
                    .parse_chewing_more("su3cl3", ChewingCorrections::FromOptions),
                6
            );
            let window = native_window(context, instance, 0);
            let text = window
                .get(index)
                .unwrap()
                .text()
                .as_str()
                .unwrap()
                .to_owned();
            let row = published(context, instance, 0, &text);
            assert_ne!(choose(context, instance, &row), Selection::Refused);
            row.token
        };
        let t1 = select(&context, &mut instance, 0);
        assert!(!context.train(&instance, 0).unwrap());
        for _ in 0..3 {
            assert!(guess(&context, &mut instance));
            assert!(context.train(&instance, 0).unwrap());
        }
        instance.bind(&context.runtime).full_reset();
        let t2 = select(&context, &mut instance, 1);
        assert_ne!(t1, t2);
        assert!(guess(&context, &mut instance));
        assert!(context.train(&instance, 0).unwrap());
        assert_eq!(
            context
                .user_store()
                .bigram_count(crate::vocab::SENTENCE_START, t2)
                .unwrap(),
            69
        );
        assert_eq!(context.user_store().bigram_count(t1, t2).unwrap(), 0);
    }

    /// Chooses a normal phrase row of the offset-0 window and re-guesses,
    /// so the decoded row carries the forced token at its span.
    fn choose_a_phrase_and_reguess(context: &ContextCore, instance: &mut InstanceCore) {
        let text = native_window(context, instance, 0)
            .iter()
            .find(|row| row.kind() == CandidateKind::Phrase && row.token().is_some())
            .expect("a phrase row")
            .text()
            .as_str()
            .unwrap()
            .to_owned();
        let row = published(context, instance, 0, &text);
        assert_ne!(choose(context, instance, &row), Selection::Refused);
        assert!(guess(context, instance));
    }

    // 074a2219 pinyin.cpp:2684 and phonetic_key_matrix.cpp:661 specify the training assertion sites.
    #[test]
    fn the_training_probe_answers_the_pins_own_aborts() {
        let user = TempUserDir::new("train-probe-true");
        let context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
        assert!(guess(&context, &mut instance));
        assert!(instance.read(&context.runtime).train_aborts(9));
        assert!(!instance.read(&context.runtime).train_aborts(0));
        choose_a_phrase_and_reguess(&context, &mut instance);
        assert_eq!(instance.bind(&context.runtime).parse_full_more("ni"), 2);
        assert!(instance.read(&context.runtime).train_aborts(0));

        let (fixture, mut no_user) = pinyin_instance();
        assert_eq!(no_user.bind(&fixture.runtime).parse_full_more("nihao"), 5);
        assert!(guess(&fixture, &mut no_user));
        assert!(!no_user.read(&fixture.runtime).train_aborts(9));
    }

    // Register row 113 covers a store fault during training. Only a planted
    // user bigram reaches it, so the probe, which answers for the faults a
    // C API call can reach, does not.
    #[test]
    fn a_planted_store_fault_aborts_production_and_not_the_probe() {
        let user = TempUserDir::new("train-probe-store-abort");
        let gram = |tokens: &[u32]| {
            let mut value = 0_u32.to_le_bytes().to_vec();
            for &token in tokens {
                value.extend_from_slice(&token.to_le_bytes());
                value.extend_from_slice(&0_u32.to_le_bytes());
            }
            value
        };
        let (lo, hi) = (2_u32, 0xFFFF_FFF0_u32);
        plant_bigram(
            &user.path,
            &[(
                crate::vocab::SENTENCE_START.to_le_bytes().to_vec(),
                gram(&[lo, hi, lo, hi, lo]),
            )],
        );
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
        assert!(guess(&context, &mut instance));
        choose_a_phrase_and_reguess(&context, &mut instance);
        assert!(!instance.read(&context.runtime).train_aborts(0));
        let result = context.train(&instance, 0);
        assert!(
            matches!(&result, Err(crate::engine::EngineError::TrainingAbort)),
            "{result:?}"
        );
        assert_eq!(
            crate::facade::training_answer(result),
            Err(crate::facade::TrainingRefusal::Abort)
        );
    }

    // Rust contract: probing a training fault preserves the user store.
    #[test]
    fn the_training_probe_observes_nothing_and_leaves_the_store_clean() {
        let user = TempUserDir::new("train-probe-pure");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
        assert!(guess(&context, &mut instance));
        choose_a_phrase_and_reguess(&context, &mut instance);
        let modified = context.user_store().is_modified();
        assert!(!instance.read(&context.runtime).train_aborts(0));
        assert_eq!(context.user_store().is_modified(), modified);
        assert!(
            context
                .user_store()
                .bigram_successors(crate::vocab::SENTENCE_START)
                .unwrap()
                .is_empty()
        );
        assert!(context.train(&instance, 0).unwrap());
        assert!(context.user_store().is_modified());
        assert!(
            !context
                .user_store()
                .bigram_successors(crate::vocab::SENTENCE_START)
                .unwrap()
                .is_empty()
        );
    }
}

#[cfg(feature = "tkrzw")]
mod native_full_data {
    use crate::OptionBits;
    use crate::engine::{CandidateKind, Selection};
    use crate::facade::test_fixture::{native_fixture::*, *};
    use crate::facade::{ContextCore, InstanceCore};
    use crate::user::UserConfLaw;
    const NORMAL: CandidateKind = CandidateKind::Phrase;
    const NBEST: CandidateKind = CandidateKind::Sentence;
    fn row(
        kind: CandidateKind,
        text: &str,
        rank: Option<u8>,
    ) -> (CandidateKind, String, Option<u8>) {
        (kind, text.to_owned(), rank)
    }
    fn rows(
        context: &ContextCore,
        instance: &mut InstanceCore,
        offset: usize,
    ) -> Vec<(CandidateKind, String, Option<u8>)> {
        native_window(context, instance, offset)
            .iter()
            .map(|item| row(item.kind(), item.text().as_str().unwrap(), item.nbest_row()))
            .collect()
    }
    // Frozen observations: deep-choose/nihao@10 and last-choose/shi.
    #[test]
    fn choosing_a_deep_or_last_row_constrains_its_span() {
        struct Case {
            input: &'static str,
            count: u32,
            index: u32,
            text: &'static str,
            cursor: i32,
            window: u32,
            head: Vec<(CandidateKind, String, Option<u8>)>,
        }
        let after_ni = |first: &str| {
            let mut head = vec![
                row(NBEST, &format!("{first}好"), Some(0)),
                row(NBEST, &format!("{first}号"), Some(1)),
                row(NBEST, &format!("{first}豪"), Some(2)),
            ];
            head.extend(["好", "号", "豪", "浩", "毫"].map(|text| row(NORMAL, text, None)));
            head
        };
        let cases = [
            Case {
                input: "nihao",
                count: 126,
                index: 10,
                text: "溺",
                cursor: 2,
                window: 96,
                head: after_ni("溺"),
            },
            Case {
                input: "shi",
                count: 280,
                index: 279,
                text: "篒",
                cursor: 3,
                window: 1,
                head: vec![row(NBEST, "篒", Some(0))],
            },
        ];
        for case in cases {
            let user = TempUserDir::new("pinned-deep-choose");
            let mut context = native_context(
                &contract_data::contract_data_dir(),
                &user.path,
                UserConfLaw::Pinyin,
            );
            context.set_options(OptionBits::from_bits_retain(0x18a));
            let mut instance = context.alloc_instance();
            assert_eq!(
                instance.bind(&context.runtime).parse_full_more(case.input),
                case.input.len()
            );
            assert!(guess(&context, &mut instance));
            let before = rows(&context, &mut instance, 0);
            assert_eq!(before.len(), case.count as usize);
            assert_eq!(before[case.index as usize], row(NORMAL, case.text, None));
            let selected = published(&context, &mut instance, 0, case.text);
            assert_ne!(
                choose(&context, &mut instance, &selected),
                Selection::Refused
            );
            assert_eq!(i32::from(selected.end), case.cursor);
            assert!(guess(&context, &mut instance));
            let window = rows(&context, &mut instance, usize::from(selected.end));
            assert_eq!(window.len(), case.window as usize);
            assert_eq!(window[..case.head.len()], case.head);
            assert_eq!(
                sentence(&context, &instance, 0),
                Some(case.head[0].1.clone())
            );
        }
    }

    // 074a2219 pinyin.cpp:2515–2520 applies the selected tail rank against result zero.
    #[test]
    fn choosing_a_nonzero_nbest_row_trains_the_selected_result() {
        let user = TempUserDir::new("nonzero-nbest");
        let mut context = native_context(
            &contract_data::contract_data_dir(),
            &user.path,
            UserConfLaw::Pinyin,
        );
        context.set_options(OptionBits::from_bits_retain(0x18a));
        let mut instance = context.alloc_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
        assert_eq!(instance.read(&context.runtime).parsed_input_length(), 5);
        assert!(guess(&context, &mut instance));
        let before = rows(&context, &mut instance, 0);
        assert_eq!(before.len(), 126);
        assert_eq!(before[1], row(NORMAL, "你", None));
        let first = published(&context, &mut instance, 0, "你");
        assert_eq!(first.end, 2);
        assert_ne!(choose(&context, &mut instance, &first), Selection::Refused);
        assert!(guess(&context, &mut instance));
        let after = rows(&context, &mut instance, 2);
        assert_eq!(after.len(), 95);
        assert_eq!(
            &after[..2],
            &[row(NBEST, "你好", Some(0)), row(NBEST, "你号", Some(2))]
        );
        assert_eq!(
            instance
                .bind(&context.runtime)
                .select_nbest_rank(after[1].2.unwrap())
                .unwrap(),
            Selection::Completed
        );
        assert!(guess(&context, &mut instance));
        assert_eq!(sentence(&context, &instance, 0).as_deref(), Some("你号"));
        assert!(context.train(&instance, 0).unwrap());
        assert!(
            instance
                .bind(&context.runtime)
                .segment_phrases("你号")
                .unwrap()
        );
        assert_eq!(instance.read(&context.runtime).phrase_count(), 2);
        for (index, token, frequency) in [(0, 16781861, 53370), (1, 16779448, 32211)] {
            assert_eq!(
                instance.read(&context.runtime).phrase_token(index),
                Some(token)
            );
            assert_eq!(unigram(&context, token), Some(frequency));
        }
    }

    // 074a2219 phonetic_lookup.cpp:120–167 revalidates a forcing against the current matrix.
    #[rstest::rstest]
    #[case::pinyin(UserConfLaw::Pinyin, 0x18a)]
    #[case::zhuyin(UserConfLaw::Zhuyin, 0x8)]
    fn a_forcing_over_an_abbreviation_is_revalidated_as_the_pin_does(
        #[case] law: UserConfLaw,
        #[case] options: u32,
    ) {
        let zhuyin = law == UserConfLaw::Zhuyin;
        let user = TempUserDir::new("constraint-revalidation");
        let mut context = native_context(&contract_data::contract_data_dir(), &user.path, law);
        context.set_options(OptionBits::from_bits_retain(options));
        let mut instance = native_instance(&context, law);
        let parse = |instance: &mut InstanceCore, text: &str| {
            let mut view = instance.bind(&context.runtime);
            let consumed = if zhuyin {
                view.parse_full_more_zhuyin(text)
            } else {
                view.parse_full_more(text)
            };
            assert_eq!(consumed, text.len());
            assert_eq!(
                instance.read(&context.runtime).parsed_input_length(),
                text.len()
            );
        };
        let decoded = |instance: &mut InstanceCore, text: &str| {
            parse(instance, text);
            assert!(guess(&context, instance));
            sentence(&context, instance, 0)
        };
        parse(&mut instance, "zhg");
        assert_eq!(
            (0..=3)
                .map(|offset| {
                    instance
                        .read(&context.runtime)
                        .abi_key_at(offset, zhuyin)
                        .unwrap()
                })
                .collect::<Vec<_>>(),
            [Some((23, 0, 2)), None, Some((7, 2, 3)), None]
        );
        assert!(guess(&context, &mut instance));
        assert_eq!(sentence(&context, &instance, 0).as_deref(), Some("中国"));
        let before = rows(&context, &mut instance, 0);
        assert_eq!(before.len(), 2025);
        let second = if zhuyin {
            row(NORMAL, "这个", None)
        } else {
            row(NBEST, "这个", Some(2))
        };
        assert_eq!(
            before[..3],
            [
                row(NBEST, "中国", Some(0)),
                second,
                row(NORMAL, "整个", None)
            ]
        );
        let selected = published(&context, &mut instance, 0, "整个");
        assert_ne!(
            choose(&context, &mut instance, &selected),
            Selection::Refused
        );
        assert_eq!(selected.end, 3);

        assert!(guess(&context, &mut instance));
        assert_eq!(sentence(&context, &instance, 0).as_deref(), Some("整个"));
        let after = rows(&context, &mut instance, 0);
        assert_eq!(after.len(), 2025);
        assert_eq!(
            after[..3],
            [
                row(NBEST, "整个", Some(0)),
                row(NORMAL, "中国", None),
                row(NORMAL, "这个", None)
            ]
        );

        parse(&mut instance, "zh");
        assert_eq!(decoded(&mut instance, "zhg").as_deref(), Some("整个"));

        assert_eq!(decoded(&mut instance, "zh").as_deref(), Some("中"));
        assert_eq!(decoded(&mut instance, "zhg").as_deref(), Some("中国"));
    }
}
