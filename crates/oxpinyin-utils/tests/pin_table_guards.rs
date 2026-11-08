//! Approved class (b) outcomes after earlier observable timed output.
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

fn run(binary: &str, input: &[u8]) -> std::process::Output {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("tests/storage");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir(root.path().join("data")).unwrap();
    fs::write(root.path().join("data/table.conf"), b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\ndefault GB_DICTIONARY gb.table gb_char.bin gb_char.dbin SYSTEM_FILE\n").unwrap();
    let table = "ni3 你 16777217 100\n";
    fs::write(root.path().join("data/gb.table"), table).unwrap();
    let mut child = Command::new(binary)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let result = child.wait_with_output().unwrap();
    assert_eq!(
        fs::read(root.path().join("data/gb.table")).unwrap(),
        table.as_bytes()
    );
    result
}

#[test]
fn invalid_phrase_query_keeps_the_previous_timed_result() {
    let newline = oxpinyin_input::TEXT_NEWLINE;
    let result = run(
        env!("CARGO_BIN_EXE_test_phrase_table"),
        b"\xe4\xbd\xa0\n\xff\n",
    );
    assert_eq!(result.status.code(), Some(22));
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stdout.contains(" us for 1000 operations, "));
    assert!(stdout.ends_with(&format!("token:16777217\t{newline}")));
}

#[test]
fn overlong_chewing_query_keeps_the_previous_timed_result() {
    let newline = oxpinyin_input::TEXT_NEWLINE;
    let input = format!("ni3\n{}\n", ["ni3"; 17].join("'"));
    let result = run(env!("CARGO_BIN_EXE_test_chewing_table"), input.as_bytes());
    assert_eq!(result.status.code(), Some(22));
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stdout.contains(" us for 1000 operations, "));
    assert!(stdout.contains(&format!("range items number:1{newline}")));
    assert!(stdout.ends_with(&format!("ni3'\x08\t100\t{newline}")));
}
