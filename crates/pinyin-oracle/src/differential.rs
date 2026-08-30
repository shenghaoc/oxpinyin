//! Parse-level differential comparison against the pinned oracle.
//!
//! Schemas, the agreement predicate and the reason precedence are frozen in
//! `docs/findings/differential-log.md`.
//!
//! The comparison is a pure function of an [`OracleObservation`] and our parse,
//! so the same logic serves the live FFI producer and the replay producer. Only
//! [`ObservationSource`] implementations differ between the two.
//!
//! At this stage only parse-level output is compared: segmentation, consumed
//! prefix length and remainder. Candidate lists are recorded, never asserted,
//! because there is no decoder to compare them against yet.

use std::collections::BTreeMap;

use oxpinyin_core::{Completeness, FullPinyinParser, InputParser, ParseError, ParseResult};

use crate::capture::{escape, segments_to_wire};
use crate::observation::{OracleCompleteness, OracleObservation};
use crate::{EXPECTED_PIN_REF, OracleError};

/// Schema tag of the per-input comparison log.
pub const DIFF_SCHEMA: &str = "pinyin-diff-v1";

/// Schema tag of the divergence-only log.
pub const DIVERGENCE_SCHEMA: &str = "pinyin-divergence-v1";

/// How many of our paths a divergence record spells out.
///
/// The uncapped count is retained in `ours_paths`, so a highly ambiguous input
/// cannot produce an unbounded log line.
pub const MAX_LOGGED_PATHS: usize = 8;

/// Class token used until the W2-T5 taxonomy is frozen.
pub const UNCLASSIFIED: &str = "unclassified";

/// Which producer supplied the oracle side.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    /// The live FFI against the pin-built prefix.
    Live,
    /// A `pinyin-capture-v1` record replayed from a fixture.
    Replay,
}

impl Source {
    /// Wire spelling.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Replay => "replay",
        }
    }
}

/// Why a comparison diverged.
///
/// Ordered by the precedence frozen in the finding: subject validity, then our
/// own failure, then the substantive comparisons from coarsest to finest.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Reason {
    /// The observation's pin ref is not the frozen pin.
    OffPin,
    /// The oracle violated its own invariants.
    OracleInconsistent,
    /// A position does not fit `u16`.
    PositionOverflow,
    /// The oracle emitted a sentinel segment (the F-E-01 shape).
    OracleSentinel,
    /// Our parser returned `Err`.
    OursError,
    /// Consumed prefix lengths disagree.
    ConsumedLength,
    /// Remainders disagree.
    Remainder,
    /// The oracle's segmentation is not in our path set.
    PathAbsent,
}

impl Reason {
    /// Wire spelling.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::OffPin => "off-pin",
            Self::OracleInconsistent => "oracle-inconsistent",
            Self::PositionOverflow => "position-overflow",
            Self::OracleSentinel => "oracle-sentinel",
            Self::OursError => "ours-error",
            Self::ConsumedLength => "consumed-length",
            Self::Remainder => "remainder",
            Self::PathAbsent => "path-absent",
        }
    }
}

/// Verdict for one input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The oracle's segmentation is in our path set, and lengths agree.
    Agreement,
    /// Something disagreed; the reason is the highest-precedence applicable one.
    Divergence(Reason),
}

impl Outcome {
    /// Wire spelling of the outcome itself.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Agreement => "agreement",
            Self::Divergence(_) => "divergence",
        }
    }

    /// The reason, when this is a divergence.
    #[must_use]
    pub const fn reason(self) -> Option<Reason> {
        match self {
            Self::Agreement => None,
            Self::Divergence(reason) => Some(reason),
        }
    }
}

/// What our parser produced for the input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OurParse {
    /// Every valid segmentation, in frozen path-set order.
    Paths(Vec<ParseResult>),
    /// The parser reported a resource bound.
    Failed(ParseError),
}

impl OurParse {
    /// Number of paths returned, or zero on failure.
    #[must_use]
    pub const fn path_count(&self) -> usize {
        match self {
            Self::Paths(paths) => paths.len(),
            Self::Failed(_) => 0,
        }
    }

    /// Bytes consumed, which every path for one input shares.
    #[must_use]
    pub fn consumed(&self, input_len: usize) -> Option<usize> {
        match self {
            Self::Paths(paths) => paths
                .first()
                .map(|path| input_len.saturating_sub(path.remainder.len())),
            Self::Failed(_) => None,
        }
    }

    /// The shared remainder, when there is one.
    #[must_use]
    pub fn remainder(&self) -> Option<&[u8]> {
        match self {
            Self::Paths(paths) => paths.first().map(|path| path.remainder.as_slice()),
            Self::Failed(_) => None,
        }
    }
}

/// Supplies the class token for a divergence.
///
/// The seam W2-T5 fills in. Keeping it a trait means the taxonomy can be
/// implemented and frozen without touching the log schema or the runner.
pub trait Classifier {
    /// Returns the class token for `comparison`, or `"-"` on agreement.
    fn classify(&self, comparison: &Comparison) -> &'static str;
}

/// Placeholder classifier: every divergence is [`UNCLASSIFIED`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Unclassified;

impl Classifier for Unclassified {
    fn classify(&self, comparison: &Comparison) -> &'static str {
        match comparison.outcome {
            Outcome::Agreement => "-",
            Outcome::Divergence(_) => UNCLASSIFIED,
        }
    }
}

/// One compared input.
#[derive(Clone, Debug)]
pub struct Comparison {
    /// Which producer supplied the oracle side.
    pub source: Source,
    /// Stratum file name, or fixture family.
    pub corpus: String,
    /// Stable case identifier.
    pub case: String,
    /// The oracle's response.
    pub observation: OracleObservation,
    /// Our parser's response.
    pub ours: OurParse,
    /// Verdict.
    pub outcome: Outcome,
    /// Index of the oracle's path in our ordered set, when present.
    pub rank: Option<usize>,
    /// Class token supplied by a [`Classifier`].
    pub class: &'static str,
}

/// A path in the shape both sides can be compared in.
type ComparablePath = Vec<(String, u16, u16, Completeness)>;

fn our_path(path: &ParseResult) -> Option<ComparablePath> {
    path.syllables
        .iter()
        .map(|segment| {
            let begin = u16::try_from(segment.start).ok()?;
            let end = u16::try_from(segment.end).ok()?;
            Some((segment.syllable.clone(), begin, end, segment.completeness))
        })
        .collect()
}

fn oracle_path(observation: &OracleObservation) -> Option<ComparablePath> {
    Some(
        observation
            .selected_path()?
            .into_iter()
            .map(|(syllable, begin, end, completeness)| {
                (
                    syllable.to_owned(),
                    begin,
                    end,
                    match completeness {
                        OracleCompleteness::Complete => Completeness::Complete,
                        OracleCompleteness::Partial => Completeness::Partial,
                    },
                )
            })
            .collect(),
    )
}

/// Compares one observation against our parser.
///
/// Pure: identical inputs give an identical result, with no I/O and no reliance
/// on the `oracle-ffi` feature.
#[must_use]
pub fn compare(
    observation: OracleObservation,
    source: Source,
    corpus: impl Into<String>,
    case: impl Into<String>,
) -> Comparison {
    let input = observation.input.clone();
    let ours = match FullPinyinParser.parse(&input) {
        Ok(paths) => OurParse::Paths(paths),
        Err(error) => OurParse::Failed(error),
    };

    let theirs = oracle_path(&observation);
    let rank = match (&ours, &theirs) {
        (OurParse::Paths(paths), Some(wanted)) => paths
            .iter()
            .position(|path| our_path(path).as_ref() == Some(wanted)),
        _ => None,
    };

    let outcome = decide(&observation, &ours, theirs.as_ref(), rank);

    let mut comparison = Comparison {
        source,
        corpus: corpus.into(),
        case: case.into(),
        observation,
        ours,
        outcome,
        rank,
        class: "-",
    };
    comparison.class = Unclassified.classify(&comparison);
    comparison
}

/// Applies the frozen reason precedence.
fn decide(
    observation: &OracleObservation,
    ours: &OurParse,
    theirs: Option<&ComparablePath>,
    rank: Option<usize>,
) -> Outcome {
    // 1. Subject validity: an off-pin observation makes everything else moot.
    if observation.pin_ref != EXPECTED_PIN_REF {
        return Outcome::Divergence(Reason::OffPin);
    }

    // 2. The oracle is a subject under test, not a trusted component.
    if observation.check_self_consistent().is_err() {
        return Outcome::Divergence(Reason::OracleInconsistent);
    }

    // 3. Positions the oracle cannot represent.
    if u16::try_from(observation.input.len()).is_err() {
        return Outcome::Divergence(Reason::PositionOverflow);
    }

    // 4. The F-E-01 shape: a key the oracle could not fully describe.
    if observation.has_sentinel_segment() {
        return Outcome::Divergence(Reason::OracleSentinel);
    }

    // 5. Membership cannot be evaluated without a path set.
    let OurParse::Paths(_) = ours else {
        return Outcome::Divergence(Reason::OursError);
    };

    // 6. Coarsest substantive comparison.
    let ours_consumed = ours.consumed(observation.input.len());
    if ours_consumed != Some(observation.parsed_input_length) {
        return Outcome::Divergence(Reason::ConsumedLength);
    }

    // 7. Remainder must agree byte for byte.
    if ours.remainder() != Some(observation.remainder.as_slice()) {
        return Outcome::Divergence(Reason::Remainder);
    }

    // 8. Finest: did we enumerate the segmentation the oracle chose?
    //
    // Step 4 already returned on sentinels, so `theirs` is `Some` here and
    // `rank` was computed. An oracle that selected nothing yields the empty
    // path, which our parser also returns as its first path for unconsumable
    // input, so empty-against-empty lands on rank 0 rather than a special case.
    debug_assert!(theirs.is_some(), "sentinels are handled at step 4");
    match rank {
        Some(_) => Outcome::Agreement,
        None => Outcome::Divergence(Reason::PathAbsent),
    }
}

/// Produces oracle observations for inputs.
///
/// The only difference between a live run and a replay run.
pub trait ObservationSource {
    /// Observes `input`.
    ///
    /// # Errors
    ///
    /// Returns an error when the producer cannot yield an observation at all,
    /// which is distinct from a divergence.
    fn observe(&mut self, input: &[u8]) -> Result<OracleObservation, OracleError>;

    /// Which producer this is.
    fn source(&self) -> Source;
}

/// Replays observations already captured from the pin.
#[derive(Clone, Debug, Default)]
pub struct ReplaySource {
    observations: Vec<OracleObservation>,
}

impl ReplaySource {
    /// Builds a replay source from decoded capture records.
    #[must_use]
    pub const fn new(observations: Vec<OracleObservation>) -> Self {
        Self { observations }
    }

    /// Every observation, in fixture order.
    #[must_use]
    pub fn observations(&self) -> &[OracleObservation] {
        &self.observations
    }
}

impl ObservationSource for ReplaySource {
    fn observe(&mut self, input: &[u8]) -> Result<OracleObservation, OracleError> {
        self.observations
            .iter()
            .find(|observation| observation.input == input)
            .cloned()
            .ok_or_else(|| OracleError::ReplayInputMissing {
                input: escape(input),
            })
    }

    fn source(&self) -> Source {
        Source::Replay
    }
}

/// Totals and distributions for one run.
#[derive(Clone, Debug, Default)]
pub struct RunReport {
    /// Inputs compared.
    pub total: usize,
    /// Inputs that agreed.
    pub agreements: usize,
    /// Count per divergence reason.
    pub reasons: BTreeMap<Reason, usize>,
    /// Count per class token.
    pub classes: BTreeMap<&'static str, usize>,
    /// Count per rank of the oracle's path in our ordered set.
    pub ranks: BTreeMap<usize, usize>,
    /// Inputs the producer could not observe at all.
    pub unobservable: usize,
}

impl RunReport {
    /// Divergences seen.
    #[must_use]
    pub const fn divergences(&self) -> usize {
        self.total - self.agreements
    }

    /// Records one comparison.
    pub fn record(&mut self, comparison: &Comparison) {
        self.total += 1;
        match comparison.outcome {
            Outcome::Agreement => self.agreements += 1,
            Outcome::Divergence(reason) => {
                *self.reasons.entry(reason).or_default() += 1;
            }
        }
        *self.classes.entry(comparison.class).or_default() += 1;
        if let Some(rank) = comparison.rank {
            *self.ranks.entry(rank).or_default() += 1;
        }
    }

    /// Renders the human-readable summary.
    ///
    /// Derived from the logs; deliberately not a schema.
    #[must_use]
    pub fn to_summary(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("compared      {}\n", self.total));
        out.push_str(&format!("agreements    {}\n", self.agreements));
        out.push_str(&format!("divergences   {}\n", self.divergences()));
        if self.unobservable > 0 {
            out.push_str(&format!("unobservable  {}\n", self.unobservable));
        }

        if !self.reasons.is_empty() {
            out.push_str("\nby reason\n");
            for (reason, count) in &self.reasons {
                out.push_str(&format!("  {:<22} {count}\n", reason.as_wire()));
            }
        }

        if !self.classes.is_empty() {
            out.push_str("\nby class\n");
            for (class, count) in &self.classes {
                out.push_str(&format!("  {class:<22} {count}\n"));
            }
        }

        if !self.ranks.is_empty() {
            out.push_str("\noracle path rank in our ordered set\n");
            for (rank, count) in &self.ranks {
                out.push_str(&format!("  {rank:<22} {count}\n"));
            }
        }
        out
    }
}

/// Renders one `pinyin-diff-v1` record.
#[must_use]
pub fn comparison_line(comparison: &Comparison) -> String {
    let observation = &comparison.observation;
    let ours_consumed = comparison
        .ours
        .consumed(observation.input.len())
        .map_or_else(|| "-".to_owned(), |value| value.to_string());

    [
        format!("schema={DIFF_SCHEMA}"),
        format!("pin_ref={}", escape(observation.pin_ref.as_bytes())),
        format!("source={}", comparison.source.as_wire()),
        format!("corpus={}", escape(comparison.corpus.as_bytes())),
        format!("case={}", escape(comparison.case.as_bytes())),
        format!("input={}", escape(&observation.input)),
        format!("flags={}", observation.flags),
        format!("outcome={}", comparison.outcome.as_wire()),
        format!("class={}", comparison.class),
        format!(
            "rank={}",
            comparison
                .rank
                .map_or_else(|| "-".to_owned(), |rank| rank.to_string())
        ),
        format!("ours_paths={}", comparison.ours.path_count()),
        format!("ours_consumed={ours_consumed}"),
        format!("theirs_consumed={}", observation.parsed_input_length),
        format!("candidate_total={}", observation.candidate_total),
    ]
    .join("\t")
}

/// Renders one `pinyin-divergence-v1` record, or `None` on agreement.
#[must_use]
pub fn divergence_line(comparison: &Comparison) -> Option<String> {
    let reason = comparison.outcome.reason()?;
    let observation = &comparison.observation;

    let (ours_shown, ours_path_set) = match &comparison.ours {
        OurParse::Paths(paths) => {
            let shown: Vec<String> = paths
                .iter()
                .take(MAX_LOGGED_PATHS)
                .map(render_our_path)
                .collect();
            (shown.len(), shown.join(";"))
        }
        OurParse::Failed(error) => (0, format!("<error:{error}>")),
    };

    let ours_consumed = comparison
        .ours
        .consumed(observation.input.len())
        .map_or_else(|| "-".to_owned(), |value| value.to_string());
    let ours_remainder = comparison
        .ours
        .remainder()
        .map_or_else(|| "-".to_owned(), escape);

    Some(
        [
            format!("schema={DIVERGENCE_SCHEMA}"),
            format!("pin_ref={}", escape(observation.pin_ref.as_bytes())),
            format!("source={}", comparison.source.as_wire()),
            format!("corpus={}", escape(comparison.corpus.as_bytes())),
            format!("case={}", escape(comparison.case.as_bytes())),
            format!("input={}", escape(&observation.input)),
            format!("flags={}", observation.flags),
            format!("class={}", comparison.class),
            format!("reason={}", reason.as_wire()),
            format!("theirs_path={}", segments_to_wire(&observation.segments)),
            format!("theirs_consumed={}", observation.parsed_input_length),
            format!("theirs_remainder={}", escape(&observation.remainder)),
            format!("ours_paths={}", comparison.ours.path_count()),
            format!("ours_shown={ours_shown}"),
            format!("ours_path_set={ours_path_set}"),
            format!("ours_consumed={ours_consumed}"),
            format!("ours_remainder={ours_remainder}"),
        ]
        .join("\t"),
    )
}

/// Renders one of our paths in capture segment notation.
fn render_our_path(path: &ParseResult) -> String {
    if path.syllables.is_empty() {
        return "-".to_owned();
    }
    path.syllables
        .iter()
        .map(|segment| {
            format!(
                "{}@{}:{}:{}",
                escape(segment.syllable.as_bytes()),
                segment.start,
                segment.end,
                match segment.completeness {
                    Completeness::Complete => "complete",
                    Completeness::Partial => "partial",
                }
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// One case to compare.
#[derive(Clone, Debug)]
pub struct Case {
    /// Stratum file name or fixture family.
    pub corpus: String,
    /// Stable identifier.
    pub case: String,
    /// Input bytes.
    pub input: Vec<u8>,
}

/// Result of a full run: the report plus both logs.
#[derive(Clone, Debug, Default)]
pub struct RunOutput {
    /// Totals and distributions.
    pub report: RunReport,
    /// Every `pinyin-diff-v1` line, in case order.
    pub comparison_log: Vec<String>,
    /// Every `pinyin-divergence-v1` line, in case order.
    pub divergence_log: Vec<String>,
}

impl RunOutput {
    /// The comparison log as file contents.
    #[must_use]
    pub fn comparison_log_text(&self) -> String {
        lines_to_text(&self.comparison_log)
    }

    /// The divergence log as file contents.
    #[must_use]
    pub fn divergence_log_text(&self) -> String {
        lines_to_text(&self.divergence_log)
    }
}

fn lines_to_text(lines: &[String]) -> String {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    text
}

/// Runs `cases` through `source`, classifying with `classifier`.
///
/// Records appear in `cases` order and nothing is parallelised, so the logs are
/// byte-identical between runs and therefore diffable.
pub fn run(
    source: &mut impl ObservationSource,
    cases: &[Case],
    classifier: &impl Classifier,
) -> RunOutput {
    let producer = source.source();
    let mut output = RunOutput::default();

    for case in cases {
        let Ok(observation) = source.observe(&case.input) else {
            output.report.unobservable += 1;
            continue;
        };

        let mut comparison = compare(observation, producer, &case.corpus, &case.case);
        comparison.class = classifier.classify(&comparison);

        output.report.record(&comparison);
        output.comparison_log.push(comparison_line(&comparison));
        if let Some(line) = divergence_line(&comparison) {
            output.divergence_log.push(line);
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::{
        Case, Comparison, Outcome, Reason, ReplaySource, Source, Unclassified, compare,
        comparison_line, divergence_line, run,
    };
    use crate::observation::{OracleCompleteness, OracleObservation, OracleSegment};
    use crate::{EXPECTED_PIN_REF, OracleFlags};

    fn observation(input: &str, parsed: usize, segments: Vec<OracleSegment>) -> OracleObservation {
        OracleObservation {
            pin_ref: EXPECTED_PIN_REF.to_owned(),
            family: Some("T".to_owned()),
            case: Some("t".to_owned()),
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

    fn complete(text: &str, begin: u16, end: u16) -> OracleSegment {
        OracleSegment::Syllable {
            syllable: text.to_owned(),
            begin,
            end,
            completeness: OracleCompleteness::Complete,
        }
    }

    fn partial(text: &str, begin: u16, end: u16) -> OracleSegment {
        OracleSegment::Syllable {
            syllable: text.to_owned(),
            begin,
            end,
            completeness: OracleCompleteness::Partial,
        }
    }

    fn compared(observation: OracleObservation) -> Comparison {
        compare(observation, Source::Replay, "t", "t")
    }

    #[test]
    fn greedy_agreement_has_rank_zero() {
        let result = compared(observation(
            "nihao",
            5,
            vec![complete("ni", 0, 2), complete("hao", 2, 5)],
        ));
        assert_eq!(result.outcome, Outcome::Agreement);
        assert_eq!(result.rank, Some(0));
        assert_eq!(result.class, "-");
    }

    #[test]
    fn non_greedy_selection_agrees_at_a_positive_rank() {
        // fangan: ours orders [fang,an] first; the oracle selects [fan,gan],
        // which is our second path. Membership holds, so this is agreement.
        let result = compared(observation(
            "fangan",
            6,
            vec![complete("fan", 0, 3), complete("gan", 3, 6)],
        ));
        assert_eq!(result.outcome, Outcome::Agreement);
        assert_eq!(result.rank, Some(1));
    }

    #[test]
    fn partial_tail_agrees() {
        let result = compared(observation(
            "nih",
            3,
            vec![complete("ni", 0, 2), partial("h", 2, 3)],
        ));
        assert_eq!(result.outcome, Outcome::Agreement);
        assert_eq!(result.rank, Some(0));
    }

    #[test]
    fn apostrophe_gap_agrees() {
        let result = compared(observation(
            "chang'an",
            8,
            vec![complete("chang", 0, 5), complete("an", 6, 8)],
        ));
        assert_eq!(result.outcome, Outcome::Agreement);
        assert_eq!(result.rank, Some(0));
    }

    #[test]
    fn empty_selection_with_nothing_consumed_agrees() {
        // The oracle selected nothing; our parser returns one empty path with
        // the whole input as remainder. Empty matches empty at rank 0.
        let result = compared(observation("!ni", 0, vec![]));
        assert_eq!(result.outcome, Outcome::Agreement);
        assert_eq!(result.rank, Some(0));

        let empty = compared(observation("", 0, vec![]));
        assert_eq!(empty.outcome, Outcome::Agreement);
        assert_eq!(empty.rank, Some(0));
    }

    #[test]
    fn a_segmentation_we_do_not_enumerate_is_path_absent() {
        // "ni" split as [n, i] is not table-valid, so it cannot be in our set.
        let result = compared(observation(
            "ni",
            2,
            vec![complete("n", 0, 1), complete("i", 1, 2)],
        ));
        assert_eq!(result.outcome, Outcome::Divergence(Reason::PathAbsent));
        assert_eq!(result.rank, None);
    }

    #[test]
    fn disagreeing_consumed_length_outranks_path_membership() {
        let mut observed = observation("nihao", 5, vec![complete("ni", 0, 2)]);
        observed.parsed_input_length = 2;
        observed.remainder = b"hao".to_vec();
        let result = compared(observed);
        assert_eq!(result.outcome, Outcome::Divergence(Reason::ConsumedLength));
    }

    #[test]
    fn off_pin_observation_outranks_every_other_reason() {
        let mut observed = observation("ni", 2, vec![complete("n", 0, 1)]);
        observed.pin_ref = "libpinyin-something-else".to_owned();
        let result = compared(observed);
        assert_eq!(result.outcome, Outcome::Divergence(Reason::OffPin));
    }

    #[test]
    fn self_inconsistent_oracle_outranks_comparison() {
        let mut observed = observation("ni", 2, vec![complete("ni", 0, 2)]);
        observed.parsed_input_length = 99;
        let result = compared(observed);
        assert_eq!(
            result.outcome,
            Outcome::Divergence(Reason::OracleInconsistent)
        );
    }

    #[test]
    fn sentinel_segment_is_reported_as_such() {
        let result = compared(observation(
            "nih",
            3,
            vec![
                complete("ni", 0, 2),
                OracleSegment::MissingKeyRest { offset: 2 },
            ],
        ));
        assert_eq!(result.outcome, Outcome::Divergence(Reason::OracleSentinel));
    }

    #[test]
    fn comparison_line_carries_the_frozen_field_order() {
        let result = compared(observation("ni", 2, vec![complete("ni", 0, 2)]));
        let line = comparison_line(&result);
        let keys: Vec<&str> = line
            .split('\t')
            .filter_map(|field| field.split('=').next())
            .collect();
        assert_eq!(
            keys,
            vec![
                "schema",
                "pin_ref",
                "source",
                "corpus",
                "case",
                "input",
                "flags",
                "outcome",
                "class",
                "rank",
                "ours_paths",
                "ours_consumed",
                "theirs_consumed",
                "candidate_total",
            ]
        );
        assert!(line.contains("outcome=agreement"));
        assert!(line.contains("class=-"));
        assert!(line.contains("flags=0x0000018a"));
    }

    #[test]
    fn agreement_produces_no_divergence_line() {
        let result = compared(observation("ni", 2, vec![complete("ni", 0, 2)]));
        assert!(divergence_line(&result).is_none());
    }

    #[test]
    fn divergence_line_spells_out_both_sides() {
        let result = compared(observation(
            "ni",
            2,
            vec![complete("n", 0, 1), complete("i", 1, 2)],
        ));
        let line = divergence_line(&result).expect("divergence");
        assert!(line.contains("reason=path-absent"));
        assert!(line.contains("theirs_path=n@0:1:complete,i@1:2:complete"));
        assert!(line.contains("ours_path_set=ni@0:2:complete"));
        assert!(line.contains("class=unclassified"));
    }

    #[test]
    fn a_run_is_deterministic_and_ordered() {
        let observations = vec![
            observation("ni", 2, vec![complete("ni", 0, 2)]),
            observation(
                "fangan",
                6,
                vec![complete("fan", 0, 3), complete("gan", 3, 6)],
            ),
            observation("ni", 2, vec![complete("n", 0, 1), complete("i", 1, 2)]),
        ];
        let cases: Vec<Case> = ["ni", "fangan"]
            .iter()
            .enumerate()
            .map(|(index, input)| Case {
                corpus: "t".to_owned(),
                case: format!("t:{index}"),
                input: input.as_bytes().to_vec(),
            })
            .collect();

        let mut first = ReplaySource::new(observations.clone());
        let mut second = ReplaySource::new(observations);
        let a = run(&mut first, &cases, &Unclassified);
        let b = run(&mut second, &cases, &Unclassified);

        assert_eq!(a.comparison_log, b.comparison_log);
        assert_eq!(a.report.total, 2);
        assert_eq!(a.report.agreements, 2);
        assert_eq!(a.report.ranks.get(&0), Some(&1));
        assert_eq!(a.report.ranks.get(&1), Some(&1));
        assert!(a.divergence_log.is_empty());
    }

    #[test]
    fn an_input_the_producer_lacks_is_unobservable_not_a_divergence() {
        let mut source = ReplaySource::new(vec![observation("ni", 2, vec![complete("ni", 0, 2)])]);
        let cases = vec![Case {
            corpus: "t".to_owned(),
            case: "absent".to_owned(),
            input: b"nihao".to_vec(),
        }];
        let output = run(&mut source, &cases, &Unclassified);
        assert_eq!(output.report.total, 0);
        assert_eq!(output.report.unobservable, 1);
    }
}
