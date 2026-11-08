//! Library loading, saves and unloaded libraries (#1175, #1624). Generated from the frozen pin observations.

use crate::bodies::library_loading as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};
// nonzero-total-candidates-zhuyin: the pin's abort, reproduced.
#[test]
fn nonzero_total_candidates_zhuyin() {
    run_aborting(
        "library_loading::nonzero_total_candidates_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[]),
        |k| {
            b::case(k, "nonzero-total-candidates-zhuyin");
        },
    );
}
// save-replacement-valid-to-invalid-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn save_replacement_valid_to_invalid_zhuyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:0\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:0|";
    run_j(
        "library_loading::save_replacement_valid_to_invalid_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
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
                    38252,
                    "59eed2f9ef43edc1f6e62b72a6f83c6ac2b83e4a56f9956a19c0452dce92a34a",
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
            (
                "user.bin",
                UserFile::Chunk(
                    27,
                    "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697",
                ),
            ),
            ("user.conf", UserFile::Conf(USER_CONF)),
            ("user_bigram.db", UserFile::Size(4198400)),
            ("user_phrase_index.bin", UserFile::Size(529408)),
            ("user_pinyin_index.bin", UserFile::Size(529408)),
        ]),
        |k| b::case(k, "save-replacement-valid-to-invalid-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/library_loading/68c0969a67e323ae.rs"
        )),
        None,
    );
}
// sparse-library-init-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
// Unix only: the expectation holds the frozen block count (st_blocks) of the file the test writes; Windows reports the allocation, not blocks.
#[cfg(unix)]
#[test]
fn sparse_library_init_zhuyin() {
    run_j(
        "library_loading::sparse_library_init_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "sparse-library-init-zhuyin"),
        raw!(M(&[
            ("ctx", B(true)),
            ("fixture_allocated", I(4096)),
            ("fixture_size", I(4294967304)),
            ("logs", L(&[]))
        ])),
        None,
    );
}
// stderr-library-init-empty-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn stderr_library_init_empty_zhuyin() {
    run_j(
        "library_loading::stderr_library_init_empty_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.mmap SCRATCH\\sys-X\\merged.bin failed!\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/merged.bin failed!\n"
        }),
        |k| b::case(k, "stderr-library-init-empty-zhuyin"),
        raw!(M(&[
            ("ctx", B(false)),
            ("logs", L(&[L(&[S("libzhuyin"), I(16)])]))
        ])),
        Some(&["ctx", "logs"]),
    );
}

// stderr-library-load-checksum-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn stderr_library_load_checksum_zhuyin() {
    run_j(
        "library_loading::stderr_library_load_checksum_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.mmap SCRATCH\\sys-X\\gbk_char.bin failed!\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/gbk_char.bin failed!\n"
        }),
        |k| b::case(k, "stderr-library-load-checksum-zhuyin"),
        raw!(M(&[
            ("load", B(true)),
            ("logs", L(&[])),
            ("unload", B(true))
        ])),
        Some(&["load", "logs", "unload"]),
    );
}
// unloaded-library-train-zhuyin: MATCH.
#[test]
fn unloaded_library_train_zhuyin() {
    run_j(
        "library_loading::unloaded_library_train_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-train-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/library_loading/2cbb4584496902be.rs"
        )),
        None,
    );
}
