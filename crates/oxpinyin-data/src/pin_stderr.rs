//! The pin's raw `fprintf(stderr, …)` lines, from one place (#545).
//!
//! The maintainer ruling of 2026-10-03 has oxpinyin write the pin's
//! diagnostics byte for byte. Every writer of such a line goes through
//! [`emit`], and every path in one is written as its bytes, which is what
//! `%s` does: `Path::display()` would turn invalid UTF-8 into U+FFFD. The
//! lines that are not tied to one crate's work live here as functions of
//! their own; the callers decide when a line is due, from the error they
//! hold, never from a message.

use std::io::Write as _;
use std::path::Path;

/// One diagnostic, as the pin's `fprintf(stderr, …)` writes it: the parts
/// concatenated and written once. A failed write is ignored, as `fprintf`'s
/// result is.
pub fn emit(parts: &[&[u8]]) {
    let line: Vec<u8> = parts.concat();
    let _ = std::io::stderr().write_all(&line);
}

/// A path's bytes as they are, which is what `%s` writes.
#[must_use]
pub fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

/// `g_build_filename(dir, name, NULL)` as bytes: one separator between the
/// two, the trailing ones of `dir` gone, and an empty `dir` leaving the bare
/// name.
#[must_use]
pub fn build_filename(dir: &Path, name: &str) -> Vec<u8> {
    let mut joined = path_bytes(dir).to_vec();
    while joined.last() == Some(&b'/') {
        joined.pop();
    }
    if !dir.as_os_str().is_empty() {
        joined.push(b'/');
    }
    joined.extend_from_slice(name.as_bytes());
    joined
}

/// The pin's own report of a `table.conf` it cannot open
/// (`table_info.cpp:201`, then `pinyin.cpp:338` and `zhuyin.cpp:282`): two
/// lines naming the file as `g_build_filename` spells it, and the init is
/// over. Writes them and answers `true` when `system_dir/table.conf` cannot
/// be opened; answers `false`, writing nothing, when it can.
#[must_use]
pub fn report_unopenable_table_conf(system_dir: &Path) -> bool {
    if std::fs::File::open(system_dir.join("table.conf")).is_ok() {
        return false;
    }
    let name = build_filename(system_dir, "table.conf");
    emit(&[b"open ", &name, b" failed.\nload ", &name, b" failed!\n"]);
    true
}

/// `mmap %s failed!\n`: `MemoryChunk::mmap` refused `path`
/// (`pinyin.cpp:256`, `:290`, `:956`, `:1265`; `zhuyin.cpp:200`, `:589`,
/// `:800`). The pin carries on with an empty chunk and dies reading it; the
/// line is all of its output.
pub fn mmap_failed(path: &Path) {
    emit(&[b"mmap ", path_bytes(path), b" failed!\n"]);
}

#[cfg(test)]
mod tests {
    use super::{build_filename, path_bytes};
    use std::path::Path;

    #[test]
    fn build_filename_joins_as_glib_does() {
        assert_eq!(build_filename(Path::new("/a/b"), "x.bin"), b"/a/b/x.bin");
        assert_eq!(build_filename(Path::new("/a/b//"), "x.bin"), b"/a/b/x.bin");
        assert_eq!(build_filename(Path::new(""), "x.bin"), b"x.bin");
        assert_eq!(build_filename(Path::new("/"), "x.bin"), b"/x.bin");
    }

    #[test]
    #[cfg(unix)]
    fn paths_are_written_as_their_bytes() {
        use std::os::unix::ffi::OsStrExt as _;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/d\xffir"));
        assert_eq!(path_bytes(path), b"/d\xffir");
    }
}
