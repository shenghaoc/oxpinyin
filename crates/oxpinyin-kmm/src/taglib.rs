//! The pin's line tagger, `src/storage/tag_utility.cpp` at libpinyin
//! 074a2219: a stack of tag registrations (`:70-104`, `:280-299`),
//! `split_line` (`:118-169`) and `taglib_read` (`:171-264`). The utilities
//! that read the pin's text models share it. Its character steps and
//! warnings are GLib's, through the tool boundary. Its
//! `g_return_val_if_fail` checks are plain Rust that answer the pin's value
//! without GLib's critical (maintainer ruling 2026-10-31 UTC).
//!
//! The pin reads a line's tokens through two undefined operations, which
//! [`TagLib::read`] answers as [`Undefined`] instead: a line without a token
//! hands `strcmp` the NULL `tokens[0]` (`:183`, `:190`), and a
//! `g_utf8_next_char` step from a truncated sequence at the line's end
//! moves past its NUL before the next `*cur` (`:122`, `:132-141`,
//! `:149-157`).

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_int};

use oxpinyin_input::tool;

/// One `tag_entry` (`:13-21`).
#[derive(Clone, Debug)]
struct TagEntry {
    line_type: c_int,
    line_tag: Vec<u8>,
    num_of_values: usize,
    required_tags: Vec<Vec<u8>>,
    ignored_tags: Vec<Vec<u8>>,
}

/// The `required` hash table `taglib_read` fills: `g_str_hash` keys, the
/// last insert of a key keeping its value.
pub type Required = HashMap<Vec<u8>, CString>;

/// Where the pin's `taglib_read` goes undefined on a line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Undefined {
    /// No token: `strcmp(entry->m_line_tag, NULL)` (`:183`, `:190`).
    NoToken,
    /// `split_line` answered NULL for a backslash ending a quoted token
    /// (`:137`); `tokens[0]` then reads through it (`:183`).
    UnterminatedEscape,
    /// A step from a truncated UTF-8 sequence at the line's end passes the
    /// NUL, and the next `*cur` reads outside the string.
    PastLineEnd,
}

impl Undefined {
    /// The reason oxpinyin's refusal names.
    #[must_use]
    pub const fn reason(self) -> &'static CStr {
        match self {
            Self::NoToken => c"line without a token",
            Self::UnterminatedEscape => c"escape at the end of the line",
            Self::PastLineEnd => c"truncated UTF-8 sequence at the end of the line",
        }
    }
}

/// `g_tagutils_stack` (`:68`): one array of entries per pushed state.
#[derive(Clone, Debug)]
pub struct TagLib {
    stack: Vec<Vec<TagEntry>>,
}

impl Default for TagLib {
    fn default() -> Self {
        Self::init()
    }
}

impl TagLib {
    /// `taglib_init` (`:70-79`): one empty state.
    #[must_use]
    pub fn init() -> Self {
        Self {
            stack: vec![Vec::new()],
        }
    }

    fn top(&self) -> &[TagEntry] {
        self.stack.last().map_or(&[], Vec::as_slice)
    }

    /// `taglib_add_tag` (`:81-104`): `false`, and nothing added, when the
    /// top state already has the line type or the tag. The tag lists are
    /// split with `g_strsplit_set(tags, ",:", -1)`.
    pub fn add_tag(
        &mut self,
        line_type: c_int,
        line_tag: &CStr,
        num_of_values: usize,
        required_tags: &CStr,
        ignored_tags: &CStr,
    ) -> bool {
        if self
            .top()
            .iter()
            .any(|entry| entry.line_type == line_type || entry.line_tag == line_tag.to_bytes())
        {
            return false;
        }
        let entry = TagEntry {
            line_type,
            line_tag: line_tag.to_bytes().to_vec(),
            num_of_values,
            required_tags: tool::strsplit_set(required_tags, c",:", -1),
            ignored_tags: tool::strsplit_set(ignored_tags, c",:", -1),
        };
        if let Some(top) = self.stack.last_mut() {
            top.push(entry);
        }
        true
    }

    /// `taglib_push_state` (`:280-291`): a copy of the top state.
    pub fn push_state(&mut self) {
        let copy = self.top().to_vec();
        self.stack.push(copy);
    }

    /// `taglib_pop_state` (`:293-299`). The pin asserts a pushed state;
    /// every caller pops only what it pushed.
    pub fn pop_state(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    /// `taglib_read(input_line, line_type, values, required)`
    /// (`:171-264`): the pin's answer, after its warnings,
    /// with `values` and `required` as it leaves them. `line_type` is set
    /// only once a tag matches, as the pin's is.
    ///
    /// # Errors
    ///
    /// [`Undefined`] where the pin's own read of the line is undefined.
    pub fn read(
        &self,
        input_line: &CStr,
        line_type: &mut c_int,
        values: &mut Vec<CString>,
        required: &mut Required,
    ) -> Result<bool, Undefined> {
        values.clear();
        required.clear();
        let Some(tokens) = split_line(input_line)? else {
            let _ = tool::strv_length_null();
            return Err(Undefined::UnterminatedEscape);
        };
        let num_of_tokens = tokens.len();
        let tag_array = self.top();
        let Some(line_tag) = tokens.first() else {
            if tag_array.is_empty() {
                return Ok(false);
            }
            return Err(Undefined::NoToken);
        };
        let Some(cur_entry) = tag_array
            .iter()
            .find(|entry| entry.line_tag == line_tag.to_bytes())
        else {
            return Ok(false);
        };
        *line_type = cur_entry.line_type;

        for i in 1..=cur_entry.num_of_values {
            // g_return_val_if_fail(i < num_of_tokens, false) (`:202`).
            let Some(value) = tokens.get(i) else {
                return Ok(false);
            };
            values.push(value.clone());
        }

        let mut i = cur_entry.num_of_values + 1;
        while i < num_of_tokens {
            let tmp = tokens[i].to_bytes();
            if cur_entry.ignored_tags.iter().any(|tag| tag == tmp) {
                i += 2;
                continue;
            }
            if !cur_entry.required_tags.iter().any(|tag| tag == tmp) {
                tool::warn_unexpected_tag(&tokens[i]);
                i += 2;
                continue;
            }
            let key = tmp.to_vec();
            i += 1;
            // g_return_val_if_fail(i < num_of_tokens, false) (`:246`).
            let Some(value) = tokens.get(i) else {
                return Ok(false);
            };
            required.insert(key, value.clone());
            i += 1;
        }

        for required_tag in &cur_entry.required_tags {
            if !required.contains_key(required_tag) {
                tool::warn_missed_required_tag(
                    &CString::new(required_tag.clone()).unwrap_or_default(),
                );
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// `split_line` (`:118-169`): the line's tokens, `None` where the pin
/// answers NULL.
///
/// # Errors
///
/// [`Undefined::PastLineEnd`] where the pin's next `*cur` would read past
/// the line's NUL.
fn split_line(line: &CStr) -> Result<Option<Vec<CString>>, Undefined> {
    const QUOTE: u32 = '"' as u32;
    const BACKSLASH: u32 = '\\' as u32;
    let bytes = line.to_bytes_with_nul();
    // `*cur`, inside the string or undefined.
    let at = |cur: usize| bytes.get(cur).copied().ok_or(Undefined::PastLineEnd);
    // `g_utf8_get_char(cur)`, called only where `*cur` was just read.
    let get_char = |cur: usize| tool::utf8_get_char_at(line, cur).ok_or(Undefined::PastLineEnd);
    let next = |cur: usize| cur + tool::utf8_next_char_step(bytes[cur]);
    let token = |begin: usize, end: usize| CString::new(&bytes[begin..end]).unwrap_or_default();

    let mut tokens = Vec::new();
    let mut cur = 0;
    while at(cur)? != 0 {
        let unichar = get_char(cur)?;
        let mut begin = cur;
        if tool::unichar_isspace(unichar) {
            cur = next(cur);
            continue;
        } else if unichar == QUOTE {
            cur = next(cur);
            begin = cur;
            while at(cur)? != 0 {
                let unichar = get_char(cur)?;
                if unichar == BACKSLASH {
                    cur = next(cur);
                    // g_return_val_if_fail(*cur, NULL) (`:137`).
                    if at(cur)? == 0 {
                        return Ok(None);
                    }
                } else if unichar == QUOTE {
                    break;
                }
                cur = next(cur);
            }
            tokens.push(token(begin, cur));
        } else {
            while at(cur)? != 0 {
                if tool::unichar_isgraph(get_char(cur)?) {
                    cur = next(cur);
                } else {
                    break;
                }
            }
            tokens.push(token(begin, cur));
        }
        if at(cur)? == 0 {
            break;
        }
        cur = next(cur);
    }
    Ok(Some(tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(line: &CStr) -> Result<Option<Vec<String>>, Undefined> {
        split_line(line).map(|tokens| {
            tokens.map(|tokens| {
                tokens
                    .iter()
                    .map(|token| token.to_string_lossy().into_owned())
                    .collect()
            })
        })
    }

    #[test]
    fn split_line_keeps_quotes_escapes_and_empty_tokens() {
        assert_eq!(
            split(c"\\data model \"k mixture model\" count 1").unwrap(),
            Some(vec![
                "\\data".into(),
                "model".into(),
                "k mixture model".into(),
                "count".into(),
                "1".into()
            ])
        );
        // The escape stays in the token; the closing quote is stepped over.
        assert_eq!(
            split(c"\"a\\\"b\"c d").unwrap(),
            Some(vec!["a\\\"b".into(), "c".into(), "d".into()])
        );
        // A character neither space nor graph ends a token, and starts an
        // empty one (U+200B ZERO WIDTH SPACE).
        assert_eq!(
            split(c"a\xe2\x80\x8bb").unwrap(),
            Some(vec!["a".into(), "b".into()])
        );
        assert_eq!(
            split(c"\xe2\x80\x8bb").unwrap(),
            Some(vec![String::new(), "b".into()])
        );
        assert_eq!(split(c"   ").unwrap(), Some(vec![]));
        assert_eq!(split(c"\"open").unwrap(), Some(vec!["open".into()]));
    }

    #[test]
    fn split_line_answers_the_pins_null_and_its_undefined_reads() {
        assert_eq!(split(c"\"a\\").unwrap(), None);
        assert_eq!(split(c"a \xe4"), Err(Undefined::PastLineEnd));
        assert_eq!(split(c"\"\xe4"), Err(Undefined::PastLineEnd));
        // Inside the line, a truncated sequence only makes odd tokens.
        // The lead byte's step of three lands on `b`.
        assert_eq!(
            split(c"\xe4 abc").unwrap(),
            Some(vec![String::new(), "bc".into()])
        );
    }

    fn tags() -> TagLib {
        let mut taglib = TagLib::init();
        assert!(taglib.add_tag(1, c"\\item", 2, c"freq", c"count"));
        taglib
    }

    #[test]
    fn read_walks_values_then_pairs() {
        let taglib = tags();
        let mut line_type = 0;
        let mut values = Vec::new();
        let mut required = Required::new();
        let read = taglib.read(
            c"\\item 10 甲 count 2 freq 3 freq 4",
            &mut line_type,
            &mut values,
            &mut required,
        );
        assert_eq!(read, Ok(true));
        assert_eq!(line_type, 1);
        assert_eq!(values, [c"10".to_owned(), c"甲".to_owned()]);
        assert_eq!(required[&b"freq".to_vec()].as_c_str(), c"4");
        assert_eq!(
            taglib.read(c"\\item 10", &mut line_type, &mut values, &mut required),
            Ok(false)
        );
        assert_eq!(
            taglib.read(c"\\other", &mut line_type, &mut values, &mut required),
            Ok(false)
        );
        assert_eq!(
            taglib.read(c"", &mut line_type, &mut values, &mut required),
            Err(Undefined::NoToken)
        );
    }

    #[test]
    fn add_tag_refuses_a_duplicate_type_or_tag_in_the_top_state() {
        let mut taglib = tags();
        assert!(!taglib.add_tag(1, c"\\other", 0, c"", c""));
        assert!(!taglib.add_tag(2, c"\\item", 0, c"", c""));
        taglib.push_state();
        assert!(!taglib.add_tag(2, c"\\item", 0, c"", c""));
        taglib.pop_state();
        assert!(taglib.add_tag(2, c"\\other", 0, c"", c""));
    }
}
