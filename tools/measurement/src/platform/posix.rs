use crate::{Result, require};
use std::ffi::{CString, c_void};
use std::path::Path;

pub(crate) fn load(path: &str) -> Result<libloading::Library> {
    // SAFETY: The selected subject implements the C ABI used by this process.
    let library = unsafe {
        libloading::os::unix::Library::open(
            Some(path),
            libloading::os::unix::RTLD_NOW | libloading::os::unix::RTLD_LOCAL,
        )?
    };
    Ok(library.into())
}
pub(crate) fn release(library: &libloading::Library) -> Result<extern "C" fn(*mut c_void)> {
    // SAFETY: GLib's g_free has this signature and the loaded subject keeps its
    // dependency alive until all returned strings have been released.
    unsafe { Ok(*library.get(b"g_free")?) }
}
pub(crate) fn now_ns() -> u64 {
    clock(libc::CLOCK_MONOTONIC)
}
pub(crate) fn cpu_ns() -> u64 {
    clock(libc::CLOCK_PROCESS_CPUTIME_ID)
}
fn clock(id: libc::clockid_t) -> u64 {
    // SAFETY: timespec contains only integer fields, which admit zero.
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    // SAFETY: time points to writable storage for one timespec.
    if unsafe { libc::clock_gettime(id, &mut time) } != 0 {
        return 0;
    }
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}
pub(crate) fn fresh_user(path: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    match builder.create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    require(
        std::fs::read_dir(path)?.next().is_none(),
        "user directory must be fresh and empty",
    )?;
    let path = CString::new(path.as_os_str().as_encoded_bytes())?;
    // SAFETY: path is a live, NUL-terminated directory path.
    require(
        unsafe { libc::access(path.as_ptr(), libc::W_OK | libc::X_OK) } == 0,
        "user directory must be readable and writable",
    )
}
pub(crate) fn immediate_exit() -> ! {
    // SAFETY: Memory-stop workloads intentionally retain every live object.
    unsafe { libc::_exit(0) }
}

pub(crate) fn temp_root() -> std::path::PathBuf {
    std::env::var_os("TMPDIR")
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/tmp".into())
}
