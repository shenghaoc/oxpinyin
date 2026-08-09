//! W2-T5 acceptance on the portable tier: the classifier over frozen fixtures.
//!
//! Runs with no oracle installed, so the taxonomy stays under test on every
//! host. The live counterpart, which pins the whole-corpus class roll-up, is in
//! `tests/differential_live.rs`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pinyin_oracle::capture;
use pinyin_oracle::differential::{self, Case, ReplaySource};
use pinyin_oracle::{DivergenceClass, OracleFlags, Taxonomy, taxonomy};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/foundation")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Classifies a whole fixture family, returning per-class counts.
fn classify_family(name: &str, family: &str) -> (BTreeMap<&'static str, usize>, usize) {
    let observations = capture::read_fixture(&fixture(name)).expect("fixture decodes");
    let cases: Vec<Case> = observations
        .iter()
        .map(|observation| Case {
            corpus: family.to_owned(),
            case: observation.case.clone().unwrap_or_default(),
            input: observation.input.clone(),
        })
        .collect();

    let mut source = ReplaySource::new(observations);
    let output = differential::run(&mut source, &cases, &Taxonomy::default());
    (output.report.classes.clone(), output.report.total)
}

#[test]
fn f_a_classifies_as_success_plus_one_tie_swap() {
    let (classes, total) = classify_family("f-a.txt", "F-A");
    assert_eq!(total, 15);

    // Every F-A record agrees. Exactly one, ambiguous-fangan, agrees at a
    // positive rank, so it is the family's whole tie-swap population.
    assert_eq!(classes.get("output-identical"), Some(&14), "{classes:?}");
    assert_eq!(classes.get("tie-swap"), Some(&1), "{classes:?}");
    assert_eq!(
        classes.len(),
        2,
        "no other class should appear: {classes:?}"
    );
}

#[test]
fn f_c_divergences_are_all_flag_semantics() {
    let (classes, total) = classify_family("f-c.txt", "F-C");
    assert_eq!(total, 46);

    // F-C varies one option bit at a time against the IS_PINYIN baseline. Our
    // parser takes no option word, so every divergence here is two engines
    // being asked different questions, never a path-set disagreement.
    assert_eq!(classes.get("flag-semantics"), Some(&10), "{classes:?}");
    assert_eq!(classes.get("path-set"), None, "{classes:?}");
    assert_eq!(classes.get("ours-bug"), None, "{classes:?}");

    let agreements: usize = ["output-identical", "tie-swap"]
        .iter()
        .filter_map(|class| classes.get(class))
        .sum();
    assert_eq!(agreements, 36);
}

#[test]
fn flag_semantics_depends_on_the_configured_parity_profile() {
    // Re-classifying F-C *as if* the baseline were the parity profile turns the
    // same records into path-set. The classifier takes the profile as data, so
    // this is a configuration question rather than a hard-coded verdict.
    let observations = capture::read_fixture(&fixture("f-c.txt")).expect("decodes");
    let cases: Vec<Case> = observations
        .iter()
        .map(|observation| Case {
            corpus: "F-C".to_owned(),
            case: observation.case.clone().unwrap_or_default(),
            input: observation.input.clone(),
        })
        .collect();

    let mut source = ReplaySource::new(observations);
    let output = differential::run(&mut source, &cases, &Taxonomy::new(OracleFlags::BASELINE));

    // Under the baseline profile the off-baseline records become flag-semantics
    // and the baseline ones become path-set, so both classes now appear.
    assert!(
        output.report.classes.contains_key("path-set"),
        "{:?}",
        output.report.classes
    );
}

#[test]
fn budget_over_f_a_is_reported_with_both_readings() {
    let (classes, total) = classify_family("f-a.txt", "F-A");
    let verdict = taxonomy::budget(total, &classes);

    // 0.5% of 15 rounds down to zero, so a curated adversarial family cannot
    // satisfy the budget under either reading. The budget is a corpus-scale
    // measure; asserting that here documents why it is not applied to fixtures.
    assert_eq!(verdict.limit, 0);
    assert_eq!(verdict.output_identical, 14);
    assert_eq!(verdict.tie_swap, 1);
    assert!(!verdict.literal_pass());
    assert!(!verdict.divergence_only_pass());
}

#[test]
fn every_class_is_reachable_and_carries_a_worked_example() {
    // The taxonomy finding requires a worked example per class. The unit tests in
    // src/taxonomy.rs hold them; this asserts the class set itself has not
    // silently grown or shrunk, which would leave a class undocumented.
    assert_eq!(DivergenceClass::ALL.len(), 8);
    for class in DivergenceClass::ALL {
        assert!(!class.as_wire().is_empty());
        assert_eq!(class.to_string(), class.as_wire());
    }

    // The two auto-accepted classes are the ones the budget constrains.
    let auto: Vec<&str> = DivergenceClass::ALL
        .iter()
        .filter(|class| class.is_auto_accepted())
        .map(|class| class.as_wire())
        .collect();
    assert_eq!(auto, vec!["output-identical", "tie-swap"]);
}

#[test]
fn class_field_replaces_the_placeholder_in_log_lines() {
    let (_, _) = classify_family("f-a.txt", "F-A");

    let observations = capture::read_fixture(&fixture("f-a.txt")).expect("decodes");
    let cases: Vec<Case> = observations
        .iter()
        .map(|observation| Case {
            corpus: "F-A".to_owned(),
            case: observation.case.clone().unwrap_or_default(),
            input: observation.input.clone(),
        })
        .collect();
    let mut source = ReplaySource::new(observations);
    let output = differential::run(&mut source, &cases, &Taxonomy::default());

    // No line may still say `unclassified` now that a real classifier is wired in.
    for line in &output.comparison_log {
        assert!(!line.contains("class=unclassified"), "{line}");
    }

    let fangan = output
        .comparison_log
        .iter()
        .find(|line| line.contains("case=ambiguous-fangan"))
        .expect("fangan present");
    assert!(fangan.contains("class=tie-swap"), "{fangan}");

    let nihao = output
        .comparison_log
        .iter()
        .find(|line| line.contains("case=valid-multiple"))
        .expect("nihao present");
    assert!(nihao.contains("class=output-identical"), "{nihao}");
}
