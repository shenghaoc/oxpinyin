//! W2-T3 acceptance: run the whole parity corpus against the live pinned oracle.
//!
//! Needs `--features oracle-ffi` and a prefix built by
//! `tools/oracle/build-oracle.sh`:
//!
//! ```bash
//! PKG_CONFIG_PATH=$HOME/.local/opt/pinyin-oracle/lib/pkgconfig \
//! cargo test -p pinyin-oracle --features oracle-ffi --test differential_live
//! ```
//!
//! These assertions deliberately pin *measured* numbers. A change in any of them
//! means parity moved, which is exactly the event this workstream exists to
//! detect. When a number legitimately changes, update it in the same commit as
//! the reason.
#![cfg(feature = "oracle-ffi")]

use std::sync::OnceLock;

use pinyin_oracle::capture::segments_to_wire;
use pinyin_oracle::corpus;
use pinyin_oracle::differential::{self, Case, Reason, Unclassified};
use pinyin_oracle::{LiveSource, Oracle, OracleFlags, OraclePrefix, OracleSegment};

/// Total corpus size, frozen in `docs/findings/parity-corpus.md`.
const CORPUS_SIZE: usize = 10_465;

fn oracle() -> Oracle {
    let prefix = OraclePrefix::locate().unwrap_or_else(|error| {
        panic!(
            "{error}\n\nBuild the oracle first:\n  \
             bash tools/oracle/build-oracle.sh \
             --prefix \"$HOME/.local/opt/pinyin-oracle\" --jobs \"$(nproc)\""
        )
    });
    Oracle::open_with_temp_user_dir(prefix).expect("pin-verified prefix opens")
}

fn corpus_cases() -> Vec<Case> {
    corpus::generate()
        .into_iter()
        .flat_map(|stratum| {
            stratum
                .inputs
                .into_iter()
                .enumerate()
                .map(move |(index, input)| Case {
                    corpus: stratum.file_name.to_owned(),
                    case: format!("{}:{index}", stratum.file_name),
                    input: input.into_bytes(),
                })
        })
        .collect()
}

fn execute_corpus() -> differential::RunOutput {
    let mut oracle = oracle();
    let session = oracle
        .session(OracleFlags::DEFAULT)
        .expect("session allocates");
    let mut source = LiveSource::new(session);
    differential::run(&mut source, &corpus_cases(), &Unclassified)
}

/// One live corpus run shared by every assertion that inspects it.
///
/// The run is deterministic, proven by `a_live_run_is_deterministic`, which does
/// execute it twice. Repeating a 10,465-input oracle run per test would add
/// minutes for no extra coverage.
fn run_corpus() -> &'static differential::RunOutput {
    static RUN: OnceLock<differential::RunOutput> = OnceLock::new();
    RUN.get_or_init(execute_corpus)
}

#[test]
fn the_whole_corpus_runs_end_to_end_without_aborting() {
    // The acceptance criterion. Reaching the assertions at all means the process
    // survived every input, which is not free: the pin aborts on apostrophe-only
    // input, and the guard in `Session::collect_segments` is what makes this
    // terminate. See docs/findings/oracle-apostrophe-abort.md.
    let output = run_corpus();

    assert_eq!(output.report.total, CORPUS_SIZE);
    assert_eq!(
        output.report.unobservable, 0,
        "every corpus input must be observable"
    );
    assert_eq!(output.comparison_log.len(), CORPUS_SIZE);
    assert_eq!(
        output.report.agreements + output.report.divergences(),
        CORPUS_SIZE
    );
}

#[test]
fn measured_divergence_counts_are_pinned() {
    let output = run_corpus();

    assert_eq!(
        output.report.agreements,
        9_974,
        "agreement count moved\n{}",
        output.report.to_summary()
    );
    assert_eq!(
        output.report.divergences(),
        491,
        "divergence count moved\n{}",
        output.report.to_summary()
    );

    // 483 of the 491 share one root cause: the pin admits initial-only keys at
    // any position and repeatedly, which the frozen parser SPEC forbids. See
    // docs/findings/parser-spec-contradiction-incomplete-keys.md.
    assert_eq!(output.report.reasons.get(&Reason::PathAbsent), Some(&481));
    assert_eq!(output.report.reasons.get(&Reason::ConsumedLength), Some(&4));
    assert_eq!(output.report.reasons.get(&Reason::OracleSentinel), Some(&6));

    // Nothing off-pin, self-inconsistent, or overflowing, and our parser never
    // errored on a corpus input.
    for absent in [
        Reason::OffPin,
        Reason::OracleInconsistent,
        Reason::PositionOverflow,
        Reason::OursError,
        Reason::Remainder,
    ] {
        assert_eq!(
            output.report.reasons.get(&absent),
            None,
            "unexpected {} divergence",
            absent.as_wire()
        );
    }
}

#[test]
fn the_tie_swap_population_is_measured_not_assumed() {
    let output = run_corpus();

    let rank_zero = output.report.ranks.get(&0).copied().unwrap_or_default();
    assert_eq!(rank_zero, 9_506);

    // Agreements at a positive rank: the oracle chose a path we enumerate, just
    // not the one our greedy order puts first. This is the tie-swap population
    // W2-T5 budgets, and at parse level it is far above 0.5% of the corpus.
    let positive_rank = output.report.agreements - rank_zero;
    assert_eq!(positive_rank, 468);
    assert!(
        positive_rank * 200 > CORPUS_SIZE,
        "expected the parse-level tie-swap rate to exceed 0.5%"
    );
}

#[test]
fn the_abort_guard_fires_exactly_on_apostrophe_only_inputs() {
    let mut oracle = oracle();

    for input in ["'", "''", "'''"] {
        let observed = oracle
            .observe(input.as_bytes(), OracleFlags::DEFAULT)
            .expect("guarded, so observable");
        assert_eq!(
            observed.segments,
            vec![OracleSegment::NoKeyColumns {
                parsed_len: input.len()
            }],
            "input {input:?}"
        );
        assert!(observed.has_sentinel_segment());
        assert_eq!(observed.parsed_input_length, input.len());
    }

    // Once any lowercase letter is present a key column exists, so the walk runs
    // and the guard must not fire.
    for input in ["'ni", "a'a", "ni'h", "xi'an", "q"] {
        let observed = oracle
            .observe(input.as_bytes(), OracleFlags::DEFAULT)
            .expect("observable");
        assert!(
            !observed
                .segments
                .iter()
                .any(|segment| matches!(segment, OracleSegment::NoKeyColumns { .. })),
            "guard fired on {input:?}: {}",
            segments_to_wire(&observed.segments)
        );
    }
}

#[test]
fn the_live_f_e_01_shape_is_recorded_rather_than_crashing() {
    // ni' yields a key column with no usable pinyin string. Until the corpus run
    // this shape was modelled but unobserved.
    let mut oracle = oracle();
    for input in ["ni'", "ni'!", "ni'i"] {
        let observed = oracle
            .observe(input.as_bytes(), OracleFlags::DEFAULT)
            .expect("observable");
        assert_eq!(
            segments_to_wire(&observed.segments),
            "ni@0:2:complete,<missing-pinyin>@2",
            "input {input:?}"
        );
    }
}

#[test]
fn the_frozen_spec_contradiction_reproduces_on_named_inputs() {
    // Worked examples cited in
    // docs/findings/parser-spec-contradiction-incomplete-keys.md. Pinned here so
    // the finding cannot drift from the pin's actual behaviour.
    let mut oracle = oracle();
    let observe = |oracle: &mut Oracle, input: &str| {
        segments_to_wire(
            &oracle
                .observe(input.as_bytes(), OracleFlags::DEFAULT)
                .expect("observable")
                .segments,
        )
    };

    assert_eq!(
        observe(&mut oracle, "yingchon"),
        "ying@0:4:complete,ch@4:6:partial,o@6:7:complete,n@7:8:partial"
    );
    assert_eq!(
        observe(&mut oracle, "fanrufe"),
        "fan@0:3:complete,ru@3:5:complete,f@5:6:partial,e@6:7:complete"
    );
    assert_eq!(
        observe(&mut oracle, "ngannve"),
        "n@0:1:partial,gan@1:4:complete,nve@4:7:complete"
    );

    // An initial-only key chained across the whole input.
    let zzz = observe(&mut oracle, "zzzzzzzz");
    assert_eq!(zzz.split(',').count(), 8);
    assert!(zzz.split(',').all(|segment| segment.ends_with(":partial")));
}

#[test]
fn a_live_run_is_deterministic() {
    let first = execute_corpus();
    let second = execute_corpus();
    assert_eq!(
        first.comparison_log_text(),
        second.comparison_log_text(),
        "two live runs must produce identical logs"
    );
    assert_eq!(first.divergence_log_text(), second.divergence_log_text());
}
