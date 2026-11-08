//! Save failures and the pin's stderr: blocked, removed and unwritable user files, interrupted saves and saved-profile reloads (#525, #545, #929). Generated from the frozen pin observations.

use crate::bodies::save_failures_stderr as b;
use crate::support::{Expect, run_j};
// Only the Unix-only abort cases list user files and abort.
#[cfg(unix)]
use crate::support::{UserFile, run_aborting};

// abort-zhuyin-save-chunk-header-checksum: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
// Unix only: the save is cut by a four-byte RLIMIT_FSIZE, a Unix resource limit.
#[cfg(unix)]
#[test]
fn abort_zhuyin_save_chunk_header_checksum() {
    run_aborting(
        "save_failures_stderr::abort_zhuyin_save_chunk_header_checksum",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[(
            "gb_char.dbin.tmp",
            UserFile::Chunk(
                4,
                "7d8e29fa389a36cca29bc0f07a7892dddd6f9070b9e33d12dce8ce3569f81810",
            ),
        )]),
        |k| {
            b::case(k, "abort-zhuyin-save-chunk-header-checksum");
        },
    );
}

// abort-zhuyin-save-chunk-header-length: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
// Linux only: the save writes through /dev/full, which macOS and Windows lack.
#[cfg(target_os = "linux")]
#[test]
fn abort_zhuyin_save_chunk_header_length() {
    run_aborting(
        "save_failures_stderr::abort_zhuyin_save_chunk_header_length",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[
            (
                "addon.bin",
                UserFile::Chunk(
                    27,
                    "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697",
                ),
            ),
            (
                "gb_char.dbin",
                UserFile::Chunk(
                    26,
                    "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3",
                ),
            ),
            (
                "gbk_char.dbin",
                UserFile::Chunk(
                    26,
                    "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153",
                ),
            ),
            (
                "merged.dbin",
                UserFile::Chunk(
                    26,
                    "8ac7679f1c49c436e73595048a3d412f68357800a18444cecc5cf9977b345b74",
                ),
            ),
            (
                "network.bin",
                UserFile::Chunk(
                    27,
                    "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697",
                ),
            ),
            (
                "opengram.dbin",
                UserFile::Chunk(
                    26,
                    "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc",
                ),
            ),
            ("user.bin.tmp", UserFile::NotFile),
        ]),
        |k| {
            b::case(k, "abort-zhuyin-save-chunk-header-length");
        },
    );
}

// save-929-logical-cross-open-zhuyin: MATCH.
#[test]
fn save_929_logical_cross_open_zhuyin() {
    #[allow(unused_imports)]
    use crate::support::Raw::*;
    static PROFILE: crate::support::Raw =
        include!("fixtures/save_failures_stderr/profile-fd8e7ef6de8c7b8d.rs");
    run_j(
        "save_failures_stderr::save_929_logical_cross_open_zhuyin",
        Expect::RESULT,
        |k| b::case_with_profile(k, "save-929-logical-cross-open-zhuyin", &PROFILE),
        raw!(include!(
            "fixtures/save_failures_stderr/38e2d26184821bbd.rs"
        )),
        None,
    );
}

// save-929-reload-zhuyin: MATCH.
#[test]
fn save_929_reload_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    let expected = raw!(include!(
        "fixtures/save_failures_stderr/581b28d907a9dff9.rs"
    ));
    #[cfg(not(windows))]
    let expected = raw!(include!(
        "fixtures/save_failures_stderr/dfbd0182be2bb8c0.rs"
    ));
    run_j(
        "save_failures_stderr::save_929_reload_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "save-929-reload-zhuyin"),
        expected,
        None,
    );
}

// save-929-rename-user.bin-zhuyin: MATCH.
#[test]
fn save_929_rename_user_dot_bin_zhuyin() {
    run_j(
        "save_failures_stderr::save_929_rename_user_dot_bin_zhuyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user.bin.tmp to SCRATCH\\user-X\\user.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user.bin.tmp to SCRATCH/user-X/user.bin failed.\n"
        }),
        |k| b::case(k, "save-929-rename-user.bin-zhuyin"),
        raw!(include!(
            "fixtures/save_failures_stderr/1744b3c574dd4910.rs"
        )),
        None,
    );
}

// save-929-rename-user_bigram.db-zhuyin: MATCH.
#[test]
fn save_929_rename_user_bigram_dot_db_zhuyin() {
    run_j(
        "save_failures_stderr::save_929_rename_user_bigram_dot_db_zhuyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_bigram.db.tmp to SCRATCH\\user-X\\user_bigram.db failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_bigram.db.tmp to SCRATCH/user-X/user_bigram.db failed.\n"
        }),
        |k| b::case(k, "save-929-rename-user_bigram.db-zhuyin"),
        raw!(include!(
            "fixtures/save_failures_stderr/45c3638cef759ba8.rs"
        )),
        None,
    );
}

// save-929-rename-user_bigram.db.tmp-zhuyin: MATCH.
#[test]
fn save_929_rename_user_bigram_dot_db_dot_tmp_zhuyin() {
    run_j(
        "save_failures_stderr::save_929_rename_user_bigram_dot_db_dot_tmp_zhuyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_bigram.db.tmp to SCRATCH\\user-X\\user_bigram.db failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_bigram.db.tmp to SCRATCH/user-X/user_bigram.db failed.\n"
        }),
        |k| b::case(k, "save-929-rename-user_bigram.db.tmp-zhuyin"),
        raw!(include!(
            "fixtures/save_failures_stderr/089a568c7421a45b.rs"
        )),
        None,
    );
}

// save-929-rename-user_phrase_index.bin-zhuyin: MATCH.
#[test]
fn save_929_rename_user_phrase_index_dot_bin_zhuyin() {
    run_j(
        "save_failures_stderr::save_929_rename_user_phrase_index_dot_bin_zhuyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_phrase_index.bin.tmp to SCRATCH\\user-X\\user_phrase_index.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_phrase_index.bin.tmp to SCRATCH/user-X/user_phrase_index.bin failed.\n"
        }),
        |k| b::case(k, "save-929-rename-user_phrase_index.bin-zhuyin"),
        raw!(include!(
            "fixtures/save_failures_stderr/33a2cf050f15e81c.rs"
        )),
        None,
    );
}

// save-929-rename-user_pinyin_index.bin-zhuyin: MATCH.
#[test]
fn save_929_rename_user_pinyin_index_dot_bin_zhuyin() {
    run_j(
        "save_failures_stderr::save_929_rename_user_pinyin_index_dot_bin_zhuyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_pinyin_index.bin.tmp to SCRATCH\\user-X\\user_pinyin_index.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_pinyin_index.bin.tmp to SCRATCH/user-X/user_pinyin_index.bin failed.\n"
        }),
        |k| b::case(k, "save-929-rename-user_pinyin_index.bin-zhuyin"),
        raw!(include!(
            "fixtures/save_failures_stderr/9c453ddcbbe5ac65.rs"
        )),
        None,
    );
}

// save-929-stale-bigram-tmp-zhuyin: MATCH.
#[test]
fn save_929_stale_bigram_tmp_zhuyin() {
    run_j(
        "save_failures_stderr::save_929_stale_bigram_tmp_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "save-929-stale-bigram-tmp-zhuyin"),
        raw!(M(&[
            ("first", B(true)),
            ("logs", L(&[])),
            ("rows", L(&[])),
            ("second", B(true)),
            ("tmp_exists", B(false))
        ])),
        None,
    );
}

// stderr-fini-dir-removed-zhuyin: MATCH.
#[test]
fn stderr_fini_dir_removed_zhuyin() {
    run_j(
        "save_failures_stderr::stderr_fini_dir_removed_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "stderr-fini-dir-removed-zhuyin"),
        raw!(M(&[("logs", L(&[]))])),
        None,
    );
}

// stderr-fresh-user-dir-zhuyin: MATCH.
#[test]
fn stderr_fresh_user_dir_zhuyin() {
    run_j(
        "save_failures_stderr::stderr_fresh_user_dir_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "stderr-fresh-user-dir-zhuyin"),
        raw!(M(&[("init", B(true)), ("logs", L(&[]))])),
        None,
    );
}

// stderr-save-dir-removed-zhuyin: MATCH.
#[test]
fn stderr_save_dir_removed_zhuyin() {
    run_j(
        "save_failures_stderr::stderr_save_dir_removed_zhuyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\gb_char.dbin.tmp to SCRATCH\\user-X\\gb_char.dbin failed.\r\nrename SCRATCH\\user-X\\gbk_char.dbin.tmp to SCRATCH\\user-X\\gbk_char.dbin failed.\r\nrename SCRATCH\\user-X\\opengram.dbin.tmp to SCRATCH\\user-X\\opengram.dbin failed.\r\nrename SCRATCH\\user-X\\merged.dbin.tmp to SCRATCH\\user-X\\merged.dbin failed.\r\nrename SCRATCH\\user-X\\addon.bin.tmp to SCRATCH\\user-X\\addon.bin failed.\r\nrename SCRATCH\\user-X\\network.bin.tmp to SCRATCH\\user-X\\network.bin failed.\r\nrename SCRATCH\\user-X\\user.bin.tmp to SCRATCH\\user-X\\user.bin failed.\r\nrename SCRATCH\\user-X\\user_pinyin_index.bin.tmp to SCRATCH\\user-X\\user_pinyin_index.bin failed.\r\nrename SCRATCH\\user-X\\user_phrase_index.bin.tmp to SCRATCH\\user-X\\user_phrase_index.bin failed.\r\nrename SCRATCH\\user-X\\user_bigram.db.tmp to SCRATCH\\user-X\\user_bigram.db failed.\r\nwrite SCRATCH\\user-X\\user.conf failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/gb_char.dbin.tmp to SCRATCH/user-X/gb_char.dbin failed.\nrename SCRATCH/user-X/gbk_char.dbin.tmp to SCRATCH/user-X/gbk_char.dbin failed.\nrename SCRATCH/user-X/opengram.dbin.tmp to SCRATCH/user-X/opengram.dbin failed.\nrename SCRATCH/user-X/merged.dbin.tmp to SCRATCH/user-X/merged.dbin failed.\nrename SCRATCH/user-X/addon.bin.tmp to SCRATCH/user-X/addon.bin failed.\nrename SCRATCH/user-X/network.bin.tmp to SCRATCH/user-X/network.bin failed.\nrename SCRATCH/user-X/user.bin.tmp to SCRATCH/user-X/user.bin failed.\nrename SCRATCH/user-X/user_pinyin_index.bin.tmp to SCRATCH/user-X/user_pinyin_index.bin failed.\nrename SCRATCH/user-X/user_phrase_index.bin.tmp to SCRATCH/user-X/user_phrase_index.bin failed.\nrename SCRATCH/user-X/user_bigram.db.tmp to SCRATCH/user-X/user_bigram.db failed.\nwrite SCRATCH/user-X/user.conf failed.\n"
        }),
        |k| b::case(k, "stderr-save-dir-removed-zhuyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("save", B(true)),
            ("train", B(true))
        ])),
        None,
    );
}

// stderr-save-one-tmp-blocked-zhuyin: MATCH.
#[test]
fn stderr_save_one_tmp_blocked_zhuyin() {
    run_j(
        "save_failures_stderr::stderr_save_one_tmp_blocked_zhuyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_pinyin_index.bin.tmp to SCRATCH\\user-X\\user_pinyin_index.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_pinyin_index.bin.tmp to SCRATCH/user-X/user_pinyin_index.bin failed.\n"
        }),
        |k| b::case(k, "stderr-save-one-tmp-blocked-zhuyin"),
        raw!(include!(
            "fixtures/save_failures_stderr/6ffd738a25abaac4.rs"
        )),
        None,
    );
}

// save-929-boundary-save-marker-zhuyin: MATCH.
// Linux only: the boundary is found through a read lease on user.conf (F_SETLEASE) that the save child, this test binary run again for the case, breaks.
#[cfg(target_os = "linux")]
#[test]
fn save_929_boundary_save_marker_zhuyin() {
    run_j(
        "save_failures_stderr::save_929_boundary_save_marker_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "save-929-boundary-save-marker-zhuyin"),
        raw!(include!(
            "fixtures/save_failures_stderr/7b151710f9e6beb2.rs"
        )),
        None,
    );
}

// save-929-boundary-after-save-zhuyin: MATCH.
// Linux only: the boundary is found through a read lease on user.conf (F_SETLEASE) that the save child, this test binary run again for the case, breaks.
#[cfg(target_os = "linux")]
#[test]
fn save_929_boundary_after_save_zhuyin() {
    run_j(
        "save_failures_stderr::save_929_boundary_after_save_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "save-929-boundary-after-save-zhuyin"),
        raw!(include!(
            "fixtures/save_failures_stderr/555bb5687b0d4b45.rs"
        )),
        None,
    );
}

// save-929-boundary-after-fini-zhuyin: MATCH.
// Linux only: the boundary is found through a read lease on user.conf (F_SETLEASE) that the save child, this test binary run again for the case, breaks.
#[cfg(target_os = "linux")]
#[test]
fn save_929_boundary_after_fini_zhuyin() {
    run_j(
        "save_failures_stderr::save_929_boundary_after_fini_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "save-929-boundary-after-fini-zhuyin"),
        raw!(include!(
            "fixtures/save_failures_stderr/555bb5687b0d4b45.rs"
        )),
        None,
    );
}
