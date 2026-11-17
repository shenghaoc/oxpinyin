use super::*;

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
    let readings: Vec<_> = training_readings(&matrix, b"xx", 0, 1, 1).collect();
    assert_eq!(
        readings.iter().map(|r| r[0].tone).collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(training_readings(&matrix, b"xx", 0, 2, 2).count(), 2);
    assert_eq!(training_readings(&matrix, b"xx", 0, 2, 1).count(), 1);
    assert_eq!(training_readings(&matrix, b"xx", 2, 1, 1).count(), 0);
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
    assert_eq!(training_readings(&trailing, b"x''", 0, 3, 1).count(), 1);
    assert_eq!(training_readings(&trailing, b"xxx", 0, 3, 1).count(), 0);
}

/// The lifecycle suite's selection tests (libchewing test-keyboardless.c).
mod session_lifecycle {
    use crate::{Selection, SelectionTarget};

    use crate::session::tests::select_row;
    use crate::session::tests::session_lifecycle::session;

    // ── libchewing test-keyboardless.c: programmatic selection ─────────

    #[test]
    fn a_programmatic_choice_retains_the_chosen_phrase() {
        // `cand_choose_word`: choosing by index retains the exact phrase.
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

        // The first choice selects the first segment's phrase and continues
        // the composition with the remaining syllables (chewing puts the
        // chosen phrase into the preedit too).
        assert_eq!(
            select_row(&mut session, 0).expect("choose"),
            Selection::Continued
        );
        assert!(
            session
                .selected_text()
                .as_str()
                .unwrap()
                .starts_with(chosen.as_str()),
            "the chosen phrase is retained in the record: {:?}",
            session.selected_text()
        );

        // Completing the rest and resetting is covered by the session replay
        // suite (`recorded_candidate_scenarios_use_direct_operations`); this
        // port stops at the single-choice invariant the chewing test protects.
        assert!(session.is_composing());
    }

    #[test]
    fn a_mid_composition_choice_targets_the_offset_window() {
        // `test_select_candidate_in_middle`: choosing can address a sub-span
        // of the composition through `candidates_at` + `select_target`,
        // retaining the raw input alongside the selected text.
        let mut session = session();
        session.replace_raw("ni,hao,zhong,guo").expect("typing");
        let full = session.raw_input().to_owned();
        assert!(!full.is_empty());

        // The offset window after the first two syllables offers candidates
        // of its own; the anchored selection moves the composition forward.
        let offset = session
            .normalized_lookup_offset(2)
            .expect("offset in range");
        let window = session
            .candidates_at(offset)
            .expect("a mid-composition window");
        assert!(
            !window.is_empty(),
            "the mid-composition window offers candidates"
        );
        session
            .select_target(&SelectionTarget::of_candidate(
                window.iter().next().expect("the row is in the window"),
                offset,
                None,
            ))
            .expect("the anchored selection succeeds over a live window");
        // The composition stays live and bounded after the anchored choice.
        assert_eq!(session.raw_input(), full);
        assert!(session.composition_offset() <= full.len());
        assert!(!session.selected_text().is_empty());
    }
}

mod session_replay {
    //! Authored state transitions driven through direct session operations.
    //!
    //! The historical F-D scenarios were authored, never captured; their
    //! expected candidates come from the pin's real lists in
    //! `fixtures/foundation/f-a.txt`, restricted to what the mini vocabulary can
    //! reach.
    //!
    //! This is the first cross-platform consumer of the session seam. Nothing in
    //! it is Linux-specific: no paths are discovered, no environment is read, no
    //! `cfg(target_os)` appears anywhere beneath it.

    use crate::Selection;

    use crate::session::tests::select_row;
    use crate::session::tests::session_replay::session;

    /// Candidate and state transitions from the authored F-D scenarios. The
    /// retired key/preedit layer is not needed to exercise these operations.
    #[test]
    fn recorded_candidate_scenarios_use_direct_operations() {
        for (input, first) in [
            ("nihao", "你好"),
            ("xian", "西安"),
            ("nih", "你好"),
            ("chang'an", "长安"),
            ("zhuan", "传"),
            ("qqq", "qqq"),
        ] {
            let mut session = session();
            session.replace_raw(input).expect("type");
            assert_eq!(session.raw_input(), input);
            assert_eq!(session.candidates().iter().next().unwrap().text(), first);
            if input == "xian" {
                assert_eq!(session.candidates().iter().nth(1).unwrap().text(), "西岸");
            }
            if input == "qqq" {
                assert!(
                    session
                        .candidates()
                        .iter()
                        .all(|candidate| candidate.text() != "你好")
                );
            }
            assert_eq!(
                select_row(&mut session, 0).expect("select"),
                Selection::Completed
            );
            assert_eq!(session.selected_text(), first);
            assert_eq!(session.composition_offset(), input.len());
            session.reset();
            assert!(!session.is_composing());
            assert!(session.selected_text().is_empty());
        }

        let mut session = session();
        session.replace_raw("nihaozhongguo").expect("type");
        assert_eq!(
            select_row(&mut session, 0).expect("select prefix"),
            Selection::Continued
        );
        assert_eq!(session.selected_text(), "你好");
        assert_eq!(
            &session.raw_input()[session.composition_offset()..],
            "zhongguo"
        );
        assert_eq!(session.candidates().iter().next().unwrap().text(), "中国");
        assert_eq!(
            select_row(&mut session, 0).expect("select tail"),
            Selection::Completed
        );
        assert_eq!(session.selected_text(), "你好中国");
        session.reset();
        assert!(!session.is_composing());
    }
}
