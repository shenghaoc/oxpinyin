//! GLib path construction: empty, NULL and repeated, leading, nested or dotted separators in the directory arguments and table.conf names (#643, #922). Generated from the frozen pin observations.

use crate::bodies::glib_path_construction as b;
use crate::support::{Expect, run_j};

// glib-922-table-conf-missing-slashes-zhuyin: MATCH.
#[test]
fn glib_922_table_conf_missing_slashes_zhuyin() {
    run_j(
        "glib_path_construction::glib_922_table_conf_missing_slashes_zhuyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open //table.conf failed.\r\nload //table.conf failed!\r\n"
        } else {
            b"open //table.conf failed.\nload //table.conf failed!\n"
        }),
        |k| b::case(k, "glib-922-table-conf-missing-slashes-zhuyin"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// glib-922-table-conf-rejected-dup-trailing-zhuyin: MATCH.
#[test]
fn glib_922_table_conf_rejected_dup_trailing_zhuyin() {
    run_j(
        "glib_path_construction::glib_922_table_conf_rejected_dup_trailing_zhuyin",
        // g_build_filename on Windows (gfileutils.c, g_build_pathname_va, GLib 2.90.1): the inserted separator is the one last seen at an argument's edge, / from the test's spelling of the directory or the table.conf name, and separators inside an argument stay; the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"load SCRATCH\\sys-X/table.conf failed!\r\n"
        } else {
            b"load SCRATCH/sys-X/table.conf failed!\n"
        }),
        |k| b::case(k, "glib-922-table-conf-rejected-dup-trailing-zhuyin"),
        raw!(M(&[("ctx", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// init-system-empty-missing-zhuyin: MATCH.
#[test]
fn init_system_empty_missing_zhuyin() {
    run_j(
        "glib_path_construction::init_system_empty_missing_zhuyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open table.conf failed.\r\nload table.conf failed!\r\n"
        } else {
            b"open table.conf failed.\nload table.conf failed!\n"
        }),
        |k| b::case(k, "init-system-empty-missing-zhuyin"),
        raw!(M(&[
            ("files", L(&[])),
            ("init", B(false)),
            ("logs", L(&[]))
        ])),
        None,
    );
}
