//! tests/storage/test_{phrase,chewing}_table.cpp at 074a2219.
use super::*;
use crate::storage::dbm::Tree;
use oxpinyin_core::{ChewingKey, PINYIN_INCOMPLETE, USE_TONE};
use oxpinyin_data::row_format::phrase_index::{decode_tokens, encode_ucs4_key_scalars};
use oxpinyin_data::{chewing_index_key as index_key, search_chewing_row as search_row};
use oxpinyin_input::tool::table_tests as output;
use std::ops::Range;

/// The source's compressed ranges, in library and stored item order.
pub(crate) type Ranges = [Vec<Range<u32>>; 16];

/// A text-loaded in-memory table and its enabled phrase libraries.
pub(crate) struct Tables {
    pub(crate) table: Tree,
    pub(crate) indices: [Option<Index>; 16],
}

impl Tables {
    pub(crate) fn load(chewing: bool) -> Self {
        let conf = conf();
        let table = Tree::memory().unwrap_or_else(|_| tool::abort());
        let indices = if chewing {
            load_text_tables(&conf, Some(&table), None)
        } else {
            load_text_tables(&conf, None, Some(&table))
        };
        Self { table, indices }
    }

    pub(crate) fn enabled(&self, token: u32) -> bool {
        self.indices[oxpinyin_core::phrase_index_library(token)].is_some()
    }

    pub(crate) fn phrase_search(&self, words: &[u32]) -> (bool, [Vec<u32>; 16]) {
        let mut tokens = core::array::from_fn(|_| Vec::new());
        let Some(value) = self.table.get(&encode_ucs4_key_scalars(words)) else {
            return (false, tokens);
        };
        for token in decode_tokens(&value).unwrap_or_else(|_| tool::abort()) {
            if self.enabled(token) {
                tokens[oxpinyin_core::phrase_index_library(token)].push(token);
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
        let (continued, items) =
            search_row(keys, value.as_deref()).unwrap_or_else(|_| tool::abort());
        let mut cursor: Option<(usize, Range<u32>)> = None;
        for item in items {
            if !self.enabled(item.token) {
                continue;
            }
            let library = oxpinyin_core::phrase_index_library(item.token);
            // null_token marks an empty cursor even after consuming token 0.
            if cursor.as_ref().is_none_or(|(_, range)| range.start == 0) {
                cursor = Some((library, item.token..item.token.wrapping_add(1)));
                continue;
            }
            if let Some((index, range)) = &mut cursor
                && *index == library
                && range.end == item.token
            {
                range.end = range.end.wrapping_add(1);
                continue;
            }
            if let Some((index, range)) = cursor.take() {
                ranges[index].push(range);
            }
            cursor = Some((library, item.token..item.token.wrapping_add(1)));
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

/// test_phrase_table: timed token lookup, continuation prefixes and final mask.
pub fn phrase_main() {
    let tables = Tables::load(false);
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
        let start = tool::timer::record_time();
        for _ in 0..1000 {
            let _ = tables.phrase_search(&words);
        }
        tool::timer::print_time(start, 1000);
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
pub fn chewing_main() {
    let tables = Tables::load(true);
    let mut input = tool::LineReader::stdin();
    while input.getline() {
        if input.line() == b"quit" {
            break;
        }
        let parsed = oxpinyin_engine::tool::parse_full(input.line(), USE_TONE | PINYIN_INCOMPLETE);
        if parsed.keys.is_empty() {
            output::invalid();
            continue;
        }
        let keys: Vec<_> = parsed.keys.into_iter().map(|key| key.key).collect();
        let start = tool::timer::record_time();
        for _ in 0..1000 {
            let _ = tables.chewing_search(&keys);
        }
        tool::timer::print_time(start, 1000);
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
