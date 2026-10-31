//! `utils/training/k_mixture_model_to_interpolation.cpp` at libpinyin
//! 074a2219: KMM text in, interpolation text out.
//!
//! [`convert`] is the conversion, over any [`LineReader`] and [`Sink`]; it
//! answers a [`Failure`] where the pin exits or aborts. [`convert_text`] runs
//! it in memory for the trainer. [`main`] is the utility: `std::cin`,
//! `stdout`, the pin's messages, `exit(ENODATA)`, `abort()` and oxpinyin's
//! class (b) refusal. The utility parses no options, reads no argument
//! (`main`, :188-213) and makes no locale call.
//!
//! The parse keeps the pin's control flow, including what follows from its
//! static `linebuf`: a section header re-reads the line its section's
//! parser stopped on (`goto retry`, :86-107), and `std::getline` leaves
//! `linebuf` unchanged once the stream is at its end.

use std::ffi::{CStr, CString, OsString, c_int, c_long};
use std::fmt;
use std::io::{BufRead, Write as _};

use oxpinyin_input::tool::k_mixture_model_to_interpolation as edge;
use oxpinyin_input::tool::{self, LineReader, OutputStream};

use crate::taglib::{Required, TagLib};

/// `enum LINE_TYPE` (:32-39).
const BEGIN_LINE: c_int = 1;
const END_LINE: c_int = 2;
const GRAM_1_LINE: c_int = 3;
const GRAM_2_LINE: c_int = 4;
const GRAM_1_ITEM_LINE: c_int = 5;
const GRAM_2_ITEM_LINE: c_int = 6;

/// `sentence_start` (`src/include/novel_types.h:122`).
const SENTENCE_START: u32 = 1;

/// Where the pin stops before `\end`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Failure {
    /// No first line: `empty file input.`, `exit(ENODATA)` (:197-201).
    EmptyInput,
    /// The first line is not a KMM header: `error: k mixture model
    /// expected.`, `exit(ENODATA)` (:59-71, :203-204).
    NotKmm,
    /// `check_result(taglib_read(...))` refused a line in a section
    /// (:88, :119, :158): the reference build's `assert` aborts.
    ReadRefused,
    /// A registered line type outside its `switch`: `abort()` (:104, :142,
    /// :179).
    LineTypeUnlisted,
    /// A line whose read is undefined in the pin (class (b)).
    Undefined {
        /// oxpinyin's reason.
        reason: &'static CStr,
        /// The line as `c_str()` shows it.
        line: CString,
    },
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => formatter.write_str("empty file input"),
            Self::NotKmm => formatter.write_str("k mixture model expected"),
            Self::ReadRefused => formatter.write_str("a section line was refused"),
            Self::LineTypeUnlisted => formatter.write_str("a line type outside its section"),
            Self::Undefined { reason, line } => write!(
                formatter,
                "{}: {}",
                reason.to_string_lossy(),
                line.to_string_lossy()
            ),
        }
    }
}

impl std::error::Error for Failure {}

/// The interpolation lines the conversion writes (:74, :91-99, :134,
/// :170-171).
pub trait Sink {
    /// `\data model interpolation`.
    fn header(&mut self);
    /// `\end`.
    fn end(&mut self);
    /// `\1-gram`.
    fn gram_1(&mut self);
    /// `\2-gram`.
    fn gram_2(&mut self);
    /// `\item %d %s count %ld`.
    fn unigram(&mut self, token: c_int, word: &CStr, freq: c_long);
    /// `\item %d %s %d %s count %ld`.
    fn bigram(&mut self, token1: c_int, word1: &CStr, token2: c_int, word2: &CStr, count: c_long);
}

/// The utility's `fprintf` calls on its output stream.
impl Sink for OutputStream {
    fn header(&mut self) {
        edge::print_header(self);
    }

    fn end(&mut self) {
        edge::print_end(self);
    }

    fn gram_1(&mut self) {
        edge::print_gram_1(self);
    }

    fn gram_2(&mut self) {
        edge::print_gram_2(self);
    }

    fn unigram(&mut self, token: c_int, word: &CStr, freq: c_long) {
        edge::print_unigram(self, token, word, freq);
    }

    fn bigram(&mut self, token1: c_int, word1: &CStr, token2: c_int, word2: &CStr, count: c_long) {
        edge::print_bigram(self, token1, word1, token2, word2, count);
    }
}

/// The same lines in memory: `%d` and `%ld` are the signed decimal Rust
/// writes, and `%s` the word's bytes. Writing to a `Vec` cannot fail.
impl Sink for Vec<u8> {
    fn header(&mut self) {
        self.extend_from_slice(b"\\data model interpolation\n");
    }

    fn end(&mut self) {
        self.extend_from_slice(b"\\end\n");
    }

    fn gram_1(&mut self) {
        self.extend_from_slice(b"\\1-gram\n");
    }

    fn gram_2(&mut self) {
        self.extend_from_slice(b"\\2-gram\n");
    }

    fn unigram(&mut self, token: c_int, word: &CStr, freq: c_long) {
        let _ = write!(self, "\\item {token} ");
        self.extend_from_slice(word.to_bytes());
        let _ = writeln!(self, " count {freq}");
    }

    fn bigram(&mut self, token1: c_int, word1: &CStr, token2: c_int, word2: &CStr, count: c_long) {
        let _ = write!(self, "\\item {token1} ");
        self.extend_from_slice(word1.to_bytes());
        let _ = write!(self, " {token2} ");
        self.extend_from_slice(word2.to_bytes());
        let _ = writeln!(self, " count {count}");
    }
}

/// `g_ptr_array_index(values, index)`, which a successful read filled.
fn value(values: &[CString], index: usize) -> &CStr {
    values.get(index).map_or(c"", CString::as_c_str)
}

/// The pin's statics (:41-44) over `main`'s streams (:189-190); the
/// reader's line is the pin's `linebuf`.
struct Converter<'a, R, S> {
    input: &'a mut LineReader<R>,
    output: &'a mut S,
    taglib: TagLib,
    line_type: c_int,
    values: Vec<CString>,
    required: Required,
}

impl<R: BufRead, S: Sink> Converter<'_, R, S> {
    /// `my_getline` (:51-53).
    fn my_getline(&mut self) -> bool {
        self.input.getline()
    }

    /// `taglib_read(linebuf, line_type, values, required)`.
    fn read(&mut self) -> Result<bool, Failure> {
        let line = self.input.c_str();
        self.taglib
            .read(
                line,
                &mut self.line_type,
                &mut self.values,
                &mut self.required,
            )
            .map_err(|undefined| Failure::Undefined {
                reason: undefined.reason(),
                line: line.to_owned(),
            })
    }

    /// `check_result(taglib_read(...))` (`src/include/pinyin_utils.h:26-31`).
    fn check_read(&mut self) -> Result<(), Failure> {
        if self.read()? {
            Ok(())
        } else {
            Err(Failure::ReadRefused)
        }
    }

    /// `TAGLIB_GET_TOKEN(var, index)` (`utils/utils_helper.h:27-33`): `atoi`
    /// into a `phrase_token_t`, which the pin prints with `%d`.
    fn token(&self, index: usize) -> c_int {
        tool::atoi(value(&self.values, index))
    }

    /// The value `TAGLIB_GET_TAGVALUE` looks up (`utils_helper.h:42-49`);
    /// a successful read holds every required tag.
    fn tag_value(&self, name: &[u8]) -> &CStr {
        self.required.get(name).map_or(c"", CString::as_c_str)
    }

    /// `parse_headline` (:55-77).
    fn parse_headline(&mut self) -> Result<(), Failure> {
        self.taglib
            .add_tag(BEGIN_LINE, c"\\data", 0, c"model", c"count:N:total_freq");
        if !self.read()? || self.tag_value(b"model") != c"k mixture model" {
            return Err(Failure::NotKmm);
        }
        self.output.header();
        Ok(())
    }

    /// `parse_body` (:79-111). Every case leaves the loop or re-reads the
    /// line, so its `while (my_getline(input))` is never tested.
    fn parse_body(&mut self) -> Result<(), Failure> {
        self.taglib.push_state();
        self.taglib.add_tag(END_LINE, c"\\end", 0, c"", c"");
        self.taglib.add_tag(GRAM_1_LINE, c"\\1-gram", 0, c"", c"");
        self.taglib.add_tag(GRAM_2_LINE, c"\\2-gram", 0, c"", c"");
        loop {
            self.check_read()?;
            match self.line_type {
                END_LINE => {
                    self.output.end();
                    break;
                }
                GRAM_1_LINE => {
                    self.output.gram_1();
                    self.my_getline();
                    self.parse_unigram()?;
                }
                GRAM_2_LINE => {
                    self.output.gram_2();
                    self.my_getline();
                    self.parse_bigram()?;
                }
                _ => return Err(Failure::LineTypeUnlisted),
            }
        }
        self.taglib.pop_state();
        Ok(())
    }

    /// `parse_unigram` (:113-149).
    fn parse_unigram(&mut self) -> Result<(), Failure> {
        self.taglib.push_state();
        self.taglib
            .add_tag(GRAM_1_ITEM_LINE, c"\\item", 2, c"freq", c"count");
        loop {
            self.check_read()?;
            match self.line_type {
                GRAM_1_ITEM_LINE => {
                    let token = self.token(0);
                    // The "<start>" of the KMM unigram is not printed.
                    if token.cast_unsigned() != SENTENCE_START {
                        let freq = tool::atol(self.tag_value(b"freq"));
                        if freq != 0 {
                            self.output.unigram(token, value(&self.values, 1), freq);
                        }
                    }
                }
                END_LINE | GRAM_1_LINE | GRAM_2_LINE => break,
                _ => return Err(Failure::LineTypeUnlisted),
            }
            if !self.my_getline() {
                break;
            }
        }
        self.taglib.pop_state();
        Ok(())
    }

    /// `parse_bigram` (:151-186).
    fn parse_bigram(&mut self) -> Result<(), Failure> {
        self.taglib.push_state();
        self.taglib
            .add_tag(GRAM_2_ITEM_LINE, c"\\item", 4, c"count", c"T:N_n_0:n_1:Mr");
        loop {
            self.check_read()?;
            match self.line_type {
                GRAM_2_ITEM_LINE => {
                    let count = tool::atol(self.tag_value(b"count"));
                    let (token1, token2) = (self.token(0), self.token(2));
                    let (word1, word2) = (value(&self.values, 1), value(&self.values, 3));
                    self.output.bigram(token1, word1, token2, word2, count);
                }
                END_LINE | GRAM_1_LINE | GRAM_2_LINE => break,
                _ => return Err(Failure::LineTypeUnlisted),
            }
            if !self.my_getline() {
                break;
            }
        }
        self.taglib.pop_state();
        Ok(())
    }
}

/// `main`'s work (:192-210): reads `input` and writes `output` as the pin
/// does, line by line, up to `\end`, the end of input or the [`Failure`]
/// where the pin exits or aborts. The lines written before a failure stay
/// written.
///
/// # Errors
///
/// The [`Failure`] at which the pin stops.
pub fn convert<R: BufRead>(
    input: &mut LineReader<R>,
    output: &mut impl Sink,
) -> Result<(), Failure> {
    let mut converter = Converter {
        input,
        output,
        taglib: TagLib::init(),
        line_type: 0,
        values: Vec::new(),
        required: Required::new(),
    };
    if !converter.my_getline() {
        return Err(Failure::EmptyInput);
    }
    converter.parse_headline()?;
    if converter.my_getline() {
        converter.parse_body()?;
    }
    Ok(())
}

/// [`convert`] in memory, as the trainer runs `k_mixture_model_to_interpolation
/// < kmm > interpolation2` (`tryprune.py`): the interpolation text.
///
/// # Errors
///
/// The [`Failure`] at which the pin's utility would stop.
pub fn convert_text(kmm: &[u8]) -> Result<Vec<u8>, Failure> {
    let mut output = Vec::new();
    convert(&mut LineReader::from_bytes(kmm), &mut output)?;
    Ok(output)
}

/// `main` (:188-213): `argv` is not read.
pub fn main(_args: impl IntoIterator<Item = OsString>) {
    let mut input = LineReader::stdin();
    // The C runtime's `stdout` is never NULL in a started process.
    let Some(mut output) = OutputStream::stdout() else {
        tool::abort();
    };
    match convert(&mut input, &mut output) {
        Ok(()) => {}
        Err(Failure::EmptyInput) => {
            edge::report_empty_input();
            tool::exit(tool::ENODATA);
        }
        Err(Failure::NotKmm) => {
            edge::report_model_expected();
            tool::exit(tool::ENODATA);
        }
        // The pin's `assert` and `abort()`, without the macro's message
        // (maintainer ruling 2026-10-31 UTC).
        Err(Failure::ReadRefused | Failure::LineTypeUnlisted) => tool::abort(),
        Err(Failure::Undefined { reason, line }) => {
            edge::refuse(reason, &line);
            tool::exit(1);
        }
    }
}
