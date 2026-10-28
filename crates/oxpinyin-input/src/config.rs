//! `SystemTableInfo2::load`, `UserTableInfo::load` and `UserTableInfo::save`
//! (`src/storage/table_info.cpp:194-397` at `074a2219`): their `setlocale`,
//! `fopen`, `fscanf`, `feof`, `atoi`, `fprintf` and `fclose` calls, in the
//! pin's order and with the pin's return checks left to the caller.
//!
//! A reader opens with `setlocale(LC_NUMERIC, "C")` and `fopen`, then runs
//! one directive per call. [`SystemTableInfoReader::finish`] and
//! [`UserTableInfoReader::finish`] are the success path's `fclose` and
//! `setlocale(LC_NUMERIC, locale)`. Dropping a reader instead is an early
//! return: `fclose` alone, the locale left at `"C"` as the pin leaves it
//! (policy row 39).

use std::ffi::{CStr, c_int};

use crate::native::{self, SavedNumeric, Stream, WORD_SIZE};

/// What one `fscanf` call answered: `1`, `0` or `EOF`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scanned<T> {
    /// The conversion was assigned.
    Assigned(T),
    /// A directive did not match before any conversion (`0`).
    Unassigned,
    /// Input ended or failed before any conversion (`EOF`).
    Eof,
}

impl<T> Scanned<T> {
    fn from_count(count: c_int, value: T) -> Self {
        match count {
            1 => Self::Assigned(value),
            count if count < 0 => Self::Eof,
            _ => Self::Unassigned,
        }
    }

    /// The assigned value, if any.
    #[must_use]
    pub fn assigned(self) -> Option<T> {
        match self {
            Self::Assigned(value) => Some(value),
            Self::Unassigned | Self::Eof => None,
        }
    }
}

/// `fopen` answered NULL. The pin's diagnostic for it has been written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenFailed;

/// A `char str[256]` buffer as `strcmp` reads it: `None` until a `%255s`
/// has written it, which is when the pin's buffer is still uninitialised.
#[derive(Clone)]
struct Word {
    bytes: [u8; WORD_SIZE],
    written: bool,
}

impl Word {
    const fn new() -> Self {
        Self {
            bytes: [0; WORD_SIZE],
            written: false,
        }
    }

    fn as_c_str(&self) -> Option<&CStr> {
        if !self.written {
            return None;
        }
        CStr::from_bytes_until_nul(&self.bytes).ok()
    }
}

/// `SystemTableInfo2::load`'s stream (`table_info.cpp:197-291`).
pub struct SystemTableInfoReader {
    stream: Stream,
    locale: SavedNumeric,
    /// `char str[256]` (`:227`), shared by the two format directives.
    str: Word,
    /// `tableinfo`, `dictstr`, `tablefile`, `sysfile`, `userfile` and
    /// `filetype` (`:252-253`), which keep their bytes across rows.
    row: [[u8; WORD_SIZE]; 6],
}

impl SystemTableInfoReader {
    /// `setlocale(LC_NUMERIC, "C")`, then `fopen(filename, "r")`
    /// (`:197-203`).
    ///
    /// # Errors
    ///
    /// [`OpenFailed`] after the pin's `open %s failed.\n` line, with the
    /// locale left at `"C"`.
    pub fn open(filename: &CStr) -> Result<Self, OpenFailed> {
        let locale = SavedNumeric::install_c();
        let Some(stream) = Stream::open_read(filename) else {
            native::report_system_open_failed(filename);
            return Err(OpenFailed);
        };
        Ok(Self {
            stream,
            locale,
            str: Word::new(),
            row: [[0; WORD_SIZE]; 6],
        })
    }

    /// `fscanf(input, "binary format version:%d\n", &binver)` (`:208`).
    pub fn binary_format_version(&mut self) -> Scanned<i32> {
        let mut binver: c_int = 0;
        let count = self.stream.scan_binary_format_version(&mut binver);
        Scanned::from_count(count, binver)
    }

    /// `fscanf(input, "model data version:%d\n", &modelver)` (`:214`).
    pub fn model_data_version(&mut self) -> Scanned<i32> {
        let mut modelver: c_int = 0;
        let count = self.stream.scan_model_data_version(&mut modelver);
        Scanned::from_count(count, modelver)
    }

    /// `fscanf(input, "lambda parameter:%f\n", &lambda)` (`:220`).
    pub fn lambda_parameter(&mut self) -> Scanned<f32> {
        let mut lambda: f32 = 0.0;
        let count = self.stream.scan_lambda_parameter(&mut lambda);
        Scanned::from_count(count, lambda)
    }

    /// `fscanf(input, "source table format:%255s\n", str)` (`:228`).
    pub fn source_table_format(&mut self) -> Scanned<()> {
        let count = self.stream.scan_source_table_format(&mut self.str.bytes);
        self.str.written |= count == 1;
        Scanned::from_count(count, ())
    }

    /// `fscanf(input, "database format:%255s\n", str)` (`:232`).
    pub fn database_format(&mut self) -> Scanned<()> {
        let count = self.stream.scan_database_format(&mut self.str.bytes);
        self.str.written |= count == 1;
        Scanned::from_count(count, ())
    }

    /// `str` as the following `strcmp` reads it: the last token either
    /// format directive wrote, or `None` while it holds no value.
    #[must_use]
    pub fn str(&self) -> Option<&CStr> {
        self.str.as_c_str()
    }

    /// `feof(input)`, the row loop's condition (`:254`).
    pub fn eof(&mut self) -> bool {
        self.stream.eof()
    }

    /// The six-word `fscanf` (`:255-257`): the six words when it assigned
    /// all six, `None` for any other count (`:259-260`'s `continue`).
    pub fn table_row(&mut self) -> Option<[&CStr; 6]> {
        if self.stream.scan_table_row(&mut self.row) != 6 {
            return None;
        }
        let mut words = self.row.iter().map(|word| CStr::from_bytes_until_nul(word));
        let mut next = || words.next().and_then(Result::ok);
        Some([next()?, next()?, next()?, next()?, next()?, next()?])
    }

    /// `fclose(input)` then `setlocale(LC_NUMERIC, locale)` (`:289-291`).
    pub fn finish(self) {
        self.stream.close();
        self.locale.restore();
    }
}

/// `UserTableInfo::load`'s stream (`table_info.cpp:328-372`).
pub struct UserTableInfoReader {
    stream: Stream,
    locale: SavedNumeric,
    /// `char str[256]` (`:351`).
    str: Word,
}

impl UserTableInfoReader {
    /// `setlocale(LC_NUMERIC, "C")`, then `fopen(filename, "r")`
    /// (`:328-334`).
    ///
    /// # Errors
    ///
    /// [`OpenFailed`] after the pin's `open %s failed.` line, which has no
    /// newline, with the locale left at `"C"`.
    pub fn open(filename: &CStr) -> Result<Self, OpenFailed> {
        let locale = SavedNumeric::install_c();
        let Some(stream) = Stream::open_read(filename) else {
            native::report_user_open_failed(filename);
            return Err(OpenFailed);
        };
        Ok(Self {
            stream,
            locale,
            str: Word::new(),
        })
    }

    /// `fscanf(input, "binary format version:%d\n", &binver)` (`:338`).
    pub fn binary_format_version(&mut self) -> Scanned<i32> {
        let mut binver: c_int = 0;
        let count = self.stream.scan_binary_format_version(&mut binver);
        Scanned::from_count(count, binver)
    }

    /// `fscanf(input, "model data version:%d\n", &modelver)` (`:344`).
    pub fn model_data_version(&mut self) -> Scanned<i32> {
        let mut modelver: c_int = 0;
        let count = self.stream.scan_model_data_version(&mut modelver);
        Scanned::from_count(count, modelver)
    }

    /// `fscanf(input, "database format:%255s\n", str)` (`:352`).
    pub fn database_format(&mut self) -> Scanned<()> {
        let count = self.stream.scan_database_format(&mut self.str.bytes);
        self.str.written |= count == 1;
        Scanned::from_count(count, ())
    }

    /// `str` as `to_table_database_format_type` reads it (`:353-354`), or
    /// `None` while the buffer holds no value.
    #[must_use]
    pub fn str(&self) -> Option<&CStr> {
        self.str.as_c_str()
    }

    /// `fscanf(input, "open counter:%d\n", &counter)` (`:357`).
    pub fn open_counter(&mut self) -> Scanned<i32> {
        let mut counter: c_int = 0;
        let count = self.stream.scan_open_counter(&mut counter);
        Scanned::from_count(count, counter)
    }

    /// `fclose(input)` then `setlocale(LC_NUMERIC, locale)` (`:370-372`).
    pub fn finish(self) {
        self.stream.close();
        self.locale.restore();
    }
}

/// The four fields `UserTableInfo::save` writes.
#[derive(Clone, Copy, Debug)]
pub struct UserTableInfoFields<'a> {
    /// `m_binary_format_version`.
    pub binary_format_version: i32,
    /// `m_model_data_version`.
    pub model_data_version: i32,
    /// `from_table_database_format_type(m_table_database_format_type)`.
    pub database_format: &'a CStr,
    /// `m_open_counter`.
    pub open_counter: i32,
}

/// `UserTableInfo::save(filename)` (`table_info.cpp:377-397`): the pin's
/// answer, `false` only when `fopen(filename, "w")` failed, after its
/// `write %s failed.\n` line. The writes and the close are not checked, as
/// the pin does not check them.
#[must_use]
pub fn save_user_table_info(filename: &CStr, fields: &UserTableInfoFields<'_>) -> bool {
    let locale = SavedNumeric::install_c();
    let Some(mut output) = Stream::open_write(filename) else {
        native::report_user_write_failed(filename);
        return false;
    };
    output.print_user_table_info(
        fields.binary_format_version,
        fields.model_data_version,
        fields.database_format,
        fields.open_counter,
    );
    output.close();
    locale.restore();
    true
}

/// `atoi(str)` (`table_info.cpp:160`).
#[must_use]
pub fn atoi(str: &CStr) -> i32 {
    native::atoi(str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn file(bytes: &[u8]) -> (tempfile::TempDir, CString) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("table.conf");
        std::fs::write(&path, bytes).expect("write");
        let name = crate::c_path(&path).expect("c path");
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
        assert_eq!(reader.str(), None);
        assert_eq!(reader.source_table_format(), Scanned::Assigned(()));
        assert_eq!(reader.str(), Some(c"pinyin"));
        assert_eq!(reader.database_format(), Scanned::Assigned(()));
        assert_eq!(reader.str(), Some(c"BerkeleyDB"));
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
    fn a_missing_directive_leaves_the_shared_buffer_as_it_was() {
        let (_dir, name) = file(b"source table format:zhuyin\nlambda parameter:1\n");
        let mut reader = SystemTableInfoReader::open(&name).expect("open");
        assert_eq!(reader.source_table_format(), Scanned::Assigned(()));
        assert_eq!(reader.database_format(), Scanned::Unassigned);
        assert_eq!(reader.str(), Some(c"zhuyin"));
    }

    #[test]
    fn words_are_cut_at_255_bytes_and_read_as_c_strings() {
        let mut bytes = b"source table format:".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', 300));
        bytes.extend_from_slice(b"\ndatabase format:Tkrzw\0junk\n");
        let (_dir, name) = file(&bytes);
        let mut reader = SystemTableInfoReader::open(&name).expect("open");
        assert_eq!(reader.source_table_format(), Scanned::Assigned(()));
        assert_eq!(reader.str().map(CStr::count_bytes), Some(255));
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
        assert_eq!(reader.str(), None);
        assert_eq!(reader.open_counter(), Scanned::Eof);
        reader.finish();
    }

    #[test]
    fn a_nul_ends_the_token_strcmp_sees() {
        let (_dir, name) = file(b"database format:BerkeleyDB\0junk\nopen counter:3\n");
        let mut reader = UserTableInfoReader::open(&name).expect("open");
        assert_eq!(reader.database_format(), Scanned::Assigned(()));
        assert_eq!(reader.str(), Some(c"BerkeleyDB"));
        assert_eq!(reader.open_counter(), Scanned::Assigned(3));
    }

    #[test]
    fn opening_a_missing_file_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let name = crate::c_path(&dir.path().join("absent")).expect("c path");
        assert!(SystemTableInfoReader::open(&name).is_err());
        assert!(UserTableInfoReader::open(&name).is_err());
    }

    #[test]
    fn the_writer_prints_the_four_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("user.conf");
        let name = crate::c_path(&path).expect("c path");
        let fields = UserTableInfoFields {
            binary_format_version: -1,
            model_data_version: 14,
            database_format: c"KyotoCabinet",
            open_counter: 3,
        };
        assert!(save_user_table_info(&name, &fields));
        assert_eq!(
            std::fs::read(&path).expect("read"),
            b"binary format version:-1\nmodel data version:14\n\
              database format:KyotoCabinet\nopen counter:3\n"
        );
        let missing = crate::c_path(&dir.path().join("absent/user.conf")).expect("c path");
        assert!(!save_user_table_info(&missing, &fields));
    }

    #[test]
    fn atoi_is_the_host_conversion() {
        assert_eq!(atoi(c"4"), 4);
        assert_eq!(atoi(c" 12abc"), 12);
        assert_eq!(atoi(c"abc"), 0);
        assert_eq!(atoi(c"-1"), -1);
    }
}
