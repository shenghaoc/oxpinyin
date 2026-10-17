//! Readers for the deterministic whole-process profilers.
//!
//! Wall-clock timing and profiling are separate runs: a profiler slows the
//! process by an order of magnitude and unevenly. These readers turn a
//! profiler's text output into numbers; the spawning is in the binary. The
//! figures are whole-process (the probe, the loader and the library), and a
//! per-phase cost is the difference between two runs that stop at consecutive
//! phases (`StopPoint`), which is exact for instruction counts because the
//! process prefix is identical.
//!
//! Callgrind's instruction count (`Ir`) and DHAT's allocation totals are
//! deterministic for a fixed input on one host and Valgrind version, and move
//! with both (`docs/runbooks/benches.md`, "State the Valgrind version").

use serde::{Deserialize, Serialize};

use crate::error::BenchError;

/// Callgrind's total instruction count.
///
/// # Errors
///
/// [`BenchError::Invalid`] if the output has no `totals:` (or `summary:`)
/// line: a truncated profile is not a count of zero.
pub fn parse_callgrind_ir(out: &str) -> Result<u64, BenchError> {
    let value = |prefix: &str| {
        out.lines()
            .rev()
            .find_map(|line| line.strip_prefix(prefix))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|digits| digits.parse::<u64>().ok())
    };
    value("totals:")
        .or_else(|| value("summary:"))
        .ok_or_else(|| BenchError::invalid("callgrind output has no totals line"))
}

/// DHAT's heap totals for a process.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HeapTotals {
    /// Bytes allocated over the whole run (not live bytes).
    pub total_bytes: u64,
    /// Allocation calls over the whole run.
    pub total_blocks: u64,
    /// Live bytes at the global peak (`t-gmax`).
    pub peak_bytes: u64,
    /// Live blocks at the global peak.
    pub peak_blocks: u64,
    /// Bytes still live at exit (`t-end`).
    pub end_bytes: u64,
    /// Blocks still live at exit.
    pub end_blocks: u64,
}

fn counts(rest: &str) -> Option<(u64, u64)> {
    // "1,234 bytes in 56 blocks"
    let cleaned: String = rest.chars().filter(|c| *c != ',').collect();
    let mut words = cleaned.split_whitespace();
    let bytes = words.next()?.parse().ok()?;
    (words.next()? == "bytes" && words.next()? == "in").then_some(())?;
    let blocks = words.next()?.parse().ok()?;
    Some((bytes, blocks))
}

/// Parses the `Total:`, `At t-gmax:` and `At t-end:` lines DHAT prints on
/// standard error.
///
/// # Errors
///
/// [`BenchError::Invalid`] if any of the three lines is missing.
pub fn parse_dhat_summary(stderr: &str) -> Result<HeapTotals, BenchError> {
    let find = |label: &str| {
        stderr.lines().find_map(|line| {
            let body = line.split_once("== ").map_or(line, |(_, rest)| rest);
            body.strip_prefix(label)
                .and_then(|rest| counts(rest.trim()))
        })
    };
    match (find("Total:"), find("At t-gmax:"), find("At t-end:")) {
        (Some(total), Some(peak), Some(end)) => Ok(HeapTotals {
            total_bytes: total.0,
            total_blocks: total.1,
            peak_bytes: peak.0,
            peak_blocks: peak.1,
            end_bytes: end.0,
            end_blocks: end.1,
        }),
        _ => Err(BenchError::invalid(
            "DHAT output lacks the Total, At t-gmax or At t-end line",
        )),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn callgrind_totals_are_read() {
        let out =
            "version: 1\ncmd: x\nevents: Ir\nsummary: 1234567\nfn=(1)\n0 5\ntotals: 1234567\n";
        assert_eq!(parse_callgrind_ir(out).unwrap(), 1_234_567);
        assert_eq!(parse_callgrind_ir("summary: 9\n").unwrap(), 9);
    }

    #[test]
    fn a_truncated_callgrind_profile_is_an_error() {
        assert!(parse_callgrind_ir("version: 1\nevents: Ir\n").is_err());
        assert!(parse_callgrind_ir("totals: abc\n").is_err());
    }

    #[test]
    fn dhat_summary_is_read() {
        let err = "==12== DHAT, a dynamic heap analysis tool\n==12== Total:     68,424 bytes in 123 blocks\n==12== At t-gmax: 6,424 bytes in 12 blocks\n==12== At t-end:  100 bytes in 2 blocks\n==12== Reads:     1 bytes\n";
        let totals = parse_dhat_summary(err).unwrap();
        assert_eq!(totals.total_bytes, 68_424);
        assert_eq!(totals.total_blocks, 123);
        assert_eq!(totals.peak_bytes, 6_424);
        assert_eq!(totals.end_blocks, 2);
    }

    #[test]
    fn missing_dhat_lines_are_an_error() {
        assert!(parse_dhat_summary("==1== Total:     1 bytes in 1 blocks\n").is_err());
        assert!(parse_dhat_summary("").is_err());
    }
}
