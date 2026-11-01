//! `mergeseq` — merge adjacent phrase runs into longer dictionary phrases
//! (`utils/segment/mergeseq.cpp` at 074a2219).
//!
//! Input is a segmented token stream: `{token} {phrase}` records and
//! `0 …` separators. `TAGLIB_PARSE_SEGMENTED_LINE`
//! (`utils/utils_helper.h:51-70`) splits with `g_strsplit_set(line, " \t",
//! 2)`, `abort()`s unless there are exactly two pieces, converts the token
//! with the C library's `atoi`, and `assert`s that the token's phrase text
//! equals the line's phrase column. A null-token line flushes the queue
//! and is echoed verbatim; end of input feeds a synthetic `"0 "` line
//! (`mergeseq.cpp:272-276`).

use std::ffi::{CStr, CString, OsString};

use oxpinyin_input::tool::{self, OutputStream};

use crate::segment::data;
use crate::segment::input::Input;
use crate::segment::phrase::PhraseSearch;
use crate::segment::{
    MAX_PHRASE_LENGTH, NULL_TOKEN, SENTENCE_START, abort_on_pin_assert, c_string, text_of,
};

/// A queued token and its length in characters (`TokenInfo`).
#[derive(Clone, Copy)]
struct TokenInfo {
    token: u32,
    token_len: usize,
}

/// The pin's rolling state: the queue's concatenated characters and the
/// queue itself (`mergeseq.cpp:260-261`).
struct Merger {
    unichars: Vec<u32>,
    queue: Vec<TokenInfo>,
}

impl Merger {
    const fn new() -> Self {
        Self {
            unichars: Vec::new(),
            queue: Vec::new(),
        }
    }

    /// `calculate_sequence_length` (`mergeseq.cpp:62-72`).
    fn sequence_length(&self) -> usize {
        self.queue.iter().map(|info| info.token_len).sum()
    }

    /// `taglib_token_to_string` (`src/storage/tag_utility.cpp:384-405`):
    /// the special `<start>` for `sentence_start`, the phrase item's text
    /// otherwise. `None` is the pin's NULL, after it wrote
    /// `error: unknown token`.
    fn token_text(&self, search: &PhraseSearch<'_>, token: u32) -> Option<CString> {
        if (token >> 24) & 0x0F == 0 {
            return if token == SENTENCE_START {
                Some(CString::new("<start>").unwrap_or_default())
            } else {
                tool::segmentation::report_unknown_token(token as i32);
                None
            };
        }
        match search.item(token) {
            Some(item) => text_of(&item.words),
            None => {
                tool::segmentation::report_unknown_token(token as i32);
                None
            }
        }
    }

    /// `taglib_validate_token_with_string` (`tag_utility.cpp:407-417`)
    /// then `check_result`'s `assert`. A token with no item, or an item
    /// GLib cannot convert, is where the pin's `strcmp(NULL, …)` is
    /// undefined: oxpinyin refuses instead of reproducing the fault.
    fn validate(&self, search: &PhraseSearch<'_>, token: u32, phrase: &CStr) -> bool {
        let Some(text) = self.token_text(search, token) else {
            tool::exit(1);
        };
        text.as_bytes() == phrase.to_bytes()
    }

    /// `merge_sequence` (`mergeseq.cpp:78-126`).
    fn merge_sequence(&mut self, search: &PhraseSearch<'_>) {
        if self.queue.is_empty() {
            abort_on_pin_assert();
        }
        let mut index = self.queue.len();
        let mut seq_len = self.sequence_length();
        let mut merged = None;
        while seq_len > 0 {
            let found = search.search(&self.unichars[..seq_len]);
            if found.ok() {
                merged = Some((found.first_token(), seq_len));
                break;
            }
            index -= 1;
            seq_len -= self.queue[index].token_len;
        }
        if let Some((token, token_len)) = merged {
            // Pop the merged prefix and prepend the one phrase covering it.
            self.queue.drain(0..index);
            self.queue.insert(0, TokenInfo { token, token_len });
        }
    }

    /// `pop_first_token` (`mergeseq.cpp:128-148`).
    fn pop_first_token(&mut self, output: &OutputStream) {
        let Some(info) = self.queue.first().copied() else {
            abort_on_pin_assert();
        };
        let words = &self.unichars[..info.token_len];
        let bytes = tool::ucs4_to_utf8(words);
        if bytes.is_none() && info.token_len != 0 {
            // `assert(read == token_len)`: the conversion wrote nothing.
            abort_on_pin_assert();
        }
        let text = bytes.as_deref().and_then(c_string);
        tool::segmentation::print_token_line(output, info.token as i32, text.as_deref());
        self.unichars.drain(0..info.token_len);
        self.queue.remove(0);
    }

    /// `feed_line` (`mergeseq.cpp:150-197`).
    fn feed_line(&mut self, search: &PhraseSearch<'_>, line: &CStr, output: &OutputStream) {
        let mut parts = Vec::new();
        let token = if line.to_bytes().is_empty() {
            // `if (0 == strlen(line)) break;`
            NULL_TOKEN
        } else {
            parts = tool::strsplit_set(line, c" \t", 2);
            if parts.len() != 2 {
                // `if (2 != g_strv_length(strs)) abort();`
                abort_on_pin_assert();
            }
            let token_field = c_string(&parts[0]).unwrap_or_default();
            tool::atoi(&token_field) as u32
        };

        if token == NULL_TOKEN {
            // Empty the queue, then echo the separator line verbatim.
            while !self.queue.is_empty() {
                self.merge_sequence(search);
                self.pop_first_token(output);
            }
            // `assert(0 == unichars->len); assert(0 == tokeninfos->len);`
            if !self.unichars.is_empty() || !self.queue.is_empty() {
                abort_on_pin_assert();
            }
            tool::segmentation::print_text_line(output, Some(line));
            return;
        }

        let phrase = c_string(&parts[1]).unwrap_or_default();
        if !self.validate(search, token, &phrase) {
            // `check_result(taglib_validate_token_with_string(...))`.
            abort_on_pin_assert();
        }

        // `phrase_index->get_phrase_item(token, item)` (answer ignored):
        // a special token has no item, and the pin's default `PhraseItem`
        // has length zero.
        let (words, token_len) = match search.item(token) {
            Some(item) => (item.words, item.length),
            None => (Vec::new(), 0),
        };
        self.queue.push(TokenInfo { token, token_len });
        self.unichars.extend(words);
        while self.sequence_length() >= MAX_PHRASE_LENGTH {
            self.merge_sequence(search);
            self.pop_first_token(output);
        }
    }
}

/// `main` (`mergeseq.cpp:200-281`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let _ = tool::set_locale_from_environment();
    let entries = [crate::segment::outputfile_entry()];
    let parsed = crate::segment::parse_options(c"- merge word sequence", &entries, args);
    let output = crate::segment::open_output(&parsed.values[0]);
    crate::segment::refuse_extra_arguments(&parsed.args);
    let mut input = Input::open(&parsed.args);
    let tables = data::load_or_exit(false);
    let search = PhraseSearch::new(tables.phrase.as_ref(), &tables.libraries);
    let mut merger = Merger::new();

    while input.getline() {
        if input.line().is_empty() {
            continue;
        }
        merger.feed_line(&search, input.c_str(), &output);
    }
    // `feed_line(..., "0 ", output)`: the stream's trailing separator.
    merger.feed_line(&search, c"0 ", &output);
    output.close_including_stdout();
}
