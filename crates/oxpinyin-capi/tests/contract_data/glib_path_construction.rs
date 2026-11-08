//! GLib path construction: empty, NULL and repeated, leading, nested or dotted separators in the directory arguments and table.conf names (#643, #922). Generated from the frozen pin observations.

use crate::bodies::glib_path_construction as b;
use crate::support::{Expect, run_j};

// glib-922-dir-both-dot-segments-pinyin: MATCH.
#[test]
fn glib_922_dir_both_dot_segments_pinyin() {
    run_j(
        "glib_path_construction::glib_922_dir_both_dot_segments_pinyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X/./user.conf failed."
        } else {
            b"open SCRATCH/user-X/./user.conf failed."
        }),
        |k| b::case(k, "glib-922-dir-both-dot-segments-pinyin"),
        raw!(include!(
            "fixtures/glib_path_construction/35cfd198aa8d0ffa.rs"
        )),
        None,
    );
}

// glib-922-dir-both-dotdot-pinyin: MATCH.
#[test]
fn glib_922_dir_both_dotdot_pinyin() {
    run_j(
        "glib_path_construction::glib_922_dir_both_dotdot_pinyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator starts as \ and changes only while an argument's leading or trailing separators are stripped, so the /../ inside the user directory argument leaves it at \; the argument's own separators stay.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X/../user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/../user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-922-dir-both-dotdot-pinyin"),
        raw!(include!(
            "fixtures/glib_path_construction/35cfd198aa8d0ffa.rs"
        )),
        None,
    );
}

// glib-922-dir-both-leading-dup-pinyin: MATCH.
// Unix only: both directories are spelled with a leading / as POSIX absolute paths.
#[cfg(unix)]
#[test]
fn glib_922_dir_both_leading_dup_pinyin() {
    run_j(
        "glib_path_construction::glib_922_dir_both_leading_dup_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open /SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open /SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-922-dir-both-leading-dup-pinyin"),
        raw!(include!(
            "fixtures/glib_path_construction/35cfd198aa8d0ffa.rs"
        )),
        None,
    );
}

// glib-922-dir-system-dup-trailing-pinyin: MATCH.
#[test]
fn glib_922_dir_system_dup_trailing_pinyin() {
    run_j(
        "glib_path_construction::glib_922_dir_system_dup_trailing_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-922-dir-system-dup-trailing-pinyin"),
        raw!(include!(
            "fixtures/glib_path_construction/35cfd198aa8d0ffa.rs"
        )),
        None,
    );
}

// glib-922-dir-user-dup-trailing-pinyin: MATCH.
#[test]
fn glib_922_dir_user_dup_trailing_pinyin() {
    run_j(
        "glib_path_construction::glib_922_dir_user_dup_trailing_pinyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X/user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-922-dir-user-dup-trailing-pinyin"),
        raw!(include!(
            "fixtures/glib_path_construction/35cfd198aa8d0ffa.rs"
        )),
        None,
    );
}

// glib-922-fini-write-failed-dup-trailing-pinyin: MATCH.
#[test]
fn glib_922_fini_write_failed_dup_trailing_pinyin() {
    run_j(
        "glib_path_construction::glib_922_fini_write_failed_dup_trailing_pinyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.write SCRATCH\\user-X/user.conf failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.write SCRATCH/user-X/user.conf failed.\n"
        }),
        |k| b::case(k, "glib-922-fini-write-failed-dup-trailing-pinyin"),
        raw!(M(&[("init", B(true)), ("logs", L(&[]))])),
        None,
    );
}

// glib-922-row-double-leading-slash-pinyin: MATCH.
#[test]
fn glib_922_row_double_leading_slash_pinyin() {
    run_j(
        "glib_path_construction::glib_922_row_double_leading_slash_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-922-row-double-leading-slash-pinyin"),
        raw!(M(&[
            ("guess", B(true)),
            ("init", B(true)),
            ("logs", L(&[])),
            ("parse", I(5))
        ])),
        None,
    );
}

// glib-922-row-leading-slash-pinyin: MATCH.
#[test]
fn glib_922_row_leading_slash_pinyin() {
    run_j(
        "glib_path_construction::glib_922_row_leading_slash_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-922-row-leading-slash-pinyin"),
        raw!(M(&[
            ("guess", B(true)),
            ("init", B(true)),
            ("logs", L(&[])),
            ("parse", I(5))
        ])),
        None,
    );
}

// glib-922-row-missing-dup-trailing-dir-pinyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn glib_922_row_missing_dup_trailing_dir_pinyin() {
    run_j(
        "glib_path_construction::glib_922_row_missing_dup_trailing_dir_pinyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.mmap SCRATCH\\sys-X/merged.bin failed!\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/merged.bin failed!\n"
        }),
        |k| b::case(k, "glib-922-row-missing-dup-trailing-dir-pinyin"),
        raw!(M(&[("ctx", B(true)), ("logs", L(&[]))])),
        Some(&["ctx", "logs"]),
    );
}

// glib-922-row-missing-leading-slash-pinyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn glib_922_row_missing_leading_slash_pinyin() {
    run_j(
        "glib_path_construction::glib_922_row_missing_leading_slash_pinyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.mmap SCRATCH\\sys-X/merged.bin failed!\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/merged.bin failed!\n"
        }),
        |k| b::case(k, "glib-922-row-missing-leading-slash-pinyin"),
        raw!(M(&[("ctx", B(true)), ("logs", L(&[]))])),
        Some(&["ctx", "logs"]),
    );
}

// glib-922-row-missing-nested-pinyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn glib_922_row_missing_nested_pinyin() {
    run_j(
        "glib_path_construction::glib_922_row_missing_nested_pinyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.mmap SCRATCH\\sys-X/sub//merged.bin failed!\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/sub//merged.bin failed!\n"
        }),
        |k| b::case(k, "glib-922-row-missing-nested-pinyin"),
        raw!(M(&[("ctx", B(true)), ("logs", L(&[]))])),
        Some(&["ctx", "logs"]),
    );
}

// glib-922-row-nested-pinyin: MATCH.
#[test]
fn glib_922_row_nested_pinyin() {
    run_j(
        "glib_path_construction::glib_922_row_nested_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-922-row-nested-pinyin"),
        raw!(M(&[
            ("guess", B(true)),
            ("init", B(true)),
            ("logs", L(&[])),
            ("parse", I(5))
        ])),
        None,
    );
}

// glib-922-save-write-failed-dup-trailing-pinyin: MATCH.
#[test]
fn glib_922_save_write_failed_dup_trailing_pinyin() {
    run_j(
        "glib_path_construction::glib_922_save_write_failed_dup_trailing_pinyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X/user.conf failed.rename SCRATCH\\user-X/gb_char.dbin.tmp to SCRATCH\\user-X/gb_char.dbin failed.\r\nrename SCRATCH\\user-X/gbk_char.dbin.tmp to SCRATCH\\user-X/gbk_char.dbin failed.\r\nrename SCRATCH\\user-X/opengram.dbin.tmp to SCRATCH\\user-X/opengram.dbin failed.\r\nrename SCRATCH\\user-X/merged.dbin.tmp to SCRATCH\\user-X/merged.dbin failed.\r\nrename SCRATCH\\user-X/addon.bin.tmp to SCRATCH\\user-X/addon.bin failed.\r\nrename SCRATCH\\user-X/network.bin.tmp to SCRATCH\\user-X/network.bin failed.\r\nrename SCRATCH\\user-X/user.bin.tmp to SCRATCH\\user-X/user.bin failed.\r\nrename SCRATCH\\user-X/user_pinyin_index.bin.tmp to SCRATCH\\user-X/user_pinyin_index.bin failed.\r\nrename SCRATCH\\user-X/user_phrase_index.bin.tmp to SCRATCH\\user-X/user_phrase_index.bin failed.\r\nrename SCRATCH\\user-X/user_bigram.db.tmp to SCRATCH\\user-X/user_bigram.db failed.\r\nwrite SCRATCH\\user-X/user.conf failed.\r\nwrite SCRATCH\\user-X/user.conf failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/gb_char.dbin.tmp to SCRATCH/user-X/gb_char.dbin failed.\nrename SCRATCH/user-X/gbk_char.dbin.tmp to SCRATCH/user-X/gbk_char.dbin failed.\nrename SCRATCH/user-X/opengram.dbin.tmp to SCRATCH/user-X/opengram.dbin failed.\nrename SCRATCH/user-X/merged.dbin.tmp to SCRATCH/user-X/merged.dbin failed.\nrename SCRATCH/user-X/addon.bin.tmp to SCRATCH/user-X/addon.bin failed.\nrename SCRATCH/user-X/network.bin.tmp to SCRATCH/user-X/network.bin failed.\nrename SCRATCH/user-X/user.bin.tmp to SCRATCH/user-X/user.bin failed.\nrename SCRATCH/user-X/user_pinyin_index.bin.tmp to SCRATCH/user-X/user_pinyin_index.bin failed.\nrename SCRATCH/user-X/user_phrase_index.bin.tmp to SCRATCH/user-X/user_phrase_index.bin failed.\nrename SCRATCH/user-X/user_bigram.db.tmp to SCRATCH/user-X/user_bigram.db failed.\nwrite SCRATCH/user-X/user.conf failed.\nwrite SCRATCH/user-X/user.conf failed.\n"
        }),
        |k| b::case(k, "glib-922-save-write-failed-dup-trailing-pinyin"),
        raw!(M(&[
            ("init", B(true)),
            ("logs", L(&[])),
            ("save", B(true)),
            ("train", B(true))
        ])),
        None,
    );
}

// init-system-absolute-pinyin: MATCH.
#[test]
fn init_system_absolute_pinyin() {
    run_j(
        "glib_path_construction::init_system_absolute_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "init-system-absolute-pinyin"),
        raw!(M(&[
            ("files", L(&[S("user.conf")])),
            ("guess", B(true)),
            ("init", B(true)),
            ("logs", L(&[])),
            ("parse", I(7))
        ])),
        None,
    );
}

// init-system-empty-pinyin: MATCH.
#[test]
fn init_system_empty_pinyin() {
    run_j(
        "glib_path_construction::init_system_empty_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "init-system-empty-pinyin"),
        raw!(M(&[
            ("files", L(&[S("user.conf")])),
            ("guess", B(true)),
            ("init", B(true)),
            ("logs", L(&[])),
            ("parse", I(7))
        ])),
        None,
    );
}

// init-system-null-pinyin: MATCH.
#[test]
fn init_system_null_pinyin() {
    run_j(
        "glib_path_construction::init_system_null_pinyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open  failed.\r\nload  failed!\r\n"
        } else {
            b"open  failed.\nload  failed!\n"
        }),
        |k| b::case(k, "init-system-null-pinyin"),
        raw!(M(&[
            ("files", L(&[])),
            ("init", B(false)),
            ("logs", L(&[]))
        ])),
        None,
    );
}
