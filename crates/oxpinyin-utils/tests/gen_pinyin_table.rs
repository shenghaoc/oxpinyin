//! `gen_pinyin_table` against results derived from the pin's source
//! (libpinyin 074a2219 `utils/storage/gen_pinyin_table.cpp`), run as the
//! built binary so the C streams, exit statuses and GLib lines are real.

use oxpinyin_input::tool;
use std::path::Path;
use std::process::{Command, Output};

// Each test builds its expected output with the host's newline: the
// utility writes stdout, stderr and its table through the C runtime's
// text-mode streams, which end lines with CRLF on Windows and LF elsewhere
// (docs/findings/external-io-boundary.md, "Native text-mode line endings").

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gen_pinyin_table"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run gen_pinyin_table")
}

#[test]
fn rows_follow_length_then_first_reading_then_phrase_bytes() {
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("in.txt"),
        "你好 ni'hao 10\n你 ni 5\n尼 ni 2\n你好 nihao 4\n呢 ne 1\n好 hao3 7\n好 hao4 2\n\
         你们 ni 3\n一二三四五六七八九十一二三四五六 yi 1\n",
    )
    .unwrap();
    let out = run(dir.path(), &["-t", "3", "-o", "out.table", "in.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("nnodes: 5{newline}")
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Duplicate item: phrase:你好\tpinyin:nihao\tfreq:4\n\
         Invalid pinyin:你们\tni\t3\n\
         Too long phrase:一二三四五六七八九十一二三四五六\tyi\t1\n"
            .replace('\n', newline)
    );
    // gen_phrase_file writes a frequency under 3 as 3: `/* avoid zero
    // freq. */ if (freq < 3) freq = 3;` (`gen_pinyin_table.cpp:311-312` at
    // 074a2219), before the row's fprintf (`:314-316`). Equal first readings
    // keep the tree's order: `memcmp` over the host-order UCS-4 words.
    // U+5C3C 尼 sorts before U+4F60 你 on a little-endian host
    // (0x3C < 0x60) and after it on a big-endian one.
    let (first, second) = if cfg!(target_endian = "little") {
        ("ni\t尼\t50331651\t3\n", "ni\t你\t50331652\t5\n")
    } else {
        ("ni\t你\t50331651\t5\n", "ni\t尼\t50331652\t3\n")
    };
    let expected = format!(
        "hao3\t好\t50331649\t7\nhao4\t好\t50331649\t3\nne\t呢\t50331650\t3\n\
         {first}{second}ni'hao\t你好\t50331653\t14\n"
    )
    .replace('\n', newline);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("out.table")).unwrap(),
        expected
    );
}

#[test]
fn the_default_output_is_temp_out_and_a_last_row_without_newline_is_dropped() {
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("in.txt"), "你 ni 5\n好 hao 3").unwrap();
    let out = run(dir.path(), &["in.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("nnodes: 1{newline}")
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("temp.out")).unwrap(),
        format!("ni\t你\t1\t5{newline}")
    );
}

#[test]
fn a_missing_input_exits_enoent_before_writing() {
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["absent.txt"]);
    assert_eq!(out.status.code(), Some(tool::ENOENT));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        format!("Can't open file absent.txt.{newline}")
    );
    assert!(!dir.path().join("temp.out").exists());
}

#[test]
fn an_unknown_option_exits_einval_with_the_glib_message() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["--bogus"]);
    assert_eq!(out.status.code(), Some(tool::EINVAL));
    assert!(
        String::from_utf8_lossy(&out.stdout).starts_with("option parsing failed:"),
        "{out:?}"
    );
}

#[test]
fn an_empty_phrase_counts_as_a_node_and_meets_glibs_null_array_check() {
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("in.txt"), b"\0 - 1\n\xe4\xbd\xa0 ni 5\n").unwrap();
    let out = run(dir.path(), &["in.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("nnodes: 2{newline}")
    );
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("g_array_append_vals: assertion 'array' failed"),
        "{out:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("temp.out")).unwrap(),
        format!("ni\t你\t1\t5{newline}")
    );
}

/// An unconverted phrase reaches the output stage as the pin's NULL
/// `uniphrase` does: every input is read and the output file is opened
/// (`gen_pinyin_table.cpp:110-112`, `:261`) before the row that reads it
/// (`:280-281`). There the pin dies of undefined behaviour; oxpinyin names
/// the phrase and exits 1, flushing what it wrote (class (b)).
#[test]
fn an_unconvertible_phrase_is_refused_at_the_pins_row_after_the_output_opens() {
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    for (input, nnodes, phrase, rows) in [
        (&b"\xff a 1\n"[..], "nnodes: 1\n", &b"\xff"[..], ""),
        (
            &b"\xe4\xbd\xa0 ni 5\n\xff\xff a'a 1\n"[..],
            "nnodes: 2\n",
            &b"\xff\xff"[..],
            "ni\t你\t1\t5\n",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("in.txt"), input).unwrap();
        let out = run(dir.path(), &["in.txt"]);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            nnodes.replace('\n', newline)
        );
        assert_eq!(
            out.stderr,
            [
                &b"gen_pinyin_table: invalid UTF-8 phrase: "[..],
                phrase,
                newline.as_bytes()
            ]
            .concat()
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("temp.out")).unwrap(),
            rows.replace('\n', newline)
        );
    }
}

#[cfg(unix)]
mod common;

#[cfg(unix)]
mod frozen {
    mod gen_pinyin_table {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        const PINYIN_NI_TEMP_OUT_FILES: &[(&str, File)] = &[(
            "temp.out",
            Sha256(
                "a9ab04a2f3ae0c6587e1204e575782b7bd8d3ed5e01bd9471b2f7421fa9187c5",
                11,
            ),
        )];

        #[rstest]
        // pinyin-bad-option: utils/storage/gen_pinyin_table.cpp at 074a2219: 97-322.
        #[case::pinyin_bad_option(
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
        // pinyin-empty-phrase: utils/storage/gen_pinyin_table.cpp at 074a2219: 97-322.
        #[case::pinyin_empty_phrase(
            &["in.txt"],
            b"",
            &[("in.txt", b"\0 - 1\n\xe4\xbd\xa0 ni 5\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(NNODES_2),
                stderr: b"\n(gen_pinyin_table:PID): GLib-CRITICAL **: TIME: g_array_append_vals: assertion 'array' failed\n",
                files: PINYIN_NI_TEMP_OUT_FILES,
            },
        )]
        // pinyin-help: utils/storage/gen_pinyin_table.cpp at 074a2219: 97-322.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::pinyin_help(
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/gen_pinyin_table/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // pinyin-invalid-utf8: utils/storage/gen_pinyin_table.cpp at 074a2219: 97-322.
        // Policy-accepted on every cell: docs/findings/eio-table-generator-invalid-utf8.md;
        // the pin's own result is in
        // fixtures/contract-policy/eio-table-generator-invalid-utf8.json.
        #[case::pinyin_invalid_utf8(
            &["in.txt"],
            b"",
            &[("in.txt", b"\xff a 1\n".as_slice())],
            Run {
                status: Exit(1),
                stdout: Exact(NNODES_1),
                stderr: b"gen_pinyin_table: invalid UTF-8 phrase: \xff\n",
                files: EMPTY_TEMP_OUT_FILES,
            },
        )]
        // pinyin-invalid-utf8-after-row: utils/storage/gen_pinyin_table.cpp at 074a2219:
        // 97-322.
        // Policy-accepted on every cell: docs/findings/eio-table-generator-invalid-utf8.md;
        // the pin's own result is in
        // fixtures/contract-policy/eio-table-generator-invalid-utf8.json.
        #[case::pinyin_invalid_utf8_after_row(
            &["in.txt"],
            b"",
            &[("in.txt", b"\xe4\xbd\xa0 ni 5\n\xff\xff a'a 1\n".as_slice())],
            Run {
                status: Exit(1),
                stdout: Exact(NNODES_2),
                stderr: b"gen_pinyin_table: invalid UTF-8 phrase: \xff\xff\n",
                files: PINYIN_NI_TEMP_OUT_FILES,
            },
        )]
        // pinyin-missing-input: utils/storage/gen_pinyin_table.cpp at 074a2219: 97-322.
        #[case::pinyin_missing_input(
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
        // pinyin-no-final-newline: utils/storage/gen_pinyin_table.cpp at 074a2219: 97-322.
        #[case::pinyin_no_final_newline(
            &["in.txt"],
            b"",
            &[("in.txt", b"\xe4\xbd\xa0 ni 5\n\xe5\xa5\xbd hao 3".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(NNODES_1),
                stderr: b"",
                files: PINYIN_NI_TEMP_OUT_FILES,
            },
        )]
        // pinyin-no-input: utils/storage/gen_pinyin_table.cpp at 074a2219: 97-322.
        #[case::pinyin_no_input(
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
        // pinyin-normal: utils/storage/gen_pinyin_table.cpp at 074a2219: 97-322.
        #[case::pinyin_normal(
            &["-t", "3", "-o", "out.table", "in.txt"],
            b"",
            &[("in.txt", b"\xe4\xbd\xa0\xe5\xa5\xbd ni'hao 10\n\xe4\xbd\xa0 ni 5\n\xe5\xb0\xbc ni 2\n\xe4\xbd\xa0\xe5\xa5\xbd nihao 4\n\xe5\x91\xa2 ne 1\n\xe5\xa5\xbd hao3 7\n\xe5\xa5\xbd hao4 2\n\xe4\xbd\xa0\xe4\xbb\xac ni 3\n\xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89\xe5\x9b\x9b\xe4\xba\x94\xe5\x85\xad\xe4\xb8\x83\xe5\x85\xab\xe4\xb9\x9d\xe5\x8d\x81\xe4\xb8\x80\xe4\xba\x8c\xe4\xb8\x89\xe5\x9b\x9b\xe4\xba\x94\xe5\x85\xad yi 1\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(NNODES_5),
                stderr: "Duplicate item: phrase:你好\tpinyin:nihao\tfreq:4\nInvalid pinyin:你们\tni\t3\nToo long phrase:一二三四五六七八九十一二三四五六\tyi\t1\n".as_bytes(),
                files: &[
                    ("out.table", Sha256("16a99a9a9d90be757bfaf0470843a16aef77f81f19e528a3186b5bfce9a051ee", 120)),
                ],
            },
        )]
        // pinyin-output-failure: utils/storage/gen_pinyin_table.cpp at 074a2219: 97-322.
        #[case::pinyin_output_failure(
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
            let mut output = cargo_bin_cmd!("gen_pinyin_table")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run gen_pinyin_table");
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
