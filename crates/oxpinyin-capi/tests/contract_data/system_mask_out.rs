//! System items masked out after an import into library 1, kept in the journal or saved, after a MODIFY record or a replaced system chunk, and a refused remap that keeps the session (#1175, #728, #545). Generated from the frozen pin observations.

use crate::bodies::system_mask_out as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};

// abort-mask-out-surviving-add-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_mask_out_surviving_add_pinyin() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "system_mask_out::abort_mask_out_surviving_add_pinyin",
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
                    58,
                    "934874f39661a3acc8986a0e97e3473d3a191222ea70a2bed0a9b00504d0361d",
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
        |k| {
            b::case(k, "abort-mask-out-surviving-add-pinyin");
        },
    );
}

// mask-out-dropped-add-full-pinyin: MATCH.
#[test]
fn mask_out_dropped_add_full_pinyin() {
    run_j(
        "system_mask_out::mask_out_dropped_add_full_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-dropped-add-full-pinyin"),
        raw!(include!("fixtures/system_mask_out/aff286cbb35090bf.rs")),
        None,
    );
}

// mask-out-dropped-add-headerless-pinyin: MATCH.
#[test]
fn mask_out_system_disk_replacement_pinyin() {
    run_j(
        "system_mask_out::mask_out_system_disk_replacement_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-system-disk-replacement-pinyin"),
        raw!(include!("fixtures/system_mask_out/d27bdd027eb4b66f.rs")),
        None,
    );
}

// mask-out-system-headerless-add-pinyin: MATCH.
#[test]
fn mask_out_system_headerless_add_pinyin() {
    run_j(
        "system_mask_out::mask_out_system_headerless_add_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-system-headerless-add-pinyin"),
        raw!(include!("fixtures/system_mask_out/7fdd6a2d6469122b.rs")),
        None,
    );
}

// mask-out-system-modify-pinyin: MATCH.
#[test]
fn mask_out_system_modify_pinyin() {
    run_j(
        "system_mask_out::mask_out_system_modify_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-system-modify-pinyin"),
        raw!(include!("fixtures/system_mask_out/d27bdd027eb4b66f.rs")),
        None,
    );
}

// mask-out-system-no-header-pinyin: MATCH.
#[test]
fn mask_out_system_no_header_pinyin() {
    run_j(
        "system_mask_out::mask_out_system_no_header_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-system-no-header-pinyin"),
        raw!(include!("fixtures/system_mask_out/ae73ea73b3062131.rs")),
        None,
    );
}

// mask-out-system-saved-pinyin: MATCH.
#[test]
fn mask_out_system_saved_pinyin() {
    run_j(
        "system_mask_out::mask_out_system_saved_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-system-saved-pinyin"),
        raw!(include!("fixtures/system_mask_out/7fdd6a2d6469122b.rs")),
        None,
    );
}

// mask-out-system-selected-add-pinyin: MATCH.
#[test]
fn mask_out_system_selected_add_pinyin() {
    run_j(
        "system_mask_out::mask_out_system_selected_add_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-system-selected-add-pinyin"),
        raw!(include!("fixtures/system_mask_out/c9787d72d31b20de.rs")),
        None,
    );
}

// mask-out-system-unsaved-pinyin: MATCH.
#[test]
fn mask_out_system_unsaved_pinyin() {
    run_j(
        "system_mask_out::mask_out_system_unsaved_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-system-unsaved-pinyin"),
        raw!(include!("fixtures/system_mask_out/b94559cd0b3a7f0a.rs")),
        None,
    );
}

// mask-out-unmappable-no-change-pinyin: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn mask_out_unmappable_no_change_pinyin() {
    run_j(
        "system_mask_out::mask_out_unmappable_no_change_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-unmappable-no-change-pinyin"),
        raw!(include!("fixtures/system_mask_out/8ebf8d8edf89c64c.rs")),
        Some(&[
            "files_unchanged",
            "line",
            "live_count_unchanged",
            "live_items_unchanged",
            "mask",
            "reopened_count_unchanged",
            "reopened_items_unchanged",
            "save",
        ]),
    );
}
