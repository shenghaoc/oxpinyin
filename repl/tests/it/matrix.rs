use crate::common::expected::*;
use crate::support::{run_table_or_lookup, w3};
use assert_fs::{TempDir, prelude::*};
use rstest::rstest;

const OPEN_ABSENT_BIN_FAILED: &[u8] = b"open absent.bin failed!\n";

fn missing_chunk() -> TempDir {
    let directory = w3();
    directory.child("data/table.conf").write_binary(b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault RESERVED NULL NULL NULL NOT_USED\ndefault GB_DICTIONARY gb_char.table absent.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\ndefault OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin SYSTEM_FILE\ndefault MERGED_DICTIONARY merged.table merged.bin merged.dbin SYSTEM_FILE\ndefault ADDON_DICTIONARY NULL NULL addon.bin USER_FILE\ndefault NETWORK_DICTIONARY NULL NULL network.bin USER_FILE\ndefault USER_DICTIONARY NULL NULL user.bin USER_FILE\n\naddon 4 art.table art.bin NULL DICTIONARY\naddon 5 culture.table culture.bin NULL DICTIONARY\naddon 6 economy.table economy.bin NULL DICTIONARY\naddon 7 geology.table geology.bin NULL DICTIONARY\naddon 8 history.table history.bin NULL DICTIONARY\n\naddon 9 life.table life.bin NULL DICTIONARY\naddon 10 nature.table nature.bin NULL DICTIONARY\naddon 11 people.table people.bin NULL DICTIONARY\naddon 12 science.table science.bin NULL DICTIONARY\naddon 13 society.table society.bin NULL DICTIONARY\naddon 14 sport.table sport.bin NULL DICTIONARY\naddon 15 technology.table technology.bin NULL DICTIONARY\n").expect("missing chunk configuration");
    directory
}

#[rstest]
// matrix-chunk-directory: tests/storage/test_matrix.cpp:93-179;
// src/storage/phonetic_key_matrix.cpp:29-84,226-437; tests/tests_helper.h:24-48 at
// 074a2219.
// Policy-accepted on every cell:
// docs/findings/upstream-divergences.md#utility-check-macros-write-no-message; only the pin
// writes the check macro's message.
#[case::matrix_chunk_directory(
    missing_chunk(),
    &[],
    b"quit\n",
    &["data/absent.bin"],
    &[("data/absent.bin/occupied", b"kept".as_slice())],
    Run {
        status: Signal(6),
        stdout: Exact(b""),
        stderr: b"",
        files: &[],
    },
)]
// matrix-missing-chunk: tests/storage/test_matrix.cpp:93-179;
// src/storage/phonetic_key_matrix.cpp:29-84,226-437; tests/tests_helper.h:24-48 at
// 074a2219.
#[case::matrix_missing_chunk(
    missing_chunk(),
    &[],
    b"quit\n",
    &[],
    &[],
    Run {
        status: Exit(2),
        stdout: Exact(b""),
        stderr: OPEN_ABSENT_BIN_FAILED,
        files: &[],
    },
)]
// matrix-session: tests/storage/test_matrix.cpp:93-179;
// src/storage/phonetic_key_matrix.cpp:29-84,226-437; tests/tests_helper.h:24-48 at
// 074a2219.
// The pin's main ignores argv (tests/storage/test_matrix.cpp:87-176) and breaks at quit (:116-118).
#[case::matrix_session(
    w3(),
    &["ignored", "--help"],
    b"'ni''hao'\nni\0'hao\nnihao\nni''hao\ncan\nxian\nbagn\nn\n\ninvalid\nquit\n",
    &[],
    &[],
    Run {
        status: Exit(0),
        stdout: Exact(include_bytes!("../../../utils/tests/fixtures/test_matrix/session.txt")),
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
        "matrix",
        &directory,
        args,
        stdin,
        directories,
        files,
        &expected,
    );
}
