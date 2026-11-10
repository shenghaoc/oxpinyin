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

use std::ffi::CStr;
use std::io::BufRead;

use oxpinyin_input::tool::{self, LineReader, k_mixture_model as edge};

use super::database::{Database, Loaded, exit_on_short_single_gram};
use super::phrase::PhraseIndex;
use super::records::{KMixtureModelArrayHeader, KMixtureModelArrayItem, KmmGram};
use crate::utils_helper::{
    BEGIN_LINE, END_LINE, GRAM_1_ITEM_LINE, GRAM_1_LINE, GRAM_2_ITEM_LINE, GRAM_2_LINE, TaggedModel,
};

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
        model: TaggedModel::new(),
    };

    if !converter.input.getline() {
        edge::report_empty_input();
        tool::exit(tool::ENODATA);
    }
    if !converter.parse_headline() {
        tool::exit(tool::ENODATA);
    }
    if converter.input.getline() {
        converter.parse_body();
    }
}

/// The pin's statics (`:51-55`) over `main`'s streams.
struct Converter<'a, R> {
    bigram: &'a Database,
    phrases: &'a dyn PhraseIndex,
    input: &'a mut LineReader<R>,
    model: TaggedModel,
}

impl<R: BufRead> Converter<'_, R> {
    /// `parse_headline` (`:72-101`).
    fn parse_headline(&mut self) -> bool {
        self.model
            .tags
            .add_tag(BEGIN_LINE, c"\\data", 0, c"model:count:N:total_freq", c"");
        if !self.model.read_or_abort(self.input.c_str())
            || self.model.value(b"model") != c"k mixture model"
        {
            edge::report_model_expected();
            return false;
        }

        let count = tool::atol(self.model.value(b"count"));
        let n = tool::atol(self.model.value(b"N"));
        let total_freq = tool::atol(self.model.value(b"total_freq"));

        self.bigram
            .set_magic_header(&super::records::KMixtureModelMagicHeader {
                m_wc: count as u32,
                m_n: n as u32,
                m_total_freq: total_freq as u32,
            });
        true
    }

    /// `parse_body` (`:103-134`).
    fn parse_body(&mut self) {
        self.model.tags.push_state();
        self.model.tags.add_tag(END_LINE, c"\\end", 0, c"", c"");
        self.model
            .tags
            .add_tag(GRAM_1_LINE, c"\\1-gram", 0, c"", c"");
        self.model
            .tags
            .add_tag(GRAM_2_LINE, c"\\2-gram", 0, c"", c"");

        loop {
            self.model.check_read(self.input.c_str());
            match self.model.line_type {
                END_LINE => break,
                GRAM_1_LINE => {
                    self.input.getline();
                    self.parse_unigram();
                }
                GRAM_2_LINE => {
                    self.input.getline();
                    self.parse_bigram();
                }
                _ => std::process::abort(),
            }
        }
        self.model.tags.pop_state();
    }

    /// `parse_unigram` (`:136-174`).
    fn parse_unigram(&mut self) {
        self.model.tags.push_state();
        self.model
            .tags
            .add_tag(GRAM_1_ITEM_LINE, c"\\item", 2, c"count:freq", c"");

        loop {
            self.model.check_read(self.input.c_str());
            match self.model.line_type {
                GRAM_1_ITEM_LINE => {
                    let token = self.model.token(0) as u32;
                    let word = self.model.phrase(1).to_owned();
                    if !self
                        .phrases
                        .validate_token_with_string(token, word.to_bytes())
                    {
                        std::process::abort();
                    }
                    let count = tool::atol(self.model.value(b"count"));
                    let freq = tool::atol(self.model.value(b"freq"));

                    self.bigram.set_array_header(
                        token,
                        &KMixtureModelArrayHeader {
                            m_wc: count as u32,
                            m_freq: freq as u32,
                        },
                    );
                }
                END_LINE | GRAM_1_LINE | GRAM_2_LINE => break,
                _ => std::process::abort(),
            }
            if !self.input.getline() {
                break;
            }
        }
        self.model.tags.pop_state();
    }

    /// `parse_bigram` (`:176-256`).
    fn parse_bigram(&mut self) {
        self.model.tags.push_state();
        self.model
            .tags
            .add_tag(GRAM_2_ITEM_LINE, c"\\item", 4, c"count:T:N_n_0:n_1:Mr", c"");

        let mut last_token = NULL_TOKEN;
        let mut last_single_gram: Option<KmmGram> = None;
        loop {
            self.model.check_read(self.input.c_str());
            match self.model.line_type {
                GRAM_2_ITEM_LINE => {
                    let token1 = self.model.token(0) as u32;
                    let word1 = self.model.phrase(1).to_owned();
                    if !self
                        .phrases
                        .validate_token_with_string(token1, word1.to_bytes())
                    {
                        std::process::abort();
                    }
                    let token2 = self.model.token(2) as u32;
                    let word2 = self.model.phrase(3).to_owned();
                    if !self
                        .phrases
                        .validate_token_with_string(token2, word2.to_bytes())
                    {
                        std::process::abort();
                    }

                    let count = tool::atol(self.model.value(b"count"));
                    let t = tool::atol(self.model.value(b"T"));
                    if count != t {
                        std::process::abort();
                    }
                    let n_n_0 = tool::atol(self.model.value(b"N_n_0"));
                    let n_1 = tool::atol(self.model.value(b"n_1"));
                    let mr = tool::atol(self.model.value(b"Mr"));

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
                        let single_gram = match self.bigram.load(token1) {
                            Some(Loaded::Gram(gram)) => gram,
                            None => KmmGram::new(),
                            // `insert_array_item` (`:233`) reads outside a
                            // short record: class (b).
                            Some(Loaded::Short) => exit_on_short_single_gram(),
                        };
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
            if !self.input.getline() {
                break;
            }
        }

        if last_token != NULL_TOKEN
            && let Some(single_gram) = last_single_gram.take()
        {
            self.bigram.store(last_token, &single_gram);
        }

        self.model.tags.pop_state();
    }
}
