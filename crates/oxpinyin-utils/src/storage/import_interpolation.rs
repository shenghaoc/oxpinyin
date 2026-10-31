//! `import_interpolation` (`utils/storage/import_interpolation.cpp` at
//! libpinyin 074a2219).
//!
//! Reads an interpolation model from `std::cin` through the pin's taglib
//! grammar, applies each `\1-gram` count to the system phrase-library chunk
//! (`FacadePhraseIndex::add_unigram_frequency`), each `\2-gram` row to
//! `bigram.db` (`SingleGram::insert_freq`), and saves the chunks
//! (`save_phrase_index`).

use std::ffi::{CStr, CString, OsString, c_int};

use oxpinyin_data::single_gram::{decode_single_gram, encode_single_gram};
use oxpinyin_input::tool;
use oxpinyin_input::tool::LineReader;

use crate::storage::dbm::Hash;
use crate::storage::libraries::Libraries;
use crate::storage::{load_table_conf, parse_options, table_dir, table_dir_entry};
use crate::taglib::{Required, TagLib};

/// `enum LINE_TYPE` (`:43-50`).
const BEGIN_LINE: c_int = 1;
const END_LINE: c_int = 2;
const GRAM_1_LINE: c_int = 3;
const GRAM_2_LINE: c_int = 4;
const GRAM_1_ITEM_LINE: c_int = 5;
const GRAM_2_ITEM_LINE: c_int = 6;

/// `main` (`:234-307`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let _ = tool::set_locale_from_environment();
    let parsed = parse_options(c"- import interpolation model", &[table_dir_entry()], args);
    let _directory = table_dir(&parsed, 0);

    let conf = load_table_conf();

    // `phrase_table.attach(SYSTEM_PHRASE_INDEX, ATTACH_READONLY)`: opened,
    // its answer checked, and never otherwise used.
    if crate::storage::dbm::Tree::open_read_only(std::path::Path::new("phrase_index.bin")).is_err()
    {
        tool::storage::report_open_failed(c"phrase_index.bin");
        tool::exit(tool::ENOENT);
    }

    let mut libraries = Libraries::load(&conf);

    let bigram = match Hash::create(std::path::Path::new("bigram.db")) {
        Ok(bigram) => bigram,
        Err(_) => {
            tool::storage::report_open_failed(c"bigram.db");
            tool::exit(tool::ENOENT);
        }
    };

    let taglib = TagLib::init();
    let mut converter = Converter {
        input: LineReader::stdin(),
        taglib,
        line_type: 0,
        values: Vec::new(),
        required: Required::new(),
        libraries: &mut libraries,
        bigram,
    };

    if !converter.getline() {
        tool::storage::report_empty_input();
        tool::exit(tool::ENODATA);
    }
    converter.parse_headline();

    if converter.getline() {
        converter.parse_body();
    }

    libraries.save(&conf);
}

/// One `GPtrArray` value, as `g_ptr_array_index(values, index)` shows it.
fn value(values: &[CString], index: usize) -> &CStr {
    values.get(index).map_or(c"", CString::as_c_str)
}

/// A `SingleGram` under construction.
struct SingleGram {
    total: u32,
    records: Vec<(u32, u32)>,
}

/// The pin's statics (`:52-56`) over `main`'s streams.
struct Converter<'a> {
    input: LineReader<std::io::StdinLock<'static>>,
    taglib: TagLib,
    line_type: c_int,
    values: Vec<CString>,
    required: Required,
    libraries: &'a mut Libraries,
    bigram: Hash,
}

impl Converter<'_> {
    /// `my_getline(input)` (`:67-69`).
    fn getline(&mut self) -> bool {
        self.input.getline()
    }

    /// `taglib_read(linebuf, line_type, values, required)`.
    fn read(&mut self) -> bool {
        let line = self.input.c_str();
        match self.taglib.read(
            line,
            &mut self.line_type,
            &mut self.values,
            &mut self.required,
        ) {
            Ok(answer) => answer,
            // The pin's read of this line is undefined (class (b)); the
            // utility's `check_result` never receives a definite answer.
            Err(_) => tool::abort(),
        }
    }

    /// `check_result(taglib_read(...))`.
    fn check_read(&mut self) {
        if !self.read() {
            tool::abort();
        }
    }

    /// The required tag's value, as `TAGLIB_GET_TAGVALUE` looks it up.
    fn tag_value(&self, name: &[u8]) -> &CStr {
        self.required.get(name).map_or(c"", CString::as_c_str)
    }

    /// `taglib_token_to_string` (`src/storage/tag_utility.cpp:384-405`).
    fn token_to_string(&self, token: u32) -> Option<CString> {
        crate::storage::libraries::token_to_string(self.libraries, token)
    }

    /// `taglib_validate_token_with_string`
    /// (`src/storage/tag_utility.cpp:407-417`); a missing string is the
    /// pin's `strcmp(NULL, ...)` (class (b)) and compares unequal here.
    fn validate(&self, token: u32, word: &CStr) -> bool {
        self.token_to_string(token)
            .is_some_and(|text| text.as_bytes() == word.to_bytes())
    }

    /// `parse_headline` (`:71-89`).
    fn parse_headline(&mut self) {
        self.taglib.add_tag(BEGIN_LINE, c"\\data", 0, c"model", c"");
        if !self.read() || self.tag_value(b"model") != c"interpolation" {
            tool::storage::report_interpolation_expected();
            tool::exit(tool::ENODATA);
        }
    }

    /// `parse_body` (`:91-122`).
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
                    self.getline();
                    self.parse_unigram();
                }
                GRAM_2_LINE => {
                    self.getline();
                    self.parse_bigram();
                }
                // `default: abort()`.
                _ => tool::abort(),
            }
        }
        self.taglib.pop_state();
    }

    /// `parse_unigram` (`:124-156`).
    fn parse_unigram(&mut self) {
        self.taglib.push_state();
        self.taglib
            .add_tag(GRAM_1_ITEM_LINE, c"\\item", 2, c"count", c"");
        loop {
            self.check_read();
            match self.line_type {
                GRAM_1_ITEM_LINE => {
                    // TAGLIB_GET_TOKEN token; TAGLIB_GET_PHRASE_STRING word.
                    // `phrase_token_t token = atoi(string)`.
                    let token = tool::atoi(value(&self.values, 0)) as u32;
                    let word = value(&self.values, 1).to_owned();
                    if !self.validate(token, &word) {
                        // `check_result(taglib_validate_token_with_string(...))`.
                        tool::abort();
                    }
                    let count = tool::atol(self.tag_value(b"count"));
                    self.libraries.add_unigram(token, count as u32);
                }
                END_LINE | GRAM_1_LINE | GRAM_2_LINE => break,
                // `default: abort()`.
                _ => tool::abort(),
            }
            if !self.getline() {
                break;
            }
        }
        self.taglib.pop_state();
    }

    /// `parse_bigram` (`:158-232`).
    fn parse_bigram(&mut self) {
        self.taglib.push_state();
        self.taglib
            .add_tag(GRAM_2_ITEM_LINE, c"\\item", 4, c"count", c"");
        let mut last_token = 0_u32;
        let mut last_gram: Option<SingleGram> = None;
        loop {
            self.check_read();
            match self.line_type {
                GRAM_2_ITEM_LINE => {
                    let token1 = tool::atoi(value(&self.values, 0)) as u32;
                    let word1 = value(&self.values, 1).to_owned();
                    if !self.validate(token1, &word1) {
                        tool::abort();
                    }
                    let token2 = tool::atoi(value(&self.values, 2)) as u32;
                    let word2 = value(&self.values, 3).to_owned();
                    if !self.validate(token2, &word2) {
                        tool::abort();
                    }
                    let count = tool::atol(self.tag_value(b"count"));

                    if last_token != token1 {
                        if last_token != 0 {
                            if let Some(gram) = last_gram.take() {
                                store_gram(&self.bigram, last_token, &gram);
                            }
                        }
                        let gram = match self.bigram.get(&token1.to_le_bytes()) {
                            Some(value) => match decode_single_gram(&value) {
                                Ok((total, records)) => SingleGram { total, records },
                                Err(_) => tool::abort(),
                            },
                            None => SingleGram {
                                total: 0,
                                records: Vec::new(),
                            },
                        };
                        last_token = token1;
                        last_gram = Some(gram);
                    }
                    let Some(gram) = last_gram.as_mut() else {
                        // `assert(NULL != last_single_gram)`.
                        tool::abort()
                    };
                    let total = gram.total;
                    if gram
                        .records
                        .binary_search_by_key(&token2, |(next, _)| *next)
                        .is_ok()
                    {
                        // `check_result(insert_freq(...))` is false for a
                        // duplicate pair.
                        tool::abort();
                    }
                    let position = gram
                        .records
                        .binary_search_by_key(&token2, |(next, _)| *next)
                        .unwrap_or_else(|position| position);
                    gram.records.insert(position, (token2, count as u32));
                    gram.total = total.wrapping_add(count as u32);
                }
                END_LINE | GRAM_1_LINE | GRAM_2_LINE => break,
                _ => tool::abort(),
            }
            if !self.getline() {
                break;
            }
        }
        if last_token != 0 {
            if let Some(gram) = last_gram.take() {
                store_gram(&self.bigram, last_token, &gram);
            }
        }
        self.taglib.pop_state();
    }
}

/// `bigram->store(token, single_gram)` (`src/storage/ngram_tkrzwdb.cpp:124-133`).
fn store_gram(bigram: &Hash, token: u32, gram: &SingleGram) {
    let value = encode_single_gram(gram.total, &gram.records);
    let _ = bigram.put(&token.to_le_bytes(), &value);
}
