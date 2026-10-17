//! Process memory at a point in time, behind one narrow interface.
//!
//! Portable measurement logic asks a [`MemoryProbe`] for a
//! [`MemorySnapshot`]; which operating-system facility answers is a detail of
//! the probe. Parsing is separated from reading so each platform's text format
//! is covered by tests on any host.
//!
//! What the figures mean (and do not):
//!
//! * `rss_kib` is the resident set: pages of the process currently in physical
//!   memory, **shared pages counted in full**. It is not the memory the process
//!   alone costs; `pss_kib` (Linux) apportions shared pages and
//!   `private_*_kib` count pages no other process maps.
//! * `anon_kib` is anonymous memory (heap, stacks, anonymous mappings);
//!   `file_kib` is `rss - anon`, resident pages backed by a file (the library's
//!   text, mapped data files).
//! * `peak_rss_kib` is the kernel's high-water mark of the resident set
//!   (`VmHWM`); it only ever rises, so it bounds everything before the point
//!   it was read.
//! * `minor_faults` and `major_faults` are the process's cumulative page-fault
//!   counts (Linux `/proc/self/stat`); a mapped file's pages show up here as
//!   they are first touched, which is how a scan of a mapped file costs time
//!   without allocating anything.
//! * `vm_size_kib` is **virtual** size: address space reserved, not RAM. It is
//!   recorded because mappings show up there, and must never be read as
//!   consumption.
//!
//! Linux reads `/proc/self`. macOS has no procfs and no unprivileged,
//! dependency-free equivalent of `smaps`; the macOS probe asks `ps` for the
//! resident and virtual sizes only, leaving the other fields `None` rather
//! than invented. Peak and private/file-backed splits on macOS need
//! `footprint`/`vmmap` and are a deeper, separate investigation.

use std::fs;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::BenchError;

/// One reading of the process's memory. Fields a platform cannot supply are
/// `None`, never zero.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemorySnapshot {
    /// Which facility produced this (`procfs`, `ps`).
    pub source: String,
    /// Resident set size, KiB (shared pages counted in full).
    pub rss_kib: Option<u64>,
    /// High-water mark of the resident set, KiB.
    pub peak_rss_kib: Option<u64>,
    /// Virtual size, KiB. Address space, not RAM.
    pub vm_size_kib: Option<u64>,
    /// Anonymous resident memory, KiB.
    pub anon_kib: Option<u64>,
    /// Resident memory backed by a file, KiB (`rss - anon`).
    pub file_kib: Option<u64>,
    /// Proportional set size, KiB.
    pub pss_kib: Option<u64>,
    /// Resident pages mapped only by this process and modified, KiB.
    pub private_dirty_kib: Option<u64>,
    /// Resident pages mapped only by this process and unmodified, KiB.
    pub private_clean_kib: Option<u64>,
    /// Page faults served without I/O (cumulative for the process).
    pub minor_faults: Option<u64>,
    /// Page faults that needed I/O (cumulative for the process).
    pub major_faults: Option<u64>,
}

/// A reading at a named point of the run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryPoint {
    /// `baseline`, `lib_load`, `init`, `options`, `alloc`, `first_candidate`, `end`.
    pub point: String,
    /// The reading.
    pub snapshot: MemorySnapshot,
}

/// A source of [`MemorySnapshot`]s for the current process.
pub trait MemoryProbe {
    /// Reads the process's memory now.
    ///
    /// # Errors
    ///
    /// The facility is unavailable or produced text that does not parse.
    fn snapshot(&self) -> Result<MemorySnapshot, BenchError>;
}

/// The probe for this host.
///
/// # Errors
///
/// [`BenchError::Unsupported`] on a host with no probe; there is no silent
/// fallback to a different, non-comparable figure.
pub fn host_probe() -> Result<Box<dyn MemoryProbe>, BenchError> {
    if cfg!(target_os = "linux") {
        Ok(Box::new(ProcfsProbe))
    } else if cfg!(target_os = "macos") {
        Ok(Box::new(PsProbe))
    } else {
        Err(BenchError::Unsupported(
            "no memory probe for this operating system".to_owned(),
        ))
    }
}

/// Linux: `/proc/self/status` and `/proc/self/smaps_rollup`.
#[derive(Clone, Copy, Debug)]
pub struct ProcfsProbe;

impl MemoryProbe for ProcfsProbe {
    fn snapshot(&self) -> Result<MemorySnapshot, BenchError> {
        let status = fs::read_to_string("/proc/self/status")
            .map_err(|e| BenchError::io("reading /proc/self/status", e))?;
        let rollup = fs::read_to_string("/proc/self/smaps_rollup")
            .map_err(|e| BenchError::io("reading /proc/self/smaps_rollup", e))?;
        let stat = fs::read_to_string("/proc/self/stat")
            .map_err(|e| BenchError::io("reading /proc/self/stat", e))?;
        let mut snapshot = parse_proc_status(&status)?;
        merge_smaps_rollup(&mut snapshot, &rollup)?;
        let (minor, major) = parse_proc_stat_faults(&stat)?;
        snapshot.minor_faults = Some(minor);
        snapshot.major_faults = Some(major);
        Ok(snapshot)
    }
}

/// macOS: the resident and virtual size from `ps`.
#[derive(Clone, Copy, Debug)]
pub struct PsProbe;

impl MemoryProbe for PsProbe {
    fn snapshot(&self) -> Result<MemorySnapshot, BenchError> {
        let pid = std::process::id().to_string();
        let output = Command::new("ps")
            .args(["-o", "rss=,vsz=", "-p", &pid])
            .output()
            .map_err(|e| BenchError::io("running ps", e))?;
        if !output.status.success() {
            return Err(BenchError::Child {
                command: format!("ps -o rss=,vsz= -p {pid}"),
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        parse_ps(&String::from_utf8_lossy(&output.stdout))
    }
}

fn kib_value(line: &str, key: &str) -> Option<u64> {
    let rest = line.strip_prefix(key)?.strip_prefix(':')?;
    let mut parts = rest.split_whitespace();
    let value = parts.next()?.parse::<u64>().ok()?;
    match parts.next() {
        Some("kB") | None => Some(value),
        Some(_) => None,
    }
}

/// Parses `/proc/<pid>/status` for `VmRSS`, `VmHWM` and `VmSize`.
///
/// # Errors
///
/// [`BenchError::Invalid`] if `VmRSS` is absent (a kernel thread or a text
/// that is not a status file).
pub fn parse_proc_status(text: &str) -> Result<MemorySnapshot, BenchError> {
    let mut snapshot = MemorySnapshot {
        source: "procfs".to_owned(),
        ..MemorySnapshot::default()
    };
    for line in text.lines() {
        if let Some(value) = kib_value(line, "VmRSS") {
            snapshot.rss_kib = Some(value);
        } else if let Some(value) = kib_value(line, "VmHWM") {
            snapshot.peak_rss_kib = Some(value);
        } else if let Some(value) = kib_value(line, "VmSize") {
            snapshot.vm_size_kib = Some(value);
        }
    }
    if snapshot.rss_kib.is_none() {
        return Err(BenchError::invalid("status text has no VmRSS line"));
    }
    Ok(snapshot)
}

/// Parses the minor and major page-fault counts (fields 10 and 12) of
/// `/proc/<pid>/stat`. The command name is parenthesised and may itself contain
/// spaces and parentheses, so fields are counted from the *last* `)`.
///
/// # Errors
///
/// [`BenchError::Invalid`] if the text is not a stat line.
pub fn parse_proc_stat_faults(text: &str) -> Result<(u64, u64), BenchError> {
    let after = text
        .rsplit_once(')')
        .map(|(_, rest)| rest)
        .ok_or_else(|| BenchError::invalid("stat text has no command field"))?;
    // After the command: state (field 3), then ppid, pgrp, session, tty_nr,
    // tpgid, flags (4-9), minflt (10), cminflt (11), majflt (12).
    let fields: Vec<&str> = after.split_whitespace().collect();
    let get = |field: usize| {
        fields
            .get(field - 3)
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| BenchError::invalid(format!("stat field {field} is missing")))
    };
    Ok((get(10)?, get(12)?))
}

/// Adds the `smaps_rollup` fields to `snapshot` and derives `file_kib`.
///
/// # Errors
///
/// [`BenchError::Invalid`] if the text has no `Rss` or no `Anonymous` line.
pub fn merge_smaps_rollup(snapshot: &mut MemorySnapshot, text: &str) -> Result<(), BenchError> {
    let mut rss = None;
    for line in text.lines() {
        if let Some(value) = kib_value(line, "Rss") {
            rss = Some(value);
        } else if let Some(value) = kib_value(line, "Pss") {
            snapshot.pss_kib = Some(value);
        } else if let Some(value) = kib_value(line, "Private_Dirty") {
            snapshot.private_dirty_kib = Some(value);
        } else if let Some(value) = kib_value(line, "Private_Clean") {
            snapshot.private_clean_kib = Some(value);
        } else if let Some(value) = kib_value(line, "Anonymous") {
            snapshot.anon_kib = Some(value);
        }
    }
    let (Some(rss), Some(anon)) = (rss, snapshot.anon_kib) else {
        return Err(BenchError::invalid(
            "smaps_rollup text lacks Rss or Anonymous",
        ));
    };
    // The kernel reads smaps_rollup and status at different instants, so keep
    // the two resident figures separate and derive the split from rollup's own.
    snapshot.file_kib = Some(rss.saturating_sub(anon));
    Ok(())
}

/// Parses the output of `ps -o rss=,vsz= -p <pid>` (both KiB on macOS).
///
/// # Errors
///
/// [`BenchError::Invalid`] if the line is not two integers.
pub fn parse_ps(text: &str) -> Result<MemorySnapshot, BenchError> {
    let mut fields = text.split_whitespace();
    let rss = fields.next().and_then(|v| v.parse::<u64>().ok());
    let vsz = fields.next().and_then(|v| v.parse::<u64>().ok());
    match (rss, vsz, fields.next()) {
        (Some(rss), Some(vsz), None) => Ok(MemorySnapshot {
            source: "ps".to_owned(),
            rss_kib: Some(rss),
            vm_size_kib: Some(vsz),
            ..MemorySnapshot::default()
        }),
        _ => Err(BenchError::invalid(format!(
            "unexpected `ps -o rss=,vsz=` output: {text:?}"
        ))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const STATUS: &str = "Name:\tprobe\nVmPeak:\t  900000 kB\nVmSize:\t  800000 kB\nVmHWM:\t   30000 kB\nVmRSS:\t   20000 kB\nThreads:\t1\n";
    const ROLLUP: &str = "00400000-7ffd0000 ---p 00000000 00:00 0   [rollup]\nRss:               20000 kB\nPss:               15000 kB\nShared_Clean:       6000 kB\nPrivate_Clean:      4000 kB\nPrivate_Dirty:     10000 kB\nAnonymous:         11000 kB\n";

    #[test]
    fn status_fields_are_read() {
        let snapshot = parse_proc_status(STATUS).unwrap();
        assert_eq!(snapshot.rss_kib, Some(20_000));
        assert_eq!(snapshot.peak_rss_kib, Some(30_000));
        assert_eq!(snapshot.vm_size_kib, Some(800_000));
        assert_eq!(snapshot.source, "procfs");
    }

    #[test]
    fn status_without_rss_is_refused() {
        assert!(parse_proc_status("Name:\tkthread\nVmSize:\t0 kB\n").is_err());
    }

    #[test]
    fn rollup_derives_the_file_backed_split() {
        let mut snapshot = parse_proc_status(STATUS).unwrap();
        merge_smaps_rollup(&mut snapshot, ROLLUP).unwrap();
        assert_eq!(snapshot.anon_kib, Some(11_000));
        assert_eq!(snapshot.file_kib, Some(9_000));
        assert_eq!(snapshot.pss_kib, Some(15_000));
        assert_eq!(snapshot.private_dirty_kib, Some(10_000));
        assert_eq!(snapshot.private_clean_kib, Some(4_000));
    }

    #[test]
    fn truncated_rollup_is_refused_not_zero_filled() {
        let mut snapshot = parse_proc_status(STATUS).unwrap();
        assert!(merge_smaps_rollup(&mut snapshot, "Rss: 5 kB\n").is_err());
        assert_eq!(snapshot.file_kib, None);
    }

    #[test]
    fn ps_output_is_two_integers() {
        let snapshot = parse_ps("  12345 4400000\n").unwrap();
        assert_eq!(snapshot.rss_kib, Some(12_345));
        assert_eq!(snapshot.vm_size_kib, Some(4_400_000));
        assert_eq!(snapshot.peak_rss_kib, None);
        assert_eq!(snapshot.file_kib, None);
        assert!(parse_ps("").is_err());
        assert!(parse_ps("1 2 3").is_err());
        assert!(parse_ps("x y").is_err());
    }

    #[test]
    fn fault_counts_come_from_after_the_last_parenthesis() {
        let stat = "123 (we ird) name) S 1 123 123 0 -1 4194560 777 0 12 0 5 3 0 0 20 0 1 0 100 5000 400 18446744073709551615";
        assert_eq!(parse_proc_stat_faults(stat).unwrap(), (777, 12));
        assert!(parse_proc_stat_faults("no parenthesis").is_err());
        assert!(parse_proc_stat_faults("1 (x) S 1 2").is_err());
    }

    #[test]
    fn a_unit_other_than_kb_is_not_misread() {
        assert_eq!(kib_value("VmRSS: 5 MB", "VmRSS"), None);
        assert_eq!(kib_value("VmRSS: 5 kB", "VmRSS"), Some(5));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_live_linux_probe_reads_this_process() {
        let snapshot = ProcfsProbe.snapshot().unwrap();
        assert!(snapshot.rss_kib.unwrap() > 0);
        assert!(snapshot.peak_rss_kib.unwrap() >= snapshot.rss_kib.unwrap());
        assert!(snapshot.file_kib.is_some());
        assert!(snapshot.minor_faults.unwrap() > 0);
        assert!(snapshot.major_faults.is_some());
    }
}
