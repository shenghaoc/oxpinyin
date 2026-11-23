#[cfg(unix)]
use std::ffi::OsString;

use super::*;

#[cfg(unix)]
fn under(dir: &str, name: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt as _;
    build_path(Path::new(dir), OsStr::from_bytes(name)).into_os_string()
}

// Frozen GLib 2.84.4 g_build_filename observations cover Unix separator and empty-part handling.
#[test]
#[cfg(unix)]
fn names_stay_beneath_their_directory() {
    for (dir, name, expected) in [
        ("/u", "//x//y/", "/u/x//y/"),
        ("/u", "../x", "/u/../x"),
        ("/u", "", "/u"),
        ("", "/tmp/x", "/tmp/x"),
        ("", "", ""),
        ("a///", "x", "a/x"),
        ("//a", "x", "//a/x"),
        ("//", "x", "//x"),
        ("/a//", "/", "/a/"),
        ("/", "/", "//"),
    ] {
        assert_eq!(under(dir, name.as_bytes()), expected);
    }
}

// Rust Windows posix_refusal preserves the Linux rename refusal for unlike file and directory types.
#[test]
fn rename_rejects_file_directory_type_changes() {
    use std::io::ErrorKind::{IsADirectory, NotADirectory};
    for (source_is_dir, expected) in [(true, NotADirectory), (false, IsADirectory)] {
        let scratch = tempfile::tempdir().unwrap();
        let staged = scratch.path().join("user_bigram.db.tmp");
        let final_path = scratch.path().join("user_bigram.db");
        let (directory, file) = if source_is_dir {
            (&staged, &final_path)
        } else {
            (&final_path, &staged)
        };
        std::fs::create_dir(directory).unwrap();
        std::fs::write(file, b"kept").unwrap();
        assert_eq!(rename(&staged, &final_path).unwrap_err().kind(), expected);
        assert!(directory.is_dir());
        assert_eq!(std::fs::read(file).unwrap(), b"kept");
    }
}

// Rust build_filename passes NULL elements to GLib, which stops at the first NULL.
#[test]
fn null_elements_are_glibs() {
    for (dir, name, expected) in [
        (None, Some(c"user.conf"), b"".as_slice()),
        (Some(c"/u"), None, b"/u".as_slice()),
    ] {
        assert_eq!(build_filename(dir, name).to_bytes(), expected);
    }
}

// 074a2219 pinyin.cpp:164–165,1059–1062 uses native rename status and ignores unlink status.
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

// Rust path conversion preserves Unix bytes and well-formed UTF-8 Windows paths.
#[test]
fn paths_round_trip() {
    let path = Path::new("/u/name");
    assert_eq!(path_from_bytes(path_to_bytes(path)), path.to_path_buf());
}

// Rust Windows filename decoding keeps NULL and empty names and replaces invalid UTF-8.
#[cfg(windows)]
#[test]
fn filenames_keep_null_empty_and_lossy_utf8_handling() {
    for (input, expected) in [
        (None, None),
        (Some(c""), Some(PathBuf::new())),
        (Some(c"\xff"), Some(PathBuf::from("\u{fffd}"))),
    ] {
        assert_eq!(filename(input), expected);
    }
}
