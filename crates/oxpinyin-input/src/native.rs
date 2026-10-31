//! The only unsafe code in the crate outside the tool boundary: the C
//! library calls of the `table.conf` and `user.conf` readers and writer
//! (074a2219 `src/storage/table_info.cpp:194-397`) and of the library's
//! `stderr` lines, made through the `libc` crate with the pin's own format
//! strings, and the two owned handles over what they return, a `FILE *` and
//! the name `setlocale` answered. Every function here makes the pin's calls
//! and no others; Rust chooses which to run and in what order.

use core::ffi::{c_char, c_float, c_int};
use core::ptr::NonNull;
use std::ffi::CStr;

/// The C library's `FILE`, never dereferenced on this side.
pub(crate) type File = libc::FILE;

/// The size of the pin's `char str[256]` buffers (`table_info.cpp:227`,
/// `:252-253`, `:351`), which `%255s` fills with at most 255 bytes and a NUL.
pub(crate) const WORD_SIZE: usize = 256;

// The C library's own `stdout` and `stderr` streams, which the `libc` crate
// does not name: each C library spells the objects behind `<stdio.h>`'s
// macros differently. Declared once here for the library and the tools.
// Writing to these streams, not to new ones on file descriptors 1 and 2,
// keeps every line in the C library's buffers and order, with GLib's.

#[cfg(target_os = "linux")]
unsafe extern "C" {
    // `extern FILE *stdout, *stderr;` (glibc and musl `stdio.h`).
    #[cfg(feature = "tools")]
    static mut stdout: *mut File;
    static mut stderr: *mut File;
}

#[cfg(any(
    target_vendor = "apple",
    target_os = "freebsd",
    target_os = "dragonfly"
))]
unsafe extern "C" {
    // `#define stdout __stdoutp` and `#define stderr __stderrp` (Apple Libc
    // and FreeBSD `stdio.h`).
    #[cfg(feature = "tools")]
    #[link_name = "__stdoutp"]
    static mut stdout: *mut File;
    #[link_name = "__stderrp"]
    static mut stderr: *mut File;
}

#[cfg(windows)]
unsafe extern "C" {
    // `#define stdout (__acrt_iob_func(1))` and `#define stderr
    // (__acrt_iob_func(2))` (UCRT `corecrt_wstdio.h`).
    fn __acrt_iob_func(index: core::ffi::c_uint) -> *mut File;
}

#[cfg(not(any(
    target_os = "linux",
    target_vendor = "apple",
    target_os = "freebsd",
    target_os = "dragonfly",
    windows
)))]
compile_error!(
    "oxpinyin-input: no declaration of the C library's `stdout` and `stderr` objects \
     for this target; add its per-platform `extern` block in src/native.rs"
);

/// The pointer `<stdio.h>`'s `stdout` expands to.
#[cfg(feature = "tools")]
pub(crate) fn c_stdout() -> *mut File {
    #[cfg(not(windows))]
    // SAFETY: the C library initialises `stdout` before `main` and nothing
    // here assigns it; the read copies the pointer.
    let stream = unsafe { stdout };
    #[cfg(windows)]
    // SAFETY: index 1 is the UCRT's standard output stream, which the
    // runtime keeps for the process's lifetime.
    let stream = unsafe { __acrt_iob_func(1) };
    stream
}

/// The pointer `<stdio.h>`'s `stderr` expands to.
pub(crate) fn c_stderr() -> *mut File {
    #[cfg(not(windows))]
    // SAFETY: as `c_stdout`: the C library initialises `stderr` before
    // `main` and nothing here assigns it.
    let stream = unsafe { stderr };
    #[cfg(windows)]
    // SAFETY: index 2 is the UCRT's standard error stream, which the
    // runtime keeps for the process's lifetime.
    let stream = unsafe { __acrt_iob_func(2) };
    stream
}

/// The name `setlocale(LC_NUMERIC, "C")` answered, held only to be passed
/// back to `setlocale` unread, as `table_info.cpp:197`/`:291` does.
pub(crate) struct SavedNumeric(*mut c_char);

impl SavedNumeric {
    /// `char * locale = setlocale(LC_NUMERIC, "C");`.
    pub(crate) fn install_c() -> Self {
        // SAFETY: the argument is a NUL-terminated literal that outlives the
        // call; `setlocale` only reads it.
        Self(unsafe { libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr()) })
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
            libc::setlocale(libc::LC_NUMERIC, self.0);
        }
    }
}

/// An open `FILE *` that this handle alone owns; dropping it is `fclose`.
pub(crate) struct Stream(NonNull<File>);

impl Stream {
    /// `fopen(filename, "r")`.
    pub(crate) fn open_read(filename: &CStr) -> Option<Self> {
        // SAFETY: `filename` and the mode literal are NUL-terminated and
        // outlive the call.
        NonNull::new(unsafe { libc::fopen(filename.as_ptr(), c"r".as_ptr()) }).map(Self)
    }

    /// `fopen(filename, "w")`.
    pub(crate) fn open_write(filename: &CStr) -> Option<Self> {
        // SAFETY: as `open_read`.
        NonNull::new(unsafe { libc::fopen(filename.as_ptr(), c"w".as_ptr()) }).map(Self)
    }

    /// `feof(input)`.
    pub(crate) fn eof(&mut self) -> bool {
        // SAFETY: the stream is open and owned by this handle.
        unsafe { libc::feof(self.0.as_ptr()) != 0 }
    }

    /// `fscanf(input, "binary format version:%d\n", &binver)`.
    pub(crate) fn scan_binary_format_version(&mut self, binver: &mut c_int) -> c_int {
        // SAFETY: the stream is open and owned; `binver` is a writable `int`
        // for the single `%d`.
        unsafe {
            libc::fscanf(
                self.0.as_ptr(),
                c"binary format version:%d\n".as_ptr(),
                binver as *mut c_int,
            )
        }
    }

    /// `fscanf(input, "model data version:%d\n", &modelver)`.
    pub(crate) fn scan_model_data_version(&mut self, modelver: &mut c_int) -> c_int {
        // SAFETY: as `scan_binary_format_version`.
        unsafe {
            libc::fscanf(
                self.0.as_ptr(),
                c"model data version:%d\n".as_ptr(),
                modelver as *mut c_int,
            )
        }
    }

    /// `fscanf(input, "lambda parameter:%f\n", &lambda)`.
    pub(crate) fn scan_lambda_parameter(&mut self, lambda: &mut c_float) -> c_int {
        // SAFETY: the stream is open and owned; `lambda` is a writable
        // `float` for the single `%f`.
        unsafe {
            libc::fscanf(
                self.0.as_ptr(),
                c"lambda parameter:%f\n".as_ptr(),
                lambda as *mut c_float,
            )
        }
    }

    /// `fscanf(input, "source table format:%255s\n", str)`.
    pub(crate) fn scan_source_table_format(&mut self, word: &mut [u8; WORD_SIZE]) -> c_int {
        // SAFETY: the stream is open and owned; `word` holds the 255 bytes
        // and NUL `%255s` can store.
        unsafe {
            libc::fscanf(
                self.0.as_ptr(),
                c"source table format:%255s\n".as_ptr(),
                word.as_mut_ptr().cast::<c_char>(),
            )
        }
    }

    /// `fscanf(input, "database format:%255s\n", str)`.
    pub(crate) fn scan_database_format(&mut self, word: &mut [u8; WORD_SIZE]) -> c_int {
        // SAFETY: as `scan_source_table_format`.
        unsafe {
            libc::fscanf(
                self.0.as_ptr(),
                c"database format:%255s\n".as_ptr(),
                word.as_mut_ptr().cast::<c_char>(),
            )
        }
    }

    /// `fscanf(input, "open counter:%d\n", &counter)`.
    pub(crate) fn scan_open_counter(&mut self, counter: &mut c_int) -> c_int {
        // SAFETY: as `scan_binary_format_version`.
        unsafe {
            libc::fscanf(
                self.0.as_ptr(),
                c"open counter:%d\n".as_ptr(),
                counter as *mut c_int,
            )
        }
    }

    /// The six-word `fscanf` of the row loop.
    pub(crate) fn scan_table_row(&mut self, words: &mut [[u8; WORD_SIZE]; 6]) -> c_int {
        let [w0, w1, w2, w3, w4, w5] = words;
        // SAFETY: the stream is open and owned; each of the six buffers
        // holds the 255 bytes and NUL its `%255s` can store.
        unsafe {
            libc::fscanf(
                self.0.as_ptr(),
                c"%255s %255s %255s %255s %255s %255s\n".as_ptr(),
                w0.as_mut_ptr().cast::<c_char>(),
                w1.as_mut_ptr().cast::<c_char>(),
                w2.as_mut_ptr().cast::<c_char>(),
                w3.as_mut_ptr().cast::<c_char>(),
                w4.as_mut_ptr().cast::<c_char>(),
                w5.as_mut_ptr().cast::<c_char>(),
            )
        }
    }

    /// The four `fprintf` calls of `UserTableInfo::save`
    /// (`table_info.cpp:386-390`), whose results the pin ignores.
    pub(crate) fn print_user_table_info(
        &mut self,
        binver: c_int,
        modelver: c_int,
        format: &CStr,
        counter: c_int,
    ) {
        let output = self.0.as_ptr();
        // SAFETY: the stream is open for writing and owned; each format
        // takes the one argument passed, an `int` for `%d` and the
        // NUL-terminated `format`, which outlives the calls, for `%s`.
        unsafe {
            libc::fprintf(output, c"binary format version:%d\n".as_ptr(), binver);
            libc::fprintf(output, c"model data version:%d\n".as_ptr(), modelver);
            libc::fprintf(output, c"database format:%s\n".as_ptr(), format.as_ptr());
            libc::fprintf(output, c"open counter:%d\n".as_ptr(), counter);
        }
    }

    /// `fclose(stream)` at the point the pin closes it.
    pub(crate) fn close(self) {
        let stream = core::mem::ManuallyDrop::new(self);
        // SAFETY: the stream is open and owned, and `ManuallyDrop` keeps
        // `Drop` from closing it a second time.
        unsafe {
            libc::fclose(stream.0.as_ptr());
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the stream is open and owned by this handle, which is
        // never used again.
        unsafe {
            libc::fclose(self.0.as_ptr());
        }
    }
}

/// `atoi(str)`.
pub(crate) fn atoi(word: &CStr) -> c_int {
    // SAFETY: `word` is NUL-terminated and outlives the call.
    unsafe { libc::atoi(word.as_ptr()) }
}

/// `fprintf(stderr, "open %s failed.\n", filename)`.
pub(crate) fn report_system_open_failed(filename: &CStr) {
    // SAFETY: `stderr` is the C library's open stream; the format takes the
    // one `%s`, `filename`, which is NUL-terminated and outlives the call.
    unsafe {
        libc::fprintf(c_stderr(), c"open %s failed.\n".as_ptr(), filename.as_ptr());
    }
}

/// `fprintf(stderr, "open %s failed.", filename)`.
pub(crate) fn report_user_open_failed(filename: &CStr) {
    // SAFETY: as `report_system_open_failed`.
    unsafe {
        libc::fprintf(c_stderr(), c"open %s failed.".as_ptr(), filename.as_ptr());
    }
}

/// `fprintf(stderr, "write %s failed.\n", filename)`.
pub(crate) fn report_user_write_failed(filename: &CStr) {
    // SAFETY: as `report_system_open_failed`.
    unsafe {
        libc::fprintf(
            c_stderr(),
            c"write %s failed.\n".as_ptr(),
            filename.as_ptr(),
        );
    }
}

/// `fprintf(stderr, "rename %s to %s failed.\n", from, to)`.
pub(crate) fn report_rename_failed(from: &CStr, to: &CStr) {
    // SAFETY: as `report_system_open_failed`, with the format's two `%s`
    // the two names.
    unsafe {
        libc::fprintf(
            c_stderr(),
            c"rename %s to %s failed.\n".as_ptr(),
            from.as_ptr(),
            to.as_ptr(),
        );
    }
}

/// `fprintf(stderr, "load %s failed!\n", filename)`.
pub(crate) fn report_load_failed(filename: &CStr) {
    // SAFETY: as `report_system_open_failed`.
    unsafe {
        libc::fprintf(c_stderr(), c"load %s failed!\n".as_ptr(), filename.as_ptr());
    }
}

/// `fprintf(stderr, "mmap %s failed!\n", filename)`.
pub(crate) fn report_mmap_failed(filename: &CStr) {
    // SAFETY: as `report_system_open_failed`.
    unsafe {
        libc::fprintf(c_stderr(), c"mmap %s failed!\n".as_ptr(), filename.as_ptr());
    }
}
