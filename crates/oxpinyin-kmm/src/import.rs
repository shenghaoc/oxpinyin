//! `import_k_mixture_model` (`utils/training/import_k_mixture_model.cpp` at
//! `074a2219`): the pin's text model into the working container.
//!
//! The tag state machine is the pin's: `\data model "k mixture model" count
//! C N N total_freq F`, then `\1-gram` with `\item token phrase count C freq
//! F` and `\2-gram` with `\item token phrase token phrase count C T T N_n_0
//! N n_1 N Mr M`, then `\end`. Every `check_result(taglib_read(...))` aborts
//! on a refused line; a token whose phrase text does not validate aborts;
//! `assert(count == T)` aborts; a line type outside its section's `switch`
//! aborts (`:103-256`, `:307-312`). The header's three `atol` values are
//! assigned to `guint32` fields, and an empty input is `ENODATA`.

use std::ffi::{CStr, CString};
use std::io::BufRead;

use oxpinyin_input::tool::{self, LineReader, k_mixture_model as edge};

use crate::database::Database;
use crate::phrase::PhraseIndex;
use crate::records::{KMixtureModelArrayHeader, KMixtureModelArrayItem};
use crate::taglib::{Required, TagLib};

/// `enum LINE_TYPE` (`:42-49`).
const BEGIN_LINE: i32 = 1;
const END_LINE: i32 = 2;
const GRAM_1_LINE: i32 = 3;
const GRAM_2_LINE: i32 = 4;
const GRAM_1_ITEM_LINE: i32 = 5;
const GRAM_2_ITEM_LINE: i32 = 6;

/// `null_token` (`src/include/novel_types.h:121`).
const NULL_TOKEN: u32 = 0;

/// `main`'s body (`:258-317`): read `input`, write `bigram`.
///
/// # Errors
///
/// The pin's ordinary exit: `ENODATA` for an empty input or a foreign header.
/// The function calls [`tool::exit`] and does not return in that case.
pub fn run<R: BufRead>(
    filename: Option<&CStr>,
    phrases: &dyn PhraseIndex,
    input: &mut LineReader<R>,
) {
    let bigram = Database::open_read_write(filename);
    let mut converter = Converter {
        bigram: &bigram,
        phrases,
        input,
        taglib: TagLib::init(),
        line_type: 0,
        values: Vec::new(),
        required: Required::new(),
    };

    if !converter.my_getline() {
        edge::report_empty_input();
        tool::exit(tool::ENODATA);
    }
    if !converter.parse_headline() {
        tool::exit(tool::ENODATA);
    }
    if converter.my_getline() {
        converter.parse_body();
    }
}

/// The pin's statics (`:51-55`) over `main`'s streams.
struct Converter<'a, R> {
    bigram: &'a Database,
    phrases: &'a dyn PhraseIndex,
    input: &'a mut LineReader<R>,
    taglib: TagLib,
    line_type: i32,
    values: Vec<CString>,
    required: Required,
}

impl<R: BufRead> Converter<'_, R> {
    /// `my_getline` (`:68-70`).
    fn my_getline(&mut self) -> bool {
        self.input.getline()
    }

    /// `taglib_read(linebuf, line_type, values, required)`, with the pin's
    /// undefined reads aborted (class (b)).
    fn read(&mut self) -> bool {
        let line = self.input.c_str();
        match self.taglib.read(
            line,
            &mut self.line_type,
            &mut self.values,
            &mut self.required,
        ) {
            Ok(answer) => answer,
            Err(_) => std::process::abort(),
        }
    }

    /// `check_result(taglib_read(…))`.
    fn check_read(&mut self) {
        if !self.read() {
            std::process::abort();
        }
    }

    /// `g_ptr_array_index(values, index)`, which a successful read filled.
    fn value(&self, index: usize) -> &CStr {
        self.values.get(index).map_or(c"", CString::as_c_str)
    }

    /// `TAGLIB_GET_TAGVALUE`'s lookup (`utils_helper.h:42-49`); a successful
    /// read holds every required tag.
    fn tag_value(&self, name: &[u8]) -> &CStr {
        self.required.get(name).map_or(c"", CString::as_c_str)
    }

    /// `TAGLIB_GET_TOKEN(var, index)`: `atoi` into a `guint32`.
    fn token(&self, index: usize) -> u32 {
        tool::atoi(self.value(index)) as u32
    }

    /// `parse_headline` (`:72-101`).
    fn parse_headline(&mut self) -> bool {
        self.taglib
            .add_tag(BEGIN_LINE, c"\\data", 0, c"model:count:N:total_freq", c"");
        if !self.read() || self.tag_value(b"model") != c"k mixture model" {
            edge::report_model_expected();
            return false;
        }

        let count = tool::atol(self.tag_value(b"count"));
        let n = tool::atol(self.tag_value(b"N"));
        let total_freq = tool::atol(self.tag_value(b"total_freq"));

        self.bigram.set_magic_header(&crate::records::KMixtureModelMagicHeader {
            m_wc: count as u32,
            m_n: n as u32,
            m_total_freq: total_freq as u32,
        });
        true
    }

    /// `parse_body` (`:103-134`).
    fn parse_body(&mut self) {
        self.taglib.push_state();
        self.taglib.add_tag(END_LINE, c"\\end", 0, c"", c"");
        self.taglib.add_tag(GRAM_1_LINE, c"\\1-gram", 0, c"", c"");
        self.taglib.add_tag(GRAM_2_LINE, c"\\2-gram", 0, c"", c"");

        loop {
            self.check_read();
            match self.line_type {
                END_LINE => break,
                GRAM_1_LINE => {
                    self.my_getline();
                    self.parse_unigram();
                }
                GRAM_2_LINE => {
                    self.my_getline();
                    self.parse_bigram();
                }
                _ => std::process::abort(),
            }
        }
        self.taglib.pop_state();
    }

    /// `parse_unigram` (`:136-174`).
    fn parse_unigram(&mut self) {
        self.taglib.push_state();
        self.taglib
            .add_tag(GRAM_1_ITEM_LINE, c"\\item", 2, c"count:freq", c"");

        loop {
            self.check_read();
            match self.line_type {
                GRAM_1_ITEM_LINE => {
                    let token = self.token(0);
                    let word = self.value(1).to_owned();
                    if !self
                        .phrases
                        .validate_token_with_string(token, word.to_bytes())
                    {
                        std::process::abort();
                    }
                    let count = tool::atol(self.tag_value(b"count"));
                    let freq = tool::atol(self.tag_value(b"freq"));

                    self.bigram
                        .set_array_header(token, &KMixtureModelArrayHeader {
                            m_wc: count as u32,
                            m_freq: freq as u32,
                        });
                }
                END_LINE | GRAM_1_LINE | GRAM_2_LINE => break,
                _ => std::process::abort(),
            }
            if !self.my_getline() {
                break;
            }
        }
        self.taglib.pop_state();
    }

    /// `parse_bigram` (`:176-256`).
    fn parse_bigram(&mut self) {
        self.taglib.push_state();
        self.taglib.add_tag(
            GRAM_2_ITEM_LINE,
            c"\\item",
            4,
            c"count:T:N_n_0:n_1:Mr",
            c"",
        );

        let mut last_token = NULL_TOKEN;
        let mut last_single_gram: Option<crate::records::KmmGram> = None;
        loop {
            self.check_read();
            match self.line_type {
                GRAM_2_ITEM_LINE => {
                    let token1 = self.token(0);
                    let word1 = self.value(1).to_owned();
                    if !self
                        .phrases
                        .validate_token_with_string(token1, word1.to_bytes())
                    {
                        std::process::abort();
                    }
                    let token2 = self.token(2);
                    let word2 = self.value(3).to_owned();
                    if !self
                        .phrases
                        .validate_token_with_string(token2, word2.to_bytes())
                    {
                        std::process::abort();
                    }

                    let count = tool::atol(self.tag_value(b"count"));
                    let t = tool::atol(self.tag_value(b"T"));
                    if count != t {
                        std::process::abort();
                    }
                    let n_n_0 = tool::atol(self.tag_value(b"N_n_0"));
                    let n_1 = tool::atol(self.tag_value(b"n_1"));
                    let mr = tool::atol(self.tag_value(b"Mr"));

                    let array_item = KMixtureModelArrayItem {
                        m_wc: count as u32,
                        m_n_n_0: n_n_0 as u32,
                        m_n_1: n_1 as u32,
                        m_mr: mr as u32,
                    };

                    if last_token != token1 {
                        if last_token != NULL_TOKEN
                            && let Some(single_gram) = last_single_gram.take()
                        {
                            self.bigram.store(last_token, &single_gram);
                        }
                        let single_gram = self.bigram.load(token1).unwrap_or_default();
                        last_token = token1;
                        last_single_gram = Some(single_gram);
                    }

                    let Some(single_gram) = last_single_gram.as_mut() else {
                        std::process::abort();
                    };
                    if !single_gram.insert_array_item(token2, array_item) {
                        std::process::abort();
                    }
                }
                END_LINE | GRAM_1_LINE | GRAM_2_LINE => break,
                _ => std::process::abort(),
            }
            if !self.my_getline() {
                break;
            }
        }

        if last_token != NULL_TOKEN
            && let Some(single_gram) = last_single_gram.take()
        {
            self.bigram.store(last_token, &single_gram);
        }

        self.taglib.pop_state();
    }
}
