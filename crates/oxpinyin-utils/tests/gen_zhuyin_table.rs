//! Frozen pin observations of `gen_zhuyin_table`; pin citations are in each case.
#[cfg(unix)]
mod common;

// Unix only: the frozen pin runs are Linux and macOS observations (signal statuses, Unix paths and line endings).
#[cfg(unix)]
mod frozen {
    mod gen_zhuyin_table {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        const ZHUYIN_NI_TEMP_OUT_FILES: &[(&str, File)] = &[(
            "temp.out",
            Sha256(
                "988db3608b0dc502d181e771abdbe14305ca6aef5208724775021ce754a6dcd7",
                15,
            ),
        )];

        #[rstest]
        // zhuyin-bad-option: utils/storage/gen_zhuyin_table.cpp at 074a2219: 101-334.
        #[case::zhuyin_bad_option(
            &["--bogus"],
            b"",
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(UNKNOWN_OPTION_BOGUS),
                stderr: b"",
                files: &[],
            },
        )]
        // zhuyin-empty-phrase: utils/storage/gen_zhuyin_table.cpp at 074a2219: 101-334.
        #[case::zhuyin_empty_phrase(
            &["in.txt"],
            b"",
            &[("in.txt", b" 1 \n\xe4\xbd\xa0 5 \xe3\x84\x8b\xe3\x84\xa7\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(NNODES_2),
                stderr: b"\n(gen_zhuyin_table:PID): GLib-CRITICAL **: TIME: g_array_append_vals: assertion 'array' failed\n",
                files: ZHUYIN_NI_TEMP_OUT_FILES,
            },
        )]
        // zhuyin-help: utils/storage/gen_zhuyin_table.cpp at 074a2219: 101-334.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::zhuyin_help(
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/gen_zhuyin_table/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // zhuyin-invalid-utf8: utils/storage/gen_zhuyin_table.cpp at 074a2219: 101-334.
        // Policy-accepted on every cell: docs/findings/eio-table-generator-invalid-utf8.md;
        // the pin's own result is in
        // fixtures/contract-policy/eio-table-generator-invalid-utf8.json.
        #[case::zhuyin_invalid_utf8(
            &["in.txt"],
            b"",
            &[("in.txt", b"\xff 1 \xe3\x84\x9a\n".as_slice())],
            Run {
                status: Exit(1),
                stdout: Exact(NNODES_1),
                stderr: b"gen_zhuyin_table: invalid UTF-8 phrase: \xff\n",
                files: EMPTY_TEMP_OUT_FILES,
            },
        )]
        // zhuyin-invalid-utf8-after-row: utils/storage/gen_zhuyin_table.cpp at 074a2219:
        // 101-334.
        // Policy-accepted on every cell: docs/findings/eio-table-generator-invalid-utf8.md;
        // the pin's own result is in
        // fixtures/contract-policy/eio-table-generator-invalid-utf8.json.
        #[case::zhuyin_invalid_utf8_after_row(
            &["in.txt"],
            b"",
            &[("in.txt", b"\xe4\xbd\xa0 5 \xe3\x84\x8b\xe3\x84\xa7\n\xff\xff 1 \xe3\x84\x9a'\xe3\x84\x9a\n".as_slice())],
            Run {
                status: Exit(1),
                stdout: Exact(NNODES_2),
                stderr: b"gen_zhuyin_table: invalid UTF-8 phrase: \xff\xff\n",
                files: ZHUYIN_NI_TEMP_OUT_FILES,
            },
        )]
        // zhuyin-missing-frequency: utils/storage/gen_zhuyin_table.cpp at 074a2219: 101-334.
        // Policy-accepted on every cell: docs/findings/eio-zhuyin-missing-frequency.md; the
        // pin's own result is in fixtures/contract-policy/eio-zhuyin-missing-frequency.json.
        #[case::zhuyin_missing_frequency(
            &["in.txt"],
            b"",
            &[("in.txt", b"\xe4\xbd\xa0\xe5\xa5\xbd\n".as_slice())],
            Run {
                status: Exit(1),
                stdout: Exact(b""),
                stderr: "gen_zhuyin_table: missing frequency: 你好\n".as_bytes(),
                files: &[],
            },
        )]
        // zhuyin-missing-input: utils/storage/gen_zhuyin_table.cpp at 074a2219: 101-334.
        #[case::zhuyin_missing_input(
            &["absent.txt"],
            b"",
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: CANT_OPEN_ABSENT_TXT,
                files: &[],
            },
        )]
        // zhuyin-no-final-newline: utils/storage/gen_zhuyin_table.cpp at 074a2219: 101-334.
        #[case::zhuyin_no_final_newline(
            &["in.txt"],
            b"",
            &[("in.txt", b"\xe4\xbd\xa0 5 \xe3\x84\x8b\xe3\x84\xa7\n\xe5\xa5\xbd 3 \xe3\x84\x8f\xe3\x84\xa0".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(NNODES_2),
                stderr: b"",
                files: &[
                    ("temp.out", Sha256("e05f119c90f1c9a9de37c8e0a5945eacfa6b32ef6742352449e9bb9e569e579c", 30)),
                ],
            },
        )]
        // zhuyin-no-input: utils/storage/gen_zhuyin_table.cpp at 074a2219: 101-334.
        #[case::zhuyin_no_input(
            &[],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(NNODES_0),
                stderr: b"",
                files: EMPTY_TEMP_OUT_FILES,
            },
        )]
        // zhuyin-normal: utils/storage/gen_zhuyin_table.cpp at 074a2219: 101-334.
        #[case::zhuyin_normal(
            &["-t", "3", "-o", "out.table", "in.txt"],
            b"",
            &[("in.txt", b"\xe4\xbd\xa0\xe5\xa5\xbd 10 \xe3\x84\x8b\xe3\x84\xa7\xcb\x87 \xe3\x84\x8f\xe3\x84\xa0\xcb\x87\n\xe4\xbd\xa0 5 \xe3\x84\x8b\xe3\x84\xa7\xcb\x87\n\xe5\xb0\xbc 2 \xe3\x84\x8b\xe3\x84\xa7\xcb\x87\n\xe4\xbd\xa0\xe5\xa5\xbd 4 \xe3\x84\x8b\xe3\x84\xa7\xcb\x87'\xe3\x84\x8f\xe3\x84\xa0\xcb\x87\n\xe5\x91\xa2 1 \xe3\x84\x8b\xe3\x84\x9c\n\xe5\xa5\xbd 7 \xe3\x84\x8f\xe3\x84\xa0\xcb\x87\n\xe5\xa5\xbd 2 \xe3\x84\x8f\xe3\x84\xa0\xcb\x8b\n\xe4\xbd\xa0\xe4\xbb\xac 3 \xe3\x84\x8b\xe3\x84\xa7\n\xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89\xe5\x9b\x9b\xe4\xba\x94\xe5\x85\xad\xe4\xb8\x83\xe5\x85\xab\xe4\xb9\x9d\xe5\x8d\x81\xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89\xe5\x9b\x9b\xe4\xba\x94\xe5\x85\xad 1 \xe3\x84\xa7\n\xe4\xbd\xa0 5\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(NNODES_5),
                stderr: "Duplicate item: phrase:你好\tpinyin:ㄋㄧˇ'ㄏㄠˇ\tfreq:4\nInvalid pinyin:你们\tㄋㄧ\t3\nToo long phrase:一二三四五六七八九十一二三四五六\tㄧ\t1\nwrong line format:你 5\n".as_bytes(),
                files: &[
                    ("out.table", Sha256("05c6c592294d277fff83fe675a5c1d1a74317340af97cde0880a18bdb0529399", 155)),
                ],
            },
        )]
        // zhuyin-output-failure: utils/storage/gen_zhuyin_table.cpp at 074a2219: 101-334.
        #[case::zhuyin_output_failure(
            &["-o", "missing/out.table"],
            b"",
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(NNODES_0),
                stderr: CANT_WRITE_MISSING_OUT_TABLE,
                files: &[],
            },
        )]
        fn captured_cases(
            #[case] args: &[&str],
            #[case] stdin: &[u8],
            #[case] files: &[(&str, &[u8])],
            #[case] expected: Run,
        ) {
            let directory = TempDir::new().expect("scratch directory");
            for (name, content) in files {
                directory
                    .child(name)
                    .write_binary(content)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("gen_zhuyin_table")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run gen_zhuyin_table");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
}
