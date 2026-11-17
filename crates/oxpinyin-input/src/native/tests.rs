use super::*;
use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT};

fn inheritable(fd: c_int) -> bool {
    // SAFETY: fd is an open CRT descriptor owned by the caller.
    let handle = unsafe { libc::get_osfhandle(fd) };
    let mut flags = 0_u32;
    // SAFETY: the handle belongs to the open descriptor and flags is a
    // live out-parameter.
    let ok = unsafe { GetHandleInformation(handle as _, &mut flags) };
    assert_ne!(ok, 0, "GetHandleInformation failed");
    flags & HANDLE_FLAG_INHERIT != 0
}

#[test]
fn fopen_stream_is_not_inheritable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fopen.bin");
    std::fs::write(&path, b"x").unwrap();
    let stream = fopen(path.as_path(), c"r").unwrap();
    // SAFETY: stream is open and owned here.
    let fd = unsafe { libc::fileno(stream.as_ptr()) };
    let result = inheritable(fd);
    // SAFETY: stream is open and closed once.
    unsafe { libc::fclose(stream.as_ptr()) };
    assert!(!result, "fopen stream handle is inheritable");
}

#[test]
fn wopen_descriptor_is_not_inheritable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wopen.bin");
    std::fs::write(&path, b"x").unwrap();
    let chunk = ReadChunk::open(&path).unwrap();
    assert!(!inheritable(chunk.0), "_wopen handle is inheritable");
}
