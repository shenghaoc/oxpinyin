//! CLI argument handling for the `oxpinyin-corpus` binary.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn xml_fixture() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures/w9/corpus-xml.xml");
    std::fs::read(path).expect("committed XML fixture")
}

/// Runs the built binary with `args`, feeding `stdin` on its standard input.
fn run(args: &[&str], stdin: Vec<u8>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_oxpinyin-corpus"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn oxpinyin-corpus");
    let mut pipe = child.stdin.take().expect("piped stdin");
    // Feed from a thread so a full stdout pipe cannot deadlock the write.
    let writer = std::thread::spawn(move || pipe.write_all(&stdin));
    let output = child.wait_with_output().expect("wait for oxpinyin-corpus");
    writer
        .join()
        .expect("stdin writer thread")
        .expect("write stdin");
    output
}

#[test]
fn bare_dash_reads_stdin_like_no_argument() {
    let xml = xml_fixture();
    let implicit = run(&["--no-convert"], xml.clone());
    let explicit = run(&["--no-convert", "-"], xml);

    assert!(implicit.status.success(), "no-argument run must succeed");
    assert!(
        explicit.status.success(),
        "bare `-` must read stdin, stderr: {}",
        String::from_utf8_lossy(&explicit.stderr)
    );
    assert!(!explicit.stdout.is_empty());
    assert_eq!(explicit.stdout, implicit.stdout);
    assert_eq!(explicit.stderr, implicit.stderr);
}

#[test]
fn unknown_option_still_rejected() {
    let output = run(&["--no-convert", "--bogus"], Vec::new());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim_end(),
        "unknown option: --bogus"
    );
}

#[test]
fn dash_plus_path_is_too_many_arguments() {
    let output = run(&["--no-convert", "-", "-"], Vec::new());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim_end(),
        "too many arguments"
    );
}

#[test]
fn dash_as_option_value_is_still_consumed_by_the_option() {
    let output = run(&["--pages", "-"], Vec::new());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim_end(),
        "invalid --pages"
    );
}
