use super::{Cell, ConstraintStore, PhraseSpan};

fn token(value: u32) -> crate::vocab::PhraseToken {
    value
}

fn span(start: usize, value: u32, text: &str) -> PhraseSpan {
    PhraseSpan {
        start,
        token: token(value),
        text: text.into(),
    }
}

// Basis: phonetic_lookup.cpp:61–117 at 074a2219.
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

    assert!(store.clear_by_offset(1));
    assert_eq!(store.cell(0), Some(&Cell::None));
    assert_eq!(store.cell(1), Some(&Cell::None));
    assert!(!store.clear_by_offset(1));
    assert!(!store.clear_by_offset(6));
}

// Basis: Rust contract of ConstraintStore::clear_by_offset and orphan tracking.
#[test]
fn an_overwritten_middle_is_not_an_orphan_after_its_clear() {
    let mut store = ConstraintStore::default();
    store.resize(10);
    store.add(0, 8, token(7), "你好嗎".into());
    assert!(store.choose_tokenless());
    assert!(store.clear_by_offset(7));

    store.add(3, 5, token(8), "好".into());
    assert!(store.clear_by_offset(3));
    store.add(0, 2, token(9), "你".into());
    assert!(store.clear_by_offset(0));

    assert!(!store.clear_by_offset(3));
    let marker = store.orphaned.as_ref().unwrap();
    assert_eq!(marker.spans, [2..3, 5..8]);
}

// Basis: phonetic_lookup.cpp:120–168 at 074a2219.
#[test]
fn validate_drops_a_run_that_overruns_or_stops_spelling() {
    let mut store = ConstraintStore::default();
    store.resize(6);
    store.add(0, 2, token(7), "你".into());
    store.add(3, 5, token(8), "好".into());

    let dropped = store
        .validate(4, |_, _, _| Ok(true))
        .expect("probe cannot fail");
    assert!(dropped);
    assert!(store.is_one_step_at(0));
    assert!(!store.is_one_step_at(3));

    let dropped = store
        .validate(6, |start, _, value| Ok(value != 7 || start != 0))
        .expect("probe cannot fail");
    assert!(dropped);
    assert!(!store.is_active());
}

// Basis: phonetic_lookup.cpp:120–168 at 074a2219.
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

// Basis: phonetic_lookup.cpp:61–117 at 074a2219.
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

// Basis: phonetic_lookup.cpp:172–205 at 074a2219.
#[test]
fn diff_result_constrains_only_the_differing_phrases() {
    let best = [span(0, 7, "你"), span(2, 8, "好")];
    for (other, changed, end) in [
        ([span(0, 7, "你"), span(2, 9, "浩")], 1, 5),
        ([span(0, 6, "尼"), span(2, 8, "好")], 0, 2),
    ] {
        let mut store = ConstraintStore::default();
        store.resize(6);
        assert!(store.diff_result(&best, &other, 5));
        let span = &other[changed];
        assert_eq!(
            store.runs(),
            [(span.start, end, span.token, span.text.clone())]
        );
        assert_eq!(
            store.cell(end - 1),
            Some(&Cell::NoSearch { start: span.start })
        );
    }
}

// Basis: phonetic_lookup.cpp:172–205 at 074a2219.
#[test]
fn diff_result_refuses_only_a_differing_phrase_past_the_array() {
    let best = [span(0, 7, "你"), span(2, 8, "好")];
    let other = [span(0, 7, "你"), span(2, 9, "浩")];
    let earlier = [span(0, 6, "尼"), span(2, 8, "好")];

    let empty = ConstraintStore::default();
    assert!(!empty.diff_result_refuses(&best, &best, 5));

    let mut shorter = ConstraintStore::default();
    shorter.resize(3);
    assert!(!shorter.diff_result_refuses(&best, &earlier, 5));
    assert!(shorter.diff_result(&best, &earlier, 5));
    assert!(matches!(
        shorter.cell(0),
        Some(Cell::OneStep { end: 2, .. })
    ));
    assert!(shorter.diff_result_refuses(&best, &other, 5));
}

// Basis: Rust contract of ConstraintStore::runs.
#[test]
fn runs_report_start_and_end_with_gaps_between() {
    let mut store = ConstraintStore::default();
    store.resize(8);
    store.add(0, 2, token(7), "你".into());
    store.add(4, 7, token(9), "浩".into());
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

// Basis: phonetic_lookup.cpp:61–85,120–168 at 074a2219.
#[test]
fn the_validate_boundary_pins_the_reserved_tail_slot() {
    for (end, added, dropped, active) in [
        (3, 3, false, true),
        (4, 4, true, false),
        (5, 0, false, false),
    ] {
        let mut store = ConstraintStore::default();
        store.resize(4);
        assert_eq!(store.add(0, end, token(7), "你".into()), added);
        assert_eq!(store.validate(4, |_, _, _| Ok(true)), Ok(dropped));
        assert_eq!(store.is_active(), active);
    }
}
