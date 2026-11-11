//! Frozen pin observations of the storage utilities and the pin's storage
//! test programs: exit status, both streams and every file the run leaves.
#[cfg(unix)]
mod common;
#[cfg(unix)]
mod fixtures;

// Unix only: the frozen pin runs are Linux and macOS observations (signal statuses, Unix paths and line endings).
#[cfg(unix)]
mod frozen {
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
    const OPEN_DATA_TABLE_CONF_FAILED: &[u8] =
        b"open ../../data/table.conf failed.\nload table.conf failed.\n";
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
    const UNKNOWN_OPTION_NO_SUCH_OPTION: &[u8] =
        b"option parsing failed:Unknown option --no-such-option\n";
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

    mod export_interpolation {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use crate::common::CELL;
        use crate::fixtures::w3;

        fn w3_model() -> Stdout {
            match CELL {
                "bdb" => Exact(include_bytes!(
                    "fixtures/export_interpolation/w3-model-bdb-order.txt"
                )),
                "kc" | "tkrzw" => Exact(include_bytes!(
                    "fixtures/export_interpolation/w3-model-kc-tkrzw-order.txt"
                )),
                _ => unreachable!("unknown cell {CELL}"),
            }
        }

        #[rstest]
        // exp-help-is-ordinary: utils/storage/export_interpolation.cpp at 074a2219: 30-73,
        // 75-143; src/storage/tag_utility.cpp: 362-417.
        // The program reads no argument: --help is one witness for every argument list.
        #[case::exp_help_is_ordinary(
            w3(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: w3_model(),
                stderr: b"",
                files: &[],
            },
        )]
        // exp-mini-fixture: utils/storage/export_interpolation.cpp at 074a2219: 30-73, 75-143;
        // src/storage/tag_utility.cpp: 362-417.
        #[case::exp_mini_fixture(
            w3(),
            &[],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: w3_model(),
                stderr: b"",
                files: &[],
            },
        )]
        // exp-missing-bigram: utils/storage/export_interpolation.cpp at 074a2219: 30-73,
        // 75-143; src/storage/tag_utility.cpp: 362-417.
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; only the pin writes Berkeley
        // DB's BDB1565 lines.
        #[case::exp_missing_bigram(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[("table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault RESERVED NULL NULL NULL NOT_USED\ndefault GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY NULL NULL gbk_char.dbin USER_FILE\ndefault OPENGRAM_DICTIONARY NULL NULL opengram.dbin USER_FILE\ndefault MERGED_DICTIONARY NULL NULL merged.dbin USER_FILE\n".as_slice()), ("gb_char.bin", include_bytes!("../../../fixtures/w3/tkt/gb_char.bin").as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/export_interpolation/w3-model-without-bigrams.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // exp-missing-table-conf: utils/storage/export_interpolation.cpp at 074a2219: 30-73,
        // 75-143; src/storage/tag_utility.cpp: 362-417.
        #[case::exp_missing_table_conf(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_TABLE_CONF_FAILED,
                files: &[],
            },
        )]
        // export_interpolation-family-e-truncated-first-chunk:
        // utils/storage/export_interpolation.cpp:60; utils/utils_helper.h:73-94;
        // src/include/memory_chunk.h:420-463 at 074a2219.
        // An absent chunk takes the same load failure and the same exit.
        #[case::export_interpolation_family_e_truncated_first_chunk(
            w3(),
            &[],
            b"",
            &[("gb_char.bin", b"\x04\0\0\0\0\0\0\0\x01".as_slice()), ("gbk_char.bin", b"\0".as_slice())],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: LOAD_GB_CHAR_BIN_FAILED,
                files: &[],
            },
        )]
        fn captured_cases(
            #[case] directory: TempDir,
            #[case] args: &[&str],
            #[case] stdin: &[u8],
            #[case] files: &[(&str, &[u8])],
            #[case] expected: Run,
        ) {
            for (name, content) in files {
                directory
                    .child(name)
                    .write_binary(content)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("export_interpolation")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run export_interpolation");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod gen_binary_files {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::{EMPTY_PHRASE_CHUNK, UNKNOWN_OPTION_NO_SUCH_OPTION};
        use crate::fixtures::w3;

        const ADDON_PHRASE_INDEX_ROWS: File =
            Rows("81d12a053baa9eec1081e5f58b310eca6a8f48bab979e5a422ca2aa907e244d0");
        const ADDON_PINYIN_INDEX_ROWS: File =
            Rows("828044f77093b893e80107597ec978fb59ee6dbca60e87c63624e366607fd151");
        const EMPTY_INDEX_FILES: &[(&str, File)] =
            &[("phrase_index.bin", NO_ROWS), ("pinyin_index.bin", NO_ROWS)];
        const MINI_GB_CHAR_BIN: File = Sha256(
            "c61a02bc627d0ccca88b511a4e239ad0da4b3b370a11bb7ea8d0e6b23ce9abbd",
            151,
        );
        const OPEN_GB_CHAR_TABLE_FAILED: &[u8] = b"open gb_char.table failed!\n";
        const W3_PHRASE_INDEX_ROWS: File =
            Rows("795c7fb51c1676472d23d7784b27f1f11530af78813d6beafbda444c6cc358e8");
        const W3_PINYIN_INDEX_ROWS: File =
            Rows("0bd2c7caf8a9dbed1d9040f6c7f1e2aa9399c392d4093dc768f3a5e172d298e1");

        // 074a2219 gen_binary_files.cpp:68-72 passes the configured type to
        // both loaders; phrase_index.cpp:562-581 parses Zhuyin with tones.
        #[rstest]
        #[case::pinyin("pinyin", "ni3")]
        #[case::zhuyin("zhuyin", "ㄋㄧˇ")]
        fn configured_phonetic_type_reaches_both_phrase_libraries(
            #[case] format: &str,
            #[case] spelling: &str,
        ) {
            let directory = TempDir::new().expect("temporary model");
            directory
                .child("table.conf")
                .write_str(&format!(
                    "binary format version:7\nmodel data version:14\nlambda parameter:0.3\n\
                 source table format:{format}\ndatabase format:{}\n\
                 default RESERVED NULL NULL NULL NOT_USED\n\
                 default GB_DICTIONARY source.table gb_char.bin NULL SYSTEM_FILE\n\
                 addon RESERVED NULL NULL NULL NOT_USED\n\
                 addon 1 source.table addon.bin NULL DICTIONARY\n",
                    oxpinyin_store::DEFAULT_STORE_DB_FORMAT,
                ))
                .expect("configuration");
            directory
                .child("source.table")
                .write_str(&format!("{spelling}\t你\t16777217\t7\n"))
                .expect("one source row");
            cargo_bin_cmd!("gen_binary_files")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .assert()
                .success()
                .stdout("")
                .stderr("");
            let expected = oxpinyin_core::ChewingKey::from_pinyin("ni")
                .expect("ni")
                .with_tone(3)
                .to_packed()
                .to_le_bytes();
            for name in ["gb_char.bin", "addon.bin"] {
                let library = oxpinyin_data::phrase_library::PhraseLibrary::open(
                    &directory.path().join(name),
                )
                .expect("generated phrase library");
                let item = library.item(16777217).expect("source token");
                assert_eq!(item.n_pronunciations(), 1, "{name}");
                let reading = item.pronunciation(0).expect("source pronunciation");
                assert_eq!(reading.keys, expected, "{name}");
                assert_eq!(reading.freq, 7, "{name}");
            }
        }

        #[rstest]
        // gbf-bad-option: utils/storage/gen_binary_files.cpp at 074a2219: 26-34, 36-109,
        // 111-152; utils/utils_helper.h: 73-145; src/storage/table_text_parser.h: 34-121.
        #[case::gbf_bad_option(
            TempDir::new().unwrap(),
            &["--no-such-option"],
            b"",
            &[],
            ( &["pinyin_index.bin", "phrase_index.bin", "addon_pinyin_index.bin", "addon_phrase_index.bin"][..], &[] as &[&str] ),
            Run {
                status: Exit(22),
                stdout: Exact(UNKNOWN_OPTION_NO_SUCH_OPTION),
                stderr: b"",
                files: &[],
            },
        )]
        // gbf-frequency-narrows-to-guint32: utils/storage/gen_binary_files.cpp at 074a2219:
        // 36-86; src/storage/table_text_parser.h: 84-103; src/storage/phrase_index.cpp:
        // 524-583, 56-91.
        #[case::gbf_frequency_narrows_to_guint32(
            w3(),
            &[],
            b"",
            &[("table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\n".as_slice()), ("gb_char.table", b"ni\t\xe4\xbd\xa0\t16777217\t4294967297\n".as_slice())],
            ( &["pinyin_index.bin", "phrase_index.bin", "addon_pinyin_index.bin", "addon_phrase_index.bin"][..], &[] as &[&str] ),
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[
                    ("addon_phrase_index.bin", ADDON_PHRASE_INDEX_ROWS),
                    ("addon_pinyin_index.bin", ADDON_PINYIN_INDEX_ROWS),
                    ("gb_char.bin", Sha256("bb0b1af185f7118c1d61f1ebed8e7fe21f1e5775a99ff0a842cec882453348c4", 59)),
                    ("phrase_index.bin", Rows("36b273e861534ca6f7fe85d3ac4aa666b686f8cfd20ac8e647eb158edc22faaf")),
                    ("pinyin_index.bin", Rows("99f7b721c31e56b610297df0ba38fd19238cd3f2c6ab0a3250afd8d058383a4c")),
                ],
            },
        )]
        // gbf-help: utils/storage/gen_binary_files.cpp at 074a2219: 26-34, 36-109, 111-152;
        // utils/utils_helper.h: 73-145; src/storage/table_text_parser.h: 34-121.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::gbf_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            ( &["pinyin_index.bin", "phrase_index.bin", "addon_pinyin_index.bin", "addon_phrase_index.bin"][..], &[] as &[&str] ),
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/gen_binary_files/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // gbf-library-bits-are-masked: src/include/novel_types.h: 40-45;
        // src/storage/phrase_index.cpp: 539 at 074a2219.
        #[case::gbf_library_bits_are_masked(
            w3(),
            &[],
            b"",
            &[("table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\naddon 15 technology.table technology.bin NULL DICTIONARY\n".as_slice()), ("technology.table", b"a\t\xe5\x95\x8a\t520093697\t1\n".as_slice())],
            (&["phrase_index.bin", "addon_phrase_index.bin"][..], &["pinyin_index.bin", "addon_pinyin_index.bin"][..]),
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[
                    ("addon_phrase_index.bin", Rows("669020d9638a6022e86dfeb33013629348223eec1bfd3fcbb15caa4edeb541c8")),
                    ("addon_pinyin_index.bin", Rows("9d85a6493ca7c0768948d46226acd36c89d70e939e9d94b7b229a1ff023a804d")),
                    ("phrase_index.bin", W3_PHRASE_INDEX_ROWS),
                    ("pinyin_index.bin", W3_PINYIN_INDEX_ROWS),
                    ("technology.bin", Sha256("8620ed6a312e06386ed4ab79cd601b94987ffae3968f53c29a0e74b8048900a5", 59)),
                ],
            },
        )]
        // gbf-mini-fixture: utils/storage/gen_binary_files.cpp at 074a2219: 26-34, 36-109,
        // 111-152; utils/utils_helper.h: 73-145; src/storage/table_text_parser.h: 34-121.
        #[case::gbf_mini_fixture(
            w3(),
            &[],
            b"",
            &[("gb_char.table", b"a\t\xe5\x90\x96\t16777217\t104\na\t\xe5\x95\x8a\t16777218\t5\nni\t\xe4\xbd\xa0\t16777219\t7\nni'hao\t\xe4\xbd\xa0\xe5\xa5\xbd\t16777220\t9\nzhong'guo\t\xe4\xb8\xad\xe5\x9b\xbd\t16777221\t3\n".as_slice()), ("gbk_char.table", b"".as_slice()), ("opengram.table", b"".as_slice()), ("merged.table", b"".as_slice()), ("art.table", b"".as_slice()), ("culture.table", b"".as_slice()), ("economy.table", b"".as_slice()), ("geology.table", b"".as_slice()), ("history.table", b"".as_slice()), ("life.table", b"".as_slice()), ("nature.table", b"".as_slice()), ("people.table", b"".as_slice()), ("science.table", b"".as_slice()), ("society.table", b"".as_slice()), ("sport.table", b"".as_slice()), ("technology.table", b"".as_slice())],
            ( &["pinyin_index.bin", "phrase_index.bin", "addon_pinyin_index.bin", "addon_phrase_index.bin"][..], &[] as &[&str] ),
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[
                    ("addon_phrase_index.bin", ADDON_PHRASE_INDEX_ROWS),
                    ("addon_pinyin_index.bin", ADDON_PINYIN_INDEX_ROWS),
                    ("art.bin", EMPTY_PHRASE_CHUNK),
                    ("culture.bin", EMPTY_PHRASE_CHUNK),
                    ("economy.bin", EMPTY_PHRASE_CHUNK),
                    ("gb_char.bin", MINI_GB_CHAR_BIN),
                    ("gbk_char.bin", EMPTY_PHRASE_CHUNK),
                    ("geology.bin", EMPTY_PHRASE_CHUNK),
                    ("history.bin", EMPTY_PHRASE_CHUNK),
                    ("life.bin", EMPTY_PHRASE_CHUNK),
                    ("nature.bin", EMPTY_PHRASE_CHUNK),
                    ("opengram.bin", EMPTY_PHRASE_CHUNK),
                    ("people.bin", EMPTY_PHRASE_CHUNK),
                    ("phrase_index.bin", Rows("5193ab6fcf042342cc4421fb0732eba55492a43d21f1b47dd781942c873fcd1c")),
                    ("pinyin_index.bin", Rows("8e87c9129cf410bef716cddc741dd02e9034a928616d716f3dd40eb6ab8e3fab")),
                    ("science.bin", EMPTY_PHRASE_CHUNK),
                    ("society.bin", EMPTY_PHRASE_CHUNK),
                    ("sport.bin", EMPTY_PHRASE_CHUNK),
                    ("technology.bin", EMPTY_PHRASE_CHUNK),
                ],
            },
        )]
        // gbf-missing-table-conf: utils/storage/gen_binary_files.cpp at 074a2219: 26-34,
        // 36-109, 111-152; utils/utils_helper.h: 73-145; src/storage/table_text_parser.h:
        // 34-121.
        #[case::gbf_missing_table_conf(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[],
            ( &["pinyin_index.bin", "phrase_index.bin", "addon_pinyin_index.bin", "addon_phrase_index.bin"][..], &[] as &[&str] ),
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_TABLE_CONF_FAILED,
                files: &[],
            },
        )]
        // gbf-missing-table-file: utils/storage/gen_binary_files.cpp at 074a2219: 26-34,
        // 36-109, 111-152; utils/utils_helper.h: 73-145; src/storage/table_text_parser.h:
        // 34-121.
        // The table file opens before the punctuation table, so --gen-punct-table does not change this result.
        #[case::gbf_missing_table_file(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[("table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault RESERVED NULL NULL NULL NOT_USED\ndefault GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY NULL NULL gbk_char.dbin USER_FILE\ndefault OPENGRAM_DICTIONARY NULL NULL opengram.dbin USER_FILE\ndefault MERGED_DICTIONARY NULL NULL merged.dbin USER_FILE\n".as_slice())],
            ( &["pinyin_index.bin", "phrase_index.bin", "addon_pinyin_index.bin", "addon_phrase_index.bin"][..], &[] as &[&str] ),
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_GB_CHAR_TABLE_FAILED,
                files: EMPTY_INDEX_FILES,
            },
        )]
        // gbf-pronunciation-count-wraps: src/storage/phrase_index.cpp: 56-91, 524-583 at
        // 074a2219; guint8 count at :28.
        #[case::gbf_pronunciation_count_wraps(
            w3(),
            &[],
            b"",
            &[("table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\n".as_slice()), ("gb_char.table", b"a'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\na'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nai'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nan'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nang'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nao'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nba'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbai'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nban'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbang'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbao'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbei'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nben'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbeng'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbi'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbian'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'a\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'ai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'an\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'ang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'ao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'ba\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'bai\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'ban\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'bang\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'bao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'bei\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'ben\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'beng\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'bi\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'bian\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\nbiao'biao\t\xe4\xbd\xa0\xe4\xbd\xa0\t16777217\t1\n".as_slice())],
            ( &["pinyin_index.bin", "phrase_index.bin", "addon_pinyin_index.bin", "addon_phrase_index.bin"][..], &[] as &[&str] ),
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[
                    ("addon_phrase_index.bin", ADDON_PHRASE_INDEX_ROWS),
                    ("addon_pinyin_index.bin", ADDON_PINYIN_INDEX_ROWS),
                    ("gb_char.bin", Sha256("c87497f45d4f9d51e4d356c4261d4cb851d6d299df781748722b7ea7b4da2377", 57)),
                    ("phrase_index.bin", Rows("f44572f23db60c9d06eccfe45dc844868608bbe24c999e0b7abcd874a6129f20")),
                    ("pinyin_index.bin", Rows("219ec7524ec7bb6b274d9a50b3f38fbef22c8fd8ee09cda84f7473f931cd8ff1")),
                ],
            },
        )]
        // gbf-toned-model: utils/storage/gen_binary_files.cpp at 074a2219: 36-109 and 111-152;
        // src/storage/chewing_large_table2.cpp: 112-158 (USE_TONE parse, the tone bits of
        // every index item); src/storage/phrase_index.cpp (SubPhraseIndex::store);
        // utils/utils_helper.h: 73-145.
        #[case::gbf_toned_model(
            w3(),
            &["--gen-punct-table"],
            b"",
            &[("gb_char.table", include_bytes!("../../../fixtures/datagen-toned/gb_char.table").as_slice()), ("gbk_char.table", include_bytes!("../../../fixtures/datagen-toned/gbk_char.table").as_slice()), ("opengram.table", include_bytes!("../../../fixtures/datagen-toned/opengram.table").as_slice()), ("merged.table", include_bytes!("../../../fixtures/datagen-toned/merged.table").as_slice()), ("art.table", include_bytes!("../../../fixtures/datagen-toned/art.table").as_slice()), ("culture.table", include_bytes!("../../../fixtures/datagen-toned/culture.table").as_slice()), ("economy.table", include_bytes!("../../../fixtures/datagen-toned/economy.table").as_slice()), ("geology.table", include_bytes!("../../../fixtures/datagen-toned/geology.table").as_slice()), ("history.table", include_bytes!("../../../fixtures/datagen-toned/history.table").as_slice()), ("life.table", include_bytes!("../../../fixtures/datagen-toned/life.table").as_slice()), ("nature.table", include_bytes!("../../../fixtures/datagen-toned/nature.table").as_slice()), ("people.table", include_bytes!("../../../fixtures/datagen-toned/people.table").as_slice()), ("science.table", include_bytes!("../../../fixtures/datagen-toned/science.table").as_slice()), ("society.table", include_bytes!("../../../fixtures/datagen-toned/society.table").as_slice()), ("sport.table", include_bytes!("../../../fixtures/datagen-toned/sport.table").as_slice()), ("technology.table", include_bytes!("../../../fixtures/datagen-toned/technology.table").as_slice()), ("punct.table", include_bytes!("../../../fixtures/datagen-toned/punct.table").as_slice())],
            (&["phrase_index.bin", "addon_pinyin_index.bin", "addon_phrase_index.bin", "punct.bin"][..], &["pinyin_index.bin"][..]),
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[
                    ("addon_phrase_index.bin", ADDON_PHRASE_INDEX_ROWS),
                    ("addon_pinyin_index.bin", Rows("59889de56da7fb5fa8d4dcf8901d05b0dbabec573870faef71bedd761b459bec")),
                    ("art.bin", Sha256("73929be36483f2faafaa34cbd0feb245d9011eedaf5ffba55e7dcd24cacee71f", 73)),
                    ("culture.bin", EMPTY_PHRASE_CHUNK),
                    ("economy.bin", EMPTY_PHRASE_CHUNK),
                    ("gb_char.bin", Sha256("c485904a106927dff259f872bf9951ccfffc44b6709522857ec9b83437d3fcb6", 307)),
                    ("gbk_char.bin", Sha256("dc9a6fd977b568efeceb93af5f7cad7638f6a9a5e857ea64aa62e1ed3139bcfd", 79)),
                    ("geology.bin", EMPTY_PHRASE_CHUNK),
                    ("history.bin", EMPTY_PHRASE_CHUNK),
                    ("life.bin", EMPTY_PHRASE_CHUNK),
                    ("merged.bin", Sha256("81e233b386272874cd326eb7e20cec8f4757bf4042756fa61be5a86133a05603", 65)),
                    ("nature.bin", EMPTY_PHRASE_CHUNK),
                    ("opengram.bin", Sha256("f16d3219101289f0ecff12df06924d4e74dbf0faddc7edc08dc2ca01d5273f5a", 97)),
                    ("people.bin", EMPTY_PHRASE_CHUNK),
                    ("phrase_index.bin", Rows("5b4b6d7783d52a1850bce68c90d62ff52e797689a30af5bb55cf1ce682613dc8")),
                    ("pinyin_index.bin", Rows("9d62a0b6c36340911dc4d5a28bd7640a24cc10c023c112ad89e664d83b07d29d")),
                    ("punct.bin", Rows("f654ac858c14f035f6e1a054ec8357b5e950be24f99b2749fbe02941089f4ab4")),
                    ("science.bin", EMPTY_PHRASE_CHUNK),
                    ("society.bin", EMPTY_PHRASE_CHUNK),
                    ("sport.bin", EMPTY_PHRASE_CHUNK),
                    ("technology.bin", EMPTY_PHRASE_CHUNK),
                ],
            },
        )]
        // gbf-too-many-arguments: utils/storage/gen_binary_files.cpp at 074a2219: 26-34,
        // 36-109, 111-152; utils/utils_helper.h: 73-145; src/storage/table_text_parser.h:
        // 34-121.
        #[case::gbf_too_many_arguments(
            TempDir::new().unwrap(),
            &["extra"],
            b"",
            &[("table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault RESERVED NULL NULL NULL NOT_USED\ndefault GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY NULL NULL gbk_char.dbin USER_FILE\ndefault OPENGRAM_DICTIONARY NULL NULL opengram.dbin USER_FILE\ndefault MERGED_DICTIONARY NULL NULL merged.dbin USER_FILE\n".as_slice()), ("gb_char.table", b"a\t\xe5\x90\x96\t16777217\t104\na\t\xe5\x95\x8a\t16777218\t5\nni\t\xe4\xbd\xa0\t16777219\t7\nni'hao\t\xe4\xbd\xa0\xe5\xa5\xbd\t16777220\t9\nzhong'guo\t\xe4\xb8\xad\xe5\x9b\xbd\t16777221\t3\n".as_slice())],
            (&["phrase_index.bin", "addon_phrase_index.bin"][..], &["pinyin_index.bin", "addon_pinyin_index.bin"][..]),
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[
                    ("addon_phrase_index.bin", NO_ROWS),
                    ("addon_pinyin_index.bin", NO_ROWS),
                    ("gb_char.bin", MINI_GB_CHAR_BIN),
                    ("phrase_index.bin", Rows("f0143c77f0078acd15d8bf296960210fea7c66e3c88a11be0442998158dd992a")),
                    ("pinyin_index.bin", Rows("43263f4c1f20900c3c9e0e9b7d368578ff9daaddcb6a242bcb8375a9e4defa03")),
                ],
            },
        )]
        fn captured_cases(
            #[case] directory: TempDir,
            #[case] args: &[&str],
            #[case] stdin: &[u8],
            #[case] files: &[(&str, &[u8])],
            #[case] row_files: (&[&str], &[&str]),
            #[case] expected: Run,
        ) {
            for (name, content) in files {
                directory
                    .child(name)
                    .write_binary(content)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("gen_binary_files")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run gen_binary_files");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, row_files.0, row_files.1, &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod import_interpolation {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::UNKNOWN_OPTION_NO_SUCH_OPTION;
        use crate::common::CELL;
        use crate::fixtures::w3;

        const IMPORTED_W3_GB_CHAR_BIN: File = Sha256(
            "52c3672c8f5f51e9ceff9e88a732bb5dcbdae1b031bbe304cdd0fd205740127f",
            167681,
        );
        const OPEN_PHRASE_INDEX_BIN_FAILED: &[u8] = b"open phrase_index.bin failed!\n";
        const W3_BIGRAM_FILES: &[(&str, File)] = &[("bigram.db", W3_BIGRAM_ROWS)];
        const W3_BIGRAM_ROWS: File =
            Rows("f83b7af73d9816a24032344f3941eb51cead0d874c0ec6753d308b003595b0f0");

        #[rstest]
        // imp-bad-header: utils/storage/import_interpolation.cpp at 074a2219: 34-40, 43-232,
        // 234-307; utils/utils_helper.h: 27-49; src/storage/tag_utility.cpp: 118-264.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        #[case::imp_bad_header(
            w3(),
            &[],
            b"garbage\n",
            &[],
            &["bigram.db"],
            Run {
                status: if cfg!(target_os = "macos") {
                    Exit(96)
                } else {
                    Exit(61)
                },
                stdout: Exact(b""),
                stderr: b"error: interpolation model expected.\n",
                files: W3_BIGRAM_FILES,
            },
        )]
        // imp-bad-option: utils/storage/import_interpolation.cpp at 074a2219: 34-40, 43-232,
        // 234-307; utils/utils_helper.h: 27-49; src/storage/tag_utility.cpp: 118-264.
        #[case::imp_bad_option(
            TempDir::new().unwrap(),
            &["--no-such-option"],
            b"",
            &[],
            &["bigram.db"],
            Run {
                status: Exit(22),
                stdout: Exact(UNKNOWN_OPTION_NO_SUCH_OPTION),
                stderr: b"",
                files: &[],
            },
        )]
        // imp-empty-stdin: utils/storage/import_interpolation.cpp at 074a2219: 34-40, 43-232,
        // 234-307; utils/utils_helper.h: 27-49; src/storage/tag_utility.cpp: 118-264.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        #[case::imp_empty_stdin(
            w3(),
            &[],
            b"",
            &[],
            &["bigram.db"],
            Run {
                status: if cfg!(target_os = "macos") {
                    Exit(96)
                } else {
                    Exit(61)
                },
                stdout: Exact(b""),
                stderr: EMPTY_FILE_INPUT,
                files: W3_BIGRAM_FILES,
            },
        )]
        // imp-help: utils/storage/import_interpolation.cpp at 074a2219: 34-40, 43-232,
        // 234-307; utils/utils_helper.h: 27-49; src/storage/tag_utility.cpp: 118-264.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::imp_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            &["bigram.db"],
            Run {
                status: Exit(0),
                stdout: GlibOption("Usage:\n  import_interpolation [OPTION?] - import interpolation model\n\nHelp Options:\n  -h, --help       Show help options\n\nApplication Options:\n  --table-dir      table directory\n\n"),
                stderr: b"",
                files: &[],
            },
        )]
        // imp-missing-chunk: utils/storage/import_interpolation.cpp at 074a2219: 34-40,
        // 43-232, 234-307; utils/utils_helper.h: 27-49; src/storage/tag_utility.cpp: 118-264.
        #[case::imp_missing_chunk(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[("table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault RESERVED NULL NULL NULL NOT_USED\ndefault GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY NULL NULL gbk_char.dbin USER_FILE\ndefault OPENGRAM_DICTIONARY NULL NULL opengram.dbin USER_FILE\ndefault MERGED_DICTIONARY NULL NULL merged.dbin USER_FILE\n".as_slice()), ("phrase_index.bin", include_bytes!("../../../fixtures/w3/tkt/phrase_index.bin").as_slice())],
            &["bigram.db"],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: match CELL {
                    "bdb" => b"BDB0641 __db_meta_setup: phrase_index.bin: unexpected file type or format\nopen phrase_index.bin failed!\n",
                    "kc" => OPEN_PHRASE_INDEX_BIN_FAILED,
                    "tkrzw" => LOAD_GB_CHAR_BIN_FAILED,
                    _ => unreachable!("unknown cell {CELL}"),
                },
                files: &[],
            },
        )]
        // imp-missing-phrase-index: utils/storage/import_interpolation.cpp at 074a2219: 34-40,
        // 43-232, 234-307; utils/utils_helper.h: 27-49; src/storage/tag_utility.cpp: 118-264.
        #[case::imp_missing_phrase_index(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[("table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault RESERVED NULL NULL NULL NOT_USED\ndefault GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY NULL NULL gbk_char.dbin USER_FILE\ndefault OPENGRAM_DICTIONARY NULL NULL opengram.dbin USER_FILE\ndefault MERGED_DICTIONARY NULL NULL merged.dbin USER_FILE\n".as_slice())],
            &["bigram.db"],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_PHRASE_INDEX_BIN_FAILED,
                files: &[],
            },
        )]
        // imp-missing-table-conf: utils/storage/import_interpolation.cpp at 074a2219: 34-40,
        // 43-232, 234-307; utils/utils_helper.h: 27-49; src/storage/tag_utility.cpp: 118-264.
        #[case::imp_missing_table_conf(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[],
            &["bigram.db"],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_TABLE_CONF_FAILED,
                files: &[],
            },
        )]
        // imp-ordinary: utils/storage/import_interpolation.cpp at 074a2219: 34-40, 43-232,
        // 234-307; utils/utils_helper.h: 27-49; src/storage/tag_utility.cpp: 118-264.
        #[case::imp_ordinary(
            w3(),
            &[],
            b"\\data model interpolation\n\\1-gram\n\\item 16777217 \xe9\x94\x95 count 5\n\\2-gram\n\\item 16777217 \xe9\x94\x95 16777218 \xe5\x90\x96 count 4\n\\end\n",
            &[],
            &["bigram.db"],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[
                    ("bigram.db", Rows("719d2ae206c014b399af2019e94b90f19df27a6177d34296036e6f6d61654508")),
                    ("gb_char.bin", IMPORTED_W3_GB_CHAR_BIN),
                ],
            },
        )]
        // imp-unigram: utils/storage/import_interpolation.cpp at 074a2219: 34-40, 43-232,
        // 234-307; utils/utils_helper.h: 27-49; src/storage/tag_utility.cpp: 118-264.
        #[case::imp_unigram(
            w3(),
            &[],
            b"\\data model interpolation\n\\1-gram\n\\item 16777217 \xe9\x94\x95 count 5\n\\item 1 <start> count 2\n\\end\n",
            &[],
            &["bigram.db"],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[
                    ("bigram.db", W3_BIGRAM_ROWS),
                    ("gb_char.bin", IMPORTED_W3_GB_CHAR_BIN),
                ],
            },
        )]
        // import_interpolation-family-e-truncated-first-chunk:
        // utils/storage/import_interpolation.cpp:272; utils/utils_helper.h:73-94;
        // src/include/memory_chunk.h:420-463 at 074a2219.
        #[case::import_interpolation_family_e_truncated_first_chunk(
            w3(),
            &[],
            b"",
            &[("gb_char.bin", b"\x04\0\0\0\0\0\0\0\x01".as_slice()), ("gbk_char.bin", b"\0".as_slice())],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: LOAD_GB_CHAR_BIN_FAILED,
                files: &[],
            },
        )]
        fn captured_cases(
            #[case] directory: TempDir,
            #[case] args: &[&str],
            #[case] stdin: &[u8],
            #[case] files: &[(&str, &[u8])],
            #[case] dbm_files: &[&str],
            #[case] expected: Run,
        ) {
            for (name, content) in files {
                directory
                    .child(name)
                    .write_binary(content)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("import_interpolation")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run import_interpolation");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, dbm_files, &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_chewing {
        use super::{OPEN_DATA_USER_CONF_FAILED, session_user_conf_files, session_user_files};
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
                stdout: Exact(include_bytes!("fixtures/test_chewing/nihao-session.txt")),
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
                stdout: Exact(include_bytes!("fixtures/test_chewing/nihao-zhongguo-session.txt")),
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
            let mut output = cargo_bin_cmd!("test_chewing")
                .current_dir(directory.path().join("tests"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_chewing");
            output.stderr = mask_glib_prefix(&output.stderr);
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
        use super::{OPEN_DATA_TABLE_CONF_FAILED, OPEN_GB_TABLE_FAILED};
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
                stdout: Exact(include_bytes!("fixtures/test_chewing_table/two-range-lookup.txt")),
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
        #[case::chewing_table_missing_config(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_DATA_TABLE_CONF_FAILED,
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
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; only the pin
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
                stdout: Exact(include_bytes!("fixtures/test_chewing_table/session.txt")),
                stderr: b"Invalid input.\n",
                files: &[],
            },
        )]
        // chewing-table-toned-initial: tests/storage/test_chewing_table.cpp;
        // tests/tests_helper.h:51-133; src/storage/phrase_large_table3.cpp:29-139;
        // src/storage/chewing_large_table2.cpp:112-198 at 074a2219.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; only the pin
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
            let mut output = cargo_bin_cmd!("test_chewing_table")
                .current_dir(directory.path().join("tests/storage"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_chewing_table");
            output.stdout = mask_print_time(&output.stdout);
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_matrix {
        use super::{OPEN_ABSENT_BIN_FAILED, OPEN_DATA_TABLE_CONF_FAILED};
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
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; only the pin
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
                stdout: Exact(include_bytes!("fixtures/test_matrix/six-char-one-key-matrix.txt")),
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
        #[case::matrix_missing_config(
            TempDir::new().unwrap(),
            &[],
            b"quit\n",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_DATA_TABLE_CONF_FAILED,
                files: &[],
            },
        )]
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
                stdout: Exact(include_bytes!("fixtures/test_matrix/session.txt")),
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
            let mut output = cargo_bin_cmd!("test_matrix")
                .current_dir(directory.path().join("tests/storage"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_matrix");
            output.stdout = mask_print_time(&output.stdout);
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_memory_chunk {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs, undo_tmp_override};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        const MEMORY_CHUNK_FILES: &[(&str, File)] = &[(
            "tmp/test.bin",
            Sha256(
                "8ad6b1fb0766ca597112abcd790c0ea8c419ea39cce187db9529539360e9e5a1",
                16,
            ),
        )];
        const MEMORY_CHUNK_SESSION: &[u8] =
            b"12\n4\n6\n12\t5\n12\n12\t5\t5\n12\t5\t5\t3\n10\t11\t12\n10\t12\n12\n";

        #[rstest]
        // memory-chunk-ignored-input: tests/include/test_memory_chunk.cpp:5-69;
        // src/include/memory_chunk.h:95-133,290-376,416-557 at 074a2219.
        // Also the removal-offset witness of a fresh run: the arguments and input are ignored.
        #[case::memory_chunk_ignored_input(
            &["--help", "ignored"],
            b"ignored\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(MEMORY_CHUNK_SESSION),
                stderr: b"",
                files: MEMORY_CHUNK_FILES,
            },
        )]
        // memory-chunk-replaces-file: tests/include/test_memory_chunk.cpp:5-69;
        // src/include/memory_chunk.h:95-133,290-376,416-557 at 074a2219.
        #[case::memory_chunk_replaces_file(
            &[],
            b"",
            &[],
            &[("tmp/test.bin", b"old bytes that must be truncated".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(MEMORY_CHUNK_SESSION),
                stderr: b"",
                files: MEMORY_CHUNK_FILES,
            },
        )]
        // memory-chunk-save-directory: tests/include/test_memory_chunk.cpp:5-69;
        // src/include/memory_chunk.h:95-133,290-376,416-557 at 074a2219.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; only the pin
        // writes the check macro's message.
        // Platform exception, macos: abort() flushes stdio on macOS, so buffered stdout
        // survives; glibc's discards it.
        #[case::memory_chunk_save_directory(
            &[],
            b"",
            &["tmp/test.bin"],
            &[("tmp/test.bin/occupied", b"kept".as_slice())],
            Run {
                status: Signal(6),
                stdout: if cfg!(target_os = "macos") {
                    Exact(MEMORY_CHUNK_SESSION)
                } else {
                    Exact(b"")
                },
                stderr: b"",
                files: &[],
            },
        )]
        fn captured_cases(
            #[case] args: &[&str],
            #[case] stdin: &[u8],
            #[case] directories: &[&str],
            #[case] files: &[(&str, &[u8])],
            #[case] expected: Run,
        ) {
            let directory = TempDir::new().expect("scratch directory");
            directory
                .child("tmp")
                .create_dir_all()
                .expect("private fixed names");
            for name in directories {
                directory
                    .child(name)
                    .create_dir_all()
                    .expect("input directory");
            }
            for (name, content) in files {
                directory
                    .child(name)
                    .write_binary(content)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("test_memory_chunk")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .env(
                    oxpinyin_utils::fixed_tmp::OVERRIDE,
                    directory.path().join("tmp"),
                )
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_memory_chunk");
            output.stderr = undo_tmp_override(
                &mask_glib_prefix(&output.stderr),
                &directory.path().join("tmp"),
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
                stdout: Exact(include_bytes!("fixtures/test_parser2/dachen26-ri.txt")),
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
                stdout: Exact(include_bytes!("fixtures/test_parser2/double-nihao.txt")),
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
                stdout: Exact(include_bytes!("fixtures/test_parser2/full-nihao.txt")),
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
                stdout: Exact(include_bytes!("fixtures/test_parser2/full-nihao-incomplete.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // parser2-help: tests/storage/test_parser2.cpp:37-147; tests/timer.h:30-44 at
        // 074a2219.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::parser2_help(
            &["--help"],
            b"",
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/test_parser2/help.txt")),
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
                stdout: Exact(include_bytes!("fixtures/test_parser2/hsu-hang.txt")),
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
                stdout: Exact(include_bytes!("fixtures/test_parser2/pinyin-direct-nihao.txt")),
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
                stdout: Exact(include_bytes!("fixtures/test_parser2/standard-nihao.txt")),
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
                stdout: Exact(include_bytes!("fixtures/test_parser2/full-nihao.txt")),
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
                stdout: Exact(include_bytes!("fixtures/test_parser2/zhuyin-direct-nihao.txt")),
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
                stdout: Exact(include_bytes!("fixtures/test_parser2/zhuyin-direct-nihao-incomplete.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        fn captured_cases(#[case] args: &[&str], #[case] stdin: &[u8], #[case] expected: Run) {
            let directory = TempDir::new().expect("scratch directory");
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("test_parser2")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_parser2");
            output.stdout = mask_print_time(&output.stdout);
            output.stderr = mask_glib_prefix(&output.stderr);
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
            let mut output = cargo_bin_cmd!("test_phrase")
                .current_dir(directory.path().join("tests"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_phrase");
            output.stderr = mask_glib_prefix(&output.stderr);
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
    mod test_phrase_index {
        use super::{OPEN_DATA_TABLE_CONF_FAILED, OPEN_GB_TABLE_FAILED};
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, mask_print_time, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::{fixture, rstest};
        use std::time::Duration;

        const PHRASE_INDEX_SESSION: &[u8] = "pinyin possiblitiy:0.250000\nSpent <TIME> us for 100000 operations, <TIME> us/op, <TIME> times/s.\n\npinyin poss:0.500000\n一二三四五六七八九十一二三四\n".as_bytes();
        const PHRASE_INDEX_SESSION_WITHOUT_PHRASES: &[u8] = b"pinyin possiblitiy:0.250000\nSpent <TIME> us for 100000 operations, <TIME> us/op, <TIME> times/s.\n\npinyin poss:0.500000\n";

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
                .write_binary(b"hao3 \xe5\xa5\xbd 33554433 10\n")
                .expect("GBK table");
            directory
        }

        #[fixture]
        fn tables() -> TempDir {
            let directory = table_config();
            directory.child("data/gb.table").write_binary(b"ni3 \xe4\xbd\xa0 16777222 100\nni2 \xe4\xbd\xa0 16777222 300\nyi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1 \xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89\xe5\x9b\x9b\xe4\xba\x94\xe5\x85\xad\xe4\xb8\x83\xe5\x85\xab\xe4\xb9\x9d\xe5\x8d\x81\xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89\xe5\x9b\x9b 16870553 100\n").expect("GB table");
            directory
        }

        #[rstest]
        // phrase-index-ignored-input: tests/storage/test_phrase_index.cpp:9-122;
        // tests/tests_helper.h:51-83; src/storage/phrase_index.cpp:56-143,335-389,522-674 at
        // 074a2219.
        // Also the round-trip witness: the arguments and input are ignored.
        #[case::phrase_index_ignored_input(
            tables(),
            &["--help", "ignored"],
            b"ignored\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(PHRASE_INDEX_SESSION),
                stderr: b"",
                files: &[],
            },
        )]
        // phrase-index-missing-config: tests/storage/test_phrase_index.cpp:9-122;
        // tests/tests_helper.h:51-83; src/storage/phrase_index.cpp:56-143,335-389,522-674 at
        // 074a2219.
        #[case::phrase_index_missing_config(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(PHRASE_INDEX_SESSION_WITHOUT_PHRASES),
                stderr: OPEN_DATA_TABLE_CONF_FAILED,
                files: &[],
            },
        )]
        // phrase-index-missing-text: tests/storage/test_phrase_index.cpp:9-122;
        // tests/tests_helper.h:51-83; src/storage/phrase_index.cpp:56-143,335-389,522-674 at
        // 074a2219.
        #[case::phrase_index_missing_text(
            table_config(),
            &[],
            b"",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(PHRASE_INDEX_SESSION_WITHOUT_PHRASES),
                stderr: OPEN_GB_TABLE_FAILED,
                files: &[],
            },
        )]
        // phrase-index-same-token-invalid-utf8-reading:
        // tests/storage/test_phrase_index.cpp:9-122; tests/tests_helper.h:51-83;
        // src/storage/phrase_index.cpp:56-143,335-389,522-674 at 074a2219.
        #[case::phrase_index_same_token_invalid_utf8_reading(
            table_config(),
            &[],
            b"",
            &[],
            &[("data/gb.table", b"ni3 \xe4\xbd\xa0 16777222 100\nni2 \xff 16777222 300\nyi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1 \xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89\xe5\x9b\x9b\xe4\xba\x94\xe5\x85\xad\xe4\xb8\x83\xe5\x85\xab\xe4\xb9\x9d\xe5\x8d\x81\xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89\xe5\x9b\x9b 16870553 100\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(PHRASE_INDEX_SESSION),
                stderr: b"",
                files: &[],
            },
        )]
        // phrase-index-same-token-invalid-utf8-skip:
        // tests/storage/test_phrase_index.cpp:9-122; tests/tests_helper.h:51-83;
        // src/storage/phrase_index.cpp:56-143,335-389,522-674 at 074a2219.
        #[case::phrase_index_same_token_invalid_utf8_skip(
            table_config(),
            &[],
            b"",
            &[],
            &[("data/gb.table", b"ni3 \xe4\xbd\xa0 16777222 100\nni2'hao3 \xff 16777222 300\nni2 \xe4\xbd\xa0 16777222 300\nyi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1 \xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89\xe5\x9b\x9b\xe4\xba\x94\xe5\x85\xad\xe4\xb8\x83\xe5\x85\xab\xe4\xb9\x9d\xe5\x8d\x81\xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89\xe5\x9b\x9b 16870553 100\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(PHRASE_INDEX_SESSION),
                stderr: b"FacadePhraseIndex::load_text:ni2'hao3\t\xff\n",
                files: &[],
            },
        )]
        // phrase-index-wrong-long-length: tests/storage/test_phrase_index.cpp:9-122;
        // tests/tests_helper.h:51-83; src/storage/phrase_index.cpp:56-143,335-389,522-674 at
        // 074a2219.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; only the pin
        // writes the check macro's message.
        // Platform exception, macos: abort() flushes stdio on macOS, so buffered stdout
        // survives; glibc's discards it.
        #[case::phrase_index_wrong_long_length(
            table_config(),
            &[],
            b"",
            &[],
            &[("data/gb.table", b"ni3 \xe4\xbd\xa0 16777222 100\nni2 \xe4\xbd\xa0 16777222 300\nyi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1 \xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89\xe5\x9b\x9b\xe4\xba\x94\xe5\x85\xad\xe4\xb8\x83\xe5\x85\xab\xe4\xb9\x9d\xe5\x8d\x81\xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89 16870553 100\n".as_slice())],
            Run {
                status: Signal(6),
                stdout: if cfg!(target_os = "macos") {
                    Exact(PHRASE_INDEX_SESSION_WITHOUT_PHRASES)
                } else {
                    Exact(b"")
                },
                stderr: "FacadePhraseIndex::load_text:yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1'yi1\t一二三四五六七八九十一二三\n".as_bytes(),
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
            let mut output = cargo_bin_cmd!("test_phrase_index")
                .current_dir(directory.path().join("tests/storage"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_phrase_index");
            output.stdout = mask_print_time(&output.stdout);
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_phrase_index_logger {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs, undo_tmp_override};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        const LOGGER_CHUNK_CONTIGUOUS: File = Sha256(
            "553199b4da8223cecaa9872a4a754170f9bf1054bc5a331c8e00a353726d7737",
            75,
        );
        const LOGGER_CHUNK_HOLES: File = Sha256(
            "227f3331380cedcc2973b273bbde3c563b6056f16ee9514289c4e759d356f397",
            79,
        );
        const LOGGER_CHUNK_OVERFLOW: File = Sha256(
            "115f71734127505a1e2f7713a6caf1c13956faf40cb2de17e93569f9953854e9",
            75,
        );
        const LOGGER_CHUNK_REFUSED_LAYOUT: File = Sha256(
            "d9357f7bb4a3e0686fe2bf3fa6eb9a89f253c0c102c68229d5b62c8f6f88806f",
            27,
        );
        const LOGGER_CONTIGUOUS_FILES: &[(&str, File)] = &[
            ("tmp/gb_char.bin", LOGGER_CHUNK_CONTIGUOUS),
            ("tmp/gb_char.dbin", LOGGER_DBIN_CONTIGUOUS),
            ("tmp/gb_char2.bin", LOGGER_CHUNK_CONTIGUOUS),
        ];
        const LOGGER_DBIN_CONTIGUOUS: File = Sha256(
            "6c1d47af4534b65ce727468e111447eda131c4e5a5c5ebc1eba804b46ed7b4b1",
            90,
        );
        const TOTAL_FREQ_9_TWICE: &[u8] = b"total freq:9\ntotal freq:9\n";

        #[rstest]
        // phrase-index-logger-holes: tests/storage/test_phrase_index_logger.cpp:26-65;
        // src/storage/phrase_index.h:628-634; src/storage/phrase_index.cpp:335-519,624-645 at
        // 074a2219.
        #[case::phrase_index_logger_holes(
            &[],
            b"",
            &["data"],
            &[("data/gb_char.bin", b"G\0\0\0&\x12P/\x07\0\0\0\x11\0\0\0\"\0\0\0G\0\0\0#\0\0\0\0\x08\0\0\0\0\0\0\0\x16\0\0\0#\0\0\0\0\0\0\0\0\x01\0\x02\0\0\0aO\0\0PAD!\x01\0\x05\0\0\0cO\0\0PAD!#".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(b"total freq:10\ntotal freq:10\n"),
                stderr: b"",
                files: &[
                    ("tmp/gb_char.bin", LOGGER_CHUNK_HOLES),
                    ("tmp/gb_char.dbin", Sha256("9e0951af855fbc60d77cb486bbbebcb535bd9553c38e4ddc0041e5f4a275a7ed", 90)),
                    ("tmp/gb_char2.bin", LOGGER_CHUNK_HOLES),
                ],
            },
        )]
        // phrase-index-logger-ignored-input: tests/storage/test_phrase_index_logger.cpp:26-65;
        // src/storage/phrase_index.h:628-634; src/storage/phrase_index.cpp:335-519,624-645 at
        // 074a2219.
        // Also the logger-delta witness of a contiguous chunk: the arguments and input are ignored.
        #[case::phrase_index_logger_ignored_input(
            &["--help", "ignored"],
            b"ignored\n",
            &["data"],
            &[("data/gb_char.bin", b"C\0\0\0\x1e\x12Q/\x07\0\0\0\x11\0\0\0\x1e\0\0\0C\0\0\0#\0\0\0\0\x08\0\0\0\x16\0\0\0#\0\0\0\0\0\0\0\0\x01\0\x02\0\0\0aO\0\0PAD!\x01\0\x05\0\0\0bO\0\0PAD!#".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(TOTAL_FREQ_9_TWICE),
                stderr: b"",
                files: LOGGER_CONTIGUOUS_FILES,
            },
        )]
        // phrase-index-logger-missing: tests/storage/test_phrase_index_logger.cpp:28-30;
        // src/storage/phrase_index.cpp:335-354 at 074a2219.
        // Class (b), memory safety (docs/findings/compatibility-policy.md, row 117): the pin is not
        // run, and the expectation is the prescribed result.
        #[case::phrase_index_logger_missing(
            &[],
            b"",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // phrase-index-logger-overflow: tests/storage/test_phrase_index_logger.cpp:26-65;
        // src/storage/phrase_index.h:628-634; src/storage/phrase_index.cpp:335-519,624-645 at
        // 074a2219.
        #[case::phrase_index_logger_overflow(
            &[],
            b"",
            &["data"],
            &[("data/gb_char.bin", b"C\0\0\0\xe6\xed\xae\xd0\xff\xff\xff\xff\x11\0\0\0\x1e\0\0\0C\0\0\0#\0\0\0\0\x08\0\0\0\x16\0\0\0#\0\0\0\0\0\0\0\0\x01\0\x02\0\0\0aO\0\0PAD!\x01\0\x05\0\0\0bO\0\0PAD!#".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(b"total freq:1\ntotal freq:1\n"),
                stderr: b"",
                files: &[
                    ("tmp/gb_char.bin", LOGGER_CHUNK_OVERFLOW),
                    ("tmp/gb_char.dbin", Sha256("34555e1c8498700ff6144979b1c44f0c5307e02b7f05f8ee360749e8a4df93c9", 26)),
                    ("tmp/gb_char2.bin", LOGGER_CHUNK_OVERFLOW),
                ],
            },
        )]
        // phrase-index-logger-refused-layout:
        // tests/storage/test_phrase_index_logger.cpp:26-65;
        // src/storage/phrase_index.h:628-634; src/storage/phrase_index.cpp:335-519,624-645 at
        // 074a2219.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; only the pin
        // writes the check macro's message.
        #[case::phrase_index_logger_refused_layout(
            &[],
            b"",
            &["data"],
            &[("data/gb_char.bin", b"\x13\0\0\x006##\0\x07\0\0\0\x11\0\0\0\x12\0\0\0\x13\0\0\0!##".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(b"total freq:0\ntotal freq:-7\n"),
                stderr: b"",
                files: &[
                    ("tmp/gb_char.bin", LOGGER_CHUNK_REFUSED_LAYOUT),
                    ("tmp/gb_char.dbin", Sha256("caf03568496a44e18b46b238c8805df2f0840c99862b8967e8f4ed48f2587d8c", 26)),
                    ("tmp/gb_char2.bin", LOGGER_CHUNK_REFUSED_LAYOUT),
                ],
            },
        )]
        // phrase-index-logger-save-directory:
        // tests/storage/test_phrase_index_logger.cpp:26-65;
        // src/storage/phrase_index.h:628-634; src/storage/phrase_index.cpp:335-519,624-645 at
        // 074a2219.
        #[case::phrase_index_logger_save_directory(
            &[],
            b"",
            &["data", "tmp/gb_char.bin"],
            &[("data/gb_char.bin", b"C\0\0\0\x1e\x12Q/\x07\0\0\0\x11\0\0\0\x1e\0\0\0C\0\0\0#\0\0\0\0\x08\0\0\0\x16\0\0\0#\0\0\0\0\0\0\0\0\x01\0\x02\0\0\0aO\0\0PAD!\x01\0\x05\0\0\0bO\0\0PAD!#".as_slice()), ("tmp/gb_char.bin/occupied", b"kept".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(TOTAL_FREQ_9_TWICE),
                stderr: b"",
                files: &[
                    ("tmp/gb_char.dbin", LOGGER_DBIN_CONTIGUOUS),
                    ("tmp/gb_char2.bin", LOGGER_CHUNK_CONTIGUOUS),
                ],
            },
        )]
        // phrase-index-logger-short: tests/storage/test_phrase_index_logger.cpp:28-30;
        // src/storage/phrase_index.cpp:335-354 at 074a2219.
        // Class (b), memory safety (docs/findings/compatibility-policy.md, row 117): the pin is not
        // run, and the expectation is the prescribed result.
        #[case::phrase_index_logger_short(
            &[],
            b"",
            &["data"],
            &[("data/gb_char.bin", b"\0\0\0\0".as_slice())],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        fn captured_cases(
            #[case] args: &[&str],
            #[case] stdin: &[u8],
            #[case] directories: &[&str],
            #[case] files: &[(&str, &[u8])],
            #[case] expected: Run,
        ) {
            let directory = TempDir::new().expect("scratch directory");
            directory
                .child("tmp")
                .create_dir_all()
                .expect("private fixed names");
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
            for (name, content) in files {
                directory
                    .child(name)
                    .write_binary(content)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("test_phrase_index_logger")
                .current_dir(directory.path().join("tests/storage"))
                .env("LC_ALL", "C")
                .env(
                    oxpinyin_utils::fixed_tmp::OVERRIDE,
                    directory.path().join("tmp"),
                )
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_phrase_index_logger");
            output.stderr = undo_tmp_override(
                &mask_glib_prefix(&output.stderr),
                &directory.path().join("tmp"),
            );
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_phrase_lookup {
        use super::{OPEN_ABSENT_BIN_FAILED, OPEN_DATA_TABLE_CONF_FAILED};
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
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; only the pin
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
        #[case::phrase_lookup_missing_config(
            TempDir::new().unwrap(),
            &[],
            b"quit\n",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_DATA_TABLE_CONF_FAILED,
                files: &[],
            },
        )]
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
            let mut output = cargo_bin_cmd!("test_phrase_lookup")
                .current_dir(directory.path().join("tests/lookup"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_phrase_lookup");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_phrase_table {
        use super::{OPEN_DATA_TABLE_CONF_FAILED, OPEN_GB_TABLE_FAILED};
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
        #[case::phrase_table_missing_config(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_DATA_TABLE_CONF_FAILED,
                files: &[],
            },
        )]
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
                stdout: Exact(include_bytes!("fixtures/test_phrase_table/session.txt")),
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
            let mut output = cargo_bin_cmd!("test_phrase_table")
                .current_dir(directory.path().join("tests/storage"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_phrase_table");
            output.stdout = mask_print_time(&output.stdout);
            output.stderr = mask_glib_prefix(&output.stderr);
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
                stdout: Exact(include_bytes!("fixtures/test_pinyin/nihao-session-embedded-nul.txt")),
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
                stdout: Exact(include_bytes!("fixtures/test_pinyin/nihao-zhongguo-session.txt")),
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
            let mut output = cargo_bin_cmd!("test_pinyin")
                .current_dir(directory.path().join("tests"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_pinyin");
            output.stderr = mask_glib_prefix(&output.stderr);
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
        use super::{OPEN_ABSENT_BIN_FAILED, OPEN_DATA_TABLE_CONF_FAILED};
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
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; only the pin
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
                stdout: Exact(include_bytes!("fixtures/test_pinyin_lookup/six-char-lookup.txt")),
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
        #[case::pinyin_lookup_missing_config(
            TempDir::new().unwrap(),
            &[],
            b"quit\n",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_DATA_TABLE_CONF_FAILED,
                files: &[],
            },
        )]
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
                stdout: Exact(include_bytes!("fixtures/test_pinyin_lookup/session.txt")),
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
            let mut output = cargo_bin_cmd!("test_pinyin_lookup")
                .current_dir(directory.path().join("tests/lookup"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_pinyin_lookup");
            output.stdout = mask_print_time(&output.stdout);
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod test_table_info {
        use super::{OPEN_DATA_TABLE_CONF_FAILED, USER_CONF_KYOTO_CABINET, USER_CONF_TKRZW};
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs, undo_tmp_override};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        const TKRZW_USER_CONF_FILES: &[(&str, File)] = &[("tmp/user.conf", USER_CONF_TKRZW)];

        #[rstest]
        // table-info-all-row-types: tests/storage/test_table_info.cpp:26-99;
        // src/storage/table_info.cpp:194-419 at 074a2219.
        #[case::table_info_all_row_types(
            &["--help", "ignored-file"],
            b"ignored\n",
            &["data"],
            &[("data/table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\ndefault GB_DICTIONARY gb.table gb.bin gb.dbin SYSTEM_FILE\ndefault USER_DICTIONARY NULL NULL user.bin USER_FILE\naddon 3 addon.table addon.bin NULL DICTIONARY\naddon 4 ignored.table ignored.bin ignored.user NOT_USED\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/test_table_info/all-row-types.txt")),
                stderr: b"",
                files: TKRZW_USER_CONF_FILES,
            },
        )]
        // table-info-empty-tables: tests/storage/test_table_info.cpp:26-99;
        // src/storage/table_info.cpp:194-419 at 074a2219.
        // The version values go to oxpinyin-data's user-file writer unchanged; the zero-version
        // policy cases below hold the fresh-marker rule.
        #[case::table_info_empty_tables(
            &[],
            b"",
            &["data"],
            &[("data/table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/test_table_info/empty-tables.txt")),
                stderr: b"",
                files: TKRZW_USER_CONF_FILES,
            },
        )]
        // table-info-malformed-header: tests/storage/test_table_info.cpp:26-99;
        // src/storage/table_info.cpp:194-419 at 074a2219.
        #[case::table_info_malformed_header(
            &[],
            b"",
            &["data"],
            &[("data/table.conf", b"binary format version:nope\n".as_slice())],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: b"load table.conf failed.\n",
                files: &[],
            },
        )]
        // table-info-missing-config: tests/storage/test_table_info.cpp:26-99;
        // src/storage/table_info.cpp:194-419 at 074a2219.
        #[case::table_info_missing_config(
            &[],
            b"",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_DATA_TABLE_CONF_FAILED,
                files: &[],
            },
        )]
        // table-info-null-names: tests/storage/test_table_info.cpp:26-99;
        // src/storage/table_info.cpp:194-419 at 074a2219.
        #[case::table_info_null_names(
            &[],
            b"",
            &["data"],
            &[("data/table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\ndefault GB_DICTIONARY NULL NULL NULL SYSTEM_FILE\naddon 2 NULL NULL NULL DICTIONARY\naddon 3 NULL NULL NULL USER_FILE\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/test_table_info/null-names.txt")),
                stderr: b"",
                files: TKRZW_USER_CONF_FILES,
            },
        )]
        // table-info-overwritten-row: tests/storage/test_table_info.cpp:26-99;
        // src/storage/table_info.cpp:194-419 at 074a2219.
        #[case::table_info_overwritten_row(
            &[],
            b"",
            &["data"],
            &[("data/table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.37\nsource table format:zhuyin\ndatabase format:KyotoCabinet\naddon 15 old.table old.bin old.user SYSTEM_FILE\naddon 1 a.table a.bin NULL DICTIONARY\naddon 15 new.table new.bin NULL DICTIONARY\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/test_table_info/overwritten-row.txt")),
                stderr: b"",
                files: &[
                    ("tmp/user.conf", USER_CONF_KYOTO_CABINET),
                ],
            },
        )]
        // table-info-save-fails: tests/storage/test_table_info.cpp:26-99;
        // src/storage/table_info.cpp:194-419 at 074a2219.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; only the pin
        // writes the check macro's message.
        // Platform exception, macos: abort() flushes stdio on macOS, so buffered stdout
        // survives; glibc's discards it.
        #[case::table_info_save_fails(
            &[],
            b"",
            &["data", "tmp/user.conf"],
            &[("data/table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\n".as_slice()), ("tmp/user.conf/child", b"occupied".as_slice())],
            Run {
                status: Signal(6),
                stdout: if cfg!(target_os = "macos") {
                    Exact(include_bytes!("fixtures/test_table_info/empty-tables.txt"))
                } else {
                    Exact(b"")
                },
                stderr: b"write /tmp/user.conf failed.\n",
                files: &[],
            },
        )]
        // table-info-unknown-database: tests/storage/test_table_info.cpp:26-99;
        // src/storage/table_info.cpp:194-419 at 074a2219.
        #[case::table_info_unknown_database(
            &[],
            b"",
            &["data"],
            &[("data/table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:unknown\n".as_slice())],
            Run {
                status: Signal(6),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // table-info-unknown-source: tests/storage/test_table_info.cpp:26-99;
        // src/storage/table_info.cpp:194-419 at 074a2219.
        #[case::table_info_unknown_source(
            &[],
            b"",
            &["data"],
            &[("data/table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:unknown\ndatabase format:Tkrzw\n".as_slice())],
            Run {
                status: Signal(6),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // table-info-zero-versions-pinyin: tests/storage/test_table_info.cpp:26-99;
        // src/storage/table_info.cpp:194-419 at 074a2219.
        // Class (b), memory safety (docs/findings/compatibility-policy.md, amendment "fresh
        // UserTableInfo with zero versions"): the pin is not run, and the expectation is the
        // prescribed result.
        #[case::table_info_zero_versions_pinyin(
            &[],
            b"",
            &["data"],
            &[("data/table.conf", b"binary format version:0\nmodel data version:0\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/test_table_info/empty-tables.txt")),
                stderr: b"",
                files: &[
                    ("tmp/user.conf", Sha256("62ec00537bb089c3a544e3f4ed6a65f97e9611a5cd5b4ba59ccb1f41485a94ce", 82)),
                ],
            },
        )]
        // table-info-zero-versions-zhuyin: tests/storage/test_table_info.cpp:26-99;
        // src/storage/table_info.cpp:194-419 at 074a2219.
        // Class (b), memory safety (docs/findings/compatibility-policy.md, amendment "fresh
        // UserTableInfo with zero versions"): the pin is not run, and the expectation is the
        // prescribed result.
        #[case::table_info_zero_versions_zhuyin(
            &[],
            b"",
            &["data"],
            &[("data/table.conf", b"binary format version:0\nmodel data version:0\nlambda parameter:0.5\nsource table format:zhuyin\ndatabase format:KyotoCabinet\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/test_table_info/empty-tables.txt")),
                stderr: b"",
                files: &[
                    ("tmp/user.conf", Sha256("90230536c613fb4910d6220af5ac5156c3083c092d4f5e8b37e18e16b0371efc", 89)),
                ],
            },
        )]
        fn captured_cases(
            #[case] args: &[&str],
            #[case] stdin: &[u8],
            #[case] directories: &[&str],
            #[case] files: &[(&str, &[u8])],
            #[case] expected: Run,
        ) {
            let directory = TempDir::new().expect("scratch directory");
            directory
                .child("tmp")
                .create_dir_all()
                .expect("private fixed names");
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
            for (name, content) in files {
                directory
                    .child(name)
                    .write_binary(content)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("test_table_info")
                .current_dir(directory.path().join("tests/storage"))
                .env("LC_ALL", "C")
                .env(
                    oxpinyin_utils::fixed_tmp::OVERRIDE,
                    directory.path().join("tmp"),
                )
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_table_info");
            output.stderr = undo_tmp_override(
                &mask_glib_prefix(&output.stderr),
                &directory.path().join("tmp"),
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
            let mut output = cargo_bin_cmd!("test_zhuyin")
                .current_dir(directory.path().join("tests"))
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_zhuyin");
            output.stderr = mask_glib_prefix(&output.stderr);
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
