//! From a stream of records to a report: equivalence first, then medians,
//! spread, uncertainty and ratios, each labelled with what it is.
//!
//! **A ratio is computed only between subjects whose equivalence verdict is
//! comparable.** For a subject that is NOT COMPARABLE the report prints its
//! absolute figures and the reasons and never a ratio: a behavioural difference
//! is not a performance win or loss. Warm-up rounds are excluded. A memory
//! sample's timings and a timing sample's mid-run memory are never used.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::error::BenchError;
use crate::observe::ExitClass;
use crate::orchestrate::{Record, RunManifest, SampleRecord};
use crate::sample::{SampleKind, StopPoint};
use crate::stats::{self, Summary};
use crate::workload::Facade;

/// A ratio to report: `numerator / denominator`, both subject labels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatioSpec {
    /// Numerator subject.
    pub numerator: String,
    /// Denominator subject.
    pub denominator: String,
}

/// What to report.
#[derive(Clone, Debug)]
pub struct ReportSpec {
    /// The reference subject (the run's first).
    pub reference: String,
    /// Ratios; if empty, every other subject over the reference.
    pub ratios: Vec<RatioSpec>,
}

type Key = (Facade, String, String);
type Cells = BTreeMap<Key, BTreeMap<&'static str, Vec<f64>>>;

const TIMING_ORDER: [&str; 10] = [
    "lib_load",
    "init",
    "config",
    "alloc",
    "first_candidate_phase",
    "ttfc",
    "setup_plus_first_candidate",
    "drop_instance",
    "fini",
    "process_wall",
];

fn ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}

fn timing_metrics(record: &SampleRecord) -> Vec<(&'static str, f64)> {
    let p = &record.sample.phases;
    let mut m = vec![
        ("lib_load", ms(p.lib_load_ns)),
        ("init", ms(p.init_ns)),
        ("config", ms(p.config_ns)),
        ("alloc", ms(p.alloc_ns)),
        ("process_wall", ms(record.process_wall_ns)),
    ];
    if let Some(f) = &p.first_candidate {
        m.push(("first_candidate_phase", ms(f.total_ns)));
    }
    if let Some(t) = record.sample.ttfc_ns {
        m.push(("ttfc", ms(t)));
    }
    if let Some(s) = record.sample.setup_plus_first_candidate_ns() {
        m.push(("setup_plus_first_candidate", ms(s)));
    }
    if let Some(n) = p.drop_instance_ns {
        m.push(("drop_instance", ms(n)));
    }
    if let Some(n) = p.fini_ns {
        m.push(("fini", ms(n)));
    }
    m
}

fn label_of(metric: &str) -> &str {
    match metric {
        "lib_load" => "library load + symbol resolution",
        "init" => "`*_init`",
        "config" => "`*_set_options`",
        "alloc" => "`*_alloc_instance`",
        "first_candidate_phase" => "first usable result (parse … candidate string)",
        "ttfc" => "**time to first candidate** (timestamps)",
        "setup_plus_first_candidate" => "load+init+options+alloc+first-result (phase sum)",
        "drop_instance" => "free instance",
        "fini" => "`*_fini`",
        "process_wall" => "whole process, spawn to exit",
        other => other,
    }
}

fn mib(kib: u64) -> f64 {
    kib as f64 / 1024.0
}

fn memory_metrics(record: &SampleRecord) -> Vec<(String, f64)> {
    let Some(base) = record.sample.memory.iter().find(|p| p.point == "baseline") else {
        return Vec::new();
    };
    let Some(base_rss) = base.snapshot.rss_kib else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for point in record
        .sample
        .memory
        .iter()
        .filter(|p| p.point != "baseline")
    {
        let (s, name) = (&point.snapshot, &point.point);
        if let Some(v) = s.rss_kib {
            out.push((format!("{name}/rss_delta_mib"), mib(v) - mib(base_rss)));
        }
        for (suffix, value) in [
            ("peak_rss_mib", s.peak_rss_kib),
            ("anon_mib", s.anon_kib),
            ("file_mib", s.file_kib),
            ("private_dirty_mib", s.private_dirty_kib),
            ("pss_mib", s.pss_kib),
            ("virtual_size_mib", s.vm_size_kib),
        ] {
            if let Some(v) = value {
                out.push((format!("{name}/{suffix}"), mib(v)));
            }
        }
        for (suffix, now, before) in [
            (
                "minor_faults_count",
                s.minor_faults,
                base.snapshot.minor_faults,
            ),
            (
                "major_faults_count",
                s.major_faults,
                base.snapshot.major_faults,
            ),
        ] {
            if let (Some(now), Some(before)) = (now, before) {
                out.push((
                    format!("{name}/{suffix}"),
                    now.saturating_sub(before) as f64,
                ));
            }
        }
    }
    out
}

fn fmt_summary(values: &[f64], precision: usize) -> String {
    let Some(s) = stats::summarize(values) else {
        return "—".to_owned();
    };
    let ci = stats::median_interval(values).map_or_else(
        || "n/a".to_owned(),
        |i| format!("{:.precision$}–{:.precision$}", i.low, i.high),
    );
    let spread = Summary::range_over_median(&s)
        .map_or_else(|| "n/a".to_owned(), |r| format!("{:.0}%", r * 100.0));
    format!("{:.precision$} [{ci}] n={} r/m={spread}", s.median, s.n)
}

fn fmt_ratio(num: &[f64], den: &[f64]) -> String {
    let (Some(n), Some(d)) = (stats::median(num), stats::median(den)) else {
        return "—".to_owned();
    };
    if d == 0.0 {
        return "n/a".to_owned();
    }
    let ci = stats::ratio_interval(num, den).map_or_else(
        || "n/a".to_owned(),
        |i| format!("{:.2}–{:.2}", i.low, i.high),
    );
    format!("{:.2}× [{ci}]", n / d)
}

/// Whether `subject` may be compared on this cell: every verdict recorded for it
/// is comparable (the reference is comparable with itself by definition). A
/// failed save-and-reopen verdict withdraws every profile that has saved data
/// (all but the `empty` fixture), because restoring saved data is what those
/// profiles measure.
fn comparable(
    records: &[Record],
    facade: Facade,
    profile: &str,
    subject: &str,
    reference: &str,
) -> bool {
    if subject == reference {
        return true;
    }
    // No recorded observation verdict is no proof of equivalence (a filtered or
    // truncated stream, or a run that stopped before its verdicts).
    let observed = records.iter().any(|r| {
        matches!(r, Record::Verdict(v)
            if v.facade == facade
                && v.subject == subject
                && v.profile == profile
                && v.phase == "observation")
    });
    observed
        && records.iter().all(|r| match r {
            Record::Verdict(v)
                if v.facade == facade
                    && v.subject == subject
                    && (v.profile == profile
                        || (v.profile.starts_with("persist:") && profile != "empty")) =>
            {
                v.comparable
            }
            _ => true,
        })
}

/// The report text.
///
/// # Errors
///
/// No manifest, a reference that is not a subject of the run, or a requested
/// ratio naming an unknown subject.
pub fn render(records: &[Record], spec: &ReportSpec) -> Result<String, BenchError> {
    let manifest = records
        .iter()
        .find_map(|r| {
            if let Record::Manifest(m) = r {
                Some(m.as_ref())
            } else {
                None
            }
        })
        .ok_or_else(|| BenchError::invalid("the result stream has no run manifest"))?;
    let known: Vec<&str> = manifest.subjects.iter().map(|s| s.label.as_str()).collect();
    for label in std::iter::once(&spec.reference).chain(
        spec.ratios
            .iter()
            .flat_map(|r| [&r.numerator, &r.denominator]),
    ) {
        if !known.contains(&label.as_str()) {
            return Err(BenchError::invalid(format!(
                "subject {label} is not in this run (subjects: {})",
                known.join(", ")
            )));
        }
    }
    let ratios: Vec<RatioSpec> = if spec.ratios.is_empty() {
        known
            .iter()
            .filter(|l| **l != spec.reference)
            .map(|l| RatioSpec {
                numerator: (*l).to_owned(),
                denominator: spec.reference.clone(),
            })
            .collect()
    } else {
        spec.ratios.clone()
    };

    let mut timing: Cells = BTreeMap::new();
    let mut memory: BTreeMap<Key, BTreeMap<String, Vec<f64>>> = BTreeMap::new();
    let mut rejected = 0_usize;
    for record in records {
        match record {
            Record::Rejected(_) => rejected += 1,
            Record::Sample(s) if !s.warmup && s.sample.stop_after == StopPoint::End => {
                let key = (s.facade, s.profile.clone(), s.subject.clone());
                match s.sample.kind {
                    SampleKind::Timing => {
                        let cell = timing.entry(key).or_default();
                        for (name, value) in timing_metrics(s) {
                            cell.entry(name).or_default().push(value);
                        }
                    }
                    SampleKind::Memory => {
                        let cell = memory.entry(key).or_default();
                        for (name, value) in memory_metrics(s) {
                            cell.entry(name).or_default().push(value);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = String::new();
    header(&mut out, manifest, rejected);
    equivalence_section(&mut out, records, manifest, &spec.reference);
    if !timing.is_empty() {
        timing_section(&mut out, records, manifest, spec, &ratios, &timing);
    }
    if !memory.is_empty() {
        memory_section(&mut out, records, manifest, spec, &ratios, &memory);
    }
    unmeasured_section(&mut out, manifest, &timing, &memory);
    definitions(&mut out);
    Ok(out)
}

fn header(out: &mut String, m: &RunManifest, rejected: usize) {
    let _ = writeln!(out, "# Initialisation and startup benchmark\n");
    let _ = writeln!(
        out,
        "Host: {} {} ({} CPUs{}). Kind: {:?}. Cache state: {:?}. Loader binding: {:?}. CPU pin: {}. \
         Rounds: {} measured + {} warm-up.",
        m.host.os,
        m.host.arch,
        m.host.cpus,
        m.host
            .kernel
            .as_ref()
            .map_or(String::new(), |k| format!(", kernel {k}")),
        m.kind,
        m.cache,
        m.bind,
        m.cpu.map_or("none".to_owned(), |c| c.to_string()),
        m.rounds,
        m.warmup,
    );
    let _ = writeln!(
        out,
        "\nBenchmark binary SHA-256 `{}`; workload definition SHA-256 `{}`.\n",
        m.probe_sha256, m.workload_digest
    );
    let _ = writeln!(
        out,
        "| subject | facade | file | bytes | SHA-256 |\n| --- | --- | --- | ---: | --- |"
    );
    for s in &m.subjects {
        for (name, id) in [("pinyin", &s.pinyin), ("zhuyin", &s.zhuyin)] {
            if let Some(f) = id {
                let _ = writeln!(
                    out,
                    "| {} | {name} | {} | {} | `{}` |",
                    s.label, f.file_name, f.size, f.sha256
                );
            }
        }
    }
    let _ = writeln!(
        out,
        "\nSystem data `{}`: {} files, {} bytes, identity `{}`.",
        m.system_data.name, m.system_data.files, m.system_data.bytes, m.system_data.identity
    );
    for p in &m.profiles {
        let _ = writeln!(
            out,
            "Profile `{}`: {} files, {} bytes, identity `{}`.",
            p.name, p.files, p.bytes, p.identity
        );
    }
    let _ = writeln!(out, "\nRejected samples (contamination): {rejected}.\n");
}

fn equivalence_section(out: &mut String, records: &[Record], m: &RunManifest, reference: &str) {
    let _ = writeln!(
        out,
        "## Behavioural equivalence (verified before any timing is used)\n"
    );
    let _ = writeln!(
        out,
        "Each subject was compared with the reference (`{reference}`) on the same profile at the same \
         path: every return value, parsed length, sentence, and every candidate's type, text and n-best \
         index in order, the stderr diagnostics, and the files left in the user directory. **A cell that \
         is NOT COMPARABLE has no ratio below.**\n"
    );
    let mut any = false;
    let _ = writeln!(
        out,
        "| facade | profile | subject | verdict | transcript lines | files by bytes / by content |\n| --- | --- | --- | --- | ---: | --- |"
    );
    type VerdictRow = (bool, Vec<String>, usize, usize, usize);
    let mut verdicts: BTreeMap<(Facade, String, String), VerdictRow> = BTreeMap::new();
    for r in records {
        if let Record::Verdict(v) = r {
            any = true;
            let e = verdicts
                .entry((v.facade, v.profile.clone(), v.subject.clone()))
                .or_insert((true, Vec::new(), 0, 0, 0));
            e.0 &= v.comparable;
            for why in &v.reasons {
                e.1.push(format!("{}: {why}", v.phase));
            }
            if v.phase == "observation" {
                e.2 = v.transcript_lines;
                e.3 = v.files_compared_by_bytes.len();
                e.4 = v.files_judged_by_content.len();
            }
        }
    }
    for ((facade, profile, subject), (ok, _, lines, by_bytes, by_content)) in &verdicts {
        let _ = writeln!(
            out,
            "| {} | {profile} | {subject} | {} | {lines} | {by_bytes} / {by_content} |",
            facade.prefix(),
            if *ok {
                "comparable"
            } else {
                "**NOT COMPARABLE**"
            }
        );
    }
    if !any {
        let _ = writeln!(
            out,
            "| — | — | — | no subject other than the reference | — | — |"
        );
    }
    for ((facade, profile, subject), (ok, reasons, ..)) in &verdicts {
        if !ok {
            let _ = writeln!(
                out,
                "\n**{} / {profile} / {subject} is NOT COMPARABLE:**",
                facade.prefix()
            );
            for why in reasons.iter().take(8) {
                let _ = writeln!(out, "- {why}");
            }
        }
    }
    let probes: Vec<_> = records
        .iter()
        .filter_map(|r| {
            if let Record::Probe(p) = r {
                Some(p.as_ref())
            } else {
                None
            }
        })
        .collect();
    if !probes.is_empty() {
        let _ = writeln!(
            out,
            "\n### Hostile inputs (one process each; outcomes compared separately, never timed)\n"
        );
        let _ = writeln!(
            out,
            "| facade | input | subject | outcome | against reference |\n| --- | --- | --- | --- | --- |"
        );
        for p in probes {
            let outcome = match &p.outcome.exit {
                ExitClass::Returned => format!("returned ({} lines)", p.outcome.lines.len()),
                ExitClass::Status(c) => format!("exit status {c}"),
                ExitClass::Signal(s) => format!("**killed by signal {s}**"),
            };
            let _ = writeln!(
                out,
                "| {} | `{}` | {} | {outcome} | {} |",
                p.facade.prefix(),
                p.outcome.input.escape_debug(),
                p.subject,
                p.against_reference
            );
        }
    }
    let _ = m;
    let _ = writeln!(out);
}

fn cell_header(
    out: &mut String,
    m: &RunManifest,
    spec: &ReportSpec,
    ratios: &[RatioSpec],
    first: &str,
) {
    let mut head = format!("| {first} |");
    let mut rule = "| --- |".to_owned();
    for s in &m.subjects {
        let tag = if s.label == spec.reference {
            " (reference)"
        } else {
            ""
        };
        let _ = write!(head, " {}{tag} |", s.label);
        rule.push_str(" --- |");
    }
    for r in ratios {
        let _ = write!(head, " {}/{} |", r.numerator, r.denominator);
        rule.push_str(" --- |");
    }
    let _ = writeln!(out, "{head}\n{rule}");
}

fn ratio_cell(
    records: &[Record],
    facade: Facade,
    profile: &str,
    r: &RatioSpec,
    reference: &str,
    get: &dyn Fn(&str) -> Option<Vec<f64>>,
    precision_note: bool,
) -> String {
    let _ = precision_note;
    let ok = comparable(records, facade, profile, &r.numerator, reference)
        && comparable(records, facade, profile, &r.denominator, reference);
    if !ok {
        return "**NOT COMPARABLE**".to_owned();
    }
    match (get(&r.numerator), get(&r.denominator)) {
        (Some(n), Some(d)) => fmt_ratio(&n, &d),
        _ => "—".to_owned(),
    }
}

fn timing_section(
    out: &mut String,
    records: &[Record],
    m: &RunManifest,
    spec: &ReportSpec,
    ratios: &[RatioSpec],
    cells: &Cells,
) {
    let _ = writeln!(out, "## Timing (milliseconds)\n");
    let _ = writeln!(
        out,
        "Cells are `median [95% bootstrap CI] n=samples r/m=(max−min)/median`. Ratios are median/median \
         with a bootstrap CI, **only between comparable subjects**.\n"
    );
    for &facade in &m.facades {
        for profile in &m.profiles {
            let _ = writeln!(
                out,
                "### {} / profile `{}`\n",
                facade.prefix(),
                profile.name
            );
            cell_header(out, m, spec, ratios, "phase");
            for metric in TIMING_ORDER {
                let key = |label: &str| (facade, profile.name.clone(), label.to_owned());
                if !m.subjects.iter().any(|s| {
                    cells
                        .get(&key(&s.label))
                        .is_some_and(|c| c.contains_key(metric))
                }) {
                    continue;
                }
                let mut row = format!("| {} |", label_of(metric));
                for s in &m.subjects {
                    let v = cells.get(&key(&s.label)).and_then(|c| c.get(metric));
                    let _ = write!(
                        row,
                        " {} |",
                        v.map_or("—".to_owned(), |v| fmt_summary(v, 3))
                    );
                }
                for r in ratios {
                    let get =
                        |label: &str| cells.get(&key(label)).and_then(|c| c.get(metric)).cloned();
                    let _ = write!(
                        row,
                        " {} |",
                        ratio_cell(
                            records,
                            facade,
                            &profile.name,
                            r,
                            &spec.reference,
                            &get,
                            false
                        )
                    );
                }
                let _ = writeln!(out, "{row}");
            }
            let _ = writeln!(out);
        }
    }
}

fn memory_section(
    out: &mut String,
    records: &[Record],
    m: &RunManifest,
    spec: &ReportSpec,
    ratios: &[RatioSpec],
    cells: &BTreeMap<Key, BTreeMap<String, Vec<f64>>>,
) {
    let _ = writeln!(out, "## Memory (MiB, from memory-mode samples)\n");
    let _ = writeln!(
        out,
        "`rss_delta` is the resident set at the point minus the baseline read before the library was \
         opened. `peak_rss` is the kernel's resident high-water mark up to that point. `virtual_size` is \
         address space, **not** RAM. `*_faults_count` is page faults since the baseline read (minor: no \
         I/O; major: I/O).\n"
    );
    for &facade in &m.facades {
        for profile in &m.profiles {
            let _ = writeln!(
                out,
                "### {} / profile `{}`\n",
                facade.prefix(),
                profile.name
            );
            cell_header(out, m, spec, ratios, "point · quantity");
            for point in [
                "lib_load",
                "init",
                "options",
                "alloc",
                "first_candidate",
                "end",
            ] {
                for q in [
                    "rss_delta_mib",
                    "peak_rss_mib",
                    "anon_mib",
                    "file_mib",
                    "private_dirty_mib",
                    "pss_mib",
                    "virtual_size_mib",
                    "minor_faults_count",
                    "major_faults_count",
                ] {
                    let name = format!("{point}/{q}");
                    let key = |label: &str| (facade, profile.name.clone(), label.to_owned());
                    if !m.subjects.iter().any(|s| {
                        cells
                            .get(&key(&s.label))
                            .is_some_and(|c| c.contains_key(&name))
                    }) {
                        continue;
                    }
                    let precision = if q.ends_with("_count") { 0 } else { 2 };
                    let mut row = format!("| {point} · {q} |");
                    for s in &m.subjects {
                        let v = cells.get(&key(&s.label)).and_then(|c| c.get(&name));
                        let _ = write!(
                            row,
                            " {} |",
                            v.map_or("—".to_owned(), |v| fmt_summary(v, precision))
                        );
                    }
                    for r in ratios {
                        let get = |label: &str| {
                            cells.get(&key(label)).and_then(|c| c.get(&name)).cloned()
                        };
                        let _ = write!(
                            row,
                            " {} |",
                            ratio_cell(
                                records,
                                facade,
                                &profile.name,
                                r,
                                &spec.reference,
                                &get,
                                false
                            )
                        );
                    }
                    let _ = writeln!(out, "{row}");
                }
            }
            let _ = writeln!(out);
        }
    }
}

fn unmeasured_section(
    out: &mut String,
    m: &RunManifest,
    timing: &Cells,
    memory: &BTreeMap<Key, BTreeMap<String, Vec<f64>>>,
) {
    let mut missing = BTreeSet::new();
    for &f in &m.facades {
        for p in &m.profiles {
            for s in &m.subjects {
                let key = (f, p.name.clone(), s.label.clone());
                let have = match m.kind {
                    SampleKind::Timing => timing.contains_key(&key),
                    SampleKind::Memory => memory.contains_key(&key),
                };
                if !have {
                    missing.insert(format!(
                        "{}/{}/{} {:?}",
                        f.prefix(),
                        p.name,
                        s.label,
                        m.kind
                    ));
                }
            }
        }
    }
    let _ = writeln!(out, "## Unmeasured cells\n");
    if missing.is_empty() {
        let _ = writeln!(
            out,
            "Every (facade, profile, subject) cell of this run has samples of the run's kind ({:?}).",
            m.kind
        );
    } else {
        let _ = writeln!(
            out,
            "No accepted samples: {}.",
            missing.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    let _ = writeln!(
        out,
        "A run covers one kind; run both for time and space. Other platforms, backends and cache states \
         are not covered by this report.\n"
    );
}

fn definitions(out: &mut String) {
    let _ = writeln!(
        out,
        "## Definitions\n\n\
         - **Library load**: `dlopen` plus resolving every symbol of the facade (one call).\n\
         - **Time to first candidate**: end of the first-result phase minus the start of library load, from \
         the same monotonic readings as the phases; includes the gaps between phases and excludes process \
         start. The *phase sum* row adds only the phase durations.\n\
         - **First usable result**: parse `nihao`, build the candidate list, read the first candidate's \
         string; **not** a typing workload and not steady state.\n\
         - **Fresh process ≠ cold cache.** Cache state is as stated in the header.\n\
         - **RSS** counts shared file-backed pages in full; use PSS or private figures for what the process \
         alone costs. Peak RSS is a high-water mark, not a live figure.\n\
         - **NOT COMPARABLE**: the subjects' complete observations differ (or a timed run contradicted its \
         verification); no ratio is given and none may be inferred.\n\
         - These figures come from a Rust driver and are not comparable with the C instruments'."
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::memory::{MemoryPoint, MemorySnapshot};
    use crate::observe::{Line, ProbeOutcome};
    use crate::orchestrate::{
        CacheState, FileIdentity, HostInfo, ProbeRecord, RUN_SCHEMA, RejectedRecord,
        SubjectIdentity, TreeIdentity, VerdictRecord,
    };
    use crate::sample::{FirstCandidate, PhaseTimes, SAMPLE_SCHEMA, Sample, Timeline};
    use crate::workload::{Bind, workload_digest};

    fn manifest(kind: SampleKind) -> RunManifest {
        let id = |l: &str| FileIdentity {
            file_name: format!("{l}.so"),
            size: 1,
            sha256: "1".repeat(64),
        };
        RunManifest {
            schema: RUN_SCHEMA.to_owned(),
            host: HostInfo {
                os: "linux".into(),
                arch: "x86_64".into(),
                cpus: 4,
                kernel: None,
            },
            probe_sha256: "0".repeat(64),
            workload_digest: workload_digest(),
            subjects: ["pin", "ox"]
                .iter()
                .map(|l| SubjectIdentity {
                    label: (*l).to_owned(),
                    pinyin: Some(id(l)),
                    zhuyin: None,
                })
                .collect(),
            system_data: TreeIdentity {
                name: "data".into(),
                identity: "d".into(),
                files: 1,
                bytes: 1,
            },
            profiles: vec![TreeIdentity {
                name: "empty".into(),
                identity: "e".into(),
                files: 0,
                bytes: 0,
            }],
            facades: vec![Facade::Pinyin],
            kind,
            rounds: 4,
            warmup: 0,
            cache: CacheState::Warm,
            cpu: None,
            bind: Bind::Now,
        }
    }

    fn verdict(subject: &str, comparable: bool, phase: &str) -> Record {
        Record::Verdict(Box::new(VerdictRecord {
            facade: Facade::Pinyin,
            profile: "empty".into(),
            subject: subject.into(),
            reference: "pin".into(),
            phase: phase.into(),
            comparable,
            reasons: if comparable {
                vec![]
            } else {
                vec!["line 4: candidate differs".into()]
            },
            files_compared_by_bytes: vec!["user.conf".into()],
            files_judged_by_content: vec![],
            transcript_lines: 50,
        }))
    }

    fn timing(subject: &str, round: usize, init_ms: u64, warmup: bool) -> Record {
        let ns = init_ms * 1_000_000;
        Record::Sample(Box::new(SampleRecord {
            round,
            warmup,
            facade: Facade::Pinyin,
            subject: subject.to_owned(),
            profile: "empty".to_owned(),
            process_wall_ns: ns * 3,
            loadavg_1m: None,
            sample: Sample {
                schema: SAMPLE_SCHEMA.to_owned(),
                kind: SampleKind::Timing,
                facade: Facade::Pinyin,
                bind: Bind::Now,
                stop_after: StopPoint::End,
                phases: PhaseTimes {
                    lib_load_ns: 1_000_000,
                    init_ns: ns,
                    config_ns: 1000,
                    alloc_ns: 2000,
                    first_candidate: Some(FirstCandidate {
                        parse_ns: 1,
                        length_ns: 1,
                        guess_ns: 1,
                        count_ns: 1,
                        candidate_ns: 1,
                        total_ns: 5_000_000,
                        text: "x".into(),
                        raw_type: 2,
                        count: 1,
                    }),
                    drop_instance_ns: Some(1),
                    fini_ns: Some(2),
                },
                timeline: Timeline::default(),
                ttfc_ns: Some(ns + 7_000_000),
                workload_digest: workload_digest(),
                trace_digest: "t".into(),
                monotonic_total_ns: 1,
                wall_total_ns: 1,
                memory: Vec::new(),
            },
        }))
    }

    fn spec() -> ReportSpec {
        ReportSpec {
            reference: "pin".into(),
            ratios: Vec::new(),
        }
    }

    fn timing_run() -> Vec<Record> {
        let mut r = vec![
            Record::Manifest(Box::new(manifest(SampleKind::Timing))),
            verdict("ox", true, "observation"),
        ];
        for round in 0..4 {
            r.push(timing("pin", round, 10 + (round as u64 % 2), false));
            r.push(timing("ox", round, 20 + (round as u64 % 2), false));
        }
        r
    }

    #[test]
    fn a_comparable_subject_gets_absolute_cells_and_a_ratio() {
        let text = render(&timing_run(), &spec()).unwrap();
        assert!(text.contains("### pinyin / profile `empty`"));
        assert!(text.contains("**time to first candidate**"));
        assert!(text.contains("| ox/pin |"));
        assert!(text.contains("1.9") || text.contains("2.0"), "{text}");
        assert!(text.contains("| pinyin | empty | ox | comparable |"));
    }

    #[test]
    fn a_not_comparable_subject_gets_its_absolute_figures_and_never_a_ratio() {
        let mut r = vec![
            Record::Manifest(Box::new(manifest(SampleKind::Timing))),
            verdict("ox", false, "observation"),
        ];
        for round in 0..4 {
            r.push(timing("pin", round, 10, false));
            r.push(timing("ox", round, 20, false));
        }
        let text = render(&r, &spec()).unwrap();
        assert!(text.contains("**NOT COMPARABLE**"));
        assert!(text.contains("is NOT COMPARABLE:"));
        assert!(text.contains("line 4: candidate differs"));
        // The absolute figure of the subject is still present…
        assert!(text.contains("20.000") || text.contains("20.500"), "{text}");
        // …but no ratio cell contains a number.
        for line in text.lines().filter(|l| l.contains("`*_init`")) {
            assert!(
                !line.contains("×"),
                "a ratio was printed for a NOT COMPARABLE subject: {line}"
            );
        }
    }

    #[test]
    fn a_timed_run_that_contradicts_its_verification_also_withdraws_the_ratio() {
        let mut r = timing_run();
        r.push(verdict("ox", false, "timed-samples"));
        let text = render(&r, &spec()).unwrap();
        assert!(text.contains("**NOT COMPARABLE**"));
        assert!(
            text.lines()
                .filter(|l| l.contains("`*_init`"))
                .all(|l| !l.contains('×'))
        );
    }

    #[test]
    fn a_failed_save_reopen_verdict_withdraws_saved_profiles_but_not_the_empty_one() {
        let mut manifest = manifest(SampleKind::Timing);
        manifest.profiles.push(TreeIdentity {
            name: "phrases".into(),
            identity: "p".into(),
            files: 3,
            bytes: 9,
        });
        let mut r = vec![
            Record::Manifest(Box::new(manifest)),
            verdict("ox", true, "observation"),
        ];
        r.push(Record::Verdict(Box::new(VerdictRecord {
            facade: Facade::Pinyin,
            profile: "persist:save-reopen".into(),
            subject: "ox".into(),
            reference: "pin".into(),
            phase: "observation".into(),
            comparable: false,
            reasons: vec!["file user_bigram.db differs".into()],
            files_compared_by_bytes: vec![],
            files_judged_by_content: vec![],
            transcript_lines: 5,
        })));
        r.push(verdict("ox", true, "observation"));
        let mut phrases = verdict("ox", true, "observation");
        if let Record::Verdict(v) = &mut phrases {
            v.profile = "phrases".into();
        }
        r.push(phrases);
        assert!(comparable(&r, Facade::Pinyin, "empty", "ox", "pin"));
        assert!(!comparable(&r, Facade::Pinyin, "phrases", "ox", "pin"));
        assert!(comparable(&r, Facade::Pinyin, "phrases", "pin", "pin"));
    }

    #[test]
    fn a_subject_with_no_observation_verdict_is_not_comparable() {
        let r = vec![verdict("ox", true, "timed-samples")];
        assert!(!comparable(&r, Facade::Pinyin, "empty", "ox", "pin"));
        assert!(!comparable(&[], Facade::Pinyin, "empty", "ox", "pin"));
        assert!(comparable(&[], Facade::Pinyin, "empty", "pin", "pin"));
    }

    #[test]
    fn warm_up_and_rejected_samples_are_not_aggregated() {
        let mut r = vec![
            Record::Manifest(Box::new(manifest(SampleKind::Timing))),
            verdict("ox", true, "observation"),
        ];
        r.push(timing("pin", 0, 999, true));
        r.push(timing("pin", 1, 10, false));
        r.push(timing("pin", 2, 10, false));
        r.push(Record::Rejected(RejectedRecord {
            round: 3,
            facade: Facade::Pinyin,
            subject: "pin".into(),
            profile: "empty".into(),
            reason: "x".into(),
        }));
        let text = render(&r, &spec()).unwrap();
        assert!(text.contains("n=2") && !text.contains("999"), "{text}");
        assert!(text.contains("Rejected samples (contamination): 1."));
    }

    #[test]
    fn a_subject_without_samples_is_listed_as_unmeasured() {
        let mut r = vec![Record::Manifest(Box::new(manifest(SampleKind::Timing)))];
        r.push(timing("pin", 0, 10, false));
        let text = render(&r, &spec()).unwrap();
        assert!(text.contains("pinyin/empty/ox Timing"), "{text}");
    }

    #[test]
    fn hostile_input_outcomes_are_shown_with_their_comparison() {
        let mut r = timing_run();
        let outcome = |exit| ProbeOutcome {
            input: "'".into(),
            exit,
            lines: vec![Line {
                op: "a".into(),
                value: "b".into(),
            }],
            stderr_sha256: "s".into(),
        };
        r.push(Record::Probe(Box::new(ProbeRecord {
            facade: Facade::Pinyin,
            subject: "pin".into(),
            outcome: outcome(ExitClass::Signal(6)),
            against_reference: "reference".into(),
        })));
        r.push(Record::Probe(Box::new(ProbeRecord {
            facade: Facade::Pinyin,
            subject: "ox".into(),
            outcome: outcome(ExitClass::Returned),
            against_reference: "DIFFERENT: reference ended Signal(6), subject ended Returned"
                .into(),
        })));
        let text = render(&r, &spec()).unwrap();
        assert!(
            text.contains("killed by signal 6") && text.contains("DIFFERENT"),
            "{text}"
        );
    }

    #[test]
    fn unknown_subjects_and_missing_manifests_are_refused() {
        assert!(render(&[], &spec()).is_err());
        let bad = ReportSpec {
            reference: "nope".into(),
            ratios: vec![],
        };
        assert!(render(&timing_run(), &bad).is_err());
        let ratio = ReportSpec {
            reference: "pin".into(),
            ratios: vec![RatioSpec {
                numerator: "ox".into(),
                denominator: "ghost".into(),
            }],
        };
        assert!(render(&timing_run(), &ratio).is_err());
    }

    fn memory_record(subject: &str, round: usize, rss: [u64; 3]) -> Record {
        let point = |name: &str, kib: u64| MemoryPoint {
            point: name.to_owned(),
            snapshot: MemorySnapshot {
                source: "procfs".into(),
                rss_kib: Some(kib),
                peak_rss_kib: Some(kib + 10),
                vm_size_kib: Some(kib * 100),
                anon_kib: Some(kib / 2),
                file_kib: Some(kib / 2),
                minor_faults: Some(kib / 10),
                ..MemorySnapshot::default()
            },
        };
        let Record::Sample(mut s) = timing(subject, round, 1, false) else {
            unreachable!()
        };
        s.sample.kind = SampleKind::Memory;
        s.sample.memory = vec![
            point("baseline", rss[0]),
            point("init", rss[1]),
            point("end", rss[2]),
        ];
        Record::Sample(s)
    }

    #[test]
    fn the_memory_report_uses_the_delta_labels_virtual_size_and_counts_faults() {
        let mut r = vec![
            Record::Manifest(Box::new(manifest(SampleKind::Memory))),
            verdict("ox", true, "observation"),
        ];
        for round in 0..3 {
            r.push(memory_record("pin", round, [10_240, 30_720, 40_960]));
            r.push(memory_record("ox", round, [10_240, 20_480, 30_720]));
        }
        let text = render(&r, &spec()).unwrap();
        assert!(text.contains("init · rss_delta_mib | 20.00"), "{text}");
        assert!(text.contains("0.50×"), "{text}");
        assert!(text.contains("**not** RAM") && text.contains("init · minor_faults_count"));
        assert!(!text.contains("## Timing"));
    }
}
