//! The pin's raw `fprintf(stderr, …)` lines, from one place (#545).
//!
//! The maintainer ruling of 2026-10-03 has oxpinyin write the pin's
//! diagnostics byte for byte. Every writer of such a line goes through
//! [`emit`], and every path in one is written as its bytes, which is what
//! `%s` does: `Path::display()` would turn invalid UTF-8 into U+FFFD. The
//! lines that are not tied to one crate's work live here as functions of
//! their own; the callers decide when a line is due, from the error they
//! hold, never from a message.
//!
//! These lines go to Rust's stderr. A line that shares a failure sequence
//! with one the `oxpinyin-input` shim writes is [`crate::c_stderr`]'s
//! instead; the rest move to the pin's calls at stage 4 of
//! `docs/findings/external-io-boundary.md`.

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
pub(crate) const fn is_separator(byte: u8) -> bool {
    byte == std::path::MAIN_SEPARATOR as u8 || (cfg!(windows) && byte == b'/')
}

/// `g_build_filename(dir, name, NULL)` as bytes, which is what `%s` writes
/// for the path the pin opened: the same join [`pin_join`](crate::table_info::pin_join) opens.
#[must_use]
pub fn build_filename(dir: &Path, name: &str) -> Vec<u8> {
    crate::table_info::join_bytes(path_bytes(dir), name.as_bytes())
}

/// `mmap %s failed!\n`: `MemoryChunk::mmap` refused `path` in a call the
/// shim writes nothing in: `mask_out` (`pinyin.cpp:1265`, `zhuyin.cpp:800`)
/// and the library load calls (`pinyin.cpp:256`, `:290` through `:446` and
/// `:477`). The pin carries on with an empty chunk and dies reading it; the
/// line is all of its output. The init's and the save's are
/// [`crate::c_stderr::mmap_failed`].
pub fn mmap_failed(path: &Path) {
    emit(&[&mmap_failed_line(path)]);
}

/// The bytes [`mmap_failed`] writes for `path`.
#[must_use]
pub fn mmap_failed_line(path: &Path) -> Vec<u8> {
    [b"mmap ", path_bytes(path), b" failed!\n"].concat()
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
        assert_eq!(join("/a/b///"), want("/a/b/x.bin"));
        // Leading separators are the directory's own: `//` is kept whole.
        assert_eq!(join("//"), want("//x.bin"));
        assert_eq!(join("//a"), want("//a/x.bin"));
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
