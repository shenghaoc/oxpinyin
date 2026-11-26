use super::*;

/// Set in the child: run the helper instead of the test body.
const STORE_ERROR_CHILD: &str = "OXPINYIN_INPUT_STORE_ERROR_CHILD";

/// 074a2219 tests/test_pinyin.cpp:47-48 writes and flushes these bytes.
#[test]
fn prefix_prompt_writes_the_pins_exact_bytes() {
    const CHILD: &str = "OXPINYIN_PREFIX_PROMPT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        pin_tests::prefix_prompt();
        std::process::exit(0);
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "input::tests::prefix_prompt_writes_the_pins_exact_bytes",
            "--quiet",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        output.stdout.strip_prefix(b"\nrunning 1 test\n").unwrap(),
        b"prefix:"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn a_store_error_is_reported_on_one_line() {
    let line = store_error_line(&"unsupported Berkeley DB 6.2: surveyed against 5.3\nsecond");
    let line = line.to_str().expect("UTF-8");
    assert!(line.contains("unsupported Berkeley DB 6.2"), "{line}");
    assert!(!line.contains('\n'), "{line:?}");
}

#[test]
fn exit_on_store_error_prints_one_line_and_exits_with_status_1() {
    if std::env::var_os(STORE_ERROR_CHILD).is_some() {
        exit_on_store_error(&"unsupported Berkeley DB 6.2");
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "input::tests::exit_on_store_error_prints_one_line_and_exits_with_status_1",
            "--test-threads=1",
        ])
        .env(STORE_ERROR_CHILD, "1")
        .output()
        .expect("child test");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unsupported Berkeley DB 6.2"), "{stderr}");
    assert_eq!(stderr.matches('\n').count(), 1, "{stderr:?}");
}

#[test]
fn getline_reads_the_final_line_once_and_clears_on_eof() {
    let mut input = LineReader::from_bytes(b"a\nb\0c\n\nlast");
    let mut lines = Vec::new();
    while input.getline() {
        lines.push((input.line().to_vec(), input.c_str().to_owned()));
    }
    assert_eq!(
        lines,
        [
            (b"a".to_vec(), c"a".to_owned()),
            (b"b\0c".to_vec(), c"b".to_owned()),
            (Vec::new(), c"".to_owned()),
            (b"last".to_vec(), c"last".to_owned()),
        ]
    );
    // POLICY-ACCEPTED: a failed read clears the previous record.
    assert!(input.line().is_empty());
    assert!(!input.getline());
    assert!(input.line().is_empty());
    let mut input = LineReader::from_bytes(b"one\n");
    assert!(input.getline());
    assert!(!input.getline());
    assert_eq!(input.line(), b"");
}

#[test]
fn a_file_opens_and_a_missing_one_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("in.txt");
    std::fs::write(&path, b"x\n").unwrap();
    let mut input = LineReader::open(&oxpinyin::input::c_path(&path).unwrap());
    assert!(input.is_open() && input.getline());
    assert_eq!(input.line(), b"x");
    let missing = LineReader::open(&oxpinyin::input::c_path(&dir.path().join("absent")).unwrap());
    assert!(!missing.is_open());
}

// Windows only: the wide-character (UTF-16) file API opens the UTF-8 C names.
#[cfg(windows)]
#[test]
fn windows_tools_open_cjk_paths_and_utf8_names() {
    use std::os::windows::ffi::OsStringExt as _;
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("目录");
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("词表.txt");
    let output = OutputStream::open_write(&path).unwrap();
    drop(output);
    std::fs::write(&path, b"x\n").unwrap();
    let name = CString::new(path.to_str().unwrap()).unwrap();
    assert!(gen_pinyin_table::TableInput::open(&name).is_some());
    assert!(storage::TableFile::open(&path).is_some());
    assert!(LineReader::open(&name).getline());
    assert!(storage::save_chunk(&dir.join("词库.bin"), 1, 0, b"x"));
    // A Rust path can contain a lone surrogate. No UTF-8 adapter may
    // replace it before the wide open or the Rust file open.
    let raw = dir.join(OsString::from_wide(&[0xd800, 0x8bcd]));
    drop(OutputStream::open_write(&raw).unwrap());
    assert!(LineReader::open(&raw).is_open());
    assert!(storage::save_chunk(&raw, 0, 0, b""));
}

fn entries() -> [OptionEntry; 3] {
    [
        OptionEntry {
            long_name: c"outputfile",
            short_name: b'o',
            flags: 0,
            arg: OptionArg::Filename(None),
            description: Some(c"output"),
            arg_description: Some(c"filename"),
        },
        OptionEntry {
            long_name: c"generate-extra-enter",
            short_name: 0,
            flags: 0,
            arg: OptionArg::Flag(false),
            description: Some(c"generate "),
            arg_description: None,
        },
        OptionEntry {
            long_name: c"CDF",
            short_name: 0,
            flags: 0,
            arg: OptionArg::Double(0.99),
            description: None,
            arg_description: None,
        },
    ]
}

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[test]
fn the_option_sequence_fills_the_entries_and_keeps_the_rest() {
    let parsed = parse_options(
        c"- n-gram segment",
        &entries(),
        os(&["ngseg", "-o", "out.txt", "--generate-extra-enter", "in.txt"]),
    )
    .unwrap();
    assert_eq!(
        parsed.values,
        [
            OptionValue::Filename(Some(c"out.txt".to_owned())),
            OptionValue::Flag(true),
            OptionValue::Double(0.99),
        ]
    );
    assert_eq!(parsed.args, [c"ngseg".to_owned(), c"in.txt".to_owned()]);
}

#[test]
fn an_unknown_option_fails_the_parse() {
    let parsed = parse_options(c"- n-gram segment", &entries(), os(&["ngseg", "--bogus"]));
    assert_eq!(parsed, Err(OptionsFailed));
}

#[test]
fn numeric_fields_are_the_c_librarys() {
    assert_eq!(atoi(c"12abc"), 12);
    assert_eq!(atoi(c"abc"), 0);
    assert_eq!(atol(c" -7x"), -7);
}

#[test]
fn text_calls_keep_their_format_contracts() {
    assert_eq!(
        strsplit_set(c"1 2\t3 4", c" \t", 2),
        [c"1".to_owned(), c"2\t3 4".to_owned()]
    );
    assert_eq!(utf8_strlen(c"你好"), 2);
    // POLICY-ACCEPTED: the malformed sequence leaves subsequent bytes to count.
    assert_eq!(utf8_strlen(c"\xe4 abc"), 5);
    assert_eq!(utf8_to_ucs4(c"你好"), (Some(vec![0x4F60, 0x597D]), 2));
    assert_eq!(utf8_to_ucs4(c"a\xff").0, None);
    assert_eq!(
        ucs4_to_utf8(&[0x4F60, 0x597D]),
        Some("你好".as_bytes().to_vec())
    );
    // g_ucs4_to_utf8 refuses a word only when `str[i] >= 0x80000000`
    // (GLib 2.84.4 glib/gutf8.c:1039; 2.90.0 :1040, the same test). A
    // word below 0x200000 takes g_unichar_to_utf8's four-byte branch
    // (2.84.4 :569-572, lead 0xf0): 0x110000 is 0xf0|0x04, 0x80|0x10,
    // 0x80|0x00, 0x80|0x00.
    assert_eq!(
        ucs4_to_utf8(&[0x11_0000]),
        Some(vec![0xF4, 0x90, 0x80, 0x80])
    );
    assert_eq!(ucs4_to_utf8(&[0x8000_0000]), None);
    assert!(unichar_isspace(0x3000));
    assert!(!unichar_isgraph(0x200B));
    assert!(unichar_isgraph(u32::from('a')));
}

#[test]
fn invalid_utf8_in_a_tool_path_follows_the_platform() {
    let bytes = b"invalid-\xff";
    #[cfg(unix)]
    assert_eq!(
        path_from_bytes_checked(bytes),
        Some(oxpinyin::input::path_from_bytes(bytes))
    );
    #[cfg(not(unix))]
    assert_eq!(path_from_bytes_checked(bytes), None);
}
