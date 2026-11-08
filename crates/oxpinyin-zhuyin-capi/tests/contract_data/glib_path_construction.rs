//! GLib path construction: empty, NULL and repeated, leading, nested or dotted separators in the directory arguments and table.conf names (#643, #922). Generated from the frozen pin observations.

use crate::bodies::glib_path_construction as b;
use crate::support::{Expect, run_j};

// glib-922-dir-system-dup-trailing-zhuyin: MATCH.
#[test]
fn glib_922_dir_system_dup_trailing_zhuyin() {
    run_j(
        "glib_path_construction::glib_922_dir_system_dup_trailing_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-922-dir-system-dup-trailing-zhuyin"),
        raw!(include!(
            "fixtures/glib_path_construction/35cfd198aa8d0ffa.rs"
        )),
        None,
    );
}

// glib-922-dir-user-dup-trailing-zhuyin: MATCH.
#[test]
fn glib_922_dir_user_dup_trailing_zhuyin() {
    run_j(
        "glib_path_construction::glib_922_dir_user_dup_trailing_zhuyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X/user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-922-dir-user-dup-trailing-zhuyin"),
        raw!(include!(
            "fixtures/glib_path_construction/35cfd198aa8d0ffa.rs"
        )),
        None,
    );
}

// glib-922-row-leading-slash-zhuyin: MATCH.
#[test]
fn glib_922_row_leading_slash_zhuyin() {
    run_j(
        "glib_path_construction::glib_922_row_leading_slash_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-922-row-leading-slash-zhuyin"),
        raw!(M(&[
            ("guess", B(true)),
            ("init", B(true)),
            ("logs", L(&[])),
            ("parse", I(6))
        ])),
        None,
    );
}

// glib-922-row-missing-leading-slash-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn glib_922_row_missing_leading_slash_zhuyin() {
    run_j(
        "glib_path_construction::glib_922_row_missing_leading_slash_zhuyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.mmap SCRATCH\\sys-X/merged.bin failed!\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/merged.bin failed!\n"
        }),
        |k| b::case(k, "glib-922-row-missing-leading-slash-zhuyin"),
        raw!(M(&[("ctx", B(true)), ("logs", L(&[]))])),
        Some(&["ctx", "logs"]),
    );
}

// glib-922-row-missing-nested-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn glib_922_row_missing_nested_zhuyin() {
    run_j(
        "glib_path_construction::glib_922_row_missing_nested_zhuyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.mmap SCRATCH\\sys-X/sub//merged.bin failed!\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/sub//merged.bin failed!\n"
        }),
        |k| b::case(k, "glib-922-row-missing-nested-zhuyin"),
        raw!(M(&[("ctx", B(true)), ("logs", L(&[]))])),
        Some(&["ctx", "logs"]),
    );
}

// glib-922-save-write-failed-dup-trailing-zhuyin: MATCH.
#[test]
fn glib_922_save_write_failed_dup_trailing_zhuyin() {
    run_j(
        "glib_path_construction::glib_922_save_write_failed_dup_trailing_zhuyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X/user.conf failed.rename SCRATCH\\user-X/gb_char.dbin.tmp to SCRATCH\\user-X/gb_char.dbin failed.\r\nrename SCRATCH\\user-X/gbk_char.dbin.tmp to SCRATCH\\user-X/gbk_char.dbin failed.\r\nrename SCRATCH\\user-X/opengram.dbin.tmp to SCRATCH\\user-X/opengram.dbin failed.\r\nrename SCRATCH\\user-X/merged.dbin.tmp to SCRATCH\\user-X/merged.dbin failed.\r\nrename SCRATCH\\user-X/addon.bin.tmp to SCRATCH\\user-X/addon.bin failed.\r\nrename SCRATCH\\user-X/network.bin.tmp to SCRATCH\\user-X/network.bin failed.\r\nrename SCRATCH\\user-X/user.bin.tmp to SCRATCH\\user-X/user.bin failed.\r\nrename SCRATCH\\user-X/user_pinyin_index.bin.tmp to SCRATCH\\user-X/user_pinyin_index.bin failed.\r\nrename SCRATCH\\user-X/user_phrase_index.bin.tmp to SCRATCH\\user-X/user_phrase_index.bin failed.\r\nrename SCRATCH\\user-X/user_bigram.db.tmp to SCRATCH\\user-X/user_bigram.db failed.\r\nwrite SCRATCH\\user-X/user.conf failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/gb_char.dbin.tmp to SCRATCH/user-X/gb_char.dbin failed.\nrename SCRATCH/user-X/gbk_char.dbin.tmp to SCRATCH/user-X/gbk_char.dbin failed.\nrename SCRATCH/user-X/opengram.dbin.tmp to SCRATCH/user-X/opengram.dbin failed.\nrename SCRATCH/user-X/merged.dbin.tmp to SCRATCH/user-X/merged.dbin failed.\nrename SCRATCH/user-X/addon.bin.tmp to SCRATCH/user-X/addon.bin failed.\nrename SCRATCH/user-X/network.bin.tmp to SCRATCH/user-X/network.bin failed.\nrename SCRATCH/user-X/user.bin.tmp to SCRATCH/user-X/user.bin failed.\nrename SCRATCH/user-X/user_pinyin_index.bin.tmp to SCRATCH/user-X/user_pinyin_index.bin failed.\nrename SCRATCH/user-X/user_phrase_index.bin.tmp to SCRATCH/user-X/user_phrase_index.bin failed.\nrename SCRATCH/user-X/user_bigram.db.tmp to SCRATCH/user-X/user_bigram.db failed.\nwrite SCRATCH/user-X/user.conf failed.\n"
        }),
        |k| b::case(k, "glib-922-save-write-failed-dup-trailing-zhuyin"),
        raw!(M(&[
            ("init", B(true)),
            ("logs", L(&[])),
            ("save", B(true)),
            ("train", B(true))
        ])),
        None,
    );
}

// init-system-empty-zhuyin: MATCH.
#[test]
fn init_system_empty_zhuyin() {
    run_j(
        "glib_path_construction::init_system_empty_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "init-system-empty-zhuyin"),
        raw!(M(&[
            ("files", L(&[])),
            ("guess", B(true)),
            ("init", B(true)),
            ("logs", L(&[])),
            ("parse", I(6))
        ])),
        None,
    );
}

// init-system-null-zhuyin: MATCH.
#[test]
fn init_system_null_zhuyin() {
    run_j(
        "glib_path_construction::init_system_null_zhuyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open  failed.\r\nload  failed!\r\n"
        } else {
            b"open  failed.\nload  failed!\n"
        }),
        |k| b::case(k, "init-system-null-zhuyin"),
        raw!(M(&[
            ("files", L(&[])),
            ("init", B(false)),
            ("logs", L(&[]))
        ])),
        None,
    );
}
