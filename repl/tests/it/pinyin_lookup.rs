use crate::common::expected::*;
use crate::support::{run_table_or_lookup, w3};
use assert_fs::TempDir;
use rstest::rstest;

#[rstest]
// pinyin-lookup-session: tests/lookup/test_pinyin_lookup.cpp:41-153;
// src/lookup/phonetic_lookup.h; tests/tests_helper.h:24-48 at 074a2219.
// The pin's main ignores argv (tests/lookup/test_pinyin_lookup.cpp:35-136) and breaks at quit (:76-78).
#[case::pinyin_lookup_session(
    w3(),
    &["ignored", "--help"],
    b"'ni''hao'\nni\0'hao\nni3hao3\nni'hao\nzhongguo\nxian\na\ninvalid\n\nquit\n",
    &[],
    &[],
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_pinyin_lookup/session.txt")),
        stderr: b"",
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
        "pinyin_lookup",
        &directory,
        args,
        stdin,
        directories,
        files,
        &expected,
    );
}
