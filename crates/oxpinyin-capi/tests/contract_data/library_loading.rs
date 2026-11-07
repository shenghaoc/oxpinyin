//! Library loading, saves and unloaded libraries (#1175, #1624). Generated from the frozen pin observations.

use crate::bodies::library_loading as b;
use crate::support::{Expect, FATAL_CRITICALS, UserFile, run_aborting, run_j};

// aliased-offset-unigram-pinyin: MATCH.
#[test]
fn aliased_offset_unigram_pinyin() {
    run_j(
        "library_loading::aliased_offset_unigram_pinyin",
        Expect::RESULT,
        |k| b::case(k, "aliased-offset-unigram-pinyin"),
        raw!(include!("fixtures/library_loading/740fb419c4088ae3.rs")),
        None,
    );
}

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

// chunk-frame-system-trailing-pinyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn chunk_frame_system_trailing_pinyin() {
    run_j(
        "library_loading::chunk_frame_system_trailing_pinyin",
        Expect::stderr(b"mmap SCRATCH/sys-X/merged.bin failed!\n"),
        |k| b::case(k, "chunk-frame-system-trailing-pinyin"),
        raw!(M(&[
            ("ctx", B(false)),
            ("logs", L(&[L(&[S("libpinyin"), I(16)])]))
        ])),
        Some(&["ctx", "logs"]),
    );
}

// crash-oob-layout: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn crash_oob_layout() {
    run_j(
        "library_loading::crash_oob_layout",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "crash-oob-layout"),
        raw!(M(&[("init", B(false))])),
        Some(&["init"]),
    );
}

// crash-save-missing-system: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn crash_save_missing_system() {
    run_j(
        "library_loading::crash_save_missing_system",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/gb_char.bin failed!\n",
        ),
        |k| b::case(k, "crash-save-missing-system"),
        raw!(M(&[("save", B(false))])),
        Some(&["save"]),
    );
}

// crash-save-remap-failed-retry: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn crash_save_remap_failed_retry() {
    run_j(
        "library_loading::crash_save_remap_failed_retry",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/gbk_char.bin failed!\n",
        ),
        |k| b::case(k, "crash-save-remap-failed-retry"),
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

// header-only-library-init: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn header_only_library_init() {
    run_j(
        "library_loading::header_only_library_init",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "header-only-library-init"),
        raw!(M(&[
            ("ctx", B(true)),
            ("fixture_allocated", I(4096)),
            ("fixture_size", I(8)),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// header-only-library-load: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn header_only_library_load() {
    run_j(
        "library_loading::header_only_library_load",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "header-only-library-load"),
        raw!(M(&[
            ("load", B(true)),
            ("logs", L(&[])),
            ("unload", B(true))
        ])),
        None,
    );
}

// header-only-library-save: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn header_only_library_save() {
    run_j(
        "library_loading::header_only_library_save",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(26, "30923d4c45a569a8cfcb0a220f3e91b83904ce40af34145848ea46f0ef0f029a")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "header-only-library-save"),
        raw!(M(&[("ctx", B(true)), ("fixture_allocated", I(4096)), ("fixture_size", I(8)), ("logs", L(&[])), ("save", B(true)), ("train", B(true))])),
        None,
    );
}

// high-system-library-pinyin: MATCH.
#[test]
fn high_system_library_pinyin() {
    run_j(
        "library_loading::high_system_library_pinyin",
        Expect::RESULT,
        |k| b::case(k, "high-system-library-pinyin"),
        raw!(include!("fixtures/library_loading/a0e1026c5784f07e.rs")),
        None,
    );
}

// internally-overlapping-modify-pinyin: MATCH.
#[test]
fn internally_overlapping_modify_pinyin() {
    run_j(
        "library_loading::internally_overlapping_modify_pinyin",
        Expect::RESULT,
        |k| b::case(k, "internally-overlapping-modify-pinyin"),
        raw!(include!("fixtures/library_loading/d1ddb54e3897b92a.rs")),
        None,
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

// library-directory-guard-unloaded-mask-out: MATCH.
#[test]
fn library_directory_guard_unloaded_mask_out() {
    run_j(
        "library_loading::library_directory_guard_unloaded_mask_out",
        Expect::RESULT,
        |k| b::case(k, "library-directory-guard-unloaded-mask-out"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// nonzero-total-candidates: the pin's abort, reproduced.
#[test]
fn nonzero_total_candidates() {
    run_aborting(
        "library_loading::nonzero_total_candidates",
        Expect::aborts(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|"))]),
        |k| {
            b::case(k, "nonzero-total-candidates");
        },
    );
}

// nonzero-total-save: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn nonzero_total_save() {
    run_j(
        "library_loading::nonzero_total_save",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "nonzero-total-save"),
        raw!(M(&[
            ("dbin", L(&[I(26), I(123), I(0)])),
            ("logs", L(&[])),
            ("save", B(true)),
            ("train", B(true))
        ])),
        None,
    );
}

// overlapping-modify-before-add-pinyin: MATCH.
#[test]
fn overlapping_modify_before_add_pinyin() {
    run_j(
        "library_loading::overlapping_modify_before_add_pinyin",
        Expect::RESULT,
        |k| b::case(k, "overlapping-modify-before-add-pinyin"),
        raw!(include!("fixtures/library_loading/90ee4125135cc3a7.rs")),
        None,
    );
}

// save-fatal-valid-to-invalid: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn save_fatal_valid_to_invalid() {
    run_j(
        "library_loading::save_fatal_valid_to_invalid",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(38252, "59eed2f9ef43edc1f6e62b72a6f83c6ac2b83e4a56f9956a19c0452dce92a34a")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]).with_env(FATAL_CRITICALS),
        |k| b::case(k, "save-fatal-valid-to-invalid"),
        raw!(include!("fixtures/library_loading/68c0969a67e323ae.rs")),
        None,
    );
}

// save-replacement-invalid-to-valid: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn save_replacement_invalid_to_valid() {
    run_j(
        "library_loading::save_replacement_invalid_to_valid",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(38252, "3791409ac5f68ca2c66856fca4cb3b18e82a233358d7d4b037438a3d4e1dc88f")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "save-replacement-invalid-to-valid"),
        raw!(include!("fixtures/library_loading/a598794cd8f3b8a1.rs")),
        None,
    );
}

// save-replacement-valid-to-different-valid: MATCH.
#[test]
fn save_replacement_valid_to_different_valid() {
    run_j(
        "library_loading::save_replacement_valid_to_different_valid",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(469888, "d9cc4602519e00ee9e2a3a1811e36cb1e4ffff8247871c8beefeb16a6d5046fd")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "save-replacement-valid-to-different-valid"),
        raw!(include!("fixtures/library_loading/b3d58b9f5728eddc.rs")),
        None,
    );
}

// save-replacement-valid-to-different-valid-with-user-override: MATCH.
#[test]
fn save_replacement_valid_to_different_valid_with_user_override() {
    run_j(
        "library_loading::save_replacement_valid_to_different_valid_with_user_override",
        Expect::stderr(b"").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(3846860, "7d0dce87604baa76c206c280b390979126e2a9833fd398a200c3547011dea487")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(26, "8ac7679f1c49c436e73595048a3d412f68357800a18444cecc5cf9977b345b74")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "save-replacement-valid-to-different-valid-with-user-override"),
        raw!(include!("fixtures/library_loading/6dbb71f8ad17a1a2.rs")),
        None,
    );
}

// save-replacement-valid-to-invalid: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn save_replacement_valid_to_invalid() {
    run_j(
        "library_loading::save_replacement_valid_to_invalid",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(38252, "59eed2f9ef43edc1f6e62b72a6f83c6ac2b83e4a56f9956a19c0452dce92a34a")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "save-replacement-valid-to-invalid"),
        raw!(include!("fixtures/library_loading/68c0969a67e323ae.rs")),
        None,
    );
}

// save-unchanged-layout-failed: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn save_unchanged_layout_failed() {
    run_j(
        "library_loading::save_unchanged_layout_failed",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(26, "d14f29a660dbd970aac6750c727860f24920c105ae0cb1a851399b0807b0bfff")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "save-unchanged-layout-failed"),
        raw!(include!("fixtures/library_loading/0af2662b230fe416.rs")),
        None,
    );
}

// save-unchanged-valid: MATCH.
#[test]
fn save_unchanged_valid() {
    run_j(
        "library_loading::save_unchanged_valid",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(26, "8ac7679f1c49c436e73595048a3d412f68357800a18444cecc5cf9977b345b74")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "save-unchanged-valid"),
        raw!(include!("fixtures/library_loading/da40159d3e3dc003.rs")),
        None,
    );
}

// save-unloaded-replacement: MATCH.
#[test]
fn save_unloaded_replacement() {
    run_j(
        "library_loading::save_unloaded_replacement",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("merged.dbin", UserFile::Chunk(26, "8ac7679f1c49c436e73595048a3d412f68357800a18444cecc5cf9977b345b74")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "save-unloaded-replacement"),
        raw!(M(&[("logs", L(&[])), ("save", B(true)), ("train", B(true)), ("unload", B(true))])),
        None,
    );
}

// second-separator-library-init: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn second_separator_library_init() {
    run_j(
        "library_loading::second_separator_library_init",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "second-separator-library-init"),
        raw!(M(&[
            ("ctx", B(true)),
            ("fixture_allocated", I(4096)),
            ("fixture_size", I(40)),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// short-chunk-layout: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn short_chunk_layout() {
    run_j(
        "library_loading::short_chunk_layout",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "short-chunk-layout"),
        raw!(M(&[("init", B(true)), ("logs", L(&[]))])),
        None,
    );
}

// sparse-library-addon: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_addon() {
    run_j(
        "library_loading::sparse_library_addon",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "sparse-library-addon"),
        raw!(M(&[
            ("first", B(true)),
            ("fixture_allocated", I(4096)),
            ("fixture_size", I(4294967304)),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// sparse-library-addon-reload: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_addon_reload() {
    run_j(
        "library_loading::sparse_library_addon_reload",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "sparse-library-addon-reload"),
        raw!(M(&[
            ("again", B(true)),
            ("first", B(true)),
            ("fixture_allocated", I(4096)),
            ("fixture_size", I(4294967304)),
            ("logs", L(&[])),
            ("unload", B(true))
        ])),
        None,
    );
}

// sparse-library-candidates: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_candidates() {
    run_j(
        "library_loading::sparse_library_candidates",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "sparse-library-candidates"),
        raw!(include!("fixtures/library_loading/6fb7e3831f5589f7.rs")),
        None,
    );
}

// sparse-library-init: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_init() {
    run_j(
        "library_loading::sparse_library_init",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
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

// sparse-library-load: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_load() {
    run_j(
        "library_loading::sparse_library_load",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "sparse-library-load"),
        raw!(M(&[
            ("load", B(true)),
            ("logs", L(&[])),
            ("unload", B(true))
        ])),
        None,
    );
}

// sparse-library-mask-out: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_mask_out() {
    run_j(
        "library_loading::sparse_library_mask_out",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|"))]),
        |k| b::case(k, "sparse-library-mask-out"),
        raw!(M(&[("ctx", B(true)), ("fixture_allocated", I(4096)), ("fixture_size", I(4294967304)), ("logs", L(&[])), ("mask_out", B(true)), ("train", B(true))])),
        None,
    );
}

// sparse-library-reinit: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_reinit() {
    run_j(
        "library_loading::sparse_library_reinit",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "sparse-library-reinit"),
        raw!(M(&[
            ("ctxs", L(&[B(true), B(true)])),
            ("fixture_allocated", I(4096)),
            ("fixture_size", I(4294967304)),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// sparse-library-save: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_save() {
    run_j(
        "library_loading::sparse_library_save",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed.").with_files(&[("addon.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("gb_char.dbin", UserFile::Chunk(26, "b0624c19cf007ca296f39ee33766942a2b2dcfa583245f0c044c36b74f3b93d3")), ("gbk_char.dbin", UserFile::Chunk(26, "f4f18f1863e8a9450209951078d41335c28a4524a528a3e53aea07f3e5df7153")), ("merged.dbin", UserFile::Chunk(26, "30923d4c45a569a8cfcb0a220f3e91b83904ce40af34145848ea46f0ef0f029a")), ("network.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("opengram.dbin", UserFile::Chunk(26, "d3b9a15d949808519d5c1a748ba3a60a198b4830106a8731fa9ab5edc1a9a8cc")), ("user.bin", UserFile::Chunk(27, "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|")), ("user_bigram.db", UserFile::Size(4198400)), ("user_phrase_index.bin", UserFile::Size(529408)), ("user_pinyin_index.bin", UserFile::Size(529408))]),
        |k| b::case(k, "sparse-library-save"),
        raw!(M(&[("ctx", B(true)), ("fixture_allocated", I(4096)), ("fixture_size", I(4294967304)), ("logs", L(&[])), ("save", B(true)), ("train", B(true))])),
        None,
    );
}

// sparse-library-tokens: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn sparse_library_tokens() {
    run_j(
        "library_loading::sparse_library_tokens",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "sparse-library-tokens"),
        raw!(include!("fixtures/library_loading/b0b385bca77c2004.rs")),
        None,
    );
}

// stderr-library-addon-checksum: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn stderr_library_addon_checksum() {
    run_j(
        "library_loading::stderr_library_addon_checksum",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/art.bin failed!\n",
        ),
        |k| b::case(k, "stderr-library-addon-checksum"),
        raw!(M(&[
            ("load", B(false)),
            ("logs", L(&[])),
            ("unload", B(true))
        ])),
        Some(&["load", "logs", "unload"]),
    );
}

// stderr-library-control-addon-already-loaded: MATCH.
#[test]
fn stderr_library_control_addon_already_loaded() {
    run_j(
        "library_loading::stderr_library_control_addon_already_loaded",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "stderr-library-control-addon-already-loaded"),
        raw!(M(&[
            ("again", B(false)),
            ("first", B(true)),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// stderr-library-control-load-already-loaded: MATCH.
#[test]
fn stderr_library_control_load_already_loaded() {
    run_j(
        "library_loading::stderr_library_control_load_already_loaded",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "stderr-library-control-load-already-loaded"),
        raw!(M(&[("load", B(false)), ("logs", L(&[]))])),
        None,
    );
}

// stderr-library-control-load-whole: MATCH.
#[test]
fn stderr_library_control_load_whole() {
    run_j(
        "library_loading::stderr_library_control_load_whole",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "stderr-library-control-load-whole"),
        raw!(M(&[
            ("load", B(true)),
            ("logs", L(&[])),
            ("unload", B(true))
        ])),
        None,
    );
}

// stderr-library-control-mask-out-unloaded: MATCH.
#[test]
fn stderr_library_control_mask_out_unloaded() {
    run_j(
        "library_loading::stderr_library_control_mask_out_unloaded",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "stderr-library-control-mask-out-unloaded"),
        raw!(M(&[
            ("logs", L(&[])),
            ("mask_out", B(true)),
            ("train", B(true)),
            ("unload", B(true))
        ])),
        None,
    );
}

// stderr-library-control-save-unloaded: MATCH.
#[test]
fn stderr_library_control_save_unloaded() {
    run_j(
        "library_loading::stderr_library_control_save_unloaded",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "stderr-library-control-save-unloaded"),
        raw!(M(&[
            ("logs", L(&[])),
            ("save", B(true)),
            ("train", B(true)),
            ("unload", B(true))
        ])),
        None,
    );
}

// stderr-library-control-save-unmodified: MATCH.
#[test]
fn stderr_library_control_save_unmodified() {
    run_j(
        "library_loading::stderr_library_control_save_unmodified",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "stderr-library-control-save-unmodified"),
        raw!(M(&[("logs", L(&[])), ("save", B(false))])),
        None,
    );
}

// stderr-library-init-missing: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn stderr_library_init_missing() {
    run_j(
        "library_loading::stderr_library_init_missing",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/merged.bin failed!\n",
        ),
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
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/gbk_char.bin failed!\n",
        ),
        |k| b::case(k, "stderr-library-load-empty"),
        raw!(M(&[
            ("load", B(true)),
            ("logs", L(&[])),
            ("unload", B(true))
        ])),
        Some(&["load", "logs", "unload"]),
    );
}

// stderr-library-mask-out-bad-length: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn stderr_library_mask_out_bad_length() {
    run_j(
        "library_loading::stderr_library_mask_out_bad_length",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/merged.bin failed!\n",
        ),
        |k| b::case(k, "stderr-library-mask-out-bad-length"),
        raw!(M(&[
            ("logs", L(&[])),
            ("mask_out", B(true)),
            ("train", B(true))
        ])),
        Some(&["logs", "mask_out", "train"]),
    );
}

// stderr-library-save-short: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn stderr_library_save_short() {
    run_j(
        "library_loading::stderr_library_save_short",
        Expect::stderr(
            b"open SCRATCH/user-X/user.conf failed.mmap SCRATCH/sys-X/merged.bin failed!\n",
        ),
        |k| b::case(k, "stderr-library-save-short"),
        raw!(M(&[
            ("logs", L(&[])),
            ("save", B(false)),
            ("train", B(true))
        ])),
        Some(&["logs", "save", "train"]),
    );
}

// third-separator-library-init: POLICY-ACCEPTED, oxpinyin's value (1297-layout-criticals.md).
#[test]
fn third_separator_library_init() {
    run_j(
        "library_loading::third_separator_library_init",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "third-separator-library-init"),
        raw!(M(&[
            ("ctx", B(true)),
            ("fixture_allocated", I(4096)),
            ("fixture_size", I(40)),
            ("logs", L(&[]))
        ])),
        None,
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

// unloaded-cached-pronunciation-pinyin: POLICY-ACCEPTED, oxpinyin's value (compatibility-policy.md).
#[test]
fn unloaded_cached_pronunciation_pinyin() {
    run_j(
        "library_loading::unloaded_cached_pronunciation_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unloaded-cached-pronunciation-pinyin"),
        raw!(include!("fixtures/library_loading/2202d73fda7155e3.rs")),
        None,
    );
}

// unloaded-library-import-pinyin: MATCH.
#[test]
fn unloaded_library_import_pinyin() {
    run_j(
        "library_loading::unloaded_library_import_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-import-pinyin"),
        raw!(include!("fixtures/library_loading/71ed666955e60673.rs")),
        None,
    );
}

// unloaded-library-longer-missing-pinyin: MATCH.
#[test]
fn unloaded_library_longer_missing_pinyin() {
    run_j(
        "library_loading::unloaded_library_longer_missing_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-longer-missing-pinyin"),
        raw!(include!("fixtures/library_loading/43ef40b85a27cfdf.rs")),
        None,
    );
}

// unloaded-library-longer-overflow-pinyin: MATCH.
#[test]
fn unloaded_library_longer_overflow_pinyin() {
    run_j(
        "library_loading::unloaded_library_longer_overflow_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-longer-overflow-pinyin"),
        raw!(include!("fixtures/library_loading/ed8e7db31772a897.rs")),
        None,
    );
}

// unloaded-library-longer-pinyin: MATCH.
#[test]
fn unloaded_library_longer_pinyin() {
    run_j(
        "library_loading::unloaded_library_longer_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-longer-pinyin"),
        raw!(include!("fixtures/library_loading/c46567857f54ce5c.rs")),
        None,
    );
}

// unloaded-library-predicted-bigram-pinyin: MATCH.
#[test]
fn unloaded_library_predicted_bigram_pinyin() {
    run_j(
        "library_loading::unloaded_library_predicted_bigram_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-predicted-bigram-pinyin"),
        raw!(include!("fixtures/library_loading/14849a844e1724ba.rs")),
        None,
    );
}

// unloaded-library-predicted-bigram-predecessor-pinyin: MATCH.
#[test]
fn unloaded_library_predicted_bigram_predecessor_pinyin() {
    run_j(
        "library_loading::unloaded_library_predicted_bigram_predecessor_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-predicted-bigram-predecessor-pinyin"),
        raw!(include!("fixtures/library_loading/241866802826ed2b.rs")),
        None,
    );
}

// unloaded-library-predicted-prefix-pinyin: MATCH.
#[test]
fn unloaded_library_predicted_prefix_pinyin() {
    run_j(
        "library_loading::unloaded_library_predicted_prefix_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-predicted-prefix-pinyin"),
        raw!(include!("fixtures/library_loading/c279fd6fe4104967.rs")),
        None,
    );
}

// unloaded-library-token-pinyin: MATCH.
#[test]
fn unloaded_library_token_pinyin() {
    run_j(
        "library_loading::unloaded_library_token_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-token-pinyin"),
        raw!(include!("fixtures/library_loading/c737d76fb9f07077.rs")),
        None,
    );
}

// unloaded-library-train-before-pinyin: MATCH.
#[test]
fn unloaded_library_train_before_pinyin() {
    run_j(
        "library_loading::unloaded_library_train_before_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-train-before-pinyin"),
        raw!(include!("fixtures/library_loading/46627254626cc6fa.rs")),
        None,
    );
}

// unloaded-library-train-journal-pinyin: MATCH.
#[test]
fn unloaded_library_train_journal_pinyin() {
    run_j(
        "library_loading::unloaded_library_train_journal_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-train-journal-pinyin"),
        raw!(include!("fixtures/library_loading/c59c93c6257d9acb.rs")),
        None,
    );
}

// unloaded-library-train-mixed-pinyin: MATCH.
#[test]
fn unloaded_library_train_mixed_pinyin() {
    run_j(
        "library_loading::unloaded_library_train_mixed_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-train-mixed-pinyin"),
        raw!(include!("fixtures/library_loading/d11b25ed1c3f5453.rs")),
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

// unloaded-library-without-missing-pinyin: MATCH.
#[test]
fn unloaded_library_without_missing_pinyin() {
    run_j(
        "library_loading::unloaded_library_without_missing_pinyin",
        Expect::RESULT,
        |k| b::case(k, "unloaded-library-without-missing-pinyin"),
        raw!(include!("fixtures/library_loading/52205fcf704060a7.rs")),
        None,
    );
}
