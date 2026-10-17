//! The record one child process writes: what was timed, in what order, and what
//! the library answered.
//!
//! A sample is evidence only for what it says. A [`SampleKind::Memory`] sample
//! read process memory between phases, which lengthens everything after the first
//! read; its timings are never aggregated as timings.

use serde::{Deserialize, Serialize};

use crate::memory::MemoryPoint;
use crate::workload::{Bind, Facade};

/// Schema tag of a sample line.
pub const SAMPLE_SCHEMA: &str = "oxpinyin-initbench-sample-v1";

/// What the sample is evidence of.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleKind {
    /// No memory reads between phases: the timings are the evidence.
    Timing,
    /// Memory read after every phase: the memory figures are the evidence.
    Memory,
}

/// The phase after which a child stops (and skips teardown), for whole-process
/// profilers that attribute cost by subtracting two runs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopPoint {
    /// Before anything: the probe process alone.
    Start,
    /// After the library is opened and its symbols resolved.
    LibLoad,
    /// After `*_init`.
    Init,
    /// After the option word is set.
    Config,
    /// After `*_alloc_instance`.
    Alloc,
    /// After the first usable candidate.
    FirstCandidate,
    /// The whole protocol, teardown included.
    End,
}

impl StopPoint {
    /// Every stop point in protocol order.
    pub const ALL: [Self; 7] = [
        Self::Start,
        Self::LibLoad,
        Self::Init,
        Self::Config,
        Self::Alloc,
        Self::FirstCandidate,
        Self::End,
    ];

    /// The command-line spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::LibLoad => "lib_load",
            Self::Init => "init",
            Self::Config => "config",
            Self::Alloc => "alloc",
            Self::FirstCandidate => "first_candidate",
            Self::End => "end",
        }
    }

    /// Parses [`Self::name`].
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|point| point.name() == text)
    }
}

/// The first usable result: parse, build candidates, read the first string.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FirstCandidate {
    /// `*_parse_more_full_pinyins`.
    pub parse_ns: u64,
    /// `*_get_parsed_input_length` (pinyin facade; zero for zhuyin).
    pub length_ns: u64,
    /// The candidate-list call.
    pub guess_ns: u64,
    /// `*_get_n_candidate`.
    pub count_ns: u64,
    /// `*_get_candidate`, its type and string, copied out.
    pub candidate_ns: u64,
    /// The whole phase, first call to last.
    pub total_ns: u64,
    /// The first candidate's text.
    pub text: String,
    /// Its raw `lookup_candidate_type_t`.
    pub raw_type: i32,
    /// The reported candidate count.
    pub count: u32,
}

/// Duration of each phase, nanoseconds on the monotonic clock.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PhaseTimes {
    /// Opening the shared object and resolving its symbols.
    pub lib_load_ns: u64,
    /// `*_init`, exactly that call.
    pub init_ns: u64,
    /// Setting the option word.
    pub config_ns: u64,
    /// `*_alloc_instance`, exactly that call.
    pub alloc_ns: u64,
    /// The first usable result, when the sample made one.
    pub first_candidate: Option<FirstCandidate>,
    /// Freeing the instance.
    pub drop_instance_ns: Option<u64>,
    /// `*_fini`.
    pub fini_ns: Option<u64>,
}

/// Cumulative time from the start of library load to the end of each phase,
/// read from the same clock readings the phases are computed from.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Timeline {
    /// End of library load.
    pub lib_load_end_ns: u64,
    /// End of `*_init`.
    pub init_end_ns: u64,
    /// End of option setting.
    pub config_end_ns: u64,
    /// End of `*_alloc_instance`.
    pub alloc_end_ns: u64,
    /// End of the first usable result.
    pub first_candidate_end_ns: Option<u64>,
    /// End of teardown.
    pub end_ns: Option<u64>,
}

/// One child process's complete record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Sample {
    /// [`SAMPLE_SCHEMA`].
    pub schema: String,
    /// What the sample is evidence of.
    pub kind: SampleKind,
    /// The facade driven.
    pub facade: Facade,
    /// Loader binding in force.
    pub bind: Bind,
    /// Where the child stopped.
    pub stop_after: StopPoint,
    /// Phase durations.
    pub phases: PhaseTimes,
    /// Cumulative timestamps.
    pub timeline: Timeline,
    /// Time to first usable candidate: end of the first-result phase minus the
    /// start of library load, both from the monotonic clock. Includes the gaps
    /// between phases; `None` when no first candidate was made.
    pub ttfc_ns: Option<u64>,
    /// SHA-256 of the workload definition the child ran.
    pub workload_digest: String,
    /// SHA-256 of every call answer, in order (equal iff the same calls got the
    /// same answers).
    pub trace_digest: String,
    /// Elapsed on the monotonic clock over the whole protocol.
    pub monotonic_total_ns: u64,
    /// Elapsed on the wall clock over the same window; a gap against the
    /// monotonic figure means the host slept or its clock was stepped.
    pub wall_total_ns: i64,
    /// Memory readings, in order.
    pub memory: Vec<MemoryPoint>,
}

impl Sample {
    /// The four set-up phases plus the first-result phase, gaps excluded.
    #[must_use]
    pub fn setup_plus_first_candidate_ns(&self) -> Option<u64> {
        let first = self.phases.first_candidate.as_ref()?;
        self.phases
            .lib_load_ns
            .checked_add(self.phases.init_ns)?
            .checked_add(self.phases.config_ns)?
            .checked_add(self.phases.alloc_ns)?
            .checked_add(first.total_ns)
    }
}
