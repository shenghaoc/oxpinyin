//! The pin's `fprintf(stderr, …)` lines that share a failure sequence with a
//! line `oxpinyin-input` writes, made with the pin's own call through that
//! shim (`docs/findings/external-io-boundary.md`, stage 1).
//!
//! Stage 1 moved `table_info.cpp`'s `open %s failed.` and `write %s failed.`
//! lines to the shim (ruling D2). The other lines of the same init or save are
//! written here, so one call's lines go to one C stream in the pin's order:
//! the init's `load %s failed!` and library `mmap %s failed!` lines after
//! `check_format`'s, and the save's `mmap %s failed!` and `rename %s to %s
//! failed.` lines before `mark_version`'s. The lines of calls the shim writes
//! nothing in stay with [`crate::pin_stderr`] until stage 4.

use std::ffi::CString;
use std::path::Path;

/// The init's `load %s failed!` for a `table.conf` that
/// `SystemTableInfo2::load` refused (074a2219 `pinyin.cpp:338`,
/// `zhuyin.cpp:282`), naming `system_dir/table.conf` as `g_build_filename`
/// spells it; `None` for a NULL system dir, whose name is `""`.
pub fn table_conf_load_failed(system_dir: Option<&Path>) {
    let name = system_dir.map_or_else(Vec::new, |dir| {
        crate::pin_stderr::build_filename(dir, "table.conf")
    });
    oxpinyin_input::report_load_failed(&c_prefix(&name));
}

/// `mmap %s failed!` for a library the init's library loop
/// (`pinyin.cpp:256`, `:290`; `zhuyin.cpp:200`, `:234`) or the save's
/// `_write_files` (`pinyin.cpp:956`, `zhuyin.cpp:589`) cannot map.
pub fn mmap_failed(path: &Path) {
    oxpinyin_input::report_mmap_failed(&c_prefix(crate::pin_stderr::path_bytes(path)));
}

/// A save's failed rename of `tmp` to `final_path`, as the pin's
/// `_rename_files` (`pinyin.cpp:1061-1123`) or `zhuyin_save`
/// (`zhuyin.cpp:611-689`) writes it.
pub fn rename_failed(tmp: &Path, final_path: &Path) {
    oxpinyin_input::report_rename_failed(
        &c_prefix(crate::pin_stderr::path_bytes(tmp)),
        &c_prefix(crate::pin_stderr::path_bytes(final_path)),
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

    #[test]
    fn a_name_is_written_up_to_its_first_nul() {
        assert_eq!(c_prefix(b"/a/b"), CString::new("/a/b").unwrap());
        assert_eq!(c_prefix(b"/a\0b"), CString::new("/a").unwrap());
        assert_eq!(c_prefix(b""), CString::default());
    }
}
