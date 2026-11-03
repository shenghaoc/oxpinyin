//! The pin's path construction, rename and unlink calls (external-I/O
//! stage 3).
//!
//! Every path the pin builds with `g_build_filename` is built here, through
//! GLib itself (074a2219 `pinyin.cpp:150-165,250-259,331-338,395-400,952-1121`;
//! `zhuyin.cpp:104-119,196-203,276-282,337-342,585-687`). The builder takes
//! the pin's C strings: a `None` directory is its `NULL` first element,
//! which makes GLib return `""`, and a `None` name is its `NULL`
//! terminator, which makes the directory itself the result. No Rust
//! `Path::join` stands in for it.
//!
//! `g_rename` is a macro to `rename` on Unix and a function around
//! `MoveFileExW` on Windows; `unlink` is the plain C call. The safe
//! functions below make those calls through `crate::native`.

use std::ffi::{CStr, CString, OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::native;

/// `g_build_filename(dir, name, NULL)` with the pin's arguments.
///
/// `None` passes the C `NULL` pointer. GLib stops at a `NULL` first
/// element and returns an empty string; a `NULL` later element is the
/// terminator, so the elements before it remain. The result is owned by the
/// caller.
#[must_use]
pub fn build_filename(dir: Option<&CStr>, name: Option<&CStr>) -> CString {
    native::build_filename(dir, name)
}

/// `g_build_filename(dir, name, NULL)` as the path Rust opens.
///
/// The bytes are the pin's C-string bytes. On Unix they become the
/// `OsString` unchanged; elsewhere GLib returns UTF-8 and the path takes
/// that text.
#[must_use]
pub fn build_path(dir: &Path, name: &OsStr) -> PathBuf {
    let joined = build_filename(Some(&c_string(dir.as_os_str())), Some(&c_string(name)));
    path_from_c_string(joined)
}

/// Build a system/addon chunk path, retaining Windows `OsStr` bytes through
/// GLib's separator handling without a lossy UTF-8 conversion.
#[must_use]
pub fn build_chunk_path(dir: &Path, name: &OsStr) -> PathBuf {
    #[cfg(not(windows))]
    {
        build_path(dir, name)
    }
    #[cfg(windows)]
    {
        let joined = build_filename(Some(&c_string(dir.as_os_str())), Some(&c_string(name)));
        PathBuf::from(native::chunk_path_from_encoded(joined.into_bytes()))
    }
}

/// `g_build_filename(dir, name, NULL)` as the bytes `fprintf("%s")`
/// writes. `None` is the pin's `NULL` element.
#[must_use]
pub fn build_bytes(dir: Option<&Path>, name: Option<&str>) -> Vec<u8> {
    let dir = dir.map(|dir| c_string(dir.as_os_str()));
    let name = name.map(|name| c_string(OsStr::new(name)));
    build_filename(dir.as_deref(), name.as_deref()).into_bytes()
}

/// `path` as a C string, cut at a NUL as the pin's `%s` input would be.
#[must_use]
pub fn c_string(path: &OsStr) -> CString {
    let bytes = path.as_encoded_bytes();
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    CString::new(&bytes[..end]).unwrap_or_default()
}

/// `g_rename(from, to)`: `rename(2)` on Unix, `MoveFileExW` through GLib
/// on Windows. The pin tests `0 != result`; the failure's `errno` is kept
/// for the strict save, and the reporting save only asks whether it failed.
///
/// # Errors
///
/// The rename's `errno` when it answers a non-zero result.
pub fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    if native::rename(&c_string(from.as_os_str()), &c_string(to.as_os_str())) {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// `unlink(path)`, with the pin's ignored result (`_clean_user_files` and
/// `_write_files` never examine it).
pub fn unlink(path: &Path) {
    native::unlink(&c_string(path.as_os_str()));
}

#[cfg(unix)]
fn path_from_c_string(joined: CString) -> PathBuf {
    use std::os::unix::ffi::OsStringExt as _;
    PathBuf::from(OsString::from_vec(joined.into_bytes()))
}

#[cfg(not(unix))]
fn path_from_c_string(joined: CString) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&joined.into_bytes()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn under(dir: &str, name: &[u8]) -> OsString {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            build_path(Path::new(dir), OsStr::from_bytes(name)).into_os_string()
        }
        #[cfg(not(unix))]
        {
            build_path(Path::new(dir), OsStr::new(&String::from_utf8_lossy(name))).into_os_string()
        }
    }

    /// `g_build_filename(dir, name, NULL)`: the separators, empty parts and
    /// separator-only parts GLib 2.84.4's `g_build_path_va` produces.
    #[test]
    #[cfg(unix)]
    fn names_stay_beneath_their_directory() {
        assert_eq!(under("/u", b"/tmp/x"), "/u/tmp/x");
        assert_eq!(under("/u", b"//x//y/"), "/u/x//y/");
        assert_eq!(under("/u", b"../x"), "/u/../x");
        assert_eq!(under("/u", b""), "/u");
        assert_eq!(under("", b"/tmp/x"), "/tmp/x");
        assert_eq!(under("", b""), "");
        assert_eq!(under("a/", b"x"), "a/x");
        assert_eq!(under("a///", b"x"), "a/x");
        assert_eq!(under("//a", b"x"), "//a/x");
        assert_eq!(under("/", b"x"), "/x");
        assert_eq!(under("//", b"x"), "//x");
        assert_eq!(under("/a", b"//"), "/a//");
        assert_eq!(under("/a//", b"/"), "/a/");
        assert_eq!(under("/", b"/"), "//");
        assert_eq!(under("/u", b"./x"), "/u/./x");
        assert_eq!(under("/u\\", b"\\x"), "/u\\/\\x");
    }

    /// The pin's `NULL` first element answers `""`, and a `NULL` name the
    /// directory, because that is what GLib does.
    #[test]
    fn null_elements_are_glibs() {
        assert_eq!(build_filename(None, Some(c"user.conf")).to_bytes(), b"");
        assert_eq!(build_filename(Some(c"/u"), None).to_bytes(), b"/u");
        assert_eq!(build_filename(None, None).to_bytes(), b"");
    }

    #[test]
    fn bytes_are_the_pins() {
        assert_eq!(build_bytes(Some(Path::new("/a//b")), Some("x")), b"/a//b/x");
        assert_eq!(build_bytes(None, Some("x")), b"");
    }

    #[test]
    fn rename_and_unlink_are_the_pins() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a");
        let to = dir.path().join("b");
        std::fs::write(&from, b"x").unwrap();
        rename(&from, &to).unwrap();
        assert!(!from.exists() && to.exists());
        unlink(&to);
        assert!(!to.exists());
        unlink(&to);
    }
}
