//! Frozen pin observations of `gen_zhuyin_table`; pin citations are in each case.
//! The cases outside `frozen` hold defined results, which run
//! only on oxpinyin.

use std::path::Path;
use std::process::{Command, Output};

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gen_zhuyin_table"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run gen_zhuyin_table")
}

/// Register entry "gen_zhuyin_table truncated final lead byte ": at 074a2219 `g_utf8_strlen` steps past the end of the
/// `g_strsplit_set` field (`gen_zhuyin_table.cpp:150,168`), so the pin's
/// result is undefined and nothing runs on it. oxpinyin's prescribed
/// result: the count stops on zero padding and the row takes the invalid
/// UTF-8 phrase refusal.
#[test]
fn a_phrase_ending_in_a_truncated_lead_byte_is_refused() {
    let newline = oxpinyin::input::TEXT_NEWLINE;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("in.txt"), b"\xe3 1 \xe3\x84\x9a\n").unwrap();
    let out = run(dir.path(), &["-o", "out.table", "in.txt"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, format!("nnodes: 1{newline}").into_bytes());
    let mut stderr = b"gen_zhuyin_table: invalid UTF-8 phrase: \xe3".to_vec();
    stderr.extend_from_slice(newline.as_bytes());
    assert_eq!(out.stderr, stderr);
    assert_eq!(std::fs::read(dir.path().join("out.table")).unwrap(), b"");
}

/// Register entry "Generator phrase index of 256 or more, or negative ": the zhuyin generator shifts its `gint` `-t` value
/// the same way (`utils/storage/gen_zhuyin_table.cpp:42,47` at 074a2219).
/// For 256 or more, or a negative value, the shift is undefined, so nothing
/// runs on the pin. oxpinyin's prescribed result: the library nibble is the
/// value modulo 16. The frozen case `zhuyin-phrase-index-131` holds the
/// defined range.
#[test]
fn a_phrase_index_outside_the_int_shift_range_wraps_into_the_library_nibble() {
    let newline = oxpinyin::input::TEXT_NEWLINE;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("in.txt"), "你 5 ㄋㄧ\n").unwrap();
    let cases: [(&[&str], u32); 2] = [
        (&["-t", "259"], 0x0300_0001),
        (&["--phraseindex=-1"], 0x0F00_0001),
    ];
    for (option, token) in cases {
        let args = [option, &["-o", "out.table", "in.txt"]].concat();
        let out = run(dir.path(), &args);
        assert_eq!(out.status.code(), Some(0), "{option:?}");
        assert_eq!(
            String::from_utf8(std::fs::read(dir.path().join("out.table")).unwrap()).unwrap(),
            format!("ㄋㄧ\t你\t{token}\t5{newline}"),
            "{option:?}"
        );
    }
}

// Unix only: the frozen pin runs are Linux and macOS observations (signal statuses, Unix paths and line endings).
#[cfg(unix)]
mod frozen {
    mod gen_zhuyin_table {
        use crate::support::expected::*;
        use crate::support::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;

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
                stdout: GlibOption(include_str!("../fixtures/gen_zhuyin_table/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // zhuyin-invalid-utf8: utils/storage/gen_zhuyin_table.cpp at 074a2219: 101-334.
        // Policy-accepted on every cell:
        // docs/findings/upstream-divergences.md#gen_pinyin_table-invalid-utf-8-phrase-class-b; the
        // pin dies with SIGSEGV where oxpinyin refuses with exit 1.
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
        // Policy-accepted on every cell:
        // docs/findings/upstream-divergences.md#gen_pinyin_table-invalid-utf-8-phrase-class-b; the
        // pin dies with SIGSEGV where oxpinyin refuses with exit 1.
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
        // Policy-accepted on every cell:
        // docs/findings/upstream-divergences.md#gen_zhuyin_table-missing-frequency-class-b; the pin
        // dies with SIGSEGV where oxpinyin refuses with exit 1.
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
        // zhuyin-phrase-index-131: pin capture runs 1 and 2 of utility/tkrzw/zhuyin-phrase-index-131
        // (tools/oracle/pin_capture.py), equal and approved. libpinyin 074a2219:
        // utils/storage/gen_zhuyin_table.cpp:42,47 (a gint) and src/include/novel_types.h:45-46. The defined
        // 128..=255 range of policy row 127: library nibble 131 mod 16 = 3, token 50331649.
        #[case::zhuyin_phrase_index_131(
            &["-t", "131", "-o", "temp.out", "/dev/stdin"],
            b"\xe4\xbd\xa0 5 \xe3\x84\x8b\xe3\x84\xa7\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(NNODES_1),
                stderr: b"",
                files: &[(
                    "temp.out",
                    Sha256("c3c2391fd3d05b3129c94ff3a9bd793514eca567259267a2e9adca8da0613c00", 22),
                )],
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
                .output()
                .expect("run gen_zhuyin_table");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
}
