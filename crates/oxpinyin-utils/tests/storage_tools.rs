//! Frozen product observations of the three storage utilities: status,
//! both streams and every file the run leaves.
#[cfg(unix)]
mod common;
#[cfg(unix)]
mod fixtures;

// Unix only: the frozen pin runs are Linux and macOS observations (signal statuses, Unix paths and line endings).
#[cfg(unix)]
mod frozen {
    use crate::common::expected::*;

    const EMPTY_PHRASE_CHUNK: File = Sha256(
        "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697",
        27,
    );
    const UNKNOWN_OPTION_NO_SUCH_OPTION: &[u8] =
        b"option parsing failed:Unknown option --no-such-option\n";

    mod export_interpolation {
        use crate::common::CELL;
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

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
        // Policy-accepted on bdb:
        // docs/findings/upstream-divergences.md#berkeley-db-handles-whose-open-failed-policy-row-113;
        // only the pin writes Berkeley DB's BDB1565 lines.
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
        use crate::common::CELL;
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::UNKNOWN_OPTION_NO_SUCH_OPTION;
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
}
