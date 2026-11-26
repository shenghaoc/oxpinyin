use crate::common::expected::*;
use crate::support::{OPEN_DATA_USER_CONF_FAILED, run_user_session, session_user_conf_files, w3};
use assert_fs::TempDir;
use rstest::rstest;

#[rstest]
// test-phrase-session: 074a2219 tests/test_phrase.cpp:34-72;
// src/include/pinyin_utils.h:LineString.
// The pin's main ignores argv (tests/test_phrase.cpp:34-73) and breaks at quit (:41-43).
#[case::test_phrase_session(
    w3(),
    &["ignored", "--help"],
    b"\xe4\xbd\xa0\xe5\xa5\xbd\xe4\xb8\xad\xe5\x9b\xbd\n\xe4\xb8\xad\xe5\x9b\xbd\xe4\xbd\xa0\xe5\xa5\xbd\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact("你\t好\t中国\t\n中国\t你\t好\t\n".as_bytes()),
        stderr: OPEN_DATA_USER_CONF_FAILED,
        files: session_user_conf_files(),
    },
)]
fn captured_cases(
    #[case] directory: TempDir,
    #[case] args: &[&str],
    #[case] stdin: &[u8],
    #[case] expected: Run,
) {
    run_user_session("phrase", &directory, args, stdin, &expected);
}
