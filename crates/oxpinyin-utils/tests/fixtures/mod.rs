//! Shared backend w3 and save-refusal fixtures.
use assert_fs::{TempDir, prelude::*};
use rstest::fixture;
use std::path::Path;

/// Copy the compiled backend's w3 into a private temporary directory.
#[fixture]
pub fn w3() -> TempDir {
    let directory = TempDir::new().expect("scratch directory");
    directory
        .copy_from(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/w3")
                .join(oxpinyin_data::DEFAULT_STORE_EXT),
            &["**"],
        )
        .expect("backend w3 fixture");
    directory
}

/// Keep the original Linux sealed-memory input (also unwritable by root).
/// Other Unix hosts retain the original read-only-file setup.
#[fixture]
pub fn sealed_w3() -> (TempDir, std::fs::File) {
    let directory = w3();
    let path = directory.child("gb_char.bin");
    #[cfg(target_os = "linux")]
    {
        use std::io::Write as _;
        use std::os::fd::{AsRawFd as _, FromRawFd as _};

        const NAME: &std::ffi::CStr = c"tool-case-chunk";
        #[allow(unsafe_code)]
        // SAFETY: NAME is NUL-terminated; this File owns the new descriptor.
        let mut memory = unsafe {
            let fd = libc::memfd_create(NAME.as_ptr(), libc::MFD_ALLOW_SEALING);
            assert!(fd >= 0, "{}", std::io::Error::last_os_error());
            std::fs::File::from_raw_fd(fd)
        };
        memory
            .write_all(&std::fs::read(path.path()).expect("original chunk"))
            .expect("memory chunk");
        let fd = memory.as_raw_fd();
        #[allow(unsafe_code)]
        // SAFETY: fd remains open while memory lives; neither call touches memory.
        let failed = unsafe {
            libc::fcntl(
                fd,
                libc::F_ADD_SEALS,
                libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE,
            ) < 0
                || libc::fcntl(fd, libc::F_SETFD, 0) < 0
        };
        assert!(!failed, "{}", std::io::Error::last_os_error());
        std::fs::remove_file(path.path()).expect("replace chunk");
        std::os::unix::fs::symlink(format!("/proc/self/fd/{fd}"), path.path())
            .expect("sealed chunk");
        (directory, memory)
    }
    #[cfg(not(target_os = "linux"))]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path.path(), std::fs::Permissions::from_mode(0o444))
            .expect("read-only chunk");
        let held = std::fs::File::open(path.path()).expect("chunk");
        (directory, held)
    }
}
