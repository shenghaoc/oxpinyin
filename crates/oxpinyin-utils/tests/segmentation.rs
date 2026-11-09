//! The pin-named segmentation utilities over the committed W3 data.
//!
//! Every test runs the built binary (`CARGO_BIN_EXE_*`) in a private copy of
//! `fixtures/w3/<backend>` — the committed mini system data directory — so
//! the relative `table.conf`, `phrase_index.bin` and chunk names are the
//! ones the pin opens. Expectations come from the pin's source
//! (`utils/segment/*.cpp` at 074a2219) and its committed W9 goldens, not
//! from a live run.

use oxpinyin_input::tool;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// `fixtures/w3/<backend>`: `db` on BDB, `kct` on KC, `tkt` on tkrzw.
fn w3_dir() -> PathBuf {
    repo_root()
        .join("fixtures/w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT)
}

fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("create data directory");
    for entry in std::fs::read_dir(source).expect("read the committed w3 directory") {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &destination.join(entry.file_name()));
        } else {
            std::fs::copy(&path, destination.join(entry.file_name())).expect("copy fixture file");
        }
    }
}

/// A private working directory holding a copy of the committed W3 data.
fn data_dir() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let data = directory.path().join("data");
    copy_tree(&w3_dir(), &data);
    (directory, data)
}

fn run(binary: &str, cwd: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(binary)
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the built utility");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(stdin)
        .expect("write stdin");
    child.wait_with_output().expect("wait for the utility")
}

fn ngseg(cwd: &Path, args: &[&str], stdin: &[u8]) -> Output {
    run(env!("CARGO_BIN_EXE_ngseg"), cwd, args, stdin)
}

// Build only the expected text for the host CRT's stream mode.
fn expected_text(text: &[u8]) -> Vec<u8> {
    let mut expected = Vec::with_capacity(text.len());
    for &byte in text {
        if cfg!(windows) && byte == b'\n' {
            expected.push(b'\r');
        }
        expected.push(byte);
    }
    expected
}

#[test]
fn a_missing_input_reports_the_open_and_exits_einval() {
    let (directory, data) = data_dir();
    let output = ngseg(&data, &["absent.txt"], b"");
    // `open file failed: No such file or directory`
    assert_eq!(output.status.code(), Some(tool::EINVAL), "EINVAL");
    // The input is opened before `table.conf` (`ngseg.cpp:136-151`), so the
    // working directory needs no data file for this path.
    let absent = directory.path().join("empty");
    std::fs::create_dir(&absent).expect("empty directory");
    let output = ngseg(&absent, &["absent.txt"], b"");
    assert_eq!(output.status.code(), Some(tool::EINVAL), "EINVAL");
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("open file failed:"));
}
#[test]
fn the_output_file_receives_the_stream() {
    let (_directory, data) = data_dir();
    let output = ngseg(&data, &["-o", "seg.out"], b"\n");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"");
    assert_eq!(output.stderr, b"");
    assert_eq!(
        std::fs::read(data.join("seg.out")).expect("output file"),
        expected_text(b"0 \n0 \n")
    );
}

#[cfg(unix)]
mod common;

#[cfg(unix)]
mod fixtures;

// Unix only: the frozen pin runs are Linux and macOS observations (signal statuses, Unix paths and line endings).
#[cfg(unix)]
mod frozen {
    use crate::common::expected::*;

    const EMPTY_OUT_TXT_FILES: &[(&str, File)] = &[("out.txt", EMPTY_FILE)];
    const NON_UCS4_CHARACTERS: &[u8] = b"non-ucs4 characters encountered:\xff.\n";
    const NULL_TOKEN_LINE: &[u8] = b"0 \n";
    const OPEN_FILE_FAILED_ENOENT: &[u8] = b"open file failed: No such file or directory\n";
    const TOO_MANY_ARGUMENTS: &[u8] = b"too many arguments.\n";
    const TWO_NULL_TOKEN_LINES: &[u8] = b"0 \n0 \n";
    const ZHONG_THEN_TWO_NULL_TOKEN_LINES: &[u8] = "16786821 中\n0 \n0 \n".as_bytes();

    mod mergeseq {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::{
            EMPTY_OUT_TXT_FILES, NULL_TOKEN_LINE, OPEN_FILE_FAILED_ENOENT, TOO_MANY_ARGUMENTS,
        };
        use crate::fixtures::w3;

        #[rstest]
        // mergeseq-bogus-option: utils/segment/mergeseq.cpp at 074a2219: 210-215.
        #[case::mergeseq_bogus_option(
            TempDir::new().unwrap(),
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
        // mergeseq-empty-input: utils/segment/mergeseq.cpp at 074a2219: 273-276 (the synthetic
        // "0 " line).
        #[case::mergeseq_empty_input(
            w3(),
            &[],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(NULL_TOKEN_LINE),
                stderr: b"",
                files: &[],
            },
        )]
        // mergeseq-empty-line: utils/segment/mergeseq.cpp at 074a2219: 264-266 (empty lines
        // skipped) and 273-276.
        #[case::mergeseq_empty_line(
            w3(),
            &[],
            b"\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(NULL_TOKEN_LINE),
                stderr: b"",
                files: &[],
            },
        )]
        // mergeseq-help: utils/segment/mergeseq.cpp at 074a2219: 207-215 (GLib --help).
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::mergeseq_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption("Usage:\n  mergeseq [OPTION?] - merge word sequence\n\nHelp Options:\n  -h, --help                    Show help options\n\nApplication Options:\n  -o, --outputfile=filename     output\n\n"),
                stderr: b"",
                files: &[],
            },
        )]
        // mergeseq-invalid-utf8-null-line: utils/segment/mergeseq.cpp at 074a2219: 159-172 (a
        // null-token line is echoed).
        #[case::mergeseq_invalid_utf8_null_line(
            w3(),
            &[],
            b"0 \xff\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b"0 \xff\n0 \n"),
                stderr: b"",
                files: &[],
            },
        )]
        // mergeseq-mini-fixture: utils/segment/mergeseq.cpp at 074a2219: 200-281;
        // fixtures/w9/mergeseq-input.txt.
        #[case::mergeseq_mini_fixture(
            w3(),
            &["seq.txt"],
            b"",
            &[("seq.txt", include_bytes!("../../../fixtures/w9/mergeseq-input.txt").as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact("16817937 中国\n0 \n16802309 你好\n0 \n0 \n".as_bytes()),
                stderr: b"",
                files: &[],
            },
        )]
        // mergeseq-missing-input: utils/segment/mergeseq.cpp at 074a2219: 230-237.
        #[case::mergeseq_missing_input(
            w3(),
            &["absent.txt"],
            b"",
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(b""),
                stderr: OPEN_FILE_FAILED_ENOENT,
                files: &[],
            },
        )]
        // mergeseq-missing-table-conf: utils/segment/mergeseq.cpp at 074a2219: 239-245.
        #[case::mergeseq_missing_table_conf(
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
        // mergeseq-too-many-arguments: utils/segment/mergeseq.cpp at 074a2219: 217-228 (output
        // opened first).
        #[case::mergeseq_too_many_arguments(
            w3(),
            &["-o", "out.txt", "first.txt", "second.txt"],
            b"",
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(b""),
                stderr: TOO_MANY_ARGUMENTS,
                files: EMPTY_OUT_TXT_FILES,
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
            let mut output = cargo_bin_cmd!("mergeseq")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run mergeseq");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod ngseg {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::{
            EMPTY_OUT_TXT_FILES, NON_UCS4_CHARACTERS, NULL_TOKEN_LINE, OPEN_FILE_FAILED_ENOENT,
            TOO_MANY_ARGUMENTS, TWO_NULL_TOKEN_LINES, ZHONG_THEN_TWO_NULL_TOKEN_LINES,
        };
        use crate::fixtures::w3;

        #[rstest]
        // ngseg-bogus-option: utils/segment/ngseg.cpp at 074a2219: 116-121.
        #[case::ngseg_bogus_option(
            TempDir::new().unwrap(),
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
        // ngseg-empty-input: utils/segment/ngseg.cpp at 074a2219: 256-260.
        #[case::ngseg_empty_input(
            w3(),
            &[],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(NULL_TOKEN_LINE),
                stderr: b"",
                files: &[],
            },
        )]
        // ngseg-empty-line: utils/segment/ngseg.cpp at 074a2219: 199-204.
        #[case::ngseg_empty_line(
            w3(),
            &[],
            b"\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(TWO_NULL_TOKEN_LINES),
                stderr: b"",
                files: &[],
            },
        )]
        // ngseg-extra-enter: utils/segment/ngseg.cpp at 074a2219: 248-251.
        #[case::ngseg_extra_enter(
            w3(),
            &["--generate-extra-enter"],
            b"\xe4\xb8\xad\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(ZHONG_THEN_TWO_NULL_TOKEN_LINES),
                stderr: b"",
                files: &[],
            },
        )]
        // ngseg-family-e-truncated-first-chunk: utils/segment/ngseg.cpp:163;
        // utils/utils_helper.h:73-94; src/include/memory_chunk.h:420-463 at 074a2219.
        // mergeseq and spseg load the chunks through the same segment::data::load_or_exit;
        // their missing-table-conf cases cover the call.
        #[case::ngseg_family_e_truncated_first_chunk(
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
        // ngseg-help: utils/segment/ngseg.cpp at 074a2219: 113-121 (GLib --help).
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::ngseg_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/ngseg/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // ngseg-invalid-utf8: utils/segment/ngseg.cpp at 074a2219: 189-198.
        #[case::ngseg_invalid_utf8(
            w3(),
            &[],
            b"\xff\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(TWO_NULL_TOKEN_LINES),
                stderr: NON_UCS4_CHARACTERS,
                files: &[],
            },
        )]
        // ngseg-mini-fixture: utils/segment/ngseg.cpp at 074a2219: 106-262;
        // fixtures/w9/segmenter-han.txt.
        // The fixture holds an ASCII run (0 abc) and a one-character phrase (16786821 中).
        #[case::ngseg_mini_fixture(
            w3(),
            &["han.txt"],
            b"",
            &[("han.txt", include_bytes!("../../../fixtures/w9/segmenter-han.txt").as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/ngseg/w3-segments.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // ngseg-missing-input: utils/segment/ngseg.cpp at 074a2219: 136-143.
        #[case::ngseg_missing_input(
            w3(),
            &["absent.txt"],
            b"",
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(b""),
                stderr: OPEN_FILE_FAILED_ENOENT,
                files: &[],
            },
        )]
        // ngseg-missing-table-conf: utils/segment/ngseg.cpp at 074a2219: 145-151.
        #[case::ngseg_missing_table_conf(
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
        // ngseg-too-many-arguments: utils/segment/ngseg.cpp at 074a2219: 123-134 (output
        // opened first).
        #[case::ngseg_too_many_arguments(
            w3(),
            &["-o", "out.txt", "first.txt", "second.txt"],
            b"",
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(b""),
                stderr: TOO_MANY_ARGUMENTS,
                files: EMPTY_OUT_TXT_FILES,
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
            let mut output = cargo_bin_cmd!("ngseg")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run ngseg");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod spseg {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::{
            EMPTY_OUT_TXT_FILES, NON_UCS4_CHARACTERS, NULL_TOKEN_LINE, OPEN_FILE_FAILED_ENOENT,
            TOO_MANY_ARGUMENTS, TWO_NULL_TOKEN_LINES, ZHONG_THEN_TWO_NULL_TOKEN_LINES,
        };
        use crate::fixtures::w3;

        #[rstest]
        // spseg-bogus-option: utils/segment/spseg.cpp at 074a2219: 211-215.
        #[case::spseg_bogus_option(
            TempDir::new().unwrap(),
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
        // spseg-empty-input: utils/segment/spseg.cpp at 074a2219: 339-343.
        #[case::spseg_empty_input(
            w3(),
            &[],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(NULL_TOKEN_LINE),
                stderr: b"",
                files: &[],
            },
        )]
        // spseg-empty-line: utils/segment/spseg.cpp at 074a2219: 281-285.
        #[case::spseg_empty_line(
            w3(),
            &[],
            b"\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(TWO_NULL_TOKEN_LINES),
                stderr: b"",
                files: &[],
            },
        )]
        // spseg-extra-enter: utils/segment/spseg.cpp at 074a2219: 332-334.
        #[case::spseg_extra_enter(
            w3(),
            &["--generate-extra-enter"],
            b"\xe4\xb8\xad\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(ZHONG_THEN_TWO_NULL_TOKEN_LINES),
                stderr: b"",
                files: &[],
            },
        )]
        // spseg-help: utils/segment/spseg.cpp at 074a2219: 208-215 (GLib --help).
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::spseg_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/spseg/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // spseg-invalid-utf8: utils/segment/spseg.cpp at 074a2219: 270-279.
        #[case::spseg_invalid_utf8(
            w3(),
            &[],
            b"\xff\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(TWO_NULL_TOKEN_LINES),
                stderr: NON_UCS4_CHARACTERS,
                files: &[],
            },
        )]
        // spseg-mini-fixture: utils/segment/spseg.cpp at 074a2219: 201-345;
        // fixtures/w9/segmenter-han.txt.
        #[case::spseg_mini_fixture(
            w3(),
            &["han.txt"],
            b"",
            &[("han.txt", include_bytes!("../../../fixtures/w9/segmenter-han.txt").as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/spseg/w3-segments.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // spseg-missing-input: utils/segment/spseg.cpp at 074a2219: 231-238.
        #[case::spseg_missing_input(
            w3(),
            &["absent.txt"],
            b"",
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(b""),
                stderr: OPEN_FILE_FAILED_ENOENT,
                files: &[],
            },
        )]
        // spseg-missing-table-conf: utils/segment/spseg.cpp at 074a2219: 240-246.
        #[case::spseg_missing_table_conf(
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
        // spseg-too-many-arguments: utils/segment/spseg.cpp at 074a2219: 218-229 (output
        // opened first).
        #[case::spseg_too_many_arguments(
            w3(),
            &["-o", "out.txt", "first.txt", "second.txt"],
            b"",
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(b""),
                stderr: TOO_MANY_ARGUMENTS,
                files: EMPTY_OUT_TXT_FILES,
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
            let mut output = cargo_bin_cmd!("spseg")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run spseg");
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
