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

use std::ffi::{CStr, CString, OsStr};
use std::path::{Path, PathBuf};

use crate::native;

/// Decode a filename from an internal C string or the C-ABI facade.
/// Unix keeps its bytes. Windows uses UTF-8 and replaces invalid input
/// with U+FFFD. The C string already ends at its first NUL.
#[must_use]
pub fn decode_filename(name: &CStr) -> PathBuf {
    path_from_bytes(name.to_bytes())
}

/// A pin byte path as the Rust path that opens it.
///
/// Unix keeps the bytes. Windows decodes UTF-8 and replaces invalid input
/// with U+FFFD, because its C paths are UTF-8. NUL handling belongs to the
/// caller, as it does at the C-string boundary.
#[must_use]
pub fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        PathBuf::from(OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// A pin byte path as the Rust path that opens it, `None` where the
/// platform cannot represent the bytes.
///
/// Unix accepts every byte. Windows requires UTF-8, as the pin's tools do
/// for a `CStr` filename; invalid input names no path. "Checked" refers to
/// the encoding alone: NULs, existence and filename validity are not
/// examined.
#[must_use]
pub fn path_from_bytes_checked(bytes: &[u8]) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        Some(path_from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(bytes).ok().map(PathBuf::from)
    }
}

/// A Rust path as the pin's byte path.
///
/// Unix keeps the path's bytes. Windows uses the encoded `OsStr` bytes,
/// which are WTF-8; they are UTF-8 only when the path is well-formed
/// UTF-16. A NUL is kept, so the caller can refuse it where a C `%s` would
/// end the name.
#[must_use]
pub fn path_to_bytes(path: &Path) -> &[u8] {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        path.as_os_str().as_bytes()
    }
    #[cfg(not(unix))]
    {
        path.as_os_str().as_encoded_bytes()
    }
}

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
    let bytes = path_to_bytes(Path::new(path));
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
/// The rename's `errno` when it answers a non-zero result; on Windows also
/// the two refusals of `posix_refusal`, a directory over an existing file
/// (`ENOTDIR`) and a file over an existing directory (`EISDIR`).
pub fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    posix_refusal(from, to)?;
    if native::rename(&c_string(from.as_os_str()), &c_string(to.as_os_str())) {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// The Windows platform adaptation of the save rename (compatibility-policy.md,
/// maintainer ruling of 2026-11-03 UTC): the Linux pin's `rename(2)` refuses a
/// directory over an existing file (`ENOTDIR`) and a file over a directory
/// (`EISDIR`), while `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`, which
/// GLib's `g_rename` calls, moves a `.tmp` directory onto the final file. Both
/// paths are examined with the native facility (`symlink_metadata`, which does
/// not follow links) and the refusal is a rename failure, so the caller prints
/// `rename %s to %s failed.` as it does on Linux. A missing source or
/// destination is left to `g_rename`.
#[cfg(windows)]
fn posix_refusal(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::io::ErrorKind::{IsADirectory, NotADirectory};
    let (Ok(source), Ok(destination)) = (
        std::fs::symlink_metadata(from),
        std::fs::symlink_metadata(to),
    ) else {
        return Ok(());
    };
    if source.is_dir() && !destination.is_dir() {
        return Err(std::io::Error::new(
            NotADirectory,
            "rename: a directory over an existing file",
        ));
    }
    if !source.is_dir() && destination.is_dir() {
        return Err(std::io::Error::new(
            IsADirectory,
            "rename: a file over an existing directory",
        ));
    }
    Ok(())
}

/// `unlink(path)`, with the pin's ignored result (`_clean_user_files` and
/// `_write_files` never examine it).
pub fn unlink(path: &Path) {
    #[cfg(not(windows))]
    native::unlink(&c_string(path.as_os_str()));
    #[cfg(windows)]
    native::unlink(path);
}

#[cfg(unix)]
fn path_from_c_string(joined: CString) -> PathBuf {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;
    PathBuf::from(OsString::from_vec(joined.into_bytes()))
}

#[cfg(windows)]
fn path_from_c_string(joined: CString) -> PathBuf {
    PathBuf::from(native::chunk_path_from_encoded(joined.into_bytes()))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::ffi::OsString;

    use super::*;

    #[cfg(unix)]
    fn under(dir: &str, name: &[u8]) -> OsString {
        use std::os::unix::ffi::OsStrExt as _;
        build_path(Path::new(dir), OsStr::from_bytes(name)).into_os_string()
    }

    /// `g_build_filename(dir, name, NULL)`: the separators, empty parts and
    /// separator-only parts GLib 2.84.4's `g_build_path_va` produces.
    // Unix only: byte-string names, and `/` as the separator GLib inserts.
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

    /// `rename(2)` refuses a directory over an existing file with `ENOTDIR`
    /// (POSIX); on Windows `rename` gives the same refusal before `g_rename`,
    /// whose `MoveFileExW` would move the directory onto the file.
    #[test]
    fn a_directory_is_not_renamed_over_a_file() {
        let scratch = tempfile::tempdir().expect("scratch");
        let staged = scratch.path().join("user_bigram.db.tmp");
        let final_path = scratch.path().join("user_bigram.db");
        std::fs::create_dir(&staged).expect("the staged directory");
        std::fs::write(&final_path, b"kept").expect("the final file");
        let error = rename(&staged, &final_path).expect_err("a directory over a file");
        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory, "{error}");
        assert!(staged.is_dir(), "the directory stays");
        assert_eq!(std::fs::read(&final_path).expect("the final file"), b"kept");
    }

    /// `rename(2)` refuses a file over an existing directory with `EISDIR`
    /// (POSIX); on Windows `rename` gives the same refusal before `g_rename`.
    #[test]
    fn a_file_is_not_renamed_over_a_directory() {
        let scratch = tempfile::tempdir().expect("scratch");
        let staged = scratch.path().join("user_bigram.db.tmp");
        let final_path = scratch.path().join("user_bigram.db");
        std::fs::write(&staged, b"staged").expect("the staged file");
        std::fs::create_dir(&final_path).expect("the final directory");
        let error = rename(&staged, &final_path).expect_err("a file over a directory");
        assert_eq!(error.kind(), std::io::ErrorKind::IsADirectory, "{error}");
        assert_eq!(std::fs::read(&staged).expect("the staged file"), b"staged");
        assert!(final_path.is_dir(), "the directory stays");
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

    /// The pair round-trips Unix bytes, and on Windows round-trips only
    /// UTF-8 paths. A path with an unpaired surrogate is representable but
    /// lossy through `path_from_bytes`. This is not a lossless Windows path
    /// codec.
    #[test]
    fn paths_round_trip_and_the_checked_form_follows_the_platform() {
        let path = Path::new("/u/name");
        assert_eq!(path_from_bytes(path_to_bytes(path)), path.to_path_buf());
        let bytes = b"/u/m\xff";
        #[cfg(unix)]
        assert_eq!(path_from_bytes_checked(bytes), Some(path_from_bytes(bytes)));
        #[cfg(not(unix))]
        assert_eq!(path_from_bytes_checked(bytes), None);
    }
}
