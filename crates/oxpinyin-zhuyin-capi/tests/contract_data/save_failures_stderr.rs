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
            "../../../oxpinyin-capi/tests/contract_data/fixtures/save_failures_stderr/38e2d26184821bbd.rs"
        )),
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
            "../../../oxpinyin-capi/tests/contract_data/fixtures/save_failures_stderr/1744b3c574dd4910.rs"
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
            "../../../oxpinyin-capi/tests/contract_data/fixtures/save_failures_stderr/6ffd738a25abaac4.rs"
        )),
        None,
    );
}
