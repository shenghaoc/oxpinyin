//! The only unsafe code in the crate outside the tool boundary: the C
//! library calls of the `table.conf` and `user.conf` readers and writer
//! (074a2219 `src/storage/table_info.cpp:194-397`), of the library's
//! `stderr` lines, and of the save path's `g_build_filename`,
//! `g_rename`, `unlink` and `MemoryChunk::save` calls, made through the
//! `libc` and `glib-sys` crates with the pin's own arguments and format
//! strings, and the owned handles over what they return. Every function
//! here makes the pin's calls; Rust chooses which to run and in what order.

use core::ffi::{c_char, c_float, c_int};
use core::ptr::NonNull;
use std::ffi::CStr;

/// The descriptor of a system/addon chunk, closed before its mapping
/// leave the loader. No `File` or borrowed raw handle escapes this module.
pub(crate) struct ReadChunk(c_int);

impl ReadChunk {
    pub(crate) fn open(path: &std::path::Path) -> std::io::Result<Self> {
        #[cfg(unix)]
        let fd = {
            let path = crate::path::c_string(path.as_os_str());
            // SAFETY: path is NUL-terminated and lives through open; without
            // O_CREAT its variadic mode argument is not read.
            unsafe { libc::open(path.as_ptr(), libc::O_RDONLY) }
        };
        #[cfg(windows)]
        let fd = {
            use std::os::windows::ffi::OsStrExt as _;
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .take_while(|&word| word != 0)
                .chain(std::iter::once(0))
                .collect();
            // SAFETY: wide is NUL-terminated and lives through _wopen; without
            // O_CREAT its variadic mode argument is not read. The returned CRT
            // descriptor is owned and closed by ReadChunk.
            unsafe { libc::wopen(wide.as_ptr(), libc::O_RDONLY | libc::O_BINARY) }
        };
        if fd < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(Self(fd))
        }
    }

    pub(crate) fn size_and_rewind(&self) -> std::io::Result<u64> {
        #[cfg(unix)]
        use libc::lseek;
        #[cfg(windows)]
        use libc::lseek64 as lseek;
        // SAFETY: the descriptor is open and owned. Both seeks use zero
        // offsets and the pin's whence values; the rewind result is ignored
        // just as at memory_chunk.h:484. Windows uses the CRT's 64-bit seek.
        let size = unsafe {
            let size = lseek(self.0, 0, libc::SEEK_END);
            lseek(self.0, 0, libc::SEEK_SET);
            size
        };
        u64::try_from(size).map_err(|_| std::io::Error::last_os_error())
    }

    pub(crate) fn read_word(&mut self) -> std::io::Result<[u8; 4]> {
        let mut word = [0_u8; 4];
        // SAFETY: the descriptor is open and owned; `word` is a writable
        // four-byte buffer that outlives the single read.
        let read = unsafe { libc::read(self.0, word.as_mut_ptr().cast(), 4) };
        if read == 4 {
            Ok(word)
        } else if read < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "short chunk header read",
            ))
        }
    }

    pub(crate) fn read_payload(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        #[cfg(unix)]
        let len = bytes.len();
        #[cfg(windows)]
        let len = u32::try_from(bytes.len()).map_err(std::io::Error::other)?;
        // SAFETY: the descriptor is owned and bytes is writable for its full
        // length. This is exactly one read, including for an empty payload.
        let count = unsafe { libc::read(self.0, bytes.as_mut_ptr().cast(), len) };
        usize::try_from(count).map_err(|_| std::io::Error::last_os_error())
    }

    pub(crate) fn map_copy(&self, file_size: usize) -> std::io::Result<memmap2::MmapMut> {
        #[cfg(unix)]
        let descriptor = self.0;
        #[cfg(windows)]
        // SAFETY: the CRT descriptor is open and owned. The OS handle is
        // borrowed only until map_copy finishes; memmap2 owns the resulting
        // view and its own Windows mapping handles.
        let descriptor = unsafe { libc::get_osfhandle(self.0) } as std::os::windows::io::RawHandle;
        let options = {
            let mut options = memmap2::MmapOptions::new();
            options.len(file_size);
            options
        };
        // SAFETY: the descriptor remains open through the call. The
        // explicit size is the file's end-seek result, and offset is zero.
        // memmap2 0.9.11 (and the unchanged 0.9.12 lockfile)
        // unix.rs::MmapInner::map_copy uses PROT_READ |
        // PROT_WRITE and MAP_PRIVATE; its owner unmaps the whole range.
        // Mutable slices borrow the sole owner exclusively. System/addon files
        // are never written by the runtime: callers must not modify or truncate them
        // while mapped. Rename replacement keeps the mapped inode alive.
        // Windows uses memmap2's copy-on-write view by maintainer ruling,
        // instead of the pin's non-mmap heap path.
        unsafe { options.map_copy(descriptor) }
    }
}

impl Drop for ReadChunk {
    fn drop(&mut self) {
        // SAFETY: this is the descriptor this owner opened. It is closed
        // once; mappings own their ranges independently of the descriptor.
        unsafe { libc::close(self.0) };
    }
}

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

/// Which header word of `MemoryChunk::save` a failed write targeted.
#[derive(Clone, Copy)]
pub(crate) enum HeaderField {
    Length,
    Checksum,
}

/// The result of `MemoryChunk::save`'s native calls.
pub(crate) enum ChunkWrite {
    Saved,
    Open(std::io::Error),
    Header(HeaderField, std::io::Error),
    Payload(std::io::Error),
}

/// `g_build_filename(dir, name, NULL)` with the pin's C arguments.
pub(crate) fn build_filename(dir: Option<&CStr>, name: Option<&CStr>) -> std::ffi::CString {
    let dir = dir.map_or(core::ptr::null(), CStr::as_ptr);
    let name = name.map_or(core::ptr::null(), CStr::as_ptr);
    // SAFETY: both arguments are NULL or NUL-terminated strings that outlive
    // the call, and the trailing NULL is the pin's own terminator. GLib
    // reads them and answers a newly allocated NUL-terminated string, or
    // NULL from its guard. `g_free` releases exactly that allocation.
    let joined = unsafe { glib_sys::g_build_filename(dir, name, core::ptr::null::<c_char>()) };
    if joined.is_null() {
        return std::ffi::CString::default();
    }
    // SAFETY: GLib answered a NUL-terminated allocation; the copy ends
    // before `g_free` releases it.
    let bytes = unsafe { CStr::from_ptr(joined) }.to_bytes().to_vec();
    // SAFETY: `joined` is the pointer GLib allocated for this call and no
    // one else owns it.
    unsafe { glib_sys::g_free(joined.cast()) };
    std::ffi::CString::new(bytes).unwrap_or_default()
}

/// GLib's filename builder joins or removes only ASCII separators. The
/// inputs are the current platform's `OsStr::as_encoded_bytes`, truncated at
/// their NULs; GLib does not alter the non-ASCII encoded segments.
#[cfg(windows)]
pub(crate) fn chunk_path_from_encoded(bytes: Vec<u8>) -> std::ffi::OsString {
    // SAFETY: `g_build_filename` has only combined self-synchronizing encoded
    // OsStr segments at ASCII separator boundaries from this same process.
    unsafe { std::ffi::OsString::from_encoded_bytes_unchecked(bytes) }
}

/// `g_rename(from, to)`; `rename(2)` is what the Unix macro expands to.
#[cfg(not(windows))]
pub(crate) fn rename(from: &CStr, to: &CStr) -> bool {
    // SAFETY: both pointers are NUL-terminated strings that outlive the
    // call; `rename` only reads them.
    unsafe { libc::rename(from.as_ptr(), to.as_ptr()) == 0 }
}

/// `g_rename(from, to)` on Windows, where GLib converts the UTF-8 names
/// and calls `MoveFileExW`.
#[cfg(windows)]
pub(crate) fn rename(from: &CStr, to: &CStr) -> bool {
    // SAFETY: both pointers are NUL-terminated strings that outlive the
    // call; GLib's `g_rename` only reads them.
    unsafe { g_rename(from.as_ptr(), to.as_ptr()) == 0 }
}

#[cfg(windows)]
unsafe extern "C" {
    fn g_rename(oldfilename: *const c_char, newfilename: *const c_char) -> c_int;
}

/// `unlink(path)`; the pin ignores the result.
pub(crate) fn unlink(path: &CStr) {
    // SAFETY: the pointer is a NUL-terminated string that outlives the call;
    // `unlink` only reads it.
    unsafe {
        let _ = libc::unlink(path.as_ptr());
    }
}

/// `MemoryChunk::save`'s calls on Unix.
#[cfg(unix)]
pub(crate) fn save_chunk(path: &CStr, bytes: &[u8]) -> ChunkWrite {
    // SAFETY: the path is NUL-terminated and outlives the call; the flags and
    // mode are the pin's (`memory_chunk.h:536-537`); `O_BINARY` is absent
    // on Unix and the pin's header defines it as zero there.
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_CREAT | libc::O_WRONLY | libc::O_TRUNC,
            // Variadic integers need int width even where mode_t is u16.
            0o644 as libc::c_uint,
        )
    };
    if fd < 0 {
        return ChunkWrite::Open(std::io::Error::last_os_error());
    }
    let split = bytes.len().min(8);
    let (header, payload) = bytes.split_at(split);
    let (length, checksum) = header.split_at(header.len().min(4));
    if let Err(source) = write_chunk_word(fd, length) {
        close(fd);
        return ChunkWrite::Header(HeaderField::Length, source);
    }
    if let Err(source) = write_chunk_word(fd, checksum) {
        close(fd);
        return ChunkWrite::Header(HeaderField::Checksum, source);
    }
    if let Err(source) = write_chunk_payload(fd, payload) {
        close(fd);
        return ChunkWrite::Payload(source);
    }
    // GLib 2.84.4 glib/gstdio.h:60,82 makes g_fsync a macro for fsync on
    // ordinary Unix builds. The 074a2219 tkrzw linux/amd64 objects from
    // tools/oracle/build-oracle.sh confirm fsync@plt in
    // pinyin::MemoryChunk::save(char const*) [clone .isra.0]: +0x108 in
    // libpinyin.so.15.0.0 and +0x118 in libzhuyin.so.15.0.0.
    // SAFETY: `fd` is the open descriptor and `fsync` only reads it; the
    // pin ignores the answer (`memory_chunk.h:555`).
    unsafe {
        let _ = libc::fsync(fd);
    }
    close(fd);
    ChunkWrite::Saved
}

/// `write(fd, word, sizeof(word))` for one header word.
#[cfg(unix)]
fn write_chunk_word(fd: c_int, word: &[u8]) -> Result<(), std::io::Error> {
    // SAFETY: `word` is a live slice for the length passed, and `fd` is the
    // open descriptor from `save_chunk`.
    let written = unsafe { libc::write(fd, word.as_ptr().cast(), word.len()) };
    if written == word.len() as isize {
        Ok(())
    } else {
        Err(short_write(word.len(), written, "header"))
    }
}

/// `write(fd, begin(), size())` for the payload.
#[cfg(unix)]
fn write_chunk_payload(fd: c_int, payload: &[u8]) -> Result<(), std::io::Error> {
    // SAFETY: as `write_chunk_word`, with the payload slice.
    let written = unsafe { libc::write(fd, payload.as_ptr().cast(), payload.len()) };
    if written == payload.len() as isize {
        Ok(())
    } else {
        Err(short_write(payload.len(), written, "payload"))
    }
}

#[cfg(unix)]
fn close(fd: c_int) {
    // SAFETY: `fd` is the descriptor `save_chunk` opened and has not closed.
    unsafe {
        let _ = libc::close(fd);
    }
}

fn short_write(expected: usize, written: isize, part: &str) -> std::io::Error {
    if written < 0 {
        std::io::Error::last_os_error()
    } else {
        std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            format!("short write: {written} of {expected} {part} bytes"),
        )
    }
}

/// `MemoryChunk::save`'s calls on Windows, where the pin's plain
/// `open`/`write` are the CRT's and its `g_fsync` is `_commit`. The
/// deferred Windows work is #1475; this keeps the same observable result.
#[cfg(windows)]
pub(crate) fn save_chunk(path: &CStr, bytes: &[u8]) -> ChunkWrite {
    use std::io::Write as _;

    let text = String::from_utf8_lossy(path.to_bytes()).into_owned();
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    let mut file = match options.open(text) {
        Ok(file) => file,
        Err(error) => return ChunkWrite::Open(error),
    };
    let split = bytes.len().min(8);
    let (header, payload) = bytes.split_at(split);
    let (length, checksum) = header.split_at(header.len().min(4));
    for (word, field) in [
        (length, HeaderField::Length),
        (checksum, HeaderField::Checksum),
    ] {
        match file.write(word) {
            Ok(written) if written == word.len() => {}
            Ok(written) => {
                return ChunkWrite::Header(
                    field,
                    short_write(word.len(), written as isize, "header"),
                );
            }
            Err(source) => return ChunkWrite::Header(field, source),
        }
    }
    match file.write(payload) {
        Ok(written) if written == payload.len() => {}
        Ok(written) => {
            return ChunkWrite::Payload(short_write(payload.len(), written as isize, "payload"));
        }
        Err(source) => return ChunkWrite::Payload(source),
    }
    let _ = file.sync_all();
    ChunkWrite::Saved
}
