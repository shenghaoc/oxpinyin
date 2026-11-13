//! Integration witnesses of the retained REPL's product routes. Pin 074a2219.
//! Frozen product outputs and persistent files remain independent expectations;
//! obsolete timing output and fixed working-directory conventions are excluded.
#[cfg(unix)]
#[path = "../../oxpinyin-utils/tests/common/mod.rs"]
mod common;
#[cfg(unix)]
#[path = "../../oxpinyin-utils/tests/fixtures/mod.rs"]
mod fixtures;
#[cfg(unix)]
mod product {
    use crate::common::CELL;
    use crate::common::expected::*;

    const EMPTY_PHRASE_CHUNK: File = Sha256(
        "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697",
        27,
    );
    const KC_EMPTY_STASH_DB: File = Sha256(
        "574563c29dbe594eafe2e30ff1ac1e3fb92fed42bbffc40fa1090fb5270b2f05",
        7,
    );
    const OPEN_ABSENT_BIN_FAILED: &[u8] = b"open absent.bin failed!\n";
    const OPEN_DATA_USER_CONF_FAILED: &[u8] = b"open ../data/user.conf failed.";
    const OPEN_GB_TABLE_FAILED: &[u8] = b"open gb.table failed!\n";
    const SESSION_GBK_CHAR_DBIN: File = Sha256(
        "c8399d91646c86e97b1cb2020c0668279e9ab2a674ae56a25016b72d36d19c6b",
        26,
    );
    const SESSION_GB_CHAR_DBIN: File = Sha256(
        "3b56b0634e338a0482092a117849be21ab4f39133201bf1fb2183d9485502537",
        26,
    );
    const SESSION_MERGED_DBIN: File = Sha256(
        "30923d4c45a569a8cfcb0a220f3e91b83904ce40af34145848ea46f0ef0f029a",
        26,
    );
    const SESSION_OPENGRAM_DBIN: File = Sha256(
        "2693d260ef5db322d24b8b3c2fd1776f10c497775a5855cde349202f41b96f61",
        26,
    );
    const USER_CONF_BERKELEY_DB: File = Sha256(
        "fbcac8c49db56a5a5a6ae546bf18bbbdc381a51629e4efdfd2c24b5dd4c7ec8a",
        88,
    );
    const USER_CONF_KYOTO_CABINET: File = Sha256(
        "ec73ecc4d02a875e4448a47d7051e5383bf43107b3386ecec12a82c94c5891f7",
        90,
    );
    const USER_CONF_TKRZW: File = Sha256(
        "38f302279f5500e5cd049f0d0230eaabc4c1b23ecdbb7b8bb57a088e203231f9",
        83,
    );
    fn session_user_conf_files() -> &'static [(&'static str, File)] {
        match CELL {
            "bdb" => &[("data/user.conf", USER_CONF_BERKELEY_DB)],
            "kc" => &[("data/user.conf", USER_CONF_KYOTO_CABINET)],
            "tkrzw" => &[("data/user.conf", USER_CONF_TKRZW)],
            _ => unreachable!("unknown cell {CELL}"),
        }
    }
    fn session_user_files() -> &'static [(&'static str, File)] {
        match CELL {
            "bdb" => &[
                ("data/addon.bin", EMPTY_PHRASE_CHUNK),
                ("data/gb_char.dbin", SESSION_GB_CHAR_DBIN),
                ("data/gbk_char.dbin", SESSION_GBK_CHAR_DBIN),
                ("data/merged.dbin", SESSION_MERGED_DBIN),
                ("data/network.bin", EMPTY_PHRASE_CHUNK),
                ("data/opengram.dbin", SESSION_OPENGRAM_DBIN),
                ("data/user.bin", EMPTY_PHRASE_CHUNK),
                ("data/user.conf", USER_CONF_BERKELEY_DB),
                ("data/user_bigram.db", NO_ROWS),
                ("data/user_phrase_index.bin", NO_ROWS),
                ("data/user_pinyin_index.bin", NO_ROWS),
            ],
            "kc" => &[
                ("data/addon.bin", EMPTY_PHRASE_CHUNK),
                ("data/gb_char.dbin", SESSION_GB_CHAR_DBIN),
                ("data/gbk_char.dbin", SESSION_GBK_CHAR_DBIN),
                ("data/merged.dbin", SESSION_MERGED_DBIN),
                ("data/network.bin", EMPTY_PHRASE_CHUNK),
                ("data/opengram.dbin", SESSION_OPENGRAM_DBIN),
                ("data/user.bin", EMPTY_PHRASE_CHUNK),
                ("data/user.conf", USER_CONF_KYOTO_CABINET),
                ("data/user_bigram.db", KC_EMPTY_STASH_DB),
                ("data/user_phrase_index.bin", KC_EMPTY_STASH_DB),
                ("data/user_pinyin_index.bin", KC_EMPTY_STASH_DB),
            ],
            "tkrzw" => &[
                ("data/addon.bin", EMPTY_PHRASE_CHUNK),
                ("data/gb_char.dbin", SESSION_GB_CHAR_DBIN),
                ("data/gbk_char.dbin", SESSION_GBK_CHAR_DBIN),
                ("data/merged.dbin", SESSION_MERGED_DBIN),
                ("data/network.bin", EMPTY_PHRASE_CHUNK),
                ("data/opengram.dbin", SESSION_OPENGRAM_DBIN),
                ("data/user.bin", EMPTY_PHRASE_CHUNK),
                ("data/user.conf", USER_CONF_TKRZW),
                ("data/user_bigram.db", NO_ROWS),
                ("data/user_phrase_index.bin", NO_ROWS),
                ("data/user_pinyin_index.bin", NO_ROWS),
            ],
            _ => unreachable!("unknown cell {CELL}"),
        }
    }

    mod test_chewing {
        use super::{OPEN_DATA_USER_CONF_FAILED, session_user_conf_files, session_user_files};
        use crate::common::expected::check_repl as check;
        use crate::common::expected::*;
        use crate::common::{CELL, inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::{fixture, rstest};
        use std::time::Duration;

        #[fixture]
        fn w3() -> TempDir {
            let data = crate::fixtures::w3();
            let directory = TempDir::new().expect("scratch directory");
            std::fs::rename(data.path(), directory.path().join("data"))
                .expect("nested data directory");
            directory
                .child("tests")
                .create_dir_all()
                .expect("working directory");
            directory
        }

        #[rstest]
        // test-chewing-embedded-nul: 074a2219 tests/test_chewing.cpp:34-72;
        // src/include/pinyin_utils.h:LineString.
        #[case::test_chewing_embedded_nul(
            w3(),
            &[],
            b"su3cl3\0ignored\nquit\n",
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_chewing/nihao-session.txt")),
                stderr: OPEN_DATA_USER_CONF_FAILED,
                files: session_user_files(),
            },
        )]
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
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_chewing/nihao-zhongguo-session.txt")),
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
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("oxpinyin-repl")
                .arg("chewing")
                .arg(directory.path().join("data"))
                .current_dir(directory.path().join("tests"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_chewing");
            output.stderr = mask_glib_prefix(&output.stderr);
            // The REPL takes an explicit isolated data directory, unlike the
            // old driver's fixed ../data spelling. Keep the diagnostic payload.
            output.stderr = crate::common::replace_directory(
                &output.stderr,
                &directory.path().join("data"),
                b"../data",
            );
            check(
                output,
                &inventory(
                    directory.path(),
                    &inputs,
                    if ["bdb", "tkrzw"].contains(&CELL) {
                        &[
                            "data/user_bigram.db",
                            "data/user_pinyin_index.bin",
                            "data/user_phrase_index.bin",
                        ]
                    } else {
                        &[]
                    },
                    &[],
                    &[],
                ),
                &inputs,
                &expected,
            );
        }
    }
    mod test_chewing_table {
        use super::OPEN_GB_TABLE_FAILED;
        use crate::common::expected::check_repl as check;
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, mask_print_time, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::{fixture, rstest};
        use std::time::Duration;

        #[fixture]
        fn table_config() -> TempDir {
            let directory = TempDir::new().expect("scratch directory");
            directory
                .child("data")
                .create_dir_all()
                .expect("input directory");
            directory.child("data/table.conf").write_binary(b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\ndefault GB_DICTIONARY gb.table gb_char.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY gbk.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\n").expect("table configuration");
            directory
                .child("data/gbk.table")
                .write_binary(b"ni3 \xe4\xbd\xa0 33554433 200\nhao3 \xe5\xa5\xbd 33554434 100\n")
                .expect("GBK table");
            directory
        }

        #[fixture]
        fn tables() -> TempDir {
            let directory = table_config();
            directory.child("data/gb.table").write_binary(b"ni3 \xe4\xbd\xa0 16777217 100\nni2 \xe4\xbd\xa0 16777217 50\nni3'hao3 \xe4\xbd\xa0\xe5\xa5\xbd 16777218 80\nni3'hao3'ma5 \xe4\xbd\xa0\xe5\xa5\xbd\xe5\x90\x97 16777219 20\nni3 \xe7\xa5\xa2 16777221 40\n").expect("GB table");
            directory
        }

        #[rstest]
        // chewing-table-embedded-nul: tests/storage/test_chewing_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        #[case::chewing_table_embedded_nul(
            tables(),
            &[],
            b"ni3\0hao3\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_chewing_table/two-range-lookup.txt")),
                stderr: b"",
                files: &[],
            },
        )]
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
        // chewing-table-missing-config: tests/storage/test_chewing_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
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
        // chewing-table-overlong-query: tests/storage/test_chewing_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        // Class (b), memory safety (docs/findings/compatibility-policy.md, row 116): the pin is not
        // run, and the expectation is the prescribed result.
        #[case::chewing_table_overlong_query(
            tables(),
            &[],
            b"ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3'ni3\n",
            &[],
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(b""),
                stderr: b"",
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
        // chewing-table-quit: tests/storage/test_chewing_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        #[case::chewing_table_quit(
            tables(),
            &["--help", "ignored"],
            b"quit\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // chewing-table-session: tests/storage/test_chewing_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        #[case::chewing_table_session(
            tables(),
            &[],
            b"ni3'hao3\nni\nni3\nni2\nn\nhao3\nzzz\n\nquit\nignored\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_chewing_table/session.txt")),
                stderr: b"Invalid input.\n",
                files: &[],
            },
        )]
        // chewing-table-toned-initial: tests/storage/test_chewing_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        // Policy-accepted on every cell:
        // docs/findings/upstream-divergences.md#utility-check-macros-write-no-message; only the pin
        // writes the check macro's message.
        #[case::chewing_table_toned_initial(
            tables(),
            &[],
            b"n4\n",
            &[],
            &[],
            Run {
                status: Signal(6),
                stdout: Exact(b""),
                stderr: b"",
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
            directory
                .child("tests/storage")
                .create_dir_all()
                .expect("working directory");
            for name in directories {
                directory
                    .child(name)
                    .create_dir_all()
                    .expect("input directory");
            }
            for (name, bytes) in files {
                directory
                    .child(name)
                    .write_binary(bytes)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("oxpinyin-repl")
                .arg("chewing_table")
                .arg(directory.path().join("data"))
                .current_dir(directory.path().join("tests/storage"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_chewing_table");
            output.stdout = mask_print_time(&output.stdout);
            output.stderr = mask_glib_prefix(&output.stderr);
            // The REPL takes an explicit isolated data directory, unlike the
            // old driver's fixed ../data spelling. Keep the diagnostic payload.
            output.stderr = crate::common::replace_directory(
                &output.stderr,
                &directory.path().join("data"),
                b"../data",
            );
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_matrix {
        use super::OPEN_ABSENT_BIN_FAILED;
        use crate::common::expected::check_repl as check;
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, mask_print_time, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::{fixture, rstest};
        use std::time::Duration;

        #[fixture]
        fn w3() -> TempDir {
            let data = crate::fixtures::w3();
            let directory = TempDir::new().expect("scratch directory");
            std::fs::rename(data.path(), directory.path().join("data"))
                .expect("nested data directory");
            directory
        }

        #[fixture]
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
        // matrix-embedded-nul: tests/storage/test_matrix.cpp:93-179;
        // src/storage/phonetic_key_matrix.cpp:29-84,226-437; tests/tests_helper.h:24-48 at
        // 074a2219.
        #[case::matrix_embedded_nul(
            w3(),
            &[],
            b"ni\0hao\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_matrix/six-char-one-key-matrix.txt")),
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
        // matrix-missing-config: tests/storage/test_matrix.cpp:93-179;
        // src/storage/phonetic_key_matrix.cpp:29-84,226-437; tests/tests_helper.h:24-48 at
        // 074a2219.
        // matrix-quit: tests/storage/test_matrix.cpp:93-179;
        // src/storage/phonetic_key_matrix.cpp:29-84,226-437; tests/tests_helper.h:24-48 at
        // 074a2219.
        #[case::matrix_quit(
            w3(),
            &["ignored", "--help"],
            b"quit\nignored\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // matrix-session: tests/storage/test_matrix.cpp:93-179;
        // src/storage/phonetic_key_matrix.cpp:29-84,226-437; tests/tests_helper.h:24-48 at
        // 074a2219.
        #[case::matrix_session(
            w3(),
            &[],
            b"'ni''hao'\nni\0'hao\nnihao\nni''hao\ncan\nxian\nbagn\nn\n\ninvalid\nquit\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_matrix/session.txt")),
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
            directory
                .child("tests/storage")
                .create_dir_all()
                .expect("working directory");
            for name in directories {
                directory
                    .child(name)
                    .create_dir_all()
                    .expect("input directory");
            }
            for (name, bytes) in files {
                directory
                    .child(name)
                    .write_binary(bytes)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("oxpinyin-repl")
                .arg("matrix")
                .arg(directory.path().join("data"))
                .current_dir(directory.path().join("tests/storage"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_matrix");
            output.stdout = mask_print_time(&output.stdout);
            output.stderr = mask_glib_prefix(&output.stderr);
            // The REPL takes an explicit isolated data directory, unlike the
            // old driver's fixed ../data spelling. Keep the diagnostic payload.
            output.stderr = crate::common::replace_directory(
                &output.stderr,
                &directory.path().join("data"),
                b"../data",
            );
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_parser2 {
        use crate::common::expected::check_repl as check;
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, mask_print_time, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::TempDir;
        use rstest::rstest;
        use std::time::Duration;

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
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_parser2/dachen26-ri.txt")),
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
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_parser2/double-nihao.txt")),
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
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_parser2/full-nihao.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // parser2-full-incomplete: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44
        // at 074a2219.
        // --incomplete sets the pinyin and zhuyin incomplete bits for every parser;
        // oxpinyin-core tests each parser with these bits.
        #[case::parser2_full_incomplete(
            &["-p", "fullpinyin", "-i"],
            b"ni3hao3\nni''hao'\n'''\nxian\nxianan\nbagn\nn\nzh\nni hao\nNI\nni!hao\n\nquit\nnihao\n",
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_parser2/full-nihao-incomplete.txt")),
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
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_parser2/hsu-hang.txt")),
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
        // parser2-no-final-newline: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44
        // at 074a2219.
        #[case::parser2_no_final_newline(
            &[],
            b"ni3hao3",
            Run {
                status: Exit(0),
                stdout: Exact(b"Spent <TIME> us for 1000 operations, <TIME> us/op, <TIME> times/s.\n\nparsed 7 chars, 2 keys.\nni3 0 3\thao3 3 7\t\n"),
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
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_parser2/pinyin-direct-nihao.txt")),
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
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_parser2/standard-nihao.txt")),
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
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_parser2/full-nihao.txt")),
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
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_parser2/zhuyin-direct-nihao.txt")),
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
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_parser2/zhuyin-direct-nihao-incomplete.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        fn captured_cases(#[case] args: &[&str], #[case] stdin: &[u8], #[case] expected: Run) {
            let directory = TempDir::new().expect("scratch directory");
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("oxpinyin-repl")
                .arg("parser2")
                .arg(directory.path())
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_parser2");
            output.stdout = mask_print_time(&output.stdout);
            output.stderr = mask_glib_prefix(&output.stderr);
            // The REPL takes an explicit isolated data directory, unlike the
            // old driver's fixed ../data spelling. Keep the diagnostic payload.
            output.stderr = crate::common::replace_directory(
                &output.stderr,
                &directory.path().join("data"),
                b"../data",
            );
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_phrase {
        use super::{OPEN_DATA_USER_CONF_FAILED, session_user_conf_files};
        use crate::common::expected::check_repl as check;
        use crate::common::expected::*;
        use crate::common::{CELL, inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::{fixture, rstest};
        use std::time::Duration;

        const NI_HAO_TABBED: &[u8] = "你\t好\t\n".as_bytes();

        #[fixture]
        fn w3() -> TempDir {
            let data = crate::fixtures::w3();
            let directory = TempDir::new().expect("scratch directory");
            std::fs::rename(data.path(), directory.path().join("data"))
                .expect("nested data directory");
            directory
                .child("tests")
                .create_dir_all()
                .expect("working directory");
            directory
        }

        #[rstest]
        // test-phrase-embedded-nul: 074a2219 tests/test_phrase.cpp:34-72;
        // src/include/pinyin_utils.h:LineString.
        #[case::test_phrase_embedded_nul(
            w3(),
            &[],
            b"\xe4\xbd\xa0\xe5\xa5\xbd\0\xe4\xb8\xad\xe5\x9b\xbd\nquit\n",
            Run {
                status: Exit(0),
                stdout: Exact(NI_HAO_TABBED),
                stderr: OPEN_DATA_USER_CONF_FAILED,
                files: session_user_conf_files(),
            },
        )]
        // test-phrase-quit: 074a2219 tests/test_phrase.cpp:34-72;
        // src/include/pinyin_utils.h:LineString.
        #[case::test_phrase_quit(
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
        // test-phrase-session: 074a2219 tests/test_phrase.cpp:34-72;
        // src/include/pinyin_utils.h:LineString.
        #[case::test_phrase_session(
            w3(),
            &[],
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
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("oxpinyin-repl")
                .arg("phrase")
                .arg(directory.path().join("data"))
                .current_dir(directory.path().join("tests"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_phrase");
            output.stderr = mask_glib_prefix(&output.stderr);
            // The REPL takes an explicit isolated data directory, unlike the
            // old driver's fixed ../data spelling. Keep the diagnostic payload.
            output.stderr = crate::common::replace_directory(
                &output.stderr,
                &directory.path().join("data"),
                b"../data",
            );
            check(
                output,
                &inventory(
                    directory.path(),
                    &inputs,
                    if ["bdb", "tkrzw"].contains(&CELL) {
                        &[
                            "data/user_bigram.db",
                            "data/user_pinyin_index.bin",
                            "data/user_phrase_index.bin",
                        ]
                    } else {
                        &[]
                    },
                    &[],
                    &[],
                ),
                &inputs,
                &expected,
            );
        }
    }
    mod test_phrase_lookup {
        use super::OPEN_ABSENT_BIN_FAILED;
        use crate::common::expected::check_repl as check;
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::{fixture, rstest};
        use std::time::Duration;

        const NI_HAO_TOKENS: &[u8] = "16781861 你\n16779429 好\n".as_bytes();

        #[fixture]
        fn w3() -> TempDir {
            let data = crate::fixtures::w3();
            let directory = TempDir::new().expect("scratch directory");
            std::fs::rename(data.path(), directory.path().join("data"))
                .expect("nested data directory");
            directory
        }

        #[fixture]
        fn missing_chunk() -> TempDir {
            let directory = w3();
            directory.child("data/table.conf").write_binary(b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault RESERVED NULL NULL NULL NOT_USED\ndefault GB_DICTIONARY gb_char.table absent.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\ndefault OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin SYSTEM_FILE\ndefault MERGED_DICTIONARY merged.table merged.bin merged.dbin SYSTEM_FILE\ndefault ADDON_DICTIONARY NULL NULL addon.bin USER_FILE\ndefault NETWORK_DICTIONARY NULL NULL network.bin USER_FILE\ndefault USER_DICTIONARY NULL NULL user.bin USER_FILE\n\naddon 4 art.table art.bin NULL DICTIONARY\naddon 5 culture.table culture.bin NULL DICTIONARY\naddon 6 economy.table economy.bin NULL DICTIONARY\naddon 7 geology.table geology.bin NULL DICTIONARY\naddon 8 history.table history.bin NULL DICTIONARY\n\naddon 9 life.table life.bin NULL DICTIONARY\naddon 10 nature.table nature.bin NULL DICTIONARY\naddon 11 people.table people.bin NULL DICTIONARY\naddon 12 science.table science.bin NULL DICTIONARY\naddon 13 society.table society.bin NULL DICTIONARY\naddon 14 sport.table sport.bin NULL DICTIONARY\naddon 15 technology.table technology.bin NULL DICTIONARY\n").expect("missing chunk configuration");
            directory
        }

        #[rstest]
        // phrase-lookup-chunk-directory: tests/lookup/test_phrase_lookup.cpp:41-137;
        // src/lookup/phrase_lookup.h:131-135; lookup.cpp:31-66; tests/tests_helper.h:24-48 at
        // 074a2219.
        // Policy-accepted on every cell:
        // docs/findings/upstream-divergences.md#utility-check-macros-write-no-message; only the pin
        // writes the check macro's message.
        #[case::phrase_lookup_chunk_directory(
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
        // phrase-lookup-missing-chunk: tests/lookup/test_phrase_lookup.cpp:41-137;
        // src/lookup/phrase_lookup.h:131-135; lookup.cpp:31-66; tests/tests_helper.h:24-48 at
        // 074a2219.
        #[case::phrase_lookup_missing_chunk(
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
        // phrase-lookup-missing-config: tests/lookup/test_phrase_lookup.cpp:41-137;
        // src/lookup/phrase_lookup.h:131-135; lookup.cpp:31-66; tests/tests_helper.h:24-48 at
        // 074a2219.
        // phrase-lookup-quit: tests/lookup/test_phrase_lookup.cpp:41-137;
        // src/lookup/phrase_lookup.h:131-135; lookup.cpp:31-66; tests/tests_helper.h:24-48 at
        // 074a2219.
        #[case::phrase_lookup_quit(
            w3(),
            &["ignored", "--help"],
            b"quit\nignored\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // phrase-lookup-session: tests/lookup/test_phrase_lookup.cpp:41-137;
        // src/lookup/phrase_lookup.h:131-135; lookup.cpp:31-66; tests/tests_helper.h:24-48 at
        // 074a2219.
        #[case::phrase_lookup_session(
            w3(),
            &[],
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
            directory
                .child("tests/lookup")
                .create_dir_all()
                .expect("working directory");
            for name in directories {
                directory
                    .child(name)
                    .create_dir_all()
                    .expect("input directory");
            }
            for (name, bytes) in files {
                directory
                    .child(name)
                    .write_binary(bytes)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("oxpinyin-repl")
                .arg("phrase_lookup")
                .arg(directory.path().join("data"))
                .current_dir(directory.path().join("tests/lookup"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_phrase_lookup");
            output.stderr = mask_glib_prefix(&output.stderr);
            // The REPL takes an explicit isolated data directory, unlike the
            // old driver's fixed ../data spelling. Keep the diagnostic payload.
            output.stderr = crate::common::replace_directory(
                &output.stderr,
                &directory.path().join("data"),
                b"../data",
            );
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_phrase_table {
        use super::OPEN_GB_TABLE_FAILED;
        use crate::common::expected::check_repl as check;
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, mask_print_time, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::{fixture, rstest};
        use std::time::Duration;

        const PHRASE_TABLE_SESSION: &[u8] = b"Spent <TIME> us for 1000 operations, <TIME> us/op, <TIME> times/s.\n\ntoken:16777217\ttoken:33554433\t\n";

        #[fixture]
        fn table_config() -> TempDir {
            let directory = TempDir::new().expect("scratch directory");
            directory
                .child("data")
                .create_dir_all()
                .expect("input directory");
            directory.child("data/table.conf").write_binary(b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\ndefault GB_DICTIONARY gb.table gb_char.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY gbk.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\n").expect("table configuration");
            directory
                .child("data/gbk.table")
                .write_binary(b"ni3 \xe4\xbd\xa0 33554433 200\nhao3 \xe5\xa5\xbd 33554434 100\n")
                .expect("GBK table");
            directory
        }

        #[fixture]
        fn tables() -> TempDir {
            let directory = table_config();
            directory.child("data/gb.table").write_binary(b"ni3 \xe4\xbd\xa0 16777217 100\nni2 \xe4\xbd\xa0 16777217 50\nni3'hao3 \xe4\xbd\xa0\xe5\xa5\xbd 16777218 80\nni3'hao3'ma5 \xe4\xbd\xa0\xe5\xa5\xbd\xe5\x90\x97 16777219 20\nni3 \xe7\xa5\xa2 16777221 40\n").expect("GB table");
            directory
        }

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
        // phrase-table-invalid-text: tests/storage/test_phrase_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        // Class (b), memory safety (docs/findings/compatibility-policy.md, row 115): the pin is not
        // run, and the expectation is the prescribed result.
        #[case::phrase_table_invalid_text(
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
        // phrase-table-invalid-utf8: tests/storage/test_phrase_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        // Class (b), memory safety (docs/findings/compatibility-policy.md, row 115): the pin is not
        // run, and the expectation is the prescribed result.
        #[case::phrase_table_invalid_utf8(
            tables(),
            &[],
            b"\xff\n",
            &[],
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // phrase-table-missing-config: tests/storage/test_phrase_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        // phrase-table-missing-text: tests/storage/test_phrase_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        #[case::phrase_table_missing_text(
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
        // phrase-table-quit: tests/storage/test_phrase_table.cpp; tests/tests_helper.h:51-133;
        // src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        #[case::phrase_table_quit(
            tables(),
            &["--help", "ignored"],
            b"quit\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // phrase-table-session: tests/storage/test_phrase_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        #[case::phrase_table_session(
            tables(),
            &[],
            b"\xe4\xbd\xa0\xe5\xa5\xbd\n\xe4\xbd\xa0\n\xe4\xbd\xa0\xe5\xa5\xbd\xe5\x90\x97\n\xe5\xa5\xbd\n\xe6\x97\xa0\n\xe7\xa5\xa2\n\nquit\nignored\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_phrase_table/session.txt")),
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
            directory
                .child("tests/storage")
                .create_dir_all()
                .expect("working directory");
            for name in directories {
                directory
                    .child(name)
                    .create_dir_all()
                    .expect("input directory");
            }
            for (name, bytes) in files {
                directory
                    .child(name)
                    .write_binary(bytes)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("oxpinyin-repl")
                .arg("phrase_table")
                .arg(directory.path().join("data"))
                .current_dir(directory.path().join("tests/storage"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_phrase_table");
            output.stdout = mask_print_time(&output.stdout);
            output.stderr = mask_glib_prefix(&output.stderr);
            // The REPL takes an explicit isolated data directory, unlike the
            // old driver's fixed ../data spelling. Keep the diagnostic payload.
            output.stderr = crate::common::replace_directory(
                &output.stderr,
                &directory.path().join("data"),
                b"../data",
            );
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_pinyin {
        use super::{OPEN_DATA_USER_CONF_FAILED, session_user_conf_files, session_user_files};
        use crate::common::expected::check_repl as check;
        use crate::common::expected::*;
        use crate::common::{CELL, inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::{fixture, rstest};
        use std::time::Duration;

        const PREFIX_PINYIN: &[u8] = b"prefix:pinyin:";

        #[fixture]
        fn w3() -> TempDir {
            let data = crate::fixtures::w3();
            let directory = TempDir::new().expect("scratch directory");
            std::fs::rename(data.path(), directory.path().join("data"))
                .expect("nested data directory");
            directory
                .child("tests")
                .create_dir_all()
                .expect("working directory");
            directory
        }

        #[rstest]
        // test-pinyin-embedded-nul: 074a2219 tests/test_pinyin.cpp:34-101;
        // src/include/pinyin_utils.h:LineString.
        #[case::test_pinyin_embedded_nul(
            w3(),
            &[],
            b"\nnihao\0ignored\n\nquit\n",
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_pinyin/nihao-session-embedded-nul.txt")),
                stderr: OPEN_DATA_USER_CONF_FAILED,
                files: session_user_files(),
            },
        )]
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
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_pinyin/nihao-zhongguo-session.txt")),
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
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("oxpinyin-repl")
                .arg("pinyin")
                .arg(directory.path().join("data"))
                .current_dir(directory.path().join("tests"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_pinyin");
            output.stderr = mask_glib_prefix(&output.stderr);
            // The REPL takes an explicit isolated data directory, unlike the
            // old driver's fixed ../data spelling. Keep the diagnostic payload.
            output.stderr = crate::common::replace_directory(
                &output.stderr,
                &directory.path().join("data"),
                b"../data",
            );
            check(
                output,
                &inventory(
                    directory.path(),
                    &inputs,
                    if ["bdb", "tkrzw"].contains(&CELL) {
                        &[
                            "data/user_bigram.db",
                            "data/user_pinyin_index.bin",
                            "data/user_phrase_index.bin",
                        ]
                    } else {
                        &[]
                    },
                    &[],
                    &[],
                ),
                &inputs,
                &expected,
            );
        }
    }
    mod test_pinyin_lookup {
        use super::OPEN_ABSENT_BIN_FAILED;
        use crate::common::expected::check_repl as check;
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, mask_print_time, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::{fixture, rstest};
        use std::time::Duration;

        #[fixture]
        fn w3() -> TempDir {
            let data = crate::fixtures::w3();
            let directory = TempDir::new().expect("scratch directory");
            std::fs::rename(data.path(), directory.path().join("data"))
                .expect("nested data directory");
            directory
        }

        #[fixture]
        fn missing_chunk() -> TempDir {
            let directory = w3();
            directory.child("data/table.conf").write_binary(b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault RESERVED NULL NULL NULL NOT_USED\ndefault GB_DICTIONARY gb_char.table absent.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\ndefault OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin SYSTEM_FILE\ndefault MERGED_DICTIONARY merged.table merged.bin merged.dbin SYSTEM_FILE\ndefault ADDON_DICTIONARY NULL NULL addon.bin USER_FILE\ndefault NETWORK_DICTIONARY NULL NULL network.bin USER_FILE\ndefault USER_DICTIONARY NULL NULL user.bin USER_FILE\n\naddon 4 art.table art.bin NULL DICTIONARY\naddon 5 culture.table culture.bin NULL DICTIONARY\naddon 6 economy.table economy.bin NULL DICTIONARY\naddon 7 geology.table geology.bin NULL DICTIONARY\naddon 8 history.table history.bin NULL DICTIONARY\n\naddon 9 life.table life.bin NULL DICTIONARY\naddon 10 nature.table nature.bin NULL DICTIONARY\naddon 11 people.table people.bin NULL DICTIONARY\naddon 12 science.table science.bin NULL DICTIONARY\naddon 13 society.table society.bin NULL DICTIONARY\naddon 14 sport.table sport.bin NULL DICTIONARY\naddon 15 technology.table technology.bin NULL DICTIONARY\n").expect("missing chunk configuration");
            directory
        }

        #[rstest]
        // pinyin-lookup-chunk-directory: tests/lookup/test_pinyin_lookup.cpp:41-153;
        // src/lookup/phonetic_lookup.h; tests/tests_helper.h:24-48 at 074a2219.
        // Policy-accepted on every cell:
        // docs/findings/upstream-divergences.md#utility-check-macros-write-no-message; only the pin
        // writes the check macro's message.
        #[case::pinyin_lookup_chunk_directory(
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
        // pinyin-lookup-embedded-nul: tests/lookup/test_pinyin_lookup.cpp:41-153;
        // src/lookup/phonetic_lookup.h; tests/tests_helper.h:24-48 at 074a2219.
        #[case::pinyin_lookup_embedded_nul(
            w3(),
            &[],
            b"ni\0hao\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_pinyin_lookup/six-char-lookup.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // pinyin-lookup-missing-chunk: tests/lookup/test_pinyin_lookup.cpp:41-153;
        // src/lookup/phonetic_lookup.h; tests/tests_helper.h:24-48 at 074a2219.
        #[case::pinyin_lookup_missing_chunk(
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
        // pinyin-lookup-missing-config: tests/lookup/test_pinyin_lookup.cpp:41-153;
        // src/lookup/phonetic_lookup.h; tests/tests_helper.h:24-48 at 074a2219.
        // pinyin-lookup-quit: tests/lookup/test_pinyin_lookup.cpp:41-153;
        // src/lookup/phonetic_lookup.h; tests/tests_helper.h:24-48 at 074a2219.
        #[case::pinyin_lookup_quit(
            w3(),
            &["ignored", "--help"],
            b"quit\nignored\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // pinyin-lookup-session: tests/lookup/test_pinyin_lookup.cpp:41-153;
        // src/lookup/phonetic_lookup.h; tests/tests_helper.h:24-48 at 074a2219.
        #[case::pinyin_lookup_session(
            w3(),
            &[],
            b"'ni''hao'\nni\0'hao\nni3hao3\nni'hao\nzhongguo\nxian\na\ninvalid\n\nquit\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("../../oxpinyin-utils/tests/fixtures/test_pinyin_lookup/session.txt")),
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
            directory
                .child("tests/lookup")
                .create_dir_all()
                .expect("working directory");
            for name in directories {
                directory
                    .child(name)
                    .create_dir_all()
                    .expect("input directory");
            }
            for (name, bytes) in files {
                directory
                    .child(name)
                    .write_binary(bytes)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("oxpinyin-repl")
                .arg("pinyin_lookup")
                .arg(directory.path().join("data"))
                .current_dir(directory.path().join("tests/lookup"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_pinyin_lookup");
            output.stdout = mask_print_time(&output.stdout);
            output.stderr = mask_glib_prefix(&output.stderr);
            // The REPL takes an explicit isolated data directory, unlike the
            // old driver's fixed ../data spelling. Keep the diagnostic payload.
            output.stderr = crate::common::replace_directory(
                &output.stderr,
                &directory.path().join("data"),
                b"../data",
            );
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_zhuyin {
        use super::{OPEN_DATA_USER_CONF_FAILED, session_user_files};
        use crate::common::expected::check_repl as check;
        use crate::common::expected::*;
        use crate::common::{CELL, inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::{fixture, rstest};
        use std::time::Duration;

        const NIHAO_LINE: &[u8] = "你好\n".as_bytes();

        #[fixture]
        fn w3() -> TempDir {
            let data = crate::fixtures::w3();
            let directory = TempDir::new().expect("scratch directory");
            std::fs::rename(data.path(), directory.path().join("data"))
                .expect("nested data directory");
            directory
                .child("tests")
                .create_dir_all()
                .expect("working directory");
            directory
        }

        #[rstest]
        // test-zhuyin-embedded-nul: 074a2219 tests/test_zhuyin.cpp:34-66;
        // src/include/pinyin_utils.h:LineString.
        #[case::test_zhuyin_embedded_nul(
            w3(),
            &[],
            b"su3cl3\0ignored\nquit\n",
            Run {
                status: Exit(0),
                stdout: Exact(NIHAO_LINE),
                stderr: OPEN_DATA_USER_CONF_FAILED,
                files: session_user_files(),
            },
        )]
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
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("oxpinyin-repl")
                .arg("zhuyin")
                .arg(directory.path().join("data"))
                .current_dir(directory.path().join("tests"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_zhuyin");
            output.stderr = mask_glib_prefix(&output.stderr);
            // The REPL takes an explicit isolated data directory, unlike the
            // old driver's fixed ../data spelling. Keep the diagnostic payload.
            output.stderr = crate::common::replace_directory(
                &output.stderr,
                &directory.path().join("data"),
                b"../data",
            );
            check(
                output,
                &inventory(
                    directory.path(),
                    &inputs,
                    if ["bdb", "tkrzw"].contains(&CELL) {
                        &[
                            "data/user_bigram.db",
                            "data/user_pinyin_index.bin",
                            "data/user_phrase_index.bin",
                        ]
                    } else {
                        &[]
                    },
                    &[],
                    &[],
                ),
                &inputs,
                &expected,
            );
        }
    }
}
