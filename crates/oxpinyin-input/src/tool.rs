//! The pin utilities' calls at their edge (`docs/findings/external-io-boundary.md`,
//! stage 5): `setlocale(LC_ALL, "")`, the GLib option sequence, the C++
//! `std::getline` input streams, the output `FILE *`, the numeric field
//! conversions and the GLib text calls, each made with the pin's own call
//! through the tool-only shims. Only with the `tools` feature: the shipping
//! libraries never link GLib's option parser or the C++ runtime through
//! this crate.
//!
//! What a field or a line means stays with the utility that reads it; this
//! module answers only what each call answered. The utilities' formatted
//! output calls join the shim with the utility that makes them.

use std::ffi::{CStr, CString, OsString, c_int, c_long};

use crate::native_tool::{self, Istream, OptionSpec, OptionValue as RawValue, Output};

pub mod gen_pinyin_table;

/// `setlocale(LC_ALL, "")`, the first call of every utility's `main`
/// (074a2219 `utils/segment/ngseg.cpp:111`); whether it answered a name.
#[must_use]
pub fn set_locale_from_environment() -> bool {
    native_tool::setlocale_all_environment()
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

impl OptionArg {
    /// The shim's kind, which it maps to GLib's own `G_OPTION_ARG_*`.
    const fn kind(&self) -> c_int {
        match self {
            Self::Flag(_) => 0,
            Self::Int(_) => 1,
            Self::Filename(_) => 2,
            Self::Double(_) => 3,
        }
    }

    fn raw(&self) -> RawValue {
        match *self {
            Self::Flag(flag) => RawValue {
                flag: c_int::from(flag),
            },
            Self::Int(integer) => RawValue { integer },
            Self::Double(number) => RawValue { number },
            Self::Filename(default) => RawValue {
                string: default.map_or(core::ptr::null_mut(), |text| text.as_ptr().cast_mut()),
            },
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
/// `exit(EINVAL)` ([`exit`], [`einval`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptionsFailed;

/// The option sequence of every option-taking utility (074a2219
/// `ngseg.cpp:113-120`, `gen_binary_files.cpp:114-121` and fifteen more):
/// `g_option_context_new(parameter_string)`,
/// `g_option_context_add_main_entries(context, entries, NULL)`,
/// `g_option_context_parse(context, &argc, &argv, &error)` and, on failure,
/// `g_print("option parsing failed:%s\n", error->message)`.
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
    let mut argv: Vec<*mut core::ffi::c_char> = owned
        .iter()
        .map(|arg| arg.as_ptr().cast_mut())
        .chain(core::iter::once(core::ptr::null_mut()))
        .collect();
    let mut argc = c_int::try_from(owned.len()).map_err(|_| OptionsFailed)?;
    let specs: Vec<OptionSpec> = entries
        .iter()
        .map(|entry| OptionSpec {
            long_name: entry.long_name.as_ptr(),
            short_name: entry.short_name as core::ffi::c_char,
            flags: entry.flags,
            arg: entry.arg.kind(),
            description: entry.description.map_or(core::ptr::null(), CStr::as_ptr),
            arg_description: entry
                .arg_description
                .map_or(core::ptr::null(), CStr::as_ptr),
        })
        .collect();
    let mut raw: Vec<RawValue> = entries.iter().map(|entry| entry.arg.raw()).collect();
    let args = native_tool::parse_options(parameter_string, &specs, &mut raw, &mut argc, &mut argv)
        .ok_or(OptionsFailed)?;
    let values = entries
        .iter()
        .zip(&raw)
        .map(|(entry, value)| read_value(&entry.arg, *value))
        .collect();
    Ok(ParsedOptions { values, args })
}

fn read_value(arg: &OptionArg, value: RawValue) -> OptionValue {
    match arg {
        OptionArg::Flag(_) => OptionValue::Flag(native_tool::option_flag(value)),
        OptionArg::Int(_) => OptionValue::Int(native_tool::option_int(value)),
        OptionArg::Double(_) => OptionValue::Double(native_tool::option_double(value)),
        OptionArg::Filename(default) => OptionValue::Filename(native_tool::take_option_filename(
            value,
            default.map_or(core::ptr::null_mut(), |text| text.as_ptr().cast_mut()),
        )),
    }
}

/// A line `std::getline` read into the pin's `LineString`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineString {
    /// The `std::string`'s bytes and the NUL `c_str()` adds.
    terminated: Vec<u8>,
}

impl LineString {
    fn new(mut bytes: Vec<u8>) -> Self {
        bytes.push(0);
        Self { terminated: bytes }
    }

    /// The `std::string`'s bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.terminated[..self.terminated.len() - 1]
    }

    /// What `operator const gchar *` (`src/include/pinyin_utils.h:33-40`)
    /// shows a C call: `c_str()`, read up to its first NUL.
    #[must_use]
    pub fn c_str(&self) -> &CStr {
        CStr::from_bytes_until_nul(&self.terminated).unwrap_or_default()
    }
}

/// A utility's input: `std::cin`, or a `std::ifstream` on a named file.
pub struct InputStream(Istream);

impl InputStream {
    /// `std::istream * input = &std::cin;` (`ngseg.cpp:105`).
    #[must_use]
    pub fn stdin() -> Option<Self> {
        Istream::cin().map(Self)
    }

    /// `input_file.open(filename)` (`ngseg.cpp:131`). The pin then tests
    /// `!input_file` ([`Self::is_open`]) and calls `perror` while `errno`
    /// is still the open's.
    #[must_use]
    pub fn open(filename: &CStr) -> Option<Self> {
        Istream::open(filename).map(Self)
    }

    /// `static_cast<bool>(input_file)`: what `!input_file` negates.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.0.good()
    }

    /// `std::getline(*input, linebuf)`: the line when the stream converts
    /// to `true` afterwards, as the pin's `while` tests it.
    pub fn getline(&mut self) -> Option<LineString> {
        let (read, line) = self.0.getline();
        read.then(|| LineString::new(line))
    }
}

/// A utility's output: `stdout`, or `fopen(outputfile, "w")`.
pub struct OutputStream(Output);

impl OutputStream {
    /// `FILE * output = stdout;` (`ngseg.cpp:106`).
    #[must_use]
    pub fn stdout() -> Option<Self> {
        Output::stdout().map(Self)
    }

    /// `fopen(filename, "w")` (`ngseg.cpp:122`); `None` for NULL, after
    /// which the pin calls `perror` ([`perror`]).
    #[must_use]
    pub fn open_write(filename: &CStr) -> Option<Self> {
        Output::open_write(filename).map(Self)
    }

    /// `fclose(output)` (`ngseg.cpp:260`). `stdout` is not closed.
    pub fn close(self) {
        self.0.close();
    }
}

/// `atoi(str)` (`utils/utils_helper.h:27-58`).
#[must_use]
pub fn atoi(str: &CStr) -> c_int {
    native_tool::atoi(str)
}

/// `atol(str)` (`utils/utils_helper.h:60-69`).
#[must_use]
pub fn atol(str: &CStr) -> c_long {
    native_tool::atol(str)
}

/// `g_strsplit_set(string, delimiters, max_tokens)`
/// (`utils/utils_helper.h:27-46`), its strings copied out.
#[must_use]
pub fn strsplit_set(string: &CStr, delimiters: &CStr, max_tokens: c_int) -> Vec<Vec<u8>> {
    native_tool::strsplit_set(string, delimiters, max_tokens)
}

/// `g_utf8_strlen(str, -1)` (`ngseg.cpp:190`).
#[must_use]
pub fn utf8_strlen(str: &CStr) -> c_long {
    native_tool::utf8_strlen(str)
}

/// `g_utf8_to_ucs4(str, -1, NULL, &len, NULL)` (`ngseg.cpp:192`): the
/// characters, `None` where GLib answers NULL, and `len`.
#[must_use]
pub fn utf8_to_ucs4(str: &CStr) -> (Option<Vec<u32>>, c_long) {
    native_tool::utf8_to_ucs4(str)
}

/// `g_ucs4_to_utf8(str, len, NULL, NULL, NULL)` (`ngseg.cpp:80`, `:98`),
/// `None` where GLib answers NULL.
#[must_use]
pub fn ucs4_to_utf8(str: &[u32]) -> Option<Vec<u8>> {
    native_tool::ucs4_to_utf8(str)
}

/// `g_unichar_isspace(c)` (`src/storage/tag_utility.cpp:118-170`).
#[must_use]
pub fn unichar_isspace(c: u32) -> bool {
    native_tool::unichar_isspace(c)
}

/// `g_unichar_isgraph(c)` (`src/storage/tag_utility.cpp:118-170`).
#[must_use]
pub fn unichar_isgraph(c: u32) -> bool {
    native_tool::unichar_isgraph(c)
}

/// `perror(s)` (`ngseg.cpp:124`).
pub fn perror(s: &CStr) {
    native_tool::perror(s);
}

/// `exit(status)`: the C library's, so its `atexit` handlers and stream
/// flushes run as the pin's do.
pub fn exit(status: c_int) -> ! {
    native_tool::exit(status)
}

/// The target's `ENOENT`, the pin's exit status for a file it cannot open
/// (`gen_pinyin_table.cpp:141`, `:264`).
#[must_use]
pub fn enoent() -> c_int {
    native_tool::enoent()
}

/// The target's `EINVAL`, the pin's exit status for a refused option or
/// argument (`ngseg.cpp:119`).
#[must_use]
pub fn einval() -> c_int {
    native_tool::einval()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(text: &str) -> CString {
        CString::new(text).unwrap()
    }

    #[test]
    fn getline_reads_the_pins_lines_and_c_str_stops_at_a_nul() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("in.txt");
        std::fs::write(&path, b"a\nb\0c\n\nlast").unwrap();
        let name = crate::c_path(&path).unwrap();
        let mut input = InputStream::open(&name).unwrap();
        assert!(input.is_open());
        let mut lines = Vec::new();
        while let Some(line) = input.getline() {
            lines.push(line);
        }
        let bytes: Vec<&[u8]> = lines.iter().map(LineString::as_bytes).collect();
        assert_eq!(bytes, [&b"a"[..], b"b\0c", b"", b"last"]);
        assert_eq!(lines[1].c_str(), c"b");
        assert_eq!(lines[3].c_str(), c"last");
    }

    #[test]
    fn a_missing_file_does_not_open() {
        let dir = tempfile::tempdir().unwrap();
        let name = crate::c_path(&dir.path().join("absent")).unwrap();
        assert!(!InputStream::open(&name).unwrap().is_open());
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
        let text = c("你好");
        assert_eq!(utf8_strlen(&text), 2);
        assert_eq!(utf8_to_ucs4(&text), (Some(vec![0x4F60, 0x597D]), 2));
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
    fn einval_is_the_targets() {
        assert!(einval() > 0);
    }
}
