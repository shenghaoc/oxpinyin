//! Source-derived checks of `k_mixture_model_to_interpolation`; the frozen
//! integration cases hold the pin's results for the rest. Pin citations are
//! in the utility.

use std::io::Write as _;
use std::process::{Command, Output, Stdio};

fn run(stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_k_mixture_model_to_interpolation"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let text = stdin.to_vec();
    let writer = std::thread::spawn(move || input.write_all(&text));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    output
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[cfg(unix)]
fn signal(output: &Output) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt as _;
    output.status.signal()
}

const HEADER: &str = "\\data model \"k mixture model\" count 1 N 1 total_freq 1\n";

/// The merged candidate model of the old KMM differential fixture: one
/// document with five `甲 乙` sentences then `丙 丁`, and one with three.
/// Source-derived from `gen_k_mixture_model.cpp:65-311` and
/// `merge_k_mixture_model.cpp:38-129`; also the `kti-merged` tool case.
#[cfg(unix)]
const MERGED_KMM: &str = "\\data model \"k mixture model\" count 20 N 2 total_freq 20\n\
\\1-gram\n\
\\item 1 <start> count 10 freq 0\n\
\\item 10 甲 count 8 freq 8\n\
\\item 20 乙 count 0 freq 8\n\
\\item 30 丙 count 2 freq 2\n\
\\item 40 丁 count 0 freq 2\n\
\\2-gram\n\
\\item 1 <start> 10 甲 count 8 T 8 N_n_0 2 n_1 0 Mr 5\n\
\\item 1 <start> 30 丙 count 2 T 2 N_n_0 2 n_1 2 Mr 1\n\
\\item 10 甲 20 乙 count 8 T 8 N_n_0 2 n_1 0 Mr 5\n\
\\item 30 丙 40 丁 count 2 T 2 N_n_0 2 n_1 2 Mr 1\n\
\\end\n";

/// The same model after `prune_k_mixture_model -k 3 --CDF 0.5`
/// (`prune_k_mixture_model.cpp:45-186`): the rare `丙` pairs go, their
/// header frequencies are reduced to zero, and the fully-zero rows vanish.
const PRUNED_KMM: &str = "\\data model \"k mixture model\" count 16 N 2 total_freq 16\n\
\\1-gram\n\
\\item 1 <start> count 8 freq 0\n\
\\item 10 甲 count 8 freq 8\n\
\\item 20 乙 count 0 freq 8\n\
\\2-gram\n\
\\item 1 <start> 10 甲 count 8 T 8 N_n_0 2 n_1 0 Mr 5\n\
\\item 10 甲 20 乙 count 8 T 8 N_n_0 2 n_1 0 Mr 5\n\
\\end\n";

#[test]
fn the_pruned_differential_fixture_projects_to_the_golden() {
    // The utility prints through the C runtime's text-mode stdout: CRLF on
    // Windows, LF elsewhere (docs/findings/external-io-boundary.md, "Native
    // text-mode line endings").
    let newline = oxpinyin_input::TEXT_NEWLINE;
    // <start> and zero-freq unigrams drop; the 1-gram count is the KMM
    // freq, the 2-gram count the pair WC. 乙 is W2-only but its header-only
    // row carries freq 8, so it projects.
    let out = run(PRUNED_KMM.as_bytes());
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stderr(&out), "");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "\\data model interpolation\n\\1-gram\n\\item 10 甲 count 8\n\\item 20 乙 count 8\n\
         \\2-gram\n\\item 1 <start> 10 甲 count 8\n\\item 10 甲 20 乙 count 8\n\\end\n"
            .replace('\n', newline)
    );
}

#[test]
fn an_empty_input_or_a_foreign_header_exits_with_enodata() {
    let newline = oxpinyin_input::TEXT_NEWLINE;
    for (input, message) in [
        ("", "empty file input.\n"),
        (
            "\\data model interpolation\n\\end\n",
            "error: k mixture model expected.\n",
        ),
    ] {
        let out = run(input.as_bytes());
        assert_ne!(out.status.code(), Some(0), "{input:?}");
        assert_ne!(out.status.code(), None, "{input:?}");
        assert!(
            stderr(&out).ends_with(&message.replace('\n', newline)),
            "{input:?}: {out:?}"
        );
        assert_eq!(out.stdout, b"", "{input:?}");
    }
}

/// A failed `check_result(taglib_read(...))` aborts, as does a registered
/// line outside its `switch`; neither writes a message (maintainer ruling
/// 2026-10-31 UTC), and the lines `std::getline` flushed before stay on
/// stdout.
#[test]
fn the_pins_assertions_and_abort_abort() {
    for input in [
        format!("{HEADER}\\1-gram\n\\item 10\n"),
        format!("{HEADER}\\1-gram\n\\data model x\n"),
    ] {
        let out = run(input.as_bytes());
        #[cfg(unix)]
        assert_eq!(signal(&out), Some(6), "{out:?}");
        #[cfg(windows)]
        // Rust's Windows abort uses __fastfail, not the Unix SIGABRT.
        assert_eq!(
            out.status.code(),
            Some(i32::from_ne_bytes(0xC000_0409_u32.to_ne_bytes())),
            "{out:?}"
        );
        assert_eq!(stderr(&out), "");
        let expected = "\\data model interpolation\n\\1-gram\n";
        let expected = expected.replace('\n', oxpinyin_input::TEXT_NEWLINE);
        assert_eq!(out.stdout, expected.as_bytes());
    }
}

#[test]
fn undefined_lines_are_refused_normally() {
    let newline = oxpinyin_input::TEXT_NEWLINE;
    for (input, reason, line) in [
        (
            format!("{HEADER}\\1-gram\n  \t\n\\end\n"),
            "line without a token",
            "  \t",
        ),
        (
            format!("{HEADER}\\1-gram\n\\item 10 \"甲\\"),
            "escape at the end of the line",
            "\\item 10 \"甲\\",
        ),
    ] {
        let out = run(input.as_bytes());
        assert_eq!(out.status.code(), Some(1), "{input:?}: {out:?}");
        assert!(
            stderr(&out).ends_with(&format!(
                "k_mixture_model_to_interpolation: {reason}: {line}{newline}"
            )),
            "{input:?}: {out:?}"
        );
    }
    let mut input = format!("{HEADER}\\1-gram\n\\item 10 甲 freq 1 x ").into_bytes();
    input.extend_from_slice(b"\xe4\n");
    let out = run(&input);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(
        out.stderr.ends_with(
            &[
                &b"k_mixture_model_to_interpolation: truncated UTF-8 sequence at the end of the line: \\item 10 \xe7\x94\xb2 freq 1 x \xe4"[..],
                newline.as_bytes()
            ]
            .concat()
        ),
        "{out:?}"
    );
}

#[cfg(unix)]
mod common;

// Unix only: the frozen pin runs are Linux and macOS observations (signal statuses, Unix paths and line endings).
#[cfg(unix)]
mod frozen {
    mod k_mixture_model_to_interpolation {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::TempDir;
        use rstest::rstest;
        use std::time::Duration;

        const INTERPOLATION_HEADER: &[u8] = b"\\data model interpolation\n\\1-gram\n";
        const INTERPOLATION_ITEM_JIA: &[u8] =
            "\\data model interpolation\n\\1-gram\n\\item 10 甲 count 1\n".as_bytes();
        const LINE_WITHOUT_A_TOKEN: &[u8] =
            b"k_mixture_model_to_interpolation: line without a token: \n";

        #[rstest]
        // kti-arguments-ignored: utils/training/k_mixture_model_to_interpolation.cpp at
        // 074a2219: 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        #[case::kti_arguments_ignored(
            &["--help", "-o", "out.txt", "missing.txt"],
            crate::MERGED_KMM.as_bytes(),
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/k_mixture_model_to_interpolation/merged-model.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // kti-bigram-assert: utils/training/k_mixture_model_to_interpolation.cpp at 074a2219:
        // 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; the pin's
        // own result is in fixtures/contract-policy/eio-tool-check-messages.json.
        #[case::kti_bigram_assert(
            &[],
            b"\\data model \"k mixture model\" count 20 N 2 total_freq 20\n\\2-gram\n\\item 1 a 2 b T 1\n",
            Run {
                status: Signal(6),
                stdout: Exact(b"\\data model interpolation\n\\2-gram\n"),
                stderr: b"\n** (process:PID): WARNING **: TIME: missed required tags: count.\n\n",
                files: &[],
            },
        )]
        // kti-blank-line: utils/training/k_mixture_model_to_interpolation.cpp at 074a2219:
        // 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        // Policy-accepted on every cell:
        // docs/findings/eio-kmm-to-interpolation-undefined-lines.md; the pin's own result is
        // in fixtures/contract-policy/eio-kmm-to-interpolation-undefined-lines.json.
        #[case::kti_blank_line(
            &[],
            b"\\data model \"k mixture model\" count 20 N 2 total_freq 20\n\\1-gram\n\\item 10 \xe7\x94\xb2 freq 1\n\n\\end\n",
            Run {
                status: Exit(1),
                stdout: Exact(INTERPOLATION_ITEM_JIA),
                stderr: LINE_WITHOUT_A_TOKEN,
                files: &[],
            },
        )]
        // kti-empty-input: utils/training/k_mixture_model_to_interpolation.cpp at 074a2219:
        // 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        #[case::kti_empty_input(
            &[],
            b"",
            Run {
                status: if cfg!(target_os = "macos") {
                    Exit(96)
                } else {
                    Exit(61)
                },
                stdout: Exact(b""),
                stderr: EMPTY_FILE_INPUT,
                files: &[],
            },
        )]
        // kti-end-missing: utils/training/k_mixture_model_to_interpolation.cpp at 074a2219:
        // 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        // Policy-accepted on every cell:
        // docs/findings/eio-kmm-to-interpolation-undefined-lines.md; the pin's own result is
        // in fixtures/contract-policy/eio-kmm-to-interpolation-undefined-lines.json.
        #[case::kti_end_missing(
            &[],
            b"\\data model \"k mixture model\" count 20 N 2 total_freq 20\n\\1-gram\n\\item 10 \xe7\x94\xb2 freq 1\n",
            Run {
                status: Exit(1),
                stdout: Exact(INTERPOLATION_ITEM_JIA),
                stderr: LINE_WITHOUT_A_TOKEN,
                files: &[],
            },
        )]
        // kti-foreign-header: utils/training/k_mixture_model_to_interpolation.cpp at 074a2219:
        // 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        #[case::kti_foreign_header(
            &[],
            b"\\data model interpolation\n\\end\n",
            Run {
                status: if cfg!(target_os = "macos") {
                    Exit(96)
                } else {
                    Exit(61)
                },
                stdout: Exact(b""),
                stderr: b"error: k mixture model expected.\n",
                files: &[],
            },
        )]
        // kti-header-line-abort: utils/training/k_mixture_model_to_interpolation.cpp at
        // 074a2219: 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        #[case::kti_header_line_abort(
            &[],
            b"\\data model \"k mixture model\" count 20 N 2 total_freq 20\n\\1-gram\n\\data model x\n",
            Run {
                status: Signal(6),
                stdout: Exact(INTERPOLATION_HEADER),
                stderr: b"",
                files: &[],
            },
        )]
        // kti-header-only: utils/training/k_mixture_model_to_interpolation.cpp at 074a2219:
        // 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        #[case::kti_header_only(
            &[],
            b"\\data model \"k mixture model\" count 20 N 2 total_freq 20",
            Run {
                status: Exit(0),
                stdout: Exact(b"\\data model interpolation\n"),
                stderr: b"",
                files: &[],
            },
        )]
        // kti-last-line-reread-assert: utils/training/k_mixture_model_to_interpolation.cpp at
        // 074a2219: 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; the pin's
        // own result is in fixtures/contract-policy/eio-tool-check-messages.json.
        // Platform exception, macos: abort() flushes stdio on macOS, so buffered stdout
        // survives; glibc's discards it.
        #[case::kti_last_line_reread_assert(
            &[],
            b"\\data model \"k mixture model\" count 20 N 2 total_freq 20\n\\1-gram\n\\item 10 \xe7\x94\xb2 freq 1",
            Run {
                status: Signal(6),
                stdout: if cfg!(target_os = "macos") {
                    Exact(INTERPOLATION_ITEM_JIA)
                } else {
                    Exact(INTERPOLATION_HEADER)
                },
                stderr: b"",
                files: &[],
            },
        )]
        // kti-merged: utils/training/k_mixture_model_to_interpolation.cpp at 074a2219:
        // 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        #[case::kti_merged(
            &[],
            crate::MERGED_KMM.as_bytes(),
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/k_mixture_model_to_interpolation/merged-model.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // kti-missing-model: utils/training/k_mixture_model_to_interpolation.cpp at 074a2219:
        // 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        #[case::kti_missing_model(
            &[],
            b"\\data count 1\n\\end\n",
            Run {
                status: if cfg!(target_os = "macos") {
                    Exit(96)
                } else {
                    Exit(61)
                },
                stdout: Exact(b""),
                stderr: b"\n** (process:PID): WARNING **: TIME: missed required tags: model.\n\nerror: k mixture model expected.\n",
                files: &[],
            },
        )]
        // kti-missing-value-assert: utils/training/k_mixture_model_to_interpolation.cpp at
        // 074a2219: 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; the pin's
        // own result is in fixtures/contract-policy/eio-tool-check-messages.json.
        #[case::kti_missing_value_assert(
            &[],
            b"\\data model \"k mixture model\" count 20 N 2 total_freq 20\n\\1-gram\n\\item 10\n",
            Run {
                status: Signal(6),
                stdout: Exact(INTERPOLATION_HEADER),
                stderr: b"",
                files: &[],
            },
        )]
        // kti-numeric-fields: utils/training/k_mixture_model_to_interpolation.cpp at 074a2219:
        // 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        #[case::kti_numeric_fields(
            &[],
            b"\\data model \"k mixture model\" count 20 N 2 total_freq 20\n\\1-gram\n\\item 16817937 \xe4\xb8\xad\xe5\x9b\xbd count 1 freq +5\n\\item 16802309 \xe4\xbd\xa0\xe5\xa5\xbd count 1 freq 4294967301\n\\item 16808451 \xe4\xb8\x96\xe7\x95\x8c count 1 freq -7\n\\item 16782711 \xe4\xba\xba count 1 freq abc\n\\item 2147483649 \xe7\x94\xb2 count 1 freq 00012x\n\\item 1 <start> count 1 freq 9\n\\2-gram\n\\item +16817937 \xe4\xb8\xad\xe5\x9b\xbd -4278149359 \xe4\xb8\xad\xe5\x9b\xbd count +3 T 3 N_n_0 1 n_1 1 Mr 3\n\\item 4311785233 \xe4\xb8\xad\xe5\x9b\xbd 16802309 \xe4\xbd\xa0\xe5\xa5\xbd count 4294967299 T 3 N_n_0 1 n_1 1 Mr 3\n\\item 16802309 \xe4\xbd\xa0\xe5\xa5\xbd 16808451 \xe4\xb8\x96\xe7\x95\x8c count 99999999999999999999 T 3 N_n_0 1 n_1 1 Mr 3\n\\item 16808451 \xe4\xb8\x96\xe7\x95\x8c 16817937 \xe4\xb8\xad\xe5\x9b\xbd count - T 3 N_n_0 1 n_1 1 Mr 3\n\\end\n",
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/k_mixture_model_to_interpolation/numeric-fields-model.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // kti-tag-pairs: utils/training/k_mixture_model_to_interpolation.cpp at 074a2219:
        // 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        #[case::kti_tag_pairs(
            &[],
            b"\\data model \"k mixture model\" count 20 N 2 total_freq 20\n\\1-gram\n\\item 10 \xe7\x94\xb2 freq 2 junk 9 freq 3 count 1\n\\item 20 \"\xe4\xb9\x99 \xe4\xb8\x99\" freq 4\n\\2-gram\n\\item 10 \xe7\x94\xb2 20 \xe4\xb9\x99 count 1 extra\n\\1-gram\n\\item 30 \xe4\xb8\x81 freq 5\n\\end trailing\nnot read\n",
            Run {
                status: Exit(0),
                stdout: Exact("\\data model interpolation\n\\1-gram\n\\item 10 甲 count 3\n\\item 20 乙 丙 count 4\n\\2-gram\n\\item 10 甲 20 乙 count 1\n\\1-gram\n\\item 30 丁 count 5\n\\end\n".as_bytes()),
                stderr: include_bytes!("fixtures/k_mixture_model_to_interpolation/unexpected-tag-warnings.txt"),
                files: &[],
            },
        )]
        // kti-trailing-escape: utils/training/k_mixture_model_to_interpolation.cpp at
        // 074a2219: 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        // Policy-accepted on every cell:
        // docs/findings/eio-kmm-to-interpolation-undefined-lines.md; the pin's own result is
        // in fixtures/contract-policy/eio-kmm-to-interpolation-undefined-lines.json.
        #[case::kti_trailing_escape(
            &[],
            b"\\data model \"k mixture model\" count 20 N 2 total_freq 20\n\\1-gram\n\\item 10 \"\xe7\x94\xb2\\",
            Run {
                status: Exit(1),
                stdout: Exact(INTERPOLATION_HEADER),
                stderr: "\n(process:PID): GLib-CRITICAL **: TIME: g_strv_length: assertion 'str_array != NULL' failed\nk_mixture_model_to_interpolation: escape at the end of the line: \\item 10 \"甲\\\n".as_bytes(),
                files: &[],
            },
        )]
        // kti-unknown-line-assert: utils/training/k_mixture_model_to_interpolation.cpp at
        // 074a2219: 188-213, 55-186; src/storage/tag_utility.cpp: 118-264.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; the pin's
        // own result is in fixtures/contract-policy/eio-tool-check-messages.json.
        #[case::kti_unknown_line_assert(
            &[],
            b"\\data model \"k mixture model\" count 20 N 2 total_freq 20\n\\1-gram\n\\item 10 \xe7\x94\xb2 freq 1\nitem 20 \xe4\xb9\x99 freq 1\n",
            Run {
                status: Signal(6),
                stdout: Exact(INTERPOLATION_ITEM_JIA),
                stderr: b"",
                files: &[],
            },
        )]
        fn captured_cases(#[case] args: &[&str], #[case] stdin: &[u8], #[case] expected: Run) {
            let directory = TempDir::new().expect("scratch directory");
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("k_mixture_model_to_interpolation")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run k_mixture_model_to_interpolation");
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
