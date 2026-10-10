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

/// Whether `byte` is a directory separator to `g_build_filename`: `/` on
/// Unix; `/` and `\` on Windows, where GLib accepts both.
const fn is_separator(byte: u8) -> bool {
    byte == std::path::MAIN_SEPARATOR as u8 || (cfg!(windows) && byte == b'/')
}

/// `g_build_filename(dir, name, NULL)` as bytes: one separator between the
/// two, the trailing ones of `dir` gone, and an empty `dir` leaving the bare
/// name. The separator is the platform's: `/` on Unix, the pin's bytes.
#[must_use]
pub fn build_filename(dir: &Path, name: &str) -> Vec<u8> {
    let mut joined = path_bytes(dir).to_vec();
    while joined.last().is_some_and(|&byte| is_separator(byte)) {
        joined.pop();
    }
    if !dir.as_os_str().is_empty() {
        joined.push(std::path::MAIN_SEPARATOR as u8);
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

/// The pin's report of a `table.conf` it opened but whose header does not
/// match (`SystemTableInfo2::load` answers `false` at `table_info.cpp:209-224`,
/// then `pinyin.cpp:338` / `zhuyin.cpp:282`): the single line `load %s
/// failed!` naming the file as `g_build_filename` spells it.
pub fn report_rejected_table_conf(system_dir: &Path) {
    let name = build_filename(system_dir, "table.conf");
    emit(&[b"load ", &name, b" failed!\n"]);
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
    use super::build_filename;
    use std::path::{MAIN_SEPARATOR_STR as SEP, Path};

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
    }

    #[test]
    #[cfg(unix)]
    fn paths_are_written_as_their_bytes() {
        use super::path_bytes;
        use std::os::unix::ffi::OsStrExt as _;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/d\xffir"));
        assert_eq!(path_bytes(path), b"/d\xffir");
    }
}
