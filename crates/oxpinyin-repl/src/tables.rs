//! tests/storage/test_{phrase,chewing}_table.cpp at 074a2219.
use crate::{load_text_tables, present};
use oxpinyin_core::input::tool;
use oxpinyin_core::input::tool::table_tests as output;
use oxpinyin_core::{ChewingKey, OptionBits};
use oxpinyin_user::data::row_format::phrase_index::{decode_tokens, encode_ucs4_key_scalars};
use oxpinyin_user::data::{chewing_index_key as index_key, search_chewing_row as search_row};
use oxpinyin_user::tool::Index;
use oxpinyin_utils::storage::dbm::Tree;
use std::ops::Range;
use std::{ffi::CString, path::Path};

/// The source's compressed ranges, in library and stored item order.
pub(crate) type Ranges = [Vec<Range<u32>>; 16];

/// A text-loaded in-memory table and its enabled phrase libraries.
pub(crate) struct Tables {
    pub(crate) table: Tree,
    pub(crate) indices: [Option<Index>; 16],
}

impl Tables {
    pub(crate) fn load(data: &Path, chewing: bool) -> Self {
        let conf = crate::conf(data);
        let table = Tree::memory().unwrap_or_else(|_| tool::abort());
        let indices = if chewing {
            load_text_tables(data, &conf, Some(&table), None)
        } else {
            load_text_tables(data, &conf, None, Some(&table))
        };
        Self { table, indices }
    }

    pub(crate) fn enabled(&self, token: u32) -> bool {
        self.indices[oxpinyin_core::model::phrase_index_library(token)].is_some()
    }

    pub(crate) fn phrase_search(&self, words: &[u32]) -> (bool, [Vec<u32>; 16]) {
        let mut tokens = core::array::from_fn(|_| Vec::new());
        let Some(value) = self.table.get(&encode_ucs4_key_scalars(words)) else {
            return (false, tokens);
        };
        for token in decode_tokens(&value).unwrap_or_else(|_| tool::abort()) {
            if self.enabled(token) {
                tokens[oxpinyin_core::model::phrase_index_library(token)].push(token);
            }
        }
        (true, tokens)
    }

    pub(crate) fn chewing_search(&self, keys: &[ChewingKey]) -> (bool, Ranges) {
        let mut ranges: Ranges = core::array::from_fn(|_| Vec::new());
        // contains_incomplete_pinyin checks only the first incomplete key,
        // before either index-copy path (`pinyin_phrase3.h:146-177`).
        if let Some(key) = keys.iter().find(|key| key.middle == 0 && key.final_ == 0)
            && key.tone != 0
        {
            tool::abort(); // pinyin_phrase3.h:152, policy row 4.
        }
        // Approved row 116 refusal immediately before the would-be copy.
        if keys.len() > 16 {
            tool::exit(tool::EINVAL);
        }
        let value = self.table.get(&index_key(keys));
        let (continued, tokens) =
            search_row(keys, value.as_deref()).unwrap_or_else(|_| tool::abort());
        let mut cursor: Option<(usize, Range<u32>)> = None;
        for token in tokens {
            if !self.enabled(token) {
                continue;
            }
            let library = oxpinyin_core::model::phrase_index_library(token);
            // null_token marks an empty cursor even after consuming token 0.
            if cursor.as_ref().is_none_or(|(_, range)| range.start == 0) {
                cursor = Some((library, token..token.wrapping_add(1)));
                continue;
            }
            if let Some((index, range)) = &mut cursor
                && *index == library
                && range.end == token
            {
                range.end = range.end.wrapping_add(1);
                continue;
            }
            if let Some((index, range)) = cursor.take() {
                ranges[index].push(range);
            }
            cursor = Some((library, token..token.wrapping_add(1)));
        }
        if let Some((index, range)) = cursor
            && range.start != 0
        {
            ranges[index].push(range);
        }
        (continued, ranges)
    }

    fn dump_ranges(&self, ranges: &Ranges) {
        for (library, ranges) in ranges.iter().enumerate() {
            if !ranges.is_empty() {
                output::range_count(ranges.len());
            }
            for range in ranges {
                output::range(range.start, range.end);
                for token in range.clone() {
                    let item = present(
                        self.indices[library]
                            .as_ref()
                            .and_then(|index| index.item(token & 0x00ff_ffff)),
                    );
                    let text = present(tool::ucs4_to_utf8(&item.phrase));
                    output::phrase(&present(CString::new(text).ok()));
                    for (keys, frequency) in item.prons {
                        for key in keys {
                            let key = ChewingKey::from_packed(key);
                            output::pronunciation(&present(CString::new(key.pinyin_string()).ok()));
                        }
                        output::frequency(frequency);
                    }
                }
                output::newline();
            }
        }
    }
}

/// test_phrase_table: token lookup, continuation prefixes and final mask.
pub fn phrase_main(data: &Path) {
    let tables = Tables::load(data, false);
    let mut input = tool::LineReader::stdin();
    while input.getline() {
        if input.line() == b"quit" {
            break;
        }
        let line = present(
            CString::new(
                input
                    .line()
                    .split(|&byte| byte == 0)
                    .next()
                    .unwrap_or_default(),
            )
            .ok(),
        );
        let length = tool::utf8_strlen(&line);
        let (words, _) = tool::utf8_to_ucs4(&line);
        let words = words.unwrap_or_else(|| tool::exit(tool::EINVAL));
        if length == 0 {
            continue;
        }
        for length in 1..words.len() {
            if tables.phrase_search(&words[..length]).0 {
                output::continued(length);
            }
        }
        let (_, tokens) = tables.phrase_search(&words);
        if tokens.iter().any(|tokens| !tokens.is_empty()) {
            for token in tokens.iter().flatten() {
                output::token(*token);
            }
            output::newline();
        }
    }
    tables.table.mask_all();
}

/// test_chewing_table: full-pinyin queries and the source's exact range dump.
pub fn chewing_main(data: &Path) {
    let tables = Tables::load(data, true);
    let mut input = tool::LineReader::stdin();
    while input.getline() {
        if input.line() == b"quit" {
            break;
        }
        let parsed = oxpinyin_core::engine::tool::parse_full(
            input.line(),
            OptionBits::USE_TONE | OptionBits::PINYIN_INCOMPLETE,
        );
        if parsed.keys.is_empty() {
            output::invalid();
            continue;
        }
        let keys: Vec<_> = parsed.keys.into_iter().map(|key| key.key).collect();
        // Not a timing leftover: the pin's first whole-key search
        // (`tests/storage/test_chewing_table.cpp:86` at 074a2219) aborts or
        // refuses before any continuation line is printed.
        let _ = tables.chewing_search(&keys);
        for length in 1..keys.len() {
            if tables.chewing_search(&keys[..length]).0 {
                output::continued(length);
            }
        }
        let (_, ranges) = tables.chewing_search(&keys);
        tables.dump_ranges(&ranges);
    }
    tables.table.mask_all();
}

#[cfg(test)]
mod tests;
