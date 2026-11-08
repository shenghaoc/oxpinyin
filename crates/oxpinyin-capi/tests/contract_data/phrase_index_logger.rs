//! Phrase-index logger journals merged at init, mask-out and reload (inventory row 37, #525, #1175). Generated from the frozen pin observations.

use crate::bodies::phrase_index_logger as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};

// abort-init-log-header-token-later-directory-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_init_log_header_token_later_directory_pinyin() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "phrase_index_logger::abort_init_log_header_token_later_directory_pinyin",
        Expect::aborts(b"").with_files(&[
            (
                "gb_char.dbin",
                UserFile::Chunk(
                    26,
                    "91b0abb16fcee2f53cd1267bd8b823bf2d730651f7b6687a007166dcfabdd48d",
                ),
            ),
            ("user.conf", UserFile::Conf(USER_CONF)),
        ]),
        |k| {
            b::case(k, "abort-init-log-header-token-later-directory-pinyin");
        },
    );
}

// abort-init-log-header-token-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_init_log_header_token_pinyin() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "phrase_index_logger::abort_init_log_header_token_pinyin",
        Expect::aborts(b"").with_files(&[
            (
                "gb_char.dbin",
                UserFile::Chunk(
                    26,
                    "91b0abb16fcee2f53cd1267bd8b823bf2d730651f7b6687a007166dcfabdd48d",
                ),
            ),
            ("user.conf", UserFile::Conf(USER_CONF)),
        ]),
        |k| {
            b::case(k, "abort-init-log-header-token-pinyin");
        },
    );
}

// abort-mask-out-log-header-token-later-directory-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_mask_out_log_header_token_later_directory_pinyin() {
    run_aborting(
        "phrase_index_logger::abort_mask_out_log_header_token_later_directory_pinyin",
        Expect::aborts(b""),
        |k| {
            b::case(k, "abort-mask-out-log-header-token-later-directory-pinyin");
        },
    );
}

// abort-mask-out-log-header-token-later-unmappable-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_mask_out_log_header_token_later_unmappable_pinyin() {
    run_aborting(
        "phrase_index_logger::abort_mask_out_log_header_token_later_unmappable_pinyin",
        Expect::aborts(b""),
        |k| {
            b::case(k, "abort-mask-out-log-header-token-later-unmappable-pinyin");
        },
    );
}

// abort-mask-out-log-header-token-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_mask_out_log_header_token_pinyin() {
    run_aborting(
        "phrase_index_logger::abort_mask_out_log_header_token_pinyin",
        Expect::aborts(b""),
        |k| {
            b::case(k, "abort-mask-out-log-header-token-pinyin");
        },
    );
}

// abort-mask-out-log-header-untouched-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_mask_out_log_header_untouched_pinyin() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "phrase_index_logger::abort_mask_out_log_header_untouched_pinyin",
        Expect::aborts(b"").with_files(&[
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
                    44,
                    "d4f8059b7b88691525af20bc1f56d255be0bc577031338419630a7ee20536653",
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
                    65,
                    "4c7750042b1c861c709973a4022d55c80ad8dac8d310140ac3cecb36f78e0bdc",
                ),
            ),
            ("user.conf", UserFile::Conf(USER_CONF)),
            ("user_bigram.db", UserFile::Size(4198400)),
            ("user_phrase_index.bin", UserFile::Size(529408)),
            ("user_pinyin_index.bin", UserFile::Size(529408)),
        ]),
        |k| {
            b::case(k, "abort-mask-out-log-header-untouched-pinyin");
        },
    );
}

// abort-mask-out-multiple-log-headers-bad-tail-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_mask_out_multiple_log_headers_pinyin() {
    run_aborting(
        "phrase_index_logger::abort_mask_out_multiple_log_headers_pinyin",
        Expect::aborts(b""),
        |k| {
            b::case(k, "abort-mask-out-multiple-log-headers-pinyin");
        },
    );
}

// abort-mask-out-second-library-multiple-log-headers-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_mask_out_second_library_multiple_log_headers_pinyin() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "phrase_index_logger::abort_mask_out_second_library_multiple_log_headers_pinyin",
        Expect::aborts(b"").with_files(&[
            (
                "gb_char.dbin",
                UserFile::Chunk(
                    26,
                    "ae4690cf21699b0e69b57d4dc824559e419a42ec2b31c0f2c2994cf323e64c5b",
                ),
            ),
            (
                "gbk_char.dbin",
                UserFile::Chunk(
                    44,
                    "d4f8059b7b88691525af20bc1f56d255be0bc577031338419630a7ee20536653",
                ),
            ),
            ("user.conf", UserFile::Conf(USER_CONF)),
        ]),
        |k| {
            b::case(
                k,
                "abort-mask-out-second-library-multiple-log-headers-pinyin",
            );
        },
    );
}

// abort-reload-log-header-token-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_reload_log_header_token_pinyin() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "phrase_index_logger::abort_reload_log_header_token_pinyin",
        Expect::aborts(b"").with_files(&[
            (
                "gbk_char.dbin",
                UserFile::Chunk(
                    26,
                    "0ef9a980cf57111faceb9adaf4788ced9e88915d8d8477cfa8bb1ee6500e6d89",
                ),
            ),
            ("user.conf", UserFile::Conf(USER_CONF)),
        ]),
        |k| {
            b::case(k, "abort-reload-log-header-token-pinyin");
        },
    );
}

// abort-reload-short-add-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_reload_short_add_pinyin() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "phrase_index_logger::abort_reload_short_add_pinyin",
        Expect::aborts(b"").with_files(&[
            (
                "gbk_char.dbin",
                UserFile::Chunk(
                    19,
                    "ac79f7f4d879e3cf14d4f538a8b3379dbf0934607549f655ca5b878c59798794",
                ),
            ),
            ("user.conf", UserFile::Conf(USER_CONF)),
        ]),
        |k| {
            b::case(k, "abort-reload-short-add-pinyin");
        },
    );
}

// init-multiple-log-headers-pinyin: MATCH.
#[test]
fn init_multiple_log_headers_pinyin() {
    run_j(
        "phrase_index_logger::init_multiple_log_headers_pinyin",
        Expect::RESULT,
        |k| b::case(k, "init-multiple-log-headers-pinyin"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// mask-out-log-header-bad-tail-pinyin: MATCH.
#[test]
fn mask_out_log_header_bad_tail_pinyin() {
    run_j(
        "phrase_index_logger::mask_out_log_header_bad_tail_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-log-header-bad-tail-pinyin"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// mask-out-log-kept-add-bad-tail-pinyin: MATCH.
#[test]
fn mask_out_log_kept_add_bad_tail_pinyin() {
    run_j(
        "phrase_index_logger::mask_out_log_kept_add_bad_tail_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-log-kept-add-bad-tail-pinyin"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// mask-out-second-library-other-bits-pinyin: MATCH.
#[test]
fn mask_out_second_library_other_bits_pinyin() {
    run_j(
        "phrase_index_logger::mask_out_second_library_other_bits_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-second-library-other-bits-pinyin"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// mask-out-unloaded-library-log-pinyin: MATCH.
#[test]
fn mask_out_unloaded_library_log_pinyin() {
    run_j(
        "phrase_index_logger::mask_out_unloaded_library_log_pinyin",
        Expect::RESULT,
        |k| b::case(k, "mask-out-unloaded-library-log-pinyin"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}
