//! Source-derived checks of `k_mixture_model_to_interpolation` that CI runs
//! without the pin; the registered `tool-diff` cases compare the rest with
//! it. Pin citations are in the utility. The ignored `rust_kmm_matches_pin_*` gate
//! compares the built utility with the pin-built one
//! (`PINYIN_KMM_TO_INTERP`; `tools/oracle/run-differentials.sh`). Run it
//! explicitly with `--include-ignored`; it fails on a missing input.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use oxpinyin_kmm::{DEFAULT_PRUNE_K, GenerateParams, KMixtureModel, export, merge_into, prune};

fn run_binary(binary: &Path, stdin: &[u8]) -> Output {
    let mut child = Command::new(binary)
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

fn run(stdin: &[u8]) -> Output {
    run_binary(
        Path::new(env!("CARGO_BIN_EXE_k_mixture_model_to_interpolation")),
        stdin,
    )
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

// The `oxpinyin-kmm` differential fixture (tests/differential.rs there):
// candidate A, five `甲 乙` sentences then one `丙 丁`; candidate B, three.
const JIAYI: &str = "10 甲\n20 乙\n0 \n";

fn built(doc: &str) -> KMixtureModel {
    let mut model = KMixtureModel::new();
    model.add_document(doc, GenerateParams::default()).unwrap();
    model
}

fn merged() -> KMixtureModel {
    let mut merged = KMixtureModel::new();
    merge_into(
        &mut merged,
        &built(&format!(
            "{JIAYI}{JIAYI}{JIAYI}{JIAYI}{JIAYI}30 丙\n40 丁\n"
        )),
    )
    .unwrap();
    merge_into(
        &mut merged,
        &built(&format!("{JIAYI}{JIAYI}{JIAYI}30 丙\n40 丁\n")),
    )
    .unwrap();
    merged
}

#[test]
fn the_pruned_differential_fixture_projects_to_the_golden() {
    // <start> and zero-freq unigrams drop; the 1-gram count is the KMM
    // freq, the 2-gram count the pair WC. 乙 is W2-only but its header-only
    // row carries freq 8, so it projects.
    let mut pruned = merged();
    prune(&mut pruned, DEFAULT_PRUNE_K, 0.5).unwrap();
    let out = run(export(&pruned).as_bytes());
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stderr(&out), "");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "\\data model interpolation\n\\1-gram\n\\item 10 甲 count 8\n\\item 20 乙 count 8\n\
         \\2-gram\n\\item 1 <start> 10 甲 count 8\n\\item 10 甲 20 乙 count 8\n\\end\n"
    );
}

#[test]
fn an_empty_input_or_a_foreign_header_exits_with_enodata() {
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
        assert!(stderr(&out).ends_with(message), "{input:?}: {out:?}");
        assert_eq!(out.stdout, b"", "{input:?}");
    }
}

/// A failed `check_result(taglib_read(...))` aborts, as does a registered
/// line outside its `switch`; neither writes a message (maintainer ruling
/// 2026-10-31 UTC), and the lines `std::getline` flushed before stay on
/// stdout.
#[cfg(unix)]
#[test]
fn the_pins_assertions_and_abort_abort() {
    for input in [
        format!("{HEADER}\\1-gram\n\\item 10\n"),
        format!("{HEADER}\\1-gram\n\\data model x\n"),
    ] {
        let out = run(input.as_bytes());
        assert_eq!(signal(&out), Some(6), "{out:?}");
        assert_eq!(stderr(&out), "");
        assert_eq!(out.stdout, b"\\data model interpolation\n\\1-gram\n");
    }
}

#[test]
fn undefined_lines_are_refused_normally() {
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
                "k_mixture_model_to_interpolation: {reason}: {line}\n"
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
            b"k_mixture_model_to_interpolation: truncated UTF-8 sequence at the end of the line: \\item 10 \xe7\x94\xb2 freq 1 x \xe4\n"
        ),
        "{out:?}"
    );
}

#[test]
#[ignore = "needs the pin-built k_mixture_model_to_interpolation (PINYIN_KMM_TO_INTERP); run with --include-ignored"]
fn rust_kmm_matches_pin_to_interpolation() {
    let Some(pin) = std::env::var_os("PINYIN_KMM_TO_INTERP")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_file())
    else {
        panic!("missing input for the live to-interpolation differential: set PINYIN_KMM_TO_INTERP")
    };
    let input = export(&merged());
    let theirs = run_binary(&pin, input.as_bytes());
    let ours = run(input.as_bytes());
    assert_eq!(theirs.status.code(), Some(0), "pin to-interpolation failed");
    assert_eq!(ours.status, theirs.status);
    assert_eq!(ours.stdout, theirs.stdout);
    assert_eq!(ours.stderr, theirs.stderr);
    eprintln!("live parity: k_mixture_model_to_interpolation matches the pin");
}
