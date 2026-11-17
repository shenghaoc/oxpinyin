//! The facade-orchestration laws both C-ABI facades rely on, driven over
//! the committed `fixtures/w3` mini set: the parse-continuation rule a
//! re-parse applies, the lookup-offset validation per parse mode, the
//! key-at and cursor laws, and the reset shapes.
//!
//! Fixture selection follows the compiled backend. Initialization succeeds
//! even after a failed system attach, so it cannot identify a file format.

use crate::ToneForwarding;

use crate::test_fixture::{pinyin_instance, zhuyin_instance};

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

#[test]
fn lookup_offset_validation_follows_the_active_mode() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    // Plain full pinyin: in range, normalised by the session's own law.
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
            .parse_chewing_more("su3cl3", ToneForwarding::ZhuyinFacade),
        6
    );
    // The zhuyin mode validates against the parse's consumed length.
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

#[test]
fn a_re_parse_that_extends_the_buffer_continues_the_composition() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("ni"), 2);
    // The frontend re-sends the whole buffer every keystroke.
    inst.bind(&context.runtime).begin_parse(b"nihao");
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    assert_eq!(inst.bind(&context.runtime).parsed_len, 5);
    // Backspace: the buffer shrinks into the composition and stays open.
    inst.bind(&context.runtime).begin_parse(b"niha");
    assert_eq!(inst.bind(&context.runtime).parse_full_more("niha"), 4);
    assert_eq!(inst.bind(&context.runtime).parsed_len, 4);
}

#[test]
fn a_divergent_buffer_starts_a_fresh_composition() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    inst.bind(&context.runtime).begin_parse(b"women");
    assert_eq!(inst.bind(&context.runtime).parse_full_more("women"), 5);
    assert_eq!(inst.bind(&context.runtime).session.raw_input(), "women");
    assert_eq!(inst.bind(&context.runtime).session.composition_offset(), 0);
}

#[test]
fn the_parse_reset_keeps_the_session_and_the_full_reset_clears_it() {
    let (context, mut inst) = pinyin_instance();
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    inst.bind(&context.runtime).reset_parse_state();
    assert_eq!(inst.bind(&context.runtime).parsed_len, 0);
    // The parse path resets the composition's parse state only: the raw
    // buffer (which the frontend re-sends every keystroke), the selection
    // record and the constraint store survive.
    assert!(inst.bind(&context.runtime).session.is_composing());
    assert_eq!(inst.bind(&context.runtime).session.raw_input(), "nihao");
    assert_eq!(inst.bind(&context.runtime).parse_full_more("nihao"), 5);
    inst.bind(&context.runtime).full_reset();
    assert_eq!(inst.bind(&context.runtime).parsed_len, 0);
    assert!(!inst.bind(&context.runtime).session.is_composing());
    assert!(inst.bind(&context.runtime).phrase_result.is_empty());
}

mod native {
    use crate::test_fixture::{native_fixture::*, *};
    use crate::{ContextCore, InstanceCore, ToneForwarding};
    use oxpinyin_core::engine::{CandidateKind, Selection};
    use oxpinyin_core::{OptionBits, PhraseToken};
    use oxpinyin_user::UserConfLaw;

    #[rstest::rstest]
    #[case::pinyin_luoma(UserConfLaw::Pinyin, 2)]
    #[case::pinyin_secondary_zhuyin(UserConfLaw::Pinyin, 3)]
    #[case::zhuyin_hanyu(UserConfLaw::Zhuyin, 1)]
    #[case::zhuyin_luoma(UserConfLaw::Zhuyin, 2)]
    #[case::zhuyin_secondary_zhuyin(UserConfLaw::Zhuyin, 3)]
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
                .iter()
                .copied()
                .map(PhraseToken::value)
                .collect::<Vec<_>>()
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

    #[test]
    fn a_choose_at_zero_after_a_null_step_brings_the_sentence_back() {
        let first = |context: &ContextCore, instance: &mut InstanceCore| {
            assert!(
                native_window(context, instance, 0)
                    .iter()
                    .all(|row| row.kind() != CandidateKind::Sentence)
            );
            let row = published(context, instance, 0, "你");
            assert_eq!(row.end, 2);
            assert_ne!(choose(context, instance, &row), Selection::Refused);
            assert!(guess(context, instance));
            sentence(context, instance, 0)
        };
        let (plain_context, mut plain) = pinyin_instance();
        assert_eq!(
            plain.bind(&plain_context.runtime).parse_full_more("nihao"),
            5
        );
        let expected = first(&plain_context, &mut plain);
        assert_eq!(expected.as_deref(), Some("你好"));
        let (context, mut instance) = pinyin_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
        instance
            .bind(&context.runtime)
            .session
            .choose_null_step()
            .unwrap();
        assert!(guess(&context, &mut instance));
        assert_eq!(sentence(&context, &instance, 0), None);
        assert_eq!(first(&context, &mut instance), expected);
    }

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

    #[test]
    fn a_reparse_after_a_guess_ignores_an_overlong_index_key() {
        let user = TempUserDir::new("overlong-reparse");
        let first = oxpinyin_core::ChewingKey::from_pinyin("ni")
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

    #[test]
    fn the_length_getters_answer_the_stored_parse_length() {
        let mut context = open_context(crate::PINYIN_DEFAULT_OPTION_WORD);
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

    #[test]
    fn a_guess_after_an_option_change_reads_the_parse_time_matrix() {
        let mut context = open_context(crate::PINYIN_DEFAULT_OPTION_WORD);
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

    #[test]
    fn empty_segment_succeeds_with_an_empty_result() {
        let (context, mut instance) = pinyin_instance();
        let mut view = instance.bind(&context.runtime);
        assert!(view.segment_phrases("").unwrap());
        assert_eq!(view.phrase_count(), 0);
        assert_eq!(view.phrase_token(0), None);
    }
    #[test]
    fn failed_match_keeps_the_sized_all_null_array() {
        let (context, mut instance) = pinyin_instance();
        let mut view = instance.bind(&context.runtime);
        assert!(!view.segment_phrases("abcd").unwrap());
        assert_eq!(view.phrase_count(), 4);
        assert_eq!(view.phrase_token(0), Some(0));
        assert_eq!(view.phrase_token(3), Some(0));
    }
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

    #[test]
    fn training_requires_a_result_and_reset_starts_new_composition() {
        let user = TempUserDir::new("train-doubling");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Zhuyin);
        let mut instance = native_instance(&context, UserConfLaw::Zhuyin);
        let select = |context: &ContextCore, instance: &mut InstanceCore, index: usize| {
            assert_eq!(
                instance
                    .bind(&context.runtime)
                    .parse_chewing_more("su3cl3", ToneForwarding::ZhuyinFacade),
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
            row.token.value()
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
                .bigram_count(oxpinyin_user::SENTENCE_START, t2)
                .unwrap(),
            69
        );
        assert_eq!(context.user_store().bigram_count(t1, t2).unwrap(), 0);
    }
}

#[cfg(feature = "tkrzw")]
mod native_full_data {
    use crate::test_fixture::{native_fixture::*, *};
    use crate::{ContextCore, InstanceCore};
    use oxpinyin_core::OptionBits;
    use oxpinyin_core::engine::{CandidateKind, Selection};
    use oxpinyin_user::UserConfLaw;
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
    /// S4: the tenth and the last row of the 0x18a list at offset 0, chosen, then the window at the
    /// cursor. A NORMAL row's choose answers `offset + add_constraint(m_begin, m_end)`, the span it
    /// constrains (`pinyin.cpp:2582-2588` at 074a2219; `lookup/phonetic_lookup.cpp:61-86`): 2 for a
    /// `ni` row, 3 for a whole `shi`. The window then leads with the constrained sentences, deduped
    /// by string (`pinyin.cpp:2295-2300`). Observations `deep-choose/nihao@10`, `deep-choose/shi@10`,
    /// `last-choose/nihao` and `last-choose/shi`.
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
                input: "nihao",
                count: 126,
                index: 125,
                text: "袮",
                cursor: 2,
                window: 96,
                head: after_ni("袮"),
            },
            Case {
                input: "shi",
                count: 280,
                index: 10,
                text: "史",
                cursor: 3,
                window: 1,
                head: vec![row(NBEST, "史", Some(0))],
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

    /// D9: pinyin.cpp:2515-2520 applies a selected tail rank against result zero.
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

    /// Constraint revalidation: 整个 forced over its abbreviation `zhg`, one initial-only key per
    /// syllable. Observations `constraint-revalidation/pinyin/zhg-整个` (`PhoneticLookup<2, 3>`,
    /// under 0x18a) and `constraint-revalidation/zhuyin/zhg-整个` (`PhoneticLookup<1, 1>`,
    /// `zhuyin.cpp:348`, parsed as full pinyin under `PINYIN_INCOMPLETE` alone) of the pin capture
    /// of 2026-11-16 (runs 1 and 2 identical), at 074a2219. The chewing parsers refuse an
    /// initial-only key (`zhuyin_parser2.cpp:256-257`), and the default `FORCE_TONE` refuses an
    /// untoned full pinyin (`pinyin_parser2.cpp:185-189`).
    ///
    /// `fill_matrix` puts `zh` at column 0 and `g` at column 2 (`phonetic_key_matrix.cpp:30-84`;
    /// `zhuyin.cpp:1017-1043`). The choose constrains `[0, 3)` (`pinyin.cpp:2578-2588`,
    /// `zhuyin.cpp:1650-1655`; `lookup/phonetic_lookup.cpp:61-86`). `validate_constraint` keeps
    /// the forcing at the next guess (`zhuyin.cpp:886-910`; `phonetic_lookup.cpp:120-167`),
    /// because an incomplete key matches any final (`pinyin_phrase3.h:80-86`), and on the pinyin
    /// facade the one forced path leaves one sentence row
    /// (`lookup/phonetic_lookup.h:552-554,601-612,776-788`). A parse never touches the constraints
    /// (`pinyin.cpp:1497-1525`), so the forcing survives `zh` and `zhg` parsed with no guess
    /// between. A guess on `zh` shrinks the store to 3 and clears the forcing that ends at 3
    /// (`phonetic_lookup.cpp:136-138,145-148`).
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
        // Parses `text` as full pinyin; the parse and the parsed length are its byte length.
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
        // Parses `text` and decodes it; answers sentence 0.
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
        // The zhuyin facade lists one sentence row; the pinyin facade lists the n-best rows.
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

        // Revalidated by the guess.
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

        // A replaced selection: no guess sees the shorter parse.
        parse(&mut instance, "zh");
        assert_eq!(decoded(&mut instance, "zhg").as_deref(), Some("整个"));

        // A guess on the shorter parse clears the forcing.
        assert_eq!(decoded(&mut instance, "zh").as_deref(), Some("中"));
        assert_eq!(decoded(&mut instance, "zhg").as_deref(), Some("中国"));
    }
}
