use crate::common::expected::*;
use crate::support::{run_table_or_lookup, w3};
use assert_fs::TempDir;
use rstest::rstest;

const NI_HAO_TOKENS: &[u8] = "16781861 你\n16779429 好\n".as_bytes();

#[rstest]
// phrase-lookup-embedded-nul: tests/lookup/test_phrase_lookup.cpp:41-137;
// src/lookup/phrase_lookup.h:131-135; lookup.cpp:31-66; tests/tests_helper.h:24-48 at
// 074a2219.
#[case::phrase_lookup_embedded_nul(
    w3(),
    &[],
    b"\xe4\xbd\xa0\0\xe5\xa5\xbd\n",
    &[],
    &[],
    Run {
        status: Exit(0),
        stdout: Exact("16781861 你\n".as_bytes()),
        stderr: b"",
        files: &[],
    },
)]
// phrase-lookup-invalid-utf8: tests/lookup/test_phrase_lookup.cpp:41-137;
// src/lookup/phrase_lookup.h:131-135; lookup.cpp:31-66; tests/tests_helper.h:24-48 at
// 074a2219.
#[case::phrase_lookup_invalid_utf8(
    w3(),
    &[],
    b"\xff\n\xe4\xbd\xa0\xe5\xa5\xbd\n",
    &[],
    &[],
    Run {
        status: Exit(0),
        stdout: Exact(NI_HAO_TOKENS),
        stderr: b"non-ucs4 characters are not accepted.\n",
        files: &[],
    },
)]
// phrase-lookup-session: tests/lookup/test_phrase_lookup.cpp:41-137;
// src/lookup/phrase_lookup.h:131-135; lookup.cpp:31-66; tests/tests_helper.h:24-48 at
// 074a2219.
// The pin's main ignores argv (tests/lookup/test_phrase_lookup.cpp:58-114) and breaks at quit (:95-97).
#[case::phrase_lookup_session(
    w3(),
    &["ignored", "--help"],
    b"\xe4\xbd\xa0\xe5\xa5\xbd\n\xe4\xb8\xad\xe5\x9b\xbd\n\xe4\xbd\xa0\xe4\xbb\xac\xe5\xa5\xbd\n\xe9\x98\xbf\xe5\x95\x8a\n\xe4\xb8\x8d\xe5\xad\x98\xe5\x9c\xa8\n\nquit\n",
    &[],
    &[],
    Run {
        status: Exit(0),
        stdout: Exact("16781861 你\n16779429 好\n16817937 中国\n16802445 你们\n16779429 好\n16777219 阿\n16777220 啊\n".as_bytes()),
        stderr: b"Error: Un-segmentable sentence encountered!\nError: Un-segmentable sentence encountered!\n",
        files: &[],
    },
)]
fn captured_cases(
    #[case] directory: TempDir,
    #[case] args: &[&str],
    #[case] stdin: &[u8],
    #[case] directories: &[&str],
    #[case] files: &[(&str, &[u8])],
    #[case] expected: Run,
) {
    run_table_or_lookup(
        "phrase_lookup",
        &directory,
        args,
        stdin,
        directories,
        files,
        &expected,
    );
}
