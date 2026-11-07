//! The import iterator's phrase-table asserts (#525, register rows 93 and 104). Generated from the frozen pin observations.

use crate::bodies::duplicate_phrase_add as b;
use crate::support::{Expect, UserFile, run_aborting};

// abort-add-phrase-duplicate-library-token: POLICY-ACCEPTED (pin-abort-assert-line.md).
// The pin asserts at pinyin.cpp:554.
// Reproduced abort; the pin also wrote glibc's assertion line, which oxpinyin does not
// (pin-abort-assert-line.md).
#[test]
fn abort_add_phrase_duplicate_library_token() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "duplicate_phrase_add::abort_add_phrase_duplicate_library_token",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[
            ("user.conf", UserFile::Conf(USER_CONF)),
            ("user_phrase_index.bin", UserFile::Size(529408)),
        ]),
        b::duplicate_library_token,
    );
}

// abort-add-phrase-index-length-mismatch: POLICY-ACCEPTED (pin-abort-assert-line.md).
// The pin asserts at pinyin.cpp:568.
// Reproduced abort; the pin also wrote glibc's assertion line, which oxpinyin does not
// (pin-abort-assert-line.md).
#[test]
fn abort_add_phrase_index_length_mismatch() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "duplicate_phrase_add::abort_add_phrase_index_length_mismatch",
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
            (
                "user.bin",
                UserFile::Chunk(
                    71,
                    "12f12326344fe39aa70f61d35321f14867bb11ed30c3d65e9c32a4abbb72884e",
                ),
            ),
            ("user.conf", UserFile::Conf(USER_CONF)),
            ("user_bigram.db", UserFile::Size(4198400)),
            ("user_phrase_index.bin", UserFile::Size(529408)),
            ("user_pinyin_index.bin", UserFile::Size(529408)),
        ]),
        b::index_length_mismatch,
    );
}

// abort-add-phrase-index-item-missing: POLICY-ACCEPTED (pin-abort-assert-line.md).
// The pin asserts at pinyin.cpp:568.
// Reproduced abort; the pin also wrote glibc's assertion line, which oxpinyin does not
// (pin-abort-assert-line.md).
#[test]
fn abort_add_phrase_index_item_missing() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "duplicate_phrase_add::abort_add_phrase_index_item_missing",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        })
        .with_files(&[
            ("user.conf", UserFile::Conf(USER_CONF)),
            ("user_phrase_index.bin", UserFile::Size(529408)),
        ]),
        b::index_item_missing,
    );
}

// abort-add-phrase-index-text-mismatch: POLICY-ACCEPTED (pin-abort-assert-line.md).
// The pin asserts at pinyin.cpp:571.
// Reproduced abort; the pin also wrote glibc's assertion line, which oxpinyin does not
// (pin-abort-assert-line.md).
#[test]
fn abort_add_phrase_index_text_mismatch() {
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "duplicate_phrase_add::abort_add_phrase_index_text_mismatch",
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
            (
                "user.bin",
                UserFile::Chunk(
                    65,
                    "a078b68d0bd994817bedf79ca9d3fa61a60953cd48f168693a9916316387ba3c",
                ),
            ),
            ("user.conf", UserFile::Conf(USER_CONF)),
            ("user_bigram.db", UserFile::Size(4198400)),
            ("user_phrase_index.bin", UserFile::Size(529408)),
            ("user_pinyin_index.bin", UserFile::Size(529408)),
        ]),
        b::index_text_mismatch,
    );
}
