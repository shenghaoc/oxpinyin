//! Emits library diagnostics on the C standard error stream. [`path_bytes`] supplies the
//! path bytes used by the filename diagnostic.

// The diagnostics use the pin's fprintf(stderr, …) call through crate::input.

use std::path::Path;

/// A path's bytes as they are, which is what `%s` writes.
#[must_use]
pub fn path_bytes(path: &Path) -> &[u8] {
    crate::input::path_to_bytes(path)
}

/// The init's `load %s failed!` for a `table.conf` that
/// `SystemTableInfo2::load` refused (074a2219 `pinyin.cpp:338`,
/// `zhuyin.cpp:282`), naming `system_dir/table.conf` as `g_build_filename`
/// spells it; `None` for a NULL system dir, whose name is `""`.
pub fn table_conf_load_failed(system_dir: Option<&Path>) {
    crate::input::diagnostic::report_load_failed(&crate::input::build_c_string(
        system_dir,
        Some("table.conf"),
    ));
}

/// `mmap %s failed!`: `MemoryChunk::mmap` refused `path`. Every caller the
/// pin has writes it: the init's library loop and the library load calls
/// (`pinyin.cpp:256`, `:290`; `zhuyin.cpp:200`, `:234`), the save's
/// `_write_files` (`pinyin.cpp:956`, `zhuyin.cpp:589`) and `mask_out`
/// (`pinyin.cpp:1265`, `zhuyin.cpp:800`). The pin carries on with an empty
/// chunk and dies reading it; the line is all of its output.
pub(crate) fn mmap_failed(path: &Path) {
    crate::input::diagnostic::report_mmap_failed(&crate::input::c_string(path.as_os_str()));
}

/// A save's failed rename of `tmp` to `final_path`, as the pin's
/// `_rename_files` (`pinyin.cpp:1061-1123`) or `zhuyin_save`
/// (`zhuyin.cpp:611-689`) writes it.
pub(crate) fn rename_failed(tmp: &Path, final_path: &Path) {
    crate::input::diagnostic::report_rename_failed(
        &crate::input::c_string(tmp.as_os_str()),
        &crate::input::c_string(final_path.as_os_str()),
    );
}

#[cfg(test)]
mod tests;
