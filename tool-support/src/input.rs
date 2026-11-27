//! Native input, output, and argument parsing for the command-line utilities. The helpers
//! own C string and stream lifetimes, typed formatting arguments, and the reusable
//! [`LineReader`] buffer.

// The native boundary is described in docs/findings/external-io-boundary.md. GLib
// supplies the option parser; libc supplies the streams.

use core::ffi::{c_char, c_int, c_long, c_void};
use core::ptr::NonNull;
use std::ffi::{CStr, CString, OsString};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use libc::FILE;

use oxpinyin::__internal::c_stderr;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    static mut stdout: *mut FILE;
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    #[link_name = "__stdoutp"]
    static mut stdout: *mut FILE;
}

#[cfg(windows)]
unsafe extern "C" {
    fn __acrt_iob_func(index: core::ffi::c_uint) -> *mut FILE;
}

/// The pointer `<stdio.h>`'s `stdout` expands to.
fn c_stdout() -> *mut FILE {
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

/// The target's `EINVAL`, the pin's exit status for a refused option or
/// argument (`ngseg.cpp:119`).
pub const EINVAL: c_int = libc::EINVAL;
/// The target's `ENOENT`, the pin's exit status for a file it cannot open
/// (`gen_pinyin_table.cpp:141`, `:264`).
pub const ENOENT: c_int = libc::ENOENT;
/// The target's `ENODATA`, the pin's exit status for an empty or wrong
/// model (`k_mixture_model_to_interpolation.cpp:200`, `:204`).
pub const ENODATA: c_int = libc::ENODATA;
/// The target's `EOVERFLOW`, `merge_k_mixture_model`'s exit status for a
/// magic header that would overflow (`merge_k_mixture_model.cpp:233`).
pub const EOVERFLOW: c_int = libc::EOVERFLOW;
/// The target's `EDOM`, `prune_k_mixture_model`'s exit status for a
/// probability outside `[0, 1]` (`prune_k_mixture_model.cpp:79`).
pub const EDOM: c_int = libc::EDOM;

/// `setlocale(LC_ALL, "")`, the first call of every utility's `main`
/// (074a2219 `utils/segment/ngseg.cpp:111`); whether it answered a name.
#[must_use]
pub fn set_locale_from_environment() -> bool {
    // SAFETY: the argument is a NUL-terminated literal; the answer is only
    // tested.
    !unsafe { libc::setlocale(libc::LC_ALL, c"".as_ptr()) }.is_null()
}

// Uses libc::exit as in the pin utilities.
/// Terminates the process with the supplied status through the C runtime.
///
/// # Aborts
///
/// Always exits. The C runtime runs its exit handlers and flushes streams.
pub fn exit(status: c_int) -> ! {
    // SAFETY: `exit` never returns.
    unsafe { libc::exit(status) }
}

// Failure behaviour in docs/findings/compatibility-policy.md specifies
// std::process::abort.
/// Terminates the process immediately.
///
/// # Aborts
///
/// Always aborts.
pub fn abort() -> ! {
    std::process::abort()
}

// Attach failures and truncated k-mixture array headers have no corresponding defined
// continuation. See class (b),
// docs/findings/upstream-divergences.md#flexiblebigram-malformed-record-lengths.
/// Reports a store failure to standard error and terminates the utility.
///
/// # Aborts
///
/// Prints one diagnostic line and exits with status 1.
pub fn exit_on_store_error(error: &dyn std::fmt::Display) -> ! {
    print_text(c_stderr(), c"%s\n", &store_error_line(error));
    exit(1)
}

/// The one line [`exit_on_store_error`] prints, without its newline.
fn store_error_line(error: &dyn std::fmt::Display) -> CString {
    let text = error.to_string().replace(['\n', '\r', '\0'], " ");
    CString::new(text.trim_end()).unwrap_or_default()
}

// perror, utils/segment/ngseg.cpp:124.
/// Prints the supplied prefix and the current C runtime error on standard error.
pub fn perror(s: &CStr) {
    // SAFETY: `s` is NUL-terminated and outlives the call.
    unsafe { libc::perror(s.as_ptr()) }
}

// atoi, utils/utils_helper.h:27–58.
/// Reads a decimal C `int` from the initial numeric portion of `str`. Supply a value
/// representable by `c_int`.
#[must_use]
pub fn atoi(str: &CStr) -> c_int {
    // SAFETY: `str` is NUL-terminated and outlives the call.
    unsafe { libc::atoi(str.as_ptr()) }
}

// atol, utils/utils_helper.h:60–69.
/// Reads a decimal C `long` from the initial numeric portion of `str`. Supply a value
/// representable by `c_long`.
#[must_use]
pub fn atol(str: &CStr) -> c_long {
    // SAFETY: as `atoi`.
    unsafe { libc::atol(str.as_ptr()) }
}

/// One `GOptionEntry`'s argument: its `GOptionArg` and the value its static
/// holds before parsing. The pin's utilities use these argument types.
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
    /// `G_OPTION_ARG_STRING`, converted by GLib to UTF-8.
    String(Option<&'static CStr>),
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
            Self::String(_) => glib_sys::G_OPTION_ARG_STRING,
            Self::Double(_) => glib_sys::G_OPTION_ARG_DOUBLE,
        }
    }

    fn default_string(&self) -> *mut c_char {
        if let Self::Filename(Some(text)) | Self::String(Some(text)) = self {
            text.as_ptr().cast_mut()
        } else {
            core::ptr::null_mut()
        }
    }

    fn raw(&self) -> RawValue {
        match *self {
            Self::Flag(flag) => RawValue {
                flag: c_int::from(flag),
            },
            Self::Int(integer) => RawValue { integer },
            Self::Double(number) => RawValue { number },
            Self::Filename(_) | Self::String(_) => RawValue {
                string: self.default_string(),
            },
        }
    }

    /// The slot after parsing.
    fn read(&self, value: RawValue) -> OptionValue {
        match self {
            Self::Flag(_) => {
                // SAFETY: this slot was initialised as a flag, and GLib writes a gboolean.
                OptionValue::Flag(unsafe { value.flag } != 0)
            }
            Self::Int(_) => {
                // SAFETY: this slot was initialised as an integer, and GLib writes a gint.
                OptionValue::Int(unsafe { value.integer })
            }
            Self::Double(_) => {
                // SAFETY: this slot was initialised as a number, and GLib writes a gdouble.
                OptionValue::Double(unsafe { value.number })
            }
            Self::Filename(default) | Self::String(default) => {
                // SAFETY: both option kinds initialise the string member;
                // GLib writes a gchar pointer to it.
                let string = unsafe { value.string };
                let text = if string == self.default_string() {
                    default.map(CStr::to_owned)
                } else {
                    take_glib_string(string)
                };
                if matches!(self, Self::Filename(_)) {
                    OptionValue::Filename(text)
                } else {
                    OptionValue::String(text)
                }
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
    /// A filename: GLib's, or the default the entry started at; `None`
    /// for a NULL default left unset.
    Filename(Option<CString>),
    /// UTF-8 text from `G_OPTION_ARG_STRING`, or the original default.
    String(Option<CString>),
}

/// What the option sequence left: each entry's value, in entry order, and
/// the arguments `g_option_context_parse` left in `argv`, `argv[0]`
/// included.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedOptions {
    /// The entries' values.
    pub values: Vec<OptionValue>,
    /// `argv[0..argc]` after parsing.
    pub args: Vec<CString>,
}

impl ParsedOptions {
    /// The flag entry `index` left.
    ///
    /// # Aborts
    ///
    /// Aborts when entry `index` is not a [`OptionArg::Flag`].
    #[must_use]
    pub fn flag(&self, index: usize) -> bool {
        match self.values[index] {
            OptionValue::Flag(value) => value,
            _ => abort(),
        }
    }

    /// The integer entry `index` left.
    ///
    /// # Aborts
    ///
    /// Aborts when entry `index` is not an [`OptionArg::Int`].
    #[must_use]
    pub fn int(&self, index: usize) -> i32 {
        match self.values[index] {
            OptionValue::Int(value) => value,
            _ => abort(),
        }
    }

    /// The double entry `index` left.
    ///
    /// # Aborts
    ///
    /// Aborts when entry `index` is not an [`OptionArg::Double`].
    #[must_use]
    pub fn double(&self, index: usize) -> f64 {
        match self.values[index] {
            OptionValue::Double(value) => value,
            _ => abort(),
        }
    }

    /// The filename entry `index` left, `None` for a NULL default left
    /// unset.
    ///
    /// # Aborts
    ///
    /// Aborts when entry `index` is not an [`OptionArg::Filename`].
    #[must_use]
    pub fn filename(&self, index: usize) -> Option<&CStr> {
        match &self.values[index] {
            OptionValue::Filename(value) => value.as_deref(),
            _ => abort(),
        }
    }

    /// The string entry `index` left, `None` for a NULL default left
    /// unset.
    ///
    /// # Aborts
    ///
    /// Aborts when entry `index` is not an [`OptionArg::String`].
    #[must_use]
    pub fn string(&self, index: usize) -> Option<&CStr> {
        match &self.values[index] {
            OptionValue::String(value) => value.as_deref(),
            _ => abort(),
        }
    }

    /// The arguments after `argv[0]`.
    #[must_use]
    pub fn operands(&self) -> &[CString] {
        &self.args[1..]
    }
}

/// `g_option_context_parse` failed and `option parsing failed:%s\n` has
/// been printed with `g_print`; the pin's caller then calls
/// `exit(EINVAL)` ([`exit`], [`EINVAL`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptionsFailed;

// g_option_context_new, add_main_entries and parse, ngseg.cpp:113–120 and
// gen_binary_files.cpp:114–121. GLib 2.84.4 goption.c:1585–1630 describes
// filename-string ownership.
/// Parses the utility's command line with GLib and returns the option values
/// and remaining arguments. Supply arguments starting with the program name.
///
/// # Errors
///
/// Returns [`OptionsFailed`] if the collected argument count exceeds `c_int`
/// or GLib rejects the options. GLib parse errors are printed on standard output.
///
/// # Aborts
///
/// GLib handles a help request by printing the help text and exiting the process.
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
    // SAFETY: the parameter string is statically allocated and NUL-terminated.
    let context = unsafe { glib_sys::g_option_context_new(parameter_string.as_ptr()) };
    // SAFETY: the context is live. Entry names and descriptions are static
    // C strings or NULL, and the zero-terminated array and value slots
    // that its entries point at outlive the context.
    unsafe {
        glib_sys::g_option_context_add_main_entries(context, options.as_ptr(), core::ptr::null())
    };
    // SAFETY: the context and its value slots are live. The writable argv
    // array holds argc valid C-string pointers followed by NULL; GLib may
    // permute and shorten the array. The error pointer starts as NULL.
    let parsed = unsafe {
        glib_sys::g_option_context_parse(context, &raw mut argc, &raw mut argv_ptr, &raw mut error)
    };
    if parsed == 0 {
        let format = c"option parsing failed:%s\n".as_ptr();
        // SAFETY: a failed parse sets the supplied out-parameter to a live GError.
        let message = unsafe { (*error).message };
        // SAFETY: the format takes one C string, and the error still owns
        // its NUL-terminated message.
        unsafe { glib_sys::g_print(format, message) };
        // SAFETY: GLib allocated this error and neither it nor its message is used again.
        unsafe { glib_sys::g_error_free(error) };
        // SAFETY: this context is still live and has no further users.
        unsafe { glib_sys::g_option_context_free(context) };
        return Err(OptionsFailed);
    }
    let count = usize::try_from(argc).unwrap_or(0);
    let mut args = Vec::with_capacity(count);
    for index in 0..count {
        // SAFETY: index is below argc, within the remaining argv array.
        let slot = unsafe { argv_ptr.add(index) };
        // SAFETY: this slot contains an initialised pointer and the array is still live.
        let arg = unsafe { *slot };
        if arg.is_null() {
            break;
        }
        // SAFETY: `arg` is one of those NUL-terminated strings, alive until
        // the context is freed.
        args.push(unsafe { CStr::from_ptr(arg) }.to_owned());
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

/// [`parse_options`], with a refused parse as the pin's `exit(EINVAL)`
/// after GLib's failure line (`ngseg.cpp:118-121`,
/// `gen_binary_files.cpp:119-121`).
///
/// # Aborts
///
/// As [`parse_options`]; a refused parse exits the process.
pub fn parse_options_or_exit(
    parameter_string: &'static CStr,
    entries: &[OptionEntry],
    args: impl IntoIterator<Item = OsString>,
) -> ParsedOptions {
    parse_options(parameter_string, entries, args).unwrap_or_else(|OptionsFailed| exit(EINVAL))
}

/// Copies a GLib-allocated string and frees it with `g_free`.
fn take_glib_string(string: *mut c_char) -> Option<CString> {
    if string.is_null() {
        return None;
    }
    // SAFETY: a non-NULL GLib string is NUL-terminated and only this call
    // reads and frees it.
    let text = unsafe { CStr::from_ptr(string) }.to_owned();
    // SAFETY: as above; it is not used again.
    unsafe { glib_sys::g_free(string.cast()) };
    Some(text)
}

/// `g_strsplit_set(string, delimiters, max_tokens)`
/// (`utils/utils_helper.h:27-46`), its strings copied out.
#[must_use]
pub fn strsplit_set(string: &CStr, delimiters: &CStr, max_tokens: c_int) -> Vec<CString> {
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let array = unsafe {
        glib_sys::g_strsplit_set(string.as_ptr(), delimiters.as_ptr().cast(), max_tokens)
    };
    let mut parts = Vec::new();
    if array.is_null() {
        return parts;
    }
    for index in 0.. {
        // SAFETY: this loop stops at the array's NULL terminator, so
        // index is within the GLib allocation, including that terminator.
        let slot = unsafe { array.add(index) };
        // SAFETY: GLib initialised the pointer in this slot; the array is still live.
        let part = unsafe { *slot };
        if part.is_null() {
            break;
        }
        // SAFETY: `part` is one of the array's strings.
        parts.push(unsafe { CStr::from_ptr(part) }.to_owned());
    }
    // SAFETY: the array came from `g_strsplit_set` and is not used again.
    unsafe { glib_sys::g_strfreev(array) };
    parts
}

/// Counts UTF-8 characters in one field, treating malformed sequences as
/// replacement characters.
#[must_use]
pub fn utf8_strlen(str: &CStr) -> c_long {
    let length = str
        .to_bytes()
        .utf8_chunks()
        .map(|chunk| chunk.valid().chars().count() + usize::from(!chunk.invalid().is_empty()))
        .sum::<usize>();
    length as c_long
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
    .map(CString::into_bytes)
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

/// A utility output stream backed by a file or standard output. Dropping it closes an
/// owned file. [`Self::close_including_stdout`] also closes standard output.
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
    pub fn open_write(
        filename: &(impl oxpinyin::__internal::input::FileName + ?Sized),
    ) -> Option<Self> {
        // SAFETY: `filename` and the mode are NUL-terminated and outlive the
        // call.
        oxpinyin::__internal::fopen(filename, c"w").map(|stream| Self {
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

    /// Close the output stream with `fclose(output)`, including C `stdout`
    /// (`ngseg.cpp:109,260`).
    ///
    /// # Safety
    ///
    /// For C `stdout`, the stream must still be open. No other code on any
    /// thread, including teardown and exit handlers, may use C `stdout`
    /// during or after this call.
    pub unsafe fn close_including_stdout(self) {
        let stream = self.stream;
        // The owned case would otherwise be closed again by `Drop`.
        core::mem::forget(self);
        // SAFETY: the stream is open, was never closed, and is not used
        // again. The caller guarantees this for C `stdout`.
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

/// A line reader with a reusable, NUL-terminated buffer.
pub struct LineReader<R> {
    reader: Option<R>,
    tied: bool,
    fail: bool,
    unterminated_line: bool,
    line: Vec<u8>,
    open_errno: Option<c_int>,
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
    // std::ifstream::open, ngseg.cpp:131.
    /// Opens an input file and returns a reader in its failed state if the open fails.
    /// [`Self::is_open`] distinguishes that state.
    #[must_use]
    pub fn open(filename: &(impl oxpinyin::__internal::input::FileName + ?Sized)) -> Self {
        let (file, open_errno) = match std::fs::File::open(filename.as_path()) {
            Ok(file) => (Some(file), None),
            Err(error) => (None, error.raw_os_error()),
        };
        let mut reader = Self::over(file.map(BufReader::new), false);
        reader.open_errno = open_errno;
        reader
    }
}

#[cfg(test)]
impl<'a> LineReader<&'a [u8]> {
    /// An `std::istringstream` over `text`: no tie, no file.
    fn from_bytes(text: &'a [u8]) -> Self {
        Self::over(Some(text), false)
    }
}

impl<R: BufRead> LineReader<R> {
    fn over(reader: Option<R>, tied: bool) -> Self {
        Self {
            fail: reader.is_none(),
            reader,
            tied,
            unterminated_line: false,
            line: vec![0],
            open_errno: None,
        }
    }

    /// The failing `open`'s `errno`, where `std::ifstream` would leave one
    /// (`gen_k_mixture_model.cpp:353-357`).
    #[must_use]
    pub fn open_error(&self) -> Option<c_int> {
        self.open_errno
    }

    /// `static_cast<bool>(stream)`: `!fail()`.
    #[must_use]
    pub fn is_open(&self) -> bool {
        !self.fail
    }

    /// Whether the last successful read ended at EOF without a newline.
    #[must_use]
    pub fn last_line_was_unterminated(&self) -> bool {
        self.unterminated_line
    }

    /// Reads the next line, which may end at EOF. A failed read clears the
    /// line buffer.
    pub fn getline(&mut self) -> bool {
        self.line.clear();
        let Some(reader) = self
            .reader
            .as_mut()
            .filter(|_| !self.fail && !self.unterminated_line)
        else {
            self.fail = true;
            self.line.push(0);
            return false;
        };
        if self.tied {
            // SAFETY: `stdout` is the C runtime's open stream.
            unsafe { libc::fflush(c_stdout()) };
        }
        match reader.read_until(b'\n', &mut self.line) {
            Ok(0) | Err(_) => {
                self.line.clear();
                self.fail = true;
            }
            Ok(_) if self.line.last() == Some(&b'\n') => {
                self.line.pop();
            }
            Ok(_) => self.unterminated_line = true,
        }
        self.line.push(0);
        !self.fail
    }

    /// Borrows the complete line bytes, including any embedded NUL. [`Self::c_str`] exposes
    /// only the prefix ending at the first NUL.
    #[must_use]
    pub fn line(&self) -> &[u8] {
        &self.line[..self.line.len() - 1]
    }

    // LineString::operator const gchar *, src/include/pinyin_utils.h:33–40.
    /// Borrows the line as a C string ending at its first NUL.
    #[must_use]
    pub fn c_str(&self) -> &CStr {
        CStr::from_bytes_until_nul(&self.line).unwrap_or_default()
    }
}

// tests/test_pinyin.cpp, test_phrase.cpp, test_chewing.cpp and test_zhuyin.cpp.
/// Standard-output prompts and result rows for the interactive C API utilities.
pub mod pin_tests {
    use super::*;

    fn text_pointer(text: Option<&CStr>) -> *const c_char {
        text.map_or(std::ptr::null(), CStr::as_ptr)
    }

    // test_pinyin.cpp:49–50.
    /// Prints and flushes the prefix prompt on standard output.
    pub fn prefix_prompt() {
        // SAFETY: the utility's stdout stream is open; the format takes no arguments.
        unsafe { libc::fprintf(c_stdout(), c"prefix:".as_ptr()) };
        // SAFETY: stdout remains open after printing the prompt.
        unsafe { libc::fflush(c_stdout()) };
    }

    // test_pinyin.cpp:55–56.
    /// Prints and flushes the pinyin prompt on standard output.
    pub fn pinyin_prompt() {
        // SAFETY: the utility's stdout stream is open; the format takes no arguments.
        unsafe { libc::fprintf(c_stdout(), c"pinyin:".as_ptr()) };
        // SAFETY: stdout remains open after printing the prompt.
        unsafe { libc::fflush(c_stdout()) };
    }

    // test_pinyin.cpp:73 and test_chewing.cpp:57 use printf("auxiliary text:%s\\n", …),
    // including a null result.
    /// Prints the auxiliary text and a newline on standard output. Supply a valid C string
    /// for the string conversion.
    pub fn auxiliary(text: Option<&CStr>) {
        // SAFETY: the literal consumes one C string. A null result follows
        // the same host printf call as the pin (the existing tools' rule).
        unsafe { libc::printf(c"auxiliary text:%s\n".as_ptr(), text_pointer(text)) };
    }

    // test_pinyin.cpp:86 and test_phrase.cpp:58.
    /// Prints a word followed by a tab on standard output. Supply a valid C string for the
    /// string conversion.
    pub fn word(text: Option<&CStr>) {
        // SAFETY: the literal consumes one live C string, or the pin's null.
        unsafe { libc::printf(c"%s\t".as_ptr(), text_pointer(text)) };
    }

    // test_chewing.cpp:51 and test_zhuyin.cpp:52.
    /// Prints a sentence followed by a newline on standard output.
    pub fn sentence(text: &CStr) {
        // SAFETY: the literal consumes one live NUL-terminated C string.
        unsafe { libc::printf(c"%s\n".as_ptr(), text.as_ptr()) };
    }

    // test_pinyin.cpp:88 and test_phrase.cpp:61.
    /// Ends the current output row with a newline.
    pub fn newline() {
        // SAFETY: this literal format consumes no arguments.
        unsafe { libc::printf(c"\n".as_ptr()) };
    }
}

// utils/storage/gen_pinyin_table.cpp.
/// File input, table output, and diagnostics for the pinyin table generator.
pub mod gen_pinyin_table {
    use super::{FILE, NonNull, OutputStream, c_int, c_stderr, c_void, print_text};
    use std::ffi::CStr;

    /// Space for one `%1023s` field and its terminator.
    const FIELD: usize = 1024;

    /// One input file of `feed_file` (`:134-157`) and the fields scanned
    /// from it. The stream is closed exactly once.
    pub struct TableInput {
        stream: NonNull<FILE>,
        phrase: Box<[u8; FIELD]>,
        pinyin: Box<[u8; FIELD]>,
        freq: u32,
    }

    impl TableInput {
        /// `fopen(filename, "r")` (`:138`); `None` for NULL.
        #[must_use]
        pub fn open(
            filename: &(impl oxpinyin::__internal::input::FileName + ?Sized),
        ) -> Option<Self> {
            // SAFETY: `filename` and the mode are NUL-terminated and outlive
            // the call.
            oxpinyin::__internal::fopen(filename, c"r").map(|stream| Self {
                stream,
                phrase: Box::new([0; FIELD]),
                pinyin: Box::new([0; FIELD]),
                freq: 0,
            })
        }

        /// `feof(infile)` (`:144`, `:151`).
        pub fn eof(&mut self) -> bool {
            // SAFETY: the stream is open and owned.
            unsafe { libc::feof(self.stream.as_ptr()) != 0 }
        }

        // fscanf(infile, "%1023s %1023s %u", …), utils/storage/gen_pinyin_table.cpp:145–146.
        /// Reads a phrase, a pinyin spelling and an unsigned frequency from the input file.
        /// Returns the number of assigned fields or the C end-of-file indicator.
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

    // The extra diagnostic precedes the pin's dereference of a NULL conversion result,
    // gen_pinyin_table.cpp:174,280–281 (class (b)).
    /// Reports an unconvertible phrase on standard error.
    pub fn report_unconverted_phrase(phrase: &CStr) {
        print_text(
            c_stderr(),
            c"gen_pinyin_table: invalid UTF-8 phrase: %s\n",
            phrase,
        );
    }

    // store_one_item calls g_array_append_val(g_item_array[0], item),
    // gen_pinyin_table.cpp:117,240.
    /// Performs the generator's empty-phrase append, invoking GLib's null-array diagnostic.
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

    // fprintf(outfile, "%s\\t%s\\t%d\\t%d\\n", …),
    // utils/storage/gen_pinyin_table.cpp:314–316.
    /// Writes a tab-separated pinyin spelling, phrase, token and frequency to the supplied
    /// output stream. Supply valid C strings for the textual fields.
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

// utils/storage/gen_zhuyin_table.cpp.
/// File input, output, and diagnostics for the Zhuyin table generator.
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

// utils/training/k_mixture_model_to_interpolation.cpp.
/// Input parsing and output for converting a k-mixture model to interpolation data.
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

    // The extra diagnostic marks a taglib_read overread or NULL token (class (b)).
    /// Prints the reason and the rejected input line on standard error.
    pub fn refuse(reason: &CStr, line: &CStr) {
        print_texts(
            c_stderr(),
            c"k_mixture_model_to_interpolation: %s: %s\n",
            reason,
            line,
        );
    }
}

// utils/segment/ngseg.cpp, spseg.cpp and mergeseq.cpp pass their arguments to the host C
// library, including NULL for some %s conversions.
/// Input and output for the segmentation and sequence-merging utilities.
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
        pub fn open(
            filename: &(impl oxpinyin::__internal::input::FileName + ?Sized),
        ) -> Option<Self> {
            // SAFETY: `filename` and the mode literal are NUL-terminated and
            // outlive the call.
            oxpinyin::__internal::fopen(filename, c"r").map(|stream| Self { stream })
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
        // SAFETY: the accessor is called on the thread whose errno will be reset.
        let errno = unsafe { errno_location() };
        // SAFETY: this is the current thread's writable errno cell.
        unsafe { *errno = 0 };
        let mut end: *mut c_char = core::ptr::null_mut();
        // SAFETY: `field` is NUL-terminated and outlives the call; `end`
        // receives a pointer into it.
        let parsed = unsafe { glib_sys::g_ascii_strtoull(field.as_ptr(), &raw mut end, 10) };
        // SAFETY: the accessor is called on the thread that just performed the conversion.
        let errno = unsafe { errno_location() };
        // SAFETY: this is the current thread's initialised errno cell.
        let failed = unsafe { *errno } != 0;
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
    #[cfg(target_os = "linux")]
    unsafe fn errno_location() -> *mut c_int {
        // SAFETY: the caller makes the access; the function is the C
        // library's thread-local cell accessor.
        unsafe { libc::__errno_location() }
    }

    /// The errno cell `g_ascii_strtoull` sets.
    #[cfg(target_os = "macos")]
    unsafe fn errno_location() -> *mut c_int {
        // SAFETY: as the Linux definition.
        unsafe { libc::__error() }
    }

    /// The errno cell `g_ascii_strtoull` sets.
    #[cfg(windows)]
    unsafe fn errno_location() -> *mut c_int {
        // SAFETY: as the Linux definition.
        unsafe { oxpinyin::__internal::errno_location() }
    }

    /// The C call converts size_t to unsigned int modulo 2^32 on Windows.
    #[cfg(all(test, windows))]
    fn write_count(count: usize) -> libc::c_uint {
        count as libc::c_uint
    }

    #[cfg(all(test, windows, target_pointer_width = "64"))]
    #[test]
    fn windows_write_count_is_the_c_unsigned_conversion() {
        assert_eq!(write_count(0), 0);
        assert_eq!(write_count(u32::MAX as usize), u32::MAX);
        assert_eq!(write_count(u32::MAX as usize + 1), 0);
        assert_eq!(write_count(1_usize << 32), 0);
    }

    #[cfg(all(test, windows))]
    #[test]
    fn windows_errno_cell_is_set_by_the_crt() {
        // SAFETY: the cell belongs to this thread; strtoul accepts a live C
        // string and a null end pointer. Restore the cell before asserting.
        let error = unsafe {
            let cell = errno_location();
            let saved = *cell;
            *cell = 0;
            libc::strtoul(c"4294967296".as_ptr(), core::ptr::null_mut(), 10);
            let error = *cell;
            *cell = saved;
            error
        };
        assert_eq!(error, libc::ERANGE);
    }

    /// `O_BINARY` (`src/include/memory_chunk.h:39-40`).
    #[cfg(all(test, windows))]
    const O_BINARY: c_int = libc::O_BINARY;

    /// `MemoryChunk::save` (`src/include/memory_chunk.h:536-557`): the
    /// `open(filename, O_CREAT|O_WRONLY|O_TRUNC|O_BINARY, 0644)` sequence,
    /// the length and checksum words, the payload write, `g_fsync` and
    /// `close`. The pin's answers are kept: a short header write aborts (its
    /// `assert`), a short payload write returns `false` after the close.
    #[cfg(all(test, windows))]
    #[must_use]
    pub fn save_chunk(
        filename: &(impl oxpinyin::__internal::input::FileName + ?Sized),
        length: u32,
        checksum: u32,
        payload: &[u8],
    ) -> bool {
        let filename = oxpinyin::__internal::wide_path(&filename.as_path());
        use libc::wopen as open;
        let flags = libc::O_CREAT | libc::O_WRONLY | libc::O_TRUNC | O_BINARY;
        // The CRT descriptor is not inherited by children.
        let flags = flags | libc::O_NOINHERIT;
        // SAFETY: `filename` is NUL-terminated and outlives the call.
        let fd = unsafe { open(filename.as_ptr(), flags, 0o644) };
        if fd < 0 {
            return false;
        }
        for word in [length.to_ne_bytes(), checksum.to_ne_bytes()] {
            // SAFETY: `fd` is open and `word` holds four bytes for a
            // four-byte `write`.
            let written =
                unsafe { libc::write(fd, word.as_ptr().cast::<c_void>(), write_count(word.len())) };
            if usize::try_from(written).ok() != Some(word.len()) {
                // `assert(ret_len == sizeof(guint32))`.
                // SAFETY: `abort` never returns.
                unsafe { libc::abort() };
            }
        }
        // SAFETY: `fd` is open and `payload` holds its length for the one
        // payload `write`.
        let written = unsafe {
            libc::write(
                fd,
                payload.as_ptr().cast::<c_void>(),
                write_count(payload.len()),
            )
        };
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

    /// `g_fsync(fd)`: Windows GLib calls `_commit`.
    #[cfg(all(test, windows))]
    unsafe fn sync_fd(fd: c_int) {
        // SAFETY: the caller owns the open descriptor.
        unsafe { libc::commit(fd) };
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
    pub fn print_unigram(output: &OutputStream, token: c_int, word: &CStr, freq: i64) {
        // The Linux pin's long holds every u32 frequency (:93-98).
        // Windows long is narrower; use its 64-bit printf conversion.
        #[cfg(windows)]
        let format = c"\\item %d %s count %lld\n";
        #[cfg(not(windows))]
        let format = c"\\item %d %s count %ld\n";
        // SAFETY: the stream is open; the format takes an int, a string and
        // a signed 64-bit integer on the selected platform.
        unsafe {
            libc::fprintf(output.as_ptr(), format.as_ptr(), token, word.as_ptr(), freq);
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

/// The n-gram and interpolation utilities' calls (`utils/training/*.cpp` at
/// 074a2219).
///
/// `estimate_interpolation` and `eval_correction_rate` print their model
/// values and diagnostics with the C library's formatted output, so the
/// `%f` spelling — `-nan` included — and `%d`'s reading of the token word
/// are the pin's.
pub mod training {
    use super::{c_int, c_stderr, print_literal, print_text};
    use std::ffi::CStr;

    /// `printf("token:%d lambda:%f\n", *token, lambda)`
    /// (`estimate_interpolation.cpp:129`): `%d` reads the `guint32` token's
    /// four bytes as the `int` it is passed as.
    pub fn print_token_lambda(token: u32, lambda: f64) {
        // SAFETY: the format takes an int and a double, which are the
        // argument types passed.
        unsafe { libc::printf(c"token:%d lambda:%f\n".as_ptr(), token as c_int, lambda) };
    }

    /// `printf("average lambda:%f\n", lambda_sum/lambda_count)`
    /// (`estimate_interpolation.cpp:139`). The caller divides: a zero
    /// `lambda_count` reaches this call with the C `0.0/0` NaN.
    pub fn print_average_lambda(lambda: f64) {
        // SAFETY: the format takes one double.
        unsafe { libc::printf(c"average lambda:%f\n".as_ptr(), lambda) };
    }

    /// `printf("correction rate:%f\n", rate)`
    /// (`eval_correction_rate.cpp:203`).
    pub fn print_correction_rate(rate: f64) {
        // SAFETY: the format takes one double.
        unsafe { libc::printf(c"correction rate:%f\n".as_ptr(), rate) };
    }

    /// `fprintf(stderr, "test sentence:%s\n", sentence)`
    /// (`eval_correction_rate.cpp:119`).
    pub fn report_test_sentence(sentence: &CStr) {
        print_text(c_stderr(), c"test sentence:%s\n", sentence);
    }

    /// `fprintf(stderr, "guessed sentence:%s\n", guessed_sentence)`
    /// (`eval_correction_rate.cpp:120`).
    pub fn report_guessed_sentence(sentence: &CStr) {
        print_text(c_stderr(), c"guessed sentence:%s\n", sentence);
    }

    /// `fprintf(stderr, "the result mis-matches.\n")`
    /// (`eval_correction_rate.cpp:121`).
    pub fn report_mismatch() {
        print_literal(c_stderr(), c"the result mis-matches.\n");
    }

    /// `fprintf(stderr, "Can't open file:%s\n", evals_text)`
    /// (`eval_correction_rate.cpp:168`).
    pub fn report_cant_open_file(filename: &CStr) {
        print_text(c_stderr(), c"Can't open file:%s\n", filename);
    }
}

/// `GHashTable` over `g_direct_hash`/`g_direct_equal` keys and pointer
/// values — the pin's per-document tables (`gen_k_mixture_model.cpp:106-131`,
/// `:290-299`). Iteration is GLib's own order, which the pin's DBM writes
/// and exports preserve; a hand-written hash table would change it.
pub struct DirectHashTable {
    raw: NonNull<glib_sys::GHashTable>,
}

impl DirectHashTable {
    /// `g_hash_table_new(g_direct_hash, g_direct_equal)`.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: the hash and equality functions are GLib's own; the table
        // owns no key or value destructor.
        let raw = unsafe {
            glib_sys::g_hash_table_new(
                Some(glib_sys::g_direct_hash),
                Some(glib_sys::g_direct_equal),
            )
        };
        Self {
            raw: NonNull::new(raw).unwrap_or_else(|| {
                // GLib's `g_hash_table_new` aborts on allocation failure.
                std::process::abort()
            }),
        }
    }

    fn key(token: u32) -> glib_sys::gpointer {
        token as usize as glib_sys::gpointer
    }

    fn value(token: u32) -> glib_sys::gpointer {
        token as usize as glib_sys::gpointer
    }

    /// `g_hash_table_insert(table, GUINT_TO_POINTER(key),
    /// GUINT_TO_POINTER(value))`.
    pub fn insert(&mut self, token: u32, value: u32) {
        // SAFETY: the table is live; the key and value are the pointers GLib
        // stores, and a NULL pointer for zero is a permitted value.
        unsafe {
            glib_sys::g_hash_table_insert(self.raw.as_ptr(), Self::key(token), Self::value(value));
        }
    }

    /// `g_hash_table_lookup_extended`, which distinguishes an absent key from
    /// a NULL value.
    #[must_use]
    pub fn lookup(&self, token: u32) -> Option<u32> {
        let mut value: glib_sys::gpointer = core::ptr::null_mut();
        // SAFETY: the table is live and `value` is a writable pointer slot;
        // the original-key out-parameter is NULL, which GLib permits.
        let found = unsafe {
            glib_sys::g_hash_table_lookup_extended(
                self.raw.as_ptr(),
                Self::key(token),
                core::ptr::null_mut(),
                &raw mut value,
            )
        };
        (found != 0).then_some(value as usize as u32)
    }

    /// `g_hash_table_steal` (`gen_k_mixture_model.cpp:161`, `:188`).
    pub fn steal(&mut self, token: u32) {
        // SAFETY: the table is live; no destroy function is installed, so
        // stealing the entry leaks nothing.
        unsafe {
            glib_sys::g_hash_table_steal(self.raw.as_ptr(), Self::key(token));
        }
    }

    /// `g_hash_table_iter_next` over every key: GLib's order, which the
    /// pin's DBM insertion order follows.
    #[must_use]
    pub fn keys(&self) -> Vec<u32> {
        let mut iter = glib_sys::GHashTableIter {
            dummy1: core::ptr::null_mut(),
            dummy2: core::ptr::null_mut(),
            dummy3: core::ptr::null_mut(),
            dummy4: 0,
            dummy5: 0,
            dummy6: core::ptr::null_mut(),
        };
        // SAFETY: the table is live and `iter` is a writable iterator; the
        // iteration outlives no mutation.
        unsafe { glib_sys::g_hash_table_iter_init(&raw mut iter, self.raw.as_ptr()) };
        let mut keys = Vec::new();
        loop {
            let mut key: glib_sys::gpointer = core::ptr::null_mut();
            let mut value: glib_sys::gpointer = core::ptr::null_mut();
            // SAFETY: the iterator is initialised over the live table; the
            // two out-parameters are writable pointer slots.
            let more = unsafe {
                glib_sys::g_hash_table_iter_next(&raw mut iter, &raw mut key, &raw mut value)
            };
            if more == 0 {
                break;
            }
            keys.push(key as usize as u32);
        }
        keys
    }
}

impl Default for DirectHashTable {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DirectHashTable {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns one reference and does not use the table
        // after the unref.
        unsafe { glib_sys::g_hash_table_unref(self.raw.as_ptr()) };
    }
}

/// `G_OPTION_FLAG_REVERSE`, the flag `gen_k_mixture_model`'s
/// `--skip-pi-gram-training` carries (`gen_k_mixture_model.cpp:57`).
pub const OPTION_FLAG_REVERSE: c_int = glib_sys::G_OPTION_FLAG_REVERSE as c_int;

/// `strerror(errnum)`, copied out of the C library's static buffer.
#[must_use]
pub fn strerror(errnum: c_int) -> CString {
    // SAFETY: the C library accepts any c_int error number.
    let message = unsafe { libc::strerror(errnum) };
    // SAFETY: the C library returns a NUL-terminated string; the single-threaded
    // utility copies it before another strerror call can replace it.
    unsafe { CStr::from_ptr(message) }.to_owned()
}

// utils/training/*_k_mixture_model.cpp uses signed %d for guint32 fields and %f for
// format_f64.
/// Output and diagnostics for the k-mixture training utilities.
pub mod k_mixture_model {
    use super::{OutputStream, c_int, c_stderr, print_literal, print_text};
    use std::ffi::CStr;

    // validate_k_mixture_model.cpp:31, prune:133, export:39.
    /// Reports a missing model header on standard error.
    pub fn report_no_magic() {
        print_literal(c_stderr(), c"no magic header in k mixture model.\n");
    }

    // validate_k_mixture_model.cpp:37.
    /// Reports a zero word count in the model header on standard error.
    pub fn report_zero_word_count() {
        print_literal(
            c_stderr(),
            c"word count in magic header is unexpected zero.\n",
        );
    }

    // validate_k_mixture_model.cpp:42.
    /// Reports a zero total frequency in the model header on standard error.
    pub fn report_zero_total_freq() {
        print_literal(
            c_stderr(),
            c"total freq in magic header is unexpected zero.\n",
        );
    }

    // validate_k_mixture_model.cpp:47.
    /// Reports inconsistent word and frequency totals on standard error.
    pub fn report_word_count_mismatch() {
        print_literal(
            c_stderr(),
            c"the word count doesn't match the total freq.\n",
        );
    }

    /// `word count in magic header:%d\\n` (`validate:64`).
    pub fn print_expected_word_count(value: c_int) {
        // SAFETY: the format takes one `int`.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"word count in magic header:%d\n".as_ptr(),
                value,
            )
        };
    }

    /// `sum of word count in array headers:%d\\n` (`validate:66`).
    pub fn print_header_word_count_sum(value: c_int) {
        // SAFETY: the format takes one `int`.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"sum of word count in array headers:%d\n".as_ptr(),
                value,
            )
        };
    }

    /// `the sum differs from word count.\\n` (`validate:67`, `:130`).
    pub fn report_word_count_sum_differs() {
        print_literal(c_stderr(), c"the sum differs from word count.\n");
    }

    /// `total freq in magic header:%d\\n` (`validate:71`).
    pub fn print_expected_total_freq(value: c_int) {
        // SAFETY: the format takes one `int`.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"total freq in magic header:%d\n".as_ptr(),
                value,
            )
        };
    }

    /// `sum of freqs in array headers:%d\\n` (`validate:73`).
    pub fn print_header_freq_sum(value: c_int) {
        // SAFETY: the format takes one `int`.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"sum of freqs in array headers:%d\n".as_ptr(),
                value,
            )
        };
    }

    /// `the total freq differs from sum of freqs.\\n` (`validate:74`).
    pub fn report_total_freq_sum_differs() {
        print_literal(c_stderr(), c"the total freq differs from sum of freqs.\n");
    }

    /// `in the array header of token %d:\\n` (`validate:104`, `:112`).
    pub fn print_array_header_token(token: c_int) {
        // SAFETY: the format takes one `int`.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"in the array header of token %d:\n".as_ptr(),
                token,
            )
        };
    }

    /// `word count is zero but has array items.\\n` (`validate:105`).
    pub fn report_zero_count_with_items() {
        print_literal(c_stderr(), c"word count is zero but has array items.\n");
    }

    /// `both word count and freq are unexpected zero.\\n` (`validate:113-114`).
    pub fn report_zero_count_and_freq() {
        print_literal(
            c_stderr(),
            c"both word count and freq are unexpected zero.\n",
        );
    }

    /// `word count in array header:%d\\n` (`validate:127`).
    pub fn print_array_header_count(value: c_int) {
        // SAFETY: the format takes one `int`.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"word count in array header:%d\n".as_ptr(),
                value,
            )
        };
    }

    /// `sum of word count in array items:%d\\n` (`validate:128`).
    pub fn print_item_count_sum(value: c_int) {
        // SAFETY: the format takes one `int`.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"sum of word count in array items:%d\n".as_ptr(),
                value,
            )
        };
    }

    /// `wrong arguments.\\n` (`validate:153`, `prune:121`).
    pub fn report_wrong_arguments() {
        print_literal(c_stderr(), c"wrong arguments.\n");
    }

    /// `k mixture model validation failed.\\n` (`validate:163`, `:168`).
    pub fn report_validation_failed() {
        print_literal(c_stderr(), c"k mixture model validation failed.\n");
    }

    /// `the m_WC integer in magic header overflows.\\n` (`merge:110`,
    /// `gen:274`).
    pub fn report_magic_wc_overflow() {
        print_literal(c_stderr(), c"the m_WC integer in magic header overflows.\n");
    }

    /// `the m_total_freq in magic header overflows.\\n` (`merge:116`,
    /// `gen:304`).
    pub fn report_magic_total_freq_overflow() {
        print_literal(c_stderr(), c"the m_total_freq in magic header overflows.\n");
    }

    /// `fprintf(stderr, "*")` (`prune:144`).
    pub fn print_progress_star() {
        print_literal(c_stderr(), c"*");
    }

    /// `fprintf(stderr, "\n")` (`prune:175`).
    pub fn print_progress_newline() {
        print_literal(c_stderr(), c"\n");
    }

    /// `some wrong possibility is encountered:%f.\\n` (`prune:73-74`).
    pub fn report_wrong_possibility(remained: f64) {
        // SAFETY: the format takes one `double`.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"some wrong possibility is encountered:%f.\n".as_ptr(),
                remained,
            )
        };
    }

    /// `k:%d N:%d WC:%d n_0:%d n_1:%d\\n` (`prune:75-78`).
    pub fn print_prune_counts(k: c_int, n: c_int, wc: c_int, n_0: c_int, n_1: c_int) {
        // SAFETY: the format takes five `int`s.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"k:%d N:%d WC:%d n_0:%d n_1:%d\n".as_ptr(),
                k,
                n,
                wc,
                n_0,
                n_1,
            )
        };
    }

    /// `token:%d lambda:%f\\n` (`estimate:141`).
    pub fn print_token_lambda(token: c_int, lambda: f64) {
        // SAFETY: the format takes one `int` and one `double`.
        unsafe { libc::printf(c"token:%d lambda:%f\n".as_ptr(), token, lambda) };
    }

    /// `average lambda:%f\\n` (`estimate:152`).
    pub fn print_average_lambda(lambda: f64) {
        // SAFETY: the format takes one `double`.
        unsafe { libc::printf(c"average lambda:%f\n".as_ptr(), lambda) };
    }

    /// `error: k mixture model expected.\\n` (`import:78`, `:86`).
    pub fn report_model_expected() {
        print_literal(c_stderr(), c"error: k mixture model expected.\n");
    }

    /// `empty file input.\\n` (`import:303`).
    pub fn report_empty_input() {
        print_literal(c_stderr(), c"empty file input.\n");
    }

    /// `load table.conf failed.\\n` (`import:277`, `export:130`, `gen:332`).
    pub fn report_table_conf_load_failed() {
        print_literal(c_stderr(), c"load table.conf failed.\n");
    }

    /// `load %s failed!\\n` (`utils/utils_helper.h:87`).
    pub fn report_chunk_load_failed(filename: &CStr) {
        print_text(c_stderr(), c"load %s failed!\n", filename);
    }

    /// `open %s failed.\\n` (`export:144`); a NULL name is the C library's
    /// `(null)`.
    pub fn report_open_failed(filename: Option<&CStr>) {
        // SAFETY: `stderr` is open; the format takes one string, or the NULL
        // the pin passes for an unset filename.
        unsafe {
            libc::fprintf(
                c_stderr(),
                c"open %s failed.\n".as_ptr(),
                filename.map_or(core::ptr::null(), CStr::as_ptr),
            )
        };
    }

    /// `error: unknown token:%d.\\n` (`tag_utility.cpp:398`, `:376`).
    pub fn report_unknown_token(token: c_int) {
        // SAFETY: the format takes one `int`.
        unsafe { libc::fprintf(c_stderr(), c"error: unknown token:%d.\n".as_ptr(), token) };
    }

    /// `can't open file: %s.\\n` (`gen:355`).
    pub fn report_cant_open_file(filename: &CStr) {
        print_text(c_stderr(), c"can't open file: %s.\n", filename);
    }

    /// `error:%s.\\n` (`gen:356`).
    pub fn report_error(message: &CStr) {
        print_text(c_stderr(), c"error:%s.\n", message);
    }

    /// `\data model "k mixture model" count %d N %d total_freq %d\n`
    /// (`export:42-44`).
    pub fn print_data_header(output: &OutputStream, wc: c_int, n: c_int, total_freq: c_int) {
        // SAFETY: the stream is open; the format takes three `int`s.
        unsafe {
            libc::fprintf(
                output.as_ptr(),
                c"\\data model \"k mixture model\" count %d N %d total_freq %d\n".as_ptr(),
                wc,
                n,
                total_freq,
            )
        };
    }

    /// `\1-gram\\n` (`export:51`).
    pub fn print_gram_1(output: &OutputStream) {
        print_literal(output.as_ptr(), c"\\1-gram\n");
    }

    /// `\2-gram\\n` (`export:73`).
    pub fn print_gram_2(output: &OutputStream) {
        print_literal(output.as_ptr(), c"\\2-gram\n");
    }

    /// `\end\\n` (`export:108`).
    pub fn print_end(output: &OutputStream) {
        print_literal(output.as_ptr(), c"\\end\n");
    }

    /// `\item %d %s count %d freq %d\\n` (`export:61-62`).
    pub fn print_unigram_item(
        output: &OutputStream,
        token: c_int,
        phrase: &CStr,
        wc: c_int,
        freq: c_int,
    ) {
        // SAFETY: the stream is open; the format takes an `int`, a string and
        // two `int`s.
        unsafe {
            libc::fprintf(
                output.as_ptr(),
                c"\\item %d %s count %d freq %d\n".as_ptr(),
                token,
                phrase.as_ptr(),
                wc,
                freq,
            )
        };
    }

    /// `\item %d %s %d %s count %d T %d N_n_0 %d n_1 %d Mr %d\n`
    /// (`export:90-94`).
    #[allow(clippy::too_many_arguments)]
    pub fn print_bigram_item(
        output: &OutputStream,
        token1: c_int,
        word1: &CStr,
        token2: c_int,
        word2: &CStr,
        wc: c_int,
        n_n_0: c_int,
        n_1: c_int,
        mr: c_int,
    ) {
        // SAFETY: the stream is open; the strings are NUL-terminated and
        // outlive the call; the format takes two `int`s, two strings and four
        // `int`s.
        unsafe {
            libc::fprintf(
                output.as_ptr(),
                c"\\item %d %s %d %s count %d T %d N_n_0 %d n_1 %d Mr %d\n".as_ptr(),
                token1,
                word1.as_ptr(),
                token2,
                word2.as_ptr(),
                wc,
                wc,
                n_n_0,
                n_1,
                mr,
            )
        };
    }
}

#[cfg(test)]
mod tests;

// tests/storage/test_parser2.cpp:130–147.
/// Standard-output rows for the parser utility.
pub mod parser {
    use super::*;

    // tests/storage/test_parser2.cpp:130–147 use the format parsed %d chars, %d keys.
    /// Prints the consumed input length and key count on standard output.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "the pin's outward int/guint arguments to %d"
    )]
    pub fn print_counts(length: usize, keys: usize) {
        // SAFETY: both arguments have the format's promoted int type.
        unsafe {
            libc::printf(
                c"parsed %d chars, %d keys.\n".as_ptr(),
                length as c_int,
                keys as c_int,
            )
        };
    }

    /// The key's spelling and its `guint16` raw offsets, promoted to int.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "ChewingKeyRest fields are guint16 at the outward boundary"
    )]
    pub fn print_key(pinyin: &CStr, begin: usize, end: usize) {
        // SAFETY: the string lives through printf; the offsets are promoted ints.
        unsafe {
            libc::printf(
                c"%s %d %d\t".as_ptr(),
                pinyin.as_ptr(),
                c_int::from(begin as u16),
                c_int::from(end as u16),
            )
        };
    }

    /// The newline following the key/key-rest sequence.
    pub fn print_newline() {
        // SAFETY: the constant format has no arguments.
        unsafe { libc::printf(c"\n".as_ptr()) };
    }
}

/// Native file-open diagnostics of the interactive lookup programs at 074a2219.
pub mod phrase_tests {
    use std::ffi::CStr;

    /// tests_helper.h:68, including the native NULL filename spelling.
    pub fn open_failed(value: Option<&CStr>) {
        // SAFETY: stderr is live and %s receives the pin's pointer, possibly NULL.
        unsafe {
            libc::fprintf(
                super::c_stderr(),
                c"open %s failed!\n".as_ptr(),
                value.map_or(core::ptr::null::<core::ffi::c_char>(), CStr::as_ptr),
            )
        };
    }
}

/// Exact native formats from test_{phrase,chewing}_table.cpp and tests_helper.h.
pub mod table_tests {
    use core::ffi::{c_int, c_long};
    use std::ffi::CStr;

    /// SEARCH_CONTINUED prefix length.
    pub fn continued(length: usize) {
        // SAFETY: %ld receives a native long.
        unsafe {
            libc::printf(
                c"return continued information with length:%ld\n".as_ptr(),
                length as c_long,
            )
        };
    }
    /// One phrase-table token, with its trailing tab.
    pub fn token(value: u32) {
        // SAFETY: %d receives a native int.
        unsafe { libc::printf(c"token:%d\t".as_ptr(), value as c_int) };
    }
    /// The range/row terminator, a separate printf in the source.
    pub fn newline() {
        // SAFETY: a literal with no conversions.
        unsafe { libc::printf(c"\n".as_ptr()) };
    }
    /// Empty-key parse diagnostic.
    pub fn invalid() {
        // SAFETY: stderr is live; the literal has no conversions.
        unsafe { libc::fprintf(super::c_stderr(), c"Invalid input.\n".as_ptr()) };
    }
    /// A library's range count.
    pub fn range_count(value: usize) {
        // SAFETY: %d receives a native int.
        unsafe { libc::printf(c"range items number:%d\n".as_ptr(), value as c_int) };
    }
    /// Half-open token range.
    pub fn range(begin: u32, end: u32) {
        // SAFETY: both %d conversions receive native ints.
        unsafe { libc::printf(c"start:%d\tend:%d\n".as_ptr(), begin as c_int, end as c_int) };
    }
    /// Phrase text followed by a tab.
    pub fn phrase(text: &CStr) {
        // SAFETY: %s receives a live NUL-terminated string.
        unsafe { libc::printf(c"%s\t".as_ptr(), text.as_ptr()) };
    }
    /// Each pronunciation key retains the source's final apostrophe.
    pub fn pronunciation(text: &CStr) {
        // SAFETY: %s receives a live NUL-terminated string.
        unsafe { libc::printf(c"%s'".as_ptr(), text.as_ptr()) };
    }
    /// The backspace is an output byte, not removal of the apostrophe.
    pub fn frequency(value: u32) {
        // SAFETY: %d receives a native int.
        unsafe { libc::printf(c"\x08\t%d\t".as_ptr(), value as c_int) };
    }
}

// The matrix and direct lookup test programs use the native printf formats below.
/// Standard-output traces of matrix columns and lookup searches.
pub mod lookup_tests {
    use super::*;
    /// Prints a physical column position on standard output.
    pub fn column(position: usize) {
        // SAFETY: %ld receives the source's native long value.
        unsafe { libc::printf(c"Column:%ld:\n".as_ptr(), position as libc::c_long) };
    }
    /// Prints the key spelling and its raw span on standard output.
    pub fn key(text: &CStr, begin: u16, end: u16) {
        // SAFETY: %s is live; promoted short arguments use native int.
        unsafe {
            libc::printf(
                c"ChewingKey:%s\nChewingKeyRest:%hd\t%hd\n".as_ptr(),
                text.as_ptr(),
                begin as i16 as c_int,
                end as i16 as c_int,
            )
        };
    }
    /// Prints the consumed byte count and number of keys on standard output.
    pub fn parsed(bytes: usize, keys: usize) {
        // SAFETY: %d arguments are native ints.
        unsafe {
            libc::printf(
                c"parsed %d chars, %d keys.\n".as_ptr(),
                bytes as c_int,
                keys as c_int,
            )
        };
    }
    /// Prints a matrix search's start and end positions on standard output.
    pub fn search(start: usize, end: usize) {
        // SAFETY: %ld arguments are native longs.
        unsafe {
            libc::printf(
                c"search index: start %ld\t end %ld\n".as_ptr(),
                start as libc::c_long,
                end as libc::c_long,
            )
        };
    }
    /// Non-null MatchResult entry.
    pub fn token(position: usize, token: u32) {
        // SAFETY: %ld receives long and %d int.
        unsafe {
            libc::printf(
                c"pos:%ld,token:%d\t".as_ptr(),
                position as libc::c_long,
                token as c_int,
            )
        };
    }
    /// Line separator.
    pub fn newline() {
        // SAFETY: constant format with no conversions.
        unsafe { libc::printf(c"\n".as_ptr()) };
    }
    /// A converted result and terminating newline.
    pub fn sentence(text: &CStr) {
        // SAFETY: %s receives a live NUL-terminated result.
        unsafe { libc::printf(c"%s\n".as_ptr(), text.as_ptr()) };
    }
    /// Failed GLib character-count check.
    pub fn non_ucs4() {
        // SAFETY: stderr is live; constant format has no conversions.
        unsafe {
            libc::fprintf(
                super::c_stderr(),
                c"non-ucs4 characters are not accepted.\n".as_ptr(),
            )
        };
    }
    /// NULL conversion result (including an empty sentence).
    pub fn unsegmentable() {
        // SAFETY: stderr is live; constant format has no conversions.
        unsafe {
            libc::fprintf(
                super::c_stderr(),
                c"Error: Un-segmentable sentence encountered!\n".as_ptr(),
            )
        };
    }
}

/// `path` as the C string `fopen` receives: its bytes as the platform
/// encodes them, or `None` when they hold a NUL, which no C path can.
#[must_use]
pub fn c_path(path: &std::path::Path) -> Option<CString> {
    CString::new(oxpinyin::__internal::input::path_to_bytes(path)).ok()
}

/// A pin byte path as the Rust path that opens it, `None` where the
/// platform cannot represent the bytes.
///
/// Unix accepts every byte. Windows requires UTF-8, as the pin's tools do
/// for a `CStr` filename; invalid input names no path. "Checked" refers to
/// the encoding alone: NULs, existence and filename validity are not
/// examined.
#[must_use]
#[cfg_attr(
    unix,
    expect(
        clippy::unnecessary_wraps,
        reason = "Unix accepts all bytes; Windows can reject a path that is not UTF-8"
    )
)]
pub fn path_from_bytes_checked(bytes: &[u8]) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        Some(oxpinyin::__internal::input::path_from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(bytes).ok().map(PathBuf::from)
    }
}
