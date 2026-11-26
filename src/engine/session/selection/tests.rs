use super::*;

// Basis: phonetic_key_matrix.cpp:603–664 at 074a2219 and the Rust training_readings contract.
#[test]
fn readings_preserve_span_tones_order_and_multiplicity() {
    let Some(key) = SyllableKey::from_text("jin") else {
        panic!("inventory");
    };
    let edge = |from, to, tone| ScanKey {
        key,
        from,
        to,
        syllable_start: from,
        crosses_separator: false,
        tone,
    };
    let matrix = super::super::matrix::ParsedMatrix::from_scan(
        &super::super::matrix::ScanMatrix::from_columns(vec![
            vec![edge(0, 1, 2), edge(0, 1, 3), edge(0, 2, 4)],
            vec![edge(1, 2, 1)],
            vec![],
        ]),
        2,
        false,
    );
    let mut stacks = ReadingStacks::default();
    let readings: Vec<_> = training_readings(&matrix, b"xx", 0, 1, 1, &mut stacks).collect_all();
    assert_eq!(
        readings.iter().map(|r| r[0].tone).collect::<Vec<_>>(),
        [2, 3]
    );
    for (start, end, keys, count) in [(0, 2, 2, 2), (0, 2, 1, 1), (2, 1, 1, 0)] {
        assert_eq!(
            training_readings(&matrix, b"xx", start, end, keys, &mut stacks).count_all(),
            count
        );
    }
    let trailing = super::super::matrix::ParsedMatrix::from_scan(
        &super::super::matrix::ScanMatrix::from_columns(vec![
            vec![edge(0, 1, 2)],
            vec![],
            vec![],
            vec![],
        ]),
        3,
        false,
    );
    for (input, count) in [(b"x''", 1), (b"xxx", 0)] {
        assert_eq!(
            training_readings(&trailing, input, 0, 3, 1, &mut stacks).count_all(),
            count
        );
    }
    // A stored length past `MAX_PHRASE_LENGTH` (a crafted item's header)
    // walks past the stacks' inline bound: two keys per column give 2^n
    // readings of n keys, each yielded whole and in matrix order.
    let n = MAX_PHRASE_LENGTH + 2;
    let long = super::super::matrix::ParsedMatrix::from_scan(
        &super::super::matrix::ScanMatrix::from_columns(
            (0..=n)
                .map(|at| {
                    if at == n {
                        vec![]
                    } else {
                        vec![edge(at, at + 1, 1), edge(at, at + 1, 2)]
                    }
                })
                .collect(),
        ),
        n,
        false,
    );
    let input = vec![b'x'; n];
    let mut long_readings = training_readings(&long, &input, 0, n, n, &mut stacks);
    let first = crate::model::ReadingWalk::next_reading(&mut long_readings).expect("a reading");
    assert_eq!(first.len(), n);
    assert!(first.iter().all(|key| key.tone == 1));
    assert_eq!(long_readings.count_all() + 1, 1 << n);
}

/// The lifecycle suite's selection tests (libchewing test-keyboardless.c).
mod session_lifecycle {
    use crate::engine::Selection;

    use crate::engine::session::tests::select_row;
    use crate::engine::session::tests::session_lifecycle::session;

    // Basis: pinyin.cpp:2577–2587 at 074a2219 and the Rust selection outcome contract.
    #[test]
    fn a_programmatic_choice_retains_the_chosen_phrase() {
        let mut session = session();
        session.replace_raw("ni,hao").expect("typing");
        let chosen = session
            .candidates()
            .iter()
            .next()
            .expect("candidates exist")
            .text()
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(chosen, "你", "the captured order leads with 你");

        assert_eq!(
            select_row(&mut session, 0).expect("choose"),
            Selection::Continued
        );
        assert_eq!(session.composition_offset(), 2);
        assert_eq!(session.selected_tokens().len(), 1);

        assert!(session.is_composing());
    }
}

mod session_replay {
    //! Authored state transitions driven through direct session operations.

    use crate::engine::Selection;

    use crate::engine::session::tests::select_row;
    use crate::engine::session::tests::session_replay::session;

    // Basis: Frozen F-A candidate lists and the Rust session replay contract.
    #[test]
    fn recorded_candidate_scenarios_use_direct_operations() {
        let mut session = session();
        session.replace_raw("zhuan").expect("type");
        let first = session.candidates().iter().next().unwrap();
        assert_eq!(first.text(), "传");
        let first_token = first.token().expect("phrase token");
        assert_eq!(
            select_row(&mut session, 0).expect("select"),
            Selection::Completed
        );
        assert_eq!(session.selected_tokens(), [first_token]);
        assert_eq!(session.composition_offset(), "zhuan".len());
        session.reset();
        assert!(!session.is_composing());
        assert!(session.selected_tokens().is_empty());

        let mut session = self::session();
        session.replace_raw("nihaozhongguo").expect("type");
        let prefix = session.candidates().iter().next().unwrap().token().unwrap();
        assert_eq!(
            select_row(&mut session, 0).expect("select prefix"),
            Selection::Continued
        );
        assert_eq!(session.selected_tokens(), [prefix]);
        assert_eq!(
            &session.raw_input()[session.composition_offset()..],
            "zhongguo"
        );
        assert_eq!(session.candidates().iter().next().unwrap().text(), "中国");
        let tail = session.candidates().iter().next().unwrap().token().unwrap();
        assert_eq!(
            select_row(&mut session, 0).expect("select tail"),
            Selection::Completed
        );
        assert_eq!(session.selected_tokens(), [prefix, tail]);
        assert_eq!(session.composition_offset(), "nihaozhongguo".len());
        session.reset();
        assert!(!session.is_composing());
    }
}
