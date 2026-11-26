//! `SystemTableInfo2::load`, `UserTableInfo::load` and `UserTableInfo::save`
//! (`src/storage/table_info.cpp:194-397` at `074a2219`): their `setlocale`,
//! `fopen`, `fscanf`, `feof`, `atoi`, `fprintf` and `fclose` calls, in the
//! pin's order and with the pin's return checks left to the caller.
//!
//! A reader opens with `setlocale(LC_NUMERIC, "C")` and `fopen`, then runs
//! one directive per call. [`SystemTableInfoReader::finish`] and
//! [`UserTableInfoReader::finish`] are the success path's `fclose` and
//! `setlocale(LC_NUMERIC, locale)`. Dropping a reader instead is an early
//! return: `fclose` alone, the locale left at `"C"` as the pin leaves it.

use std::ffi::{CStr, c_int};

use crate::input::native::{self, SavedNumeric, Stream, WORD_SIZE};

/// What one `fscanf` call answered: `1`, `0` or `EOF`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Scanned<T> {
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
    pub(crate) fn assigned(self) -> Option<T> {
        match self {
            Self::Assigned(value) => Some(value),
            Self::Unassigned | Self::Eof => None,
        }
    }
}

/// `fopen` answered NULL. The pin's diagnostic for it has been written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OpenFailed;

fn scanned_word(count: c_int, bytes: &[u8; WORD_SIZE]) -> Scanned<&CStr> {
    match count {
        1 => CStr::from_bytes_until_nul(bytes).map_or(Scanned::Unassigned, Scanned::Assigned),
        count if count < 0 => Scanned::Eof,
        _ => Scanned::Unassigned,
    }
}

/// `SystemTableInfo2::load`'s stream (`table_info.cpp:197-291`).
pub(crate) struct SystemTableInfoReader {
    stream: Stream,
    locale: SavedNumeric,
    word: [u8; WORD_SIZE],
    /// Storage for the current six-word row.
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
    pub(crate) fn open(
        filename: &(impl crate::input::FileName + ?Sized),
    ) -> Result<Self, OpenFailed> {
        let locale = SavedNumeric::install_c();
        let Some(stream) = Stream::open_read(filename) else {
            native::report_system_open_failed(&filename.as_c_name());
            return Err(OpenFailed);
        };
        Ok(Self {
            stream,
            locale,
            word: [0; WORD_SIZE],
            row: [[0; WORD_SIZE]; 6],
        })
    }

    /// `fscanf(input, "binary format version:%d\n", &binver)` (`:208`).
    pub(crate) fn binary_format_version(&mut self) -> Scanned<i32> {
        let mut binver: c_int = 0;
        let count = self.stream.scan_binary_format_version(&mut binver);
        Scanned::from_count(count, binver)
    }

    /// `fscanf(input, "model data version:%d\n", &modelver)` (`:214`).
    pub(crate) fn model_data_version(&mut self) -> Scanned<i32> {
        let mut modelver: c_int = 0;
        let count = self.stream.scan_model_data_version(&mut modelver);
        Scanned::from_count(count, modelver)
    }

    /// `fscanf(input, "lambda parameter:%f\n", &lambda)` (`:220`).
    pub(crate) fn lambda_parameter(&mut self) -> Scanned<f32> {
        let mut lambda: f32 = 0.0;
        let count = self.stream.scan_lambda_parameter(&mut lambda);
        Scanned::from_count(count, lambda)
    }

    /// `fscanf(input, "source table format:%255s\n", str)` (`:228`).
    pub(crate) fn source_table_format(&mut self) -> Scanned<&CStr> {
        let count = self.stream.scan_source_table_format(&mut self.word);
        scanned_word(count, &self.word)
    }

    /// `fscanf(input, "database format:%255s\n", str)` (`:232`).
    pub(crate) fn database_format(&mut self) -> Scanned<&CStr> {
        let count = self.stream.scan_database_format(&mut self.word);
        scanned_word(count, &self.word)
    }

    /// `feof(input)`, the row loop's condition (`:254`).
    pub(crate) fn eof(&mut self) -> bool {
        self.stream.eof()
    }

    /// The six-word `fscanf` (`:255-257`): the six words when it assigned
    /// all six, `None` for any other count (`:259-260`'s `continue`).
    pub(crate) fn table_row(&mut self) -> Option<[&CStr; 6]> {
        if self.stream.scan_table_row(&mut self.row) != 6 {
            return None;
        }
        let mut words = self.row.iter().map(|word| CStr::from_bytes_until_nul(word));
        let mut next = || words.next().and_then(Result::ok);
        Some([next()?, next()?, next()?, next()?, next()?, next()?])
    }

    /// `fclose(input)` then `setlocale(LC_NUMERIC, locale)` (`:289-291`).
    pub(crate) fn finish(self) {
        self.stream.close();
        self.locale.restore();
    }
}

/// `UserTableInfo::load`'s stream (`table_info.cpp:328-372`).
pub(crate) struct UserTableInfoReader {
    stream: Stream,
    locale: SavedNumeric,
    word: [u8; WORD_SIZE],
}

impl UserTableInfoReader {
    /// `setlocale(LC_NUMERIC, "C")`, then `fopen(filename, "r")`
    /// (`:328-334`).
    ///
    /// # Errors
    ///
    /// [`OpenFailed`] after the pin's `open %s failed.` line, which has no
    /// newline, with the locale left at `"C"`.
    pub(crate) fn open(
        filename: &(impl crate::input::FileName + ?Sized),
    ) -> Result<Self, OpenFailed> {
        let locale = SavedNumeric::install_c();
        let Some(stream) = Stream::open_read(filename) else {
            native::report_user_open_failed(&filename.as_c_name());
            return Err(OpenFailed);
        };
        Ok(Self {
            stream,
            locale,
            word: [0; WORD_SIZE],
        })
    }

    /// `fscanf(input, "binary format version:%d\n", &binver)` (`:338`).
    pub(crate) fn binary_format_version(&mut self) -> Scanned<i32> {
        let mut binver: c_int = 0;
        let count = self.stream.scan_binary_format_version(&mut binver);
        Scanned::from_count(count, binver)
    }

    /// `fscanf(input, "model data version:%d\n", &modelver)` (`:344`).
    pub(crate) fn model_data_version(&mut self) -> Scanned<i32> {
        let mut modelver: c_int = 0;
        let count = self.stream.scan_model_data_version(&mut modelver);
        Scanned::from_count(count, modelver)
    }

    /// `fscanf(input, "database format:%255s\n", str)` (`:352`).
    pub(crate) fn database_format(&mut self) -> Scanned<&CStr> {
        let count = self.stream.scan_database_format(&mut self.word);
        scanned_word(count, &self.word)
    }

    /// `fscanf(input, "open counter:%d\n", &counter)` (`:357`).
    pub(crate) fn open_counter(&mut self) -> Scanned<i32> {
        let mut counter: c_int = 0;
        let count = self.stream.scan_open_counter(&mut counter);
        Scanned::from_count(count, counter)
    }

    /// `fclose(input)` then `setlocale(LC_NUMERIC, locale)` (`:370-372`).
    pub(crate) fn finish(self) {
        self.stream.close();
        self.locale.restore();
    }
}

/// The four fields `UserTableInfo::save` writes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct UserTableInfoFields<'a> {
    /// `m_binary_format_version`.
    pub(crate) binary_format_version: i32,
    /// `m_model_data_version`.
    pub(crate) model_data_version: i32,
    /// `from_table_database_format_type(m_table_database_format_type)`.
    pub(crate) database_format: &'a CStr,
    /// `m_open_counter`.
    pub(crate) open_counter: i32,
}

/// `UserTableInfo::save(filename)` (`table_info.cpp:377-397`): the pin's
/// answer, `false` only when `fopen(filename, "w")` failed, after its
/// `write %s failed.\n` line. The writes and the close are not checked, as
/// the pin does not check them.
#[must_use]
pub(crate) fn save_user_table_info(
    filename: &(impl crate::input::FileName + ?Sized),
    fields: &UserTableInfoFields<'_>,
) -> bool {
    let locale = SavedNumeric::install_c();
    let Some(mut output) = Stream::open_write(filename) else {
        native::report_user_write_failed(&filename.as_c_name());
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
pub(crate) fn atoi(str: &CStr) -> i32 {
    native::atoi(str)
}

#[cfg(test)]
mod tests;
