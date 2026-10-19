//! A phrase's stored words, and the string the pin makes of them.
//!
//! The pin keeps a phrase as `ucs4_t phrase[L]` and the item header's `L`
//! (`PhraseItem::get_phrase_length`, `PhraseItem::get_phrase_string`). It
//! makes a string only where a caller needs one: `g_ucs4_to_utf8(phrase, L,
//! NULL, NULL, NULL)` in `_token_get_phrase` (`pinyin.cpp:1644`,
//! `zhuyin.cpp:1112`), `convert_to_utf8` (`lookup.cpp:54,58`) and the
//! export iterators (`pinyin.cpp:715,822,828`). GLib 2.89.4, the GLib of
//! the pinned oracle image (`glib/gutf8.c:1023-1070`), reads the words in
//! order and:
//!
//! * stops at the first zero word (`:1037-1038`): what follows is never
//!   examined, not even a value out of range;
//! * writes any other value below 0x80000000 as the one to six bytes of
//!   `g_unichar_to_utf8` (`:549-599`), which are not UTF-8 from a surrogate
//!   on;
//! * answers NULL when it meets a value of 0x80000000 or more before a zero
//!   (`:1040-1045`).
//!
//! [`pin_string`] is that function. [`PhraseString`] is what it answers: the
//! bytes, or NULL. [`PhraseWords`] is what it reads: the words of a stored
//! phrase, as text while every word is a Unicode scalar value (every phrase a
//! UTF-8 import makes) and as the raw words otherwise. A phrase is read as
//! words wherever it is carried on (the user store, a profile file, a
//! prediction that starts behind a prefix) and as a string wherever the pin
//! makes one.

use core::fmt;
use std::borrow::Cow;

use compact_str::CompactString;

/// The number of bytes `g_unichar_to_utf8` writes for `code`
/// (`glib/gutf8.c:549-599`, `UTF8_LENGTH`): one to six.
#[must_use]
pub const fn unichar_len(code: u32) -> usize {
    match code {
        0..=0x7f => 1,
        0x80..=0x7ff => 2,
        0x800..=0xffff => 3,
        0x1_0000..=0x1f_ffff => 4,
        0x20_0000..=0x3ff_ffff => 5,
        _ => 6,
    }
}

/// The bytes `g_unichar_to_utf8` writes for `code`, at the front of `bytes`;
/// returns how many (`glib/gutf8.c:549-599`): one to six, for any value, with
/// no check that the value is a Unicode scalar value. A value of 0x80000000
/// or more gets the six-byte form with the lead byte 0xFE or 0xFF.
pub fn encode_unichar(code: u32, bytes: &mut [u8; 6]) -> usize {
    let length = unichar_len(code);
    let first = match length {
        1 => 0x00_u8,
        2 => 0xc0,
        3 => 0xe0,
        4 => 0xf0,
        5 => 0xf8,
        _ => 0xfc,
    };
    let mut rest = code;
    for byte in bytes.iter_mut().take(length).skip(1).rev() {
        *byte = (rest & 0x3f) as u8 | 0x80;
        rest >>= 6;
    }
    if let Some(lead) = bytes.first_mut() {
        // `outbuf[0] = c | first`, kept to the low eight bits as in C.
        *lead = (rest & 0xff) as u8 | first;
    }
    length
}

/// Appends the bytes `g_unichar_to_utf8` writes for `code` ([`encode_unichar`]).
pub fn push_unichar(out: &mut Vec<u8>, code: u32) {
    let mut bytes = [0_u8; 6];
    let length = encode_unichar(code, &mut bytes);
    out.extend(bytes.iter().take(length));
}

/// The length in bytes of [`pin_string`] of `words`, without making it:
/// `None` for the NULL it answers.
#[must_use]
pub fn pin_string_len(words: impl IntoIterator<Item = u32>) -> Option<usize> {
    let mut length = 0_usize;
    for word in words {
        if word == 0 {
            break;
        }
        if word >= 0x8000_0000 {
            return None;
        }
        length = length.saturating_add(unichar_len(word));
    }
    Some(length)
}

/// Writes [`pin_string`] of `words` to the front of `out`, stopping before a
/// word whose bytes would not fit; returns the bytes written. A word of
/// 0x80000000 or more is not written (the string is NULL: see
/// [`pin_string_len`]).
pub fn write_pin_string(words: impl IntoIterator<Item = u32>, out: &mut [u8]) -> usize {
    let mut at = 0_usize;
    for word in words {
        if word == 0 || word >= 0x8000_0000 {
            break;
        }
        let mut bytes = [0_u8; 6];
        let length = encode_unichar(word, &mut bytes);
        let Some(slot) = at.checked_add(length).and_then(|end| out.get_mut(at..end)) else {
            break;
        };
        slot.copy_from_slice(&bytes[..length]);
        at += length;
    }
    at
}

/// `g_ucs4_to_utf8(words, len, NULL, NULL, NULL)` (`glib/gutf8.c:1023-1070`):
/// the bytes of the words before the first zero, or `None` for the NULL it
/// answers when a word of 0x80000000 or more comes before any zero.
///
/// The bytes hold no zero byte. They are UTF-8 exactly when every word is a
/// Unicode scalar value.
#[must_use]
pub fn pin_string(words: &[u32]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(words.len().saturating_mul(3));
    for &word in words {
        if word == 0 {
            break;
        }
        if word >= 0x8000_0000 {
            return None;
        }
        push_unichar(&mut out, word);
    }
    Some(out)
}

/// The text up to its first U+0000: the string the pin makes of a phrase
/// kept one `char` per stored word.
///
/// A text with no U+0000, the only kind a UTF-8 import can make, is
/// returned whole.
#[must_use]
pub fn phrase_c_string(text: &str) -> &str {
    text.split_once('\0').map_or(text, |(string, _)| string)
}

/// The string the pin hands out for a phrase, a candidate or a sentence: a
/// C string, or NULL.
///
/// A string holds no zero byte. It is valid UTF-8 unless it comes from a
/// stored word that is not a Unicode scalar value (a surrogate, or a value
/// above U+10FFFF), and it is NULL when the pin's conversion answers NULL.
/// The common case is inline text and costs no allocation.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PhraseString(StringRepr);

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum StringRepr {
    /// Valid UTF-8, with no zero byte.
    Text(CompactString),
    /// Bytes that are not valid UTF-8, with no zero byte.
    Bytes(Box<[u8]>),
    /// The pin's NULL.
    Null,
}

impl PhraseString {
    /// The pin's NULL.
    pub const NULL: Self = Self(StringRepr::Null);

    /// The string of `text`: the text up to its first U+0000.
    #[must_use]
    pub fn from_text(text: impl Into<CompactString>) -> Self {
        let text = text.into();
        match text.find('\0') {
            None => Self(StringRepr::Text(text)),
            Some(end) => Self(StringRepr::Text(CompactString::from(&text[..end]))),
        }
    }

    /// The string of `bytes`: the bytes up to the first zero byte.
    #[must_use]
    pub fn from_bytes(mut bytes: Vec<u8>) -> Self {
        if let Some(end) = bytes.iter().position(|&byte| byte == 0) {
            bytes.truncate(end);
        }
        match String::from_utf8(bytes) {
            Ok(text) => Self(StringRepr::Text(CompactString::from(text))),
            Err(error) => Self(StringRepr::Bytes(error.into_bytes().into_boxed_slice())),
        }
    }

    /// The string [`pin_string`] makes of `words`.
    #[must_use]
    pub fn from_words(words: &[u32]) -> Self {
        pin_string(words).map_or(Self::NULL, Self::from_bytes)
    }

    /// [`Self::from_words`] for a dictionary hit: the words come from an
    /// iterator that can be read twice, and a phrase of scalar values (the
    /// only kind a real data file holds) is built straight into inline text,
    /// so a hit whose UTF-8 is at most 24 bytes costs no heap allocation.
    #[must_use]
    pub fn from_word_iter(words: impl Iterator<Item = u32> + Clone) -> Self {
        let mut text = CompactString::default();
        for code in words.clone() {
            if code == 0 {
                break;
            }
            match char::from_u32(code) {
                Some(ch) => text.push(ch),
                None => return Self::from_raw_words(words),
            }
        }
        Self(StringRepr::Text(text))
    }

    /// The words of [`Self::from_word_iter`] one of which, in front of the
    /// first zero, is not a scalar value. Out of line: no real data file
    /// holds such a word.
    #[cold]
    #[inline(never)]
    fn from_raw_words(words: impl Iterator<Item = u32>) -> Self {
        Self::from_words(&words.collect::<Vec<_>>())
    }

    /// The bytes, or `None` for NULL.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match &self.0 {
            StringRepr::Text(text) => Some(text.as_bytes()),
            StringRepr::Bytes(bytes) => Some(bytes),
            StringRepr::Null => None,
        }
    }

    /// The text, or `None` for NULL and for bytes that are not UTF-8.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match &self.0 {
            StringRepr::Text(text) => Some(text),
            StringRepr::Bytes(_) | StringRepr::Null => None,
        }
    }

    /// Whether this is the pin's NULL.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self.0, StringRepr::Null)
    }

    /// The text for a reader that shows it: the bytes with U+FFFD for each
    /// byte run that is not UTF-8, and the empty text for NULL.
    #[must_use]
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        match &self.0 {
            StringRepr::Text(text) => Cow::Borrowed(text),
            StringRepr::Bytes(bytes) => String::from_utf8_lossy(bytes),
            StringRepr::Null => Cow::Borrowed(""),
        }
    }
}

impl Default for PhraseString {
    /// The empty string, which is not NULL.
    fn default() -> Self {
        Self(StringRepr::Text(CompactString::const_new("")))
    }
}

impl From<&str> for PhraseString {
    fn from(text: &str) -> Self {
        Self::from_text(text)
    }
}

impl From<String> for PhraseString {
    fn from(text: String) -> Self {
        Self::from_text(text)
    }
}

impl From<CompactString> for PhraseString {
    fn from(text: CompactString) -> Self {
        Self::from_text(text)
    }
}

impl PartialEq<str> for PhraseString {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == Some(other)
    }
}

impl PartialEq<&str> for PhraseString {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}

impl PartialEq<String> for PhraseString {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == Some(other.as_str())
    }
}

impl fmt::Display for PhraseString {
    /// For a reader that shows the string: the text, U+FFFD for each byte run
    /// that is not UTF-8, and `(null)` for NULL, as glibc's `%s` prints it.
    /// Not for data: it is lossy.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            StringRepr::Text(text) => formatter.write_str(text),
            StringRepr::Bytes(bytes) => formatter.write_str(&String::from_utf8_lossy(bytes)),
            StringRepr::Null => formatter.write_str("(null)"),
        }
    }
}

impl fmt::Debug for PhraseString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            StringRepr::Text(text) => fmt::Debug::fmt(text.as_str(), formatter),
            StringRepr::Bytes(bytes) => write!(formatter, "b\"{}\"", bytes.escape_ascii()),
            StringRepr::Null => formatter.write_str("NULL"),
        }
    }
}

/// The joiner `convert_to_utf8` is (`lookup.cpp:61-67` with an empty
/// delimiter): the strings of the phrases in order. A NULL string adds
/// nothing, and the result is NULL while no string was added.
#[derive(Clone, Debug, Default)]
pub struct PhraseStringJoin {
    bytes: Vec<u8>,
    added: bool,
}

impl PhraseStringJoin {
    /// A joiner with nothing added.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            added: false,
        }
    }

    /// Adds the string of one phrase.
    pub fn push(&mut self, string: &PhraseString) {
        if let Some(bytes) = string.as_bytes() {
            self.bytes.extend_from_slice(bytes);
            self.added = true;
        }
    }

    /// Adds text that is not a phrase string, such as input typed but not
    /// chosen. Empty text adds nothing.
    pub fn push_str(&mut self, text: &str) {
        if !text.is_empty() {
            self.bytes.extend_from_slice(text.as_bytes());
            self.added = true;
        }
    }

    /// Whether nothing was added.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        !self.added
    }

    /// The bytes added so far. They are empty both when nothing was added and
    /// when only empty strings were.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Forgets everything added.
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.added = false;
    }

    /// The joined string.
    #[must_use]
    pub fn string(&self) -> PhraseString {
        if self.added {
            PhraseString::from_bytes(self.bytes.clone())
        } else {
            PhraseString::NULL
        }
    }
}

/// The words of a stored phrase, `ucs4_t phrase[L]`.
///
/// Text while every word is a Unicode scalar value (a zero word is U+0000);
/// the raw words when one is not, so the phrase round-trips whatever a data
/// file holds. [`Self::string`] is the string the pin makes of it.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PhraseWords(WordsRepr);

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum WordsRepr {
    /// Every word is a Unicode scalar value.
    Text(CompactString),
    /// At least one word is not a Unicode scalar value.
    Words(Box<[u32]>),
}

impl PhraseWords {
    /// The words of `text`, one per character.
    #[must_use]
    pub fn from_text(text: impl Into<CompactString>) -> Self {
        Self(WordsRepr::Text(text.into()))
    }

    /// The phrase of `words`.
    #[must_use]
    pub fn from_words(words: impl IntoIterator<Item = u32>) -> Self {
        let words: Vec<u32> = words.into_iter().collect();
        match words
            .iter()
            .map(|&word| char::from_u32(word))
            .collect::<Option<CompactString>>()
        {
            Some(text) => Self(WordsRepr::Text(text)),
            None => Self(WordsRepr::Words(words.into_boxed_slice())),
        }
    }

    /// How many words the phrase has: the item header's length.
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.0 {
            WordsRepr::Text(text) => text.chars().count(),
            WordsRepr::Words(words) => words.len(),
        }
    }

    /// Whether the phrase has no word.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match &self.0 {
            WordsRepr::Text(text) => text.is_empty(),
            WordsRepr::Words(words) => words.is_empty(),
        }
    }

    /// The text, when every word is a Unicode scalar value.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match &self.0 {
            WordsRepr::Text(text) => Some(text),
            WordsRepr::Words(_) => None,
        }
    }

    /// The words in order.
    pub fn words(&self) -> impl Iterator<Item = u32> + '_ {
        let (text, words) = match &self.0 {
            WordsRepr::Text(text) => (Some(text.chars()), None),
            WordsRepr::Words(words) => (None, Some(words.iter().copied())),
        };
        text.into_iter()
            .flatten()
            .map(u32::from)
            .chain(words.into_iter().flatten())
    }

    /// The string the pin makes of the whole phrase.
    #[must_use]
    pub fn string(&self) -> PhraseString {
        match &self.0 {
            WordsRepr::Text(text) => PhraseString::from_text(phrase_c_string(text)),
            WordsRepr::Words(words) => PhraseString::from_words(words),
        }
    }

    /// The string the pin makes from word `begin` on
    /// (`_token_get_phrase(..., begin, ...)`, `pinyin.cpp:1644`): the
    /// conversion starts there, so a zero word in front of `begin` does not
    /// stop it. Empty from the end on.
    #[must_use]
    pub fn string_from(&self, begin: usize) -> PhraseString {
        match &self.0 {
            WordsRepr::Text(text) => {
                let tail = text
                    .char_indices()
                    .nth(begin)
                    .map_or("", |(start, _)| &text[start..]);
                PhraseString::from_text(phrase_c_string(tail))
            }
            WordsRepr::Words(words) => PhraseString::from_words(words.get(begin..).unwrap_or(&[])),
        }
    }

    /// The phrase as the bytes the user store keeps in its text columns: the
    /// UTF-8 of the text, or each word written as `g_unichar_to_utf8` writes
    /// it. The two agree for text, so a store written before non-scalar
    /// words could be carried reads back unchanged.
    #[must_use]
    pub fn to_store_bytes(&self) -> Cow<'_, [u8]> {
        match &self.0 {
            WordsRepr::Text(text) => Cow::Borrowed(text.as_bytes()),
            WordsRepr::Words(words) => {
                let mut out = Vec::with_capacity(words.len().saturating_mul(3));
                for &word in words.iter() {
                    push_unichar(&mut out, word);
                }
                Cow::Owned(out)
            }
        }
    }

    /// The phrase [`Self::to_store_bytes`] wrote; `None` for bytes it cannot
    /// have written (an overlong or truncated sequence).
    #[must_use]
    pub fn from_store_bytes(bytes: &[u8]) -> Option<Self> {
        if let Ok(text) = core::str::from_utf8(bytes) {
            return Some(Self::from_text(text));
        }
        let mut words = Vec::new();
        let mut rest = bytes;
        while let Some((&lead, tail)) = rest.split_first() {
            let (extra, mut word) = match lead {
                0x00..=0x7f => (0, u32::from(lead)),
                0xc0..=0xdf => (1, u32::from(lead & 0x1f)),
                0xe0..=0xef => (2, u32::from(lead & 0x0f)),
                0xf0..=0xf7 => (3, u32::from(lead & 0x07)),
                0xf8..=0xfb => (4, u32::from(lead & 0x03)),
                0xfc..=0xff => (5, u32::from(lead & 0x03)),
                0x80..=0xbf => return None,
            };
            let (continuation, after) = tail.split_at_checked(extra)?;
            for &byte in continuation {
                if byte & 0xc0 != 0x80 {
                    return None;
                }
                word = (word << 6) | u32::from(byte & 0x3f);
            }
            words.push(word);
            rest = after;
        }
        // Only the canonical (shortest) form is a store value: a word that
        // writes back to other bytes names no phrase.
        let phrase = Self::from_words(words);
        let canonical = phrase.to_store_bytes().as_ref() == bytes;
        canonical.then_some(phrase)
    }
}

impl Default for PhraseWords {
    /// The empty phrase.
    fn default() -> Self {
        Self(WordsRepr::Text(CompactString::const_new("")))
    }
}

impl From<&str> for PhraseWords {
    fn from(text: &str) -> Self {
        Self::from_text(text)
    }
}

impl From<String> for PhraseWords {
    fn from(text: String) -> Self {
        Self::from_text(text)
    }
}

impl PartialEq<str> for PhraseWords {
    fn eq(&self, other: &str) -> bool {
        self.as_text() == Some(other)
    }
}

impl PartialEq<&str> for PhraseWords {
    fn eq(&self, other: &&str) -> bool {
        self.as_text() == Some(*other)
    }
}

impl PartialEq<String> for PhraseWords {
    fn eq(&self, other: &String) -> bool {
        self.as_text() == Some(other.as_str())
    }
}

impl fmt::Debug for PhraseWords {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            WordsRepr::Text(text) => fmt::Debug::fmt(text.as_str(), formatter),
            WordsRepr::Words(words) => {
                formatter.write_str("[")?;
                for (index, word) in words.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{word:#x}")?;
                }
                formatter.write_str("]")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PhraseString, PhraseStringJoin, PhraseWords, phrase_c_string, pin_string, pin_string_len,
        push_unichar, unichar_len, write_pin_string,
    };
    use compact_str::CompactString;

    /// The 32-bit values worth trying: both sides of every encoding limit, the
    /// surrogates, the Unicode limit, the 31-bit limit and the top.
    const EDGES: [u32; 24] = [
        0x01,
        0x7f,
        0x80,
        0x7ff,
        0x800,
        0xd7ff,
        0xd800,
        0xdbff,
        0xdc00,
        0xdfff,
        0xe000,
        0xffff,
        0x1_0000,
        0x10_ffff,
        0x11_0000,
        0x1f_ffff,
        0x20_0000,
        0x3ff_ffff,
        0x400_0000,
        0x7fff_ffff,
        0x8000_0000,
        0xc000_0000,
        0xffff_fffe,
        0xffff_ffff,
    ];

    fn encoded(code: u32) -> Vec<u8> {
        let mut out = Vec::new();
        push_unichar(&mut out, code);
        out
    }

    #[test]
    fn a_scalar_value_encodes_as_utf8() {
        for ch in ['a', 'é', '你', '𠀀', '\u{10ffff}'] {
            let mut buf = [0_u8; 4];
            assert_eq!(encoded(u32::from(ch)), ch.encode_utf8(&mut buf).as_bytes());
        }
    }

    #[test]
    fn the_glib_sequences_for_values_utf8_does_not_have() {
        // The bytes the pin printed in the #894 cases.
        assert_eq!(encoded(0xd800), [0xed, 0xa0, 0x80]);
        assert_eq!(encoded(0x11_0000), [0xf4, 0x90, 0x80, 0x80]);
        assert_eq!(
            encoded(0x7fff_ffff),
            [0xfd, 0xbf, 0xbf, 0xbf, 0xbf, 0xbf],
            "31 bits: a six-byte sequence"
        );
        assert_eq!(encoded(0x20_0000), [0xf8, 0x88, 0x80, 0x80, 0x80]);
        assert_eq!(encoded(0x400_0000), [0xfc, 0x84, 0x80, 0x80, 0x80, 0x80]);
        assert_eq!(
            encoded(0xffff_ffff),
            [0xff, 0xbf, 0xbf, 0xbf, 0xbf, 0xbf],
            "above 0x7FFFFFFF the lead byte is 0xFE or 0xFF"
        );
    }

    #[test]
    fn every_value_has_a_prefix_free_encoding_of_one_to_six_bytes() {
        for code in EDGES {
            let bytes = encoded(code);
            assert!((1..=6).contains(&bytes.len()), "{code:#x}");
            assert!(!bytes.contains(&0), "{code:#x}");
            let lead = bytes[0];
            let expected = match lead {
                0x00..=0x7f => 1,
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf7 => 4,
                0xf8..=0xfb => 5,
                _ => 6,
            };
            assert_eq!(bytes.len(), expected, "{code:#x}");
        }
    }

    #[test]
    fn the_streaming_helpers_agree_with_the_string() {
        let samples: [&[u32]; 9] = [
            &[],
            &[0],
            &[0x59d4, 0, 0x4f1a],
            &[0x51fa, 0xd800, 0x793e],
            &[0x535a, 0x11_0000, 0x9986],
            &[0x610f, 0x7fff_ffff, 0x5229],
            &[0x8ba1, 0x8000_0000, 0x673a],
            &[0x4fc4, 0, 0xffff_ffff],
            &[0x4fc4, 0xffff_ffff, 0],
        ];
        for words in samples {
            let expected = pin_string(words);
            assert_eq!(
                pin_string_len(words.iter().copied()),
                expected.as_ref().map(Vec::len),
                "{words:x?}"
            );
            let mut out = vec![0xaa_u8; 64];
            let written = write_pin_string(words.iter().copied(), &mut out);
            match &expected {
                Some(bytes) => assert_eq!(&out[..written], bytes.as_slice(), "{words:x?}"),
                // NULL: what comes before the offending word is written, the
                // word is not.
                None => assert!(written <= 64, "{words:x?}"),
            }
        }
        for code in EDGES {
            assert_eq!(encoded(code).len(), unichar_len(code), "{code:#x}");
        }
    }

    #[test]
    fn a_write_stops_before_a_word_that_does_not_fit() {
        let words = [0x59d4, 0x51fa, 0xd800, 0x793e];
        let full = pin_string(&words).expect("not NULL");
        for room in 0..=full.len() {
            let mut out = vec![0_u8; room];
            let written = write_pin_string(words, &mut out);
            assert!(written <= room);
            assert_eq!(&out[..written], &full[..written], "room {room}");
            // Whole words only: the next word would not have fit.
            let mut at = 0;
            for word in words {
                if at + unichar_len(word) > room {
                    break;
                }
                at += unichar_len(word);
            }
            assert_eq!(written, at, "room {room}");
        }
    }

    #[test]
    fn the_string_stops_at_the_first_zero() {
        assert_eq!(pin_string(&[]), Some(Vec::new()));
        assert_eq!(pin_string(&[0]), Some(Vec::new()));
        assert_eq!(pin_string(&[0x59d4, 0, 0x4f1a]), Some("委".into()));
        assert_eq!(pin_string(&[0, 0x52a1, 0x9662]), Some(Vec::new()));
        assert_eq!(pin_string(&[0x529e, 0x516c, 0]), Some("办公".into()));
        // A word behind the zero is never read, whatever it is.
        for bad in EDGES {
            assert_eq!(pin_string(&[0x4fc4, 0, bad]), Some("俄".into()), "{bad:#x}");
        }
    }

    #[test]
    fn a_value_of_0x80000000_or_more_before_a_zero_is_null() {
        for bad in [0x8000_0000, 0xc000_0000, 0xffff_ffff] {
            assert_eq!(pin_string(&[bad]), None, "{bad:#x}");
            assert_eq!(pin_string(&[0x59d4, bad, 0x4f1a]), None, "{bad:#x}");
            assert_eq!(pin_string(&[0x4fc4, bad, 0]), None, "{bad:#x}");
        }
    }

    #[test]
    fn other_values_become_bytes_that_may_not_be_utf8() {
        let bytes = pin_string(&[0x51fa, 0xd800, 0x793e]).expect("not NULL");
        assert_eq!(
            bytes,
            [0xe5, 0x87, 0xba, 0xed, 0xa0, 0x80, 0xe7, 0xa4, 0xbe]
        );
        assert!(core::str::from_utf8(&bytes).is_err());
        let string = PhraseString::from_words(&[0x51fa, 0xd800, 0x793e]);
        assert_eq!(string.as_bytes(), Some(bytes.as_slice()));
        assert_eq!(string.as_str(), None);
        assert!(!string.is_null());
        assert_eq!(string.to_string_lossy(), "出\u{fffd}\u{fffd}\u{fffd}社");
    }

    #[test]
    fn a_string_made_of_text_stays_text() {
        let string = PhraseString::from_words(&[0x4f60, 0x597d]);
        assert_eq!(string.as_str(), Some("你好"));
        assert_eq!(string, "你好");
        assert_eq!(PhraseString::from_text("委\0会"), "委");
        assert_eq!(PhraseString::from_bytes(b"ab\0cd".to_vec()), "ab");
        assert_eq!(PhraseString::default(), "");
        assert!(!PhraseString::default().is_null());
        assert!(PhraseString::NULL.is_null());
        assert_eq!(PhraseString::NULL.as_bytes(), None);
        assert_ne!(PhraseString::NULL, "");
    }

    #[test]
    fn a_string_costs_what_the_text_it_replaces_cost() {
        assert_eq!(
            core::mem::size_of::<PhraseString>(),
            core::mem::size_of::<CompactString>()
        );
        assert_eq!(
            core::mem::size_of::<Option<PhraseString>>(),
            core::mem::size_of::<CompactString>()
        );
    }

    #[test]
    fn the_join_adds_what_convert_to_utf8_adds() {
        let mut join = PhraseStringJoin::new();
        assert!(join.is_empty());
        assert!(join.string().is_null(), "no phrase: NULL");
        join.push(&PhraseString::NULL);
        assert!(join.string().is_null(), "NULL phrases only: NULL");
        join.push(&PhraseString::from_text(""));
        assert_eq!(join.string(), "", "an empty string is a string");
        join.push(&PhraseString::NULL);
        join.push(&PhraseString::from_text("北京市"));
        join.push(&PhraseString::NULL);
        join.push(&PhraseString::from_words(&[0x51fa, 0xd800]));
        assert_eq!(
            join.string().as_bytes(),
            Some(b"\xe5\x8c\x97\xe4\xba\xac\xe5\xb8\x82\xe5\x87\xba\xed\xa0\x80".as_slice())
        );
        let mut text = PhraseStringJoin::new();
        text.push_str("");
        assert!(text.is_empty(), "empty text adds nothing");
        text.push_str("nihao");
        assert_eq!(text.string(), "nihao");
        text.clear();
        assert!(text.string().is_null());
    }

    #[test]
    fn words_are_text_while_every_word_is_a_scalar_value() {
        let text = PhraseWords::from_words([0x4f60, 0, 0x597d]);
        assert_eq!(text.as_text(), Some("你\0好"));
        assert_eq!(text.len(), 3);
        assert_eq!(text.words().collect::<Vec<_>>(), [0x4f60, 0, 0x597d]);
        let words = PhraseWords::from_words([0x4f60, 0xd800, 0x597d]);
        assert_eq!(words.as_text(), None);
        assert_eq!(words.len(), 3);
        assert_eq!(words.words().collect::<Vec<_>>(), [0x4f60, 0xd800, 0x597d]);
        assert!(PhraseWords::default().is_empty());
        assert_eq!(PhraseWords::from_text("你好"), "你好");
        assert_eq!(PhraseWords::from_text("你好").len(), 2);
    }

    #[test]
    fn the_string_of_words_is_what_the_pin_makes_of_them() {
        for (words, string) in [
            (vec![0x59d4, 0, 0x4f1a], Some("委".as_bytes().to_vec())),
            (vec![0, 0x52a1, 0x9662], Some(Vec::new())),
            (vec![0x529e, 0x516c, 0], Some("办公".as_bytes().to_vec())),
            (vec![0x4fc4, 0, 0x8000_0000], Some("俄".as_bytes().to_vec())),
            (vec![0x8ba1, 0x8000_0000, 0x673a], None),
            (
                vec![0x51fa, 0xd800, 0x793e],
                Some(vec![0xe5, 0x87, 0xba, 0xed, 0xa0, 0x80, 0xe7, 0xa4, 0xbe]),
            ),
        ] {
            let phrase = PhraseWords::from_words(words.iter().copied());
            assert_eq!(phrase.string().as_bytes(), string.as_deref(), "{words:x?}");
            assert_eq!(phrase.len(), words.len(), "{words:x?}");
        }
    }

    #[test]
    fn a_string_that_starts_behind_a_prefix_is_not_stopped_by_a_zero_in_front() {
        // `_token_get_phrase(..., begin, ...)` converts from `begin` on.
        for phrase in [
            PhraseWords::from_words([0x4fc4, 0, 0x4f1a, 0x597d]),
            PhraseWords::from_words([0x4fc4, 0, 0xd800, 0x597d]),
        ] {
            assert_eq!(phrase.string(), "俄");
        }
        let text = PhraseWords::from_words([0x4fc4, 0, 0x4f1a, 0x597d]);
        assert_eq!(text.string_from(2), "会好");
        assert_eq!(text.string_from(1), "");
        assert_eq!(text.string_from(4), "");
        assert_eq!(text.string_from(9), "");
        let raw = PhraseWords::from_words([0x4fc4, 0, 0xd800, 0x597d]);
        assert_eq!(
            raw.string_from(2).as_bytes(),
            Some([0xed, 0xa0, 0x80, 0xe5, 0xa5, 0xbd].as_slice())
        );
        let null = PhraseWords::from_words([0x4fc4, 0, 0x8000_0000, 0x597d]);
        assert!(null.string_from(2).is_null());
        assert_eq!(null.string_from(3), "好");
    }

    #[test]
    fn the_store_bytes_of_text_are_its_utf8() {
        let text = PhraseWords::from_text("你好");
        assert_eq!(text.to_store_bytes().as_ref(), "你好".as_bytes());
        assert_eq!(PhraseWords::from_store_bytes("你好".as_bytes()), Some(text));
        assert_eq!(
            PhraseWords::from_store_bytes(b""),
            Some(PhraseWords::default())
        );
    }

    #[test]
    fn the_store_bytes_round_trip_every_value() {
        for code in EDGES {
            for words in [vec![code], vec![0x4f60, code, 0x597d], vec![code, 0, code]] {
                let phrase = PhraseWords::from_words(words.iter().copied());
                let bytes = phrase.to_store_bytes().into_owned();
                assert_eq!(
                    PhraseWords::from_store_bytes(&bytes),
                    Some(phrase),
                    "{words:x?}"
                );
            }
        }
    }

    #[test]
    fn bytes_the_store_never_writes_name_no_phrase() {
        for bytes in [
            &[0x80_u8][..],                  // a continuation byte alone
            &[0xc0, 0x80],                   // overlong form of U+0000
            &[0xc1, 0xbf],                   // overlong form of U+007F
            &[0xe0, 0x80, 0x80],             // overlong form
            &[0xe5, 0x87],                   // truncated
            &[0xe5, 0x87, 0x41],             // a continuation byte that is not one
            &[0xfc, 0x80, 0x80, 0x80, 0x80], // truncated six-byte form
            &[0xed, 0xa0, 0x80, 0xff],       // a lead byte with no continuation
        ] {
            assert_eq!(PhraseWords::from_store_bytes(bytes), None, "{bytes:x?}");
        }
    }

    #[test]
    fn the_string_is_the_text_up_to_the_first_zero() {
        assert_eq!(phrase_c_string("你好"), "你好");
        assert_eq!(phrase_c_string(""), "");
        assert_eq!(phrase_c_string("委\0会"), "委");
        assert_eq!(phrase_c_string("\0务院"), "");
        assert_eq!(phrase_c_string("办公\0"), "办公");
    }
}
