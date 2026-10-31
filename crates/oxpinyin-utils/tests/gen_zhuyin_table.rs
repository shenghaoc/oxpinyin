//! Source-derived integration checks; pin citations in gen_zhuyin_table.
use std::path::Path;
use std::process::{Command, Output};

// Each test builds its expected output with the host's newline: the
// utility writes stdout, stderr and its table through the C runtime's
// text-mode streams, which end lines with CRLF on Windows and LF elsewhere
// (docs/findings/external-io-boundary.md, "Native text-mode line endings").

fn run(path: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gen_zhuyin_table"))
        .current_dir(path)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn zhuyin_rows_use_tones_duplicates_and_last_unterminated_line() {
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("in.txt"),
        "你好 10 ㄋㄧˇ ㄏㄠˇ\n你 5 ㄋㄧˇ\n你好 4 ㄋㄧˇ'ㄏㄠˇ\n好 2 ㄏㄠˋ\n你 9\n呢 1 ㄋㄜ",
    )
    .unwrap();
    let out = run(dir.path(), &["-t", "3", "in.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, format!("nnodes: 4{newline}").as_bytes());
    assert_eq!(
        out.stderr,
        "Duplicate item: phrase:你好\tpinyin:ㄋㄧˇ'ㄏㄠˇ\tfreq:4\nwrong line format:你 9\n"
            .replace('\n', newline)
            .as_bytes()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("temp.out")).unwrap(),
        "ㄏㄠˋ\t好\t50331649\t3\nㄋㄜ\t呢\t50331650\t3\nㄋㄧˇ\t你\t50331651\t5\nㄋㄧˇ'ㄏㄠˇ\t你好\t50331652\t14\n"
            .replace('\n', newline)
    );
}

#[test]
fn malformed_input_refuses_normally_at_its_source_stage() {
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    for (input, diagnostic, output_exists, stdout, rows) in [
        (
            b"one-field\n".to_vec(),
            [
                &b"gen_zhuyin_table: missing frequency: one-field"[..],
                newline.as_bytes(),
            ]
            .concat(),
            false,
            "",
            "",
        ),
        (
            [
                "你 5 ㄋㄧ\n".as_bytes(),
                b"\xff\xff 1 ",
                "ㄚ'ㄚ\n".as_bytes(),
            ]
            .concat(),
            [
                &b"gen_zhuyin_table: invalid UTF-8 phrase: \xff\xff"[..],
                newline.as_bytes(),
            ]
            .concat(),
            true,
            "nnodes: 2\n",
            "ㄋㄧ\t你\t1\t5\n",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("in.txt"), input).unwrap();
        let out = run(dir.path(), &["in.txt"]);
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(out.stderr, diagnostic);
        assert_eq!(out.stdout, stdout.replace('\n', newline).as_bytes());
        let output = dir.path().join("temp.out");
        assert_eq!(output.exists(), output_exists);
        if output_exists {
            assert_eq!(
                std::fs::read_to_string(output).unwrap(),
                rows.replace('\n', newline)
            );
        }
    }
}

#[test]
fn empty_phrase_is_a_glib_precondition_diagnostic_and_not_a_crash() {
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("in.txt"), " 1 \n你 5 ㄋㄧ\n").unwrap();
    let out = run(dir.path(), &["in.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, format!("nnodes: 2{newline}").as_bytes());
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("g_array_append_vals: assertion 'array' failed")
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("temp.out")).unwrap(),
        format!("ㄋㄧ\t你\t1\t5{newline}")
    );
}
