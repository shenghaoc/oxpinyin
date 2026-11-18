use crate::common::expected::*;
use crate::support::run_parser;
use rstest::rstest;

#[rstest]
// parser2-bad-option: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44 at
// 074a2219.
#[case::parser2_bad_option(
    &["--bogus"],
    b"",
    Run {
        status: Exit(22),
        stdout: Exact(UNKNOWN_OPTION_BOGUS),
        stderr: b"",
        files: &[],
    },
)]
// parser2-dachen26: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44 at
// 074a2219.
#[case::parser2_dachen26(
    &["-p", "zhuyin", "-s", "dachen26"],
    b"b\nbb\nbbb\na\naa\naaa\nsu3cl3\n\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_parser2/dachen26-ri.txt")),
        stderr: b"",
        files: &[],
    },
)]
// parser2-double: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44 at
// 074a2219.
#[case::parser2_double(
    &["-p", "doublepinyin"],
    b"nihk\nni3hk3\nuang\nb\naaa\nni'hk\n\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_parser2/double-nihao.txt")),
        stderr: b"",
        files: &[],
    },
)]
// parser2-full: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44 at
// 074a2219.
// Every parser name other than the four known names selects the full pinyin parser.
#[case::parser2_full(
    &["--parser", "fullpinyin"],
    b"ni3hao3\nni''hao'\n'''\nxian\nxianan\nbagn\nn\nzh\nni hao\nNI\nni!hao\n\nquit\nnihao\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_parser2/full-nihao.txt")),
        stderr: b"",
        files: &[],
    },
)]
// parser2-full-incomplete: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44
// at 074a2219.
// --incomplete sets the pinyin and zhuyin incomplete bits for every parser;
// oxpinyin tests each parser with these bits.
#[case::parser2_full_incomplete(
    &["-p", "fullpinyin", "-i"],
    b"ni3hao3\nni''hao'\n'''\nxian\nxianan\nbagn\nn\nzh\nni hao\nNI\nni!hao\n\nquit\nnihao\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_parser2/full-nihao-incomplete.txt")),
        stderr: b"",
        files: &[],
    },
)]
// parser2-hsu: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44 at 074a2219.
#[case::parser2_hsu(
    &["-p", "zhuyin", "-s", "hsu"],
    b"hk\nny\nb\nj\nnyka\n\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_parser2/hsu-hang.txt")),
        stderr: b"",
        files: &[],
    },
)]
// parser2-ignored-args: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44 at
// 074a2219.
#[case::parser2_ignored_args(
    &["unread-file", "another-file"],
    b"nihao\n",
    Run {
        status: Exit(0),
        stdout: Exact(b"Spent <TIME> us for 1000 operations, <TIME> us/op, <TIME> times/s.\n\nparsed 5 chars, 2 keys.\nni 0 2\thao 2 5\t\n"),
        stderr: b"",
        files: &[],
    },
)]
// parser2-missing-parser: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44
// at 074a2219.
// A missing option value is one GLib refusal for every option.
#[case::parser2_missing_parser(
    &["--parser"],
    b"",
    Run {
        status: Exit(22),
        stdout: Exact(b"option parsing failed:Missing argument for --parser\n"),
        stderr: b"",
        files: &[],
    },
)]
// parser2-pinyindirect: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44 at
// 074a2219.
#[case::parser2_pinyindirect(
    &["-p", "pinyindirect"],
    b"ni3'' hao3  invalid\nbagn n\nnihao\nni'\nni6\n\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_parser2/pinyin-direct-nihao.txt")),
        stderr: b"",
        files: &[],
    },
)]
// parser2-quit-exact: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44 at
// 074a2219.
#[case::parser2_quit_exact(
    &[],
    b"quitx\nquit\nnihao\n",
    Run {
        status: Exit(0),
        stdout: Exact(b"Spent <TIME> us for 1000 operations, <TIME> us/op, <TIME> times/s.\n\nparsed 2 chars, 1 keys.\nqu 0 2\t\n"),
        stderr: b"",
        files: &[],
    },
)]
// parser2-standard: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44 at
// 074a2219.
#[case::parser2_standard(
    &["-p", "zhuyin", "-s", "standard"],
    b"su3cl3\nsu cl\n1\n5j/\nli\nju\n\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_parser2/standard-nihao.txt")),
        stderr: b"",
        files: &[],
    },
)]
// parser2-unknown-scheme: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44
// at 074a2219.
// The zhuyin parser with no known scheme also selects the full pinyin parser.
#[case::parser2_unknown_scheme(
    &["--parser", "zhuyin", "--scheme", "unknown"],
    b"ni3hao3\nni''hao'\n'''\nxian\nxianan\nbagn\nn\nzh\nni hao\nNI\nni!hao\n\nquit\nnihao\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_parser2/full-nihao.txt")),
        stderr: b"",
        files: &[],
    },
)]
// parser2-zhuyindirect: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44 at
// 074a2219.
#[case::parser2_zhuyindirect(
    &["-p", "zhuyindirect"],
    b"\xe3\x84\x8b\xe3\x84\xa7\xcb\x87' \xe3\x84\x8f\xe3\x84\xa0\xcb\x87  bad\n\xe3\x84\x8b\xe3\x84\xa7 \xe3\x84\x8f\xe3\x84\xa0\n\xe3\x84\x85\n\xe3\x84\x8b\xe3\x84\xa7\xcb\x8a  \n\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_parser2/zhuyin-direct-nihao.txt")),
        stderr: b"",
        files: &[],
    },
)]
// parser2-zhuyindirect-incomplete: tests/storage/test_parser2.cpp:37-147;
// tests/timer.h:30-44 at 074a2219.
#[case::parser2_zhuyindirect_incomplete(
    &["-p", "zhuyindirect", "--incomplete"],
    b"\xe3\x84\x8b\xe3\x84\xa7\xcb\x87' \xe3\x84\x8f\xe3\x84\xa0\xcb\x87  bad\n\xe3\x84\x8b\xe3\x84\xa7 \xe3\x84\x8f\xe3\x84\xa0\n\xe3\x84\x85\n\xe3\x84\x8b\xe3\x84\xa7\xcb\x8a  \n\nquit\n",
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_parser2/zhuyin-direct-nihao-incomplete.txt")),
        stderr: b"",
        files: &[],
    },
)]
fn captured_cases(#[case] args: &[&str], #[case] stdin: &[u8], #[case] expected: Run) {
    run_parser(args, stdin, &expected);
}
