//! Non-Linux targets do not compile Valgrind or perf-event dependencies.
use crate::Result;
use std::ffi::CStr;
pub(crate) struct Collection;
impl Drop for Collection {
    fn drop(&mut self) {}
}
impl Collection {
    pub(crate) fn new() -> Self {
        Self
    }
}
/// The strict cycle count of the native `--measure` modes, which only these targets run.
pub(crate) fn strict_cycles() -> Result<usize> {
    let value = std::env::var("PERF_CYCLES")?;
    let cycles = value.trim_start().parse::<usize>()?;
    crate::require(
        (1..=100000).contains(&cycles),
        "measurement: invalid cycle count",
    )?;
    Ok(cycles)
}
pub(crate) fn profiling_reset() {}
pub(crate) fn dump(_label: &CStr) {}
pub(crate) struct Counter;
impl Counter {
    pub(crate) fn open() -> Result<Self> {
        Err(std::io::Error::from_raw_os_error(78).into())
    }
    pub(crate) fn enable(&mut self) {}
    pub(crate) fn sample(&mut self) -> Option<u64> {
        None
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Memory;
impl Memory {
    pub(crate) fn read() -> Self {
        Self
    }
}
impl std::fmt::Display for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("{\"rss_kib\":-1,\"hwm_kib\":-1,\"vm_size_kib\":-1,\"rss_anon_kib\":-1,\"rss_file_kib\":-1}")
    }
}
pub(crate) struct Diagnostic;
impl Diagnostic {
    pub(crate) fn read() -> Self {
        Self
    }
    pub(crate) fn dump(&self, _directory: &str, _tag: &str, _stage: &str) {}
    pub(crate) fn write(&self, out: &mut impl std::io::Write, stage: &str) -> std::io::Result<()> {
        write!(
            out,
            ",\"rollup_{stage}\":{{\"rss_kib\":-1,\"pss_kib\":-1,\"shared_clean_kib\":-1,\"shared_dirty_kib\":-1,\"private_clean_kib\":-1,\"private_dirty_kib\":-1,\"anonymous_kib\":-1,\"file_kib\":-1}},\"arena_{stage}\":null"
        )
    }
}
pub(crate) fn trim() -> &'static str {
    "null"
}
