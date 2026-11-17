use super::*;

#[test]
fn a_name_is_written_up_to_its_first_nul() {
    assert_eq!(c_prefix(b"/a/b"), CString::new("/a/b").unwrap());
    assert_eq!(c_prefix(b"/a\0b"), CString::new("/a").unwrap());
    assert_eq!(c_prefix(b""), CString::default());
}

// Unix only: byte-string paths (`OsStrExt::from_bytes`).
#[test]
#[cfg(unix)]
fn paths_are_written_as_their_bytes() {
    use std::os::unix::ffi::OsStrExt as _;
    let path = Path::new(std::ffi::OsStr::from_bytes(b"/d\xffir"));
    assert_eq!(path_bytes(path), b"/d\xffir");
}
