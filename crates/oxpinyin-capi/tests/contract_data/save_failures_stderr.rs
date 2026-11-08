//! Save failures and the pin's stderr: blocked, removed and unwritable user files, interrupted saves and saved-profile reloads (#525, #545, #929). Generated from the frozen pin observations.

use crate::bodies::save_failures_stderr as b;
use crate::support::{Expect, run_j};
// Only the Unix-only abort cases list user files and abort.
#[cfg(unix)]
use crate::support::{UserFile, run_aborting};

// abort-save-chunk-header-checksum: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
// Unix only: the save is cut by a four-byte RLIMIT_FSIZE, a Unix resource limit.
#[cfg(unix)]
#[test]
fn abort_save_chunk_header_checksum() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "save_failures_stderr::abort_save_chunk_header_checksum",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[
            (
                "gb_char.dbin.tmp",
                UserFile::Chunk(
                    4,
                    "7d8e29fa389a36cca29bc0f07a7892dddd6f9070b9e33d12dce8ce3569f81810",
                ),
            ),
            ("user.conf", UserFile::Conf(USER_CONF)),
        ]),
        |k| {
            b::case(k, "abort-save-chunk-header-checksum");
        },
    );
}

// abort-save-chunk-header-length: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
// Linux only: the save writes through /dev/full, which macOS and Windows lack.
#[cfg(target_os = "linux")]
#[test]
fn abort_save_chunk_header_length() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "save_failures_stderr::abort_save_chunk_header_length",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[
            (
                "addon.bin.tmp",
                UserFile::Chunk(
                    27,
                    "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697",
                ),
            ),
            (
                "gb_char.dbin.tmp",
                UserFile::Chunk(
                    26,
                    "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3",
                ),
            ),
            (
                "gbk_char.dbin.tmp",
                UserFile::Chunk(
                    26,
                    "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153",
                ),
            ),
            (
                "merged.dbin.tmp",
                UserFile::Chunk(
                    26,
                    "8ac7679f1c49c436e73595048a3d412f68357800a18444cecc5cf9977b345b74",
                ),
            ),
            (
                "network.bin.tmp",
                UserFile::Chunk(
                    27,
                    "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697",
                ),
            ),
            (
                "opengram.dbin.tmp",
                UserFile::Chunk(
                    26,
                    "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc",
                ),
            ),
            ("user.bin.tmp", UserFile::NotFile),
            ("user.conf", UserFile::Conf(USER_CONF)),
        ]),
        |k| {
            b::case(k, "abort-save-chunk-header-length");
        },
    );
}

// save-929-logical-cross-open-pinyin: MATCH.
#[test]
fn save_929_logical_cross_open_pinyin() {
    #[allow(unused_imports)]
    use crate::support::Raw::*;
    static PROFILE: crate::support::Raw =
        include!("fixtures/save_failures_stderr/profile-f500cc2efa82383c.rs");
    run_j(
        "save_failures_stderr::save_929_logical_cross_open_pinyin",
        Expect::RESULT,
        |k| b::case_with_profile(k, "save-929-logical-cross-open-pinyin", &PROFILE),
        raw!(include!(
            "fixtures/save_failures_stderr/38e2d26184821bbd.rs"
        )),
        None,
    );
}
// save-929-rename-user.bin-pinyin: MATCH.
#[test]
fn save_929_rename_user_dot_bin_pinyin() {
    run_j(
        "save_failures_stderr::save_929_rename_user_dot_bin_pinyin",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user.bin.tmp to SCRATCH\\user-X\\user.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user.bin.tmp to SCRATCH/user-X/user.bin failed.\n"
        }),
        |k| b::case(k, "save-929-rename-user.bin-pinyin"),
        raw!(include!(
            "fixtures/save_failures_stderr/1744b3c574dd4910.rs"
        )),
        None,
    );
}
// save-929-stale-bigram-tmp-pinyin: MATCH.
#[test]
fn save_929_stale_bigram_tmp_pinyin() {
    run_j(
        "save_failures_stderr::save_929_stale_bigram_tmp_pinyin",
        Expect::RESULT,
        |k| b::case(k, "save-929-stale-bigram-tmp-pinyin"),
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
// stderr-fini-dir-removed: MATCH.
#[test]
fn stderr_fini_dir_removed() {
    run_j(
        "save_failures_stderr::stderr_fini_dir_removed",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.write SCRATCH\\user-X\\user.conf failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.write SCRATCH/user-X/user.conf failed.\n"
        }),
        |k| b::case(k, "stderr-fini-dir-removed"),
        raw!(M(&[("logs", L(&[]))])),
        None,
    );
}
// stderr-non-utf8-user-dir: MATCH.
// Linux only: the user directory has a non-UTF-8 name, which APFS and Windows refuse.
#[cfg(target_os = "linux")]
#[test]
fn stderr_non_utf8_user_dir() {
    run_j(
        "save_failures_stderr::stderr_non_utf8_user_dir",
        Expect::stderr(b"open SCRATCH/user-\xffX/user.conf failed.rename SCRATCH/user-\xffX/gb_char.dbin.tmp to SCRATCH/user-\xffX/gb_char.dbin failed.\nrename SCRATCH/user-\xffX/gbk_char.dbin.tmp to SCRATCH/user-\xffX/gbk_char.dbin failed.\nrename SCRATCH/user-\xffX/opengram.dbin.tmp to SCRATCH/user-\xffX/opengram.dbin failed.\nrename SCRATCH/user-\xffX/merged.dbin.tmp to SCRATCH/user-\xffX/merged.dbin failed.\nrename SCRATCH/user-\xffX/addon.bin.tmp to SCRATCH/user-\xffX/addon.bin failed.\nrename SCRATCH/user-\xffX/network.bin.tmp to SCRATCH/user-\xffX/network.bin failed.\nrename SCRATCH/user-\xffX/user.bin.tmp to SCRATCH/user-\xffX/user.bin failed.\nrename SCRATCH/user-\xffX/user_pinyin_index.bin.tmp to SCRATCH/user-\xffX/user_pinyin_index.bin failed.\nrename SCRATCH/user-\xffX/user_phrase_index.bin.tmp to SCRATCH/user-\xffX/user_phrase_index.bin failed.\nrename SCRATCH/user-\xffX/user_bigram.db.tmp to SCRATCH/user-\xffX/user_bigram.db failed.\nwrite SCRATCH/user-\xffX/user.conf failed.\nwrite SCRATCH/user-\xffX/user.conf failed.\n"),
        |k| b::case(k, "stderr-non-utf8-user-dir"),
        raw!(M(&[("logs", L(&[])), ("save", B(true)), ("train", B(true))])),
        None,
    );
}
// stderr-save-dir-removed: MATCH.
#[test]
fn stderr_save_dir_removed() {
    run_j(
        "save_failures_stderr::stderr_save_dir_removed",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\gb_char.dbin.tmp to SCRATCH\\user-X\\gb_char.dbin failed.\r\nrename SCRATCH\\user-X\\gbk_char.dbin.tmp to SCRATCH\\user-X\\gbk_char.dbin failed.\r\nrename SCRATCH\\user-X\\opengram.dbin.tmp to SCRATCH\\user-X\\opengram.dbin failed.\r\nrename SCRATCH\\user-X\\merged.dbin.tmp to SCRATCH\\user-X\\merged.dbin failed.\r\nrename SCRATCH\\user-X\\addon.bin.tmp to SCRATCH\\user-X\\addon.bin failed.\r\nrename SCRATCH\\user-X\\network.bin.tmp to SCRATCH\\user-X\\network.bin failed.\r\nrename SCRATCH\\user-X\\user.bin.tmp to SCRATCH\\user-X\\user.bin failed.\r\nrename SCRATCH\\user-X\\user_pinyin_index.bin.tmp to SCRATCH\\user-X\\user_pinyin_index.bin failed.\r\nrename SCRATCH\\user-X\\user_phrase_index.bin.tmp to SCRATCH\\user-X\\user_phrase_index.bin failed.\r\nrename SCRATCH\\user-X\\user_bigram.db.tmp to SCRATCH\\user-X\\user_bigram.db failed.\r\nwrite SCRATCH\\user-X\\user.conf failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/gb_char.dbin.tmp to SCRATCH/user-X/gb_char.dbin failed.\nrename SCRATCH/user-X/gbk_char.dbin.tmp to SCRATCH/user-X/gbk_char.dbin failed.\nrename SCRATCH/user-X/opengram.dbin.tmp to SCRATCH/user-X/opengram.dbin failed.\nrename SCRATCH/user-X/merged.dbin.tmp to SCRATCH/user-X/merged.dbin failed.\nrename SCRATCH/user-X/addon.bin.tmp to SCRATCH/user-X/addon.bin failed.\nrename SCRATCH/user-X/network.bin.tmp to SCRATCH/user-X/network.bin failed.\nrename SCRATCH/user-X/user.bin.tmp to SCRATCH/user-X/user.bin failed.\nrename SCRATCH/user-X/user_pinyin_index.bin.tmp to SCRATCH/user-X/user_pinyin_index.bin failed.\nrename SCRATCH/user-X/user_phrase_index.bin.tmp to SCRATCH/user-X/user_phrase_index.bin failed.\nrename SCRATCH/user-X/user_bigram.db.tmp to SCRATCH/user-X/user_bigram.db failed.\nwrite SCRATCH/user-X/user.conf failed.\n"
        }),
        |k| b::case(k, "stderr-save-dir-removed"),
        raw!(M(&[
            ("logs", L(&[])),
            ("save", B(true)),
            ("train", B(true))
        ])),
        None,
    );
}

// stderr-save-one-tmp-blocked: MATCH.
#[test]
fn stderr_save_one_tmp_blocked() {
    run_j(
        "save_failures_stderr::stderr_save_one_tmp_blocked",
        // The UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_pinyin_index.bin.tmp to SCRATCH\\user-X\\user_pinyin_index.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_pinyin_index.bin.tmp to SCRATCH/user-X/user_pinyin_index.bin failed.\n"
        }),
        |k| b::case(k, "stderr-save-one-tmp-blocked"),
        raw!(include!(
            "fixtures/save_failures_stderr/6ffd738a25abaac4.rs"
        )),
        None,
    );
}

// stderr-unmodified-save-dir-removed: MATCH.
#[test]
fn stderr_unmodified_save_dir_removed() {
    run_j(
        "save_failures_stderr::stderr_unmodified_save_dir_removed",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "stderr-unmodified-save-dir-removed"),
        raw!(M(&[("logs", L(&[])), ("save", B(false))])),
        None,
    );
}
