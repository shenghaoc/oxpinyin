use crate::Result;
use std::ffi::{CStr, CString};
use std::io::{Read, Write};
use valgrind_requests::callgrind;

pub(crate) struct Collection;
impl Collection {
    pub(crate) fn new() -> Self {
        callgrind::toggle_collect();
        Self
    }
}
impl Drop for Collection {
    fn drop(&mut self) {
        callgrind::toggle_collect();
    }
}
/// Stops the collection that Callgrind starts with the process and zeroes what it counted.
pub(crate) fn profiling_reset() {
    callgrind::toggle_collect();
    callgrind::zero_stats();
}
pub(crate) fn dump(label: &CStr) {
    callgrind::dump_stats_at(label);
}

pub(crate) struct Counter(perf_event::Counter);
impl Counter {
    pub(crate) fn open() -> Result<Self> {
        // `kind` consumes the builder and the exclusions borrow it, so `build` needs a binding.
        let mut builder =
            perf_event::Builder::new().kind(perf_event::events::Hardware::INSTRUCTIONS);
        builder.exclude_kernel(true).exclude_hv(true);
        Ok(Self(builder.build()?))
    }
    pub(crate) fn enable(&mut self) {
        let _ = self.0.enable();
    }
    pub(crate) fn sample(&mut self) -> Option<u64> {
        let _ = self.0.disable();
        let count = self.0.read().ok();
        let _ = self.0.reset();
        count
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Memory {
    values: [i64; 5],
}
impl Memory {
    pub(crate) fn read() -> Self {
        Self {
            values: proc_values(
                "/proc/self/status",
                &["VmRSS", "VmHWM", "VmSize", "RssAnon", "RssFile"],
            ),
        }
    }
}
impl std::fmt::Display for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [rss, hwm, size, anon, file] = self.values;
        write!(
            f,
            "{{\"rss_kib\":{rss},\"hwm_kib\":{hwm},\"vm_size_kib\":{size},\"rss_anon_kib\":{anon},\"rss_file_kib\":{file}}}"
        )
    }
}

// Stack-only /proc parser. Reading diagnostics must not create a retained Rust
// allocation that changes the allocator or post-trim measurements.
fn proc_values<const N: usize>(path: &str, names: &[&str; N]) -> [i64; N] {
    let mut result = [-1; N];
    let Ok(mut file) = std::fs::File::open(path) else {
        return result;
    };
    let mut line = [0u8; 512];
    let mut used = 0;
    let mut byte = [0];
    while file.read(&mut byte).is_ok_and(|n| n == 1) {
        if byte[0] == b'\n' {
            if let Ok(text) = std::str::from_utf8(&line[..used])
                && let Some((key, value)) = text.split_once(':')
            {
                for (i, name) in names.iter().enumerate() {
                    if key == *name {
                        result[i] = value
                            .split_whitespace()
                            .next()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(-1);
                    }
                }
            }
            used = 0;
        } else if used < line.len() {
            line[used] = byte[0];
            used += 1;
        }
    }
    result
}

pub(crate) struct Diagnostic {
    rollup: [i64; 8],
    arena: Option<[u64; 6]>,
}
impl Diagnostic {
    pub(crate) fn read() -> Self {
        let mut rollup = proc_values(
            "/proc/self/smaps_rollup",
            &[
                "Rss",
                "Pss",
                "Shared_Clean",
                "Shared_Dirty",
                "Private_Clean",
                "Private_Dirty",
                "Anonymous",
                "__derived_file",
            ],
        );
        if rollup[0] >= 0 && rollup[6] >= 0 {
            rollup[7] = rollup[0] - rollup[6];
        }
        Self {
            rollup,
            arena: allocator::arena(),
        }
    }
    pub(crate) fn write(&self, out: &mut impl Write, stage: &str) -> std::io::Result<()> {
        write!(out, ",\"rollup_{stage}\":{{")?;
        for (i, name) in [
            "rss_kib",
            "pss_kib",
            "shared_clean_kib",
            "shared_dirty_kib",
            "private_clean_kib",
            "private_dirty_kib",
            "anonymous_kib",
            "file_kib",
        ]
        .iter()
        .enumerate()
        {
            if i != 0 {
                out.write_all(b",")?;
            }
            write!(out, "\"{name}\":{}", self.rollup[i])?;
        }
        write!(out, "}},\"arena_{stage}\":")?;
        match self.arena {
            None => out.write_all(b"null"),
            Some(values) => {
                out.write_all(b"{")?;
                for (i, name) in [
                    "arena", "hblks", "hblkhd", "uordblks", "fordblks", "keepcost",
                ]
                .iter()
                .enumerate()
                {
                    if i != 0 {
                        out.write_all(b",")?;
                    }
                    write!(out, "\"{name}\":{}", values[i])?;
                }
                out.write_all(b"}")
            }
        }
    }
    #[expect(
        clippy::unused_self,
        reason = "The dump is attached to the diagnostic snapshot at shared call sites"
    )]
    pub(crate) fn dump(&self, directory: &str, tag: &str, stage: &str) {
        for (what, source) in [
            ("maps", "/proc/self/maps"),
            ("smaps", "/proc/self/smaps"),
            ("smaps-rollup", "/proc/self/smaps_rollup"),
        ] {
            let target = format!("{directory}/{tag}-{what}-{stage}.txt");
            if target.len() < 512 {
                let _ = std::fs::copy(source, target);
            }
        }
        if stage == "init" {
            allocator::dump(directory, tag);
        }
    }
}
pub(crate) fn trim() -> &'static str {
    allocator::trim()
}

// libc-specific capabilities are also selected as whole modules.
#[cfg(target_env = "gnu")]
mod allocator {
    use super::*;
    #[expect(
        clippy::unnecessary_wraps,
        reason = "The non-GNU allocator cannot provide arena counters"
    )]
    pub(super) fn arena() -> Option<[u64; 6]> {
        // SAFETY: mallinfo2 has no preconditions and returns a value snapshot.
        let info = unsafe { libc::mallinfo2() };
        Some([
            info.arena as u64,
            info.hblks as u64,
            info.hblkhd as u64,
            info.uordblks as u64,
            info.fordblks as u64,
            info.keepcost as u64,
        ])
    }
    pub(super) fn trim() -> &'static str {
        // SAFETY: malloc_trim(0) requests release of all free top-of-heap pages.
        if unsafe { libc::malloc_trim(0) } != 0 {
            "1"
        } else {
            "0"
        }
    }
    pub(super) fn dump(directory: &str, tag: &str) {
        let Ok(path) = CString::new(format!("{directory}/{tag}-mallocinfo.xml")) else {
            return;
        };
        // SAFETY: path and mode are live, NUL-terminated strings.
        let file = unsafe { libc::fopen(path.as_ptr(), c"w".as_ptr()) };
        if !file.is_null() {
            // SAFETY: fopen returned an open writable stream and options is zero.
            unsafe { libc::malloc_info(0, file) };
            // SAFETY: this is the successful fopen stream, not yet closed.
            unsafe { libc::fclose(file) };
        }
    }
}
#[cfg(not(target_env = "gnu"))]
mod allocator {
    pub(super) fn arena() -> Option<[u64; 6]> {
        None
    }
    pub(super) fn trim() -> &'static str {
        "null"
    }
    pub(super) fn dump(_directory: &str, _tag: &str) {}
}

pub(crate) fn native(_args: &[String]) -> Result<()> {
    Err("Linux uses --perf".into())
}

/// Linux runs every `bisect` mode that the shared dispatch names.
pub(crate) fn refusal(_mode: Option<&str>) -> Option<&'static str> {
    None
}
