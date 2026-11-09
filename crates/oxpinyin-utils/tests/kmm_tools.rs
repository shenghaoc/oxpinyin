//! The seven KMM utilities on the committed W3 fixtures, for the backend
//! this build selected. Every expected byte is source-derived from
//! `utils/training/*_k_mixture_model.cpp` at libpinyin 074a2219 and from the
//! fixture corpus `fixtures/w9/segmenter-spseg-w3.txt`; the native DBM walk
//! order is never asserted, only the set of records it emits.

use oxpinyin_input::tool;

/// Format independently with libc's `%f`, as the utility's printf family does.
#[allow(unsafe_code)]
fn host_f(value: f64) -> String {
    let mut buffer = [0_u8; 64];
    // SAFETY: the buffer is writable and `%f` takes one double.
    let length = unsafe {
        libc::snprintf(
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            c"%f".as_ptr(),
            value,
        )
    };
    let length = usize::try_from(length).expect("snprintf failed");
    assert!(length < buffer.len());
    String::from_utf8(buffer[..length].to_vec()).expect("libc output is UTF-8")
}

/// The fixture's gamma produces 0/0, which the later mixture arithmetic propagates.
fn prune_fixture_nan() -> f64 {
    let n = std::hint::black_box(1.0_f64);
    let n_0 = std::hint::black_box(1.0_f64);
    let n_1 = std::hint::black_box(0.0_f64);
    let t = std::hint::black_box(10.0_f64);
    let alpha = 1.0 - n_0 / n;
    let gamma = 1.0 - n_1 / (n - n_0);
    let b = (t - n_1) / (n - n_0 - n_1);
    let mut remained = 1.0;
    remained -= 1.0 - alpha;
    remained -= alpha * (1.0 - gamma);
    remained -= (alpha * gamma / (b - 1.0)) * (1.0 - 1.0 / (b - 1.0)).powf(0.0);
    remained
}

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const DOCUMENT: &str = "fixtures/w9/segmenter-spseg-w3.txt";
/// The same model merged with itself: counts and document frequencies
/// double, `m_Mr` keeps its maximum (`merge_k_mixture_model.cpp:59-75`).
const MERGED_UNIGRAMS: &[&str] = &[
    "\\item 1 <start> count 44 freq 0",
    "\\item 16779429 好 count 2 freq 2",
    "\\item 16779448 号 count 0 freq 10",
    "\\item 16780268 过 count 0 freq 2",
    "\\item 16780275 国 count 0 freq 4",
    "\\item 16780944 见 count 0 freq 2",
    "\\item 16785125 现 count 0 freq 4",
    "\\item 16786821 中 count 6 freq 18",
    "\\item 16802309 你好 count 2 freq 6",
    "\\item 16817937 中国 count 2 freq 8",
];
const MERGED_BIGRAMS: &[&str] = &[
    "\\item 1 <start> 16779429 好 count 2 T 2 N_n_0 2 n_1 2 Mr 1",
    "\\item 1 <start> 16779448 号 count 10 T 10 N_n_0 2 n_1 0 Mr 5",
    "\\item 1 <start> 16780268 过 count 2 T 2 N_n_0 2 n_1 2 Mr 1",
    "\\item 1 <start> 16780275 国 count 4 T 4 N_n_0 2 n_1 0 Mr 2",
    "\\item 1 <start> 16780944 见 count 2 T 2 N_n_0 2 n_1 2 Mr 1",
    "\\item 1 <start> 16785125 现 count 4 T 4 N_n_0 2 n_1 0 Mr 2",
    "\\item 1 <start> 16786821 中 count 10 T 10 N_n_0 2 n_1 0 Mr 5",
    "\\item 1 <start> 16802309 你好 count 4 T 4 N_n_0 2 n_1 0 Mr 2",
    "\\item 1 <start> 16817937 中国 count 6 T 6 N_n_0 2 n_1 0 Mr 3",
    "\\item 16779429 好 16786821 中 count 2 T 2 N_n_0 2 n_1 2 Mr 1",
    "\\item 16786821 中 16786821 中 count 6 T 6 N_n_0 2 n_1 0 Mr 3",
    "\\item 16802309 你好 16817937 中国 count 2 T 2 N_n_0 2 n_1 2 Mr 1",
    "\\item 16817937 中国 16802309 你好 count 2 T 2 N_n_0 2 n_1 2 Mr 1",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// `fixtures/w3/<the compiled backend's directory>`; a missing directory
/// fails the test, never skips it.
fn fixture_dir() -> PathBuf {
    let dir = repo_root()
        .join("fixtures/w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT);
    assert!(
        dir.is_dir(),
        "missing W3 fixture directory {}",
        dir.display()
    );
    dir
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A scratch copy of the fixture directory: cwd-relative `table.conf` and
/// chunk names, exactly as the utilities open them.
fn scratch() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_dir(), dir.path());
    dir
}

fn binary(name: &str) -> PathBuf {
    PathBuf::from(match name {
        "validate_k_mixture_model" => env!("CARGO_BIN_EXE_validate_k_mixture_model"),
        "merge_k_mixture_model" => env!("CARGO_BIN_EXE_merge_k_mixture_model"),
        "prune_k_mixture_model" => env!("CARGO_BIN_EXE_prune_k_mixture_model"),
        "estimate_k_mixture_model" => env!("CARGO_BIN_EXE_estimate_k_mixture_model"),
        "import_k_mixture_model" => env!("CARGO_BIN_EXE_import_k_mixture_model"),
        "export_k_mixture_model" => env!("CARGO_BIN_EXE_export_k_mixture_model"),
        "gen_k_mixture_model" => env!("CARGO_BIN_EXE_gen_k_mixture_model"),
        other => panic!("unregistered utility {other}"),
    })
}

fn run(cwd: &Path, name: &str, args: &[&str], stdin: Option<&[u8]>) -> Output {
    let path = binary(name);
    assert!(path.is_file(), "missing built utility {}", path.display());
    let mut command = Command::new(path);
    command
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("LC_ALL", "C");
    let mut child = command.spawn().unwrap();
    if let Some(input) = stdin {
        use std::io::Write as _;
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn expected_text(text: &str) -> String {
    text.replace('\n', oxpinyin_input::TEXT_NEWLINE)
}

fn block(export: &str, header: &str, until: &str) -> Vec<String> {
    let header = expected_text(header);
    let until = expected_text(until);
    let section = export
        .split(header.as_str())
        .nth(1)
        .unwrap_or_else(|| panic!("no {header:?} section"));
    let section = section.split(until.as_str()).next().unwrap();
    let mut lines: Vec<String> = section
        .lines()
        .filter(|line| line.starts_with("\\item "))
        .map(str::to_owned)
        .collect();
    lines.sort();
    lines
}
#[test]
fn merge_sums_the_same_model_and_refuses_a_missing_result_file() {
    let dir = scratch();
    let document = repo_root().join(DOCUMENT);
    let out = run(
        dir.path(),
        "gen_k_mixture_model",
        &[
            "--k-mixture-model-file",
            "model.db",
            document.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(0));
    let out = run(
        dir.path(),
        "merge_k_mixture_model",
        &["--result-file", "merged.db", "model.db", "model.db"],
        None,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    let out = run(
        dir.path(),
        "export_k_mixture_model",
        &["--k-mixture-model-file", "merged.db"],
        None,
    );
    assert_eq!(out.status.code(), Some(0));
    let export = text(&out.stdout);
    assert_eq!(
        export.lines().next(),
        Some("\\data model \"k mixture model\" count 56 N 2 total_freq 56")
    );
    assert_eq!(block(&export, "\\1-gram\n", "\\2-gram\n"), MERGED_UNIGRAMS);
    assert_eq!(block(&export, "\\2-gram\n", "\\end\n"), MERGED_BIGRAMS);

    // No result file: the target is the pin's NULL attach, and the first
    // source's magic-header check aborts (`merge:104-128`).
    let _out = run(dir.path(), "merge_k_mixture_model", &["model.db"], None);
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(_out.status.signal(), Some(6), "SIGABRT");
    }
    #[cfg(windows)]
    assert_eq!(_out.status.code(), Some(-1_073_740_791));
    assert!(!dir.path().join("merged-null.db").exists());
}

#[test]
fn prune_with_zero_k_and_cdf_keeps_the_model_and_prints_one_star_per_row() {
    let dir = scratch();
    let document = repo_root().join(DOCUMENT);
    let out = run(
        dir.path(),
        "gen_k_mixture_model",
        &[
            "--k-mixture-model-file",
            "model.db",
            document.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(0));
    // `items->len / 20` is zero for the model's ten rows, so every row
    // prints one asterisk, then the trailing newline (`prune:141-146,175`).
    let out = run(
        dir.path(),
        "prune_k_mixture_model",
        &["-k", "0", "--CDF", "0", "model.db"],
        None,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    assert_eq!(text(&out.stderr), expected_text("**********\n"));
    let out = run(dir.path(), "validate_k_mixture_model", &["model.db"], None);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
}
#[test]
fn prune_attaches_without_create_and_leaves_a_missing_model_missing() {
    // `prune_k_mixture_model.cpp:129` attaches ATTACH_READWRITE without
    // ATTACH_CREATE, so a missing model is not created and its magic
    // header is missing (`:131-134`).
    let dir = scratch();
    let out = run(dir.path(), "prune_k_mixture_model", &["absent.db"], None);
    assert_eq!(out.status.code(), Some(tool::ENODATA));
    assert_eq!(
        text(&out.stderr),
        expected_text("no magic header in k mixture model.\n")
    );
    assert!(!dir.path().join("absent.db").exists(), "no file is created");
}
#[test]
fn the_committed_kmm_fixtures_are_the_pin_records() {
    let dir = scratch();
    // The valid model validates and exports with the source-derived header
    // and record set.
    let out = run(
        dir.path(),
        "validate_k_mixture_model",
        &["k_mixture_model.db"],
        None,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    let out = run(
        dir.path(),
        "export_k_mixture_model",
        &["--k-mixture-model-file", "k_mixture_model.db"],
        None,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let export = text(&out.stdout);
    assert_eq!(
        export.lines().next(),
        Some("\\data model \"k mixture model\" count 3 N 1 total_freq 3")
    );
    assert_eq!(
        block(&export, "\\1-gram\n", "\\2-gram\n"),
        [
            "\\item 1 <start> count 2 freq 0",
            "\\item 16802309 你好 count 0 freq 2",
            "\\item 16817937 中国 count 1 freq 1",
        ]
    );

    // The EDOM model exits before its first store, with the two pin lines.
    let out = run(
        dir.path(),
        "prune_k_mixture_model",
        &["k_mixture_model_edom.db"],
        None,
    );
    assert_eq!(out.status.code(), Some(tool::EDOM));
    assert_eq!(
        text(&out.stderr),
        expected_text(&format!(
            "*some wrong possibility is encountered:{}.\nk:3 N:1 WC:10 n_0:1 n_1:0\n",
            host_f(prune_fixture_nan())
        ))
    );

    // The zero-total candidate aborts in `compute_interpolation`.
    let out = run(
        dir.path(),
        "estimate_k_mixture_model",
        &[
            "--bigram-file",
            "k_mixture_model_zero.db",
            "--deleted-bigram-file",
            "k_mixture_model.db",
        ],
        None,
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(out.status.signal(), Some(6), "SIGABRT");
    }
    #[cfg(windows)]
    assert_eq!(out.status.code(), Some(-1_073_740_791));
    assert!(out.stdout.is_empty());

    // No result file: the NULL attach aborts in `merge_magic_header`.
    let _out = run(
        dir.path(),
        "merge_k_mixture_model",
        &["k_mixture_model.db"],
        None,
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(_out.status.signal(), Some(6), "SIGABRT");
    }
    #[cfg(windows)]
    assert_eq!(_out.status.code(), Some(-1_073_740_791));
    assert!(!dir.path().join("k_mixture_model_ngram.db").exists());
}

#[cfg(unix)]
mod common;

#[cfg(unix)]
mod fixtures;

// Unix only: the frozen pin runs are Linux and macOS observations (signal statuses, Unix paths and line endings).
#[cfg(unix)]
mod frozen {
    use crate::common::CELL;

    const WRONG_ARGUMENTS: &[u8] = b"wrong arguments.\n";
    fn empty_db_not_berkeley_db_twice() -> &'static [u8] {
        match CELL {
            "bdb" => b"BDB0004 fop_read_meta: empty.db: unexpected file type or format\nBDB0004 fop_read_meta: empty.db: unexpected file type or format\n",
            "kc" | "tkrzw" => b"",
            _ => unreachable!("unknown cell {CELL}"),
        }
    }

    mod estimate_k_mixture_model {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::empty_db_not_berkeley_db_twice;
        use crate::common::CELL;
        use crate::fixtures::w3;

        #[rstest]
        // kmm-estimate-bad-option: utils/training/estimate_k_mixture_model.cpp at 074a2219:
        // 98-155, 36-96.
        #[case::kmm_estimate_bad_option(
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
        // kmm-estimate-existing-empty: utils/training/estimate_k_mixture_model.cpp:111-153 at
        // 074a2219.
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        // Platform exception, aarch64, x86_64: a default NaN prints nan on aarch64 and -nan on
        // x86_64.
        #[case::kmm_estimate_existing_empty(
            TempDir::new().unwrap(),
            &["--bigram-file", "empty.db", "--deleted-bigram-file", "empty.db"],
            b"",
            &[("empty.db", b"".as_slice())],
            Run {
                status: Exit(0),
                stdout: AVERAGE_LAMBDA_NAN,
                stderr: empty_db_not_berkeley_db_twice(),
                files: &[],
            },
        )]
        // kmm-estimate-good: utils/training/estimate_k_mixture_model.cpp at 074a2219: 98-155,
        // 36-96.
        // Input: KC fixture contains an unsorted sentence-start row assembled manually; see
        // eio-flexible-bigram-unsorted-records.md. Sorted bdb/tkrzw rows are controls.
        #[case::kmm_estimate_good(
            w3(),
            &["--bigram-file", "k_mixture_model.db", "--deleted-bigram-file", "k_mixture_model.db"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: match CELL {
                    "bdb" | "tkrzw" => Exact(b"token:1 lambda:0.901963\ntoken:16817937 lambda:0.998480\naverage lambda:0.950222\n"),
                    "kc" => Exact(b"token:1 lambda:0.000000\ntoken:16817937 lambda:0.998480\naverage lambda:0.499240\n"),
                    _ => unreachable!("unknown cell {CELL}"),
                },
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-estimate-help: utils/training/estimate_k_mixture_model.cpp at 074a2219: 98-155,
        // 36-96.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::kmm_estimate_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/estimate_k_mixture_model/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-estimate-no-models: utils/training/estimate_k_mixture_model.cpp at 074a2219:
        // 98-155, 36-96.
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        // Platform exception, aarch64, x86_64: a default NaN prints nan on aarch64 and -nan on
        // x86_64.
        #[case::kmm_estimate_no_models(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: AVERAGE_LAMBDA_NAN,
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-estimate-zero-total: utils/training/estimate_k_mixture_model.cpp at 074a2219:
        // 98-155, 36-96.
        // Policy-accepted on every cell: docs/findings/eio-kmm-tools.md; the pin's own result
        // is in fixtures/contract-policy/eio-kmm-tools.json.
        #[case::kmm_estimate_zero_total(
            w3(),
            &["--bigram-file", "k_mixture_model_zero.db", "--deleted-bigram-file", "k_mixture_model.db"],
            b"",
            &[],
            Run {
                status: Signal(6),
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
            let mut output = cargo_bin_cmd!("estimate_k_mixture_model")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run estimate_k_mixture_model");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod export_k_mixture_model {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use crate::common::CELL;
        use crate::fixtures::w3;

        #[rstest]
        // export_k_mixture_model-family-e-truncated-first-chunk:
        // utils/training/export_k_mixture_model.cpp:139; utils/utils_helper.h:73-94;
        // src/include/memory_chunk.h:420-463 at 074a2219.
        // gen_k_mixture_model and import_k_mixture_model load the chunks through the same
        // phrase_index::load_system_phrases; their missing-table-conf cases cover the call.
        #[case::export_k_mixture_model_family_e_truncated_first_chunk(
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
        // kmm-export-absent-file: utils/training/export_k_mixture_model.cpp at 074a2219:
        // 112-155, 35-110; src/storage/tag_utility.cpp:360-417.
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::kmm_export_absent_file(
            w3(),
            &["--k-mixture-model-file", "absent.db"],
            b"",
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: b"open absent.db failed.\n",
                files: &[],
            },
        )]
        // kmm-export-bad-option: utils/training/export_k_mixture_model.cpp at 074a2219:
        // 112-155, 35-110; src/storage/tag_utility.cpp:360-417.
        #[case::kmm_export_bad_option(
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
        // kmm-export-existing-empty: utils/training/export_k_mixture_model.cpp:142-150 at
        // 074a2219.
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        #[case::kmm_export_existing_empty(
            w3(),
            &["--k-mixture-model-file", "empty.db"],
            b"",
            &[("empty.db", b"".as_slice())],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: match CELL {
                    "bdb" => b"BDB0004 fop_read_meta: empty.db: unexpected file type or format\nopen empty.db failed.\n",
                    "kc" | "tkrzw" => b"open empty.db failed.\n",
                    _ => unreachable!("unknown cell {CELL}"),
                },
                files: &[],
            },
        )]
        // kmm-export-good: utils/training/export_k_mixture_model.cpp at 074a2219: 112-155,
        // 35-110; src/storage/tag_utility.cpp:360-417.
        #[case::kmm_export_good(
            w3(),
            &["--k-mixture-model-file", "k_mixture_model.db"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: match CELL {
                    "bdb" | "tkrzw" => Exact(include_bytes!("fixtures/export_k_mixture_model/model-bdb-tkrzw-order.txt")),
                    "kc" => Exact(include_bytes!("fixtures/export_k_mixture_model/model-kc-order.txt")),
                    _ => unreachable!("unknown cell {CELL}"),
                },
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-export-help: utils/training/export_k_mixture_model.cpp at 074a2219: 112-155,
        // 35-110; src/storage/tag_utility.cpp:360-417.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::kmm_export_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/export_k_mixture_model/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-export-missing-table-conf: utils/training/export_k_mixture_model.cpp at
        // 074a2219: 112-155, 35-110; src/storage/tag_utility.cpp:360-417.
        #[case::kmm_export_missing_table_conf(
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
        // kmm-export-null-name: utils/training/export_k_mixture_model.cpp at 074a2219:
        // 112-155, 35-110; src/storage/tag_utility.cpp:360-417.
        #[case::kmm_export_null_name(
            w3(),
            &[],
            b"",
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: b"open (null) failed.\n",
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
            let mut output = cargo_bin_cmd!("export_k_mixture_model")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run export_k_mixture_model");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, &[], &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod gen_k_mixture_model {
        use crate::common::expected::*;
        use crate::common::{CELL, inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::empty_db_not_berkeley_db_twice;
        use crate::fixtures::w3;

        #[rstest]
        // kmm-gen-bad-option: utils/training/gen_k_mixture_model.cpp at 074a2219: 313-403,
        // 65-311; utils/utils_helper.h:51-95.
        #[case::kmm_gen_bad_option(
            TempDir::new().unwrap(),
            &["--bogus"],
            b"",
            &[],
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(UNKNOWN_OPTION_BOGUS),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-gen-existing-empty: utils/training/gen_k_mixture_model.cpp:348-403;
        // src/storage/flexible_ngram_tkrzwdb.h:145-177 at 074a2219;
        // src/storage/flexible_ngram_kyotodb.h:282-308 at 074a2219.
        // Policy-accepted on kc: docs/findings/eio-tool-check-messages.md; the pin's own
        // result is in fixtures/contract-policy/eio-tool-check-messages.json.
        // Platform exception, macos: a written DBM container's size and bytes follow the 16
        // KiB page, against 4 KiB on Linux.
        #[case::kmm_gen_existing_empty(
            w3(),
            &["--k-mixture-model-file", "empty.db", "doc.txt"],
            b"",
            &[("empty.db", b"".as_slice()), ("doc.txt", b"1 <start>\n16817937 \xe4\xb8\xad\xe5\x9b\xbd\n16802309 \xe4\xbd\xa0\xe5\xa5\xbd\n".as_slice())],
            if ["bdb", "tkrzw"].contains(&CELL) { &["empty.db"][..] } else { &[] },
            Run {
                status: match CELL {
                    "bdb" | "tkrzw" => Exit(0),
                    "kc" => Signal(6),
                    _ => unreachable!("unknown cell {CELL}"),
                },
                stdout: Exact(b""),
                stderr: empty_db_not_berkeley_db_twice(),
                files: match CELL {
                    "bdb" | "tkrzw" => &[
                        ("empty.db", Rows("fcc763fb9a18a33cdfd2f6243e06f7d45ab2656c742251b9b59bb4295ff076c7")),
                    ],
                    "kc" => if cfg!(target_os = "macos") {
                        &[
                            ("empty.db", Sha256("ddbe757462d2111ffafcd0538242d6e147348e1c526ae2b3180926d19d8d9602", 9453568)),
                        ]
                    } else {
                        &[
                            ("empty.db", Sha256("7f072ce7cc8e11878a63d4c690528df09387573b842601e5aea3700c3e9fbcc8", 9449472)),
                        ]
                    },
                    _ => unreachable!("unknown cell {CELL}"),
                },
            },
        )]
        // kmm-gen-good-document: utils/training/gen_k_mixture_model.cpp at 074a2219: 313-403,
        // 65-311; utils/utils_helper.h:51-95.
        // Policy-accepted on every cell: docs/findings/eio-kmm-tools.md; the pin's own result
        // is in fixtures/contract-policy/eio-kmm-tools.json.
        #[case::kmm_gen_good_document(
            w3(),
            &["doc.txt"],
            b"",
            &[("doc.txt", b"1 <start>\n16817937 \xe4\xb8\xad\xe5\x9b\xbd\n16802309 \xe4\xbd\xa0\xe5\xa5\xbd\n".as_slice())],
            &[],
            Run {
                status: Signal(6),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-gen-help: utils/training/gen_k_mixture_model.cpp at 074a2219: 313-403, 65-311;
        // utils/utils_helper.h:51-95.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::kmm_gen_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/gen_k_mixture_model/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-gen-missing-document: utils/training/gen_k_mixture_model.cpp at 074a2219:
        // 313-403, 65-311; utils/utils_helper.h:51-95.
        #[case::kmm_gen_missing_document(
            w3(),
            &["absent.txt"],
            b"",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: b"can't open file: absent.txt.\nerror:No such file or directory.\n",
                files: &[],
            },
        )]
        // kmm-gen-missing-table-conf: utils/training/gen_k_mixture_model.cpp at 074a2219:
        // 313-403, 65-311; utils/utils_helper.h:51-95.
        #[case::kmm_gen_missing_table_conf(
            TempDir::new().unwrap(),
            &["absent.txt"],
            b"",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_TABLE_CONF_FAILED,
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
            let mut output = cargo_bin_cmd!("gen_k_mixture_model")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run gen_k_mixture_model");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, dbm_files, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod import_k_mixture_model {
        use crate::common::expected::*;
        use crate::common::{CELL, inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::empty_db_not_berkeley_db_twice;
        use crate::fixtures::w3;

        #[rstest]
        // kmm-import-bad-option: utils/training/import_k_mixture_model.cpp at 074a2219:
        // 258-317, 72-256; src/storage/tag_utility.cpp:118-264.
        #[case::kmm_import_bad_option(
            TempDir::new().unwrap(),
            &["--bogus"],
            b"",
            &[],
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(UNKNOWN_OPTION_BOGUS),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-import-empty-input: utils/training/import_k_mixture_model.cpp at 074a2219:
        // 258-317, 72-256; src/storage/tag_utility.cpp:118-264.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        #[case::kmm_import_empty_input(
            w3(),
            &[],
            b"",
            &[],
            &[],
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
        // kmm-import-existing-empty: utils/training/import_k_mixture_model.cpp:292-316 at
        // 074a2219.
        #[case::kmm_import_existing_empty(
            w3(),
            &["--k-mixture-model-file", "empty.db"],
            b"\\data model \"k mixture model\" count 3 N 1 total_freq 3\n\\1-gram\n\\item 1 <start> count 2 freq 0\n\\item 16817937 \xe4\xb8\xad\xe5\x9b\xbd count 1 freq 1\n\\item 16802309 \xe4\xbd\xa0\xe5\xa5\xbd count 0 freq 2\n\\2-gram\n\\item 1 <start> 16817937 \xe4\xb8\xad\xe5\x9b\xbd count 1 T 1 N_n_0 1 n_1 1 Mr 1\n\\item 1 <start> 16802309 \xe4\xbd\xa0\xe5\xa5\xbd count 1 T 1 N_n_0 1 n_1 1 Mr 1\n\\item 16817937 \xe4\xb8\xad\xe5\x9b\xbd 16802309 \xe4\xbd\xa0\xe5\xa5\xbd count 1 T 1 N_n_0 1 n_1 1 Mr 1\n\\end\n",
            &[("empty.db", b"".as_slice())],
            if ["bdb", "tkrzw"].contains(&CELL) { &["empty.db"][..] } else { &[] },
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: empty_db_not_berkeley_db_twice(),
                files: match CELL {
                    "bdb" | "tkrzw" => &[
                        ("empty.db", Rows("981be74bdf7626e50c7e75b20a885de4d2adc1ec59e758d67fcba44ab4fd0973")),
                    ],
                    "kc" => &[
                        ("empty.db", Sha256("938362d1aa417adaf1fefb58343fa368806badab7b37acb0ce5de79f3fe666df", 6297976)),
                    ],
                    _ => unreachable!("unknown cell {CELL}"),
                },
            },
        )]
        // kmm-import-foreign-header: utils/training/import_k_mixture_model.cpp at 074a2219:
        // 258-317, 72-256; src/storage/tag_utility.cpp:118-264.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        #[case::kmm_import_foreign_header(
            w3(),
            &[],
            b"\\data model interpolation\n\\end\n",
            &[],
            &[],
            Run {
                status: if cfg!(target_os = "macos") {
                    Exit(96)
                } else {
                    Exit(61)
                },
                stdout: Exact(b""),
                stderr: b"\n** (import_k_mixture_model:PID): WARNING **: TIME: missed required tags: count.\n\nerror: k mixture model expected.\n",
                files: &[],
            },
        )]
        // kmm-import-good: utils/training/import_k_mixture_model.cpp at 074a2219: 258-317,
        // 72-256; src/storage/tag_utility.cpp:118-264.
        #[case::kmm_import_good(
            w3(),
            &[],
            b"\\data model \"k mixture model\" count 3 N 1 total_freq 3\n\\1-gram\n\\item 1 <start> count 2 freq 0\n\\item 16817937 \xe4\xb8\xad\xe5\x9b\xbd count 1 freq 1\n\\item 16802309 \xe4\xbd\xa0\xe5\xa5\xbd count 0 freq 2\n\\2-gram\n\\item 1 <start> 16817937 \xe4\xb8\xad\xe5\x9b\xbd count 1 T 1 N_n_0 1 n_1 1 Mr 1\n\\item 1 <start> 16802309 \xe4\xbd\xa0\xe5\xa5\xbd count 1 T 1 N_n_0 1 n_1 1 Mr 1\n\\item 16817937 \xe4\xb8\xad\xe5\x9b\xbd 16802309 \xe4\xbd\xa0\xe5\xa5\xbd count 1 T 1 N_n_0 1 n_1 1 Mr 1\n\\end\n",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-import-help: utils/training/import_k_mixture_model.cpp at 074a2219: 258-317,
        // 72-256; src/storage/tag_utility.cpp:118-264.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::kmm_import_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/import_k_mixture_model/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-import-missing-table-conf: utils/training/import_k_mixture_model.cpp at
        // 074a2219: 258-317, 72-256; src/storage/tag_utility.cpp:118-264.
        #[case::kmm_import_missing_table_conf(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[],
            &[],
            Run {
                status: Exit(2),
                stdout: Exact(b""),
                stderr: OPEN_TABLE_CONF_FAILED,
                files: &[],
            },
        )]
        // kmm-import-unigram-only: utils/training/import_k_mixture_model.cpp at 074a2219:
        // 258-317, 72-256; src/storage/tag_utility.cpp:118-264. A 1-gram-only model with no
        // --k-mixture-model-file: the pin exits 0, prints nothing and leaves the tree as it was.
        // Pin capture of 2026-11-07 on the bdb, kc and tkrzw cells, runs 1 and 2 identical,
        // observations utility/<cell>/kmm-import-unigram.
        #[case::kmm_import_unigram_only(
            w3(),
            &[],
            b"\\data model \"k mixture model\" count 3 N 1 total_freq 3\n\\1-gram\n\\item 1 <start> count 2 freq 0\n\\item 16817937 \xe4\xb8\xad\xe5\x9b\xbd count 1 freq 1\n\\item 16802309 \xe4\xbd\xa0\xe5\xa5\xbd count 0 freq 2\n\\end\n",
            &[],
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
            let mut output = cargo_bin_cmd!("import_k_mixture_model")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run import_k_mixture_model");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, dbm_files, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod merge_k_mixture_model {
        use crate::common::expected::*;
        use crate::common::{CELL, inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::empty_db_not_berkeley_db_twice;
        use crate::fixtures::w3;

        #[rstest]
        // kmm-merge-bad-option: utils/training/merge_k_mixture_model.cpp at 074a2219: 210-237,
        // 96-129; src/include/pinyin_utils.h:26-31.
        #[case::kmm_merge_bad_option(
            TempDir::new().unwrap(),
            &["--bogus"],
            b"",
            &[],
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(UNKNOWN_OPTION_BOGUS),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-merge-existing-empty: utils/training/merge_k_mixture_model.cpp:225-237 at
        // 074a2219.
        #[case::kmm_merge_existing_empty(
            TempDir::new().unwrap(),
            &["--result-file", "empty.db"],
            b"",
            &[("empty.db", b"".as_slice())],
            if ["bdb", "tkrzw"].contains(&CELL) { &["empty.db"][..] } else { &[] },
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: empty_db_not_berkeley_db_twice(),
                files: match CELL {
                    "bdb" => &[
                        ("empty.db", Rows("32f22980ef437bfce6b0604d57bd79fb236df250117313212e9c9cd88f5f1897")),
                    ],
                    "kc" => &[
                        ("empty.db", Sha256("ce29037244cafcc7ebef77c90b6434d16a4876b9867e2eba2a70ce101b7f7566", 6297720)),
                    ],
                    "tkrzw" => &[
                        ("empty.db", NO_ROWS),
                    ],
                    _ => unreachable!("unknown cell {CELL}"),
                },
            },
        )]
        // kmm-merge-help: utils/training/merge_k_mixture_model.cpp at 074a2219: 210-237,
        // 96-129; src/include/pinyin_utils.h:26-31.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::kmm_merge_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption("Usage:\n  merge_k_mixture_model [OPTION?] - merge k mixture model\n\nHelp Options:\n  -h, --help        Show help options\n\nApplication Options:\n  --result-file     merged result file\n\n"),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-merge-no-result-file: utils/training/merge_k_mixture_model.cpp at 074a2219:
        // 210-237, 96-129; src/include/pinyin_utils.h:26-31.
        // Policy-accepted on every cell: docs/findings/eio-kmm-tools.md; the pin's own result
        // is in fixtures/contract-policy/eio-kmm-tools.json.
        #[case::kmm_merge_no_result_file(
            w3(),
            &["k_mixture_model.db"],
            b"",
            &[],
            &[],
            Run {
                status: Signal(6),
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
            let mut output = cargo_bin_cmd!("merge_k_mixture_model")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run merge_k_mixture_model");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, dbm_files, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod prune_k_mixture_model {
        use crate::common::expected::*;
        use crate::common::{CELL, inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::WRONG_ARGUMENTS;
        use crate::fixtures::w3;

        #[rstest]
        // kmm-prune-bad-option: utils/training/prune_k_mixture_model.cpp at 074a2219: 107-191;
        // k_mixture_model.h:40-95.
        #[case::kmm_prune_bad_option(
            TempDir::new().unwrap(),
            &["--bogus"],
            b"",
            &[],
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(UNKNOWN_OPTION_BOGUS),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-prune-edom: utils/training/prune_k_mixture_model.cpp at 074a2219: 107-191;
        // k_mixture_model.h:40-95.
        // Platform exception, aarch64, x86_64: a default NaN prints nan on aarch64 and -nan on
        // x86_64.
        #[case::kmm_prune_edom(
            w3(),
            &["k_mixture_model_edom.db"],
            b"",
            &[],
            &[],
            Run {
                status: Exit(33),
                stdout: Exact(b""),
                stderr: if cfg!(target_arch = "aarch64") {
                    b"*some wrong possibility is encountered:nan.\nk:3 N:1 WC:10 n_0:1 n_1:0\n"
                } else {
                    b"*some wrong possibility is encountered:-nan.\nk:3 N:1 WC:10 n_0:1 n_1:0\n"
                },
                files: match CELL {
                    "bdb" => &[],
                    "kc" => &[
                        ("k_mixture_model_edom.db", Sha256("3560fabf415732a006a29d74b4ff5f95232e06517303a3b2985a117dfbe9149d", 6696)),
                    ],
                    "tkrzw" => &[
                        ("k_mixture_model_edom.db", Sha256("c04a50789ab8477be91e3ed71b7bff2d0e5cee02deebc645af23503d6bbd87a1", 1048576)),
                    ],
                    _ => unreachable!("unknown cell {CELL}"),
                },
            },
        )]
        // kmm-prune-existing-empty: utils/training/prune_k_mixture_model.cpp:125-135 at
        // 074a2219; src/storage/flexible_ngram_kyotodb.h:282-308 at 074a2219.
        // Policy-accepted on bdb, kc: docs/findings/eio-tool-check-messages.md; the pin's own
        // result is in fixtures/contract-policy/eio-tool-check-messages.json.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        #[case::kmm_prune_existing_empty(
            TempDir::new().unwrap(),
            &["empty.db"],
            b"",
            &[("empty.db", b"".as_slice())],
            if CELL == "tkrzw" { &["empty.db"][..] } else { &[] },
            Run {
                status: match CELL {
                    "bdb" | "tkrzw" => if cfg!(target_os = "macos") {
                        Exit(96)
                    } else {
                        Exit(61)
                    },
                    "kc" => Signal(6),
                    _ => unreachable!("unknown cell {CELL}"),
                },
                stdout: Exact(b""),
                stderr: match CELL {
                    "bdb" => b"BDB0004 fop_read_meta: empty.db: unexpected file type or format\nno magic header in k mixture model.\n",
                    "kc" => b"",
                    "tkrzw" => b"no magic header in k mixture model.\n",
                    _ => unreachable!("unknown cell {CELL}"),
                },
                files: match CELL {
                    "bdb" => &[],
                    "kc" => &[
                        ("empty.db", KC_EMPTY_TREE_DB),
                    ],
                    "tkrzw" => &[
                        ("empty.db", NO_ROWS),
                    ],
                    _ => unreachable!("unknown cell {CELL}"),
                },
            },
        )]
        // kmm-prune-help: utils/training/prune_k_mixture_model.cpp at 074a2219: 107-191;
        // k_mixture_model.h:40-95.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::kmm_prune_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption(include_str!("fixtures/prune_k_mixture_model/help.txt")),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-prune-wrong-args: utils/training/prune_k_mixture_model.cpp at 074a2219: 107-191;
        // k_mixture_model.h:40-95.
        #[case::kmm_prune_wrong_args(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[],
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(b""),
                stderr: WRONG_ARGUMENTS,
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
            let mut output = cargo_bin_cmd!("prune_k_mixture_model")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run prune_k_mixture_model");
            output.stderr = mask_glib_prefix(&output.stderr);
            check(
                output,
                &inventory(directory.path(), &inputs, dbm_files, &[], &[], &[]),
                &inputs,
                &expected,
            );
        }
    }
    mod validate_k_mixture_model {
        use crate::common::expected::*;
        use crate::common::{inventory, mask_glib_prefix, snapshot_inputs};
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::time::Duration;

        use super::WRONG_ARGUMENTS;
        use crate::common::CELL;
        use crate::fixtures::w3;

        const NO_MAGIC_HEADER: &[u8] =
            b"no magic header in k mixture model.\nk mixture model validation failed.\n";

        #[rstest]
        // kmm-validate-bad-option: utils/training/validate_k_mixture_model.cpp at 074a2219:
        // 141-173, 28-139; src/include/pinyin_utils.h:26-31.
        #[case::kmm_validate_bad_option(
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
        // kmm-validate-existing-empty: utils/training/validate_k_mixture_model.cpp:157-170 at
        // 074a2219.
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        #[case::kmm_validate_existing_empty(
            TempDir::new().unwrap(),
            &["empty.db"],
            b"",
            &[("empty.db", b"".as_slice())],
            Run {
                status: if cfg!(target_os = "macos") {
                    Exit(96)
                } else {
                    Exit(61)
                },
                stdout: Exact(b""),
                stderr: match CELL {
                    "bdb" => b"BDB0004 fop_read_meta: empty.db: unexpected file type or format\nno magic header in k mixture model.\nk mixture model validation failed.\n",
                    "kc" | "tkrzw" => NO_MAGIC_HEADER,
                    _ => unreachable!("unknown cell {CELL}"),
                },
                files: &[],
            },
        )]
        // kmm-validate-good: utils/training/validate_k_mixture_model.cpp at 074a2219: 141-173,
        // 28-139; src/include/pinyin_utils.h:26-31.
        #[case::kmm_validate_good(
            w3(),
            &["k_mixture_model.db"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-validate-help: utils/training/validate_k_mixture_model.cpp at 074a2219: 141-173,
        // 28-139; src/include/pinyin_utils.h:26-31.
        // Platform exception, stdout.glib-*: GLib spells the help placeholder [OPTION?],
        // [OPTION…] or [OPTION...] by version and locale; the case accepts any of them.
        #[case::kmm_validate_help(
            TempDir::new().unwrap(),
            &["--help"],
            b"",
            &[],
            Run {
                status: Exit(0),
                stdout: GlibOption("Usage:\n  validate_k_mixture_model [OPTION?] - validate k mixture model\n\nHelp Options:\n  -h, --help       Show help options\n\n"),
                stderr: b"",
                files: &[],
            },
        )]
        // kmm-validate-missing-file: utils/training/validate_k_mixture_model.cpp at 074a2219:
        // 141-173, 28-139; src/include/pinyin_utils.h:26-31.
        // Policy-accepted on bdb: docs/findings/bdb-failed-handle.md; the pin's own result is
        // in fixtures/contract-policy/bdb-failed-handle.json.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        #[case::kmm_validate_missing_file(
            TempDir::new().unwrap(),
            &["absent.db"],
            b"",
            &[],
            Run {
                status: if cfg!(target_os = "macos") {
                    Exit(96)
                } else {
                    Exit(61)
                },
                stdout: Exact(b""),
                stderr: NO_MAGIC_HEADER,
                files: &[],
            },
        )]
        // kmm-validate-wrong-args: utils/training/validate_k_mixture_model.cpp at 074a2219:
        // 141-173, 28-139; src/include/pinyin_utils.h:26-31.
        #[case::kmm_validate_wrong_args(
            TempDir::new().unwrap(),
            &[],
            b"",
            &[],
            Run {
                status: Exit(22),
                stdout: Exact(b""),
                stderr: WRONG_ARGUMENTS,
                files: &[],
            },
        )]
        // kmm-validate-zero-header: utils/training/validate_k_mixture_model.cpp at 074a2219:
        // 141-173, 28-139; src/include/pinyin_utils.h:26-31.
        // Platform exception, macos: exit(ENODATA) is 96 on macOS and 61 on Linux.
        #[case::kmm_validate_zero_header(
            w3(),
            &["k_mixture_model_zero.db"],
            b"",
            &[],
            Run {
                status: if cfg!(target_os = "macos") {
                    Exit(96)
                } else {
                    Exit(61)
                },
                stdout: Exact(b""),
                stderr: b"word count in magic header is unexpected zero.\nk mixture model validation failed.\n",
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
            let mut output = cargo_bin_cmd!("validate_k_mixture_model")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run validate_k_mixture_model");
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
