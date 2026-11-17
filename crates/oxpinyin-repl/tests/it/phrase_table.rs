use crate::common::expected::*;
use crate::support::{run_table_or_lookup, tables};
use assert_fs::TempDir;
use rstest::rstest;

const PHRASE_TABLE_SESSION: &[u8] = b"Spent <TIME> us for 1000 operations, <TIME> us/op, <TIME> times/s.\n\ntoken:16777217\ttoken:33554433\t\n";

#[rstest]
// phrase-table-embedded-nul: tests/storage/test_phrase_table.cpp;
// tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
// src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
#[case::phrase_table_embedded_nul(
    tables(),
    &[],
    b"\xe4\xbd\xa0\0\xe5\xa5\xbd\n",
    &[],
    &[],
    Run {
        status: Exit(0),
        stdout: Exact(PHRASE_TABLE_SESSION),
        stderr: b"",
        files: &[],
    },
)]
// phrase-table-session: tests/storage/test_phrase_table.cpp;
// tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
// src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
// The pin's main ignores argv (tests/storage/test_phrase_table.cpp:14-99) and breaks at quit (:41-43).
#[case::phrase_table_session(
    tables(),
    &["--help", "ignored"],
    b"\xe4\xbd\xa0\xe5\xa5\xbd\n\xe4\xbd\xa0\n\xe4\xbd\xa0\xe5\xa5\xbd\xe5\x90\x97\n\xe5\xa5\xbd\n\xe6\x97\xa0\n\xe7\xa5\xa2\n\nquit\n\xe4\xbd\xa0\n",
    &[],
    &[],
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../oxpinyin-utils/tests/fixtures/test_phrase_table/session.txt")),
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
        "phrase_table",
        &directory,
        args,
        stdin,
        directories,
        files,
        &expected,
    );
}
