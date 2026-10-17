//! The benchmark units, defined once.
//!
//! [`run_units`] is the single function that both the timed child and the
//! verification child execute, so "the two subjects ran the same workload" and
//! "the timed run ran what was verified" hold by construction: there is one
//! sequence of calls, and the verification phase adds its deep inspection through
//! the [`run_units`] hook, on the same live instance, never as a second
//! sequence.
//!
//! # The units
//!
//! `p` is `pinyin` or `zhuyin`; the call sequence is the same for both facades
//! except where stated. Option word [`OPTIONS`], first input [`FIRST_INPUT`].
//!
//! | unit | calls, in order | required state before | expected (pin) | timed interval |
//! | --- | --- | --- | --- | --- |
//! | U1 load | `dlopen`, resolve the facade's symbols | none | all symbols present | the open call and the resolution |
//! | U2 init | `p_init(system, user)` | U1; user directory as the profile fixture | non-NULL context | that one call |
//! | U3 options | `p_set_options(ctx, 0x18a)` | U2 | `true` | that one call |
//! | U4 alloc | `p_alloc_instance(ctx)` | U3 | non-NULL instance | that one call |
//! | U5 first candidate | `p_parse_more_full_pinyins(inst, "nihao")`; pinyin: `get_parsed_input_length`; pinyin: `guess_candidates(inst, 0, 0x1e)`, zhuyin: `guess_candidates_after_cursor(inst, 0)`; `get_n_candidate`; `get_candidate(0)`; `get_candidate_type`; `get_candidate_string` (copied out) | U4, no earlier parse | consumed 5; length 5; `true`; at least one candidate; a non-empty string | parse through the string copy |
//! | U6 teardown | `p_free_instance`, then `p_fini` | U5 | none | each call separately |
//!
//! State carried: U2's context into U3 and U4; U4's instance into U5 and U6;
//! the user directory is a private working copy of the profile fixture, so no
//! sample changes what the next one starts from. **Excluded from every timed
//! interval**: building the trace, memory reads, serialisation, the verification
//! hook. Diagnostics the library writes to stderr during the units are captured
//! by the parent and compared in the verification phase.
//!
//! What is *not* a unit: sentence guessing, repeated queries, training, saving.
//! Steady-state throughput is out of scope.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::digest::sha256_hex;
use crate::error::BenchError;
use crate::memory::{MemoryPoint, MemoryProbe};
use crate::sample::{FirstCandidate, PhaseTimes, StopPoint, Timeline};

/// The F-A capture profile's option word: `IS_PINYIN | PINYIN_INCOMPLETE |
/// USE_DIVIDED_TABLE | USE_RESPLIT_TABLE` (`src/pinyin_custom2.h` at the pin).
pub const OPTIONS: u32 = (1 << 1) | (1 << 3) | (1 << 7) | (1 << 8);

/// `SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY` (`src/pinyin.h`).
pub const SORT: u32 = 0x1e;

/// The first input: a complete two-syllable word every full-pinyin model has.
pub const FIRST_INPUT: &str = "nihao";

/// Which public facade a library implements.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Facade {
    /// `libpinyin`.
    Pinyin,
    /// `libzhuyin`.
    Zhuyin,
}

impl Facade {
    /// The symbol prefix.
    #[must_use]
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Pinyin => "pinyin",
            Self::Zhuyin => "zhuyin",
        }
    }

    /// Parses [`Self::prefix`].
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "pinyin" => Some(Self::Pinyin),
            "zhuyin" => Some(Self::Zhuyin),
            _ => None,
        }
    }
}

/// How the dynamic loader binds symbols.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Bind {
    /// `RTLD_NOW`: resolve everything at load.
    Now,
    /// `RTLD_LAZY`: resolve on first call.
    Lazy,
}

/// Which parser entry a parse call uses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseKind {
    /// `p_parse_more_full_pinyins`.
    FullPinyin,
    /// `p_parse_more_chewings`.
    Chewing,
}

/// One candidate as the library reports it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Candidate {
    /// The raw `lookup_candidate_type_t`, unfiltered.
    pub raw_type: i32,
    /// The candidate's UTF-8 string.
    pub text: String,
    /// The n-best index, read only for an n-best candidate.
    pub nbest: Option<u8>,
}

/// One phrase to import or one exported.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PhraseRow {
    /// The phrase.
    pub phrase: String,
    /// Its pinyin, syllables joined by `'`.
    pub pinyin: String,
    /// The import count (`-1` means the default on export).
    pub count: i32,
}

/// One libpinyin or libzhuyin, driven call by call. Implemented over the real
/// library by [`crate::ffi::Native`] and by doubles in tests.
///
/// Every method that cannot run (no context, no instance) is an error, never a
/// call; library answers of `false` or NULL are values, not errors.
pub trait Driver {
    /// The facade.
    fn facade(&self) -> Facade;
    /// `p_init`; `false` when the library returns NULL.
    ///
    /// # Errors
    ///
    /// A context is already live, or a path cannot cross the C boundary.
    fn init(&mut self, system: &Path, user: &Path) -> Result<bool, BenchError>;
    /// `p_set_options`.
    ///
    /// # Errors
    ///
    /// No live context.
    fn set_options(&mut self, bits: u32) -> Result<bool, BenchError>;
    /// `p_alloc_instance`; `false` when NULL.
    ///
    /// # Errors
    ///
    /// No live context, or an instance is already live.
    fn alloc_instance(&mut self) -> Result<bool, BenchError>;
    /// `p_reset`.
    ///
    /// # Errors
    ///
    /// No live instance.
    fn reset(&mut self) -> Result<bool, BenchError>;
    /// A `p_parse_more_*` call; the bytes consumed.
    ///
    /// # Errors
    ///
    /// No live instance or an interior NUL.
    fn parse(&mut self, kind: ParseKind, input: &str) -> Result<usize, BenchError>;
    /// `p_get_parsed_input_length`.
    ///
    /// # Errors
    ///
    /// No live instance.
    fn parsed_length(&mut self) -> Result<usize, BenchError>;
    /// `p_guess_sentence`.
    ///
    /// # Errors
    ///
    /// No live instance.
    fn guess_sentence(&mut self) -> Result<bool, BenchError>;
    /// `p_get_sentence`, copied and released; `None` when the call is `false`.
    ///
    /// # Errors
    ///
    /// No live instance, or `g_free` unavailable.
    fn sentence(&mut self, index: u8) -> Result<Option<String>, BenchError>;
    /// The facade's candidate-list call (`guess_candidates(offset, sort)` for
    /// pinyin, `guess_candidates_after_cursor(offset)` for zhuyin).
    ///
    /// # Errors
    ///
    /// No live instance.
    fn guess_candidates(&mut self, offset: usize, sort: u32) -> Result<bool, BenchError>;
    /// `p_get_n_candidate`; `None` when the call is `false`.
    ///
    /// # Errors
    ///
    /// No live instance.
    fn candidate_count(&mut self) -> Result<Option<u32>, BenchError>;
    /// One candidate, copied; `None` when `p_get_candidate` is `false`.
    ///
    /// # Errors
    ///
    /// No live instance, or a library inconsistency (NULL with success).
    fn candidate(&mut self, index: u32) -> Result<Option<Candidate>, BenchError>;
    /// `p_save`.
    ///
    /// # Errors
    ///
    /// No live context.
    fn save(&mut self) -> Result<bool, BenchError>;
    /// `p_load_phrase_library`.
    ///
    /// # Errors
    ///
    /// No live context.
    fn load_library(&mut self, library: u8) -> Result<bool, BenchError>;
    /// Imports rows into phrase library `library`; one answer per row.
    ///
    /// # Errors
    ///
    /// No live context, a NULL iterator, or an interior NUL.
    fn add_phrases(&mut self, library: u8, rows: &[PhraseRow]) -> Result<Vec<bool>, BenchError>;
    /// Exports every row of phrase library `library` (pinyin facade).
    ///
    /// # Errors
    ///
    /// No live context, an unsupported facade, or a failing step.
    fn export_phrases(&mut self, library: u32) -> Result<Vec<PhraseRow>, BenchError>;
    /// `p_free_instance`.
    ///
    /// # Errors
    ///
    /// No live instance.
    fn free_instance(&mut self) -> Result<(), BenchError>;
    /// `p_fini`.
    ///
    /// # Errors
    ///
    /// No live context, or an instance still live.
    fn fini(&mut self) -> Result<(), BenchError>;
}

/// The canonical text of the workload: units, order, inputs and option words. A
/// change to any of them changes [`workload_digest`], which every sample and
/// every observation carries, so two subjects can only be compared if they ran
/// the same definition.
#[must_use]
pub fn workload_spec() -> String {
    format!(
        "initbench-workload-v1\n\
         options={OPTIONS:#x}\nsort={SORT:#x}\nfirst_input={FIRST_INPUT}\n\
         units=load,init,options,alloc,first_candidate,free_instance,fini\n\
         first_candidate=parse_more_full_pinyins;get_parsed_input_length(pinyin);\
         guess_candidates(pinyin:0,sort)|guess_candidates_after_cursor(zhuyin:0);\
         get_n_candidate;get_candidate(0);get_candidate_type;get_candidate_string\n"
    )
}

/// SHA-256 of [`workload_spec`].
#[must_use]
pub fn workload_digest() -> String {
    sha256_hex(workload_spec().as_bytes())
}

/// What one pass over the units produced.
#[derive(Clone, Debug)]
pub struct UnitsOutcome {
    /// Phase durations.
    pub phases: PhaseTimes,
    /// Cumulative timestamps.
    pub timeline: Timeline,
    /// Time to first usable candidate: end of U5 minus start of U1.
    pub ttfc_ns: Option<u64>,
    /// Every checked answer, in call order, outside every timed interval.
    pub trace: Vec<String>,
    /// Memory readings, as the caller's observer took them.
    pub memory: Vec<MemoryPoint>,
    /// Where the pass ended.
    pub stopped_at: StopPoint,
}

impl UnitsOutcome {
    /// SHA-256 of the trace: equal for two subjects iff they made the same
    /// calls and got the same answers.
    #[must_use]
    pub fn trace_digest(&self) -> String {
        sha256_hex(self.trace.join("\n").as_bytes())
    }
}

/// How a pass is run.
#[derive(Clone, Debug)]
pub struct UnitsConfig<'a> {
    /// The system data directory.
    pub system_dir: &'a Path,
    /// The user directory: a private working copy.
    pub user_dir: &'a Path,
    /// Where to stop; teardown is skipped before [`StopPoint::End`].
    pub stop_after: StopPoint,
    /// Take a memory reading after every phase (memory-mode runs only).
    pub memory_between_phases: bool,
}

fn ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn between(start: Instant, end: Instant) -> u64 {
    ns(end.saturating_duration_since(start))
}

fn must(ok: bool, function: &'static str) -> Result<(), BenchError> {
    if ok {
        Ok(())
    } else {
        Err(BenchError::Call { function })
    }
}

/// U5: parse the first input and read the first candidate, validating every
/// observation. Returns the phase breakdown and the instant the unit ended.
fn first_candidate_unit<D: Driver>(
    driver: &mut D,
    trace: &mut Vec<String>,
) -> Result<(FirstCandidate, Instant), BenchError> {
    let facade = driver.facade();
    let s = Instant::now();
    let consumed = driver.parse(ParseKind::FullPinyin, FIRST_INPUT)?;
    let after_parse = Instant::now();
    let parsed_length = if facade == Facade::Pinyin {
        Some(driver.parsed_length()?)
    } else {
        None
    };
    let after_length = Instant::now();
    let guessed = driver.guess_candidates(0, SORT)?;
    let after_guess = Instant::now();
    let count = driver.candidate_count()?;
    let after_count = Instant::now();
    let first = driver.candidate(0)?;
    let e = Instant::now();
    trace.push(format!("parse({FIRST_INPUT}):{consumed}"));
    if let Some(length) = parsed_length {
        trace.push(format!("get_parsed_input_length:{length}"));
    }
    trace.push(format!("guess_candidates:{guessed}"));
    trace.push(format!("get_n_candidate:{count:?}"));
    match &first {
        Some(c) => trace.push(format!(
            "get_candidate(0):type={}:text={}:nbest={:?}",
            c.raw_type, c.text, c.nbest
        )),
        None => trace.push("get_candidate(0):none".to_owned()),
    }
    if consumed != FIRST_INPUT.len() {
        return Err(BenchError::invalid(format!(
            "the first input parsed {consumed} of {} bytes",
            FIRST_INPUT.len()
        )));
    }
    if let Some(length) = parsed_length
        && length != FIRST_INPUT.len()
    {
        return Err(BenchError::invalid(format!(
            "parsed input length {length}, expected {}",
            FIRST_INPUT.len()
        )));
    }
    must(guessed, "guess_candidates")?;
    let count = count.ok_or(BenchError::Call {
        function: "get_n_candidate",
    })?;
    if count == 0 {
        return Err(BenchError::invalid(
            "zero candidates for the first input, where at least one is expected",
        ));
    }
    let first = first.ok_or(BenchError::Call {
        function: "get_candidate",
    })?;
    if first.text.is_empty() {
        return Err(BenchError::invalid("the first candidate's string is empty"));
    }
    Ok((
        FirstCandidate {
            parse_ns: between(s, after_parse),
            length_ns: between(after_parse, after_length),
            guess_ns: between(after_length, after_guess),
            count_ns: between(after_guess, after_count),
            candidate_ns: between(after_count, e),
            total_ns: between(s, e),
            text: first.text,
            raw_type: first.raw_type,
            count,
        },
        e,
    ))
}

/// Runs the units once.
///
/// `open` is U1 and is timed. `hook` runs after U5 on the live instance and
/// before U6; the verification phase uses it for its deep inspection and the
/// timed phase passes a no-op. Memory is read before the clock starts
/// (`baseline`), after each phase when asked, and once at the end.
///
/// A failure anywhere ends the pass with an error: "no candidate where one is
/// expected" is an error, never a zero.
///
/// # Errors
///
/// Any failed or unexpected step; the partial outcome is discarded.
pub fn run_units<D: Driver>(
    open: impl FnOnce() -> Result<D, BenchError>,
    config: &UnitsConfig<'_>,
    probe: &dyn MemoryProbe,
    hook: &mut dyn FnMut(&mut D) -> Result<(), BenchError>,
) -> Result<UnitsOutcome, BenchError> {
    let mut memory = Vec::new();
    let take = |point: &str, memory: &mut Vec<MemoryPoint>| -> Result<(), BenchError> {
        memory.push(MemoryPoint {
            point: point.to_owned(),
            snapshot: probe.snapshot()?,
        });
        Ok(())
    };
    take("baseline", &mut memory)?;
    let between_phases = config.memory_between_phases;
    macro_rules! phase_end {
        ($name:expr) => {
            if between_phases {
                take($name, &mut memory)?;
            }
        };
    }
    let mut trace: Vec<String> = Vec::new();
    let mut phases = PhaseTimes::default();
    let mut timeline = Timeline::default();
    let t0 = Instant::now();
    let finish = |memory: Vec<MemoryPoint>,
                  phases: PhaseTimes,
                  timeline: Timeline,
                  trace: Vec<String>,
                  ttfc: Option<u64>,
                  stop: StopPoint| UnitsOutcome {
        phases,
        timeline,
        ttfc_ns: ttfc,
        trace,
        memory,
        stopped_at: stop,
    };
    if config.stop_after == StopPoint::Start {
        take("end", &mut memory)?;
        return Ok(finish(
            memory,
            phases,
            timeline,
            trace,
            None,
            StopPoint::Start,
        ));
    }

    // U1 load.
    let mut driver = open()?;
    let e_load = Instant::now();
    phases.lib_load_ns = between(t0, e_load);
    timeline.lib_load_end_ns = phases.lib_load_ns;
    trace.push(format!("load:facade={}", driver.facade().prefix()));
    phase_end!("lib_load");
    if config.stop_after == StopPoint::LibLoad {
        std::mem::forget(driver);
        take("end", &mut memory)?;
        return Ok(finish(
            memory,
            phases,
            timeline,
            trace,
            None,
            StopPoint::LibLoad,
        ));
    }

    // U2 init.
    let s = Instant::now();
    let initialised = driver.init(config.system_dir, config.user_dir)?;
    let e = Instant::now();
    phases.init_ns = between(s, e);
    timeline.init_end_ns = between(t0, e);
    trace.push(format!("init:non_null={initialised}"));
    must(initialised, "init (returned NULL)")?;
    phase_end!("init");
    if config.stop_after == StopPoint::Init {
        std::mem::forget(driver);
        take("end", &mut memory)?;
        return Ok(finish(
            memory,
            phases,
            timeline,
            trace,
            None,
            StopPoint::Init,
        ));
    }

    // U3 options.
    let s = Instant::now();
    let accepted = driver.set_options(OPTIONS)?;
    let e = Instant::now();
    phases.config_ns = between(s, e);
    timeline.config_end_ns = between(t0, e);
    trace.push(format!("set_options({OPTIONS:#x}):{accepted}"));
    must(accepted, "set_options")?;
    phase_end!("options");
    if config.stop_after == StopPoint::Config {
        std::mem::forget(driver);
        take("end", &mut memory)?;
        return Ok(finish(
            memory,
            phases,
            timeline,
            trace,
            None,
            StopPoint::Config,
        ));
    }

    // U4 alloc.
    let s = Instant::now();
    let allocated = driver.alloc_instance()?;
    let e = Instant::now();
    phases.alloc_ns = between(s, e);
    timeline.alloc_end_ns = between(t0, e);
    trace.push(format!("alloc_instance:non_null={allocated}"));
    must(allocated, "alloc_instance (returned NULL)")?;
    phase_end!("alloc");
    if config.stop_after == StopPoint::Alloc {
        std::mem::forget(driver);
        take("end", &mut memory)?;
        return Ok(finish(
            memory,
            phases,
            timeline,
            trace,
            None,
            StopPoint::Alloc,
        ));
    }

    // U5 first candidate.
    let (first_candidate, e) = first_candidate_unit(&mut driver, &mut trace)?;
    phases.first_candidate = Some(first_candidate);
    timeline.first_candidate_end_ns = Some(between(t0, e));
    let ttfc = Some(between(t0, e));
    phase_end!("first_candidate");
    if config.stop_after == StopPoint::FirstCandidate {
        std::mem::forget(driver);
        take("end", &mut memory)?;
        return Ok(finish(
            memory,
            phases,
            timeline,
            trace,
            ttfc,
            StopPoint::FirstCandidate,
        ));
    }

    // The verification hook: outside every interval, on the same instance.
    hook(&mut driver)?;

    // U6 teardown.
    let s = Instant::now();
    driver.free_instance()?;
    let e = Instant::now();
    phases.drop_instance_ns = Some(between(s, e));
    trace.push("free_instance".to_owned());
    let s = Instant::now();
    driver.fini()?;
    let e = Instant::now();
    phases.fini_ns = Some(between(s, e));
    trace.push("fini".to_owned());
    timeline.end_ns = Some(between(t0, e));
    take("end", &mut memory)?;
    Ok(finish(
        memory,
        phases,
        timeline,
        trace,
        ttfc,
        StopPoint::End,
    ))
}

#[cfg(test)]
pub(crate) mod testing {
    //! A scripted [`Driver`] for sequencing, timing-boundary and failure tests.

    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Shared call log.
    pub type Log = Rc<RefCell<Vec<String>>>;

    /// Behaviour switches.
    #[derive(Clone, Default)]
    pub struct Script {
        pub log: Log,
        pub facade_zhuyin: bool,
        pub init_null: bool,
        pub options_false: bool,
        pub alloc_null: bool,
        pub parse_short: bool,
        pub no_guess: bool,
        pub zero_count: bool,
        pub empty_text: bool,
        pub no_candidate: bool,
        pub text: Option<String>,
    }

    pub struct Mock {
        pub s: Script,
        ctx: bool,
        inst: bool,
    }

    impl Mock {
        pub fn new(s: Script) -> Self {
            Self {
                s,
                ctx: false,
                inst: false,
            }
        }
        fn note(&self, entry: &str) {
            self.s.log.borrow_mut().push(entry.to_owned());
        }
    }

    impl Drop for Mock {
        fn drop(&mut self) {
            if self.inst {
                self.note("drop:free_instance");
            }
            if self.ctx {
                self.note("drop:fini");
            }
        }
    }

    fn no(op: &str) -> BenchError {
        BenchError::invalid(format!("{op} without its handle"))
    }

    impl Driver for Mock {
        fn facade(&self) -> Facade {
            if self.s.facade_zhuyin {
                Facade::Zhuyin
            } else {
                Facade::Pinyin
            }
        }
        fn init(&mut self, _: &Path, _: &Path) -> Result<bool, BenchError> {
            self.note("init");
            self.ctx = !self.s.init_null;
            Ok(self.ctx)
        }
        fn set_options(&mut self, bits: u32) -> Result<bool, BenchError> {
            if !self.ctx {
                return Err(no("set_options"));
            }
            self.note(&format!("set_options={bits:#x}"));
            Ok(!self.s.options_false)
        }
        fn alloc_instance(&mut self) -> Result<bool, BenchError> {
            if !self.ctx {
                return Err(no("alloc"));
            }
            self.note("alloc");
            self.inst = !self.s.alloc_null;
            Ok(self.inst)
        }
        fn reset(&mut self) -> Result<bool, BenchError> {
            self.note("reset");
            Ok(true)
        }
        fn parse(&mut self, _: ParseKind, input: &str) -> Result<usize, BenchError> {
            if !self.inst {
                return Err(no("parse"));
            }
            self.note(&format!("parse={input}"));
            Ok(if self.s.parse_short {
                input.len() - 1
            } else {
                input.len()
            })
        }
        fn parsed_length(&mut self) -> Result<usize, BenchError> {
            self.note("parsed_length");
            Ok(FIRST_INPUT.len())
        }
        fn guess_sentence(&mut self) -> Result<bool, BenchError> {
            self.note("guess_sentence");
            Ok(true)
        }
        fn sentence(&mut self, _: u8) -> Result<Option<String>, BenchError> {
            self.note("sentence");
            Ok(Some("你好".into()))
        }
        fn guess_candidates(&mut self, offset: usize, sort: u32) -> Result<bool, BenchError> {
            self.note(&format!("guess_candidates={offset},{sort:#x}"));
            Ok(!self.s.no_guess)
        }
        fn candidate_count(&mut self) -> Result<Option<u32>, BenchError> {
            self.note("count");
            Ok(Some(if self.s.zero_count { 0 } else { 3 }))
        }
        fn candidate(&mut self, index: u32) -> Result<Option<Candidate>, BenchError> {
            self.note(&format!("candidate={index}"));
            if self.s.no_candidate {
                return Ok(None);
            }
            let text = if self.s.empty_text {
                String::new()
            } else {
                self.s.text.clone().unwrap_or_else(|| "你好".into())
            };
            Ok(Some(Candidate {
                raw_type: 2,
                text,
                nbest: None,
            }))
        }
        fn save(&mut self) -> Result<bool, BenchError> {
            Ok(true)
        }
        fn load_library(&mut self, _: u8) -> Result<bool, BenchError> {
            Ok(true)
        }
        fn add_phrases(&mut self, _: u8, rows: &[PhraseRow]) -> Result<Vec<bool>, BenchError> {
            Ok(vec![true; rows.len()])
        }
        fn export_phrases(&mut self, _: u32) -> Result<Vec<PhraseRow>, BenchError> {
            Ok(Vec::new())
        }
        fn free_instance(&mut self) -> Result<(), BenchError> {
            if !self.inst {
                return Err(no("free_instance"));
            }
            self.note("free_instance");
            self.inst = false;
            Ok(())
        }
        fn fini(&mut self) -> Result<(), BenchError> {
            if !self.ctx {
                return Err(no("fini"));
            }
            if self.inst {
                return Err(BenchError::invalid("fini with a live instance"));
            }
            self.note("fini");
            self.ctx = false;
            Ok(())
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::testing::{Mock, Script};
    use super::*;
    use crate::memory::MemorySnapshot;
    use std::cell::RefCell;

    struct FakeProbe(RefCell<u64>);
    impl MemoryProbe for FakeProbe {
        fn snapshot(&self) -> Result<MemorySnapshot, BenchError> {
            let mut n = self.0.borrow_mut();
            *n += 100;
            Ok(MemorySnapshot {
                source: "fake".into(),
                rss_kib: Some(*n),
                ..MemorySnapshot::default()
            })
        }
    }

    fn run(script: &Script, stop: StopPoint, memory: bool) -> Result<UnitsOutcome, BenchError> {
        let s = script.clone();
        let cfg = UnitsConfig {
            system_dir: Path::new("sys"),
            user_dir: Path::new("usr"),
            stop_after: stop,
            memory_between_phases: memory,
        };
        run_units(
            || Ok(Mock::new(s)),
            &cfg,
            &FakeProbe(RefCell::new(0)),
            &mut |_| Ok(()),
        )
    }

    #[test]
    fn the_option_word_is_the_frozen_capture_profile() {
        assert_eq!(OPTIONS, 0x18a);
        assert_eq!(SORT, 0x1e);
    }

    #[test]
    fn units_run_in_the_documented_order_and_teardown_is_instance_then_context() {
        let script = Script::default();
        let outcome = run(&script, StopPoint::End, false).unwrap();
        assert_eq!(
            *script.log.borrow(),
            [
                "init",
                "set_options=0x18a",
                "alloc",
                "parse=nihao",
                "parsed_length",
                "guess_candidates=0,0x1e",
                "count",
                "candidate=0",
                "free_instance",
                "fini"
            ]
        );
        assert_eq!(outcome.stopped_at, StopPoint::End);
        assert!(outcome.phases.drop_instance_ns.is_some() && outcome.phases.fini_ns.is_some());
    }

    #[test]
    fn the_zhuyin_facade_skips_the_pinyin_only_length_call() {
        let script = Script {
            facade_zhuyin: true,
            ..Script::default()
        };
        run(&script, StopPoint::End, false).unwrap();
        assert!(!script.log.borrow().iter().any(|e| e == "parsed_length"));
    }

    #[test]
    fn time_to_first_candidate_is_a_timestamp_difference_bounded_by_the_phases() {
        let o = run(&Script::default(), StopPoint::End, false).unwrap();
        let ttfc = o.ttfc_ns.unwrap();
        assert_eq!(Some(ttfc), o.timeline.first_candidate_end_ns);
        let sum = o.phases.lib_load_ns
            + o.phases.init_ns
            + o.phases.config_ns
            + o.phases.alloc_ns
            + o.phases.first_candidate.as_ref().unwrap().total_ns;
        assert!(
            sum <= ttfc,
            "the phases cannot exceed the interval that contains them"
        );
        assert!(ttfc <= o.timeline.end_ns.unwrap());
    }

    #[test]
    fn the_timeline_is_monotonic() {
        let o = run(&Script::default(), StopPoint::End, false).unwrap();
        let t = &o.timeline;
        let chain = [
            t.lib_load_end_ns,
            t.init_end_ns,
            t.config_end_ns,
            t.alloc_end_ns,
            t.first_candidate_end_ns.unwrap(),
            t.end_ns.unwrap(),
        ];
        assert!(chain.windows(2).all(|w| w[0] <= w[1]), "{chain:?}");
    }

    #[test]
    fn memory_is_read_between_phases_only_when_asked() {
        let names = |o: &UnitsOutcome| o.memory.iter().map(|p| p.point.clone()).collect::<Vec<_>>();
        assert_eq!(
            names(&run(&Script::default(), StopPoint::End, false).unwrap()),
            ["baseline", "end"]
        );
        assert_eq!(
            names(&run(&Script::default(), StopPoint::End, true).unwrap()),
            [
                "baseline",
                "lib_load",
                "init",
                "options",
                "alloc",
                "first_candidate",
                "end"
            ]
        );
    }

    #[test]
    fn every_stop_point_stops_and_skips_teardown() {
        for stop in [
            StopPoint::Start,
            StopPoint::LibLoad,
            StopPoint::Init,
            StopPoint::Config,
            StopPoint::Alloc,
            StopPoint::FirstCandidate,
        ] {
            let script = Script::default();
            let o = run(&script, stop, false).unwrap();
            assert_eq!(o.stopped_at, stop);
            assert!(o.phases.fini_ns.is_none(), "{stop:?}");
            assert!(
                !script
                    .log
                    .borrow()
                    .iter()
                    .any(|e| e.contains("fini") || e.contains("free")),
                "{stop:?} ran teardown (or its drop): {:?}",
                script.log.borrow()
            );
        }
        let o = run(&Script::default(), StopPoint::Alloc, false).unwrap();
        assert!(o.phases.first_candidate.is_none() && o.ttfc_ns.is_none());
    }

    #[test]
    fn every_refusal_is_an_error_not_a_success() {
        for script in [
            Script {
                init_null: true,
                ..Script::default()
            },
            Script {
                options_false: true,
                ..Script::default()
            },
            Script {
                alloc_null: true,
                ..Script::default()
            },
            Script {
                parse_short: true,
                ..Script::default()
            },
            Script {
                no_guess: true,
                ..Script::default()
            },
            Script {
                zero_count: true,
                ..Script::default()
            },
            Script {
                empty_text: true,
                ..Script::default()
            },
            Script {
                no_candidate: true,
                ..Script::default()
            },
        ] {
            assert!(run(&script, StopPoint::End, false).is_err());
        }
    }

    #[test]
    fn a_failure_after_init_still_releases_the_context() {
        let script = Script {
            options_false: true,
            ..Script::default()
        };
        let _ = run(&script, StopPoint::End, false);
        assert_eq!(
            *script.log.borrow(),
            ["init", "set_options=0x18a", "drop:fini"]
        );
    }

    #[test]
    fn a_failed_load_is_an_error() {
        let cfg = UnitsConfig {
            system_dir: Path::new("s"),
            user_dir: Path::new("u"),
            stop_after: StopPoint::End,
            memory_between_phases: false,
        };
        let r = run_units::<Mock>(
            || Err(BenchError::invalid("no library")),
            &cfg,
            &FakeProbe(RefCell::new(0)),
            &mut |_| Ok(()),
        );
        assert!(r.is_err());
    }

    #[test]
    fn the_hook_runs_on_the_live_instance_between_first_candidate_and_teardown() {
        let script = Script::default();
        let s = script.clone();
        let cfg = UnitsConfig {
            system_dir: Path::new("s"),
            user_dir: Path::new("u"),
            stop_after: StopPoint::End,
            memory_between_phases: false,
        };
        let mut seen = Vec::new();
        run_units(
            || Ok(Mock::new(s)),
            &cfg,
            &FakeProbe(RefCell::new(0)),
            &mut |d: &mut Mock| {
                seen.push(script.log.borrow().last().cloned());
                d.reset()?;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(seen, [Some("candidate=0".to_owned())]);
        let log = script.log.borrow();
        let reset = log.iter().position(|e| e == "reset").unwrap();
        let free = log.iter().position(|e| e == "free_instance").unwrap();
        assert!(reset < free);
    }

    #[test]
    fn the_trace_digest_follows_what_the_library_answered() {
        let a = run(&Script::default(), StopPoint::End, false)
            .unwrap()
            .trace_digest();
        assert_eq!(
            a,
            run(&Script::default(), StopPoint::End, false)
                .unwrap()
                .trace_digest()
        );
        let other = Script {
            text: Some("泥好".into()),
            ..Script::default()
        };
        assert_ne!(
            a,
            run(&other, StopPoint::End, false).unwrap().trace_digest()
        );
        let zh = Script {
            facade_zhuyin: true,
            ..Script::default()
        };
        assert_ne!(a, run(&zh, StopPoint::End, false).unwrap().trace_digest());
    }

    #[test]
    fn the_workload_digest_pins_the_definition() {
        assert_eq!(workload_digest(), workload_digest());
        assert!(workload_spec().contains("options=0x18a"));
        assert!(workload_spec().contains("first_input=nihao"));
        assert_eq!(
            workload_digest(),
            crate::digest::sha256_hex(workload_spec().as_bytes())
        );
    }
}
