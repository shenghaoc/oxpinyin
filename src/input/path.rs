//! Builds filenames, converts path encodings, and renames or unlinks files.
//! [`path_from_bytes`] preserves Unix path bytes and replaces malformed UTF-8 on Windows.
//! [`path_from_bytes_checked`] requires representable bytes. [`crate::input::c_path`]
//! rejects an interior NUL. [`c_string`] stops at it.

// The native calls are g_build_filename, g_rename and unlink. On Unix g_rename maps to
// rename; the Windows implementation calls MoveFileExW.

use std::ffi::{CStr, CString, OsStr};
use std::path::{Path, PathBuf};

use crate::input::native;

/// Decode a filename from an internal C string or the C-ABI facade.
/// Unix keeps its bytes. Windows uses UTF-8 and replaces invalid input
/// with U+FFFD. The C string already ends at its first NUL.
#[must_use]
pub fn decode_filename(name: &CStr) -> PathBuf {
    path_from_bytes(name.to_bytes())
}

/// Decode a nullable C filename at either C API boundary.
#[must_use]
pub fn filename(input: Option<&CStr>) -> Option<PathBuf> {
    input.map(decode_filename)
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
pub(crate) fn build_chunk_path(dir: &Path, name: &OsStr) -> PathBuf {
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
pub(crate) fn build_bytes(dir: Option<&Path>, name: Option<&str>) -> Vec<u8> {
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

/// The Windows adaptation of save rename preserves the Linux pin's `rename(2)`
/// refusal for a directory over an existing file (`ENOTDIR`) and a file over a directory
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
mod tests;
