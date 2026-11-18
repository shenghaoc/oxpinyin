use super::{Cell, ConstraintStore, PhraseSpan};

fn token(value: u32) -> crate::PhraseToken {
    value
}

fn span(start: usize, value: u32, text: &str) -> PhraseSpan {
    PhraseSpan {
        start,
        token: token(value),
        text: text.into(),
    }
}

#[test]
fn add_writes_the_run_and_clear_walks_it_back() {
    let mut store = ConstraintStore::default();
    store.resize(6);
    assert_eq!(store.add(0, 2, token(7), "你".into()), 2);
    assert_eq!(
        store.cell(0),
        Some(&Cell::OneStep {
            token: token(7),
            end: 2,
            text: "你".into()
        })
    );
    assert_eq!(store.cell(1), Some(&Cell::NoSearch { start: 0 }));

    // A hit inside the run clears the whole run.
    assert!(store.clear_by_offset(1));
    assert_eq!(store.cell(0), Some(&Cell::None));
    assert_eq!(store.cell(1), Some(&Cell::None));
    // Now free: false, exactly upstream's defined return.
    assert!(!store.clear_by_offset(1));
}

/// `add_constraint(0, 0, null_token)` (`phonetic_lookup.cpp:61-86`):
/// cell 0 becomes a zero-length null-token ONESTEP over the run it
/// replaces, whose interior cells stay; neither a clear nor a
/// validation can remove it (`:88-118`, `:142-168`), and it is no run of
/// the record. On an empty store it writes nothing. A non-empty forcing
/// that starts at 0 overwrites it: `add_constraint` finds nothing to
/// clear in the empty run and writes its ONESTEP over the cell
/// (`:61-86`).
#[test]
fn a_null_step_replaces_cell_zero_and_nothing_clears_it() {
    let mut empty = ConstraintStore::default();
    empty.add_null_step();
    assert_eq!(empty.cell_count(), 0);

    let mut store = ConstraintStore::default();
    store.resize(6);
    assert_eq!(store.add(0, 2, token(7), "你".into()), 2);
    assert!(store.has_runs());
    store.add_null_step();
    let null = Cell::OneStep {
        token: token(0),
        end: 0,
        text: "".into(),
    };
    assert_eq!(store.cell(0), Some(&null));
    assert_eq!(store.cell(1), Some(&Cell::NoSearch { start: 0 }));
    assert!(store.clear_by_offset(0));
    assert!(store.clear_by_offset(1));
    assert!(
        !store.validate(6, |_, _, _| Ok(false)).expect("validate"),
        "nothing is cleared"
    );
    assert_eq!(store.cell(0), Some(&null));
    assert_eq!(store.cell(1), Some(&Cell::NoSearch { start: 0 }));
    assert!(store.runs().is_empty());
    assert!(store.is_active());
    assert!(!store.has_runs(), "active, with no run to rebuild from");
    assert_eq!(store.add(2, 5, token(8), "好".into()), 3);
    assert!(store.has_runs(), "a run beside the null step counts");
    assert_eq!(store.add(0, 2, token(9), "你".into()), 2);
    assert_eq!(
        store.cell(0),
        Some(&Cell::OneStep {
            token: token(9),
            end: 2,
            text: "你".into()
        }),
        "a forcing that starts at 0 overwrites the null step"
    );
    assert_eq!(store.cell(1), Some(&Cell::NoSearch { start: 0 }));
    assert_eq!(
        store
            .runs()
            .iter()
            .map(|run| (run.0, run.1))
            .collect::<Vec<_>>(),
        [(0, 2), (2, 5)]
    );
    store.resize(0);
    store.resize(6);
    assert!(!store.is_active(), "a shrink to nothing frees it");
}

#[test]
fn add_refuses_an_overrunning_span() {
    let mut store = ConstraintStore::default();
    store.resize(2);
    assert_eq!(store.add(0, 3, token(7), "你".into()), 0);
    assert!(!store.is_active());
}

#[test]
fn clear_out_of_range_answers_false() {
    let mut store = ConstraintStore::default();
    store.resize(2);
    assert!(!store.clear_by_offset(9));
}

#[test]
fn validate_drops_a_run_that_overruns_or_stops_spelling() {
    let mut store = ConstraintStore::default();
    store.resize(6);
    store.add(0, 2, token(7), "你".into());
    store.add(3, 5, token(8), "好".into());

    // Shrink below the second run's end: the second run drops, the
    // first survives (growing never drops).
    let dropped = store
        .validate(4, |_, _, _| Ok(true))
        .expect("probe cannot fail");
    assert!(dropped);
    assert!(store.is_one_step_at(0));
    assert!(!store.is_one_step_at(3));

    // A token that no longer spells over its span drops.
    let dropped = store
        .validate(6, |start, _, value| Ok(value != 7 || start != 0))
        .expect("probe cannot fail");
    assert!(dropped);
    assert!(!store.is_active());
}

#[test]
fn validate_grows_with_free_cells() {
    let mut store = ConstraintStore::default();
    store.resize(3);
    store.add(0, 2, token(7), "你".into());
    let dropped = store
        .validate(8, |_, _, _| Ok(true))
        .expect("probe cannot fail");
    assert!(!dropped);
    assert!(store.is_one_step_at(0));
    assert_eq!(store.cell(7), Some(&Cell::None));
}

/// `add_constraint` clears every run the span overlaps through
/// `clear_constraint` (`phonetic_lookup.cpp:66-68`), so the part of an
/// overlapped run outside the new span goes too — no dangling
/// `OneStep` survives in front of it.
#[test]
fn add_clears_a_partly_overlapped_run_whole() {
    let mut store = ConstraintStore::default();
    store.resize(8);
    store.add(0, 4, token(7), "你好".into());
    assert_eq!(store.add(2, 6, token(8), "好事".into()), 4);
    assert_eq!(store.cell(0), Some(&Cell::None));
    assert_eq!(store.cell(1), Some(&Cell::None));
    assert!(store.is_one_step_at(2));
    assert_eq!(store.cell(5), Some(&Cell::NoSearch { start: 2 }));
    assert_eq!(store.cell(6), Some(&Cell::None));
}

#[test]
fn diff_result_constrains_only_the_differing_phrases() {
    let mut store = ConstraintStore::default();
    store.resize(6);
    // best: 你@[0] 好@[2]; chosen: 你@[0] 浩@[2]
    let best = [span(0, 7, "你"), span(2, 8, "好")];
    let other = [span(0, 7, "你"), span(2, 9, "浩")];
    assert!(store.diff_result(&best, &other, 5));
    assert!(!store.is_one_step_at(0));
    assert!(store.is_one_step_at(2));
    assert_eq!(store.cell(3), Some(&Cell::NoSearch { start: 2 }));
    assert_eq!(store.cell(4), Some(&Cell::NoSearch { start: 2 }));
}

#[test]
fn diff_result_tail_phrase_spans_to_the_tail() {
    let mut store = ConstraintStore::default();
    store.resize(6);
    let best = [span(0, 10, "你好")];
    let other = [span(0, 11, "你浩")];
    assert!(store.diff_result(&best, &other, 5));
    match store.cell(0) {
        Some(Cell::OneStep { end, .. }) => assert_eq!(*end, 5),
        other @ (Some(Cell::None | Cell::NoSearch { .. }) | None) => {
            panic!("expected OneStep at 0, got {other:?}")
        }
    }
}

/// The n-best choose writes without validation (`pinyin.cpp:2515-2520`), so
/// the array can be shorter than the results. The predicate answers the
/// abort of `diff_result` (`phonetic_lookup.cpp:203`) and no other case.
#[test]
fn diff_result_refuses_only_a_differing_phrase_past_the_array() {
    // The results of "nihao": the tail is 5 and the array had 6 cells.
    let best = [span(0, 7, "你"), span(2, 8, "好")];
    let other = [span(0, 7, "你"), span(2, 9, "浩")];
    let earlier = [span(0, 6, "尼"), span(2, 8, "好")];

    // `capi/tests/it/sentence.rs`: a guess on an empty matrix shrinks the
    // array to 0 and keeps the results. The write of [2, 5) answers zero.
    let empty = ConstraintStore::default();
    assert!(empty.diff_result_refuses(&best, &other, 5));
    // Rank 0 differs nowhere, so it writes nothing.
    assert!(!empty.diff_result_refuses(&best, &best, 5));

    // A normal choose after the shorter parse "ni" sizes the array to 3.
    // The stale row that differs over [0, 2) fits, and the pin accepts it.
    let mut shorter = ConstraintStore::default();
    shorter.resize(3);
    assert!(!shorter.diff_result_refuses(&best, &earlier, 5));
    assert!(shorter.diff_result(&best, &earlier, 5));
    assert!(matches!(
        shorter.cell(0),
        Some(Cell::OneStep { end: 2, .. })
    ));
    // The row that differs over [2, 5) ends past the same array.
    assert!(shorter.diff_result_refuses(&best, &other, 5));
}

#[test]
fn runs_report_start_and_end_with_gaps_between() {
    let mut store = ConstraintStore::default();
    store.resize(8);
    store.add(0, 2, token(7), "你".into());
    store.add(4, 7, token(9), "浩".into());
    // diff_result's shape: a free gap [2, 4) between the forcings —
    // the caller fills it from the buffer, the store only reports
    // the runs' own coordinates.
    let runs = store.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0].0, runs[0].1, runs[0].2), (0, 2, 7));
    assert_eq!((runs[1].0, runs[1].1, runs[1].2), (4, 7, 9));

    store.clear_by_offset(4);
    let runs = store.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!((runs[0].0, runs[0].1), (0, 2));

    store.clear_by_offset(0);
    assert!(store.runs().is_empty());
}

#[test]
fn the_validate_boundary_pins_the_reserved_tail_slot() {
    let mut store = ConstraintStore::default();
    store.resize(4);
    // end == len - 1: a run ending at the last real byte — the
    // exact-buffer-end forcing — survives validation.
    store.add(0, 3, token(7), "你".into());
    let dropped = store.validate(4, |_, _, _| Ok(true)).expect("probe ok");
    assert!(!dropped);
    assert!(store.is_one_step_at(0));

    // end == len: add accepts the span (upstream's own asymmetry),
    // validate drops it — the walk would insert on the reserved tail
    // slot, one past the last real column.
    let mut store = ConstraintStore::default();
    store.resize(4);
    assert_eq!(store.add(0, 4, token(7), "你".into()), 4);
    let dropped = store.validate(4, |_, _, _| Ok(true)).expect("probe ok");
    assert!(dropped);
    assert!(!store.is_active());
}
