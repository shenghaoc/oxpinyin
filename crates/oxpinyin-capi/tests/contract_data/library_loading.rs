//! Library loading, saves and unloaded libraries (#1175, #1624). Generated from the frozen pin observations.

use crate::bodies::library_loading as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};
// chunk-frame-system-exact-pinyin: MATCH.
#[test]
fn chunk_frame_system_exact_pinyin() {
    run_j(
        "library_loading::chunk_frame_system_exact_pinyin",
        Expect::RESULT,
        |k| b::case(k, "chunk-frame-system-exact-pinyin"),
        raw!(M(&[("ctx", B(true)), ("logs", L(&[]))])),
        None,
    );
}
// crash-save-missing-system: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn crash_save_missing_system() {
    run_j(
        "library_loading::crash_save_missing_system",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.mmap SCRATCH\\sys-X\\gb_char.bin failed!\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/gb_char.bin failed!\n"
        }),
        |k| b::case(k, "crash-save-missing-system"),
        raw!(M(&[("save", B(false))])),
        Some(&["save"]),
    );
}
// library-directory-guard-reserved-save: MATCH.
#[test]
fn library_directory_guard_reserved_save() {
    run_j(
        "library_loading::library_directory_guard_reserved_save",
        Expect::RESULT,
        |k| b::case(k, "library-directory-guard-reserved-save"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}
// nonzero-total-candidates: the pin's abort, reproduced.
#[test]
fn nonzero_total_candidates() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "library_loading::nonzero_total_candidates",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[("user.conf", UserFile::Conf(USER_CONF))]),
        |k| {
            b::case(k, "nonzero-total-candidates");
        },
    );
}
// save-replacement-valid-to-invalid: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn save_replacement_valid_to_invalid() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_j(
        "library_loading::save_replacement_valid_to_invalid",
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
        |k| b::case(k, "save-replacement-valid-to-invalid"),
        raw!(include!("fixtures/library_loading/68c0969a67e323ae.rs")),
        None,
    );
}
// sparse-library-init: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
// Unix only: the expectation holds the frozen block count (st_blocks) of the file the test writes; Windows reports the allocation, not blocks.
#[cfg(unix)]
#[test]
fn sparse_library_init() {
    run_j(
        "library_loading::sparse_library_init",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "sparse-library-init"),
        raw!(M(&[
            ("ctx", B(true)),
            ("fixture_allocated", I(4096)),
            ("fixture_size", I(4294967304)),
            ("logs", L(&[]))
        ])),
        None,
    );
}
// stderr-library-init-missing: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn stderr_library_init_missing() {
    run_j(
        "library_loading::stderr_library_init_missing",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.mmap SCRATCH\\sys-X\\merged.bin failed!\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/merged.bin failed!\n"
        }),
        |k| b::case(k, "stderr-library-init-missing"),
        raw!(M(&[("ctx", B(true)), ("logs", L(&[]))])),
        Some(&["ctx", "logs"]),
    );
}

// stderr-library-load-empty: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn stderr_library_load_empty() {
    run_j(
        "library_loading::stderr_library_load_empty",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.mmap SCRATCH\\sys-X\\gbk_char.bin failed!\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/gbk_char.bin failed!\n"
        }),
        |k| b::case(k, "stderr-library-load-empty"),
        raw!(M(&[
            ("load", B(true)),
            ("logs", L(&[])),
            ("unload", B(true))
        ])),
        Some(&["load", "logs", "unload"]),
    );
}
// transient-reload-replacement-pinyin: MATCH.
#[test]
fn transient_reload_replacement_pinyin() {
    run_j(
        "library_loading::transient_reload_replacement_pinyin",
        Expect::RESULT,
        |k| b::case(k, "transient-reload-replacement-pinyin"),
        raw!(M(&[
            ("import", B(true)),
            ("import_replacement_key", B(true)),
            ("logs", L(&[])),
            ("replacement_key", B(true)),
            ("replacement_unigram", B(true))
        ])),
        None,
    );
}
// unloaded-library-train-pinyin: MATCH.
#[test]
fn unloaded_library_train_pinyin() {
    run_j(
        "library_loading::unloaded_library_train_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-train-pinyin"),
        raw!(include!("fixtures/library_loading/2cbb4584496902be.rs")),
        None,
    );
}
