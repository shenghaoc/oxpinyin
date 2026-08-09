//! W2-T3 acceptance: run the differential comparison end to end.
//!
//! Replay mode over `fixtures/foundation/f-a.txt`, so this runs on the portable
//! tier with no oracle installed. The oracle side is real output the pinned
//! program already produced, so a pass here means the comparison, the log
//! schemas and the runner all work against genuine data.
//!
//! The live counterpart over the full 10,465-input corpus lives in
//! `tests/differential_live.rs` and needs `--features oracle-ffi`.

use std::path::PathBuf;

use pinyin_oracle::capture;
use pinyin_oracle::differential::{
    self, Case, DIFF_SCHEMA, DIVERGENCE_SCHEMA, ObservationSource, Outcome, Reason, ReplaySource,
    Source, Unclassified,
};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/foundation")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Builds a replay source and the case list covering every F-A record.
fn f_a_run() -> (ReplaySource, Vec<Case>) {
    let observations = capture::read_fixture(&fixture("f-a.txt")).expect("F-A decodes");
    let cases = observations
        .iter()
        .map(|observation| Case {
            corpus: "F-A".to_owned(),
            case: observation.case.clone().unwrap_or_default(),
            input: observation.input.clone(),
        })
        .collect();
    (ReplaySource::new(observations), cases)
}

#[test]
fn every_frozen_f_a_record_is_compared_end_to_end() {
    let (mut source, cases) = f_a_run();
    assert_eq!(source.source(), Source::Replay);

    let output = differential::run(&mut source, &cases, &Unclassified);

    assert_eq!(output.report.total, cases.len());
    assert_eq!(output.report.unobservable, 0);
    assert_eq!(output.comparison_log.len(), cases.len());
}

#[test]
fn the_pinned_oracle_agrees_with_our_parser_on_every_f_a_input() {
    let (mut source, cases) = f_a_run();
    let output = differential::run(&mut source, &cases, &Unclassified);

    assert_eq!(
        output.report.divergences(),
        0,
        "unexpected divergence on frozen F-A:\n{}\n{}",
        output.divergence_log_text(),
        output.report.to_summary()
    );
    assert_eq!(output.report.agreements, cases.len());
    assert!(output.divergence_log.is_empty());
}

#[test]
fn fangan_agrees_at_a_positive_rank_and_the_rest_at_zero() {
    let (mut source, cases) = f_a_run();
    let output = differential::run(&mut source, &cases, &Unclassified);

    // Ranks are recorded for every record, not just divergences. Exactly one
    // frozen F-A input has the oracle choosing other than our first path:
    // fangan, where the oracle selects [fan, gan] and we order [fang, an]
    // first. That single record is the tie-swap population at parse level.
    assert_eq!(
        output.report.ranks.get(&1),
        Some(&1),
        "expected exactly one rank-1 record, got {:?}",
        output.report.ranks
    );
    let rank_zero = output.report.ranks.get(&0).copied().unwrap_or_default();
    assert_eq!(
        rank_zero + 1,
        cases.len(),
        "every other record should be rank 0, got {:?}",
        output.report.ranks
    );

    let fangan = output
        .comparison_log
        .iter()
        .find(|line| line.contains("case=ambiguous-fangan"))
        .expect("fangan is present");
    assert!(fangan.contains("rank=1"), "{fangan}");
    assert!(fangan.contains("outcome=agreement"), "{fangan}");
    assert!(fangan.contains("ours_paths=3"), "{fangan}");
}

#[test]
fn comparison_log_lines_carry_the_documented_schema_and_stay_single_line() {
    let (mut source, cases) = f_a_run();
    let output = differential::run(&mut source, &cases, &Unclassified);

    for line in &output.comparison_log {
        assert!(
            line.starts_with(&format!("schema={DIFF_SCHEMA}\t")),
            "{line}"
        );
        assert!(
            !line.contains('\n') && !line.contains('\r'),
            "a record must occupy one line"
        );
        assert!(line.contains("source=replay"), "{line}");
        assert!(line.contains("corpus=F-A"), "{line}");
        assert!(line.contains("flags=0x0000018a"), "{line}");
    }

    // The 4,096-byte junk input must stay on one escaped line.
    let long = output
        .comparison_log
        .iter()
        .find(|line| line.contains("case=very-long-junk-4096"))
        .expect("long case present");
    assert!(long.contains("theirs_consumed=0"), "{long}");
    assert!(long.contains("ours_consumed=0"), "{long}");
}

#[test]
fn a_replayed_run_is_byte_identical_between_runs() {
    let (mut first, cases) = f_a_run();
    let (mut second, _) = f_a_run();

    let a = differential::run(&mut first, &cases, &Unclassified);
    let b = differential::run(&mut second, &cases, &Unclassified);

    assert_eq!(a.comparison_log_text(), b.comparison_log_text());
    assert_eq!(a.divergence_log_text(), b.divergence_log_text());
}

#[test]
fn f_c_records_are_comparable_and_flag_variation_is_visible() {
    // F-C varies one option bit at a time. The parse-level comparison does not
    // model option bits, so this run is not expected to be divergence-free; it
    // exists to prove the runner handles the family and to record where flag
    // semantics show up. W2-T5 classifies these as flag-semantics.
    let observations = capture::read_fixture(&fixture("f-c.txt")).expect("F-C decodes");
    let cases: Vec<Case> = observations
        .iter()
        .map(|observation| Case {
            corpus: "F-C".to_owned(),
            case: observation.case.clone().unwrap_or_default(),
            input: observation.input.clone(),
        })
        .collect();

    // Records repeat inputs across off/on pairs, so the replay source resolves
    // by input and the run compares as many cases as there are records.
    let mut source = ReplaySource::new(observations);
    let output = differential::run(&mut source, &cases, &Unclassified);

    assert_eq!(output.report.total, cases.len());
    assert_eq!(output.report.unobservable, 0);

    for line in &output.divergence_log {
        assert!(
            line.starts_with(&format!("schema={DIVERGENCE_SCHEMA}\t")),
            "{line}"
        );
        assert!(line.contains("class=unclassified"), "{line}");
    }
    println!("{}", output.report.to_summary());
}

#[test]
fn an_off_pin_record_is_reported_as_a_divergence_not_dropped() {
    // Rewriting the pin ref in a frozen line must surface as off-pin rather
    // than being rejected by the reader.
    let tampered = fixture("f-a.txt")
        .lines()
        .next()
        .expect("first record")
        .replace("dbm-tkrzw", "dbm-berkeleydb");

    let observations = capture::read_fixture(&tampered).expect("still decodes");
    let cases: Vec<Case> = observations
        .iter()
        .map(|observation| Case {
            corpus: "tampered".to_owned(),
            case: "off-pin".to_owned(),
            input: observation.input.clone(),
        })
        .collect();

    let mut source = ReplaySource::new(observations);
    let output = differential::run(&mut source, &cases, &Unclassified);

    assert_eq!(output.report.total, 1);
    assert_eq!(
        output.report.reasons.get(&Reason::OffPin),
        Some(&1),
        "{:?}",
        output.report.reasons
    );
    assert_eq!(output.divergence_log.len(), 1);
    assert!(output.divergence_log[0].contains("reason=off-pin"));
}

#[test]
fn summary_reports_totals_reasons_classes_and_ranks() {
    let (mut source, cases) = f_a_run();
    let output = differential::run(&mut source, &cases, &Unclassified);
    let summary = output.report.to_summary();

    assert!(summary.contains("compared      15"), "{summary}");
    assert!(summary.contains("agreements    15"), "{summary}");
    assert!(summary.contains("divergences   0"), "{summary}");
    assert!(summary.contains("oracle path rank"), "{summary}");
}

#[test]
fn outcome_and_reason_wire_spellings_are_stable() {
    assert_eq!(Outcome::Agreement.as_wire(), "agreement");
    assert_eq!(Outcome::Divergence(Reason::OffPin).as_wire(), "divergence");
    for (reason, wire) in [
        (Reason::OffPin, "off-pin"),
        (Reason::OracleInconsistent, "oracle-inconsistent"),
        (Reason::PositionOverflow, "position-overflow"),
        (Reason::OracleSentinel, "oracle-sentinel"),
        (Reason::OursError, "ours-error"),
        (Reason::ConsumedLength, "consumed-length"),
        (Reason::Remainder, "remainder"),
        (Reason::PathAbsent, "path-absent"),
    ] {
        assert_eq!(reason.as_wire(), wire);
    }
}
