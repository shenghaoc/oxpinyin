//! The three things a child process does: one timed sample, one verification
//! observation, one hostile-input probe.
//!
//! All three call [`run_units`], so they execute the same call sequence; they
//! differ only in the hook that runs on the live instance after the first
//! candidate and in whether memory is read between phases. Each is generic over
//! how the library is opened, which is what lets the tests drive them with a
//! double.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use crate::error::BenchError;
use crate::memory::MemoryProbe;
use crate::observe::{
    FileObs, Line, OBSERVATION_SCHEMA, Observation, observe_input, observe_script,
};
use crate::profile::Manifest;
use crate::sample::{SAMPLE_SCHEMA, Sample, SampleKind, StopPoint};
use crate::workload::{
    Bind, Driver, Facade, OPTIONS, ParseKind, PhraseRow, SORT, UnitsConfig, UnitsOutcome,
    run_units, workload_digest,
};

/// What a child is asked to do.
#[derive(Clone, Debug)]
pub struct ChildOptions {
    /// The facade.
    pub facade: Facade,
    /// The system data directory.
    pub system_dir: PathBuf,
    /// The user directory (a private working copy).
    pub user_dir: PathBuf,
    /// Timing or memory.
    pub kind: SampleKind,
    /// Where to stop.
    pub stop_after: StopPoint,
    /// Loader binding, recorded in the sample.
    pub bind: Bind,
}

fn wall_elapsed_ns(start: SystemTime) -> i64 {
    match SystemTime::now().duration_since(start) {
        Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_nanos()).unwrap_or(i64::MAX),
    }
}

fn units_config(options: &ChildOptions) -> UnitsConfig<'_> {
    UnitsConfig {
        system_dir: &options.system_dir,
        user_dir: &options.user_dir,
        stop_after: options.stop_after,
        memory_between_phases: options.kind == SampleKind::Memory,
    }
}

/// One timed sample.
///
/// # Errors
///
/// Any failed or unexpected step.
pub fn timed<D: Driver>(
    open: impl FnOnce() -> Result<D, BenchError>,
    options: &ChildOptions,
    probe: &dyn MemoryProbe,
) -> Result<Sample, BenchError> {
    let wall = SystemTime::now();
    let started = Instant::now();
    let outcome = run_units(open, &units_config(options), probe, &mut |_| Ok(()))?;
    let monotonic_total_ns = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let wall_total_ns = wall_elapsed_ns(wall);
    let trace_digest = outcome.trace_digest();
    let UnitsOutcome {
        phases,
        timeline,
        ttfc_ns,
        memory,
        stopped_at,
        ..
    } = outcome;
    Ok(Sample {
        schema: SAMPLE_SCHEMA.to_owned(),
        kind: options.kind,
        facade: options.facade,
        bind: options.bind,
        stop_after: stopped_at,
        phases,
        timeline,
        ttfc_ns,
        workload_digest: workload_digest(),
        trace_digest,
        monotonic_total_ns,
        wall_total_ns,
        memory,
    })
}

/// Lists the user directory as the verification phase compares it.
///
/// # Errors
///
/// An unreadable directory or an entry that is not a regular file.
pub fn list_files(dir: &Path) -> Result<Vec<FileObs>, BenchError> {
    Ok(Manifest::of_directory(dir)?
        .files
        .into_iter()
        .map(|(name, entry)| FileObs {
            name,
            size: entry.size,
            sha256: entry.sha256,
        })
        .collect())
}

/// One verification observation of the subject on the profile in
/// `options.user_dir`. The parent fills in `stderr`.
///
/// # Errors
///
/// Any failed step.
pub fn verify<D: Driver>(
    open: impl FnOnce() -> Result<D, BenchError>,
    options: &ChildOptions,
    profile: &str,
    witness: &[String],
    probe: &dyn MemoryProbe,
) -> Result<Observation, BenchError> {
    let lines: RefCell<Vec<Line>> = RefCell::new(Vec::new());
    let mut cfg = options.clone();
    cfg.stop_after = StopPoint::End;
    cfg.kind = SampleKind::Timing;
    let outcome = run_units(open, &units_config(&cfg), probe, &mut |d: &mut D| {
        *lines.borrow_mut() = observe_script(d, SORT, witness)?;
        Ok(())
    })?;
    Ok(Observation {
        schema: OBSERVATION_SCHEMA.to_owned(),
        facade: options.facade,
        workload_digest: workload_digest(),
        profile: profile.to_owned(),
        unit_trace_digest: outcome.trace_digest(),
        lines: lines.into_inner(),
        files: list_files(&options.user_dir)?,
        stderr: String::new(),
    })
}

/// One hostile input, alone in its process: the units, then the transcript for
/// that input. A crash is the child's exit status, observed by the parent.
///
/// # Errors
///
/// Any failed step.
pub fn probe_input<D: Driver>(
    open: impl FnOnce() -> Result<D, BenchError>,
    options: &ChildOptions,
    input: &str,
    probe: &dyn MemoryProbe,
) -> Result<Vec<Line>, BenchError> {
    let lines: RefCell<Vec<Line>> = RefCell::new(Vec::new());
    let mut cfg = options.clone();
    cfg.stop_after = StopPoint::End;
    cfg.kind = SampleKind::Timing;
    run_units(open, &units_config(&cfg), probe, &mut |d: &mut D| {
        *lines.borrow_mut() = observe_input(d, ParseKind::FullPinyin, input, SORT)?;
        Ok(())
    })?;
    Ok(lines.into_inner())
}

/// Rows imported by the save-and-reopen scenario: a toned spelling, two toneless
/// export-style spellings with a large count, and a row the library may refuse
/// (a negative count), so the scenario also records per-row answers.
#[must_use]
pub fn persist_rows() -> Vec<PhraseRow> {
    let row = |p: &str, y: &str, c: i32| PhraseRow {
        phrase: p.to_owned(),
        pinyin: y.to_owned(),
        count: c,
    };
    vec![
        row("测试", "ce4'shi4", 5),
        row("爱河", "ai'he", 20000),
        row("锕", "a", 20000),
        row("测侧", "ce4'ce4", -1),
    ]
}

/// Inputs queried after the reopen of the persistence scenario.
pub const PERSIST_INPUTS: [&str; 5] = ["ceshi", "aihe", "a", "cece", "nihao"];

/// The save-and-reopen scenario: import rows, save, close, reopen the same user
/// directory, and observe. Everything the library answers is recorded, then the
/// user directory is listed. The caller compares two subjects' observations
/// exactly (and the reference against a second run of itself, to learn which
/// files are byte-stable).
///
/// # Errors
///
/// Any failed step; a `false` `save` after a successful import is an error (the
/// pin returns `false` only for an unmodified store).
pub fn persist<D: Driver>(
    open: impl FnOnce() -> Result<D, BenchError>,
    options: &ChildOptions,
) -> Result<Observation, BenchError> {
    let mut d = open()?;
    let mut lines: Vec<Line> = Vec::new();
    let mut note = |op: &str, value: String| {
        lines.push(Line {
            op: op.to_owned(),
            value,
        })
    };
    note(
        "init",
        d.init(&options.system_dir, &options.user_dir)?.to_string(),
    );
    note(
        &format!("set_options({OPTIONS:#x})"),
        d.set_options(OPTIONS)?.to_string(),
    );
    note("load_phrase_library(1)", d.load_library(1)?.to_string());
    let accepted = d.add_phrases(1, &persist_rows())?;
    note("add_phrases(1)", format!("{accepted:?}"));
    let saved = d.save()?;
    note("save", saved.to_string());
    if !saved {
        return Err(BenchError::Call {
            function: "save (false after a modifying import)",
        });
    }
    d.fini()?;
    note("fini", "ok".to_owned());
    // Reopen the same directory.
    note(
        "reinit",
        d.init(&options.system_dir, &options.user_dir)?.to_string(),
    );
    note("reset_options", d.set_options(OPTIONS)?.to_string());
    note("alloc_instance", d.alloc_instance()?.to_string());
    for input in PERSIST_INPUTS {
        for l in observe_input(&mut d, ParseKind::FullPinyin, input, SORT)? {
            note(&l.op, l.value);
        }
    }
    d.free_instance()?;
    d.fini()?;
    Ok(Observation {
        schema: OBSERVATION_SCHEMA.to_owned(),
        facade: options.facade,
        workload_digest: workload_digest(),
        profile: "persist:save-reopen".to_owned(),
        unit_trace_digest: String::new(),
        lines,
        files: list_files(&options.user_dir)?,
        stderr: String::new(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::memory::MemorySnapshot;
    use crate::workload::testing::{Mock, Script};

    struct Probe;
    impl MemoryProbe for Probe {
        fn snapshot(&self) -> Result<MemorySnapshot, BenchError> {
            Ok(MemorySnapshot {
                source: "fake".into(),
                rss_kib: Some(1),
                ..MemorySnapshot::default()
            })
        }
    }

    fn options(dir: &Path, kind: SampleKind) -> ChildOptions {
        ChildOptions {
            facade: Facade::Pinyin,
            system_dir: PathBuf::from("sys"),
            user_dir: dir.to_path_buf(),
            kind,
            stop_after: StopPoint::End,
            bind: Bind::Now,
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("initbench-child-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_timed_sample_carries_the_workload_and_trace_identity() {
        let dir = scratch("timed");
        let s = timed(
            || Ok(Mock::new(Script::default())),
            &options(&dir, SampleKind::Timing),
            &Probe,
        )
        .unwrap();
        assert_eq!(s.schema, SAMPLE_SCHEMA);
        assert_eq!(s.workload_digest, workload_digest());
        assert_eq!(s.stop_after, StopPoint::End);
        assert!(s.ttfc_ns.is_some());
        assert_eq!(s.trace_digest.len(), 64);
    }

    #[test]
    fn the_verification_child_runs_the_same_units_as_the_timed_child() {
        let dir = scratch("same");
        let t = timed(
            || Ok(Mock::new(Script::default())),
            &options(&dir, SampleKind::Timing),
            &Probe,
        )
        .unwrap();
        let v = verify(
            || Ok(Mock::new(Script::default())),
            &options(&dir, SampleKind::Timing),
            "empty",
            &[],
            &Probe,
        )
        .unwrap();
        assert_eq!(
            v.unit_trace_digest, t.trace_digest,
            "one call sequence, two entry points"
        );
        assert!(!v.lines.is_empty());
        assert_eq!(v.profile, "empty");
    }

    #[test]
    fn the_hook_does_not_enter_the_timed_trace() {
        // Extra witness inputs change the transcript but not the unit trace.
        let dir = scratch("hook");
        let a = verify(
            || Ok(Mock::new(Script::default())),
            &options(&dir, SampleKind::Timing),
            "p",
            &[],
            &Probe,
        )
        .unwrap();
        let b = verify(
            || Ok(Mock::new(Script::default())),
            &options(&dir, SampleKind::Timing),
            "p",
            &["extra".into()],
            &Probe,
        )
        .unwrap();
        assert_eq!(a.unit_trace_digest, b.unit_trace_digest);
        assert_ne!(a.lines, b.lines);
    }

    #[test]
    fn a_failed_unit_fails_every_entry_point() {
        let dir = scratch("fail");
        let bad = || {
            Ok(Mock::new(Script {
                zero_count: true,
                ..Script::default()
            }))
        };
        let o = options(&dir, SampleKind::Timing);
        assert!(timed(bad, &o, &Probe).is_err());
        assert!(verify(bad, &o, "p", &[], &Probe).is_err());
        assert!(probe_input(bad, &o, "x", &Probe).is_err());
    }

    #[test]
    fn the_probe_records_one_inputs_transcript() {
        let dir = scratch("probe");
        let lines = probe_input(
            || Ok(Mock::new(Script::default())),
            &options(&dir, SampleKind::Timing),
            "'",
            &Probe,
        )
        .unwrap();
        assert!(lines.iter().all(|l| l.op.contains("\"'\"")));
        assert!(lines.iter().any(|l| l.op.ends_with(": parse")));
    }

    #[test]
    fn the_persistence_scenario_imports_saves_closes_and_reopens() {
        let dir = scratch("persist");
        let script = Script::default();
        let log = script.log.clone();
        let o = persist(|| Ok(Mock::new(script)), &options(&dir, SampleKind::Timing)).unwrap();
        let ops: Vec<&str> = o.lines.iter().map(|l| l.op.as_str()).collect();
        assert_eq!(
            &ops[..9],
            [
                "init",
                "set_options(0x18a)",
                "load_phrase_library(1)",
                "add_phrases(1)",
                "save",
                "fini",
                "reinit",
                "reset_options",
                "alloc_instance"
            ]
        );
        assert_eq!(o.profile, "persist:save-reopen");
        // Per-row answers are recorded, including the refused row.
        let add = o.lines.iter().find(|l| l.op == "add_phrases(1)").unwrap();
        assert_eq!(add.value, "[true, true, true, true]");
        // The reopened instance is queried for every input, then released in order.
        let tail: Vec<String> = log.borrow().iter().rev().take(2).cloned().collect();
        assert_eq!(tail, ["fini", "free_instance"]);
        assert!(o.lines.iter().any(|l| l.op.contains("\"ceshi\"")));
    }

    #[test]
    fn user_directory_files_are_listed_after_the_run() {
        let dir = scratch("files");
        std::fs::write(dir.join("user.conf"), b"x").unwrap();
        let v = verify(
            || Ok(Mock::new(Script::default())),
            &options(&dir, SampleKind::Timing),
            "p",
            &[],
            &Probe,
        )
        .unwrap();
        assert_eq!(v.files.len(), 1);
        assert_eq!(v.files[0].name, "user.conf");
    }

    #[test]
    fn a_memory_sample_reads_between_phases_and_a_timing_sample_does_not() {
        let dir = scratch("mem");
        let t = timed(
            || Ok(Mock::new(Script::default())),
            &options(&dir, SampleKind::Timing),
            &Probe,
        )
        .unwrap();
        let m = timed(
            || Ok(Mock::new(Script::default())),
            &options(&dir, SampleKind::Memory),
            &Probe,
        )
        .unwrap();
        assert_eq!(t.memory.len(), 2);
        assert_eq!(m.memory.len(), 7);
    }
}
