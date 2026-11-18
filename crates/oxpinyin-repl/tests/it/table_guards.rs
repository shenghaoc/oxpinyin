//! Approved class (b) outcomes preserve earlier product results in the REPL.
use crate::common::snapshot_inputs;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

fn run(mode: &str, input: &[u8]) -> std::process::Output {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("tests/storage");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir(root.path().join("data")).unwrap();
    fs::write(root.path().join("data/table.conf"), b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\ndefault GB_DICTIONARY gb.table gb_char.bin gb_char.dbin SYSTEM_FILE\n").unwrap();
    let table = "ni3 你 16777217 100\n";
    fs::write(root.path().join("data/gb.table"), table).unwrap();
    let before = snapshot_inputs(root.path());
    let mut child = Command::new(env!("CARGO_BIN_EXE_oxpinyin-repl"))
        .arg(mode)
        .arg(root.path().join("data"))
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let result = child.wait_with_output().unwrap();
    let after = snapshot_inputs(root.path());
    assert_eq!(after.files, before.files);
    assert_eq!(after.dirs, before.dirs);
    result
}

/// Policy row 115 covers the consumed failed conversion at
/// 074a2219 tests/storage/test_phrase_table.cpp:45-59.
#[test]
fn invalid_phrase_query_keeps_the_previous_result() {
    let newline = oxpinyin::input::TEXT_NEWLINE;
    let result = run("phrase_table", b"\xe4\xbd\xa0\n\xff\n");
    assert_eq!(result.status.code(), Some(22));
    assert!(result.stderr.is_empty());
    assert_eq!(
        result.stdout,
        format!("token:16777217\t{newline}").as_bytes()
    );
}

/// Policy row 116 covers the whole-key search before any continuation output
/// at 074a2219 tests/storage/test_chewing_table.cpp:71-102.
#[test]
fn overlong_chewing_query_keeps_the_previous_result() {
    let newline = oxpinyin::input::TEXT_NEWLINE;
    let input = format!("ni3\n{}\n", ["ni3"; 17].join("'"));
    let result = run("chewing_table", input.as_bytes());
    assert_eq!(result.status.code(), Some(22));
    assert!(result.stderr.is_empty());
    let stdout = format!(
        "range items number:1{newline}start:16777217\tend:16777218{newline}你\tni3'\x08\t100\t{newline}"
    );
    assert_eq!(result.stdout, stdout.as_bytes());
}
