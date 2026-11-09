//! The pin's training utilities on the committed `fixtures/w3` data set
//! (`utils/training/*.cpp` at libpinyin 074a2219).
//!
//! Every test copies the compiled backend's directory
//! (`fixtures/w3/<DEFAULT_STORE_EXT>`) to a private temporary directory and
//! runs the binary in it, so the cwd-relative names are the pin's. The
//! expectations are derived from the pin's source and the frozen fixture
//! values.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use oxpinyin_data::row_format::encode_token_key;
use oxpinyin_data::single_gram::decode_single_gram;
use oxpinyin_store::{DefaultStore, RawReadStore};

/// The repository root.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The compiled backend's fixture directory.
fn fixture() -> PathBuf {
    root()
        .join("fixtures/w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT)
}

/// A private copy of [`fixture`].
fn fixture_copy() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temp dir");
    for entry in std::fs::read_dir(fixture()).expect("fixture dir") {
        let entry = entry.expect("fixture entry");
        if entry.file_type().expect("fixture file type").is_file() {
            std::fs::copy(entry.path(), temp.path().join(entry.file_name())).expect("copy fixture");
        }
    }
    temp
}

/// The binary named `name`, built for this feature set.
fn binary(name: &str) -> PathBuf {
    match name {
        "gen_unigram" => env!("CARGO_BIN_EXE_gen_unigram").into(),
        "gen_ngram" => env!("CARGO_BIN_EXE_gen_ngram").into(),
        "gen_deleted_ngram" => env!("CARGO_BIN_EXE_gen_deleted_ngram").into(),
        "estimate_interpolation" => env!("CARGO_BIN_EXE_estimate_interpolation").into(),
        "eval_correction_rate" => env!("CARGO_BIN_EXE_eval_correction_rate").into(),
        other => panic!("no such utility: {other}"),
    }
}

/// Runs `binary` in `dir` with `args` and `stdin`.
fn run(name: &str, dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(binary(name))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn utility");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin)
        .expect("write stdin");
    child.wait_with_output().expect("utility output")
}

/// `outcome.exit` as a status word: the exit code, or `-signal` for a
/// signal (the pin's shell status).
fn exit_word(outcome: &Output) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        outcome
            .status
            .code()
            .unwrap_or_else(|| -outcome.status.signal().unwrap_or(0))
    }
    #[cfg(windows)]
    {
        outcome.status.code().expect("Windows process exit code")
    }
}

fn expected_text(text: &str) -> String {
    text.replace('\n', oxpinyin_input::TEXT_NEWLINE)
}
/// One `SingleGram` row as `(total, records)`; the row must exist.
fn bigram_row(path: &Path, token: u32) -> (u32, Vec<(u32, u32)>) {
    let store = DefaultStore::open_hash_read_only(path).expect("bigram opens");
    let mut value = Vec::new();
    assert!(
        store
            .get_bigram_raw(&encode_token_key(token), &mut value)
            .expect("bigram read"),
        "bigram row {token} is present"
    );
    decode_single_gram(&value).expect("single gram")
}

/// Whether the row exists at all.
fn has_bigram_row(path: &Path, token: u32) -> bool {
    let store = DefaultStore::open_hash_read_only(path).expect("bigram opens");
    let mut value = Vec::new();
    store
        .get_bigram_raw(&encode_token_key(token), &mut value)
        .expect("bigram read")
}

const A: u32 = 0x0100_0003; // 阿
const AH: u32 = 0x0100_0004; // 啊
const START: u32 = 1;
// The pin's rows for this input (utility/<cell>/gen-deleted-ngram, identical on every backend):
// a blank line restarts at sentence_start, and 啊 → 阿 crosses it, so 啊 gets no row.

#[test]
fn gen_deleted_ngram_restarts_after_a_blank_line_as_the_pin_does() {
    let dir = fixture_copy();
    let outcome = run(
        "gen_deleted_ngram",
        dir.path(),
        &["--deleted-bigram-file", "deleted.db"],
        "16777219 阿\n16777220 啊\n\n16777219 阿\n".as_bytes(),
    );
    assert_eq!(exit_word(&outcome), 0, "stderr: {:?}", outcome.stderr);
    assert!(outcome.stdout.is_empty() && outcome.stderr.is_empty());
    let deleted = dir.path().join("deleted.db");
    assert_eq!(bigram_row(&deleted, START), (2, vec![(A, 2)]));
    assert_eq!(bigram_row(&deleted, A), (1, vec![(AH, 1)]));
    assert!(!has_bigram_row(&deleted, AH));
}
#[test]
fn eval_correction_rate_refuses_an_unknown_token() {
    let dir = fixture_copy();
    std::fs::write(dir.path().join("evals2.text"), "999999 阿\n").expect("corpus");
    let outcome = run("eval_correction_rate", dir.path(), &[], b"");
    // The pin's `strcmp(NULL, phrase)` reads the NULL (class (b)); oxpinyin
    // writes the helper's own line and exits 1.
    assert_eq!(exit_word(&outcome), 1);
    assert_eq!(
        outcome.stderr,
        expected_text("error: unknown token:999999.\n").as_bytes()
    );
}
#[cfg(unix)]
mod common;
#[cfg(unix)]
mod fixtures;

// Unix only: the frozen pin runs are Linux and macOS observations (signal statuses, Unix paths and line endings).
#[cfg(unix)]
mod frozen {
    use crate::common::CELL;

    fn bigram_db_not_berkeley_db() -> &'static [u8] {
        match CELL {
            "bdb" => b"BDB0004 fop_read_meta: bigram.db: unexpected file type or format\n",
            "kc" | "tkrzw" => b"",
            _ => unreachable!("unknown cell {CELL}"),
        }
    }

    mod estimate_interpolation {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::bigram_db_not_berkeley_db;
        use crate::common::CELL;
        use crate::fixtures::w3;

        fn w3_lambdas() -> Stdout {
            match CELL {
                "bdb" => Exact(b"token:16777219 lambda:0.999892\ntoken:1 lambda:0.000000\naverage lambda:0.499946\n"),
                "kc" | "tkrzw" => Exact(b"token:1 lambda:0.000000\ntoken:16777219 lambda:0.999892\naverage lambda:0.499946\n"),
                _ => unreachable!("unknown cell {CELL}"),
            }
        }

        // Pin source citations and platform notes remain beside each observation.
        // Only these case arguments choose setup; the ID selects expectations only.
        #[rstest]
        // estimate_interpolation-family-e-truncated-first-chunk:
        // utils/training/estimate_interpolation.cpp:104; utils/utils_helper.h:73-94;
        // src/include/memory_chunk.h:420-463 at 074a2219.
        #[case::estimate_interpolation_family_e_truncated_first_chunk(
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
        // estimate_interpolation-help: utils/training/estimate_interpolation.cpp at 074a2219:
        // 90-142 (no g_option_context: an option is an ignored argument).
        // The program reads no argument: --help is one witness for every argument list.
        #[case::estimate_interpolation_help(
            w3(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: w3_lambdas(),
                stderr: b"",
                files: &[],
            },
        )]
        // estimate_interpolation-layout-refusal-total:
        // src/storage/phrase_index.cpp:237-248,335-356;
        // utils/training/estimate_interpolation.cpp:65-71,104 at 074a2219.
        // Policy-accepted on bdb, kc, tkrzw: docs/findings/1297-layout-criticals.md; the pin's
        // own result is in fixtures/contract-policy/1297-layout-criticals.json.
        #[case::estimate_interpolation_layout_refusal_total(
            w3(),
            &[],
            b"",
            &[("gbk_char.bin", b"\x11\0\0\0H\xca\x9a;\0\xca\x9a;\x11\0\0\0\x16\0\0\0\x17\0\0\0X".as_slice())],
            Run {
                status: Exit(0),
                stdout: w3_lambdas(),
                stderr: b"",
                files: &[],
            },
        )]
        // estimate_interpolation-missing-bigram-file:
        // utils/training/estimate_interpolation.cpp at 074a2219: 107-111 (both attaches are
        // ignored) and 56-63 (a missing system row leaves the numerator zero).
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::estimate_interpolation_missing_bigram_file(
            w3(),
            &[],
            b"",
            &[("bigram.db", b"".as_slice())],
            Run {
                status: Exit(0),
                stdout: match CELL {
                    "bdb" => Exact(b"token:16777219 lambda:0.000000\ntoken:1 lambda:0.000000\naverage lambda:0.000000\n"),
                    "kc" | "tkrzw" => Exact(b"token:1 lambda:0.000000\ntoken:16777219 lambda:0.000000\naverage lambda:0.000000\n"),
                    _ => unreachable!("unknown cell {CELL}"),
                },
                stderr: bigram_db_not_berkeley_db(),
                files: &[],
            },
        )]
        // estimate_interpolation-missing-deleted-bigram:
        // utils/training/estimate_interpolation.cpp at 074a2219: 113-114 (get_all_items on a
        // failed attach answers empty) and 139 (0.0/0 prints the host NaN).
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        // Platform exception, aarch64, x86_64: a default NaN prints nan on aarch64 and -nan on
        // x86_64.
        #[case::estimate_interpolation_missing_deleted_bigram(
            w3(),
            &[],
            b"",
            &[("deleted_bigram.db", b"".as_slice())],
            Run {
                status: Exit(0),
                stdout: AVERAGE_LAMBDA_NAN,
                stderr: match CELL {
                    "bdb" => b"BDB0004 fop_read_meta: deleted_bigram.db: unexpected file type or format\n",
                    "kc" | "tkrzw" => b"",
                    _ => unreachable!("unknown cell {CELL}"),
                },
                files: &[],
            },
        )]
        // estimate_interpolation-missing-table-conf: utils/training/estimate_interpolation.cpp
        // at 074a2219: 93-97.
        #[case::estimate_interpolation_missing_table_conf(
            TempDir::new().expect("empty directory"),
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
        // estimate_interpolation-ordinary: utils/training/estimate_interpolation.cpp at
        // 074a2219: 30-88 and 119-139 over the committed deleted_bigram.db rows.
        #[case::estimate_interpolation_ordinary(
            w3(),
            &[],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: w3_lambdas(),
                stderr: b"",
                files: &[],
            },
        )]
        fn captured_cases(
            #[case] directory: TempDir,
            #[case] args: &[&str],
            #[case] stdin: &[u8],
            #[case] files: &[(&str, &'static [u8])],
            #[case] expected: Run,
        ) {
            for (name, content) in files {
                directory
                    .child(name)
                    .write_binary(content)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("estimate_interpolation")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run estimate_interpolation");
            output.stderr = mask_glib_prefix(&output.stderr);
            for (name, content) in files {
                directory.child(name).assert(*content);
            }
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod eval_correction_rate {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::bigram_db_not_berkeley_db;
        use crate::fixtures::w3;

        const CANT_OPEN_EVALS2_TEXT: &[u8] = b"Can't open file:evals2.text\n";
        const CORRECTION_RATE_NAN: Stdout = if cfg!(target_arch = "aarch64") {
            Exact(b"correction rate:nan\n")
        } else {
            Exact(b"correction rate:-nan\n")
        };
        const CORRECTION_RATE_ONE: &[u8] = b"correction rate:1.000000\n";

        #[rstest]
        // eval_correction_rate-bad-segmented-line: utils/training/eval_correction_rate.cpp at
        // 074a2219: 178 and utils/utils_helper.h:57-59 (g_strv_length != 2 aborts).
        #[case::eval_correction_rate_bad_segmented_line(
            w3(),
            &[],
            b"",
            &[("evals2.text", b"abc\n".as_slice())],
            Run {
                status: Signal(6),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // eval_correction_rate-empty-corpus: utils/training/eval_correction_rate.cpp at
        // 074a2219: 202-203 (0/0 prints the host NaN).
        // Platform exception, aarch64, x86_64: a default NaN prints nan on aarch64 and -nan on
        // x86_64.
        #[case::eval_correction_rate_empty_corpus(
            w3(),
            &[],
            b"",
            &[("evals2.text", b"".as_slice())],
            Run {
                status: Exit(0),
                stdout: CORRECTION_RATE_NAN,
                stderr: b"",
                files: &[],
            },
        )]
        // eval_correction_rate-family-e-truncated-first-chunk:
        // utils/training/eval_correction_rate.cpp:151; utils/utils_helper.h:73-94;
        // src/include/memory_chunk.h:420-463 at 074a2219.
        #[case::eval_correction_rate_family_e_truncated_first_chunk(
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
        // eval_correction_rate-help: utils/training/eval_correction_rate.cpp at 074a2219:
        // 132-170 (no g_option_context; print_help is dead and an option is an ignored
        // argument, then the missing evals2.text ends the run).
        // The program reads no argument: --help is one witness for every argument list.
        #[case::eval_correction_rate_help(
            w3(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: CANT_OPEN_EVALS2_TEXT,
                files: &[],
            },
        )]
        // eval_correction_rate-mismatch: utils/training/eval_correction_rate.cpp at 074a2219:
        // 112-125 (two tokens; the decode answers 阿阿).
        #[case::eval_correction_rate_mismatch(
            w3(),
            &[],
            b"",
            &[("evals2.text", b"16777219 \xe9\x98\xbf\n16777220 \xe5\x95\x8a\n\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(b"correction rate:0.000000\n"),
                stderr: "test sentence:阿啊\nguessed sentence:阿阿\nthe result mis-matches.\n".as_bytes(),
                files: &[],
            },
        )]
        // eval_correction_rate-missing-bigram-file: utils/training/eval_correction_rate.cpp at
        // 074a2219: 154-155 (attach's answer is ignored; the decode falls back to the unigram
        // term).
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::eval_correction_rate_missing_bigram_file(
            w3(),
            &[],
            b"",
            &[("bigram.db", b"".as_slice()), ("evals2.text", b"16777219 \xe9\x98\xbf\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(CORRECTION_RATE_ONE),
                stderr: bigram_db_not_berkeley_db(),
                files: &[],
            },
        )]
        // eval_correction_rate-missing-corpus: utils/training/eval_correction_rate.cpp at
        // 074a2219: 166-170.
        #[case::eval_correction_rate_missing_corpus(
            w3(),
            &[],
            b"",
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: CANT_OPEN_EVALS2_TEXT,
                files: &[],
            },
        )]
        // eval_correction_rate-missing-table-conf: utils/training/eval_correction_rate.cpp at
        // 074a2219: 135-141.
        #[case::eval_correction_rate_missing_table_conf(
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
        // eval_correction_rate-pass: utils/training/eval_correction_rate.cpp at 074a2219:
        // 36-130 and 177-203 (one token decodes back to itself).
        #[case::eval_correction_rate_pass(
            w3(),
            &[],
            b"",
            &[("evals2.text", b"16777219 \xe9\x98\xbf\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(CORRECTION_RATE_ONE),
                stderr: b"",
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
            let mut output = cargo_bin_cmd!("eval_correction_rate")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run eval_correction_rate");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod gen_deleted_ngram {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use crate::fixtures::w3;

        #[rstest]
        // gen_deleted_ngram-bad-segmented-line: utils/training/gen_deleted_ngram.cpp at
        // 074a2219: 81 and utils/utils_helper.h:57-59 (g_strv_length != 2 aborts).
        #[case::gen_deleted_ngram_bad_segmented_line(
            w3(),
            &["--deleted-bigram-file", "absent/deleted.db"],
            b"abc\n",
            &[],
            Run {
                status: Signal(6),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // gen_deleted_ngram-empty-stdin: utils/training/gen_deleted_ngram.cpp at 074a2219:
        // 80-118 (no line, then return 0).
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::gen_deleted_ngram_empty_stdin(
            w3(),
            &["--deleted-bigram-file", "absent/deleted.db"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // gen_deleted_ngram-family-e-truncated-first-chunk:
        // utils/training/gen_deleted_ngram.cpp:72-73; utils/utils_helper.h:73-94;
        // src/include/memory_chunk.h:420-463 at 074a2219.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        // An absent chunk takes the same load failure and the same ENODATA exit.
        #[case::gen_deleted_ngram_family_e_truncated_first_chunk(
            w3(),
            &[],
            b"",
            &[("gb_char.bin", b"\x04\0\0\0\0\0\0\0\x01".as_slice()), ("gbk_char.bin", b"\0".as_slice())],
            Run {
                status: if cfg!(target_os = "macos") {
                    Exit(96)
                } else {
                    Exit(61)
                },
                stdout: Exact(b""),
                stderr: LOAD_GB_CHAR_BIN_FAILED,
                files: &[],
            },
        )]
        // gen_deleted_ngram-help: utils/training/gen_deleted_ngram.cpp at 074a2219: 46-57
        // (GLib --help).
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::gen_deleted_ngram_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/gen_deleted_ngram/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // gen_deleted_ngram-missing-bigram-file: utils/training/gen_deleted_ngram.cpp at
        // 074a2219: 75-76 (attach's answer is ignored) and 118 (no chunk save).
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::gen_deleted_ngram_missing_bigram_file(
            w3(),
            &["--deleted-bigram-file", "absent/deleted.db"],
            b"16777219 \xe9\x98\xbf\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // gen_deleted_ngram-ordinary: utils/training/gen_deleted_ngram.cpp at 074a2219: 80-116
        // (sentence_start after a null token, deleted bi-gram rows; no chunk save).
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::gen_deleted_ngram_ordinary(
            w3(),
            &["--deleted-bigram-file", "absent/deleted.db"],
            b"16777219 \xe9\x98\xbf\n16777220 \xe5\x95\x8a\n\n16777219 \xe9\x98\xbf\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // gen_deleted_ngram-too-many-arguments: utils/training/gen_deleted_ngram.cpp at
        // 074a2219: 54-118 (g_option_context_parse leaves extra arguments; main never reads
        // argc).
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::gen_deleted_ngram_too_many_arguments(
            w3(),
            &["--deleted-bigram-file", "absent/deleted.db", "first", "second"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
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
            let mut output = cargo_bin_cmd!("gen_deleted_ngram")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run gen_deleted_ngram");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod gen_ngram {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use crate::fixtures::w3;

        #[rstest]
        // gen_ngram-bad-segmented-line: utils/training/gen_ngram.cpp at 074a2219: 80 and
        // utils/utils_helper.h:57-59 (g_strv_length != 2 aborts).
        #[case::gen_ngram_bad_segmented_line(
            w3(),
            &["--bigram-file", "absent/bigram.db"],
            b"abc\n",
            &[],
            Run {
                status: Signal(6),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // gen_ngram-bogus-option: utils/training/gen_ngram.cpp at 074a2219: 53-56.
        // gen_deleted_ngram runs the same gen_ngram::run, so this case covers both programs.
        #[case::gen_ngram_bogus_option(
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
        // gen_ngram-empty-stdin: utils/training/gen_ngram.cpp at 074a2219: 79-121 (no line,
        // then save_phrase_index).
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::gen_ngram_empty_stdin(
            w3(),
            &["--bigram-file", "absent/bigram.db"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // gen_ngram-family-e-truncated-first-chunk: utils/training/gen_ngram.cpp:71;
        // utils/utils_helper.h:73-94; src/include/memory_chunk.h:420-463 at 074a2219.
        #[case::gen_ngram_family_e_truncated_first_chunk(
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
        // gen_ngram-help: utils/training/gen_ngram.cpp at 074a2219: 45-56 (GLib --help).
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::gen_ngram_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/gen_ngram/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // gen_ngram-missing-bigram-file: utils/training/gen_ngram.cpp at 074a2219: 74-75
        // (attach's answer is ignored; a failed attach leaves every load and store a no-op)
        // and 120-121 (the chunks are still saved).
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::gen_ngram_missing_bigram_file(
            w3(),
            &["--bigram-file", "absent/bigram.db"],
            b"16777219 \xe9\x98\xbf\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[
                    ("gb_char.bin", Sha256("3082af7a8ab157fb989678295f5986e365b5b52d183955d567aafd704171a80d", 167681)),
                ],
            },
        )]
        // gen_ngram-missing-table-conf: utils/training/gen_ngram.cpp at 074a2219: 58-64.
        // gen_deleted_ngram runs the same gen_ngram::run, so this case covers both programs.
        #[case::gen_ngram_missing_table_conf(
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
        // gen_ngram-ordinary: utils/training/gen_ngram.cpp at 074a2219: 79-121 (uni-gram +1,
        // sentence_start after a null token, bi-gram rows, chunk save).
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::gen_ngram_ordinary(
            w3(),
            &["--bigram-file", "absent/bigram.db"],
            b"16777219 \xe9\x98\xbf\n16777220 \xe5\x95\x8a\n\n16777219 \xe9\x98\xbf\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[
                    ("gb_char.bin", Sha256("ca909032383990e6a99b8866a3855700c7b731c7a2b7fc1ca6ff658b7a5053ab", 167681)),
                ],
            },
        )]
        // gen_ngram-too-many-arguments: utils/training/gen_ngram.cpp at 074a2219: 53-123
        // (g_option_context_parse leaves extra arguments; main never reads argc).
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::gen_ngram_too_many_arguments(
            w3(),
            &["--bigram-file", "absent/bigram.db", "first", "second"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // gen_ngram-skip-pi-gram: utils/training/gen_ngram.cpp at 074a2219: 40
        // (G_OPTION_FLAG_REVERSE) and 93-98.
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::gen_ngram_skip_pi_gram(
            w3(),
            &["--skip-pi-gram-training", "--bigram-file", "absent/bigram.db"],
            b"16777219 \xe9\x98\xbf\n16777220 \xe5\x95\x8a\n",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[
                    ("gb_char.bin", Sha256("3eade373d5ddf341c36adc53bba914c606176811d5b2bfd5e5380738281aae48", 167681)),
                ],
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
            let mut output = cargo_bin_cmd!("gen_ngram")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run gen_ngram");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod gen_unigram {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::{fixture, rstest};
        use std::path::Path;
        use std::time::Duration;

        use crate::fixtures::w3;

        const UNIGRAM_W3_CHUNK_FILES: &[(&str, File)] = &[
            (
                "art.bin",
                Sha256(
                    "d1d290503566daa14f159719b2e882893da7379743aceb5b7b1e06431b8d1fdb",
                    151,
                ),
            ),
            ("gb_char.bin", UNIGRAM_W3_GB_CHAR_BIN),
            ("gbk_char.bin", UNIGRAM_W3_GBK_CHAR_BIN),
            ("opengram.bin", UNIGRAM_W3_OPENGRAM_BIN),
        ];
        const UNIGRAM_W3_GBK_CHAR_BIN: File = Sha256(
            "7bab65cb960148f16e8b557907eff1fcfe9fb51524a16093c7d9c5810a712749",
            70241,
        );
        const UNIGRAM_W3_GB_CHAR_BIN: File = Sha256(
            "f4b30b2d0e1837d3d851c61c57abdd1b60e63ab17c0b23f3904d30bed1df899b",
            167681,
        );
        const UNIGRAM_W3_OPENGRAM_BIN: File = Sha256(
            "a9bee67e87d126ab6908550327d5d4f8fec462c0caf87b6a74ec6f68d0ebb548",
            4915,
        );

        #[fixture]
        fn db_chunks() -> TempDir {
            let directory = TempDir::new().expect("scratch directory");
            directory
                .copy_from(
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3/db"),
                    &[
                        "table.conf",
                        "gb_char.bin",
                        "gbk_char.bin",
                        "opengram.bin",
                        "merged.bin",
                        "art.bin",
                    ],
                )
                .expect("original chunk inputs");
            directory
        }

        #[rstest]
        // gen_unigram-bogus-option: utils/training/gen_unigram.cpp at 074a2219: 89-92.
        #[case::gen_unigram_bogus_option(
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
        // gen_unigram-default-chunks-then-missing-addon: utils/training/gen_unigram.cpp at
        // 074a2219: 103-110 (the default sweep saves its chunks before the addon sweep fails
        // at the first absent addon chunk).
        #[case::gen_unigram_default_chunks_then_missing_addon(
            db_chunks(),
            &[],
            b"",
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: b"load culture.bin failed!\n",
                files: &[
                    ("gb_char.bin", UNIGRAM_W3_GB_CHAR_BIN),
                    ("gbk_char.bin", UNIGRAM_W3_GBK_CHAR_BIN),
                    ("opengram.bin", UNIGRAM_W3_OPENGRAM_BIN),
                ],
            },
        )]
        // gen_unigram-empty-stdin: utils/training/gen_unigram.cpp at 074a2219: 35-79
        // (gen_unigram reads no stdin; this is the successful sweep).
        #[case::gen_unigram_empty_stdin(
            db_chunks(),
            &[],
            b"",
            &[("table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault RESERVED NULL NULL NULL NOT_USED\ndefault GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\ndefault OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin SYSTEM_FILE\ndefault MERGED_DICTIONARY merged.table merged.bin merged.dbin SYSTEM_FILE\ndefault ADDON_DICTIONARY NULL NULL addon.bin USER_FILE\ndefault NETWORK_DICTIONARY NULL NULL network.bin USER_FILE\ndefault USER_DICTIONARY NULL NULL user.bin USER_FILE\n\naddon 4 art.table art.bin NULL DICTIONARY\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: UNIGRAM_W3_CHUNK_FILES,
            },
        )]
        // gen_unigram-family-e-truncated-first-chunk: utils/training/gen_unigram.cpp:53-58;
        // utils/utils_helper.h:73-94; src/include/memory_chunk.h:420-463 at 074a2219.
        // An absent chunk takes the same load failure and the same exit.
        #[case::gen_unigram_family_e_truncated_first_chunk(
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
        // gen_unigram-help: utils/training/gen_unigram.cpp at 074a2219: 81-92 (GLib --help;
        // the static table_dir is never read).
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::gen_unigram_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption("Usage:\n  gen_unigram [OPTION?] - increase uni-gram\n\nHelp Options:\n  -h, --help       Show help options\n\nApplication Options:\n  --table-dir      table directory\n\n"),
                stderr: b"",
                files: &[],
            },
        )]
        // gen_unigram-missing-table-conf: utils/training/gen_unigram.cpp at 074a2219: 94-101.
        #[case::gen_unigram_missing_table_conf(
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
        // gen_unigram-too-many-arguments: utils/training/gen_unigram.cpp at 074a2219: 89-112
        // (g_option_context_parse leaves extra arguments; main never reads argc).
        #[case::gen_unigram_too_many_arguments(
            db_chunks(),
            &["first", "second"],
            b"",
            &[("table.conf", b"binary format version:7\nmodel data version:14\nlambda parameter:0.312699\n\nsource table format:pinyin\ndatabase format:Tkrzw\n\ndefault RESERVED NULL NULL NULL NOT_USED\ndefault GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\ndefault OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin SYSTEM_FILE\ndefault MERGED_DICTIONARY merged.table merged.bin merged.dbin SYSTEM_FILE\ndefault ADDON_DICTIONARY NULL NULL addon.bin USER_FILE\ndefault NETWORK_DICTIONARY NULL NULL network.bin USER_FILE\ndefault USER_DICTIONARY NULL NULL user.bin USER_FILE\n\naddon 4 art.table art.bin NULL DICTIONARY\n".as_slice())],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: UNIGRAM_W3_CHUNK_FILES,
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
            let mut output = cargo_bin_cmd!("gen_unigram")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run gen_unigram");
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
