use super::*;
use std::ffi::CString;

fn file(bytes: &[u8]) -> (tempfile::TempDir, CString) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("table.conf");
    std::fs::write(&path, bytes).expect("write");
    let name = crate::input::c_path(&path).expect("c path");
    (dir, name)
}

#[test]
fn the_system_reader_follows_the_scanner() {
    let (_dir, name) = file(
        b"binary format version:7 model data version:14\nlambda parameter:0x1p-1\n\
              source table format:pinyin\ndatabase format:BerkeleyDB\n\
              default GB_DICTIONARY a b\nc SYSTEM_FILE\naddon 4 x",
    );
    let mut reader = SystemTableInfoReader::open(&name).expect("open");
    assert_eq!(reader.binary_format_version(), Scanned::Assigned(7));
    assert_eq!(reader.model_data_version(), Scanned::Assigned(14));
    assert_eq!(reader.lambda_parameter(), Scanned::Assigned(0.5));
    assert_eq!(reader.source_table_format(), Scanned::Assigned(c"pinyin"));
    assert_eq!(reader.database_format(), Scanned::Assigned(c"BerkeleyDB"));
    assert!(!reader.eof());
    // Six words, across the line break.
    assert_eq!(
        reader.table_row(),
        Some([
            c"default",
            c"GB_DICTIONARY",
            c"a",
            c"b",
            c"c",
            c"SYSTEM_FILE"
        ])
    );
    assert_eq!(reader.table_row(), None);
    assert!(reader.eof());
    reader.finish();
}

#[test]
fn a_missing_directive_does_not_supply_the_previous_word() {
    let (_dir, name) = file(b"source table format:zhuyin\nlambda parameter:1\n");
    let mut reader = SystemTableInfoReader::open(&name).expect("open");
    assert_eq!(reader.source_table_format(), Scanned::Assigned(c"zhuyin"));
    assert_eq!(reader.database_format(), Scanned::Unassigned);
}

#[test]
fn words_are_cut_at_255_bytes_and_read_as_c_strings() {
    let mut bytes = b"source table format:".to_vec();
    bytes.extend(std::iter::repeat_n(b'x', 300));
    bytes.extend_from_slice(b"\ndatabase format:Tkrzw\0junk\n");
    let (_dir, name) = file(&bytes);
    let mut reader = SystemTableInfoReader::open(&name).expect("open");
    assert_eq!(
        reader
            .source_table_format()
            .assigned()
            .map(CStr::count_bytes),
        Some(255)
    );
    // The 45 bytes left over do not match the next directive.
    assert_eq!(reader.database_format(), Scanned::Unassigned);
}

#[test]
fn the_user_reader_reports_end_of_input() {
    let (_dir, name) = file(b"binary format version:7\nmodel data version:14\n");
    let mut reader = UserTableInfoReader::open(&name).expect("open");
    assert_eq!(reader.binary_format_version(), Scanned::Assigned(7));
    assert_eq!(reader.model_data_version(), Scanned::Assigned(14));
    assert_eq!(reader.database_format(), Scanned::Eof);
    assert_eq!(reader.open_counter(), Scanned::Eof);
    reader.finish();
}

#[test]
fn a_nul_ends_the_token_strcmp_sees() {
    let (_dir, name) = file(b"database format:BerkeleyDB\0junk\nopen counter:3\n");
    let mut reader = UserTableInfoReader::open(&name).expect("open");
    assert_eq!(reader.database_format(), Scanned::Assigned(c"BerkeleyDB"));
    assert_eq!(reader.open_counter(), Scanned::Assigned(3));
}

#[test]
fn opening_a_missing_file_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let name = crate::input::c_path(&dir.path().join("absent")).expect("c path");
    assert!(SystemTableInfoReader::open(&name).is_err());
    assert!(UserTableInfoReader::open(&name).is_err());
}

#[test]
fn the_writer_prints_the_four_lines() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("user.conf");
    let name = crate::input::c_path(&path).expect("c path");
    let fields = UserTableInfoFields {
        binary_format_version: -1,
        model_data_version: 14,
        database_format: c"KyotoCabinet",
        open_counter: 3,
    };
    assert!(save_user_table_info(&name, &fields));
    // The pin's `fopen("w")` uses the host C runtime's text mode.
    let newline = crate::input::TEXT_NEWLINE;
    let expected = format!(
        "binary format version:-1{newline}model data version:14{newline}database format:KyotoCabinet{newline}open counter:3{newline}"
    );
    assert_eq!(std::fs::read(&path).expect("read"), expected.as_bytes());
    let mut reader = UserTableInfoReader::open(&name).expect("reopen saved file");
    assert_eq!(reader.binary_format_version(), Scanned::Assigned(-1));
    assert_eq!(reader.model_data_version(), Scanned::Assigned(14));
    assert_eq!(reader.database_format(), Scanned::Assigned(c"KyotoCabinet"));
    assert_eq!(reader.open_counter(), Scanned::Assigned(3));
    reader.finish();
    let missing = crate::input::c_path(&dir.path().join("absent/user.conf")).expect("c path");
    assert!(!save_user_table_info(&missing, &fields));
}

#[test]
fn atoi_is_the_host_conversion() {
    assert_eq!(atoi(c"4"), 4);
    assert_eq!(atoi(c" 12abc"), 12);
    assert_eq!(atoi(c"abc"), 0);
    assert_eq!(atoi(c"-1"), -1);
}

// Windows only: UTF-16 file names with lone surrogates.
#[cfg(windows)]
#[test]
fn windows_config_paths_keep_cjk_and_lone_surrogates() {
    use std::os::windows::ffi::OsStringExt as _;
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("目录");
    std::fs::create_dir(&dir).unwrap();
    let fields = UserTableInfoFields {
        binary_format_version: 7,
        model_data_version: 14,
        database_format: c"Tkrzw",
        open_counter: 0,
    };
    for name in [
        std::ffi::OsString::from("用户.conf"),
        std::ffi::OsString::from_wide(&[0xd800, 0x7528]),
    ] {
        let path = dir.join(name);
        assert!(save_user_table_info(&path, &fields));
        let mut reader = UserTableInfoReader::open(&path).unwrap();
        assert_eq!(reader.binary_format_version(), Scanned::Assigned(7));
        reader.finish();
        assert!(SystemTableInfoReader::open(&path).is_ok());
        crate::input::unlink(&path);
        assert!(!path.exists());
    }
}

mod numeric_locale {
    // Nextest must run each test in its own process to isolate the process locale.
    #![expect(
        unsafe_code,
        reason = "queries and restores the process numeric locale"
    )]

    use std::ffi::{CStr, CString};
    use std::ptr;

    use libc::{LC_NUMERIC, setlocale};

    fn numeric_locale() -> CString {
        // SAFETY: a NULL locale queries LC_NUMERIC. Nextest runs this test in its own
        // process with no other locale-changing threads; copy libc's string before
        // the next setlocale call can invalidate it.
        let name = unsafe { setlocale(LC_NUMERIC, ptr::null()) };
        assert!(!name.is_null(), "LC_NUMERIC query failed");
        // SAFETY: the successful query returns a NUL-terminated libc string
        // and no intervening call or other thread can invalidate it.
        unsafe { CStr::from_ptr(name) }.to_owned()
    }

    struct RestoreNumericLocale(CString);

    impl Drop for RestoreNumericLocale {
        fn drop(&mut self) {
            // SAFETY: the saved name came from a successful LC_NUMERIC query
            // in this process, is NUL-terminated, and remains alive for the
            // call. The only test owns all locale changes, including unwind.
            unsafe { setlocale(LC_NUMERIC, self.0.as_ptr()) };
        }
    }

    /// A failed init still inherits table_info.cpp's LC_NUMERIC reset at the
    /// pin (074a2219). The setup locale is the host's UTF-8 locale.
    #[test]
    fn a_failed_init_leaves_lc_numeric_at_c() {
        let _restore = RestoreNumericLocale(numeric_locale());
        let utf8_locale = if cfg!(windows) { c".UTF-8" } else { c"C.UTF-8" };
        // SAFETY: the literal is NUL-terminated and outlives the call. No
        // other test or thread in this process can change the locale.
        let installed = unsafe { setlocale(LC_NUMERIC, utf8_locale.as_ptr()) };
        assert!(
            !installed.is_null(),
            "missing input: {utf8_locale:?} locale"
        );
        #[cfg(not(windows))]
        assert_eq!(numeric_locale().as_c_str(), utf8_locale);
        // UCRT expands .UTF-8 to the current language and UTF-8 code page.
        #[cfg(windows)]
        assert_ne!(numeric_locale().as_c_str(), c"C");

        assert!(super::SystemTableInfoReader::open(c"table.conf").is_err());
        assert_eq!(numeric_locale().as_c_str(), c"C");
    }
}
