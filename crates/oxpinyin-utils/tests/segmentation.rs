//! The pin-named segmentation utilities over the committed W3 data.
//!
//! Every test runs the built binary (`CARGO_BIN_EXE_*`) in a private copy of
//! `fixtures/w3/<backend>` — the committed mini system data directory — so
//! the relative `table.conf`, `phrase_index.bin` and chunk names are the
//! ones the pin opens. Expectations come from the pin's source
//! (`utils/segment/*.cpp` at 074a2219) and its committed W9 goldens, not
//! from a live run; the ignored `*_matches_live` tests are the opt-in
//! cross-check against a pin-built binary.

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

fn spseg(cwd: &Path, args: &[&str], stdin: &[u8]) -> Output {
    run(env!("CARGO_BIN_EXE_spseg"), cwd, args, stdin)
}

fn mergeseq(cwd: &Path, args: &[&str], stdin: &[u8]) -> Output {
    run(env!("CARGO_BIN_EXE_mergeseq"), cwd, args, stdin)
}

fn assert_streams(output: &Output, stdout: &[u8], stderr: &[u8], status: i32) {
    assert_eq!(output.status.code(), Some(status), "exit status");
    assert_eq!(output.stdout, stdout, "stdout");
    assert_eq!(output.stderr, stderr, "stderr");
}

#[test]
fn empty_input_is_the_file_tail_null_token() {
    let (_directory, data) = data_dir();
    for output in [
        ngseg(&data, &[], b""),
        spseg(&data, &[], b""),
        mergeseq(&data, &[], b""),
    ] {
        // `ngseg.cpp:258`, `spseg.cpp:341`: the trailing `%d \n`; `mergeseq`
        // reaches the same bytes through its end-of-input `feed_line("0 ")`.
        assert_streams(&output, b"0 \n", b"", 0);
    }
}

#[test]
fn empty_lines_and_the_extra_enter_flag() {
    let (_directory, data) = data_dir();
    // One empty line is `%d \n` (`ngseg.cpp:199-202`) plus the file tail.
    assert_streams(&ngseg(&data, &[], b"\n"), b"0 \n0 \n", b"", 0);
    // The empty-line branch `continue`s before the flag (`:199-204`), so
    // the flag adds nothing to a blank line.
    assert_streams(
        &ngseg(&data, &["--generate-extra-enter"], b"\n"),
        b"0 \n0 \n",
        b"",
        0,
    );
    // A processed line does reach it (`:249-251`).
    assert_streams(
        &ngseg(&data, &["--generate-extra-enter"], "中\n".as_bytes()),
        "16786821 中\n0 \n0 \n".as_bytes(),
        b"",
        0,
    );
    assert_streams(
        &spseg(&data, &["--generate-extra-enter"], "中\n".as_bytes()),
        "16786821 中\n0 \n0 \n".as_bytes(),
        b"",
        0,
    );
    // `spseg.cpp:281-285`: the empty-line null token.
    assert_streams(&spseg(&data, &[], b"\n"), b"0 \n0 \n", b"", 0);
}

#[test]
fn invalid_utf8_is_a_null_token_and_the_pins_stderr_line() {
    let (_directory, data) = data_dir();
    // `len != num_of_chars`: the line on stderr, `%d \n` on output, then the
    // next line and the file tail (`ngseg.cpp:189-197`, `spseg.cpp:270-278`).
    for output in [ngseg(&data, &[], b"\xff\n"), spseg(&data, &[], b"\xff\n")] {
        assert_streams(
            &output,
            b"0 \n0 \n",
            b"non-ucs4 characters encountered:\xff.\n",
            0,
        );
    }
    // The extra-enter flag is not reached after the `continue`.
    let output = ngseg(&data, &["--generate-extra-enter"], b"\xff\n");
    assert_streams(
        &output,
        b"0 \n0 \n",
        b"non-ucs4 characters encountered:\xff.\n",
        0,
    );
}

#[test]
fn ascii_is_an_unknown_run() {
    let (_directory, data) = data_dir();
    // `deal_with_unknown`: `%d %s\n` with `null_token` (`ngseg.cpp:96-103`).
    assert_streams(&ngseg(&data, &[], b"abc\n"), b"0 abc\n0 \n", b"", 0);
    assert_streams(&spseg(&data, &[], b"abc\n"), b"0 abc\n0 \n", b"", 0);
}

#[test]
fn a_single_character_phrase_uses_its_stored_token() {
    let (_directory, data) = data_dir();
    // 中 is phrase token 16786821 in the committed W3 table (the token the
    // committed `mergeseq-input.txt` names); a one-character segmentable run
    // has the one path `中`.
    assert_streams(
        &ngseg(&data, &[], "中\n".as_bytes()),
        "16786821 中\n0 \n".as_bytes(),
        b"",
        0,
    );
}

#[test]
fn spseg_matches_the_committed_w3_golden() {
    let (_directory, data) = data_dir();
    let input = repo_root().join("fixtures/w9/segmenter-han.txt");
    let golden = std::fs::read(repo_root().join("fixtures/w9/segmenter-spseg-w3.txt"))
        .expect("committed spseg golden");
    let output = spseg(&data, &[input.to_str().expect("utf-8 path")], b"");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, golden, "spseg stdout");
    assert_eq!(output.stderr, b"");
}

#[test]
fn mergeseq_matches_the_committed_w3_golden() {
    let (_directory, data) = data_dir();
    let input = repo_root().join("fixtures/w9/mergeseq-input.txt");
    let golden = std::fs::read(repo_root().join("fixtures/w9/segmenter-mergeseq-w3.txt"))
        .expect("committed mergeseq golden");
    let output = mergeseq(&data, &[input.to_str().expect("utf-8 path")], b"");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, golden, "mergeseq stdout");
    assert_eq!(output.stderr, b"");
}

#[test]
fn help_is_glibs_and_exits_zero() {
    let (_directory, data) = data_dir();
    for (output, parameter) in [
        (ngseg(&data, &["--help"], b""), "- n-gram segment"),
        (spseg(&data, &["--help"], b""), "- shortest path segment"),
        (mergeseq(&data, &["--help"], b""), "- merge word sequence"),
    ] {
        assert_eq!(output.status.code(), Some(0), "help exit status");
        assert_eq!(output.stderr, b"");
        let text = String::from_utf8(output.stdout).expect("help is UTF-8");
        assert!(text.contains("Usage:\n  "), "{text}");
        assert!(text.contains(parameter), "{text}");
        assert!(text.contains("--outputfile"), "{text}");
    }
}

#[test]
fn a_bad_option_prints_glibs_line_and_exits_einval() {
    let (_directory, data) = data_dir();
    let output = ngseg(&data, &["--bogus"], b"");
    assert_eq!(output.status.code(), Some(22), "EINVAL");
    assert_eq!(output.stderr, b"");
    let text = String::from_utf8(output.stdout).expect("g_print is UTF-8");
    assert!(text.starts_with("option parsing failed:"), "{text}");
}

#[test]
fn a_missing_input_reports_the_open_and_exits_einval() {
    let (directory, data) = data_dir();
    let output = ngseg(&data, &["absent.txt"], b"");
    // `open file failed: No such file or directory`
    assert_eq!(output.status.code(), Some(22), "EINVAL");
    // The input is opened before `table.conf` (`ngseg.cpp:136-151`), so the
    // working directory needs no data file for this path.
    let absent = directory.path().join("empty");
    std::fs::create_dir(&absent).expect("empty directory");
    let output = ngseg(&absent, &["absent.txt"], b"");
    assert_eq!(output.status.code(), Some(22), "EINVAL");
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("open file failed:"));
}

#[test]
fn a_missing_table_conf_prints_the_readers_line_and_the_utilitys() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let output = ngseg(directory.path(), &[], b"");
    assert_eq!(output.status.code(), Some(2), "ENOENT");
    assert_eq!(
        output.stderr,
        b"open table.conf failed.\nload table.conf failed.\n"
    );
    assert_eq!(output.stdout, b"");
}

#[test]
fn a_missing_system_chunk_names_the_column_and_exits_enoent() {
    let (_directory, data) = data_dir();
    let conf = std::fs::read_to_string(data.join("table.conf")).expect("table.conf");
    std::fs::write(
        data.join("table.conf"),
        conf.replacen("gb_char.bin", "absent.bin", 1),
    )
    .expect("write table.conf");
    let output = ngseg(&data, &[], b"");
    assert_eq!(output.status.code(), Some(2), "ENOENT");
    assert_eq!(output.stderr, b"load absent.bin failed!\n");
    assert_eq!(output.stdout, b"");
}

#[test]
fn too_many_arguments_opens_the_output_then_exits_einval() {
    let (_directory, data) = data_dir();
    let output = ngseg(&data, &["-o", "extra.out", "a", "b"], b"");
    assert_eq!(output.status.code(), Some(22), "EINVAL");
    assert_eq!(output.stderr, b"too many arguments.\n");
    // The output is opened before the argument-count check (`:123-134`).
    assert_eq!(
        std::fs::read(data.join("extra.out")).expect("created output"),
        b""
    );
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
        b"0 \n0 \n"
    );
}

// ── opt-in live cross-checks against pin-built binaries ─────────────────

fn locate_pin(variable: &str) -> PathBuf {
    let path = std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("missing input: set {variable} to the pin-built binary"));
    assert!(
        path.is_file(),
        "{variable} is not a file: {}",
        path.display()
    );
    path
}

fn locate_data(variable: &str) -> PathBuf {
    let path = std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("missing input: set {variable} to the pin data directory"));
    assert!(
        path.join("table.conf").is_file(),
        "{variable} has no table.conf: {}",
        path.display()
    );
    path
}

fn live_compare(pin: PathBuf, subject: &str, data: &Path, input: &Path) {
    let pin_output = Command::new(pin)
        .current_dir(data)
        .arg(input)
        .output()
        .expect("run the pin-built utility");
    let subject_output = Command::new(subject)
        .current_dir(data)
        .arg(input)
        .output()
        .expect("run the built utility");
    assert_eq!(subject_output.status, pin_output.status, "exit status");
    assert_eq!(subject_output.stdout, pin_output.stdout, "stdout");
    assert_eq!(subject_output.stderr, pin_output.stderr, "stderr");
}

#[test]
#[ignore = "needs the pin-built ngseg and its data (PINYIN_NGSEG, PINYIN_NGSEG_DATA); run with --include-ignored"]
fn rust_ngseg_matches_live() {
    let pin = locate_pin("PINYIN_NGSEG");
    let data = locate_data("PINYIN_NGSEG_DATA");
    live_compare(
        pin,
        env!("CARGO_BIN_EXE_ngseg"),
        &data,
        &repo_root().join("fixtures/w9/segmenter-han.txt"),
    );
}

#[test]
#[ignore = "needs the pin-built spseg and its data (PINYIN_SPSEG, PINYIN_NGSEG_DATA); run with --include-ignored"]
fn rust_spseg_matches_live() {
    let pin = locate_pin("PINYIN_SPSEG");
    let data = locate_data("PINYIN_NGSEG_DATA");
    live_compare(
        pin,
        env!("CARGO_BIN_EXE_spseg"),
        &data,
        &repo_root().join("fixtures/w9/segmenter-han.txt"),
    );
}

#[test]
#[ignore = "needs the pin-built mergeseq and its data (PINYIN_MERGESEQ, PINYIN_NGSEG_DATA); run with --include-ignored"]
fn rust_mergeseq_matches_live() {
    let pin = locate_pin("PINYIN_MERGESEQ");
    let data = locate_data("PINYIN_NGSEG_DATA");
    live_compare(
        pin,
        env!("CARGO_BIN_EXE_mergeseq"),
        &data,
        &repo_root().join("fixtures/w9/segmenter-ngseg.txt"),
    );
}
