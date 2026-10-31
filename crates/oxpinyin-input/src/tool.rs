//! The pin utilities' calls at their edge (`docs/findings/external-io-boundary.md`,
//! stage 5), in one safe layer over the `libc` crate and `glib-sys`
//! (maintainer ruling 2026-10-31 UTC: no hand-written C or C++ shims). Each
//! function makes the pin's call and owns what makes it safe: NUL-terminated
//! strings, the format fixed with arguments of its types, and a `FILE *`
//! closed exactly once. C++ `std::getline` is [`LineReader`], under
//! getline's contract. Only with the `tools` feature: the shipping libraries
//! never link GLib's option parser through this crate.
//!
//! One symbol neither crate binds is declared here, GLib's `g_utf8_skip`
//! table; the C runtime's `stdout` and `stderr` are declared per platform
//! once for the library and the tools, in the crate's native module. What a
//! field or a line means stays with the utility that reads it.

use core::ffi::{c_char, c_int, c_long, c_void};
use core::ptr::NonNull;
use std::ffi::{CStr, CString, OsString};
use std::io::{BufRead, BufReader};

use libc::FILE;

use crate::native::{c_stderr, c_stdout};

unsafe extern "C" {
    /// `GLIB_VAR const gchar * const g_utf8_skip` (`glib/gunicode.h`), the
    /// table `g_utf8_next_char` steps by.
    static g_utf8_skip: *const c_char;
}

/// The target's `EINVAL`, the pin's exit status for a refused option or
/// argument (`ngseg.cpp:119`).
pub const EINVAL: c_int = libc::EINVAL;
/// The target's `ENOENT`, the pin's exit status for a file it cannot open
/// (`gen_pinyin_table.cpp:141`, `:264`).
pub const ENOENT: c_int = libc::ENOENT;
/// The target's `ENODATA`, the pin's exit status for an empty or wrong
/// model (`k_mixture_model_to_interpolation.cpp:200`, `:204`).
pub const ENODATA: c_int = libc::ENODATA;

/// `setlocale(LC_ALL, "")`, the first call of every utility's `main`
/// (074a2219 `utils/segment/ngseg.cpp:111`); whether it answered a name.
#[must_use]
pub fn set_locale_from_environment() -> bool {
    // SAFETY: the argument is a NUL-terminated literal; the answer is only
    // tested.
    !unsafe { libc::setlocale(libc::LC_ALL, c"".as_ptr()) }.is_null()
}

/// `exit(status)`: the C library's, so its `atexit` handlers and stream
/// flushes run as the pin's do.
pub fn exit(status: c_int) -> ! {
    // SAFETY: `exit` never returns.
    unsafe { libc::exit(status) }
}

/// `abort()`, where the pin aborts (constitution §4).
pub fn abort() -> ! {
    // SAFETY: `abort` never returns.
    unsafe { libc::abort() }
}

/// `perror(s)` (`ngseg.cpp:124`).
pub fn perror(s: &CStr) {
    // SAFETY: `s` is NUL-terminated and outlives the call.
    unsafe { libc::perror(s.as_ptr()) }
}

/// `atoi(str)` (`utils/utils_helper.h:27-58`).
#[must_use]
pub fn atoi(str: &CStr) -> c_int {
    // SAFETY: `str` is NUL-terminated and outlives the call.
    unsafe { libc::atoi(str.as_ptr()) }
}

/// `atol(str)` (`utils/utils_helper.h:60-69`).
#[must_use]
pub fn atol(str: &CStr) -> c_long {
    // SAFETY: as `atoi`.
    unsafe { libc::atol(str.as_ptr()) }
}

/// One `GOptionEntry`'s argument: its `GOptionArg` and the value its static
/// holds before parsing. The pin's utilities use these four.
#[derive(Clone, Debug, PartialEq)]
pub enum OptionArg {
    /// `G_OPTION_ARG_NONE` over a `gboolean`.
    Flag(bool),
    /// `G_OPTION_ARG_INT` over a `gint`.
    Int(i32),
    /// `G_OPTION_ARG_DOUBLE` over a `gdouble`.
    Double(f64),
    /// `G_OPTION_ARG_FILENAME` over a `gchar *` that starts at this
    /// string, or NULL.
    Filename(Option<&'static CStr>),
}

/// The storage behind one entry's `arg_data`: the type its `GOptionArg`
/// names.
#[repr(C)]
#[derive(Clone, Copy)]
union RawValue {
    flag: glib_sys::gboolean,
    string: *mut c_char,
    integer: c_int,
    number: f64,
}

impl OptionArg {
    const fn kind(&self) -> glib_sys::GOptionArg {
        match self {
            Self::Flag(_) => glib_sys::G_OPTION_ARG_NONE,
            Self::Int(_) => glib_sys::G_OPTION_ARG_INT,
            Self::Filename(_) => glib_sys::G_OPTION_ARG_FILENAME,
            Self::Double(_) => glib_sys::G_OPTION_ARG_DOUBLE,
        }
    }

    fn default_string(&self) -> *mut c_char {
        match self {
            Self::Filename(Some(text)) => text.as_ptr().cast_mut(),
            _ => core::ptr::null_mut(),
        }
    }

    fn raw(&self) -> RawValue {
        match *self {
            Self::Flag(flag) => RawValue {
                flag: c_int::from(flag),
            },
            Self::Int(integer) => RawValue { integer },
            Self::Double(number) => RawValue { number },
            Self::Filename(_) => RawValue {
                string: self.default_string(),
            },
        }
    }

    /// The slot after parsing.
    fn read(&self, value: RawValue) -> OptionValue {
        // SAFETY: each slot was initialised as the member its `GOptionArg`
        // names, and GLib writes that type there.
        unsafe {
            match self {
                Self::Flag(_) => OptionValue::Flag(value.flag != 0),
                Self::Int(_) => OptionValue::Int(value.integer),
                Self::Double(_) => OptionValue::Double(value.number),
                Self::Filename(_) if value.string == self.default_string() => {
                    OptionValue::Filename(
                        (!value.string.is_null())
                            .then(|| CStr::from_ptr(value.string).to_bytes().to_vec()),
                    )
                }
                Self::Filename(_) => OptionValue::Filename(take_glib_string(value.string)),
            }
        }
    }
}

/// One entry of a utility's `static GOptionEntry entries[]`.
#[derive(Clone, Debug)]
pub struct OptionEntry {
    /// `long_name`.
    pub long_name: &'static CStr,
    /// `short_name`, `0` for none.
    pub short_name: u8,
    /// `flags`.
    pub flags: c_int,
    /// `arg` with the static's initial value.
    pub arg: OptionArg,
    /// `description`.
    pub description: Option<&'static CStr>,
    /// `arg_description`.
    pub arg_description: Option<&'static CStr>,
}

/// An entry's value after parsing.
#[derive(Clone, Debug, PartialEq)]
pub enum OptionValue {
    /// A `gboolean`.
    Flag(bool),
    /// A `gint`.
    Int(i32),
    /// A `gdouble`.
    Double(f64),
    /// A filename's bytes: GLib's, or the default the entry started at;
    /// `None` for a NULL default left unset.
    Filename(Option<Vec<u8>>),
}

/// What the option sequence left: each entry's value, in entry order, and
/// the arguments `g_option_context_parse` left in `argv`, `argv[0]`
/// included.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedOptions {
    /// The entries' values.
    pub values: Vec<OptionValue>,
    /// `argv[0..argc]` after parsing.
    pub args: Vec<Vec<u8>>,
}

/// `g_option_context_parse` failed and `option parsing failed:%s\n` has
/// been printed with `g_print`; the pin's caller then calls
/// `exit(EINVAL)` ([`exit`], [`EINVAL`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptionsFailed;

/// The option sequence of every option-taking utility (074a2219
/// `ngseg.cpp:113-120`, `gen_binary_files.cpp:114-121` and fifteen more):
/// `g_option_context_new(parameter_string)`,
/// `g_option_context_add_main_entries(context, entries, NULL)`,
/// `g_option_context_parse(context, &argc, &argv, &error)` and, on failure,
/// `g_print("option parsing failed:%s\n", error->message)`. The entries and
/// their value slots live until the context is freed after the parse; the
/// values and arguments are copied out first. A successful parse leaves its
/// filename strings to the caller (GLib 2.84.4 `glib/goption.c:1585-1630`
/// frees them only when reverting a failed parse), and they are copied and
/// freed with `g_free`.
///
/// `args` is the process's `argv`, `argv[0]` first; an argument holding a
/// NUL, which no C `argv` can, ends the list there. GLib handles `--help`
/// itself and exits, as it does for the pin.
///
/// # Errors
///
/// [`OptionsFailed`] when GLib refused the arguments.
pub fn parse_options(
    parameter_string: &'static CStr,
    entries: &[OptionEntry],
    args: impl IntoIterator<Item = OsString>,
) -> Result<ParsedOptions, OptionsFailed> {
    let owned: Vec<CString> = args
        .into_iter()
        .map_while(|arg| CString::new(arg.into_encoded_bytes()).ok())
        .collect();
    let mut argv: Vec<*mut c_char> = owned
        .iter()
        .map(|arg| arg.as_ptr().cast_mut())
        .chain(core::iter::once(core::ptr::null_mut()))
        .collect();
    let mut argc = c_int::try_from(owned.len()).map_err(|_| OptionsFailed)?;
    let mut values: Vec<RawValue> = entries.iter().map(|entry| entry.arg.raw()).collect();
    let options: Vec<glib_sys::GOptionEntry> = entries
        .iter()
        .zip(values.iter_mut())
        .map(|(entry, value)| glib_sys::GOptionEntry {
            long_name: entry.long_name.as_ptr(),
            short_name: entry.short_name as c_char,
            flags: entry.flags,
            arg: entry.arg.kind(),
            arg_data: core::ptr::from_mut(value).cast(),
            description: entry.description.map_or(core::ptr::null(), CStr::as_ptr),
            arg_description: entry
                .arg_description
                .map_or(core::ptr::null(), CStr::as_ptr),
        })
        .chain(core::iter::once(glib_sys::GOptionEntry {
            long_name: core::ptr::null(),
            short_name: 0,
            flags: 0,
            arg: 0,
            arg_data: core::ptr::null_mut(),
            description: core::ptr::null(),
            arg_description: core::ptr::null(),
        }))
        .collect();
    let mut argv_ptr = argv.as_mut_ptr();
    let mut error: *mut glib_sys::GError = core::ptr::null_mut();
    // SAFETY: every name and description is a `'static` NUL-terminated
    // string or NULL; the entry array ends with a zeroed entry and, with the
    // value slots each `arg_data` points at, outlives the context, which is
    // freed below; `argv` is `argc` valid C strings and a NULL, which GLib
    // permutes and shortens in place.
    let (context, parsed) = unsafe {
        let context = glib_sys::g_option_context_new(parameter_string.as_ptr());
        glib_sys::g_option_context_add_main_entries(context, options.as_ptr(), core::ptr::null());
        let parsed = glib_sys::g_option_context_parse(
            context,
            &raw mut argc,
            &raw mut argv_ptr,
            &raw mut error,
        );
        (context, parsed)
    };
    if parsed == 0 {
        // SAFETY: a failed parse sets `error` to a GError whose message is a
        // NUL-terminated string; the format takes one string. The error and
        // the context are freed once, after the message.
        unsafe {
            glib_sys::g_print(c"option parsing failed:%s\n".as_ptr(), (*error).message);
            glib_sys::g_error_free(error);
            glib_sys::g_option_context_free(context);
        }
        return Err(OptionsFailed);
    }
    let count = usize::try_from(argc).unwrap_or(0);
    let mut args = Vec::with_capacity(count);
    for index in 0..count {
        // SAFETY: GLib leaves `argv_ptr` pointing at `argc` valid strings
        // (the caller's, or GLib's own where it converts them).
        let arg = unsafe { *argv_ptr.add(index) };
        if arg.is_null() {
            break;
        }
        // SAFETY: `arg` is one of those NUL-terminated strings, alive until
        // the context is freed.
        args.push(unsafe { CStr::from_ptr(arg) }.to_bytes().to_vec());
    }
    let values = entries
        .iter()
        .zip(&values)
        .map(|(entry, value)| entry.arg.read(*value))
        .collect();
    // SAFETY: the context is not used again; GLib no longer writes the value
    // slots, whose strings were copied and freed above.
    unsafe { glib_sys::g_option_context_free(context) };
    drop(options);
    Ok(ParsedOptions { values, args })
}

/// Copies a GLib-allocated string and frees it with `g_free`.
fn take_glib_string(string: *mut c_char) -> Option<Vec<u8>> {
    if string.is_null() {
        return None;
    }
    // SAFETY: a non-NULL GLib string is NUL-terminated and only this call
    // reads and frees it.
    let bytes = unsafe { CStr::from_ptr(string) }.to_bytes().to_vec();
    // SAFETY: as above; it is not used again.
    unsafe { glib_sys::g_free(string.cast()) };
    Some(bytes)
}

/// `g_strsplit_set(string, delimiters, max_tokens)`
/// (`utils/utils_helper.h:27-46`), its strings copied out.
#[must_use]
pub fn strsplit_set(string: &CStr, delimiters: &CStr, max_tokens: c_int) -> Vec<Vec<u8>> {
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let array = unsafe {
        glib_sys::g_strsplit_set(string.as_ptr(), delimiters.as_ptr().cast(), max_tokens)
    };
    let mut parts = Vec::new();
    if array.is_null() {
        return parts;
    }
    for index in 0.. {
        // SAFETY: `g_strsplit_set` answers a NULL-terminated array of
        // NUL-terminated strings; `index` never passes its NULL.
        let part = unsafe { *array.add(index) };
        if part.is_null() {
            break;
        }
        // SAFETY: `part` is one of the array's strings.
        parts.push(unsafe { CStr::from_ptr(part) }.to_bytes().to_vec());
    }
    // SAFETY: the array came from `g_strsplit_set` and is not used again.
    unsafe { glib_sys::g_strfreev(array) };
    parts
}

/// `g_utf8_strlen(str, -1)` (`ngseg.cpp:190`).
#[must_use]
pub fn utf8_strlen(str: &CStr) -> c_long {
    // SAFETY: `str` is NUL-terminated and outlives the call.
    unsafe { glib_sys::g_utf8_strlen(str.as_ptr(), -1) }
}

/// `g_utf8_to_ucs4(str, -1, NULL, &len, NULL)` (`ngseg.cpp:192`): the
/// characters, `None` where GLib answers NULL, and `len`.
#[must_use]
pub fn utf8_to_ucs4(str: &CStr) -> (Option<Vec<u32>>, c_long) {
    let mut written: c_long = 0;
    // SAFETY: `str` is NUL-terminated and outlives the call; `written` is a
    // writable `glong`; the two NULL out-parameters are permitted.
    let result = unsafe {
        glib_sys::g_utf8_to_ucs4(
            str.as_ptr(),
            -1,
            core::ptr::null_mut(),
            &raw mut written,
            core::ptr::null_mut(),
        )
    };
    if result.is_null() {
        return (None, written);
    }
    let count = usize::try_from(written).unwrap_or(0);
    // SAFETY: GLib wrote `written` characters at `result`.
    let chars = unsafe { core::slice::from_raw_parts(result, count) }.to_vec();
    // SAFETY: `result` is GLib's allocation and is not used again.
    unsafe { glib_sys::g_free(result.cast()) };
    (Some(chars), written)
}

/// `g_ucs4_to_utf8(str, len, NULL, NULL, NULL)` (`ngseg.cpp:80`, `:98`),
/// `None` where GLib answers NULL.
#[must_use]
pub fn ucs4_to_utf8(str: &[u32]) -> Option<Vec<u8>> {
    let len = c_long::try_from(str.len()).ok()?;
    // SAFETY: `str` holds `len` characters and outlives the call; the NULL
    // out-parameters are permitted.
    take_glib_string(unsafe {
        glib_sys::g_ucs4_to_utf8(
            str.as_ptr(),
            len,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    })
}

/// `g_unichar_isspace(c)` (`src/storage/tag_utility.cpp:127`).
#[must_use]
pub fn unichar_isspace(c: u32) -> bool {
    // SAFETY: a plain value call.
    unsafe { glib_sys::g_unichar_isspace(c) != 0 }
}

/// `g_unichar_isgraph(c)` (`src/storage/tag_utility.cpp:152`).
#[must_use]
pub fn unichar_isgraph(c: u32) -> bool {
    // SAFETY: a plain value call.
    unsafe { glib_sys::g_unichar_isgraph(c) != 0 }
}

/// `g_utf8_get_char(line + offset)` (`tag_utility.cpp:122`, `:133`,
/// `:150`): `None` for an offset past the line's NUL, where the pin's read
/// would leave the string.
#[must_use]
pub fn utf8_get_char_at(line: &CStr, offset: usize) -> Option<u32> {
    let bytes = line.to_bytes_with_nul();
    if offset >= bytes.len() {
        return None;
    }
    // SAFETY: `offset` is within the NUL-terminated string, and
    // `g_utf8_get_char` reads a continuation byte only after a continuation
    // byte, so it stops at the NUL at the latest (GLib 2.84.4
    // glib/gutf8.c:44-63, UTF8_GET).
    Some(unsafe { glib_sys::g_utf8_get_char(bytes.as_ptr().add(offset).cast()) })
}

/// The step `g_utf8_next_char` takes from the byte `c` (`g_utf8_skip[c]`).
#[must_use]
pub fn utf8_next_char_step(c: u8) -> usize {
    // SAFETY: GLib's table has an entry for every byte value.
    let step = unsafe { *g_utf8_skip.add(usize::from(c)) };
    usize::try_from(step).unwrap_or(1)
}

/// `g_warning(format, text)` for a format with one `%s`: `g_log` with
/// libpinyin's NULL domain at `G_LOG_LEVEL_WARNING`.
fn warning(format: &'static CStr, text: &CStr) {
    // SAFETY: both strings are NUL-terminated and outlive the call; the
    // callers' formats take exactly one `%s`.
    unsafe {
        glib_sys::g_log(
            core::ptr::null(),
            glib_sys::G_LOG_LEVEL_WARNING,
            format.as_ptr(),
            text.as_ptr(),
        );
    }
}

/// taglib's `g_warning("un-expected tags:%s.\n", tmp)`
/// (`tag_utility.cpp:239`).
pub fn warn_unexpected_tag(tmp: &CStr) {
    warning(c"un-expected tags:%s.\n", tmp);
}

/// taglib's `g_warning("missed required tags: %s.\n", required_tag_str)`
/// (`tag_utility.cpp:256`).
pub fn warn_missed_required_tag(required_tag_str: &CStr) {
    warning(c"missed required tags: %s.\n", required_tag_str);
}

/// taglib's `g_strv_length(tokens)` on the NULL `split_line` answered
/// (`tag_utility.cpp:181`): GLib's own precondition logs its critical and
/// answers 0 before reading.
#[must_use]
pub fn strv_length_null() -> u32 {
    // SAFETY: GLib refuses the NULL before dereferencing it.
    unsafe { glib_sys::g_strv_length(core::ptr::null_mut()) }
}

/// One `fprintf(stream, format)` with a format that takes no argument.
fn print_literal(stream: *mut FILE, format: &'static CStr) {
    // SAFETY: the stream is open; the callers' literals hold no conversion.
    unsafe { libc::fprintf(stream, format.as_ptr()) };
}

/// One `fprintf(stream, format, text)` with a format taking one string.
fn print_text(stream: *mut FILE, format: &'static CStr, text: &CStr) {
    // SAFETY: the stream is open; the string is NUL-terminated and outlives
    // the call; the callers' formats take exactly one `%s`.
    unsafe { libc::fprintf(stream, format.as_ptr(), text.as_ptr()) };
}

/// One `fprintf(stream, format, first, second)` with a format taking two
/// strings.
fn print_texts(stream: *mut FILE, format: &'static CStr, first: &CStr, second: &CStr) {
    // SAFETY: as `print_text`; the callers' formats take exactly two `%s`.
    unsafe { libc::fprintf(stream, format.as_ptr(), first.as_ptr(), second.as_ptr()) };
}

/// A utility's output `FILE *`: `stdout`, which is never closed here, or a
/// stream `fopen(filename, "w")` opened, which is closed exactly once.
pub struct OutputStream {
    stream: NonNull<FILE>,
    owned: bool,
}

impl OutputStream {
    /// `FILE * output = stdout;` (`ngseg.cpp:106`).
    #[must_use]
    pub fn stdout() -> Option<Self> {
        NonNull::new(c_stdout()).map(|stream| Self {
            stream,
            owned: false,
        })
    }

    /// `fopen(filename, "w")` (`ngseg.cpp:122`); `None` for NULL.
    #[must_use]
    pub fn open_write(filename: &CStr) -> Option<Self> {
        // SAFETY: `filename` and the mode are NUL-terminated and outlive the
        // call.
        NonNull::new(unsafe { libc::fopen(filename.as_ptr(), c"w".as_ptr()) }).map(|stream| Self {
            stream,
            owned: true,
        })
    }

    fn as_ptr(&self) -> *mut FILE {
        self.stream.as_ptr()
    }

    /// `fclose(output)` (`ngseg.cpp:260`). `stdout` is not closed.
    pub fn close(self) {
        drop(self);
    }

    /// `fclose(output)` (`ngseg.cpp:260`), including when the pin's
    /// `output` is the C runtime's `stdout` (`ngseg.cpp:106`, `:260`): the
    /// stream is closed exactly once, and a later exit has no buffered
    /// line left to flush.
    pub fn close_including_stdout(self) {
        let stream = self.stream;
        // The owned case would otherwise be closed again by `Drop`.
        core::mem::forget(self);
        // SAFETY: the stream is open, was never closed, and is not used
        // again; for `stdout` the C runtime keeps the object itself alive.
        unsafe {
            libc::fclose(stream.as_ptr());
        }
    }
}

impl Drop for OutputStream {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: the owned stream is open and never used again.
            unsafe {
                libc::fclose(self.stream.as_ptr());
            }
        }
    }
}

/// `filename` as a Rust path. On Unix the C string's bytes are the path.
/// Elsewhere the C runtime converts a narrow name through the active code
/// page, which is not reproduced: a name that is not UTF-8 does not open.
fn path_of(filename: &CStr) -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        Some(std::ffi::OsStr::from_bytes(filename.to_bytes()).into())
    }
    #[cfg(not(unix))]
    {
        filename.to_str().ok().map(Into::into)
    }
}

/// A C++ input stream as `std::getline(stream, linebuf)` reads it into the
/// pin's `LineString`: its `eofbit`/`failbit` and one reusable line, which
/// ends in the NUL `c_str()` adds.
pub struct LineReader<R> {
    reader: Option<R>,
    tied: bool,
    eof: bool,
    fail: bool,
    line: Vec<u8>,
}

impl LineReader<std::io::StdinLock<'static>> {
    /// `std::istream * input = &std::cin;` (`ngseg.cpp:105`). `cin.tie()`
    /// is `&cout` ([iostream.objects]), so every read first flushes it, and
    /// with `sync_with_stdio` that flush is `fflush(stdout)` (libstdc++
    /// `stdio_sync_filebuf::sync`, libc++ `__stdoutbuf::sync`).
    #[must_use]
    pub fn stdin() -> Self {
        Self::over(Some(std::io::stdin().lock()), true)
    }
}

impl LineReader<BufReader<std::fs::File>> {
    /// `input_file.open(filename)` (`ngseg.cpp:131`): an `std::ifstream`
    /// has no tie, and a failed open sets `failbit`, which `!input_file`
    /// ([`Self::is_open`]) tests.
    #[must_use]
    pub fn open(filename: &CStr) -> Self {
        let file = path_of(filename).and_then(|path| std::fs::File::open(path).ok());
        Self::over(file.map(BufReader::new), false)
    }
}

impl<'a> LineReader<&'a [u8]> {
    /// An `std::istringstream` over `text`: no tie, no file.
    #[must_use]
    pub fn from_bytes(text: &'a [u8]) -> Self {
        Self::over(Some(text), false)
    }
}

impl<R: BufRead> LineReader<R> {
    fn over(reader: Option<R>, tied: bool) -> Self {
        Self {
            fail: reader.is_none(),
            reader,
            tied,
            eof: false,
            line: vec![0],
        }
    }

    /// `static_cast<bool>(stream)`: `!fail()`.
    #[must_use]
    pub fn is_open(&self) -> bool {
        !self.fail
    }

    /// `static_cast<bool>(std::getline(stream, linebuf))` ([string.io]):
    /// the sentry fails, sets `failbit` and leaves the line as it was on a
    /// stream that is not `good()`; otherwise it erases the line and
    /// extracts up to and including the next `'\n'`, which it does not
    /// store. End of input sets `eofbit`, and `failbit` too when nothing was
    /// extracted, so a last line without a newline is still read.
    pub fn getline(&mut self) -> bool {
        let Some(reader) = self.reader.as_mut().filter(|_| !self.eof && !self.fail) else {
            self.fail = true;
            return false;
        };
        if self.tied {
            // SAFETY: `stdout` is the C runtime's open stream.
            unsafe { libc::fflush(c_stdout()) };
        }
        self.line.clear();
        match reader.read_until(b'\n', &mut self.line) {
            Ok(0) => {
                self.eof = true;
                self.fail = true;
            }
            Ok(_) if self.line.last() == Some(&b'\n') => {
                self.line.pop();
            }
            Ok(_) => self.eof = true,
            Err(_) => self.fail = true,
        }
        self.line.push(0);
        !self.fail
    }

    /// The `std::string`'s bytes.
    #[must_use]
    pub fn line(&self) -> &[u8] {
        &self.line[..self.line.len() - 1]
    }

    /// What `operator const gchar *` (`src/include/pinyin_utils.h:33-40`)
    /// shows a C call: `c_str()`, read up to its first NUL.
    #[must_use]
    pub fn c_str(&self) -> &CStr {
        CStr::from_bytes_until_nul(&self.line).unwrap_or_default()
    }
}

/// `gen_pinyin_table`'s calls (`utils/storage/gen_pinyin_table.cpp` at
/// 074a2219).
pub mod gen_pinyin_table {
    use super::{FILE, NonNull, OutputStream, c_int, c_long, c_stderr, c_void, print_text};
    use std::ffi::CStr;

    /// gen_pinyin_table's `char phrase[1024], pinyin[1024]` (`:136`) plus
    /// eight zero bytes past them, so a `g_utf8_next_char` step over a
    /// truncated sequence at the end of a `%1023s` word stays inside the
    /// buffer.
    const FIELD: usize = 1024 + 8;

    /// One input file of `feed_file` (`:134-157`) and its `char phrase[1024],
    /// pinyin[1024]` and `guint32 freq` locals, which keep their bytes from
    /// one `fscanf` to the next as the pin's do. They start zeroed where the
    /// pin's start indeterminate. The stream is closed exactly once.
    pub struct TableInput {
        stream: NonNull<FILE>,
        phrase: Box<[u8; FIELD]>,
        pinyin: Box<[u8; FIELD]>,
        freq: u32,
    }

    impl TableInput {
        /// `fopen(filename, "r")` (`:138`); `None` for NULL.
        #[must_use]
        pub fn open(filename: &CStr) -> Option<Self> {
            // SAFETY: `filename` and the mode are NUL-terminated and outlive
            // the call.
            NonNull::new(unsafe { libc::fopen(filename.as_ptr(), c"r".as_ptr()) }).map(|stream| {
                Self {
                    stream,
                    phrase: Box::new([0; FIELD]),
                    pinyin: Box::new([0; FIELD]),
                    freq: 0,
                }
            })
        }

        /// `feof(infile)` (`:144`, `:151`).
        pub fn eof(&mut self) -> bool {
            // SAFETY: the stream is open and owned.
            unsafe { libc::feof(self.stream.as_ptr()) != 0 }
        }

        /// `fscanf(infile, "%1023s %1023s %u", phrase, pinyin, &freq)`
        /// (`:145-146`): the conversions assigned, or `EOF` (negative).
        pub fn scan(&mut self) -> c_int {
            // SAFETY: the stream is open and owned; each buffer holds at
            // least the 1023 bytes and NUL a `%1023s` stores; `freq` is a
            // writable `guint32` for the `%u`.
            unsafe {
                libc::fscanf(
                    self.stream.as_ptr(),
                    c"%1023s %1023s %u".as_ptr(),
                    self.phrase.as_mut_ptr(),
                    self.pinyin.as_mut_ptr(),
                    &raw mut self.freq,
                )
            }
        }

        /// The `phrase` buffer as `%s` reads it.
        #[must_use]
        pub fn phrase(&self) -> &CStr {
            CStr::from_bytes_until_nul(&self.phrase[..]).unwrap_or_default()
        }

        /// The `pinyin` buffer as `%s` and `strlen` read it.
        #[must_use]
        pub fn pinyin(&self) -> &CStr {
            CStr::from_bytes_until_nul(&self.pinyin[..]).unwrap_or_default()
        }

        /// `freq`.
        #[must_use]
        pub const fn freq(&self) -> u32 {
            self.freq
        }

        /// `g_utf8_strlen(phrase, -1)` (`:162`) over the buffer itself: a
        /// truncated sequence at the word's end steps past its NUL into the
        /// bytes an earlier, longer word left, as the pin's does.
        #[must_use]
        pub fn phrase_utf8_strlen(&self) -> c_long {
            // SAFETY: `%1023s` (or the zeroed buffer) leaves a NUL within the
            // first 1024 bytes, and `g_utf8_strlen` steps at most six bytes
            // past the last lead byte before it reads that NUL or one of the
            // eight zero bytes after the 1024, so every byte it reads is in
            // the buffer.
            unsafe { glib_sys::g_utf8_strlen(self.phrase.as_ptr().cast(), -1) }
        }

        /// `fclose(infile)` (`:156`).
        pub fn close(self) {
            drop(self);
        }
    }

    impl Drop for TableInput {
        fn drop(&mut self) {
            // SAFETY: the stream is open, owned and never used again.
            unsafe {
                libc::fclose(self.stream.as_ptr());
            }
        }
    }

    /// `Can't open file %s.\n` on `stderr` (`:140`).
    pub fn report_open_failed(filename: &CStr) {
        print_text(c_stderr(), c"Can't open file %s.\n", filename);
    }

    /// `Can't write file %s.\n` on `stderr` (`:263`).
    pub fn report_write_failed(filename: &CStr) {
        print_text(c_stderr(), c"Can't write file %s.\n", filename);
    }

    /// `printf("nnodes: %d\n", g_tree_nnodes(g_chewing_tree))` (`:114`).
    pub fn print_nnodes(nnodes: c_int) {
        // SAFETY: the format takes one `int`.
        unsafe { libc::printf(c"nnodes: %d\n".as_ptr(), nnodes) };
    }

    /// One of the item formats below, which take two strings and one
    /// 32-bit integer.
    fn report_item(format: &'static CStr, phrase: &CStr, pinyin: &CStr, freq: u32) {
        // SAFETY: `stderr` is open; the strings are NUL-terminated and
        // outlive the call; each caller's format takes two strings and one
        // 32-bit integer.
        unsafe {
            libc::fprintf(
                c_stderr(),
                format.as_ptr(),
                phrase.as_ptr(),
                pinyin.as_ptr(),
                freq,
            )
        };
    }

    /// `Too long phrase:%s\t%s\t%d\n` on `stderr` (`:169`).
    pub fn report_too_long(phrase: &CStr, pinyin: &CStr, freq: u32) {
        report_item(c"Too long phrase:%s\t%s\t%d\n", phrase, pinyin, freq);
    }

    /// `Invalid pinyin:%s\t%s\t%d\n` on `stderr` (`:186`).
    pub fn report_invalid_pinyin(phrase: &CStr, pinyin: &CStr, freq: u32) {
        report_item(c"Invalid pinyin:%s\t%s\t%d\n", phrase, pinyin, freq);
    }

    /// `Duplicate item: phrase:%s\tpinyin:%s\tfreq:%u\n` on `stderr`
    /// (`:215-216`).
    pub fn report_duplicate(phrase: &CStr, pinyin: &CStr, freq: u32) {
        report_item(
            c"Duplicate item: phrase:%s\tpinyin:%s\tfreq:%u\n",
            phrase,
            pinyin,
            freq,
        );
    }

    /// `gen_pinyin_table: invalid UTF-8 phrase: %s\n` on `stderr`: not a pin
    /// line. oxpinyin's report where the pin reads through the NULL
    /// `g_utf8_to_ucs4` left for `phrase` (`:174`, `:280-281`; class (b)).
    pub fn report_unconverted_phrase(phrase: &CStr) {
        print_text(
            c_stderr(),
            c"gen_pinyin_table: invalid UTF-8 phrase: %s\n",
            phrase,
        );
    }

    /// `store_one_item`'s `g_array_append_val(g_item_array[0], item)` for a
    /// phrase of length zero (`:117`, `:240`): the array is NULL, so GLib's
    /// own precondition logs its critical and appends nothing.
    pub fn store_into_null_array() {
        let item: c_int = 0;
        // SAFETY: GLib refuses the NULL array before reading the item.
        unsafe {
            glib_sys::g_array_append_vals(
                core::ptr::null_mut(),
                (&raw const item).cast::<c_void>(),
                1,
            );
        }
    }

    /// `fprintf(outfile, "%s\t%s\t%d\t%d\n", pinyin_str, phrase_str, token,
    /// freq)` (`:314-316`); a `None` phrase is the NULL the pin would pass.
    pub fn print_row(
        output: &OutputStream,
        pinyin_str: &CStr,
        phrase_str: Option<&CStr>,
        token: c_int,
        freq: u32,
    ) {
        // SAFETY: the stream is open; the strings are NUL-terminated and
        // outlive the call, or NULL where the pin's would be; the format
        // takes two strings and two 32-bit integers.
        unsafe {
            libc::fprintf(
                output.as_ptr(),
                c"%s\t%s\t%d\t%d\n".as_ptr(),
                pinyin_str.as_ptr(),
                phrase_str.map_or(core::ptr::null(), CStr::as_ptr),
                token,
                freq,
            );
        }
    }
}

/// `gen_zhuyin_table`'s own calls (`utils/storage/gen_zhuyin_table.cpp`
/// at 074a2219).
pub mod gen_zhuyin_table {
    use super::{c_stderr, print_text, print_texts};
    use std::ffi::CStr;

    /// `fprintf(stderr, "wrong line format:%s\n", linebuf)` (:156-159).
    pub fn wrong_line(line: &CStr) {
        print_text(c_stderr(), c"wrong line format:%s\n", line);
    }

    /// Normal refusal where `atoi(strs[1])` would dereference NULL (:153).
    pub fn missing_frequency(line: &CStr) {
        print_texts(
            c_stderr(),
            c"gen_zhuyin_table: %s: %s\n",
            c"missing frequency",
            line,
        );
    }

    /// Normal refusal where `g_ucs4_to_utf8` would read a NULL phrase
    /// (:286-287).
    pub fn unconverted_phrase(phrase: &CStr) {
        print_texts(
            c_stderr(),
            c"gen_zhuyin_table: %s: %s\n",
            c"invalid UTF-8 phrase",
            phrase,
        );
    }
}

/// `k_mixture_model_to_interpolation`'s calls
/// (`utils/training/k_mixture_model_to_interpolation.cpp` at 074a2219).
pub mod k_mixture_model_to_interpolation {
    use super::{OutputStream, c_int, c_long, c_stderr, print_literal, print_texts};
    use std::ffi::CStr;

    /// `fprintf(output, "\\data model interpolation\n")` (`:74`).
    pub fn print_header(output: &OutputStream) {
        print_literal(output.as_ptr(), c"\\data model interpolation\n");
    }

    /// `fprintf(output, "\\end\n")` (`:91`).
    pub fn print_end(output: &OutputStream) {
        print_literal(output.as_ptr(), c"\\end\n");
    }

    /// `fprintf(output, "\\1-gram\n")` (`:94`).
    pub fn print_gram_1(output: &OutputStream) {
        print_literal(output.as_ptr(), c"\\1-gram\n");
    }

    /// `fprintf(output, "\\2-gram\n")` (`:99`).
    pub fn print_gram_2(output: &OutputStream) {
        print_literal(output.as_ptr(), c"\\2-gram\n");
    }

    /// `fprintf(output, "\\item %d %s count %ld\n", token, word, freq)`
    /// (`:134`).
    pub fn print_unigram(output: &OutputStream, token: c_int, word: &CStr, freq: c_long) {
        // SAFETY: the stream is open; the word is NUL-terminated and
        // outlives the call; the format takes an `int`, a string and a
        // `long`.
        unsafe {
            libc::fprintf(
                output.as_ptr(),
                c"\\item %d %s count %ld\n".as_ptr(),
                token,
                word.as_ptr(),
                freq,
            );
        }
    }

    /// `fprintf(output, "\\item %d %s %d %s count %ld\n", ...)`
    /// (`:170-171`).
    pub fn print_bigram(
        output: &OutputStream,
        token1: c_int,
        word1: &CStr,
        token2: c_int,
        word2: &CStr,
        count: c_long,
    ) {
        // SAFETY: as `print_unigram`; the format takes an `int`, a string, an
        // `int`, a string and a `long`.
        unsafe {
            libc::fprintf(
                output.as_ptr(),
                c"\\item %d %s %d %s count %ld\n".as_ptr(),
                token1,
                word1.as_ptr(),
                token2,
                word2.as_ptr(),
                count,
            );
        }
    }

    /// `error: k mixture model expected.\n` on `stderr` (`:62`, `:69`).
    pub fn report_model_expected() {
        print_literal(c_stderr(), c"error: k mixture model expected.\n");
    }

    /// `empty file input.\n` on `stderr` (`:199`).
    pub fn report_empty_input() {
        print_literal(c_stderr(), c"empty file input.\n");
    }

    /// `k_mixture_model_to_interpolation: <reason>: <line>\n` on `stderr`:
    /// not a pin line. oxpinyin's report where the pin's `taglib_read`
    /// reads past the line or through a NULL token (class (b)).
    pub fn refuse(reason: &CStr, line: &CStr) {
        print_texts(
            c_stderr(),
            c"k_mixture_model_to_interpolation: %s: %s\n",
            reason,
            line,
        );
    }
}

/// `ngseg`, `spseg` and `mergeseq`'s calls
/// (`utils/segment/ngseg.cpp`, `spseg.cpp` and `mergeseq.cpp` at
/// 074a2219).
///
/// `%s` receives a NULL where the pin passes one: the C library prints its
/// own spelling for that argument (glibc's `(null)`), so the call, not a
/// Rust substitution, decides it.
pub mod segmentation {
    use super::{OutputStream, c_char, c_int, c_stderr, print_literal, print_text};
    use std::ffi::CStr;

    fn text_pointer(text: Option<&CStr>) -> *const c_char {
        text.map_or(core::ptr::null(), CStr::as_ptr)
    }

    /// `fprintf(output, "%d %s\n", token, text)` (`spseg.cpp:183`,
    /// `mergeseq.cpp:141`).
    pub fn print_token_line(output: &OutputStream, token: c_int, text: Option<&CStr>) {
        // SAFETY: the stream is open; the format takes an `int` and one
        // string, present or NULL.
        unsafe {
            libc::fprintf(
                output.as_ptr(),
                c"%d %s\n".as_ptr(),
                token,
                text_pointer(text),
            );
        }
    }

    /// `fprintf(output, "%s\n", text)` (`mergeseq.cpp:170`).
    pub fn print_text_line(output: &OutputStream, text: Option<&CStr>) {
        // SAFETY: the stream is open; the format takes one string, present
        // or NULL.
        unsafe {
            libc::fprintf(output.as_ptr(), c"%s\n".as_ptr(), text_pointer(text));
        }
    }

    /// `fprintf(output, "%d \n", null_token)` (`ngseg.cpp:196`, `:202`,
    /// `:251`, `:258`; `spseg.cpp:277`, `:283`, `:334`, `:341`).
    pub fn print_null_line(output: &OutputStream) {
        // SAFETY: the stream is open; the format takes one `int`.
        unsafe {
            libc::fprintf(output.as_ptr(), c"%d \n".as_ptr(), 0 as c_int);
        }
    }

    /// `fprintf(stderr, "non-ucs4 characters encountered:%s.\n", linebuf)`
    /// (`ngseg.cpp:194`, `spseg.cpp:275`).
    pub fn report_non_ucs4(line: &CStr) {
        print_text(c_stderr(), c"non-ucs4 characters encountered:%s.\n", line);
    }

    /// `fprintf(stderr, "Un-segmentable sentence encountered:%s\n",
    /// tmp_string)` (`ngseg.cpp:86`); `tmp_string` is NULL when GLib could
    /// not convert the run.
    pub fn report_unsegmentable(text: Option<&CStr>) {
        // SAFETY: `stderr` is open; the format takes one string, present or
        // NULL.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"Un-segmentable sentence encountered:%s\n".as_ptr(),
                text_pointer(text),
            );
        }
    }

    /// `fprintf(stderr, "too many arguments.\n")` (`ngseg.cpp:132`,
    /// `spseg.cpp:227`, `mergeseq.cpp:226`).
    pub fn report_too_many_arguments() {
        print_literal(c_stderr(), c"too many arguments.\n");
    }

    /// `fprintf(stderr, "load table.conf failed.\n")` (`ngseg.cpp:149`,
    /// `spseg.cpp:244`, `mergeseq.cpp:243`).
    pub fn report_table_conf_failed() {
        print_literal(c_stderr(), c"load table.conf failed.\n");
    }

    /// `fprintf(stderr, "load %s failed!\n", binfile)`
    /// (`utils/utils_helper.h:87`); `binfile` is NULL when the row has no
    /// system filename.
    pub fn report_load_failed(filename: Option<&CStr>) {
        // SAFETY: `stderr` is open; the format takes one string, present or
        // NULL.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"load %s failed!\n".as_ptr(),
                text_pointer(filename),
            );
        }
    }

    /// `fprintf(stderr, "error: unknown token:%d.\n", token)`
    /// (`src/storage/tag_utility.cpp:397`, `:380`).
    pub fn report_unknown_token(token: c_int) {
        // SAFETY: `stderr` is open; the format takes one `int`.
        unsafe {
            libc::fprintf(c_stderr(), c"error: unknown token:%d.\n".as_ptr(), token);
        }
    }
}

/// The storage utilities' calls (`utils/storage/*.cpp` at 074a2219).
///
/// `gen_binary_files`, `import_interpolation` and `export_interpolation`
/// read the `.table` text with `fopen`/`fgets`/`fseek`/`fclose`, build
/// cwd-relative paths with `g_build_filename`, scan the four fields with
/// `g_ascii_strtoull` and `g_strlcpy`, save the per-library chunks with
/// `MemoryChunk::save`'s `open`/`write`/`fsync`/`close` sequence, and print
/// the pin's diagnostics and interpolation model to the C runtime's
/// streams. What a field or a line means stays with the utility.
pub mod storage {
    use super::{OutputStream, c_int, c_stderr, print_literal, print_text};
    use core::ffi::{c_char, c_void};
    use core::ptr::NonNull;
    use std::ffi::{CStr, CString};

    use libc::FILE;

    /// One input `.table` stream (`fopen(filename, "r")`), closed exactly
    /// once.
    pub struct TableFile {
        stream: NonNull<FILE>,
    }

    impl TableFile {
        /// `fopen(filename, "r")` (`gen_binary_files.cpp:61`, `:97`).
        #[must_use]
        pub fn open(filename: &CStr) -> Option<Self> {
            // SAFETY: `filename` and the mode literal are NUL-terminated and
            // outlive the call.
            NonNull::new(unsafe { libc::fopen(filename.as_ptr(), c"r".as_ptr()) })
                .map(|stream| Self { stream })
        }

        /// `fseek(tablefile, 0L, SEEK_SET)` (`gen_binary_files.cpp:69`,
        /// `:71`).
        pub fn rewind(&mut self) {
            // SAFETY: the stream is open and owned by this handle.
            unsafe { libc::fseek(self.stream.as_ptr(), 0, libc::SEEK_SET) };
        }

        /// One `fgets(line, sizeof(line), infile)` with the pin's
        /// `char line[4096]` array, as the C string it leaves (the bytes up
        /// to the first NUL).
        #[must_use]
        pub fn read_line(&mut self) -> Option<Vec<u8>> {
            let mut line = [0_u8; 4096];
            // SAFETY: the array holds `line.len()` bytes and the stream is
            // open; `fgets` stores at most that many including the NUL.
            let answer = unsafe {
                libc::fgets(
                    line.as_mut_ptr().cast::<c_char>(),
                    c_int::try_from(line.len()).unwrap_or(c_int::MAX),
                    self.stream.as_ptr(),
                )
            };
            if answer.is_null() {
                return None;
            }
            let end = line.iter().position(|byte| *byte == 0).unwrap_or(0);
            Some(line[..end].to_vec())
        }
    }

    impl Drop for TableFile {
        fn drop(&mut self) {
            // SAFETY: the stream is open and never used again.
            unsafe { libc::fclose(self.stream.as_ptr()) };
        }
    }

    /// `g_build_filename(dir, name, NULL)`, freed with `g_free`.
    #[must_use]
    pub fn build_filename(dir: Option<&CStr>, name: &CStr) -> Option<CString> {
        // SAFETY: the parts are NUL-terminated and outlive the call; the
        // answer is GLib's allocation, copied and freed here.
        let answer = unsafe {
            glib_sys::g_build_filename(
                dir.map_or(core::ptr::null(), CStr::as_ptr),
                name.as_ptr(),
                core::ptr::null::<c_char>(),
            )
        };
        let answer = NonNull::new(answer)?;
        // SAFETY: a non-NULL GLib string is NUL-terminated and only this
        // call reads and frees it.
        let bytes = unsafe { CStr::from_ptr(answer.as_ptr()) }
            .to_bytes()
            .to_vec();
        // SAFETY: as above; it is not used again.
        unsafe { glib_sys::g_free(answer.as_ptr().cast::<c_void>()) };
        CString::new(bytes).ok()
    }

    /// `g_ascii_strtoull(field, &end, 10)` with the pin's errno reset and
    /// the answers `table_text_parse_integer` tests: `None` when `errno`
    /// was set, nothing converted, or trailing bytes remained.
    #[must_use]
    pub fn ascii_strtoull(field: &CStr) -> Option<u64> {
        // SAFETY: the errno cell is this thread's; the call below sets it.
        unsafe { *errno_location() = 0 };
        let mut end: *mut c_char = core::ptr::null_mut();
        // SAFETY: `field` is NUL-terminated and outlives the call; `end`
        // receives a pointer into it.
        let parsed = unsafe { glib_sys::g_ascii_strtoull(field.as_ptr(), &raw mut end, 10) };
        // SAFETY: the errno cell is this thread's.
        let failed = unsafe { *errno_location() } != 0;
        if failed || end.is_null() || end == field.as_ptr().cast_mut() {
            return None;
        }
        // SAFETY: `end` points into `field`'s NUL-terminated bytes.
        if unsafe { *end } != 0 {
            return None;
        }
        Some(parsed)
    }

    /// `g_strlcpy(dest, field, 256) < 256`: the 255-byte field the pin
    /// keeps, `None` when the source does not fit.
    #[must_use]
    pub fn strlcpy_256(field: &CStr) -> Option<CString> {
        let mut buffer = [0_u8; 256];
        // SAFETY: the destination holds 256 bytes and the source is
        // NUL-terminated; GLib writes at most the destination size.
        let length = unsafe {
            glib_sys::g_strlcpy(
                buffer.as_mut_ptr().cast::<c_char>(),
                field.as_ptr(),
                buffer.len(),
            )
        };
        if length >= buffer.len() {
            return None;
        }
        CString::new(&buffer[..length]).ok()
    }

    /// The errno cell `g_ascii_strtoull` sets.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    unsafe fn errno_location() -> *mut c_int {
        // SAFETY: the caller makes the access; the function is the C
        // library's thread-local cell accessor.
        unsafe { libc::__errno_location() }
    }

    /// The errno cell `g_ascii_strtoull` sets.
    #[cfg(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "dragonfly"
    ))]
    unsafe fn errno_location() -> *mut c_int {
        // SAFETY: as the Linux definition.
        unsafe { libc::__error() }
    }

    /// The errno cell `g_ascii_strtoull` sets.
    #[cfg(windows)]
    unsafe fn errno_location() -> *mut c_int {
        // SAFETY: as the Linux definition.
        unsafe { libc::_errno() }
    }

    /// `O_BINARY`, defined as zero where the target has no text/binary
    /// distinction (`src/include/memory_chunk.h:39-40`).
    #[cfg(windows)]
    const O_BINARY: c_int = libc::O_BINARY;
    /// See the Windows definition.
    #[cfg(not(windows))]
    const O_BINARY: c_int = 0;

    /// `MemoryChunk::save` (`src/include/memory_chunk.h:536-557`): the
    /// `open(filename, O_CREAT|O_WRONLY|O_TRUNC|O_BINARY, 0644)` sequence,
    /// the length and checksum words, the payload write, `g_fsync` and
    /// `close`. The pin's answers are kept: a short header write aborts (its
    /// `assert`), a short payload write returns `false` after the close.
    pub fn save_chunk(filename: &CStr, length: u32, checksum: u32, payload: &[u8]) -> bool {
        // SAFETY: `filename` is NUL-terminated and outlives the call.
        let fd = unsafe {
            libc::open(
                filename.as_ptr(),
                libc::O_CREAT | libc::O_WRONLY | libc::O_TRUNC | O_BINARY,
                0o644,
            )
        };
        if fd < 0 {
            return false;
        }
        for word in [length.to_ne_bytes(), checksum.to_ne_bytes()] {
            // SAFETY: `fd` is open and `word` holds four bytes for a
            // four-byte `write`.
            let written = unsafe { libc::write(fd, word.as_ptr().cast::<c_void>(), word.len()) };
            if usize::try_from(written).ok() != Some(word.len()) {
                // `assert(ret_len == sizeof(guint32))`.
                // SAFETY: `abort` never returns.
                unsafe { libc::abort() };
            }
        }
        // SAFETY: `fd` is open and `payload` holds its length for the one
        // payload `write`.
        let written = unsafe { libc::write(fd, payload.as_ptr().cast::<c_void>(), payload.len()) };
        if usize::try_from(written).ok() != Some(payload.len()) {
            // SAFETY: `fd` is open and is closed once.
            unsafe { libc::close(fd) };
            return false;
        }
        // SAFETY: `fd` is open; the sync and close are the pin's.
        unsafe {
            sync_fd(fd);
            libc::close(fd);
        }
        true
    }

    /// `g_fsync(fd)` on this target: ordinary Unix GLib headers expand it
    /// to `fsync`; Windows GLib calls `_commit`.
    #[cfg(unix)]
    unsafe fn sync_fd(fd: c_int) {
        // SAFETY: the caller owns the open descriptor.
        unsafe { libc::fsync(fd) };
    }

    /// See the Unix definition.
    #[cfg(windows)]
    unsafe fn sync_fd(fd: c_int) {
        // SAFETY: the caller owns the open descriptor.
        unsafe { libc::_commit(fd) };
    }

    /// `fprintf(stderr, "open %s failed!\n", tablename)`
    /// (`gen_binary_files.cpp:64`, `:92`, `:99`).
    pub fn report_open_failed(filename: &CStr) {
        print_text(c_stderr(), c"open %s failed!\n", filename);
    }

    /// `fprintf(stderr, "save %s failed.", binfile)`
    /// (`utils/utils_helper.h:112`, `:137`), without a newline.
    pub fn report_save_failed(filename: &CStr) {
        print_text(c_stderr(), c"save %s failed.", filename);
    }

    /// `fprintf(stderr, "load %s failed!\n", binfile)`
    /// (`utils/utils_helper.h:87`); `None` is the pin's NULL name.
    pub fn report_load_failed(filename: Option<&CStr>) {
        // SAFETY: `stderr` is open; the format takes one string, present or
        // NULL.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"load %s failed!\n".as_ptr(),
                filename.map_or(core::ptr::null::<c_char>(), CStr::as_ptr),
            );
        }
    }

    /// `fprintf(stderr, "load table.conf failed.\n")`
    /// (`gen_binary_files.cpp:130`, `import_interpolation.cpp:255`,
    /// `export_interpolation.cpp:51`).
    pub fn report_table_conf_failed() {
        print_literal(c_stderr(), c"load table.conf failed.\n");
    }

    /// `fprintf(stderr, "error: interpolation model expected.\n")`
    /// (`import_interpolation.cpp:77`, `:85`).
    pub fn report_interpolation_expected() {
        print_literal(c_stderr(), c"error: interpolation model expected.\n");
    }

    /// `fprintf(stderr, "empty file input.\n")`
    /// (`import_interpolation.cpp:290`).
    pub fn report_empty_input() {
        print_literal(c_stderr(), c"empty file input.\n");
    }

    /// `fprintf(stderr, "error: unknown token:%d.\n", token)`
    /// (`src/storage/tag_utility.cpp:397`, `:380`).
    pub fn report_unknown_token(token: c_int) {
        // SAFETY: `stderr` is open; the format takes one `int`.
        unsafe {
            libc::fprintf(c_stderr(), c"error: unknown token:%d.\n".as_ptr(), token);
        }
    }

    /// `fprintf(stderr, "duplicated punctuations: %s\n", punct)`
    /// (`src/storage/punct_table.cpp:102`).
    pub fn report_duplicated_punctuation(punctuation: &CStr) {
        print_text(c_stderr(), c"duplicated punctuations: %s\n", punctuation);
    }

    /// `fprintf(stderr, "ChewingLargeTable2::load_text:%s\t%s\t%u\t%zu\n",
    /// ...)` (`src/storage/chewing_large_table2.cpp:145-147`).
    pub fn report_chewing_skip(pinyin: &CStr, phrase: &CStr, token: u32, freq: usize) {
        // SAFETY: `stderr` is open; the format takes two strings, an
        // unsigned int and a size_t.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"ChewingLargeTable2::load_text:%s\t%s\t%u\t%zu\n".as_ptr(),
                pinyin.as_ptr(),
                phrase.as_ptr(),
                token,
                freq,
            );
        }
    }

    /// `fprintf(stderr, "FacadePhraseIndex::load_text:%s\t%s\n", ...)`
    /// (`src/storage/phrase_index.cpp:581-582`).
    pub fn report_phrase_index_skip(pinyin: &CStr, phrase: &CStr) {
        // SAFETY: `stderr` is open; the format takes two strings.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"FacadePhraseIndex::load_text:%s\t%s\n".as_ptr(),
                pinyin.as_ptr(),
                phrase.as_ptr(),
            );
        }
    }

    /// `fprintf(output, "\\data model interpolation\n")`
    /// (`export_interpolation.cpp:34`).
    pub fn print_header(output: &OutputStream) {
        print_literal(output.as_ptr(), c"\\data model interpolation\n");
    }

    /// `fprintf(output, "\\end\n")` (`export_interpolation.cpp:39`).
    pub fn print_end(output: &OutputStream) {
        print_literal(output.as_ptr(), c"\\end\n");
    }

    /// `fprintf(output, "\\1-gram\n")` (`export_interpolation.cpp:76`).
    pub fn print_gram_1(output: &OutputStream) {
        print_literal(output.as_ptr(), c"\\1-gram\n");
    }

    /// `fprintf(output, "\\2-gram\n")` (`export_interpolation.cpp:107`).
    pub fn print_gram_2(output: &OutputStream) {
        print_literal(output.as_ptr(), c"\\2-gram\n");
    }

    /// `fprintf(output, "\\item %d %s count %ld\n", token, phrase, freq)`
    /// (`export_interpolation.cpp:98`).
    pub fn print_unigram(
        output: &OutputStream,
        token: c_int,
        word: &CStr,
        freq: core::ffi::c_long,
    ) {
        // SAFETY: the stream is open; the format takes an int, a string and
        // a long.
        unsafe {
            libc::fprintf(
                output.as_ptr(),
                c"\\item %d %s count %ld\n".as_ptr(),
                token,
                word.as_ptr(),
                freq,
            );
        }
    }

    /// `fprintf(output, "\\item %d %s %d %s count %d\n", ...)`
    /// (`export_interpolation.cpp:131-132`).
    pub fn print_bigram(
        output: &OutputStream,
        token1: c_int,
        word1: &CStr,
        token2: c_int,
        word2: &CStr,
        count: c_int,
    ) {
        // SAFETY: the stream is open; the format takes two ints, two
        // strings and an int.
        unsafe {
            libc::fprintf(
                output.as_ptr(),
                c"\\item %d %s %d %s count %d\n".as_ptr(),
                token1,
                word1.as_ptr(),
                token2,
                word2.as_ptr(),
                count,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn getline_keeps_the_pins_lines_and_its_last_line() {
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
        // The sentry refuses the stream at its end: the line stays.
        assert_eq!(input.line(), b"last");
        assert!(!input.getline());
        assert_eq!(input.line(), b"last");
        // After a newline-terminated last line, the next read erases it.
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
        let mut input = LineReader::open(&crate::c_path(&path).unwrap());
        assert!(input.is_open() && input.getline());
        assert_eq!(input.line(), b"x");
        let missing = LineReader::open(&crate::c_path(&dir.path().join("absent")).unwrap());
        assert!(!missing.is_open());
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
                OptionValue::Filename(Some(b"out.txt".to_vec())),
                OptionValue::Flag(true),
                OptionValue::Double(0.99),
            ]
        );
        assert_eq!(parsed.args, [b"ngseg".to_vec(), b"in.txt".to_vec()]);
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
    fn text_calls_are_glibs() {
        assert_eq!(
            strsplit_set(c"1 2\t3 4", c" \t", 2),
            [b"1".to_vec(), b"2\t3 4".to_vec()]
        );
        assert_eq!(utf8_strlen(c"你好"), 2);
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
        assert_eq!(utf8_next_char_step(0xE4), 3);
        assert_eq!(utf8_get_char_at(c"a\xe4\xbd\xa0", 1), Some(0x4F60));
        assert_eq!(utf8_get_char_at(c"a", 2), None);
    }
}
