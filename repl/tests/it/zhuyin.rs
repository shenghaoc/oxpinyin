use crate::common::expected::*;
use crate::support::{OPEN_DATA_USER_CONF_FAILED, run_user_session, session_user_files, w3};
use assert_fs::TempDir;
use rstest::rstest;

const NIHAO_LINE: &[u8] = "你好\n".as_bytes();

#[rstest]
// test-zhuyin-quit: 074a2219 tests/test_zhuyin.cpp:34-66;
// src/include/pinyin_utils.h:LineString.
#[case::test_zhuyin_quit(
    w3(),
    &["ignored", "--help"],
    b"quit\n",
    Run {
        status: Exit(0),
        stdout: Exact(b""),
        stderr: OPEN_DATA_USER_CONF_FAILED,
        files: &[],
    },
)]
// test-zhuyin-session: 074a2219 tests/test_zhuyin.cpp:34-66;
// src/include/pinyin_utils.h:LineString.
#[case::test_zhuyin_session(
    w3(),
    &[],
    b"su3cl3\n5j/1xj/2\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact(NIHAO_LINE),
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
    run_user_session("zhuyin", &directory, args, stdin, &expected);
}
