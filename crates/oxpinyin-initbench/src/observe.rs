//! The correctness-verification phase: complete observations and their
//! comparison.
//!
//! Timing is accepted only for a cell whose subjects were first shown to behave
//! identically here. An observation is a transcript of **every** answer the
//! library gives to a defined script (return values, parsed lengths, sentences,
//! and every candidate's type, text and n-best index, in order), plus the
//! diagnostics the process wrote to stderr and the files it left in its user
//! directory. Nothing is normalised: two plausible outputs that differ are a
//! difference. Observation, serialisation and comparison happen in separate
//! processes from the timed runs and never inside a timed interval.
//!
//! A difference makes the cell [`Verdict::NotComparable`], never a performance
//! win or loss.
//!
//! Inputs the pin is known to abort on, or any input whose outcome is not a
//! normal return, are run one per process ([`ProbeOutcome`]); their outcomes are
//! recorded and compared **separately**: an abort in one subject and a safe
//! refusal in the other is reported as a difference of behaviour, not as
//! equivalence, and no timing is ever taken through one.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::digest::sha256_hex;
use crate::error::BenchError;
use crate::workload::{Candidate, Driver, Facade, ParseKind};

/// Schema tag of an observation.
pub const OBSERVATION_SCHEMA: &str = "oxpinyin-initbench-observation-v1";

/// Inputs run in the main script, in this order, on one instance. The repeated
/// `nihao` at the end checks that state left by the others does not leak
/// through `reset`.
pub const SCRIPT_INPUTS: [&str; 18] = [
    "nihao",
    "ni",
    "n",
    "zh",
    "zhongguo",
    "xi'an",
    "bu'tian",
    "fan'gan",
    "fangan",
    "chongke",
    "caisho",
    "paolen",
    "waimenggu",
    "naoxion",
    "liangniejue",
    "chuaipengdengzaimiu",
    "a",
    "nihao",
];

/// Chewing (bopomofo keyboard) inputs for the zhuyin facade's second parser.
pub const CHEWING_INPUTS: [&str; 3] = ["su3cl3", "ji3", "5j/ "];

/// Inputs that probe the edges of the parser. Each runs alone in its own process
/// (see [`ProbeOutcome`]) because any of them may abort a subject.
pub const HOSTILE_INPUTS: [&str; 12] = [
    "",
    "'",
    "''",
    "'ni",
    "ni'",
    "nihao'",
    "zzzz",
    "12345",
    "ni hao",
    "NIHAO",
    "你好",
    "nihaonihaonihaonihaonihaonihaonihaonihaonihaonihaonihaonihaonihaonihaonihao",
];

/// One answer in a transcript.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Line {
    /// What was asked.
    pub op: String,
    /// What the library answered, rendered exactly.
    pub value: String,
}

/// A file the run left in its user directory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileObs {
    /// Name relative to the user directory.
    pub name: String,
    /// Size in bytes.
    pub size: u64,
    /// SHA-256.
    pub sha256: String,
}

/// Everything observed about one subject on one (facade, profile) cell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Observation {
    /// [`OBSERVATION_SCHEMA`].
    pub schema: String,
    /// The facade driven.
    pub facade: Facade,
    /// SHA-256 of the workload definition that produced the transcript.
    pub workload_digest: String,
    /// The profile fixture's name.
    pub profile: String,
    /// SHA-256 of the timed units' trace (same sequence the timed child runs).
    pub unit_trace_digest: String,
    /// The transcript.
    pub lines: Vec<Line>,
    /// The user directory after `*_fini`.
    pub files: Vec<FileObs>,
    /// What the process wrote to stderr (filled in by the parent).
    pub stderr: String,
}

fn line(op: impl Into<String>, value: impl Into<String>) -> Line {
    Line {
        op: op.into(),
        value: value.into(),
    }
}

fn render_candidate(index: u32, c: &Candidate) -> String {
    format!(
        "{index}:type={}:nbest={:?}:text={}",
        c.raw_type, c.nbest, c.text
    )
}

/// Records every answer to one input, in the order a frontend drives a library:
/// reset, parse, (pinyin) parsed length, sentence guess and read, candidate
/// list, count and every candidate.
///
/// Only sentence indexes the candidate list proves exist are read (the pin
/// asserts on an index past the n-best results), the same rule the retained C
/// instruments use.
///
/// # Errors
///
/// A failed call that is not a value (no instance, interior NUL).
pub fn observe_input<D: Driver>(
    d: &mut D,
    kind: ParseKind,
    input: &str,
    sort: u32,
) -> Result<Vec<Line>, BenchError> {
    let tag = format!("{kind:?}({input:?})");
    let mut out = Vec::new();
    out.push(line(format!("{tag}: reset"), d.reset()?.to_string()));
    let consumed = d.parse(kind, input)?;
    out.push(line(format!("{tag}: parse"), consumed.to_string()));
    if d.facade() == Facade::Pinyin {
        out.push(line(
            format!("{tag}: get_parsed_input_length"),
            d.parsed_length()?.to_string(),
        ));
    }
    let guessed = d.guess_sentence()?;
    out.push(line(format!("{tag}: guess_sentence"), guessed.to_string()));
    if guessed {
        out.push(line(
            format!("{tag}: sentence(0)"),
            format!("{:?}", d.sentence(0)?),
        ));
    }
    let listed = d.guess_candidates(0, sort)?;
    out.push(line(format!("{tag}: guess_candidates"), listed.to_string()));
    let count = d.candidate_count()?;
    out.push(line(
        format!("{tag}: get_n_candidate"),
        format!("{count:?}"),
    ));
    let mut max_rank = 0_u8;
    if let Some(n) = count {
        for index in 0..n {
            match d.candidate(index)? {
                Some(c) => {
                    if let Some(rank) = c.nbest {
                        max_rank = max_rank.max(rank);
                    }
                    out.push(line(
                        format!("{tag}: candidate"),
                        render_candidate(index, &c),
                    ));
                }
                None => out.push(line(format!("{tag}: candidate"), format!("{index}:none"))),
            }
        }
    }
    if guessed && d.facade() == Facade::Pinyin {
        for rank in 1..=max_rank.min(2) {
            out.push(line(
                format!("{tag}: sentence({rank})"),
                format!("{:?}", d.sentence(rank)?),
            ));
        }
    }
    Ok(out)
}

/// The deep script run as the [`crate::workload::run_units`] hook: the state the
/// units left (before any reset), then every input of the script.
///
/// # Errors
///
/// As [`observe_input`].
pub fn observe_script<D: Driver>(
    d: &mut D,
    sort: u32,
    extra_inputs: &[String],
) -> Result<Vec<Line>, BenchError> {
    let mut out = Vec::new();
    // The first-candidate state exactly as the timed units left it.
    let count = d.candidate_count()?;
    out.push(line(
        "state-after-units: get_n_candidate",
        format!("{count:?}"),
    ));
    if let Some(n) = count {
        for index in 0..n {
            match d.candidate(index)? {
                Some(c) => out.push(line(
                    "state-after-units: candidate",
                    render_candidate(index, &c),
                )),
                None => out.push(line(
                    "state-after-units: candidate",
                    format!("{index}:none"),
                )),
            }
        }
    }
    for input in SCRIPT_INPUTS
        .iter()
        .map(|s| (*s).to_owned())
        .chain(extra_inputs.iter().cloned())
    {
        out.extend(observe_input(d, ParseKind::FullPinyin, &input, sort)?);
    }
    if d.facade() == Facade::Zhuyin {
        for input in CHEWING_INPUTS {
            out.extend(observe_input(d, ParseKind::Chewing, input, sort)?);
        }
    }
    Ok(out)
}

/// How a one-input child ended.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitClass {
    /// Exited with status 0 and printed an observation.
    Returned,
    /// Exited with a non-zero status.
    Status(i32),
    /// Killed by a signal (an abort is `SIGABRT`, 6).
    Signal(i32),
}

/// What happened to one hostile input in one subject.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProbeOutcome {
    /// The input.
    pub input: String,
    /// How the process ended.
    pub exit: ExitClass,
    /// The transcript when it returned.
    pub lines: Vec<Line>,
    /// SHA-256 of what it wrote to stderr (diagnostics, an assertion message).
    pub stderr_sha256: String,
}

/// The comparison of two probe outcomes. Kept apart from [`Verdict`]: it does
/// not gate any timing, because no timing runs these inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeVerdict {
    /// Same exit class, and (when both returned) the same transcript and stderr.
    SameOutcome,
    /// A difference, described.
    Different(String),
}

/// Compares two outcomes of the same input.
#[must_use]
pub fn compare_probe(reference: &ProbeOutcome, other: &ProbeOutcome) -> ProbeVerdict {
    if reference.input != other.input {
        return ProbeVerdict::Different("different inputs".to_owned());
    }
    if reference.exit != other.exit {
        return ProbeVerdict::Different(format!(
            "reference ended {:?}, subject ended {:?}",
            reference.exit, other.exit
        ));
    }
    if reference.exit == ExitClass::Returned {
        if let Some(d) = first_difference(&reference.lines, &other.lines) {
            return ProbeVerdict::Different(d);
        }
        if reference.stderr_sha256 != other.stderr_sha256 {
            return ProbeVerdict::Different("diagnostics on stderr differ".to_owned());
        }
    }
    ProbeVerdict::SameOutcome
}

fn first_difference(a: &[Line], b: &[Line]) -> Option<String> {
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if x != y {
            return Some(format!(
                "line {i}: reference `{}` = `{}`, subject `{}` = `{}`",
                x.op, x.value, y.op, y.value
            ));
        }
    }
    (a.len() != b.len()).then(|| format!("transcripts have {} and {} lines", a.len(), b.len()))
}

/// The result of comparing a subject with the reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// Every defined observation is identical.
    Comparable,
    /// At least one is not: no ratio may be reported for this subject.
    NotComparable(Vec<String>),
}

impl Verdict {
    /// Whether timing and memory results may be compared.
    #[must_use]
    pub const fn is_comparable(&self) -> bool {
        matches!(self, Self::Comparable)
    }
}

/// Names of files whose bytes are the same in two runs of the **same** library
/// (a determinism control). Only these can be compared byte for byte across
/// libraries; a file that differs between two runs of one library (a database
/// header with a random file id, for instance) is judged by the logical
/// observations instead, and that classification is reported, not hidden.
#[must_use]
pub fn stable_files(first: &Observation, second: &Observation) -> BTreeSet<String> {
    let by_name: BTreeMap<&str, &FileObs> =
        second.files.iter().map(|f| (f.name.as_str(), f)).collect();
    first
        .files
        .iter()
        .filter(|f| {
            by_name
                .get(f.name.as_str())
                .is_some_and(|g| g.sha256 == f.sha256 && g.size == f.size)
        })
        .map(|f| f.name.clone())
        .collect()
}

/// Compares `other` with `reference`. `stable` is the output of
/// [`stable_files`] for the reference library.
#[must_use]
pub fn compare(reference: &Observation, other: &Observation, stable: &BTreeSet<String>) -> Verdict {
    let mut why = Vec::new();
    if reference.schema != OBSERVATION_SCHEMA || other.schema != OBSERVATION_SCHEMA {
        why.push("an observation has an unknown schema".to_owned());
    }
    if reference.workload_digest != other.workload_digest {
        why.push("the subjects ran different workload definitions".to_owned());
    }
    if reference.facade != other.facade {
        why.push(format!(
            "facades differ: {:?} against {:?}",
            reference.facade, other.facade
        ));
    }
    if reference.profile != other.profile {
        why.push(format!(
            "profiles differ: {} against {}",
            reference.profile, other.profile
        ));
    }
    if reference.unit_trace_digest != other.unit_trace_digest {
        why.push("the timed units' call/answer trace differs".to_owned());
    }
    if let Some(d) = first_difference(&reference.lines, &other.lines) {
        let n = reference
            .lines
            .iter()
            .zip(&other.lines)
            .filter(|(a, b)| a != b)
            .count();
        why.push(format!("{d} ({n} differing line(s) in total)"));
    }
    if reference.stderr != other.stderr {
        why.push("diagnostics on stderr differ".to_owned());
    }
    let names_a: BTreeSet<&str> = reference.files.iter().map(|f| f.name.as_str()).collect();
    let names_b: BTreeSet<&str> = other.files.iter().map(|f| f.name.as_str()).collect();
    if names_a != names_b {
        let mut detail = String::new();
        for n in names_a.symmetric_difference(&names_b) {
            let _ = write!(detail, " {n}");
        }
        why.push(format!(
            "the user directories hold different files after fini:{detail}"
        ));
    }
    let by_name: BTreeMap<&str, &FileObs> =
        other.files.iter().map(|f| (f.name.as_str(), f)).collect();
    for f in &reference.files {
        if let Some(g) = by_name.get(f.name.as_str())
            && stable.contains(&f.name)
            && (g.sha256 != f.sha256 || g.size != f.size)
        {
            why.push(format!(
                "file {} differs in bytes ({} against {})",
                f.name, f.size, g.size
            ));
        }
    }
    if why.is_empty() {
        Verdict::Comparable
    } else {
        Verdict::NotComparable(why)
    }
}

/// SHA-256 of a text, for stderr digests.
#[must_use]
pub fn text_digest(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::workload::testing::{Mock, Script};
    use crate::workload::workload_digest;

    fn obs(lines: &[(&str, &str)]) -> Observation {
        Observation {
            schema: OBSERVATION_SCHEMA.to_owned(),
            facade: Facade::Pinyin,
            workload_digest: workload_digest(),
            profile: "empty".into(),
            unit_trace_digest: "t".into(),
            lines: lines.iter().map(|(a, b)| line(*a, *b)).collect(),
            files: vec![
                FileObs {
                    name: "user.conf".into(),
                    size: 88,
                    sha256: "a".into(),
                },
                FileObs {
                    name: "user.db".into(),
                    size: 10,
                    sha256: "b".into(),
                },
            ],
            stderr: "open /x/user.conf failed.\n".into(),
        }
    }

    fn stable_all(o: &Observation) -> BTreeSet<String> {
        o.files.iter().map(|f| f.name.clone()).collect()
    }

    #[test]
    fn identical_observations_are_comparable() {
        let a = obs(&[("parse", "5"), ("candidate", "0:你好")]);
        assert_eq!(
            compare(&a, &a.clone(), &stable_all(&a)),
            Verdict::Comparable
        );
    }

    #[test]
    fn any_changed_answer_is_not_comparable_and_names_the_line() {
        let a = obs(&[("parse", "5"), ("candidate", "0:你好")]);
        let mut b = a.clone();
        b.lines[1].value = "0:泥好".into();
        let Verdict::NotComparable(why) = compare(&a, &b, &stable_all(&a)) else {
            panic!("accepted")
        };
        assert!(
            why[0].contains("line 1") && why[0].contains("泥好"),
            "{why:?}"
        );
    }

    #[test]
    fn every_kind_of_difference_is_detected() {
        let a = obs(&[("parse", "5")]);
        let stable = stable_all(&a);
        type Mutation = Box<dyn Fn(&mut Observation)>;
        let mutations: Vec<(&str, Mutation)> = vec![
            ("workload", Box::new(|o| o.workload_digest = "other".into())),
            ("facade", Box::new(|o| o.facade = Facade::Zhuyin)),
            ("profile", Box::new(|o| o.profile = "modest".into())),
            ("trace", Box::new(|o| o.unit_trace_digest = "x".into())),
            ("extra line", Box::new(|o| o.lines.push(line("extra", "1")))),
            (
                "missing line",
                Box::new(|o| {
                    o.lines.clear();
                }),
            ),
            ("stderr", Box::new(|o| o.stderr.clear())),
            (
                "extra file",
                Box::new(|o| {
                    o.files.push(FileObs {
                        name: "z".into(),
                        size: 1,
                        sha256: "c".into(),
                    })
                }),
            ),
            (
                "file bytes",
                Box::new(|o| o.files[0].sha256 = "changed".into()),
            ),
            ("schema", Box::new(|o| o.schema = "old".into())),
        ];
        for (name, mutate) in mutations {
            let mut b = a.clone();
            mutate(&mut b);
            assert!(
                !compare(&a, &b, &stable).is_comparable(),
                "mutation `{name}` was not detected"
            );
        }
    }

    #[test]
    fn an_unstable_file_is_judged_by_content_not_bytes_but_the_set_must_match() {
        let a = obs(&[("parse", "5")]);
        let mut rerun = a.clone();
        rerun.files[1].sha256 = "different-in-a-second-run-of-the-same-library".into();
        let stable = stable_files(&a, &rerun);
        assert!(stable.contains("user.conf") && !stable.contains("user.db"));
        let mut b = a.clone();
        b.files[1].sha256 = "other".into();
        assert_eq!(compare(&a, &b, &stable), Verdict::Comparable);
        b.files[0].sha256 = "other".into();
        assert!(
            !compare(&a, &b, &stable).is_comparable(),
            "a stable file must match exactly"
        );
        let mut c = a.clone();
        c.files.pop();
        assert!(
            !compare(&a, &c, &stable).is_comparable(),
            "a missing file is never excused"
        );
    }

    #[test]
    fn probe_outcomes_distinguish_a_crash_from_a_safe_refusal() {
        let returned = ProbeOutcome {
            input: "'".into(),
            exit: ExitClass::Returned,
            lines: vec![line("parse", "0")],
            stderr_sha256: "s".into(),
        };
        let aborted = ProbeOutcome {
            input: "'".into(),
            exit: ExitClass::Signal(6),
            lines: vec![],
            stderr_sha256: "s".into(),
        };
        assert_eq!(
            compare_probe(&returned, &returned.clone()),
            ProbeVerdict::SameOutcome
        );
        assert_eq!(
            compare_probe(&aborted, &aborted.clone()),
            ProbeVerdict::SameOutcome
        );
        let ProbeVerdict::Different(why) = compare_probe(&aborted, &returned) else {
            panic!("equated")
        };
        assert!(why.contains("Signal(6)"), "{why}");
        let mut other = returned.clone();
        other.lines[0].value = "1".into();
        assert!(matches!(
            compare_probe(&returned, &other),
            ProbeVerdict::Different(_)
        ));
        other = returned.clone();
        other.stderr_sha256 = "t".into();
        assert!(matches!(
            compare_probe(&returned, &other),
            ProbeVerdict::Different(_)
        ));
        let mut elsewhere = returned.clone();
        elsewhere.input = "''".into();
        assert!(matches!(
            compare_probe(&returned, &elsewhere),
            ProbeVerdict::Different(_)
        ));
    }

    fn live_mock(zhuyin: bool) -> Mock {
        let mut m = Mock::new(Script {
            facade_zhuyin: zhuyin,
            ..Script::default()
        });
        m.init(std::path::Path::new("s"), std::path::Path::new("u"))
            .unwrap();
        m.alloc_instance().unwrap();
        m
    }

    #[test]
    fn the_script_walks_every_input_in_frontend_order_and_every_candidate() {
        let mut m = live_mock(false);
        let lines = observe_script(&mut m, 0x1e, &["extra".into()]).unwrap();
        assert!(lines[0].op.starts_with("state-after-units"));
        let parses: Vec<&str> = lines
            .iter()
            .filter(|l| l.op.ends_with(": parse"))
            .map(|l| l.op.as_str())
            .collect();
        assert_eq!(parses.len(), SCRIPT_INPUTS.len() + 1);
        // Frontend order within one input: reset, parse, length, sentence guess, candidates.
        let first: Vec<&str> = lines
            .iter()
            .filter(|l| l.op.contains("\"nihao\")"))
            .take(8)
            .map(|l| l.op.rsplit(": ").next().unwrap())
            .collect();
        assert_eq!(
            first[..5],
            [
                "reset",
                "parse",
                "get_parsed_input_length",
                "guess_sentence",
                "sentence(0)"
            ]
        );
        // All three of the mock's candidates are recorded, not just the first.
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.op.contains("\"nihao\")") && l.op.ends_with(": candidate"))
                .count()
                % 3,
            0
        );
    }

    #[test]
    fn the_zhuyin_script_adds_the_chewing_parser_and_skips_the_pinyin_length_call() {
        let mut m = live_mock(true);
        let lines = observe_script(&mut m, 0x1e, &[]).unwrap();
        assert!(lines.iter().any(|l| l.op.starts_with("Chewing(")));
        assert!(
            !lines
                .iter()
                .any(|l| l.op.contains("get_parsed_input_length"))
        );
    }

    #[test]
    fn a_changed_input_list_changes_the_transcript() {
        let mut a = live_mock(false);
        let mut b = live_mock(false);
        let x = observe_script(&mut a, 0x1e, &[]).unwrap();
        let y = observe_script(&mut b, 0x1e, &["extra".into()]).unwrap();
        assert_ne!(x, y, "a mutated workload must be visible in the transcript");
    }

    #[test]
    fn the_input_lists_contain_no_duplicates_except_the_deliberate_repeat() {
        let mut seen = BTreeSet::new();
        for i in &SCRIPT_INPUTS[..SCRIPT_INPUTS.len() - 1] {
            assert!(seen.insert(*i), "{i} repeated");
        }
        assert_eq!(SCRIPT_INPUTS[0], SCRIPT_INPUTS[SCRIPT_INPUTS.len() - 1]);
        let mut hostile = BTreeSet::new();
        for i in HOSTILE_INPUTS {
            assert!(hostile.insert(i));
        }
    }
}
