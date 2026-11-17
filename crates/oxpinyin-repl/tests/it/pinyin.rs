use crate::common::expected::*;
use crate::support::{
    OPEN_DATA_USER_CONF_FAILED, run_user_session, session_user_conf_files, session_user_files, w3,
};
use assert_fs::TempDir;
use rstest::rstest;

const PREFIX_PINYIN: &[u8] = b"prefix:pinyin:";

#[rstest]
// test-pinyin-eof: 074a2219 tests/test_pinyin.cpp:34-101;
// src/include/pinyin_utils.h:LineString.
#[case::test_pinyin_eof(
    w3(),
    &[],
    b"",
    Run {
        status: Exit(0),
        stdout: Exact(b"prefix:"),
        stderr: OPEN_DATA_USER_CONF_FAILED,
        files: session_user_conf_files(),
    },
)]
// test-pinyin-quit: 074a2219 tests/test_pinyin.cpp:34-101;
// src/include/pinyin_utils.h:LineString.
#[case::test_pinyin_quit(
    w3(),
    &["ignored", "--help"],
    b"\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact(PREFIX_PINYIN),
        stderr: OPEN_DATA_USER_CONF_FAILED,
        files: session_user_conf_files(),
    },
)]
// test-pinyin-session: 074a2219 tests/test_pinyin.cpp:34-101;
// src/include/pinyin_utils.h:LineString.
#[case::test_pinyin_session(
    w3(),
    &[],
    b"\nnihao\n\xe4\xbd\xa0\xe5\xa5\xbd\nzhongguo\n\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../oxpinyin-utils/tests/fixtures/test_pinyin/nihao-zhongguo-session.txt")),
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
    run_user_session("pinyin", &directory, args, stdin, &expected);
}
