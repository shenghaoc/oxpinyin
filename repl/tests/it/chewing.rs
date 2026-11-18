use crate::common::expected::*;
use crate::support::{
    OPEN_DATA_USER_CONF_FAILED, run_user_session, session_user_conf_files, session_user_files, w3,
};
use assert_fs::TempDir;
use rstest::rstest;

#[rstest]
// test-chewing-quit: 074a2219 tests/test_chewing.cpp:34-72;
// src/include/pinyin_utils.h:LineString.
#[case::test_chewing_quit(
    w3(),
    &["ignored", "--help"],
    b"quit\n",
    Run {
        status: Exit(0),
        stdout: Exact(b""),
        stderr: OPEN_DATA_USER_CONF_FAILED,
        files: session_user_conf_files(),
    },
)]
// test-chewing-session: 074a2219 tests/test_chewing.cpp:34-72;
// src/include/pinyin_utils.h:LineString.
#[case::test_chewing_session(
    w3(),
    &[],
    b"su3cl3\n5j/1xj/2\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_chewing/nihao-zhongguo-session.txt")),
        stderr: OPEN_DATA_USER_CONF_FAILED,
        files: session_user_files(),
    },
)]
fn captured_cases(
    #[case] directory: TempDir,
    #[case] args: &[&str],
    #[case] stdin: &[u8],
    #[case] expected: Run,
) {
    run_user_session("chewing", &directory, args, stdin, &expected);
}
