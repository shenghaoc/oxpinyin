use super::*;

// 074a2219 pinyin.cpp:338 writes the filename with %s.
#[test]
fn a_name_is_written_up_to_its_first_nul() {
    for (name, prefix) in [("/a/b", b"/a/b".as_slice()), ("/a\0b", b"/a")] {
        assert_eq!(
            crate::input::c_string(std::ffi::OsStr::new(name)).as_bytes(),
            prefix
        );
    }
}

// Rust path_bytes contract preserves Unix filename bytes.
#[test]
#[cfg(unix)]
fn paths_are_written_as_their_bytes() {
    use std::os::unix::ffi::OsStrExt as _;
    let path = Path::new(std::ffi::OsStr::from_bytes(b"/d\xffir"));
    assert_eq!(path_bytes(path), b"/d\xffir");
}
