//! The seven KMM utilities on the committed W3 fixtures, for the backend
//! this build selected. Every expected byte is source-derived from
//! `utils/training/*_k_mixture_model.cpp` at libpinyin 074a2219 and from the
//! fixture corpus `fixtures/w9/segmenter-spseg-w3.txt`; the native DBM walk
//! order is never asserted, only the set of records it emits.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const DOCUMENT: &str = "fixtures/w9/segmenter-spseg-w3.txt";

/// The expected `gen_k_mixture_model` model of [`DOCUMENT`]: 28 array-header
/// word counts, one document. Derived from the pin's own export of that run.
const MODEL_HEADER: &str = "\\data model \"k mixture model\" count 28 N 1 total_freq 28";
const UNIGRAMS: &[&str] = &[
    "\\item 1 <start> count 22 freq 0",
    "\\item 16779429 好 count 1 freq 1",
    "\\item 16779448 号 count 0 freq 5",
    "\\item 16780268 过 count 0 freq 1",
    "\\item 16780275 国 count 0 freq 2",
    "\\item 16780944 见 count 0 freq 1",
    "\\item 16785125 现 count 0 freq 2",
    "\\item 16786821 中 count 3 freq 9",
    "\\item 16802309 你好 count 1 freq 3",
    "\\item 16817937 中国 count 1 freq 4",
];
const BIGRAMS: &[&str] = &[
    "\\item 1 <start> 16779429 好 count 1 T 1 N_n_0 1 n_1 1 Mr 1",
    "\\item 1 <start> 16779448 号 count 5 T 5 N_n_0 1 n_1 0 Mr 5",
    "\\item 1 <start> 16780268 过 count 1 T 1 N_n_0 1 n_1 1 Mr 1",
    "\\item 1 <start> 16780275 国 count 2 T 2 N_n_0 1 n_1 0 Mr 2",
    "\\item 1 <start> 16780944 见 count 1 T 1 N_n_0 1 n_1 1 Mr 1",
    "\\item 1 <start> 16785125 现 count 2 T 2 N_n_0 1 n_1 0 Mr 2",
    "\\item 1 <start> 16786821 中 count 5 T 5 N_n_0 1 n_1 0 Mr 5",
    "\\item 1 <start> 16802309 你好 count 2 T 2 N_n_0 1 n_1 0 Mr 2",
    "\\item 1 <start> 16817937 中国 count 3 T 3 N_n_0 1 n_1 0 Mr 3",
    "\\item 16779429 好 16786821 中 count 1 T 1 N_n_0 1 n_1 1 Mr 1",
    "\\item 16786821 中 16786821 中 count 3 T 3 N_n_0 1 n_1 0 Mr 3",
    "\\item 16802309 你好 16817937 中国 count 1 T 1 N_n_0 1 n_1 1 Mr 1",
    "\\item 16817937 中国 16802309 你好 count 1 T 1 N_n_0 1 n_1 1 Mr 1",
];

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
    repo_root().join("target/debug").join(name)
}

fn run(cwd: &Path, name: &str, args: &[&str], stdin: Option<&[u8]>) -> Output {
    let path = binary(name);
    assert!(path.is_file(), "missing built utility {}", path.display());
    let mut command = Command::new(path);
    command
        .current_dir(cwd)
        .env("LC_ALL", "C")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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

fn block(export: &str, header: &str, until: &str) -> Vec<String> {
    let section = export
        .split(header)
        .nth(1)
        .unwrap_or_else(|| panic!("no {header:?} section"));
    let section = section.split(until).next().unwrap();
    let mut lines: Vec<String> = section
        .lines()
        .filter(|line| line.starts_with("\\item "))
        .map(str::to_owned)
        .collect();
    lines.sort();
    lines
}

#[test]
fn help_is_the_pins_glib_help_for_every_utility() {
    for (name, suffix) in [
        ("validate_k_mixture_model", "- validate k mixture model"),
        ("merge_k_mixture_model", "- merge k mixture model"),
        ("prune_k_mixture_model", "- prune k mixture model"),
        ("estimate_k_mixture_model", "- estimate k mixture model"),
        ("import_k_mixture_model", "- import k mixture model"),
        ("export_k_mixture_model", "- export k mixture model"),
        ("gen_k_mixture_model", "- generate k mixture model"),
    ] {
        let out = run(&repo_root(), name, &["--help"], None);
        assert_eq!(out.status.code(), Some(0), "{name}");
        let stdout = text(&out.stdout);
        assert!(
            stdout.starts_with(&format!("Usage:\n  {name} [OPTION\u{2026}] {suffix}\n")),
            "{name}: {stdout:?}"
        );
        assert!(stdout.contains("\nHelp Options:\n  -h, --help"), "{name}");
        assert!(out.stderr.is_empty(), "{name}");
    }
}

#[test]
fn an_unknown_option_is_einval_on_stdout() {
    for name in [
        "validate_k_mixture_model",
        "merge_k_mixture_model",
        "prune_k_mixture_model",
        "estimate_k_mixture_model",
        "import_k_mixture_model",
        "export_k_mixture_model",
        "gen_k_mixture_model",
    ] {
        let out = run(&repo_root(), name, &["--bogus"], None);
        assert_eq!(out.status.code(), Some(22), "{name}");
        assert_eq!(
            text(&out.stdout),
            "option parsing failed:Unknown option --bogus\n",
            "{name}"
        );
        assert!(out.stderr.is_empty(), "{name}");
    }
}

#[test]
fn the_positional_programs_refuse_a_wrong_argument_count() {
    for name in ["validate_k_mixture_model", "prune_k_mixture_model"] {
        for args in [&[][..], &["a", "b"][..]] {
            let out = run(&repo_root(), name, args, None);
            assert_eq!(out.status.code(), Some(22), "{name} {args:?}");
            assert_eq!(text(&out.stderr), "wrong arguments.\n", "{name}");
            assert!(out.stdout.is_empty());
        }
    }
}

#[test]
fn missing_table_conf_is_enoent_after_the_pins_two_lines() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "gen_k_mixture_model",
        "import_k_mixture_model",
        "export_k_mixture_model",
    ] {
        let out = run(dir.path(), name, &[], Some(b""));
        assert_eq!(out.status.code(), Some(2), "{name}");
        assert_eq!(
            text(&out.stderr),
            "open table.conf failed.\nload table.conf failed.\n",
            "{name}"
        );
    }
}

#[test]
fn a_missing_system_chunk_names_itself_and_is_enoent() {
    let dir = scratch();
    std::fs::remove_file(dir.path().join("gbk_char.bin")).unwrap();
    let out = run(
        dir.path(),
        "export_k_mixture_model",
        &["--k-mixture-model-file", "absent.db"],
        None,
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(text(&out.stderr), "load gbk_char.bin failed!\n");
}

#[test]
fn gen_then_validate_then_export_is_the_source_derived_model() {
    let dir = scratch();
    let document = repo_root().join(DOCUMENT);
    let document = document.to_str().unwrap();
    let out = run(
        dir.path(),
        "gen_k_mixture_model",
        &["--k-mixture-model-file", "model.db", document],
        None,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());

    let out = run(dir.path(), "validate_k_mixture_model", &["model.db"], None);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());

    let out = run(
        dir.path(),
        "export_k_mixture_model",
        &["--k-mixture-model-file", "model.db"],
        None,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(out.stderr.is_empty());
    let export = text(&out.stdout);
    let mut lines = export.lines();
    assert_eq!(lines.next(), Some(MODEL_HEADER));
    assert_eq!(block(&export, "\\1-gram\n", "\\2-gram\n"), UNIGRAMS);
    assert_eq!(block(&export, "\\2-gram\n", "\\end\n"), BIGRAMS);

    // The exported text imports back into a container whose export has the
    // same header and the same record set; the native walk order may differ.
    let out = run(
        dir.path(),
        "import_k_mixture_model",
        &["--k-mixture-model-file", "round.db"],
        Some(export.as_bytes()),
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(out.stderr.is_empty());
    let out = run(
        dir.path(),
        "export_k_mixture_model",
        &["--k-mixture-model-file", "round.db"],
        None,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let round = text(&out.stdout);
    assert_eq!(round.lines().next(), Some(MODEL_HEADER));
    assert_eq!(block(&round, "\\1-gram\n", "\\2-gram\n"), UNIGRAMS);
    assert_eq!(block(&round, "\\2-gram\n", "\\end\n"), BIGRAMS);
}

#[test]
fn import_without_a_model_file_only_validates_the_text() {
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
        "export_k_mixture_model",
        &["--k-mixture-model-file", "model.db"],
        None,
    );
    let export = out.stdout;
    let before: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    let out = run(dir.path(), "import_k_mixture_model", &[], Some(&export));
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(out.stderr.is_empty());
    let mut after: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    let mut before = before;
    before.sort();
    after.sort();
    assert_eq!(before, after, "no model file means no container is created");
}

#[test]
fn an_empty_or_foreign_import_is_enodata() {
    let dir = scratch();
    for (input, message) in [
        (&b""[..], "empty file input.\n"),
        (
            &b"\\data model interpolation\n\\end\n"[..],
            "error: k mixture model expected.\n",
        ),
    ] {
        let out = run(dir.path(), "import_k_mixture_model", &[], Some(input));
        assert_eq!(out.status.code(), Some(61), "{message:?}");
        assert!(
            text(&out.stderr).ends_with(message),
            "{:?}",
            text(&out.stderr)
        );
    }
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
    let out = run(dir.path(), "merge_k_mixture_model", &["model.db"], None);
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(out.status.signal(), Some(6), "SIGABRT");
    }
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
    assert_eq!(text(&out.stderr), "**********\n");
    let out = run(dir.path(), "validate_k_mixture_model", &["model.db"], None);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
}

#[test]
fn prune_refuses_a_nan_probability_with_edom_and_leaves_the_file_alone() {
    let dir = scratch();
    let input = "\\data model \"k mixture model\" count 10 N 1 total_freq 10\n\
                 \\1-gram\n\
                 \\item 1 <start> count 10 freq 10\n\
                 \\2-gram\n\
                 \\item 1 <start> 1 <start> count 10 T 10 N_n_0 0 n_1 0 Mr 10\n\
                 \\end\n";
    let out = run(
        dir.path(),
        "import_k_mixture_model",
        &["--k-mixture-model-file", "edom.db"],
        Some(input.as_bytes()),
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let out = run(dir.path(), "prune_k_mixture_model", &["edom.db"], None);
    assert_eq!(out.status.code(), Some(33), "EDOM");
    assert_eq!(
        text(&out.stderr),
        "*some wrong possibility is encountered:-nan.\n\
         k:3 N:1 WC:10 n_0:1 n_1:0\n"
    );
}

#[test]
fn estimate_with_no_models_divides_zero_by_zero() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), "estimate_k_mixture_model", &[], None);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    let stdout = text(&out.stdout);
    assert_eq!(stdout.lines().count(), 1);
    let value = stdout
        .strip_prefix("average lambda:")
        .and_then(|line| line.strip_suffix('\n'))
        .expect("the pin's average line");
    // `0.0 / 0` is NaN; the C library's `%f` spells its sign per host.
    assert!(value.ends_with("nan"), "{value:?}");
}

#[test]
fn estimate_reports_a_lambda_line_for_every_deleted_context() {
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
    // Every token in the model is also a deleted context, so each row with
    // a nonzero array-header WC prints `token:%d lambda:%f` (`estimate:141`).
    let out = run(
        dir.path(),
        "estimate_k_mixture_model",
        &[
            "--bigram-file",
            "model.db",
            "--deleted-bigram-file",
            "model.db",
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(out.stderr.is_empty());
    let stdout = text(&out.stdout);
    let mut tokens = Vec::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("token:") {
            let (token, lambda) = rest.split_once(" lambda:").unwrap();
            let token: u32 = token.parse().unwrap();
            let lambda: f64 = lambda.parse().unwrap();
            assert!((0.0..=1.0).contains(&lambda), "{line}");
            tokens.push(token);
        } else {
            assert!(line.starts_with("average lambda:"), "{line}");
        }
    }
    tokens.sort_unstable();
    assert_eq!(
        tokens,
        [1, 16779429, 16786821, 16802309, 16817937],
        "header-only deleted rows (m_WC 0) are not scored"
    );
}

#[test]
fn an_absent_export_target_is_open_failed_and_enoent() {
    let dir = scratch();
    for args in [&["--k-mixture-model-file", "absent.db"][..], &[][..]] {
        let out = run(dir.path(), "export_k_mixture_model", args, None);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let expected = match args.len() {
            0 => "open (null) failed.\n",
            _ => "open absent.db failed.\n",
        };
        assert_eq!(text(&out.stderr), expected, "{args:?}");
    }
}

#[test]
fn validate_of_an_absent_container_is_enodata() {
    let dir = scratch();
    let out = run(dir.path(), "validate_k_mixture_model", &["absent.db"], None);
    assert_eq!(out.status.code(), Some(61));
    assert_eq!(
        text(&out.stderr),
        "no magic header in k mixture model.\nk mixture model validation failed.\n"
    );
}

#[test]
fn gen_reports_a_missing_document_with_errno() {
    let dir = scratch();
    let out = run(
        dir.path(),
        "gen_k_mixture_model",
        &["--k-mixture-model-file", "model.db", "absent.txt"],
        None,
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        text(&out.stderr),
        "can't open file: absent.txt.\nerror:No such file or directory.\n"
    );
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
    assert_eq!(out.status.code(), Some(33));
    assert_eq!(
        text(&out.stderr),
        "*some wrong possibility is encountered:-nan.\n\
         k:3 N:1 WC:10 n_0:1 n_1:0\n"
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
    assert!(out.stdout.is_empty());

    // No result file: the NULL attach aborts in `merge_magic_header`.
    let out = run(
        dir.path(),
        "merge_k_mixture_model",
        &["k_mixture_model.db"],
        None,
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(out.status.signal(), Some(6), "SIGABRT");
    }
    assert!(!dir.path().join("k_mixture_model_ngram.db").exists());
}

/// The captured pin-built utilities, where the environment names them. A
/// selected test fails on a missing input rather than skipping.
#[test]
#[ignore = "needs the pin-built KMM utilities; run with --include-ignored"]
fn rust_kmm_tools_match_the_pin_built_ones() {
    let pin_dir = std::env::var_os("PINYIN_KMM_TOOLS")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| {
            panic!("missing input: set PINYIN_KMM_TOOLS to the pin's utils/training")
        });
    for name in [
        "validate_k_mixture_model",
        "merge_k_mixture_model",
        "prune_k_mixture_model",
        "estimate_k_mixture_model",
        "import_k_mixture_model",
        "export_k_mixture_model",
        "gen_k_mixture_model",
    ] {
        assert!(pin_dir.join(name).is_file(), "missing pin-built {name}");
    }
    let dir = scratch();
    let pin = |name: &str, args: &[&str], stdin: Option<&[u8]>| -> Output {
        let mut child = Command::new(pin_dir.join(name))
            .current_dir(dir.path())
            .env("LC_ALL", "C")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(input) = stdin {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(input).unwrap();
        }
        child.wait_with_output().unwrap()
    };
    for (name, args, stdin) in [
        (
            "validate_k_mixture_model",
            &["k_mixture_model.db"][..],
            None,
        ),
        (
            "export_k_mixture_model",
            &["--k-mixture-model-file", "k_mixture_model.db"][..],
            None,
        ),
        (
            "estimate_k_mixture_model",
            &[
                "--bigram-file",
                "k_mixture_model.db",
                "--deleted-bigram-file",
                "k_mixture_model.db",
            ][..],
            None,
        ),
        (
            "prune_k_mixture_model",
            &["k_mixture_model_edom.db"][..],
            None,
        ),
        (
            "import_k_mixture_model",
            &[][..],
            Some(sample_text().as_bytes()),
        ),
    ] {
        let theirs = pin(name, args, stdin);
        let ours = run(dir.path(), name, args, stdin);
        assert_eq!(ours.status.code(), theirs.status.code(), "{name} {args:?}");
        assert_eq!(ours.stdout, theirs.stdout, "{name} {args:?}");
        assert_eq!(ours.stderr, theirs.stderr, "{name} {args:?}");
    }
}

fn sample_text() -> String {
    "\\data model \"k mixture model\" count 3 N 1 total_freq 3\n\
     \\1-gram\n\
     \\item 1 <start> count 2 freq 0\n\
     \\item 16817937 中国 count 1 freq 1\n\
     \\item 16802309 你好 count 0 freq 2\n\
     \\end\n"
        .to_owned()
}
