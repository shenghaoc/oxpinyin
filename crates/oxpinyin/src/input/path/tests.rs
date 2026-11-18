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

#[cfg(windows)]
#[test]
fn filenames_keep_null_empty_and_lossy_utf8_handling() {
    assert_eq!(filename(None), None);
    assert_eq!(filename(Some(c"")), Some(PathBuf::new()));
    let input = c"\xff";
    assert_eq!(filename(Some(input)), Some(PathBuf::from("\u{fffd}")));
}
