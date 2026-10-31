//! The pin's library `fprintf(stderr, …)` lines, made with the pin's own call
//! through the `oxpinyin-input` boundary (`docs/findings/external-io-boundary.md`,
//! stages 1 and 4).
//!
//! `table_info.cpp`'s `open %s failed.` and `write %s failed.` lines are the
//! boundary's `config` readers' and writer's own (ruling D2). The library's other
//! lines are written here: `load %s failed!`, `mmap %s failed!` and `rename %s
//! to %s failed.`. Every line therefore goes to the host C library's `stderr`
//! stream in the order the callers make them. The maintainer ruling of
//! 2026-10-03 (#545) has oxpinyin write them byte for byte; every path is
//! passed as its bytes, which is what `%s` reads: `Path::display()` would turn
//! invalid UTF-8 into U+FFFD. The callers decide when a line is due, from the
//! error they hold, never from a message.

use std::ffi::CString;
use std::path::Path;

/// A path's bytes as they are, which is what `%s` writes.
#[must_use]
pub fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

/// `g_build_filename(dir, name, NULL)` as bytes, which is what `%s` writes
/// for the path the pin opened: the same join
/// [`pin_join`](crate::table_info::pin_join) opens.
#[must_use]
pub fn build_filename(dir: &Path, name: &str) -> Vec<u8> {
    crate::table_info::join_bytes(path_bytes(dir), name.as_bytes())
}

/// The init's `load %s failed!` for a `table.conf` that
/// `SystemTableInfo2::load` refused (074a2219 `pinyin.cpp:338`,
/// `zhuyin.cpp:282`), naming `system_dir/table.conf` as `g_build_filename`
/// spells it; `None` for a NULL system dir, whose name is `""`.
pub fn table_conf_load_failed(system_dir: Option<&Path>) {
    let name = system_dir.map_or_else(Vec::new, |dir| build_filename(dir, "table.conf"));
    oxpinyin_input::diagnostic::report_load_failed(&c_prefix(&name));
}

/// `mmap %s failed!`: `MemoryChunk::mmap` refused `path`. Every caller the
/// pin has writes it: the init's library loop and the library load calls
/// (`pinyin.cpp:256`, `:290`; `zhuyin.cpp:200`, `:234`), the save's
/// `_write_files` (`pinyin.cpp:956`, `zhuyin.cpp:589`) and `mask_out`
/// (`pinyin.cpp:1265`, `zhuyin.cpp:800`). The pin carries on with an empty
/// chunk and dies reading it; the line is all of its output (policy row 76).
pub fn mmap_failed(path: &Path) {
    oxpinyin_input::diagnostic::report_mmap_failed(&c_prefix(path_bytes(path)));
}

/// A save's failed rename of `tmp` to `final_path`, as the pin's
/// `_rename_files` (`pinyin.cpp:1061-1123`) or `zhuyin_save`
/// (`zhuyin.cpp:611-689`) writes it.
pub fn rename_failed(tmp: &Path, final_path: &Path) {
    oxpinyin_input::diagnostic::report_rename_failed(
        &c_prefix(path_bytes(tmp)),
        &c_prefix(path_bytes(final_path)),
    );
}

/// `bytes` up to the first NUL, as `%s` reads them; no C caller's path holds
/// one.
fn c_prefix(bytes: &[u8]) -> CString {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    CString::new(&bytes[..end]).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::MAIN_SEPARATOR_STR as SEP;

    #[test]
    fn a_name_is_written_up_to_its_first_nul() {
        assert_eq!(c_prefix(b"/a/b"), CString::new("/a/b").unwrap());
        assert_eq!(c_prefix(b"/a\0b"), CString::new("/a").unwrap());
        assert_eq!(c_prefix(b""), CString::default());
    }

    #[test]
    fn build_filename_joins_as_glib_does() {
        // Written with `/` and spelled with the platform's separator, so
        // Unix checks the pin's bytes and Windows its own.
        let join = |dir: &str| build_filename(Path::new(&dir.replace('/', SEP)), "x.bin");
        let want = |text: &str| text.replace('/', SEP).into_bytes();
        assert_eq!(join("/a/b"), want("/a/b/x.bin"));
        assert_eq!(join("/a/b//"), want("/a/b/x.bin"));
        assert_eq!(join(""), b"x.bin");
        assert_eq!(join("/"), want("/x.bin"));
        assert_eq!(join("/a/b///"), want("/a/b/x.bin"));
        // Leading separators are the directory's own: `//` is kept whole.
        assert_eq!(join("//"), want("//x.bin"));
        assert_eq!(join("//a"), want("//a/x.bin"));
    }

    #[test]
    #[cfg(unix)]
    fn paths_are_written_as_their_bytes() {
        use std::os::unix::ffi::OsStrExt as _;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/d\xffir"));
        assert_eq!(path_bytes(path), b"/d\xffir");
    }
}
