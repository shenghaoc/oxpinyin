//! Divergence classification and the auto-accept budget.
//!
//! Classes, rules and the budget discussion are in
//! `docs/findings/divergence-taxonomy.md`, which is **proposed, not frozen**.
//!
//! Reasons and classes are separate on purpose. A [`crate::differential::Reason`]
//! states which check failed and is implementation-level; a
//! [`DivergenceClass`] states whose problem it is and whether a human must look.
//! Keeping them apart means a reason can never silently redefine a class.
//!
//! Pure and always compiled, so the taxonomy is under test on every platform
//! with no oracle present.

use crate::differential::{Classifier, Comparison, Outcome, Reason};
use crate::{EXPECTED_PIN_REF, OracleFlags};

/// Auto-accept budget denominator: 0.5% is one in two hundred.
pub const AUTO_ACCEPT_BUDGET_DENOMINATOR: usize = 200;

/// Whose problem a comparison is, and whether a human must triage it.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DivergenceClass {
    /// Nothing differed: the oracle's choice is our first path. Success.
    OutputIdentical,
    /// Agreement at a positive rank: same path set, different member chosen.
    TieSwap,
    /// Both sides implement the feature but admit different path sets.
    PathSet,
    /// The two sides were asked different questions, via option bits.
    FlagSemantics,
    /// Our side could not produce a comparable answer.
    OursBug,
    /// The pin violated its own invariants or could not describe its output.
    TheirsBug,
    /// Pinned code, different data payload.
    DataVersion,
    /// Not our pin-built oracle at all. Advisory; never gates.
    DistroDelta,
}

impl DivergenceClass {
    /// Every class, in the order the finding lists them.
    pub const ALL: [Self; 8] = [
        Self::OutputIdentical,
        Self::TieSwap,
        Self::PathSet,
        Self::FlagSemantics,
        Self::OursBug,
        Self::TheirsBug,
        Self::DataVersion,
        Self::DistroDelta,
    ];

    /// Wire spelling used in both log schemas.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::OutputIdentical => "output-identical",
            Self::TieSwap => "tie-swap",
            Self::PathSet => "path-set",
            Self::FlagSemantics => "flag-semantics",
            Self::OursBug => "ours-bug",
            Self::TheirsBug => "theirs-bug",
            Self::DataVersion => "data-version",
            Self::DistroDelta => "distro-delta",
        }
    }

    /// Whether this class is accepted without human triage.
    #[must_use]
    pub const fn is_auto_accepted(self) -> bool {
        matches!(self, Self::OutputIdentical | Self::TieSwap)
    }

    /// Whether this class is a divergence at all.
    ///
    /// [`Self::OutputIdentical`] is the success population, which is why the
    /// budget's meaning needs an Architect decision.
    #[must_use]
    pub const fn is_divergence(self) -> bool {
        !matches!(self, Self::OutputIdentical)
    }

    /// Whether this class blocks the S1b parity gate.
    ///
    /// `theirs-bug` does not: the pin is the subject and we cannot fix it.
    /// `flag-semantics` does not: the two sides were asked different questions.
    /// `distro-delta` never does, per `docs/findings/oracle-environment.md`.
    #[must_use]
    pub const fn gates_parity(self) -> bool {
        matches!(self, Self::PathSet | Self::OursBug)
    }
}

impl core::fmt::Display for DivergenceClass {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_wire())
    }
}

/// Splits an off-pin reference into `data-version` or `distro-delta`.
///
/// The pin ref is `libpinyin-<tag>-<sha>+model20-<digest>+dbm-<backend>`. If only
/// the model component differs, the pinned code ran against different tables and
/// the divergence is about data. Anything else — a different build, a different
/// DBM backend, or a ref that is not in this shape — is not our oracle.
fn classify_off_pin(pin_ref: &str) -> DivergenceClass {
    let split = |reference: &str| {
        let mut parts = reference.split('+');
        let code = parts.next()?.to_owned();
        let model = parts.next()?.to_owned();
        let dbm = parts.next()?.to_owned();
        if parts.next().is_some() {
            return None;
        }
        Some((code, model, dbm))
    };

    match (split(pin_ref), split(EXPECTED_PIN_REF)) {
        (Some((code, model, dbm)), Some((want_code, want_model, want_dbm))) => {
            if code == want_code && dbm == want_dbm && model != want_model {
                DivergenceClass::DataVersion
            } else {
                DivergenceClass::DistroDelta
            }
        }
        _ => {
            #[cfg(test)]
            eprintln!("classify_off_pin: unparseable pin_ref: {pin_ref:?}");
            DivergenceClass::DistroDelta
        }
    }
}

/// Applies the classification rules from the taxonomy finding.
///
/// Takes the parity profile as data, so changing the profile does not require
/// changing the classifier.
#[derive(Clone, Copy, Debug)]
pub struct Taxonomy {
    parity_profile: OracleFlags,
}

impl Default for Taxonomy {
    fn default() -> Self {
        Self {
            parity_profile: OracleFlags::DEFAULT,
        }
    }
}

impl Taxonomy {
    /// Classifier for `parity_profile`.
    #[must_use]
    pub const fn new(parity_profile: OracleFlags) -> Self {
        Self { parity_profile }
    }

    /// The profile divergences are judged against.
    #[must_use]
    pub const fn parity_profile(&self) -> OracleFlags {
        self.parity_profile
    }

    /// Classifies one comparison. Total: every comparison gets exactly one class.
    #[must_use]
    pub fn class_of(&self, comparison: &Comparison) -> DivergenceClass {
        let reason = match comparison.outcome {
            Outcome::Agreement => {
                return if comparison.rank == Some(0) {
                    DivergenceClass::OutputIdentical
                } else {
                    DivergenceClass::TieSwap
                };
            }
            Outcome::Divergence(reason) => reason,
        };

        match reason {
            Reason::OffPin => classify_off_pin(&comparison.observation.pin_ref),
            Reason::OracleInconsistent | Reason::PositionOverflow | Reason::OracleSentinel => {
                DivergenceClass::TheirsBug
            }
            Reason::OursError => DivergenceClass::OursBug,
            Reason::ConsumedLength | Reason::Remainder | Reason::PathAbsent => {
                // Our parser takes no option word: it implements exactly the
                // parity profile. A run under any other flag word is comparing
                // two different questions, so its divergences are about flag
                // semantics rather than about which paths exist.
                if comparison.observation.flags == self.parity_profile {
                    DivergenceClass::PathSet
                } else {
                    DivergenceClass::FlagSemantics
                }
            }
        }
    }
}

impl Classifier for Taxonomy {
    fn classify(&self, comparison: &Comparison) -> &'static str {
        self.class_of(comparison).as_wire()
    }
}

/// Verdict on the auto-accept budget.
///
/// Both readings of the W2-T5 card are computed. `docs/findings/divergence-taxonomy.md`
/// proposes the divergence-only reading and requests an Architect decision; until
/// that is frozen, neither number is silently assumed to be the answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BudgetVerdict {
    /// Inputs compared.
    pub corpus_size: usize,
    /// `output-identical` records, the success population.
    pub output_identical: usize,
    /// `tie-swap` records.
    pub tie_swap: usize,
    /// Largest count the budget permits.
    pub limit: usize,
}

impl BudgetVerdict {
    /// Both auto-accepted classes together.
    #[must_use]
    pub const fn auto_accepted(&self) -> usize {
        self.output_identical + self.tie_swap
    }

    /// Literal reading: both auto-accepted classes counted against the budget.
    ///
    /// Reported for the decision, not endorsed. Satisfying it would require the
    /// engine to agree with the oracle almost never.
    #[must_use]
    pub const fn literal_pass(&self) -> bool {
        self.auto_accepted() <= self.limit
    }

    /// Divergence-only reading: `tie-swap` alone, since `output-identical` is not
    /// a divergence. This is the reading the finding proposes.
    #[must_use]
    pub const fn divergence_only_pass(&self) -> bool {
        self.tie_swap <= self.limit
    }

    /// Renders the verdict for the run summary.
    #[must_use]
    pub fn to_summary(&self) -> String {
        let verdict = |pass: bool| if pass { "within" } else { "OVER" };
        format!(
            "auto-accept budget (0.5% = {} of {})\n  \
             output-identical      {}\n  \
             tie-swap              {}\n  \
             literal (both)        {} {}\n  \
             divergence-only       {} {}\n  \
             reported, not gating until a decoder exists (W4)\n",
            self.limit,
            self.corpus_size,
            self.output_identical,
            self.tie_swap,
            self.auto_accepted(),
            verdict(self.literal_pass()),
            self.tie_swap,
            verdict(self.divergence_only_pass()),
        )
    }
}

/// Computes the budget verdict from per-class counts.
///
/// `counts` is looked up by wire spelling, matching [`crate::differential::RunReport::classes`].
#[must_use]
pub fn budget(
    corpus_size: usize,
    counts: &std::collections::BTreeMap<&'static str, usize>,
) -> BudgetVerdict {
    let count_of =
        |class: DivergenceClass| counts.get(class.as_wire()).copied().unwrap_or_default();

    BudgetVerdict {
        corpus_size,
        output_identical: count_of(DivergenceClass::OutputIdentical),
        tie_swap: count_of(DivergenceClass::TieSwap),
        limit: corpus_size / AUTO_ACCEPT_BUDGET_DENOMINATOR,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{DivergenceClass, Taxonomy, budget, classify_off_pin};
    use crate::differential::{Source, compare};
    use crate::observation::{OracleCompleteness, OracleObservation, OracleSegment};
    use crate::{EXPECTED_PIN_REF, OracleFlags};

    fn observation(input: &str, parsed: usize, segments: Vec<OracleSegment>) -> OracleObservation {
        OracleObservation {
            pin_ref: EXPECTED_PIN_REF.to_owned(),
            family: None,
            case: None,
            input: input.as_bytes().to_vec(),
            flags: OracleFlags::DEFAULT,
            parse_return: parsed,
            parsed_input_length: parsed,
            segments,
            candidate_total: 0,
            candidates: Vec::new(),
            remainder: input.as_bytes()[parsed..].to_vec(),
        }
    }

    fn segment(text: &str, begin: u16, end: u16, complete: bool) -> OracleSegment {
        OracleSegment::Syllable {
            syllable: text.to_owned(),
            begin,
            end,
            completeness: if complete {
                OracleCompleteness::Complete
            } else {
                OracleCompleteness::Partial
            },
        }
    }

    fn class_of(observation: OracleObservation) -> DivergenceClass {
        Taxonomy::default().class_of(&compare(observation, Source::Replay, "t", "t"))
    }

    #[test]
    fn worked_example_output_identical() {
        // nihao: the oracle's choice is our first path.
        let class = class_of(observation(
            "nihao",
            5,
            vec![segment("ni", 0, 2, true), segment("hao", 2, 5, true)],
        ));
        assert_eq!(class, DivergenceClass::OutputIdentical);
        assert!(class.is_auto_accepted());
        assert!(!class.is_divergence());
        assert!(!class.gates_parity());
    }

    #[test]
    fn worked_example_tie_swap() {
        // fangan: the oracle selects our second path.
        let class = class_of(observation(
            "fangan",
            6,
            vec![segment("fan", 0, 3, true), segment("gan", 3, 6, true)],
        ));
        assert_eq!(class, DivergenceClass::TieSwap);
        assert!(class.is_auto_accepted());
        assert!(class.is_divergence());
        assert!(!class.gates_parity());
    }

    #[test]
    fn worked_example_path_set() {
        // yingchon: the pin puts a partial in a non-final position, which the
        // frozen parser SPEC forbids, so the path sets differ.
        let class = class_of(observation(
            "yingchon",
            8,
            vec![
                segment("ying", 0, 4, true),
                segment("ch", 4, 6, false),
                segment("o", 6, 7, true),
                segment("n", 7, 8, false),
            ],
        ));
        assert_eq!(class, DivergenceClass::PathSet);
        assert!(!class.is_auto_accepted());
        assert!(class.gates_parity());
    }

    #[test]
    fn worked_example_flag_semantics() {
        // F-C pinyin-incomplete-off: nih under IS_PINYIN alone. The oracle
        // refuses the partial tail; our parser always models partials.
        let mut observed = observation("nih", 2, vec![segment("ni", 0, 2, true)]);
        observed.flags = OracleFlags::BASELINE;
        observed.remainder = b"h".to_vec();

        let class = class_of(observed);
        assert_eq!(class, DivergenceClass::FlagSemantics);
        assert!(!class.gates_parity());
    }

    #[test]
    fn worked_example_ours_bug() {
        // Thirteen two-way groups exceed MAX_PARSE_RESULTS, so our parser errors.
        let input = std::iter::repeat_n("xian", 13)
            .collect::<Vec<_>>()
            .join("'");
        let mut observed = observation("ni", 2, vec![segment("ni", 0, 2, true)]);
        observed.input = input.clone().into_bytes();
        observed.parse_return = input.len();
        observed.parsed_input_length = input.len();
        observed.remainder = Vec::new();

        let class = class_of(observed);
        assert_eq!(class, DivergenceClass::OursBug);
        assert!(class.gates_parity());
    }

    #[test]
    fn worked_example_theirs_bug_abort_guard() {
        // Apostrophe-only input: parsed length 1 with no key columns.
        let mut observed = observation("'", 1, vec![OracleSegment::NoKeyColumns { parsed_len: 1 }]);
        observed.remainder = Vec::new();

        let class = class_of(observed);
        assert_eq!(class, DivergenceClass::TheirsBug);
        assert!(!class.gates_parity(), "we cannot fix the pin");
    }

    #[test]
    fn worked_example_theirs_bug_missing_string() {
        // ni': a key column with no usable pinyin string, the live F-E-01 shape.
        let mut observed = observation(
            "ni'",
            3,
            vec![
                segment("ni", 0, 2, true),
                OracleSegment::MissingPinyinString { offset: 2 },
            ],
        );
        observed.remainder = Vec::new();
        assert_eq!(class_of(observed), DivergenceClass::TheirsBug);
    }

    #[test]
    fn worked_example_data_version() {
        // Pinned code, different tables.
        let mut observed = observation("ni", 2, vec![segment("ni", 0, 2, true)]);
        observed.pin_ref = EXPECTED_PIN_REF.replace("model20-59c68e89", "model20-deadbeef");
        assert_eq!(class_of(observed), DivergenceClass::DataVersion);
    }

    #[test]
    fn worked_example_distro_delta() {
        // A different DBM backend is not our oracle.
        let mut observed = observation("ni", 2, vec![segment("ni", 0, 2, true)]);
        observed.pin_ref = EXPECTED_PIN_REF.replace("dbm-tkrzw", "dbm-berkeleydb");

        let class = class_of(observed);
        assert_eq!(class, DivergenceClass::DistroDelta);
        assert!(!class.gates_parity(), "distro-delta never gates S1b");
    }

    #[test]
    fn off_pin_split_distinguishes_data_from_build() {
        assert_eq!(
            classify_off_pin(&EXPECTED_PIN_REF.replace("model20-59c68e89", "model20-ff")),
            DivergenceClass::DataVersion
        );
        assert_eq!(
            classify_off_pin(&EXPECTED_PIN_REF.replace("libpinyin-2.11.91", "libpinyin-2.10.0")),
            DivergenceClass::DistroDelta
        );
        assert_eq!(
            classify_off_pin(&EXPECTED_PIN_REF.replace("dbm-tkrzw", "dbm-berkeleydb")),
            DivergenceClass::DistroDelta
        );
        // Not in our shape at all.
        assert_eq!(
            classify_off_pin("some-distro-package-1.2.3"),
            DivergenceClass::DistroDelta
        );
        assert_eq!(classify_off_pin(""), DivergenceClass::DistroDelta);
    }

    #[test]
    fn every_class_has_a_distinct_stable_wire_spelling() {
        let mut spellings: Vec<&str> = DivergenceClass::ALL
            .iter()
            .map(|class| class.as_wire())
            .collect();
        assert_eq!(
            spellings,
            vec![
                "output-identical",
                "tie-swap",
                "path-set",
                "flag-semantics",
                "ours-bug",
                "theirs-bug",
                "data-version",
                "distro-delta",
            ]
        );
        spellings.sort_unstable();
        spellings.dedup();
        assert_eq!(spellings.len(), DivergenceClass::ALL.len());
    }

    #[test]
    fn exactly_two_classes_are_auto_accepted() {
        let auto: Vec<&str> = DivergenceClass::ALL
            .iter()
            .filter(|class| class.is_auto_accepted())
            .map(|class| class.as_wire())
            .collect();
        assert_eq!(auto, vec!["output-identical", "tie-swap"]);
    }

    #[test]
    fn only_our_own_problems_gate_parity() {
        let gating: Vec<&str> = DivergenceClass::ALL
            .iter()
            .filter(|class| class.gates_parity())
            .map(|class| class.as_wire())
            .collect();
        assert_eq!(gating, vec!["path-set", "ours-bug"]);
    }

    #[test]
    fn budget_reports_both_readings() {
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        counts.insert("output-identical", 9_506);
        counts.insert("tie-swap", 468);

        let verdict = budget(10_465, &counts);
        assert_eq!(verdict.limit, 52);
        assert_eq!(verdict.auto_accepted(), 9_974);
        assert!(!verdict.literal_pass(), "95% cannot be within 0.5%");
        assert!(!verdict.divergence_only_pass(), "468 exceeds 52");

        let summary = verdict.to_summary();
        assert!(summary.contains("literal (both)"));
        assert!(summary.contains("divergence-only"));
        assert!(summary.contains("not gating"));
    }

    #[test]
    fn budget_passes_when_tie_swaps_are_rare() {
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        counts.insert("output-identical", 10_000);
        counts.insert("tie-swap", 10);

        let verdict = budget(10_465, &counts);
        assert!(
            verdict.divergence_only_pass(),
            "10 tie-swaps is inside a limit of 52"
        );
        assert!(
            !verdict.literal_pass(),
            "the literal reading still fails, which is the point being raised"
        );
    }

    #[test]
    fn classification_is_total_over_every_reason() {
        use crate::differential::{Outcome, Reason, Source, compare};

        // Every reason must map to the expected class.  Construct a minimal
        // Comparison so `class_of` is exercised — a new reason without a rule
        // in `class_of` would fail to compile rather than silently fall through.
        let taxonomy = Taxonomy::default();
        for (reason, expected) in [
            (Reason::OffPin, DivergenceClass::DistroDelta),
            (Reason::OracleInconsistent, DivergenceClass::TheirsBug),
            (Reason::PositionOverflow, DivergenceClass::TheirsBug),
            (Reason::OracleSentinel, DivergenceClass::TheirsBug),
            (Reason::OursError, DivergenceClass::OursBug),
            (Reason::ConsumedLength, DivergenceClass::PathSet),
            (Reason::Remainder, DivergenceClass::PathSet),
            (Reason::PathAbsent, DivergenceClass::PathSet),
        ] {
            let obs = observation("a", 1, vec![]);
            let mut c = compare(obs, Source::Replay, "t", "t");
            c.outcome = Outcome::Divergence(reason);
            if matches!(reason, Reason::OffPin) {
                c.observation.pin_ref = "distro-1.0.0".into();
            }
            assert_eq!(
                taxonomy.class_of(&c),
                expected,
                "{} maps to {expected}",
                reason.as_wire(),
            );
        }
    }
}
