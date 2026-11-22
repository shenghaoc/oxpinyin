//! Whole-target modules. Shared workload code has no OS or CPU branches.
#[cfg(target_os = "linux")]
#[expect(
    unsafe_code,
    reason = "Linux process and allocator diagnostics through libc"
)]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::*;
#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "Darwin kernel counters and loaded-image identity through libc"
)]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows process counters, loaded-module and allocator identity"
)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::*;
#[cfg(unix)]
#[expect(
    unsafe_code,
    reason = "POSIX timing, loader and immediate exit through libc"
)]
mod posix;
#[cfg(unix)]
pub(crate) use posix::*;
#[cfg(not(target_os = "linux"))]
mod unprofiled;
#[cfg(not(target_os = "linux"))]
pub(crate) use unprofiled::*;
