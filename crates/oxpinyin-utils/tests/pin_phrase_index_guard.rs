//! Source-only guard for the approved family E invalid-UTF-8 outcome.
//! The pin is never invoked: phrase_index.cpp:541-547 reads unwritten length.
use std::fs;
use std::process::Command;

fn refuse(table: &[u8], reserved: bool) {
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("tests/storage");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir(root.path().join("data")).unwrap();
    let dictionary = if reserved {
        "RESERVED"
    } else {
        "GB_DICTIONARY"
    };
    fs::write(root.path().join("data/table.conf"), format!("binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\ndefault {dictionary} gb.table gb_char.bin gb_char.dbin SYSTEM_FILE\n")).unwrap();
    fs::write(root.path().join("data/gb.table"), table).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_test_phrase_index"))
        .current_dir(&cwd)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(22));
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stdout.starts_with(&format!("pinyin possiblitiy:0.250000{newline}Spent ")));
    assert!(stdout.contains(" us for 100000 operations, "));
    assert!(stdout.ends_with(&format!("{newline}{newline}pinyin poss:0.500000{newline}")));
    assert_eq!(fs::read(root.path().join("data/gb.table")).unwrap(), table);
}

#[test]
fn invalid_text_keeps_the_preceding_output_and_exits_einval() {
    refuse(b"ni3 \xff 16777222 100\n", false);
}

#[test]
fn changed_token_consumes_the_failed_conversion() {
    refuse(
        "ni3 你 16777222 100\n"
            .as_bytes()
            .iter()
            .copied()
            .chain(b"ni2 \xff 16777223 300\n".iter().copied())
            .collect::<Vec<_>>()
            .as_slice(),
        false,
    );
}

#[test]
fn token_zero_consumes_each_failed_conversion() {
    refuse(
        "ni3 你 0 100\n"
            .as_bytes()
            .iter()
            .copied()
            .chain(b"ni2 \xff 0 300\n".iter().copied())
            .collect::<Vec<_>>()
            .as_slice(),
        true,
    );
}
