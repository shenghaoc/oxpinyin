//! The pin's library `fprintf(stderr, …)` lines, made with the pin's own call
//! through the `oxpinyin-core` boundary (`docs/findings/external-io-boundary.md`,
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
    crate::input::path::path_to_bytes(path)
}

/// The init's `load %s failed!` for a `table.conf` that
/// `SystemTableInfo2::load` refused (074a2219 `pinyin.cpp:338`,
/// `zhuyin.cpp:282`), naming `system_dir/table.conf` as `g_build_filename`
/// spells it; `None` for a NULL system dir, whose name is `""`.
pub fn table_conf_load_failed(system_dir: Option<&Path>) {
    let name = crate::input::path::build_bytes(system_dir, Some("table.conf"));
    crate::input::diagnostic::report_load_failed(&c_prefix(&name));
}

/// `mmap %s failed!`: `MemoryChunk::mmap` refused `path`. Every caller the
/// pin has writes it: the init's library loop and the library load calls
/// (`pinyin.cpp:256`, `:290`; `zhuyin.cpp:200`, `:234`), the save's
/// `_write_files` (`pinyin.cpp:956`, `zhuyin.cpp:589`) and `mask_out`
/// (`pinyin.cpp:1265`, `zhuyin.cpp:800`). The pin carries on with an empty
/// chunk and dies reading it; the line is all of its output (policy row 76).
pub fn mmap_failed(path: &Path) {
    crate::input::diagnostic::report_mmap_failed(&c_prefix(path_bytes(path)));
}

/// A save's failed rename of `tmp` to `final_path`, as the pin's
/// `_rename_files` (`pinyin.cpp:1061-1123`) or `zhuyin_save`
/// (`zhuyin.cpp:611-689`) writes it.
pub(crate) fn rename_failed(tmp: &Path, final_path: &Path) {
    crate::input::diagnostic::report_rename_failed(
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
mod tests;
