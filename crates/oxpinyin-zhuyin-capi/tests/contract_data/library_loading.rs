//! Library loading, saves and unloaded libraries (#1175, #1624). Generated from the frozen pin observations.

use crate::bodies::library_loading as b;
use crate::support::{Expect, FATAL_CRITICALS, UserFile, run_aborting, run_j};

// aliased-offset-unigram-zhuyin: MATCH.
#[test]
fn aliased_offset_unigram_zhuyin() {
    run_j(
        "library_loading::aliased_offset_unigram_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "aliased-offset-unigram-zhuyin"),
        raw!(include!("fixtures/library_loading/740fb419c4088ae3.rs")),
        None,
    );
}

// chunk-frame-system-trailing-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn chunk_frame_system_trailing_zhuyin() {
    run_j(
        "library_loading::chunk_frame_system_trailing_zhuyin",
        Expect::stderr(b"mmap SCRATCH/sys-X/merged.bin failed!\n"),
        |k| b::case(k, "chunk-frame-system-trailing-zhuyin"),
        raw!(M(&[
            ("ctx", B(false)),
            ("logs", L(&[L(&[S("libzhuyin"), I(16)])]))
        ])),
        Some(&["ctx", "logs"]),
    );
}

// crash-oob-layout-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn crash_oob_layout_zhuyin() {
    run_j(
        "library_loading::crash_oob_layout_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "crash-oob-layout-zhuyin"),
        raw!(M(&[("init", B(false))])),
        Some(&["init"]),
    );
}

// crash-save-missing-system-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn crash_save_missing_system_zhuyin() {
    run_j(
        "library_loading::crash_save_missing_system_zhuyin",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/gb_char.bin failed!\n",
        ),
        |k| b::case(k, "crash-save-missing-system-zhuyin"),
        raw!(M(&[("save", B(false))])),
        Some(&["save"]),
    );
}

// crash-save-remap-failed-retry-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn crash_save_remap_failed_retry_zhuyin() {
    run_j(
        "library_loading::crash_save_remap_failed_retry_zhuyin",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/gbk_char.bin failed!\n",
        ),
        |k| b::case(k, "crash-save-remap-failed-retry-zhuyin"),
        raw!(M(&[
            ("marker-kept", B(true)),
            ("retrain", B(true)),
            ("retry-save", B(true)),
            ("save", B(false)),
            ("stale-tmp", S("stale"))
        ])),
        Some(&["marker-kept", "retrain", "retry-save", "save", "stale-tmp"]),
    );
}

// high-system-library-zhuyin: MATCH.
#[test]
fn high_system_library_zhuyin() {
    run_j(
        "library_loading::high_system_library_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "high-system-library-zhuyin"),
        raw!(include!("fixtures/library_loading/49a4dcef396b60bc.rs")),
        None,
    );
}

// library-directory-guard-reserved-mask-out-zhuyin: MATCH.
#[test]
fn library_directory_guard_reserved_mask_out_zhuyin() {
    run_j(
        "library_loading::library_directory_guard_reserved_mask_out_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "library-directory-guard-reserved-mask-out-zhuyin"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// library-directory-guard-unloaded-save-zhuyin: MATCH.
#[test]
fn library_directory_guard_unloaded_save_zhuyin() {
    run_j(
        "library_loading::library_directory_guard_unloaded_save_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "library-directory-guard-unloaded-save-zhuyin"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// nonzero-total-candidates-zhuyin: the pin's abort, reproduced.
#[test]
fn nonzero_total_candidates_zhuyin() {
    run_aborting(
        "library_loading::nonzero_total_candidates_zhuyin",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed.").with_files(&[]),
        |k| {
            b::case(k, "nonzero-total-candidates-zhuyin");
        },
    );
}

// save-fatal-valid-to-invalid-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn save_fatal_valid_to_invalid_zhuyin() {
    run_j(
        "library_loading::save_fatal_valid_to_invalid_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(38252, "59eed2f9ef43edc1f6e62b72a6f83c6ac2b83e4a56f9956a19c0452dce92a34a")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:0|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]).with_env(FATAL_CRITICALS),
        |k| b::case(k, "save-fatal-valid-to-invalid-zhuyin"),
        raw!(include!("fixtures/library_loading/68c0969a67e323ae.rs")),
        None,
    );
}

// save-replacement-valid-to-different-valid-zhuyin: MATCH.
#[test]
fn save_replacement_valid_to_different_valid_zhuyin() {
    run_j(
        "library_loading::save_replacement_valid_to_different_valid_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(469888, "d9cc4602519e00ee9e2a3a1811e36cb1e4ffff8247871c8beefeb16a6d5046fd")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:0|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "save-replacement-valid-to-different-valid-zhuyin"),
        raw!(include!("fixtures/library_loading/b3d58b9f5728eddc.rs")),
        None,
    );
}

// save-replacement-valid-to-invalid-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn save_replacement_valid_to_invalid_zhuyin() {
    run_j(
        "library_loading::save_replacement_valid_to_invalid_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(38252, "59eed2f9ef43edc1f6e62b72a6f83c6ac2b83e4a56f9956a19c0452dce92a34a")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:0|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "save-replacement-valid-to-invalid-zhuyin"),
        raw!(include!("fixtures/library_loading/68c0969a67e323ae.rs")),
        None,
    );
}

// second-separator-library-init-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn second_separator_library_init_zhuyin() {
    run_j(
        "library_loading::second_separator_library_init_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "second-separator-library-init-zhuyin"),
        raw!(M(&[
            ("ctx", B(true)),
            ("fixture_allocated", I(4096)),
            ("fixture_size", I(40)),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// sparse-library-candidates-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_candidates_zhuyin() {
    run_j(
        "library_loading::sparse_library_candidates_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "sparse-library-candidates-zhuyin"),
        raw!(include!("fixtures/library_loading/c95d2a5f23a27ab8.rs")),
        None,
    );
}

// sparse-library-init-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_init_zhuyin() {
    run_j(
        "library_loading::sparse_library_init_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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

// sparse-library-load-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_load_zhuyin() {
    run_j(
        "library_loading::sparse_library_load_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "sparse-library-load-zhuyin"),
        raw!(M(&[
            ("load", B(true)),
            ("logs", L(&[])),
            ("unload", B(true))
        ])),
        None,
    );
}

// sparse-library-mask-out-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_mask_out_zhuyin() {
    run_j(
        "library_loading::sparse_library_mask_out_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[]),
        |k| b::case(k, "sparse-library-mask-out-zhuyin"),
        raw!(M(&[
            ("ctx", B(true)),
            ("fixture_allocated", I(4096)),
            ("fixture_size", I(4294967304)),
            ("logs", L(&[])),
            ("mask_out", B(true)),
            ("train", B(true))
        ])),
        None,
    );
}

// sparse-library-reinit-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_reinit_zhuyin() {
    run_j(
        "library_loading::sparse_library_reinit_zhuyin",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.open SCRATCH/user-X/user.conf failed.",
        ),
        |k| b::case(k, "sparse-library-reinit-zhuyin"),
        raw!(M(&[
            ("ctxs", L(&[B(true), B(true)])),
            ("fixture_allocated", I(4096)),
            ("fixture_size", I(4294967304)),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// sparse-library-save-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_save_zhuyin() {
    run_j(
        "library_loading::sparse_library_save_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(26, "30923d4c45a569a8cfcb0a220f3e91b83904ce40af34145848ea46f0ef0f029a")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:0|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "sparse-library-save-zhuyin"),
        raw!(M(&[("ctx", B(true)), ("fixture_allocated", I(4096)), ("fixture_size", I(4294967304)), ("logs", L(&[])), ("save", B(true)), ("train", B(true))])),
        None,
    );
}

// sparse-library-tokens-zhuyin: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_tokens_zhuyin() {
    run_j(
        "library_loading::sparse_library_tokens_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "sparse-library-tokens-zhuyin"),
        raw!(M(&[
            (
                "0x4000001",
                L(&[B(false), I(0), Nil, B(false), I(0), B(false), I(0)])
            ),
            (
                "0x4000002",
                L(&[B(false), I(0), Nil, B(false), I(0), B(false), I(0)])
            ),
            ("ctx", B(true)),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// stderr-library-control-load-already-loaded-zhuyin: MATCH.
#[test]
fn stderr_library_control_load_already_loaded_zhuyin() {
    run_j(
        "library_loading::stderr_library_control_load_already_loaded_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "stderr-library-control-load-already-loaded-zhuyin"),
        raw!(M(&[("load", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// stderr-library-control-mask-out-unloaded-zhuyin: MATCH.
#[test]
fn stderr_library_control_mask_out_unloaded_zhuyin() {
    run_j(
        "library_loading::stderr_library_control_mask_out_unloaded_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "stderr-library-control-mask-out-unloaded-zhuyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("mask_out", B(true)),
            ("train", B(true)),
            ("unload", B(true))
        ])),
        None,
    );
}

// stderr-library-control-save-unmodified-zhuyin: MATCH.
#[test]
fn stderr_library_control_save_unmodified_zhuyin() {
    run_j(
        "library_loading::stderr_library_control_save_unmodified_zhuyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "stderr-library-control-save-unmodified-zhuyin"),
        raw!(M(&[("logs", L(&[])), ("save", B(false))])),
        None,
    );
}

// stderr-library-init-empty-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn stderr_library_init_empty_zhuyin() {
    run_j(
        "library_loading::stderr_library_init_empty_zhuyin",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/merged.bin failed!\n",
        ),
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
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/gbk_char.bin failed!\n",
        ),
        |k| b::case(k, "stderr-library-load-checksum-zhuyin"),
        raw!(M(&[
            ("load", B(true)),
            ("logs", L(&[])),
            ("unload", B(true))
        ])),
        Some(&["load", "logs", "unload"]),
    );
}

// stderr-library-mask-out-short-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn stderr_library_mask_out_short_zhuyin() {
    run_j(
        "library_loading::stderr_library_mask_out_short_zhuyin",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/merged.bin failed!\n",
        ),
        |k| b::case(k, "stderr-library-mask-out-short-zhuyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("mask_out", B(true)),
            ("train", B(true))
        ])),
        Some(&["logs", "mask_out", "train"]),
    );
}

// stderr-library-save-missing-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn stderr_library_save_missing_zhuyin() {
    run_j(
        "library_loading::stderr_library_save_missing_zhuyin",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/merged.bin failed!\n",
        ),
        |k| b::case(k, "stderr-library-save-missing-zhuyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("save", B(false)),
            ("train", B(true))
        ])),
        Some(&["logs", "save", "train"]),
    );
}

// transient-reload-replacement-zhuyin: MATCH.
#[test]
fn transient_reload_replacement_zhuyin() {
    run_j(
        "library_loading::transient_reload_replacement_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "transient-reload-replacement-zhuyin"),
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

// unloaded-library-import-zhuyin: MATCH.
#[test]
fn unloaded_library_import_zhuyin() {
    run_j(
        "library_loading::unloaded_library_import_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-import-zhuyin"),
        raw!(include!("fixtures/library_loading/71ed666955e60673.rs")),
        None,
    );
}

// unloaded-library-token-zhuyin: MATCH.
#[test]
fn unloaded_library_token_zhuyin() {
    run_j(
        "library_loading::unloaded_library_token_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-token-zhuyin"),
        raw!(include!("fixtures/library_loading/c737d76fb9f07077.rs")),
        None,
    );
}

// unloaded-library-train-user-zhuyin: MATCH.
#[test]
fn unloaded_library_train_user_zhuyin() {
    run_j(
        "library_loading::unloaded_library_train_user_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-train-user-zhuyin"),
        raw!(include!("fixtures/library_loading/99396823f63de809.rs")),
        None,
    );
}

// unloaded-library-train-zhuyin: MATCH.
#[test]
fn unloaded_library_train_zhuyin() {
    run_j(
        "library_loading::unloaded_library_train_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-train-zhuyin"),
        raw!(include!("fixtures/library_loading/2cbb4584496902be.rs")),
        None,
    );
}
