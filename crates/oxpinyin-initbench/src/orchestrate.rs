//! Running the benchmark: verify first, then time, one fresh child per sample.
//!
//! For every (facade, profile) cell the orchestrator
//!
//! 1. **verifies**: runs the verification child once per subject on the same
//!    profile at the same path, captures the stderr diagnostics, runs the
//!    reference a second time (a determinism control that decides which user
//!    files can be compared byte for byte), and compares every subject with the
//!    reference; the hostile inputs are run one per process and their outcomes
//!    recorded and compared separately;
//! 2. **times**: alternates the subjects over fresh child processes with private
//!    profile copies, rotating who goes first, and checks that every timed
//!    sample's trace digest is the one its subject was verified to produce.
//!
//! A cell that fails (1) or whose timed samples contradict it is
//! **NOT COMPARABLE**: its timings are still recorded and still shown as
//! absolute figures, but no ratio is ever computed for it.
//!
//! What is *rejected* (a sample dropped with a reason) and what is a *failure*
//! (the run stops) is as before: clock-skewed or overloaded samples are rejected;
//! a child that exits non-zero, times out, or prints something that is not a
//! sample is a failure. A behavioural difference between subjects is neither: it
//! is a verdict.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::digest::sha256_file;
use crate::error::BenchError;
use crate::observe::{
    ExitClass, HOSTILE_INPUTS, Line, Observation, ProbeOutcome, ProbeVerdict, Verdict, compare,
    compare_probe, stable_files, text_digest,
};
use crate::profile::{Fixture, Manifest, WITNESS_FILE};
use crate::sample::{Sample, SampleKind, StopPoint};
use crate::workload::{Bind, Facade, workload_digest};

/// Schema tag of a run manifest line.
pub const RUN_SCHEMA: &str = "oxpinyin-initbench-run-v1";

/// What is known about the filesystem cache at the start of a sample.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheState {
    /// Nothing was done. A fresh process is *not* a cold cache.
    Warm,
    /// The page cache was dropped (`sync`, then the host's facility)
    /// immediately before the process started and the drop succeeded. It says
    /// nothing about caches below the guest.
    ColdDropCaches,
}

/// A library under test.
#[derive(Clone, Debug)]
pub struct SubjectSpec {
    /// The name reports use.
    pub label: String,
    /// The shared object that serves the pinyin facade.
    pub pinyin: Option<PathBuf>,
    /// The shared object that serves the zhuyin facade.
    pub zhuyin: Option<PathBuf>,
}

impl SubjectSpec {
    fn path(&self, facade: Facade) -> Option<&Path> {
        match facade {
            Facade::Pinyin => self.pinyin.as_deref(),
            Facade::Zhuyin => self.zhuyin.as_deref(),
        }
    }
}

/// What to run.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The binary started as each child.
    pub probe: PathBuf,
    /// The subjects; the first is the reference every other is compared with.
    pub subjects: Vec<SubjectSpec>,
    /// The facades to cover.
    pub facades: Vec<Facade>,
    /// The one system data directory every subject reads.
    pub system_dir: PathBuf,
    /// Profile fixture directories.
    pub profiles: Vec<PathBuf>,
    /// Measured rounds per (facade, profile, subject); 0 runs the verification
    /// phase only.
    pub rounds: usize,
    /// Rounds run first and excluded from reports.
    pub warmup: usize,
    /// Timing or memory samples.
    pub kind: SampleKind,
    /// Cache state to establish before each sample.
    pub cache: CacheState,
    /// A CPU to pin each child to (`taskset`, Linux).
    pub cpu: Option<u32>,
    /// Loader binding.
    pub bind: Bind,
    /// Parent of the private working copies.
    pub work_dir: PathBuf,
    /// Largest wall-versus-monotonic disagreement a sample may show.
    pub max_skew: Duration,
    /// Largest one-minute load average a sample may start under.
    pub max_load: Option<f64>,
    /// Longest a child may run.
    pub timeout: Duration,
}

/// The host a run happened on.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostInfo {
    /// `std::env::consts::OS`.
    pub os: String,
    /// `std::env::consts::ARCH`.
    pub arch: String,
    /// Logical CPUs available to the process.
    pub cpus: usize,
    /// Kernel release, where the host reports one.
    pub kernel: Option<String>,
}

impl HostInfo {
    /// Describes this host.
    #[must_use]
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            cpus: thread::available_parallelism().map_or(0, std::num::NonZero::get),
            kernel: fs::read_to_string("/proc/sys/kernel/osrelease")
                .ok()
                .map(|s| s.trim().to_owned()),
        }
    }
}

/// A library by content.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubjectIdentity {
    /// The report label.
    pub label: String,
    /// Pinyin facade library: file name, size, SHA-256.
    pub pinyin: Option<FileIdentity>,
    /// Zhuyin facade library.
    pub zhuyin: Option<FileIdentity>,
}

/// A file by content.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileIdentity {
    /// The file's name (the directory is not recorded).
    pub file_name: String,
    /// Size in bytes.
    pub size: u64,
    /// SHA-256.
    pub sha256: String,
}

/// A directory by content.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TreeIdentity {
    /// Name (the directory's last component).
    pub name: String,
    /// Digest naming the listing.
    pub identity: String,
    /// Number of files.
    pub files: usize,
    /// Total bytes.
    pub bytes: u64,
}

/// First record of a run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunManifest {
    /// [`RUN_SCHEMA`].
    pub schema: String,
    /// The host.
    pub host: HostInfo,
    /// SHA-256 of the benchmark binary.
    pub probe_sha256: String,
    /// SHA-256 of the workload definition.
    pub workload_digest: String,
    /// The subjects, reference first.
    pub subjects: Vec<SubjectIdentity>,
    /// The system data directory.
    pub system_data: TreeIdentity,
    /// The profile fixtures.
    pub profiles: Vec<TreeIdentity>,
    /// The facades covered.
    pub facades: Vec<Facade>,
    /// Timing or memory.
    pub kind: SampleKind,
    /// Measured rounds.
    pub rounds: usize,
    /// Warm-up rounds.
    pub warmup: usize,
    /// Cache state.
    pub cache: CacheState,
    /// CPU pinning.
    pub cpu: Option<u32>,
    /// Loader binding.
    pub bind: Bind,
}

/// The verdict of the equivalence check for one subject on one cell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerdictRecord {
    /// The facade.
    pub facade: Facade,
    /// The profile.
    pub profile: String,
    /// The subject compared.
    pub subject: String,
    /// The reference it was compared with.
    pub reference: String,
    /// `observation` (the verification phase) or `timed-samples` (the timed runs
    /// contradicting it).
    pub phase: String,
    /// Whether timing and memory may be compared.
    pub comparable: bool,
    /// Why not, in full.
    pub reasons: Vec<String>,
    /// User files compared byte for byte (stable across two runs of the
    /// reference).
    pub files_compared_by_bytes: Vec<String>,
    /// User files that differ between two runs of the reference itself and were
    /// therefore judged by the logical observations only.
    pub files_judged_by_content: Vec<String>,
    /// Lines in the compared transcripts.
    pub transcript_lines: usize,
}

/// A hostile input's outcome in one subject, with its comparison.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProbeRecord {
    /// The facade.
    pub facade: Facade,
    /// The subject.
    pub subject: String,
    /// What happened.
    pub outcome: ProbeOutcome,
    /// `reference`, `same outcome`, or `DIFFERENT: …`.
    pub against_reference: String,
}

/// One accepted sample with its context.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SampleRecord {
    /// Round index (0-based, warm-up rounds first).
    pub round: usize,
    /// Whether this was a warm-up round.
    pub warmup: bool,
    /// The facade.
    pub facade: Facade,
    /// Subject label.
    pub subject: String,
    /// Profile name.
    pub profile: String,
    /// Parent-observed wall time from spawn to exit (includes exec, teardown of
    /// the process and, with `--cpu`, the `taskset` exec).
    pub process_wall_ns: u64,
    /// One-minute load average when the sample started, where the host has one.
    pub loadavg_1m: Option<f64>,
    /// The child's record.
    pub sample: Sample,
}

/// One dropped sample.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RejectedRecord {
    /// Round index.
    pub round: usize,
    /// Facade.
    pub facade: Facade,
    /// Subject label.
    pub subject: String,
    /// Profile name.
    pub profile: String,
    /// Why.
    pub reason: String,
}

/// A line of the result stream.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Record {
    /// First line.
    Manifest(Box<RunManifest>),
    /// An equivalence verdict.
    Verdict(Box<VerdictRecord>),
    /// A hostile-input outcome.
    Probe(Box<ProbeRecord>),
    /// An accepted sample.
    Sample(Box<SampleRecord>),
    /// A rejected sample.
    Rejected(RejectedRecord),
}

/// Totals of a finished run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Summary {
    /// Accepted measured samples (warm-up excluded).
    pub accepted: usize,
    /// Rejected samples, warm-up included.
    pub rejected: usize,
    /// (facade, profile, subject) cells that are NOT COMPARABLE.
    pub not_comparable: usize,
}

/// Why a finished sample is not evidence, if it is not.
#[must_use]
pub fn contamination(
    sample: &Sample,
    max_skew: Duration,
    loadavg: Option<f64>,
    max_load: Option<f64>,
) -> Option<String> {
    let skew = sample
        .wall_total_ns
        .saturating_sub(i64::try_from(sample.monotonic_total_ns).unwrap_or(i64::MAX))
        .unsigned_abs();
    if u128::from(skew) > max_skew.as_nanos() {
        return Some(format!(
            "wall clock and monotonic clock disagree by {} ms over the sample (suspend or clock step)",
            skew / 1_000_000
        ));
    }
    if let (Some(load), Some(limit)) = (loadavg, max_load)
        && load > limit
    {
        return Some(format!(
            "one-minute load average {load:.2} exceeds the allowed {limit:.2}"
        ));
    }
    None
}

/// What a finished process left, success or not.
#[derive(Debug)]
pub struct RawOutput {
    /// The exit status.
    pub status: ExitStatus,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// Wall time from spawn to exit, nanoseconds.
    pub wall_ns: u64,
    /// The command, for messages.
    pub command: String,
}

fn drain(mut stream: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// Runs `command` to completion under `timeout` and returns whatever happened,
/// including a non-zero exit or a signal.
///
/// # Errors
///
/// A spawn failure or a timeout (the child is killed).
pub fn run_raw(mut command: Command, timeout: Duration) -> Result<RawOutput, BenchError> {
    let text = format!("{command:?}");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let start = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|e| BenchError::io(format!("starting {text}"), e))?;
    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(BenchError::invalid("child pipes were not created"));
    };
    let out_reader = drain(out);
    let err_reader = drain(err);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BenchError::Child {
                    command: text,
                    code: None,
                    stderr: format!("timed out after {} s", timeout.as_secs()),
                });
            }
            Ok(None) => thread::sleep(Duration::from_millis(1)),
            Err(e) => return Err(BenchError::io(format!("waiting for {text}"), e)),
        }
    };
    let wall_ns = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
    Ok(RawOutput {
        status,
        stdout: out_reader.join().unwrap_or_default(),
        stderr: err_reader.join().unwrap_or_default(),
        wall_ns,
        command: text,
    })
}

/// [`run_raw`], but a non-zero exit is an error carrying the command and stderr.
///
/// # Errors
///
/// A spawn failure, a timeout, or a non-zero exit.
pub fn run_process(command: Command, timeout: Duration) -> Result<RawOutput, BenchError> {
    let out = run_raw(command, timeout)?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(BenchError::Child {
            command: out.command.clone(),
            code: out.status.code(),
            stderr: out.stderr.chars().take(2000).collect(),
        })
    }
}

/// Classifies how a process ended.
#[must_use]
pub fn exit_class(status: &ExitStatus) -> ExitClass {
    if status.success() {
        return ExitClass::Returned;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return ExitClass::Signal(signal);
        }
    }
    ExitClass::Status(status.code().unwrap_or(-1))
}

fn json_line<T: for<'de> Deserialize<'de>>(stdout: &str) -> Result<T, BenchError> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .ok_or_else(|| BenchError::invalid("the child printed nothing"))?;
    Ok(serde_json::from_str(line)?)
}

/// Parses the single sample line a timing child prints.
///
/// # Errors
///
/// No line, or a line that is not a sample of this schema.
pub fn parse_sample(stdout: &str) -> Result<Sample, BenchError> {
    let sample: Sample = json_line(stdout)?;
    if sample.schema != crate::sample::SAMPLE_SCHEMA {
        return Err(BenchError::invalid(format!(
            "unexpected sample schema {}",
            sample.schema
        )));
    }
    Ok(sample)
}

/// The lines a probe child prints.
#[derive(Debug, Deserialize, Serialize)]
pub struct ProbeLines {
    /// The transcript.
    pub lines: Vec<Line>,
}

fn base_args(sub: &str, plan: &Plan, facade: Facade, lib: &Path, user_dir: &Path) -> Vec<OsString> {
    vec![
        sub.into(),
        "--facade".into(),
        facade.prefix().into(),
        "--lib".into(),
        lib.as_os_str().to_owned(),
        "--system-dir".into(),
        plan.system_dir.as_os_str().to_owned(),
        "--user-dir".into(),
        user_dir.as_os_str().to_owned(),
        "--bind".into(),
        match plan.bind {
            Bind::Now => "now",
            Bind::Lazy => "lazy",
        }
        .into(),
    ]
}

/// The argument vector of one timing child.
#[must_use]
pub fn timing_args(plan: &Plan, facade: Facade, lib: &Path, user_dir: &Path) -> Vec<OsString> {
    let mut args = base_args("child", plan, facade, lib, user_dir);
    args.push("--kind".into());
    args.push(
        match plan.kind {
            SampleKind::Timing => "timing",
            SampleKind::Memory => "memory",
        }
        .into(),
    );
    args.push("--stop-after".into());
    args.push(StopPoint::End.name().into());
    args
}

/// The command for one child, with `taskset` applied.
///
/// # Errors
///
/// [`BenchError::Unsupported`] if a CPU is requested on a host without
/// `taskset`: a requested pin is never silently dropped.
pub fn child_command(plan: &Plan, args: &[OsString]) -> Result<Command, BenchError> {
    let mut command = if let Some(cpu) = plan.cpu {
        if !cfg!(target_os = "linux") {
            return Err(BenchError::Unsupported(
                "CPU pinning is only wired up through taskset on Linux".to_owned(),
            ));
        }
        let mut c = Command::new("taskset");
        c.arg("-c").arg(cpu.to_string()).arg(&plan.probe);
        c
    } else {
        Command::new(&plan.probe)
    };
    command.args(args);
    Ok(command)
}

/// Linux one-minute load average.
#[must_use]
pub fn loadavg_1m() -> Option<f64> {
    fs::read_to_string("/proc/loadavg")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Drops the page cache, or fails.
///
/// # Errors
///
/// [`BenchError::Unsupported`] where there is no facility or it is not
/// permitted: a run that asked for a cold cache and could not get one stops.
pub fn drop_file_caches() -> Result<(), BenchError> {
    if cfg!(target_os = "linux") {
        let sync = Command::new("sync")
            .status()
            .map_err(|e| BenchError::io("running sync", e))?;
        if !sync.success() {
            return Err(BenchError::Unsupported("sync failed".to_owned()));
        }
        fs::write("/proc/sys/vm/drop_caches", "3").map_err(|e| {
            BenchError::Unsupported(format!(
                "cannot write /proc/sys/vm/drop_caches ({e}); run privileged or measure warm"
            ))
        })
    } else if cfg!(target_os = "macos") {
        let status = Command::new("purge")
            .status()
            .map_err(|e| BenchError::Unsupported(format!("cannot run purge: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(BenchError::Unsupported(
                "purge failed (it needs administrator rights)".to_owned(),
            ))
        }
    } else {
        Err(BenchError::Unsupported(
            "no way to drop the file cache on this operating system".to_owned(),
        ))
    }
}

fn tree_identity(dir: &Path) -> Result<TreeIdentity, BenchError> {
    let manifest = Manifest::of_directory(dir)?;
    Ok(TreeIdentity {
        name: dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_owned(),
        identity: manifest.identity(),
        files: manifest.files.len(),
        bytes: manifest.total_bytes(),
    })
}

fn file_identity(path: &Path) -> Result<FileIdentity, BenchError> {
    Ok(FileIdentity {
        file_name: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_owned(),
        size: fs::metadata(path)
            .map_err(|e| BenchError::io(format!("inspecting {}", path.display()), e))?
            .len(),
        sha256: sha256_file(path)
            .map_err(|e| BenchError::io(format!("hashing {}", path.display()), e))?,
    })
}

fn witness_args(fixture: &Fixture) -> Vec<OsString> {
    if fixture.witness.is_empty() {
        Vec::new()
    } else {
        vec![
            "--witness-file".into(),
            fixture.root.join(WITNESS_FILE).into_os_string(),
        ]
    }
}

/// One verification of one subject.
fn verify_once(
    plan: &Plan,
    fixture: &Fixture,
    facade: Facade,
    lib: &Path,
    user_dir: &Path,
) -> Result<Observation, BenchError> {
    fixture.restore_to(user_dir)?;
    let mut args = base_args("verify-child", plan, facade, lib, user_dir);
    args.push("--profile".into());
    args.push(fixture.name.clone().into());
    args.extend(witness_args(fixture));
    let command = child_command(plan, &args)?;
    let out = run_process(command, plan.timeout)?;
    let mut observation: Observation = json_line(&out.stdout)?;
    observation.stderr = out.stderr;
    fixture.verify()?;
    Ok(observation)
}

/// One run of the save-and-reopen scenario on a fresh empty directory.
fn persist_once(
    plan: &Plan,
    facade: Facade,
    lib: &Path,
    user_dir: &Path,
) -> Result<Observation, BenchError> {
    if user_dir.exists() {
        fs::remove_dir_all(user_dir)
            .map_err(|e| BenchError::io("clearing the persistence directory", e))?;
    }
    fs::create_dir_all(user_dir)
        .map_err(|e| BenchError::io("creating the persistence directory", e))?;
    let args = base_args("persist-child", plan, facade, lib, user_dir);
    let out = run_process(child_command(plan, &args)?, plan.timeout)?;
    let mut observation: Observation = json_line(&out.stdout)?;
    observation.stderr = out.stderr;
    Ok(observation)
}

fn verdict_record(
    facade: Facade,
    profile: &str,
    subject: &str,
    reference: &str,
    verdict: &Verdict,
    reference_obs: &Observation,
    stable: &std::collections::BTreeSet<String>,
) -> VerdictRecord {
    let (comparable, reasons) = match verdict {
        Verdict::Comparable => (true, Vec::new()),
        Verdict::NotComparable(why) => (false, why.clone()),
    };
    VerdictRecord {
        facade,
        profile: profile.to_owned(),
        subject: subject.to_owned(),
        reference: reference.to_owned(),
        phase: "observation".to_owned(),
        comparable,
        reasons,
        files_compared_by_bytes: stable.iter().cloned().collect(),
        files_judged_by_content: reference_obs
            .files
            .iter()
            .map(|f| f.name.clone())
            .filter(|n| !stable.contains(n))
            .collect(),
        transcript_lines: reference_obs.lines.len(),
    }
}

fn hostile_probe(
    plan: &Plan,
    fixture: &Fixture,
    facade: Facade,
    lib: &Path,
    user_dir: &Path,
    input: &str,
) -> Result<ProbeOutcome, BenchError> {
    fixture.restore_to(user_dir)?;
    let mut args = base_args("probe-child", plan, facade, lib, user_dir);
    args.push("--input".into());
    args.push(input.into());
    let command = child_command(plan, &args)?;
    let out = run_raw(command, plan.timeout)?;
    let exit = exit_class(&out.status);
    let lines = if exit == ExitClass::Returned {
        json_line::<ProbeLines>(&out.stdout)?.lines
    } else {
        Vec::new()
    };
    Ok(ProbeOutcome {
        input: input.to_owned(),
        exit,
        lines,
        stderr_sha256: text_digest(&out.stderr),
    })
}

/// Runs the plan, handing every record to `sink` as it is produced.
///
/// # Errors
///
/// A child that fails or hangs, an unreadable fixture, a fixture changed during
/// the run, a cold-cache request the host cannot honour. A behavioural
/// difference is not an error; it is a [`VerdictRecord`].
pub fn run(plan: &Plan, sink: &mut Sink<'_>) -> Result<Summary, BenchError> {
    validate_plan(plan)?;
    if plan.cache == CacheState::ColdDropCaches {
        drop_file_caches()?;
    }
    let fixtures = plan
        .profiles
        .iter()
        .map(|root| Fixture::open(root))
        .collect::<Result<Vec<_>, _>>()?;
    emit_manifest(plan, &fixtures, sink)?;

    let verify_path = plan
        .work_dir
        .join(format!("verify-user-{}", std::process::id()));
    fs::create_dir_all(&plan.work_dir).map_err(|e| BenchError::io("creating the work dir", e))?;
    let mut summary = Summary::default();
    let reference = &plan.subjects[0];
    for &facade in &plan.facades {
        for (profile_index, fixture) in fixtures.iter().enumerate() {
            let cell = Cell {
                plan,
                facade,
                fixture,
                verify_path: &verify_path,
            };
            let observations = verify_cell(&cell, &mut summary, sink)?;
            if profile_index == 0 {
                hostile_cell(&cell, sink)?;
                persist_cell(&cell, &mut summary, sink)?;
            }
            let _ = fs::remove_dir_all(&verify_path);
            let mismatches = timed_cell(&cell, &observations, &mut summary, sink)?;
            fixture.verify()?;
            for (label, reasons) in mismatches {
                summary.not_comparable += 1;
                sink(&Record::Verdict(Box::new(VerdictRecord {
                    facade,
                    profile: fixture.name.clone(),
                    subject: label,
                    reference: reference.label.clone(),
                    phase: "timed-samples".to_owned(),
                    comparable: false,
                    reasons: reasons.into_iter().take(5).collect(),
                    files_compared_by_bytes: Vec::new(),
                    files_judged_by_content: Vec::new(),
                    transcript_lines: 0,
                })))?;
            }
        }
    }
    Ok(summary)
}

/// The record consumer [`run`] hands every record to.
pub type Sink<'a> = dyn FnMut(&Record) -> Result<(), BenchError> + 'a;

/// One (facade, profile) cell of a run.
struct Cell<'a> {
    plan: &'a Plan,
    facade: Facade,
    fixture: &'a Fixture,
    verify_path: &'a Path,
}

impl Cell<'_> {
    fn library<'s>(&self, subject: &'s SubjectSpec) -> Result<&'s Path, BenchError> {
        subject
            .path(self.facade)
            .ok_or_else(|| BenchError::invalid("no library"))
    }
}

fn validate_plan(plan: &Plan) -> Result<(), BenchError> {
    if plan.subjects.is_empty() || plan.profiles.is_empty() || plan.facades.is_empty() {
        return Err(BenchError::invalid(
            "a run needs at least one subject, profile and facade",
        ));
    }
    for (i, s) in plan.subjects.iter().enumerate() {
        if s.label.is_empty() || plan.subjects[..i].iter().any(|o| o.label == s.label) {
            return Err(BenchError::invalid(format!(
                "subject label {:?} is empty or repeated",
                s.label
            )));
        }
    }
    for (i, p) in plan.profiles.iter().enumerate() {
        let name = p.file_name();
        if name.is_none() || plan.profiles[..i].iter().any(|o| o.file_name() == name) {
            return Err(BenchError::invalid(format!(
                "profile {} has no name or repeats another profile's name",
                p.display()
            )));
        }
    }
    for facade in &plan.facades {
        for s in &plan.subjects {
            if s.path(*facade).is_none() {
                return Err(BenchError::invalid(format!(
                    "subject {} has no {} library",
                    s.label,
                    facade.prefix()
                )));
            }
        }
    }
    Ok(())
}

fn emit_manifest(plan: &Plan, fixtures: &[Fixture], sink: &mut Sink<'_>) -> Result<(), BenchError> {
    let mut identities = Vec::new();
    for s in &plan.subjects {
        identities.push(SubjectIdentity {
            label: s.label.clone(),
            pinyin: s.pinyin.as_deref().map(file_identity).transpose()?,
            zhuyin: s.zhuyin.as_deref().map(file_identity).transpose()?,
        });
    }
    sink(&Record::Manifest(Box::new(RunManifest {
        schema: RUN_SCHEMA.to_owned(),
        host: HostInfo::current(),
        probe_sha256: sha256_file(&plan.probe)
            .map_err(|e| BenchError::io("hashing the benchmark binary", e))?,
        workload_digest: workload_digest(),
        subjects: identities,
        system_data: tree_identity(&plan.system_dir)?,
        profiles: fixtures
            .iter()
            .map(|f| TreeIdentity {
                name: f.name.clone(),
                identity: f.manifest.identity(),
                files: f.manifest.files.len(),
                bytes: f.manifest.total_bytes(),
            })
            .collect(),
        facades: plan.facades.clone(),
        kind: plan.kind,
        rounds: plan.rounds,
        warmup: plan.warmup,
        cache: plan.cache,
        cpu: plan.cpu,
        bind: plan.bind,
    })))
}

/// The verification phase: every subject against the reference, with the
/// reference run twice to find the files that are not byte-stable.
fn verify_cell(
    cell: &Cell<'_>,
    summary: &mut Summary,
    sink: &mut Sink<'_>,
) -> Result<BTreeMap<String, Observation>, BenchError> {
    let Cell {
        plan,
        facade,
        fixture,
        verify_path,
    } = *cell;
    let reference = &plan.subjects[0];
    let ref_lib = cell.library(reference)?;
    let ref_obs = verify_once(plan, fixture, facade, ref_lib, verify_path)?;
    let ref_again = verify_once(plan, fixture, facade, ref_lib, verify_path)?;
    let stable = stable_files(&ref_obs, &ref_again);
    // The control must itself agree on everything but unstable files.
    if let Verdict::NotComparable(why) = compare(&ref_obs, &ref_again, &stable) {
        return Err(BenchError::invalid(format!(
            "the reference is not deterministic on {} / {}: {}",
            facade.prefix(),
            fixture.name,
            why.join("; ")
        )));
    }
    let mut observations = BTreeMap::new();
    observations.insert(reference.label.clone(), ref_obs.clone());
    for subject in &plan.subjects[1..] {
        let lib = cell.library(subject)?;
        let obs = verify_once(plan, fixture, facade, lib, verify_path)?;
        let verdict = compare(&ref_obs, &obs, &stable);
        if !verdict.is_comparable() {
            summary.not_comparable += 1;
        }
        sink(&Record::Verdict(Box::new(verdict_record(
            facade,
            &fixture.name,
            &subject.label,
            &reference.label,
            &verdict,
            &ref_obs,
            &stable,
        ))))?;
        observations.insert(subject.label.clone(), obs);
    }
    Ok(observations)
}

/// Hostile inputs, one process each; their outcomes are compared, never timed.
fn hostile_cell(cell: &Cell<'_>, sink: &mut Sink<'_>) -> Result<(), BenchError> {
    let Cell {
        plan,
        facade,
        fixture,
        verify_path,
    } = *cell;
    let reference = &plan.subjects[0];
    let ref_lib = cell.library(reference)?;
    for input in HOSTILE_INPUTS {
        let ref_probe = hostile_probe(plan, fixture, facade, ref_lib, verify_path, input)?;
        sink(&Record::Probe(Box::new(ProbeRecord {
            facade,
            subject: reference.label.clone(),
            outcome: ref_probe.clone(),
            against_reference: "reference".to_owned(),
        })))?;
        for subject in &plan.subjects[1..] {
            let lib = cell.library(subject)?;
            let outcome = hostile_probe(plan, fixture, facade, lib, verify_path, input)?;
            let against = match compare_probe(&ref_probe, &outcome) {
                ProbeVerdict::SameOutcome => "same outcome".to_owned(),
                ProbeVerdict::Different(why) => format!("DIFFERENT: {why}"),
            };
            sink(&Record::Probe(Box::new(ProbeRecord {
                facade,
                subject: subject.label.clone(),
                outcome,
                against_reference: against,
            })))?;
        }
    }
    Ok(())
}

/// Save and reopen on a fresh empty directory.
fn persist_cell(
    cell: &Cell<'_>,
    summary: &mut Summary,
    sink: &mut Sink<'_>,
) -> Result<(), BenchError> {
    let Cell {
        plan,
        facade,
        verify_path,
        ..
    } = *cell;
    let reference = &plan.subjects[0];
    let ref_lib = cell.library(reference)?;
    let ref_persist = persist_once(plan, facade, ref_lib, verify_path)?;
    let ref_persist_again = persist_once(plan, facade, ref_lib, verify_path)?;
    let stable = stable_files(&ref_persist, &ref_persist_again);
    if let Verdict::NotComparable(why) = compare(&ref_persist, &ref_persist_again, &stable) {
        return Err(BenchError::invalid(format!(
            "the reference is not deterministic in the save/reopen scenario ({}): {}",
            facade.prefix(),
            why.join("; ")
        )));
    }
    for subject in &plan.subjects[1..] {
        let lib = cell.library(subject)?;
        let obs = persist_once(plan, facade, lib, verify_path)?;
        let verdict = compare(&ref_persist, &obs, &stable);
        if !verdict.is_comparable() {
            summary.not_comparable += 1;
        }
        sink(&Record::Verdict(Box::new(verdict_record(
            facade,
            "persist:save-reopen",
            &subject.label,
            &reference.label,
            &verdict,
            &ref_persist,
            &stable,
        ))))?;
    }
    Ok(())
}

/// The timed rounds. Returns, per subject, the samples whose trace contradicts
/// the verified transcript.
fn timed_cell(
    cell: &Cell<'_>,
    observations: &BTreeMap<String, Observation>,
    summary: &mut Summary,
    sink: &mut Sink<'_>,
) -> Result<BTreeMap<String, Vec<String>>, BenchError> {
    let Cell {
        plan,
        facade,
        fixture,
        ..
    } = *cell;
    let n = plan.subjects.len();
    let mut mismatches: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if plan.rounds == 0 {
        return Ok(mismatches);
    }
    for round in 0..plan.warmup + plan.rounds {
        let warmup = round < plan.warmup;
        for step in 0..n {
            let subject = &plan.subjects[(round + step) % n];
            let lib = cell.library(subject)?;
            let copy = fixture.working_copy(&plan.work_dir)?;
            let command = child_command(plan, &timing_args(plan, facade, lib, copy.path()))?;
            if plan.cache == CacheState::ColdDropCaches {
                drop_file_caches()?;
            }
            let load = loadavg_1m();
            let output = run_process(command, plan.timeout)?;
            let sample = parse_sample(&output.stdout)?;
            if let Some(reason) = contamination(&sample, plan.max_skew, load, plan.max_load) {
                summary.rejected += 1;
                sink(&Record::Rejected(RejectedRecord {
                    round,
                    facade,
                    subject: subject.label.clone(),
                    profile: fixture.name.clone(),
                    reason,
                }))?;
                continue;
            }
            let verified = observations
                .get(&subject.label)
                .map(|o| o.unit_trace_digest.as_str());
            if let Some(why) = trace_mismatch(&sample, verified, round) {
                mismatches
                    .entry(subject.label.clone())
                    .or_default()
                    .push(why);
            }
            if !warmup {
                summary.accepted += 1;
            }
            sink(&Record::Sample(Box::new(SampleRecord {
                round,
                warmup,
                facade,
                subject: subject.label.clone(),
                profile: fixture.name.clone(),
                process_wall_ns: output.wall_ns,
                loadavg_1m: load,
                sample,
            })))?;
        }
    }
    Ok(mismatches)
}

/// Why a timed sample does not reproduce the verified workload, if it does not.
fn trace_mismatch(sample: &Sample, verified: Option<&str>, round: usize) -> Option<String> {
    if sample.workload_digest != workload_digest() {
        Some(format!(
            "round {round}: the timed child ran workload {}, not the verified {}",
            sample.workload_digest,
            workload_digest()
        ))
    } else if verified != Some(sample.trace_digest.as_str()) {
        Some(format!(
            "round {round}: the timed units' trace digest differs from the verified one"
        ))
    } else {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::sample::{PhaseTimes, SAMPLE_SCHEMA, Timeline};

    fn sample(wall: i64, mono: u64) -> Sample {
        Sample {
            schema: SAMPLE_SCHEMA.to_owned(),
            kind: SampleKind::Timing,
            facade: Facade::Pinyin,
            bind: Bind::Now,
            stop_after: StopPoint::End,
            phases: PhaseTimes::default(),
            timeline: Timeline::default(),
            ttfc_ns: Some(1),
            workload_digest: workload_digest(),
            trace_digest: "t".into(),
            monotonic_total_ns: mono,
            wall_total_ns: wall,
            memory: Vec::new(),
        }
    }

    #[test]
    fn a_clock_step_or_suspend_rejects_the_sample() {
        let limit = Duration::from_millis(50);
        assert!(contamination(&sample(1_000_020_000, 1_000_000_000), limit, None, None).is_none());
        assert!(
            contamination(&sample(31_000_000_000, 1_000_000_000), limit, None, None)
                .unwrap()
                .contains("suspend")
        );
        assert!(contamination(&sample(-5, 1_000_000_000), limit, None, None).is_some());
    }

    #[test]
    fn load_is_only_checked_when_a_limit_is_set() {
        let s = sample(10, 10);
        let limit = Duration::from_secs(1);
        assert!(contamination(&s, limit, Some(9.0), None).is_none());
        assert!(contamination(&s, limit, Some(9.0), Some(1.0)).is_some());
        assert!(contamination(&s, limit, None, Some(1.0)).is_none());
        assert!(contamination(&s, limit, Some(0.5), Some(1.0)).is_none());
    }

    fn plan() -> Plan {
        Plan {
            probe: PathBuf::from("/probe"),
            subjects: vec![SubjectSpec {
                label: "pin".into(),
                pinyin: Some(PathBuf::from("/lib/pin.so")),
                zhuyin: None,
            }],
            facades: vec![Facade::Pinyin],
            system_dir: PathBuf::from("/data"),
            profiles: vec![PathBuf::from("/fx/empty")],
            rounds: 3,
            warmup: 1,
            kind: SampleKind::Timing,
            cache: CacheState::Warm,
            cpu: None,
            bind: Bind::Now,
            work_dir: PathBuf::from("/work"),
            max_skew: Duration::from_millis(50),
            max_load: None,
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn profiles_that_share_a_name_are_refused() {
        let mut p = plan();
        p.profiles = vec![PathBuf::from("/a/modest"), PathBuf::from("/b/modest")];
        assert!(validate_plan(&p).is_err());
        p.profiles = vec![PathBuf::from("/a/modest"), PathBuf::from("/b/empty")];
        assert!(validate_plan(&p).is_ok());
    }

    #[test]
    fn child_arguments_name_everything_the_child_needs() {
        let p = plan();
        let args = timing_args(
            &p,
            Facade::Pinyin,
            Path::new("/lib/pin.so"),
            Path::new("/work/u"),
        );
        let joined = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        for expected in [
            "child --facade pinyin --lib /lib/pin.so",
            "--system-dir /data --user-dir /work/u --bind now",
            "--kind timing --stop-after end",
        ] {
            assert!(joined.contains(expected), "{joined}");
        }
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn a_requested_cpu_pin_is_never_silently_dropped() {
        let mut p = plan();
        p.cpu = Some(0);
        assert!(child_command(&p, &[]).is_err());
    }

    #[test]
    fn child_output_must_be_a_sample_of_this_schema() {
        let s = sample(1, 1);
        let line = serde_json::to_string(&s).unwrap();
        assert_eq!(parse_sample(&format!("noise\n{line}\n")).unwrap(), s);
        assert!(parse_sample("").is_err());
        assert!(parse_sample("not json\n").is_err());
        let mut wrong = s;
        wrong.schema = "other".into();
        assert!(parse_sample(&serde_json::to_string(&wrong).unwrap()).is_err());
    }

    #[test]
    fn records_round_trip_through_json_with_their_tags() {
        let v = Record::Verdict(Box::new(VerdictRecord {
            facade: Facade::Zhuyin,
            profile: "empty".into(),
            subject: "ox".into(),
            reference: "pin".into(),
            phase: "observation".into(),
            comparable: false,
            reasons: vec!["line 3 differs".into()],
            files_compared_by_bytes: vec!["user.conf".into()],
            files_judged_by_content: vec![],
            transcript_lines: 9,
        }));
        let line = serde_json::to_string(&v).unwrap();
        assert!(line.contains("\"type\":\"verdict\""));
        assert_eq!(serde_json::from_str::<Record>(&line).unwrap(), v);
        let s = Record::Sample(Box::new(SampleRecord {
            round: 0,
            warmup: false,
            facade: Facade::Pinyin,
            subject: "pin".into(),
            profile: "empty".into(),
            process_wall_ns: 1,
            loadavg_1m: Some(0.5),
            sample: sample(1, 1),
        }));
        let line = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Record>(&line).unwrap(), s);
    }

    #[cfg(unix)]
    #[test]
    fn exit_classes_distinguish_status_from_signal() {
        let ok = run_raw(
            {
                let mut c = Command::new("sh");
                c.args(["-c", "exit 0"]);
                c
            },
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(exit_class(&ok.status), ExitClass::Returned);
        let code = run_raw(
            {
                let mut c = Command::new("sh");
                c.args(["-c", "exit 3"]);
                c
            },
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(exit_class(&code.status), ExitClass::Status(3));
        let sig = run_raw(
            {
                let mut c = Command::new("sh");
                c.args(["-c", "kill -ABRT $$"]);
                c
            },
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(exit_class(&sig.status), ExitClass::Signal(6));
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_child_is_an_error_with_its_stderr() {
        let mut c = Command::new("sh");
        c.args(["-c", "echo boom >&2; exit 3"]);
        match run_process(c, Duration::from_secs(5)).unwrap_err() {
            BenchError::Child { code, stderr, .. } => {
                assert_eq!(code, Some(3));
                assert!(stderr.contains("boom"));
            }
            other => panic!("{other}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_hangs_is_killed_and_reported() {
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 30"]);
        let started = Instant::now();
        let error = run_raw(c, Duration::from_millis(100)).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(error.to_string().contains("timed out"), "{error}");
    }

    #[test]
    fn run_refuses_an_empty_plan_duplicate_labels_and_a_missing_facade_library() {
        let mut sink = |_: &Record| Ok(());
        let mut empty = plan();
        empty.subjects.clear();
        assert!(run(&empty, &mut sink).is_err());
        let mut dup = plan();
        dup.subjects.push(dup.subjects[0].clone());
        assert!(
            run(&dup, &mut sink)
                .unwrap_err()
                .to_string()
                .contains("repeated")
        );
        let mut zh = plan();
        zh.facades = vec![Facade::Zhuyin];
        assert!(
            run(&zh, &mut sink)
                .unwrap_err()
                .to_string()
                .contains("no zhuyin library")
        );
    }
}
