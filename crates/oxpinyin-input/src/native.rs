//! The only unsafe code in the crate: the declarations of `native/config.c`
//! and the two owned handles over what it returns, a `FILE *` and the name
//! `setlocale` answered. Every function here makes exactly one shim call.

use core::ffi::{c_char, c_float, c_int};
use core::marker::{PhantomData, PhantomPinned};
use core::ptr::NonNull;
use std::ffi::CStr;

/// The C library's `FILE`, never dereferenced on this side.
#[repr(C)]
pub(crate) struct File {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// The size of the pin's `char str[256]` buffers (`table_info.cpp:227`,
/// `:252-253`, `:351`), which `%255s` fills with at most 255 bytes and a NUL.
pub(crate) const WORD_SIZE: usize = 256;

unsafe extern "C" {
    fn oxpinyin_input_setlocale_numeric(locale: *const c_char) -> *mut c_char;
    fn oxpinyin_input_fopen_read(filename: *const c_char) -> *mut File;
    fn oxpinyin_input_fopen_write(filename: *const c_char) -> *mut File;
    fn oxpinyin_input_fclose(stream: *mut File) -> c_int;
    fn oxpinyin_input_feof(stream: *mut File) -> c_int;
    fn oxpinyin_input_scan_binary_format_version(input: *mut File, binver: *mut c_int) -> c_int;
    fn oxpinyin_input_scan_model_data_version(input: *mut File, modelver: *mut c_int) -> c_int;
    fn oxpinyin_input_scan_lambda_parameter(input: *mut File, lambda: *mut c_float) -> c_int;
    fn oxpinyin_input_scan_source_table_format(input: *mut File, word: *mut c_char) -> c_int;
    fn oxpinyin_input_scan_database_format(input: *mut File, word: *mut c_char) -> c_int;
    fn oxpinyin_input_scan_open_counter(input: *mut File, counter: *mut c_int) -> c_int;
    fn oxpinyin_input_scan_table_row(input: *mut File, words: *mut [c_char; WORD_SIZE]) -> c_int;
    fn oxpinyin_input_atoi(word: *const c_char) -> c_int;
    fn oxpinyin_input_print_user_table_info(
        output: *mut File,
        binver: c_int,
        modelver: c_int,
        format: *const c_char,
        counter: c_int,
    );
    fn oxpinyin_input_report_system_open_failed(filename: *const c_char);
    fn oxpinyin_input_report_user_open_failed(filename: *const c_char);
    fn oxpinyin_input_report_user_write_failed(filename: *const c_char);
    fn oxpinyin_input_report_rename_failed(from: *const c_char, to: *const c_char);
    fn oxpinyin_input_report_load_failed(filename: *const c_char);
    fn oxpinyin_input_report_mmap_failed(filename: *const c_char);
}

/// The name `setlocale(LC_NUMERIC, "C")` answered, held only to be passed
/// back to `setlocale` unread, as `table_info.cpp:197`/`:291` does.
pub(crate) struct SavedNumeric(*mut c_char);

impl SavedNumeric {
    /// `char * locale = setlocale(LC_NUMERIC, "C");`.
    pub(crate) fn install_c() -> Self {
        // SAFETY: the argument is a NUL-terminated literal that outlives the
        // call; `setlocale` only reads it.
        Self(unsafe { oxpinyin_input_setlocale_numeric(c"C".as_ptr()) })
    }

    /// `setlocale(LC_NUMERIC, locale);` with the saved answer.
    pub(crate) fn restore(self) {
        // SAFETY: the pointer is what `setlocale` answered at `install_c`,
        // or NULL when it failed, which makes this call a query. `setlocale`
        // answers the name of the locale it has just installed (C11
        // 7.11.1.1p7), here "C", from storage that stays readable until the
        // next `setlocale`; nothing between the two calls of one reader or
        // writer calls it. The pin passes the same pointer back at the same
        // point (`table_info.cpp:291`, `:372`, `:394`), and like the pin this
        // is not safe against a concurrent `setlocale` in another thread,
        // which POSIX marks MT-Unsafe.
        unsafe {
            oxpinyin_input_setlocale_numeric(self.0);
        }
    }
}

/// An open `FILE *` that this handle alone owns; dropping it is `fclose`.
pub(crate) struct Stream(NonNull<File>);

impl Stream {
    /// `fopen(filename, "r")`.
    pub(crate) fn open_read(filename: &CStr) -> Option<Self> {
        // SAFETY: `filename` is NUL-terminated and outlives the call; the
        // mode is the shim's literal.
        NonNull::new(unsafe { oxpinyin_input_fopen_read(filename.as_ptr()) }).map(Self)
    }

    /// `fopen(filename, "w")`.
    pub(crate) fn open_write(filename: &CStr) -> Option<Self> {
        // SAFETY: as `open_read`.
        NonNull::new(unsafe { oxpinyin_input_fopen_write(filename.as_ptr()) }).map(Self)
    }

    /// `feof(input)`.
    pub(crate) fn eof(&mut self) -> bool {
        // SAFETY: the stream is open and owned by this handle.
        unsafe { oxpinyin_input_feof(self.0.as_ptr()) != 0 }
    }

    /// `fscanf(input, "binary format version:%d\n", &binver)`.
    pub(crate) fn scan_binary_format_version(&mut self, binver: &mut c_int) -> c_int {
        // SAFETY: the stream is open and owned; `binver` is a writable `int`
        // for the single `%d`.
        unsafe { oxpinyin_input_scan_binary_format_version(self.0.as_ptr(), binver) }
    }

    /// `fscanf(input, "model data version:%d\n", &modelver)`.
    pub(crate) fn scan_model_data_version(&mut self, modelver: &mut c_int) -> c_int {
        // SAFETY: as `scan_binary_format_version`.
        unsafe { oxpinyin_input_scan_model_data_version(self.0.as_ptr(), modelver) }
    }

    /// `fscanf(input, "lambda parameter:%f\n", &lambda)`.
    pub(crate) fn scan_lambda_parameter(&mut self, lambda: &mut c_float) -> c_int {
        // SAFETY: the stream is open and owned; `lambda` is a writable
        // `float` for the single `%f`.
        unsafe { oxpinyin_input_scan_lambda_parameter(self.0.as_ptr(), lambda) }
    }

    /// `fscanf(input, "source table format:%255s\n", str)`.
    pub(crate) fn scan_source_table_format(&mut self, word: &mut [u8; WORD_SIZE]) -> c_int {
        // SAFETY: the stream is open and owned; `word` holds the 255 bytes
        // and NUL `%255s` can store.
        unsafe {
            oxpinyin_input_scan_source_table_format(self.0.as_ptr(), word.as_mut_ptr().cast())
        }
    }

    /// `fscanf(input, "database format:%255s\n", str)`.
    pub(crate) fn scan_database_format(&mut self, word: &mut [u8; WORD_SIZE]) -> c_int {
        // SAFETY: as `scan_source_table_format`.
        unsafe { oxpinyin_input_scan_database_format(self.0.as_ptr(), word.as_mut_ptr().cast()) }
    }

    /// `fscanf(input, "open counter:%d\n", &counter)`.
    pub(crate) fn scan_open_counter(&mut self, counter: &mut c_int) -> c_int {
        // SAFETY: as `scan_binary_format_version`.
        unsafe { oxpinyin_input_scan_open_counter(self.0.as_ptr(), counter) }
    }

    /// The six-word `fscanf` of the row loop.
    pub(crate) fn scan_table_row(&mut self, words: &mut [[u8; WORD_SIZE]; 6]) -> c_int {
        // SAFETY: the stream is open and owned; `words` is six contiguous
        // 256-byte buffers, one for each `%255s`.
        unsafe { oxpinyin_input_scan_table_row(self.0.as_ptr(), words.as_mut_ptr().cast()) }
    }

    /// The four `fprintf` calls of `UserTableInfo::save`.
    pub(crate) fn print_user_table_info(
        &mut self,
        binver: c_int,
        modelver: c_int,
        format: &CStr,
        counter: c_int,
    ) {
        // SAFETY: the stream is open for writing and owned; `format` is
        // NUL-terminated and outlives the call.
        unsafe {
            oxpinyin_input_print_user_table_info(
                self.0.as_ptr(),
                binver,
                modelver,
                format.as_ptr(),
                counter,
            );
        }
    }

    /// `fclose(stream)` at the point the pin closes it.
    pub(crate) fn close(self) {
        let stream = core::mem::ManuallyDrop::new(self);
        // SAFETY: the stream is open and owned, and `ManuallyDrop` keeps
        // `Drop` from closing it a second time.
        unsafe {
            oxpinyin_input_fclose(stream.0.as_ptr());
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the stream is open and owned by this handle, which is
        // never used again.
        unsafe {
            oxpinyin_input_fclose(self.0.as_ptr());
        }
    }
}

/// `atoi(str)`.
pub(crate) fn atoi(word: &CStr) -> c_int {
    // SAFETY: `word` is NUL-terminated and outlives the call.
    unsafe { oxpinyin_input_atoi(word.as_ptr()) }
}

/// `fprintf(stderr, "open %s failed.\n", filename)`.
pub(crate) fn report_system_open_failed(filename: &CStr) {
    // SAFETY: `filename` is NUL-terminated and outlives the call.
    unsafe { oxpinyin_input_report_system_open_failed(filename.as_ptr()) }
}

/// `fprintf(stderr, "open %s failed.", filename)`.
pub(crate) fn report_user_open_failed(filename: &CStr) {
    // SAFETY: as `report_system_open_failed`.
    unsafe { oxpinyin_input_report_user_open_failed(filename.as_ptr()) }
}

/// `fprintf(stderr, "write %s failed.\n", filename)`.
pub(crate) fn report_user_write_failed(filename: &CStr) {
    // SAFETY: as `report_system_open_failed`.
    unsafe { oxpinyin_input_report_user_write_failed(filename.as_ptr()) }
}

/// `fprintf(stderr, "rename %s to %s failed.\n", from, to)`.
pub(crate) fn report_rename_failed(from: &CStr, to: &CStr) {
    // SAFETY: both names are NUL-terminated and outlive the call.
    unsafe { oxpinyin_input_report_rename_failed(from.as_ptr(), to.as_ptr()) }
}

/// `fprintf(stderr, "load %s failed!\n", filename)`.
pub(crate) fn report_load_failed(filename: &CStr) {
    // SAFETY: as `report_system_open_failed`.
    unsafe { oxpinyin_input_report_load_failed(filename.as_ptr()) }
}

/// `fprintf(stderr, "mmap %s failed!\n", filename)`.
pub(crate) fn report_mmap_failed(filename: &CStr) {
    // SAFETY: as `report_system_open_failed`.
    unsafe { oxpinyin_input_report_mmap_failed(filename.as_ptr()) }
}
