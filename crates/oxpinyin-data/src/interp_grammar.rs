//! One written copy of the taglib line grammar `interpolation2.text` is
//! written in.
//!
//! The model export is not a whitespace-separated table; it is a
//! *tagged-line* format, and upstream reads every line of it through one
//! function — `pinyin::taglib_read` (`src/storage/tag_utility.cpp:171-264`,
//! read from libpinyin at `074a2219`). `import_interpolation` registers
//! five tags against it and does nothing else with the text:
//!
//! ```text
//! \data     0 values, required `model`        (import_interpolation.cpp:73)
//! \end      0 values, no required tags        (:96)
//! \1-gram   0 values, no required tags        (:97)
//! \2-gram   0 values, no required tags        (:98)
//! \item     2 values, required `count`        (:128, inside \1-gram)
//! \item     4 values, required `count`        (:163, inside \2-gram)
//! ```
//!
//! A line is `<tag> <value>… <key> <value> <key> <value>…`: the tag
//! selects the entry, the next `m_num_of_values` tokens are positional,
//! and **everything after them is walked in key/value pairs** — not as
//! trailing fields. A key that is not a registered tag is warned over and
//! *its pair is skipped anyway* (`tag_utility.cpp:238-242`), so the walk
//! only ever inspects every second token. That is the part every
//! hand-rolled reader gets wrong: whether the line is accepted depends on
//! whether `count` happens to land on an inspected step — i.e. on the
//! **parity of the token count between the positional values and the
//! `count` keyword** — and not on how many fields the line has:
//!
//! ```text
//! \item 10 甲 count 5            → accepted, count 5      (0 between: even)
//! \item 10 甲 乙 count 5         → REFUSED                (1 between: odd)
//! \item 10 a b c count 5         → accepted, count 5      (2 between: even)
//!                                  and value 1 is `a` — `b`/`c` dropped
//! \item 10 x y z w count 5       → REFUSED                (3 between: odd)
//! \item 10 甲 count 5 foo bar    → accepted, count 5, `foo`/`bar` dropped
//! \item 10 甲 count 5 foo        → accepted, count 5, `foo` dropped
//! \item 10 甲 count               → REFUSED (key with no value)
//! ```
//!
//! Measured against the pin's own `tag_utility.cpp` (probe in
//! `docs/findings/interpolation2-grammar.md`), not inferred.
//!
//! # What upstream does that this module deliberately does not
//!
//! `taglib_read` reports refusal by returning `false`, and every caller
//! wraps it in `check_result` (`src/include/pinyin_utils.h:26-31`), which
//! is `assert(expr)` unless the build defines `NDEBUG` or
//! `G_DISABLE_ASSERT` — the pin's `configure.ac` defines neither, so the
//! pin-built tools **abort** on a refused line. Constitution item 4
//! (nothing panics on any input) rules that out here: a refusal is a typed
//! [`GrammarError`] and the caller decides. Two upstream crash paths are
//! answered the same way, both null-pointer dereferences rather than
//! aborts — see [`GrammarError::EmptyLine`] and
//! [`GrammarError::UnterminatedEscape`].
//!
//! The residue is recorded in `docs/findings/interpolation2-grammar.md`;
//! this file is the code half of that document.

use std::fmt;

/// Positional values an `\item` line carries inside `\1-gram`
/// (`import_interpolation.cpp:128`).
pub const UNIGRAM_VALUES: usize = 2;

/// Positional values an `\item` line carries inside `\2-gram`
/// (`import_interpolation.cpp:163`).
pub const BIGRAM_VALUES: usize = 4;

/// The `\data` line's required tag (`import_interpolation.cpp:73`).
const MODEL_TAG: &str = "model";

/// The `\item` line's required tag (`import_interpolation.cpp:128,163`).
const COUNT_TAG: &str = "count";

/// A line's tag — the entry `taglib_read` selected from `tokens[0]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tag {
    /// `\data`, the header line.
    Data,
    /// `\1-gram`, opening the unigram section.
    OneGram,
    /// `\2-gram`, opening the bigram section.
    TwoGram,
    /// `\end`, closing the body.
    End,
    /// `\item`, one record of the section in force.
    Item,
}

impl Tag {
    /// The literal `taglib_read` matches `tokens[0]` against.
    #[must_use]
    pub const fn literal(self) -> &'static str {
        match self {
            Self::Data => "\\data",
            Self::OneGram => "\\1-gram",
            Self::TwoGram => "\\2-gram",
            Self::End => "\\end",
            Self::Item => "\\item",
        }
    }
}

/// A line read under the grammar above.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Line<'a> {
    /// `\data <…> model <name>`. `model` is the required tag's value.
    Data {
        /// The `model` tag's value — `interpolation` for this format.
        model: &'a str,
    },
    /// `\1-gram`.
    OneGram,
    /// `\2-gram`.
    TwoGram,
    /// `\end`.
    End,
    /// `\item …`; the positional values were written into the caller's
    /// slots and `count` is the required tag's value.
    Item {
        /// The `count` tag's value, unparsed — upstream reads it with
        /// `atol` into a `glong` (`utils_helper.h:42-49`).
        count: &'a str,
    },
}

/// Why a line could not be read under the taglib grammar.
///
/// Every variant is a line the pin-built tools refuse. The first four are
/// upstream's `taglib_read` returning `false` (an `assert` abort under the
/// pin's build); the last two are upstream crashing outright.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum GrammarError {
    /// `tokens[0]` matched no registered tag
    /// (`tag_utility.cpp:196-197`). Upstream's callers `abort()` on the
    /// stale `line_type` this leaves behind.
    UnknownTag {
        /// The unmatched first token.
        tag: String,
    },
    /// The line ran out of tokens before the tag's positional values were
    /// filled (`tag_utility.cpp:202`).
    MissingValue {
        /// 0-based index of the value that had no token.
        index: usize,
    },
    /// A required tag's key was the line's last token, so it had no value
    /// (`tag_utility.cpp:246`).
    DanglingKey {
        /// The key left without a value.
        key: String,
    },
    /// A required tag never appeared in the key/value tail
    /// (`tag_utility.cpp:252-259`). This is what an odd-length tail
    /// produces when the pairwise walk swallows the keyword.
    MissingRequiredTag {
        /// The tag that was required and absent.
        tag: String,
    },
    /// The line held no tokens at all.
    ///
    /// **Upstream segfaults here.** `split_line` returns a zero-length
    /// `strv`, so `tokens[0]` is `NULL`, and `taglib_read` passes it
    /// straight to `strcmp` (`tag_utility.cpp:183,190`). Measured: a blank
    /// line inside the body kills `import_interpolation`.
    EmptyLine,
    /// A quoted token ended with a backslash and no following character.
    ///
    /// **Upstream segfaults here too**, by a different route:
    /// `split_line`'s `g_return_val_if_fail(*cur, NULL)`
    /// (`tag_utility.cpp:137`) makes the whole split return `NULL`, and
    /// `taglib_read` then dereferences it.
    UnterminatedEscape,
}

impl fmt::Display for GrammarError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTag { tag } => write!(formatter, "unknown line tag {tag:?}"),
            Self::MissingValue { index } => {
                write!(formatter, "line ends before value {index} of the tag")
            }
            Self::DanglingKey { key } => write!(formatter, "tag {key:?} has no value"),
            Self::MissingRequiredTag { tag } => {
                write!(formatter, "missing required tag {tag:?}")
            }
            Self::EmptyLine => write!(formatter, "line has no tokens"),
            Self::UnterminatedEscape => {
                write!(formatter, "quoted token ends with a trailing backslash")
            }
        }
    }
}

impl std::error::Error for GrammarError {}

/// Reads one line of the model export under the grammar above.
///
/// `values` is the caller's slot array; its length is the tag's
/// `m_num_of_values` and is only read for `\item` lines
/// ([`UNIGRAM_VALUES`] or [`BIGRAM_VALUES`]). Every other tag takes no
/// positional values and leaves the slots untouched. Nothing is
/// allocated: the slots and the returned values all borrow `line`.
///
/// # Errors
///
/// Returns [`GrammarError`] for every line the pin-built tools refuse; see
/// that type for which upstream site each variant stands in for.
pub fn read<'a>(line: &'a str, values: &mut [&'a str]) -> Result<Line<'a>, GrammarError> {
    let mut tokens = Tokens::new(line);
    let Some(tag_token) = tokens.next().transpose()? else {
        // tag_utility.cpp:183 — `tokens[0]` is NULL and `strcmp` takes it.
        return Err(GrammarError::EmptyLine);
    };

    // tag_utility.cpp:188-197 — a linear scan of the registered entries.
    let tag = [Tag::Data, Tag::OneGram, Tag::TwoGram, Tag::End, Tag::Item]
        .into_iter()
        .find(|candidate| candidate.literal() == tag_token)
        .ok_or_else(|| GrammarError::UnknownTag {
            tag: tag_token.to_owned(),
        })?;

    // tag_utility.cpp:201-205 — the positional values, one token each.
    // Only `\item` carries any; every other tag is registered with
    // `m_num_of_values` 0 and leaves the caller's slots untouched.
    let wanted = if tag == Tag::Item { values.len() } else { 0 };
    for (index, slot) in values.iter_mut().enumerate().take(wanted) {
        let Some(value) = tokens.next().transpose()? else {
            return Err(GrammarError::MissingValue { index });
        };
        *slot = value;
    }

    // tag_utility.cpp:210-249 — the tail, walked strictly in pairs.
    let required = match tag {
        Tag::Data => Some(MODEL_TAG),
        Tag::Item => Some(COUNT_TAG),
        Tag::OneGram | Tag::TwoGram | Tag::End => None,
    };
    let mut found: Option<&'a str> = None;
    while let Some(key) = tokens.next().transpose()? {
        // An un-required key is warned over and its pair skipped
        // (`:238-242`); a required one takes the next token as its value
        // (`:244-248`). Either way the walk advances by two.
        let Some(value) = tokens.next().transpose()? else {
            if required == Some(key) {
                // tag_utility.cpp:246 — the key was the last token.
                return Err(GrammarError::DanglingKey {
                    key: key.to_owned(),
                });
            }
            break;
        };
        if required == Some(key) {
            // g_hash_table_insert replaces, so the last pair wins.
            found = Some(value);
        }
    }

    // tag_utility.cpp:252-259 — every required tag must have been seen.
    let missing = |tag: &str| GrammarError::MissingRequiredTag {
        tag: tag.to_owned(),
    };
    match tag {
        Tag::OneGram => Ok(Line::OneGram),
        Tag::TwoGram => Ok(Line::TwoGram),
        Tag::End => Ok(Line::End),
        Tag::Data => found
            .map(|model| Line::Data { model })
            .ok_or_else(|| missing(MODEL_TAG)),
        Tag::Item => found
            .map(|count| Line::Item { count })
            .ok_or_else(|| missing(COUNT_TAG)),
    }
}

/// `split_line` (`tag_utility.cpp:118-169`) as a borrowing iterator.
///
/// Upstream builds a `gchar **` of freshly allocated tokens per line; this
/// yields `&str` slices of the line instead, which is the whole reason the
/// ~1.9M-line export can be read without a per-line allocation.
struct Tokens<'a> {
    line: &'a str,
    /// Byte offset of the next character to inspect.
    at: usize,
}

impl<'a> Tokens<'a> {
    const fn new(line: &'a str) -> Self {
        Self { line, at: 0 }
    }

    /// Character at `at`, with its UTF-8 width.
    fn peek(&self) -> Option<(char, usize)> {
        self.line[self.at..]
            .chars()
            .next()
            .map(|c| (c, c.len_utf8()))
    }
}

/// `g_unichar_isgraph` — "printable and not a space".
///
/// **Divergence, bounded and recorded:** GLib decides this from its own
/// Unicode tables, which also exclude format characters and unassigned
/// code points; this asks only whether the character is neither
/// whitespace nor a control character. The two agree on every ASCII
/// character and on every assigned printable character, which is the
/// whole of the pinned export (63,907 unigram and 1,849,609 bigram lines,
/// all ASCII digits, ASCII keywords and CJK phrase text). They differ
/// only on format/unassigned code points, which no producer of this
/// format emits. See `docs/findings/interpolation2-grammar.md`.
fn is_graph(c: char) -> bool {
    !c.is_whitespace() && !c.is_control()
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Result<&'a str, GrammarError>;

    fn next(&mut self) -> Option<Self::Item> {
        // tag_utility.cpp:127 — leading whitespace is skipped outright.
        while let Some((c, width)) = self.peek() {
            if c.is_whitespace() {
                self.at += width;
            } else {
                break;
            }
        }
        let (first, first_width) = self.peek()?;

        let token = if first == '"' {
            // tag_utility.cpp:129-147. The token is the text between the
            // quotes; upstream's own TODO records that `\"` is *not*
            // unescaped, so the backslash stays in the token.
            self.at += first_width;
            let start = self.at;
            while let Some((c, width)) = self.peek() {
                if c == '\\' {
                    self.at += width;
                    // tag_utility.cpp:137 — nothing after the backslash
                    // makes the whole split return NULL upstream.
                    let Some((_, escaped)) = self.peek() else {
                        return Some(Err(GrammarError::UnterminatedEscape));
                    };
                    self.at += escaped;
                } else if c == '"' {
                    break;
                } else {
                    self.at += width;
                }
            }
            &self.line[start..self.at]
        } else {
            // tag_utility.cpp:148-161 — a run of printable characters.
            let start = self.at;
            while let Some((c, width)) = self.peek() {
                if is_graph(c) {
                    self.at += width;
                } else {
                    break;
                }
            }
            &self.line[start..self.at]
        };

        // tag_utility.cpp:122 — the `for` header's own increment, which
        // runs after every token and steps over whatever stopped the run:
        // the space, the closing quote, or a character that is neither
        // space nor graph. That last case is why a control byte *between*
        // two printable runs does not split them into three tokens: it is
        // consumed here rather than starting an iteration of its own. One
        // that follows whitespace does start an iteration, and then yields
        // the empty token this same step walks past.
        if let Some((_, width)) = self.peek() {
            self.at += width;
        }
        Some(Ok(token))
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test module; the crate-level denials cover library builds"
)]
mod tests {
    use super::{BIGRAM_VALUES, GrammarError, Line, Tokens, UNIGRAM_VALUES, read};

    fn read_unigram(line: &str) -> Result<(String, String, String), GrammarError> {
        let mut values = [""; UNIGRAM_VALUES];
        match read(line, &mut values)? {
            Line::Item { count } => {
                Ok((values[0].to_owned(), values[1].to_owned(), count.to_owned()))
            }
            other => panic!("expected an \\item line, got {other:?}"),
        }
    }

    /// Every one of these was measured against the pin's own
    /// `tag_utility.cpp` before it was written down; see
    /// `docs/findings/interpolation2-grammar.md` for the probe and its
    /// output.
    #[test]
    fn item_lines_match_the_pin_measured_taglib() {
        // The shape every line in the pinned export has.
        assert_eq!(
            read_unigram("\\item 10 甲 count 5").unwrap(),
            ("10".to_owned(), "甲".to_owned(), "5".to_owned())
        );
        // A zero count is a value like any other: taglib returns true and
        // `add_unigram_frequency`'s overflow guard is `delta > 0 && …`.
        assert_eq!(
            read_unigram("\\item 10 甲 count 0").unwrap(),
            ("10".to_owned(), "甲".to_owned(), "0".to_owned())
        );
        // Odd tail: the pairwise walk reads `乙`/`count` as one key/value
        // pair, so the `count` keyword is consumed as a value and the
        // required tag is never seen.
        assert_eq!(
            read_unigram("\\item 10 甲 乙 count 5").unwrap_err(),
            GrammarError::MissingRequiredTag {
                tag: "count".to_owned()
            }
        );
        assert_eq!(
            read_unigram("\\item 10 a b count 5").unwrap_err(),
            GrammarError::MissingRequiredTag {
                tag: "count".to_owned()
            }
        );
        // Two between `甲`'s slot and `count`: even, so `count` lands on
        // an inspected step and the line reads — with the second
        // positional value `a`, `b`/`c` warned over and dropped. The
        // token↔word check is what then rejects this line upstream, not
        // the grammar.
        assert_eq!(
            read_unigram("\\item 10 a b c count 5").unwrap(),
            ("10".to_owned(), "a".to_owned(), "5".to_owned())
        );
        // Three between: odd again, so a longer line is refused where a
        // shorter one was accepted. Field *count* is not the rule.
        assert_eq!(
            read_unigram("\\item 10 x y z w count 5").unwrap_err(),
            GrammarError::MissingRequiredTag {
                tag: "count".to_owned()
            }
        );
        // Trailing junk after a good pair is dropped, both parities.
        assert_eq!(
            read_unigram("\\item 10 甲 count 5 foo bar").unwrap(),
            ("10".to_owned(), "甲".to_owned(), "5".to_owned())
        );
        assert_eq!(
            read_unigram("\\item 10 甲 count 5 foo").unwrap(),
            ("10".to_owned(), "甲".to_owned(), "5".to_owned())
        );
        // Short lines.
        assert_eq!(
            read_unigram("\\item 10 甲").unwrap_err(),
            GrammarError::MissingRequiredTag {
                tag: "count".to_owned()
            }
        );
        assert_eq!(
            read_unigram("\\item 10").unwrap_err(),
            GrammarError::MissingValue { index: 1 }
        );
    }

    #[test]
    fn unknown_tags_and_empty_lines_are_refusals_not_crashes() {
        let mut values = [""; UNIGRAM_VALUES];
        assert_eq!(
            read("\\frobnicate 1 2", &mut values).unwrap_err(),
            GrammarError::UnknownTag {
                tag: "\\frobnicate".to_owned()
            }
        );
        assert_eq!(
            read("hello world", &mut values).unwrap_err(),
            GrammarError::UnknownTag {
                tag: "hello".to_owned()
            }
        );
        // No leading backslash: `item` is not `\item`.
        assert_eq!(
            read("item 10 x count 5", &mut values).unwrap_err(),
            GrammarError::UnknownTag {
                tag: "item".to_owned()
            }
        );
        // Upstream dereferences a NULL `tokens[0]` for both of these.
        assert_eq!(read("", &mut values).unwrap_err(), GrammarError::EmptyLine);
        assert_eq!(
            read("   ", &mut values).unwrap_err(),
            GrammarError::EmptyLine
        );
    }

    #[test]
    fn section_tags_carry_no_values() {
        let mut values = [""; UNIGRAM_VALUES];
        assert_eq!(read("\\1-gram", &mut values).unwrap(), Line::OneGram);
        assert_eq!(read("\\2-gram", &mut values).unwrap(), Line::TwoGram);
        assert_eq!(read("\\end", &mut values).unwrap(), Line::End);
        // Zero required tags, so trailing junk is warned over, not fatal.
        assert_eq!(read("\\end trailing junk", &mut values).unwrap(), Line::End);
        assert_eq!(
            read("\\data model interpolation", &mut values).unwrap(),
            Line::Data {
                model: "interpolation"
            }
        );
        // `\data` requires `model`; taglib does not care about spacing.
        assert_eq!(
            read("\\data   model    interpolation", &mut values).unwrap(),
            Line::Data {
                model: "interpolation"
            }
        );
        assert_eq!(
            read("\\data", &mut values).unwrap_err(),
            GrammarError::MissingRequiredTag {
                tag: "model".to_owned()
            }
        );
    }

    #[test]
    fn bigram_item_lines_take_four_values() {
        let mut values = [""; BIGRAM_VALUES];
        assert_eq!(
            read("\\item 10 甲 20 乙 count 3", &mut values).unwrap(),
            Line::Item { count: "3" }
        );
        assert_eq!(values, ["10", "甲", "20", "乙"]);
        // One token short of the four positional values.
        assert_eq!(
            read("\\item 10 甲 20", &mut values).unwrap_err(),
            GrammarError::MissingValue { index: 3 }
        );
    }

    #[test]
    fn a_dangling_required_key_is_refused() {
        let mut values = [""; UNIGRAM_VALUES];
        // `count` is the last token: upstream's
        // `g_return_val_if_fail(i < num_of_tokens, false)` fires.
        assert_eq!(
            read("\\item 10 甲 count", &mut values).unwrap_err(),
            GrammarError::DanglingKey {
                key: "count".to_owned()
            }
        );
    }

    #[test]
    fn quoted_tokens_keep_their_escapes() {
        // `split_line` copies the quoted span verbatim — upstream's own
        // TODO says `\"` is not yet unescaped.
        let tokens: Vec<&str> = Tokens::new(r#"a "b c" d"#).map(|t| t.unwrap()).collect();
        assert_eq!(tokens, ["a", "b c", "d"]);
        let tokens: Vec<&str> = Tokens::new(r#""a\"b""#).map(|t| t.unwrap()).collect();
        assert_eq!(tokens, [r#"a\"b"#]);
        // A doubled backslash escapes the backslash, so the quote that
        // follows it closes the token and the rest of the line becomes a
        // token of its own — `"` is `isgraph`, so it stays in it. Measured
        // on the pin: this line is refused, because the two tokens push
        // `count` onto an odd step.
        let tokens: Vec<&str> = Tokens::new(r#""a\\"b""#).map(|t| t.unwrap()).collect();
        assert_eq!(tokens, [r"a\\", r#"b""#]);
        let mut values = [""; UNIGRAM_VALUES];
        assert_eq!(
            read(r#"\item 10 "a\\"b" count 5"#, &mut values).unwrap_err(),
            GrammarError::MissingRequiredTag {
                tag: "count".to_owned()
            }
        );
        // A quoted phrase containing a space is one token, so this line
        // reads where the same text unquoted would not.
        assert_eq!(
            read(r#"\item 10 "a b" count 5"#, &mut values).unwrap(),
            Line::Item { count: "5" }
        );
        assert_eq!(values, ["10", "a b"]);
        // A trailing backslash inside a quote makes upstream's split
        // return NULL, which `taglib_read` then dereferences.
        assert_eq!(
            Tokens::new("\"a\\").next().unwrap().unwrap_err(),
            GrammarError::UnterminatedEscape
        );
    }

    #[test]
    fn the_tokenizer_matches_split_whitespace_on_printable_input() {
        // Every line shape the pinned export contains is ASCII plus CJK
        // plus spaces, and on that input the `isgraph` run and a
        // whitespace split are the same partition.
        for line in [
            "\\item 10 甲 count 5",
            "  \\item  +10 甲 乙\tcount  007 ",
            "\\item 10 \u{3000}甲\u{3000} count 5",
            "\\item 10 甲\u{c}count 5",
            "\\item 10 x y count 5",
        ] {
            let ours: Vec<&str> = Tokens::new(line).map(|t| t.unwrap()).collect();
            let theirs: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(ours, theirs, "tokenizing {line:?}");
        }
        // A control byte is neither space nor graph, so it closes a run —
        // but the `for` increment then steps over it, so it does not
        // become a token boundary of its own.
        let ours: Vec<&str> = Tokens::new("a\u{1}b").map(|t| t.unwrap()).collect();
        assert_eq!(ours, ["a", "b"]);
        // Following whitespace, though, it does start an iteration, and
        // the run that starts on it is empty. `split_whitespace` would
        // give `["a", "\u{1}b"]` here; this is the one shape where the
        // two partitions differ, and no producer of this format emits it.
        let ours: Vec<&str> = Tokens::new("a \u{1}b").map(|t| t.unwrap()).collect();
        assert_eq!(ours, ["a", "", "b"]);
    }
}
