use crate::Result;
use std::ffi::{CStr, CString, OsStr};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
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
    /// The count since `enable`, or `None` when the read fails or the kernel
    /// has timeshared the counter: a sample counts only when the counter ran
    /// for its whole span (`time_running == time_enabled`). A reset clears the
    /// count, not the times, so a counter that was timeshared once gives no
    /// further samples.
    pub(crate) fn sample(&mut self) -> Option<u64> {
        let _ = self.0.disable();
        let read = self.0.read_count_and_time().ok();
        let _ = self.0.reset();
        read.filter(|read| read.time_running == read.time_enabled)
            .map(|read| read.count)
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

/// `PERF_PAGE_OUT`: pages out, in this process, the mappings of the library and of the files
/// directly in the system data directory, as an idle system reclaims them. A file is known by
/// its device and inode, so a symbolic link in the given paths does not hide its mappings.
pub(crate) struct PageOut {
    library: FileId,
    data: FileId,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileId {
    device: u64,
    inode: u64,
}
impl FileId {
    fn of(path: &Path) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}
impl PageOut {
    pub(crate) fn new(library: &str, data: &str) -> Result<Self> {
        let id = |path: &str| {
            FileId::of(path.as_ref()).map_err(|error| format!("PERF_PAGE_OUT: {path}: {error}"))
        };
        Ok(Self {
            library: id(library)?,
            data: id(data)?,
        })
    }
    /// Calls `madvise(MADV_PAGEOUT)` on each mapping of the library and of a data file, then
    /// `posix_fadvise(POSIX_FADV_DONTNEED)` on its file. Fails when the kernel refuses, or when
    /// the library or the data has no mapping. The read runs before the measured key starts.
    pub(crate) fn run(&self) -> Result<super::PagedOut> {
        // Bytes, not a string: a mapped path need not be UTF-8.
        let maps = std::fs::read("/proc/self/maps")?;
        let mut paged = super::PagedOut::default();
        for line in maps.split(|&b| b == b'\n') {
            self.advise(line, &mut paged)?;
        }
        crate::require(
            paged.library_bytes != 0 && paged.data_bytes != 0,
            "PERF_PAGE_OUT: the library or the data has no mapping",
        )?;
        Ok(paged)
    }
    /// Pages out one `/proc/self/maps` line's mapping when it maps the library or a data file.
    fn advise(&self, line: &[u8], paged: &mut super::PagedOut) -> Result<()> {
        // The path starts at the line's first `/`: no earlier field contains one.
        let Some(at) = line.iter().position(|&b| b == b'/') else {
            return Ok(());
        };
        let path = Path::new(OsStr::from_bytes(&line[at..]));
        // A path that no longer names a file, such as a deleted one, is not the subject's.
        let Ok(id) = FileId::of(path) else {
            return Ok(());
        };
        let library = id == self.library;
        if !library && path.parent().and_then(|parent| FileId::of(parent).ok()) != Some(self.data) {
            return Ok(());
        }
        let range = mapping_range(line).ok_or("PERF_PAGE_OUT: unreadable /proc/self/maps line")?;
        let bytes = range.len();
        // SAFETY: the range is a mapping of this process, as /proc/self/maps lists it.
        // MADV_PAGEOUT reclaims its pages and leaves the mapping and its contents unchanged.
        let status = unsafe {
            libc::madvise(
                std::ptr::without_provenance_mut(range.start),
                bytes,
                libc::MADV_PAGEOUT,
            )
        };
        if status != 0 {
            let error = std::io::Error::last_os_error();
            return Err(format!("PERF_PAGE_OUT: madvise {}: {error}", path.display()).into());
        }
        let file = std::fs::File::open(path)
            .map_err(|error| format!("PERF_PAGE_OUT: {}: {error}", path.display()))?;
        // SAFETY: file is open for the call; offset 0 and length 0 cover the whole file.
        let status =
            unsafe { libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
        if status != 0 {
            let error = std::io::Error::from_raw_os_error(status);
            return Err(format!("PERF_PAGE_OUT: posix_fadvise {}: {error}", path.display()).into());
        }
        paged.mappings += 1;
        if library {
            paged.library_bytes += bytes as u64;
        } else {
            paged.data_bytes += bytes as u64;
        }
        Ok(())
    }
}
/// The address range of a `/proc/self/maps` line: its first field, `start-end` in hexadecimal.
fn mapping_range(line: &[u8]) -> Option<std::ops::Range<usize>> {
    let field = line.split(|&b| b == b' ').next()?;
    let (start, end) = std::str::from_utf8(field).ok()?.split_once('-')?;
    Some(usize::from_str_radix(start, 16).ok()?..usize::from_str_radix(end, 16).ok()?)
}

pub(crate) fn native(_args: &[String]) -> Result<()> {
    Err("Linux uses --perf".into())
}

/// Linux runs every `bisect` mode that the shared dispatch names.
pub(crate) fn refusal(_mode: Option<&str>) -> Option<&'static str> {
    None
}
