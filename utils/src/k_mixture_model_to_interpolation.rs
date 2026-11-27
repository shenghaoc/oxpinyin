//! `utils/training/k_mixture_model_to_interpolation.cpp` at libpinyin
//! 074a2219: KMM text in, interpolation text out.
//!
//! [`convert`] reads lines through [`LineReader`] and writes each result to an
//! [`OutputStream`]. It returns a [`Failure`] where the pin exits or aborts.
//! [`main`] reads `std::cin`, writes to `stdout`,
//! follows the pin's messages and its calls to `exit(ENODATA)` and `abort()`,
//! and refuses undefined reads. The utility parses no options, reads no argument
//! (`main`, :188-213) and makes no locale call.
//!
//! A section header re-reads the line its section's parser stopped on
//! (`goto retry`, :86-107). A final unterminated item with no end marker
//! reaches the pin's section assertion. Other missing-token paths retain
//! their prescribed refusal.

use oxpinyin::vocab::SENTENCE_START;
use std::ffi::{CStr, CString, OsString};
use std::fmt;
use std::io::BufRead;

use oxpinyin_tool_support::input::k_mixture_model_to_interpolation as edge;
use oxpinyin_tool_support::input::{self as tool, LineReader, OutputStream};

use crate::utils_helper::{
    BEGIN_LINE, END_LINE, GRAM_1_ITEM_LINE, GRAM_1_LINE, GRAM_2_ITEM_LINE, GRAM_2_LINE, TaggedModel,
};

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
    /// A line whose read is undefined in the pin.
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

/// The pin's statics (:41-44) over `main`'s streams (:189-190); the
/// reader's line is the pin's `linebuf`.
struct Converter<'a, R> {
    input: &'a mut LineReader<R>,
    output: &'a mut OutputStream,
    model: TaggedModel,
}

impl<R: BufRead> Converter<'_, R> {
    /// `taglib_read(linebuf, line_type, values, required)`.
    fn read(
        &mut self,
        malformed: crate::utils_helper::taglib::MalformedTag,
    ) -> Result<bool, Failure> {
        let line = self.input.c_str();
        self.model
            .read(line, malformed)
            .map_err(|undefined| Failure::Undefined {
                reason: undefined.reason(),
                line: line.to_owned(),
            })
    }

    /// `check_result(taglib_read(...))` (`src/include/pinyin_utils.h:26-31`).
    fn check_read(&mut self, final_item_at_eof: bool) -> Result<(), Failure> {
        match self.read(crate::utils_helper::taglib::MalformedTag::Refuse) {
            Ok(true) => Ok(()),
            Ok(false) => Err(Failure::ReadRefused),
            // The section has ended. Its item tag is unavailable at the
            // outer check, which rejects an unterminated final item.
            Err(Failure::Undefined { .. }) if final_item_at_eof => Err(Failure::ReadRefused),
            Err(error) => Err(error),
        }
    }

    /// `parse_headline` (:55-77).
    fn parse_headline(&mut self) -> Result<(), Failure> {
        self.model
            .tags
            .add_tag(BEGIN_LINE, c"\\data", 0, c"model", c"count:N:total_freq");
        if !self.read(crate::utils_helper::taglib::MalformedTag::Read)?
            || self.model.value(b"model") != c"k mixture model"
        {
            return Err(Failure::NotKmm);
        }
        edge::print_header(self.output);
        Ok(())
    }

    /// `parse_body` (:79-111). Every case leaves the loop or re-reads the
    /// line, so its `while (my_getline(input))` is never tested.
    fn parse_body(&mut self) -> Result<(), Failure> {
        self.model.tags.push_state();
        self.model.tags.add_tag(END_LINE, c"\\end", 0, c"", c"");
        self.model
            .tags
            .add_tag(GRAM_1_LINE, c"\\1-gram", 0, c"", c"");
        self.model
            .tags
            .add_tag(GRAM_2_LINE, c"\\2-gram", 0, c"", c"");
        let mut final_item_at_eof = false;
        loop {
            self.check_read(final_item_at_eof)?;
            match self.model.line_type {
                END_LINE => {
                    edge::print_end(self.output);
                    break;
                }
                GRAM_1_LINE => {
                    edge::print_gram_1(self.output);
                    self.input.getline();
                    final_item_at_eof = self.parse_unigram()?;
                }
                GRAM_2_LINE => {
                    edge::print_gram_2(self.output);
                    self.input.getline();
                    final_item_at_eof = self.parse_bigram()?;
                }
                _ => return Err(Failure::LineTypeUnlisted),
            }
        }
        self.model.tags.pop_state();
        Ok(())
    }

    /// `parse_unigram` (:113-149).
    fn parse_unigram(&mut self) -> Result<bool, Failure> {
        self.model.tags.push_state();
        self.model
            .tags
            .add_tag(GRAM_1_ITEM_LINE, c"\\item", 2, c"freq", c"count");
        let final_item_at_eof = loop {
            self.check_read(false)?;
            match self.model.line_type {
                GRAM_1_ITEM_LINE => {
                    let token = self.model.token(0);
                    // The "<start>" of the KMM unigram is not printed.
                    if token.cast_unsigned() != SENTENCE_START {
                        let freq = tool::atol(self.model.value(b"freq"));
                        if freq != 0 {
                            edge::print_unigram(self.output, token, self.model.phrase(1), freq);
                        }
                    }
                }
                END_LINE | GRAM_1_LINE | GRAM_2_LINE => break false,
                _ => return Err(Failure::LineTypeUnlisted),
            }
            if !self.input.getline() {
                break self.input.last_line_was_unterminated();
            }
        };
        self.model.tags.pop_state();
        Ok(final_item_at_eof)
    }

    /// `parse_bigram` (:151-186).
    fn parse_bigram(&mut self) -> Result<bool, Failure> {
        self.model.tags.push_state();
        self.model
            .tags
            .add_tag(GRAM_2_ITEM_LINE, c"\\item", 4, c"count", c"T:N_n_0:n_1:Mr");
        let final_item_at_eof = loop {
            self.check_read(false)?;
            match self.model.line_type {
                GRAM_2_ITEM_LINE => {
                    let count = tool::atol(self.model.value(b"count"));
                    let (token1, token2) = (self.model.token(0), self.model.token(2));
                    let (word1, word2) = (self.model.phrase(1), self.model.phrase(3));
                    edge::print_bigram(self.output, token1, word1, token2, word2, count);
                }
                END_LINE | GRAM_1_LINE | GRAM_2_LINE => break false,
                _ => return Err(Failure::LineTypeUnlisted),
            }
            if !self.input.getline() {
                break self.input.last_line_was_unterminated();
            }
        };
        self.model.tags.pop_state();
        Ok(final_item_at_eof)
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
    output: &mut OutputStream,
) -> Result<(), Failure> {
    let mut converter = Converter {
        input,
        output,
        model: TaggedModel::new(),
    };
    if !converter.input.getline() {
        return Err(Failure::EmptyInput);
    }
    converter.parse_headline()?;
    if converter.input.getline() {
        converter.parse_body()?;
    }
    Ok(())
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
        Err(Failure::ReadRefused | Failure::LineTypeUnlisted) => tool::abort(),
        Err(Failure::Undefined { reason, line }) => {
            edge::refuse(reason, &line);
            tool::exit(1);
        }
    }
}
