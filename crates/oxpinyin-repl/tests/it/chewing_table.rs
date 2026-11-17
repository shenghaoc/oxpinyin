use crate::common::expected::*;
use crate::support::{run_table_or_lookup, table_config, tables};
use assert_fs::TempDir;
use rstest::rstest;

const OPEN_GB_TABLE_FAILED: &[u8] = b"open gb.table failed!\n";

#[rstest]
// chewing-table-first-incomplete-untoned: tests/storage/test_chewing_table.cpp;
// tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
// src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
#[case::chewing_table_first_incomplete_untoned(
    tables(),
    &[],
    b"n'n4\nquit\n",
    &[],
    &[],
    Run {
        status: Exit(0),
        stdout: Exact(b"Spent <TIME> us for 1000 operations, <TIME> us/op, <TIME> times/s.\n\nreturn continued information with length:1\n"),
        stderr: b"",
        files: &[],
    },
)]
// chewing-table-invalid-text: tests/storage/test_chewing_table.cpp;
// tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
// src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
// Class (b), memory safety (docs/findings/compatibility-policy.md, row 115): the pin is not
// run, and the expectation is the prescribed result.
#[case::chewing_table_invalid_text(
    table_config(),
    &[],
    b"quit\n",
    &[],
    &[("data/gb.table", b"ni3 \xff 16777217 100\n".as_slice())],
    Run {
        status: Exit(22),
        stdout: Exact(b""),
        stderr: b"",
        files: &[],
    },
)]
// chewing-table-missing-text: tests/storage/test_chewing_table.cpp;
// tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
// src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
#[case::chewing_table_missing_text(
    table_config(),
    &[],
    b"",
    &[],
    &[],
    Run {
        status: Exit(2),
        stdout: Exact(b""),
        stderr: OPEN_GB_TABLE_FAILED,
        files: &[],
    },
)]
// chewing-table-overlong-toned-first: tests/storage/test_chewing_table.cpp;
// tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
// src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
// Policy-accepted on every cell:
// docs/findings/upstream-divergences.md#utility-check-macros-write-no-message; only the pin
// writes the check macro's message.
#[case::chewing_table_overlong_toned_first(
    tables(),
    &[],
    b"n4'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3\n",
    &[],
    &[],
    Run {
        status: Signal(6),
        stdout: Exact(b""),
        stderr: b"",
        files: &[],
    },
)]
// chewing-table-session: tests/storage/test_chewing_table.cpp;
// tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
// src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
// The pin's main ignores argv (tests/storage/test_chewing_table.cpp:34-113) and breaks at quit (:62-64).
#[case::chewing_table_session(
    tables(),
    &["--help", "ignored"],
    b"ni3'hao3\nni\nni3\nni2\nn\nhao3\nzzz\n\nquit\nni3\n",
    &[],
    &[],
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../oxpinyin-utils/tests/fixtures/test_chewing_table/session.txt")),
        stderr: b"Invalid input.\n",
        files: &[],
    },
)]
// chewing-table-zero-token: tests/storage/test_chewing_table.cpp;
// tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
// src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
#[case::chewing_table_zero_token(
    TempDir::new().unwrap(),
    &[],
    b"ni3\nquit\n",
    &["data"],
    &[("data/table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\ndefault RESERVED reserved.table reserved.bin reserved.dbin SYSTEM_FILE\n".as_slice()), ("data/reserved.table", b"ni3 \xe4\xbd\xa0 0 100\n".as_slice())],
    Run {
        status: Exit(0),
        stdout: Exact(b"Spent <TIME> us for 1000 operations, <TIME> us/op, <TIME> times/s.\n\n"),
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
        "chewing_table",
        &directory,
        args,
        stdin,
        directories,
        files,
        &expected,
    );
}
