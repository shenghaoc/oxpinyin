//! The declarations of `native/tool.c` and `native/tool_stream.cc` and the
//! owned handles over what they return: a C++ input stream, a `FILE *`
//! output stream and GLib's allocations. Every function here makes exactly
//! one shim call; no raw pointer leaves the module.

use core::ffi::{c_char, c_int, c_long, c_void};
use core::marker::{PhantomData, PhantomPinned};
use core::ptr::NonNull;
use std::ffi::CStr;

use crate::native::File;

/// The shim's `struct oxpinyin_tool_istream`, never dereferenced here.
#[repr(C)]
pub(crate) struct IstreamHandle {
    _opaque: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// The shim's `struct oxpinyin_tool_option`: one `GOptionEntry` without its
/// `arg_data`, which the shim points at the matching value.
#[repr(C)]
pub(crate) struct OptionSpec {
    pub(crate) long_name: *const c_char,
    pub(crate) short_name: c_char,
    pub(crate) flags: c_int,
    pub(crate) arg: c_int,
    pub(crate) description: *const c_char,
    pub(crate) arg_description: *const c_char,
}

/// The shim's `union oxpinyin_tool_option_value`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union OptionValue {
    pub(crate) flag: c_int,
    pub(crate) string: *mut c_char,
    pub(crate) integer: c_int,
    pub(crate) number: f64,
}

unsafe extern "C" {
    fn oxpinyin_tool_setlocale_all_environment() -> *mut c_char;
    fn oxpinyin_tool_parse_options(
        parameter_string: *const c_char,
        options: *const OptionSpec,
        count: usize,
        values: *mut OptionValue,
        argc: *mut c_int,
        argv: *mut *mut *mut c_char,
    ) -> c_int;
    fn oxpinyin_tool_g_free(memory: *mut c_void);
    fn oxpinyin_tool_atoi(str: *const c_char) -> c_int;
    fn oxpinyin_tool_atol(str: *const c_char) -> c_long;
    fn oxpinyin_tool_strsplit_set(
        string: *const c_char,
        delimiters: *const c_char,
        max_tokens: c_int,
    ) -> *mut *mut c_char;
    fn oxpinyin_tool_strfreev(str_array: *mut *mut c_char);
    fn oxpinyin_tool_utf8_strlen(str: *const c_char) -> c_long;
    fn oxpinyin_tool_utf8_to_ucs4(str: *const c_char, items_written: *mut c_long) -> *mut u32;
    fn oxpinyin_tool_ucs4_to_utf8(str: *const u32, len: c_long) -> *mut c_char;
    fn oxpinyin_tool_unichar_isspace(c: u32) -> c_int;
    fn oxpinyin_tool_unichar_isgraph(c: u32) -> c_int;
    fn oxpinyin_tool_stdout() -> *mut File;
    fn oxpinyin_tool_fopen_write(filename: *const c_char) -> *mut File;
    fn oxpinyin_tool_fclose(stream: *mut File) -> c_int;
    fn oxpinyin_tool_perror(s: *const c_char);
    fn oxpinyin_tool_exit(status: c_int) -> !;
    fn oxpinyin_tool_einval() -> c_int;
    fn oxpinyin_tool_istream_cin() -> *mut IstreamHandle;
    fn oxpinyin_tool_istream_open(filename: *const c_char) -> *mut IstreamHandle;
    fn oxpinyin_tool_istream_good(stream: *const IstreamHandle) -> c_int;
    fn oxpinyin_tool_istream_getline(
        stream: *mut IstreamHandle,
        data: *mut *const c_char,
        size: *mut usize,
    ) -> c_int;
    fn oxpinyin_tool_istream_close(stream: *mut IstreamHandle);
}

/// `setlocale(LC_ALL, "")`; whether it answered a name.
pub(crate) fn setlocale_all_environment() -> bool {
    // SAFETY: the shim passes its own literal; the answer is only tested.
    !unsafe { oxpinyin_tool_setlocale_all_environment() }.is_null()
}

/// The option sequence: the `argv[0..argc]` GLib left, or `None` when
/// `g_option_context_parse` failed. `argv` holds `argc` pointers to
/// NUL-terminated strings and a final NULL.
pub(crate) fn parse_options(
    parameter_string: &CStr,
    options: &[OptionSpec],
    values: &mut [OptionValue],
    argc: &mut c_int,
    argv: &mut [*mut c_char],
) -> Option<Vec<Vec<u8>>> {
    debug_assert_eq!(options.len(), values.len());
    let mut argv_ptr = argv.as_mut_ptr();
    // SAFETY: every name and description is NUL-terminated or NULL and
    // outlives the process-long context, because the callers pass
    // `'static` strings; `values` has one slot per option, of the type its
    // `arg` names, and outlives the call, after which GLib no longer writes
    // it; `argv` is `argc` valid C strings and a NULL, which GLib permutes
    // and shortens in place.
    let parsed = unsafe {
        oxpinyin_tool_parse_options(
            parameter_string.as_ptr(),
            options.as_ptr(),
            options.len(),
            values.as_mut_ptr(),
            argc,
            &raw mut argv_ptr,
        )
    };
    if parsed == 0 {
        return None;
    }
    let count = usize::try_from(*argc).unwrap_or(0);
    let mut args = Vec::with_capacity(count);
    for index in 0..count {
        // SAFETY: GLib leaves `argv_ptr` pointing at `*argc` valid strings
        // (the caller's, or GLib's own where it converts them).
        let arg = unsafe { *argv_ptr.add(index) };
        if arg.is_null() {
            break;
        }
        // SAFETY: `arg` is one of those NUL-terminated strings, alive for
        // the caller's borrow.
        args.push(unsafe { CStr::from_ptr(arg) }.to_bytes().to_vec());
    }
    Some(args)
}

/// A parsed `G_OPTION_ARG_NONE` slot.
pub(crate) fn option_flag(value: OptionValue) -> bool {
    // SAFETY: the slot was initialised as `flag` and GLib writes a
    // `gboolean` there.
    unsafe { value.flag != 0 }
}

/// A parsed `G_OPTION_ARG_INT` slot.
pub(crate) fn option_int(value: OptionValue) -> c_int {
    // SAFETY: the slot was initialised as `integer` and GLib writes a
    // `gint` there.
    unsafe { value.integer }
}

/// A parsed `G_OPTION_ARG_DOUBLE` slot.
pub(crate) fn option_double(value: OptionValue) -> f64 {
    // SAFETY: the slot was initialised as `number` and GLib writes a
    // `gdouble` there.
    unsafe { value.number }
}

/// A parsed `G_OPTION_ARG_FILENAME` slot: NULL or GLib's `g_strdup`.
pub(crate) fn option_string(value: OptionValue) -> *mut c_char {
    // SAFETY: the slot was initialised as a NULL `string` and GLib writes a
    // `gchar *` there.
    unsafe { value.string }
}

/// Copies a GLib-allocated option string and frees it with `g_free`.
pub(crate) fn take_glib_string(string: *mut c_char) -> Option<Vec<u8>> {
    if string.is_null() {
        return None;
    }
    // SAFETY: a non-NULL option string is a NUL-terminated `g_strdup` result
    // that only this call reads and frees.
    let bytes = unsafe { CStr::from_ptr(string) }.to_bytes().to_vec();
    // SAFETY: as above; it is not used again.
    unsafe { oxpinyin_tool_g_free(string.cast()) };
    Some(bytes)
}

/// `atoi(str)`.
pub(crate) fn atoi(str: &CStr) -> c_int {
    // SAFETY: `str` is NUL-terminated and outlives the call.
    unsafe { oxpinyin_tool_atoi(str.as_ptr()) }
}

/// `atol(str)`.
pub(crate) fn atol(str: &CStr) -> c_long {
    // SAFETY: as `atoi`.
    unsafe { oxpinyin_tool_atol(str.as_ptr()) }
}

/// `g_strsplit_set(string, delimiters, max_tokens)`, copied out and freed.
pub(crate) fn strsplit_set(string: &CStr, delimiters: &CStr, max_tokens: c_int) -> Vec<Vec<u8>> {
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let array =
        unsafe { oxpinyin_tool_strsplit_set(string.as_ptr(), delimiters.as_ptr(), max_tokens) };
    let mut parts = Vec::new();
    if array.is_null() {
        return parts;
    }
    let mut index = 0;
    loop {
        // SAFETY: `g_strsplit_set` answers a NULL-terminated array of
        // NUL-terminated strings; `index` never passes its NULL.
        let part = unsafe { *array.add(index) };
        if part.is_null() {
            break;
        }
        // SAFETY: `part` is one of the array's strings.
        parts.push(unsafe { CStr::from_ptr(part) }.to_bytes().to_vec());
        index += 1;
    }
    // SAFETY: the array came from `g_strsplit_set` and is not used again.
    unsafe { oxpinyin_tool_strfreev(array) };
    parts
}

/// `g_utf8_strlen(str, -1)`.
pub(crate) fn utf8_strlen(str: &CStr) -> c_long {
    // SAFETY: `str` is NUL-terminated and outlives the call.
    unsafe { oxpinyin_tool_utf8_strlen(str.as_ptr()) }
}

/// `g_utf8_to_ucs4(str, -1, NULL, &len, NULL)`: the characters, or `None`
/// for GLib's NULL, and `len` either way.
pub(crate) fn utf8_to_ucs4(str: &CStr) -> (Option<Vec<u32>>, c_long) {
    let mut written: c_long = 0;
    // SAFETY: `str` is NUL-terminated and outlives the call; `written` is a
    // writable `glong`.
    let result = unsafe { oxpinyin_tool_utf8_to_ucs4(str.as_ptr(), &raw mut written) };
    if result.is_null() {
        return (None, written);
    }
    let count = usize::try_from(written).unwrap_or(0);
    // SAFETY: GLib wrote `written` characters at `result`.
    let chars = unsafe { core::slice::from_raw_parts(result, count) }.to_vec();
    // SAFETY: `result` is GLib's allocation and is not used again.
    unsafe { oxpinyin_tool_g_free(result.cast()) };
    (Some(chars), written)
}

/// `g_ucs4_to_utf8(str, len, NULL, NULL, NULL)`, or `None` for GLib's NULL.
pub(crate) fn ucs4_to_utf8(str: &[u32]) -> Option<Vec<u8>> {
    let len = c_long::try_from(str.len()).ok()?;
    // SAFETY: `str` holds `len` characters and outlives the call.
    let result = unsafe { oxpinyin_tool_ucs4_to_utf8(str.as_ptr(), len) };
    take_glib_string(result)
}

/// `g_unichar_isspace(c)`.
pub(crate) fn unichar_isspace(c: u32) -> bool {
    // SAFETY: a plain value call.
    unsafe { oxpinyin_tool_unichar_isspace(c) != 0 }
}

/// `g_unichar_isgraph(c)`.
pub(crate) fn unichar_isgraph(c: u32) -> bool {
    // SAFETY: a plain value call.
    unsafe { oxpinyin_tool_unichar_isgraph(c) != 0 }
}

/// `perror(s)`.
pub(crate) fn perror(s: &CStr) {
    // SAFETY: `s` is NUL-terminated and outlives the call.
    unsafe { oxpinyin_tool_perror(s.as_ptr()) }
}

/// `exit(status)`.
pub(crate) fn exit(status: c_int) -> ! {
    // SAFETY: `exit` runs the C library's handlers and never returns.
    unsafe { oxpinyin_tool_exit(status) }
}

/// The target's `EINVAL`.
pub(crate) fn einval() -> c_int {
    // SAFETY: a constant read.
    unsafe { oxpinyin_tool_einval() }
}

/// An output `FILE *`: `stdout`, which is never closed here, or a stream
/// this handle owns.
pub(crate) struct Output {
    stream: NonNull<File>,
    owned: bool,
}

impl Output {
    /// `FILE * output = stdout;`.
    pub(crate) fn stdout() -> Option<Self> {
        // SAFETY: reads the C library's `stdout`.
        NonNull::new(unsafe { oxpinyin_tool_stdout() }).map(|stream| Self {
            stream,
            owned: false,
        })
    }

    /// `fopen(filename, "w")`.
    pub(crate) fn open_write(filename: &CStr) -> Option<Self> {
        // SAFETY: `filename` is NUL-terminated and outlives the call.
        NonNull::new(unsafe { oxpinyin_tool_fopen_write(filename.as_ptr()) }).map(|stream| Self {
            stream,
            owned: true,
        })
    }

    /// `fclose(output)`, as the pin closes a named output file.
    pub(crate) fn close(self) {
        drop(self);
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: the owned stream is open and never used again.
            unsafe {
                oxpinyin_tool_fclose(self.stream.as_ptr());
            }
        }
    }
}

/// A C++ input stream this handle owns.
pub(crate) struct Istream(NonNull<IstreamHandle>);

impl Istream {
    /// `&std::cin`. Never `None`: the shim's `new` terminates on failure.
    pub(crate) fn cin() -> Option<Self> {
        // SAFETY: the shim allocates a handle around `std::cin`.
        NonNull::new(unsafe { oxpinyin_tool_istream_cin() }).map(Self)
    }

    /// `std::ifstream::open(filename)`; [`Self::good`] then says whether it
    /// opened. Never `None`, as [`Self::cin`].
    pub(crate) fn open(filename: &CStr) -> Option<Self> {
        // SAFETY: `filename` is NUL-terminated and outlives the call.
        NonNull::new(unsafe { oxpinyin_tool_istream_open(filename.as_ptr()) }).map(Self)
    }

    /// `static_cast<bool>(stream)`.
    pub(crate) fn good(&self) -> bool {
        // SAFETY: the handle is live and owned.
        unsafe { oxpinyin_tool_istream_good(self.0.as_ptr()) != 0 }
    }

    /// `std::getline(stream, line)`: the stream's state after it, and the
    /// line's bytes.
    pub(crate) fn getline(&mut self) -> (bool, Vec<u8>) {
        let mut data: *const c_char = core::ptr::null();
        let mut size = 0;
        // SAFETY: the handle is live and owned; `data` and `size` are
        // writable.
        let read =
            unsafe { oxpinyin_tool_istream_getline(self.0.as_ptr(), &raw mut data, &raw mut size) };
        let line = if size == 0 || data.is_null() {
            Vec::new()
        } else {
            // SAFETY: the shim's `std::string` holds `size` bytes at `data`
            // until the next call on this handle.
            unsafe { core::slice::from_raw_parts(data.cast::<u8>(), size) }.to_vec()
        };
        (read != 0, line)
    }
}

impl Drop for Istream {
    fn drop(&mut self) {
        // SAFETY: the handle is owned and never used again.
        unsafe { oxpinyin_tool_istream_close(self.0.as_ptr()) }
    }
}
