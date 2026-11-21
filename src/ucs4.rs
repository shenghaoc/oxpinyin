//! A phrase's text as the pin stores it: ordinary UTF-8, or the full
//! UCS-4 words when a word is not a nonzero Unicode scalar.

use core::hash::{Hash, Hasher};

use compact_str::CompactString;

/// A phrase's ordinary UTF-8 text or its full stored words. Equality,
/// ordering and hashing use the outward prefix before the first zero.
/// General Rust conversions preserve NUL and its suffix as stored words.
#[derive(Clone, Debug)]
pub struct PhraseText(TextStorage);

/// Private storage makes the no-NUL Scalar invariant construction-only.
/// Scalar contains exactly the text, with no metadata or cached length.
#[derive(Clone, Debug)]
enum TextStorage {
    Scalar(CompactString),
    Stored(Box<[u32]>),
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<PhraseText>() == 24);

// Only an unvalidated external str in PartialEq<str> reaches this helper.
// A Scalar's own text never needs a zero scan.
fn scalar_outward(text: &str) -> &str {
    text.split_once('\0').map_or(text, |(prefix, _)| prefix)
}

fn stored_outward(buffer: &[u32]) -> &[u32] {
    let words = buffer.get(1..).unwrap_or_default();
    words.split(|&word| word == 0).next().unwrap_or(words)
}

impl PhraseText {
    // convert_to_utf8 converts each phrase separately, lookup/lookup.cpp:39–68.
    /// Reads one phrase’s UCS-4 words. A word at or above `0x8000_0000` before the first zero
    /// rejects the phrase. A zero ends the outward text. That zero and its suffix remain
    /// available through [`Self::stored_words`].
    #[must_use]
    pub fn from_words(words: impl IntoIterator<Item = u32>) -> Option<Self> {
        Self::from_stored_words(words.into_iter().map(Some))
    }

    /// Reads every declared word, rejecting unreadable words and values at
    /// or above 0x80000000 before the first zero. This preserves the pin's
    /// per-item cut (`src/lookup/lookup.cpp:39-68` at 074a2219).
    #[must_use]
    pub(crate) fn from_stored_words(words: impl IntoIterator<Item = Option<u32>>) -> Option<Self> {
        let mut text = CompactString::new("");
        let mut raw: Option<Vec<u32>> = None;
        let mut past_zero = false;
        let mut total = 0_usize;
        for word in words {
            let word = word?;
            total = total.saturating_add(1);
            if !past_zero && word >= 0x8000_0000 {
                return None;
            }
            past_zero |= word == 0;
            if let Some(raw) = &mut raw {
                raw.push(word);
            } else if let Some(character) = char::from_u32(word).filter(|_| word != 0) {
                text.push(character);
            } else {
                let mut collected: Vec<u32> = text.chars().map(u32::from).collect();
                collected.push(word);
                raw = Some(collected);
            }
        }
        Some(match raw {
            Some(words) => Self::stored(words, total),
            None => Self(TextStorage::Scalar(text)),
        })
    }

    /// Builds stored words with a declared count for ranking. Counts above
    /// `u32::MAX` are stored as `u32::MAX`.
    #[must_use]
    pub fn from_stored_parts(words: Vec<u32>, length: usize) -> Self {
        Self::stored(words, length)
    }

    fn stored(words: Vec<u32>, length: usize) -> Self {
        let mut buffer = Vec::with_capacity(words.len().saturating_add(1));
        buffer.push(u32::try_from(length).unwrap_or(u32::MAX));
        buffer.extend(words);
        Self(TextStorage::Stored(buffer.into_boxed_slice()))
    }

    /// The ordinary NUL-free text, or None for stored words.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match &self.0 {
            TextStorage::Scalar(text) => Some(text),
            TextStorage::Stored(_) => None,
        }
    }

    /// Ordinary outward UTF-8 bytes without validation, decoding or a NUL scan.
    #[must_use]
    #[inline]
    pub fn outward_bytes(&self) -> Option<&[u8]> {
        match &self.0 {
            TextStorage::Scalar(text) => Some(text.as_bytes()),
            TextStorage::Stored(_) => None,
        }
    }

    /// Full stored words, including the zero and its suffix, if present.
    #[must_use]
    pub fn stored_words(&self) -> Option<&[u32]> {
        if let TextStorage::Stored(buffer) = &self.0 {
            Some(buffer.get(1..).unwrap_or_default())
        } else {
            None
        }
    }

    /// Code points before the first zero, in either representation.
    #[must_use]
    pub fn outward_words(&self) -> OutwardWords<'_> {
        OutwardWords(match &self.0 {
            TextStorage::Stored(buffer) => OutwardSource::Stored(stored_outward(buffer).iter()),
            TextStorage::Scalar(text) => OutwardSource::Scalar(text.chars()),
        })
    }

    /// Returns the scalar text's character count. For stored words, returns
    /// the nonzero declared count, or the number of stored words when the
    /// declared count is zero.
    #[must_use]
    pub fn phrase_length(&self) -> usize {
        match &self.0 {
            TextStorage::Scalar(text) => text.chars().count(),
            TextStorage::Stored(buffer) => match buffer.first().copied().unwrap_or(0) {
                0 => buffer.len().saturating_sub(1),
                length => usize::try_from(length).unwrap_or(usize::MAX),
            },
        }
    }

    /// The text after count characters; stored words retain their declared
    /// remaining length, even when a zero cut the outward prefix earlier.
    #[must_use]
    pub fn skip_chars(&self, count: usize) -> Self {
        match self.as_str() {
            Some(text) => Self::from(text.chars().skip(count).collect::<CompactString>()),
            None => Self::stored(
                self.outward_words().skip(count).collect(),
                self.phrase_length().saturating_sub(count),
            ),
        }
    }

    /// Scalar count, or None for stored words.
    #[must_use]
    pub fn char_count(&self) -> Option<usize> {
        matches!(self.0, TextStorage::Scalar(_)).then(|| self.phrase_length())
    }

    /// Whether the outward text is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.outward_bytes()
            .map_or_else(|| self.outward_words().next().is_none(), <[u8]>::is_empty)
    }
}

// The per-phrase conversion boundary is convert_to_utf8, lookup/lookup.cpp:39–69.
/// Concatenates the outward text of a sequence of phrases. Any component in stored-word
/// form switches the result to stored words, even when that component contains only
/// Unicode scalar values. Each component ends at its own first zero. When the result uses
/// stored words, its combined declared length is capped at `u32::MAX`.
#[derive(Default)]
pub struct PhraseTextConcat {
    scalar: CompactString,
    words: Option<Vec<u32>>,
    length: usize,
}

impl PhraseTextConcat {
    /// An empty concatenation.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one component without scanning ordinary text for NUL.
    pub fn push(&mut self, part: &PhraseText) {
        self.length = self.length.saturating_add(part.phrase_length());
        match &part.0 {
            TextStorage::Stored(_) => {
                self.switch_to_words();
                if let Some(words) = self.words.as_mut() {
                    words.extend(part.outward_words());
                }
            }
            TextStorage::Scalar(_) => {
                if let Some(words) = &mut self.words {
                    words.extend(part.outward_words());
                } else if let Some(text) = part.as_str() {
                    self.scalar.push_str(text);
                }
            }
        }
    }

    fn switch_to_words(&mut self) {
        if self.words.is_none() {
            let words = self.scalar.chars().map(u32::from).collect();
            self.scalar = CompactString::new("");
            self.words = Some(words);
        }
    }

    /// Finishes with ordinary text, whose length is its character count, or with stored
    /// words and their combined declared count.
    #[must_use]
    pub fn finish(self) -> PhraseText {
        match self.words {
            Some(words) => PhraseText::from_stored_parts(words, self.length),
            None => PhraseText(TextStorage::Scalar(self.scalar)),
        }
    }
}

/// Fused iterator over outward words, excluding the zero and suffix.
#[derive(Clone, Debug)]
pub struct OutwardWords<'a>(OutwardSource<'a>);

#[derive(Clone, Debug)]
enum OutwardSource<'a> {
    Scalar(core::str::Chars<'a>),
    Stored(core::slice::Iter<'a, u32>),
}

impl Iterator for OutwardWords<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        match &mut self.0 {
            OutwardSource::Scalar(chars) => chars.next().map(u32::from),
            OutwardSource::Stored(words) => words.next().copied(),
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            OutwardSource::Scalar(chars) => chars.size_hint(),
            OutwardSource::Stored(words) => words.size_hint(),
        }
    }
}
impl core::iter::FusedIterator for OutwardWords<'_> {}

impl Default for PhraseText {
    fn default() -> Self {
        Self(TextStorage::Scalar(CompactString::new("")))
    }
}
impl From<&str> for PhraseText {
    fn from(text: &str) -> Self {
        Self::from(CompactString::from(text))
    }
}
impl From<&String> for PhraseText {
    fn from(text: &String) -> Self {
        Self::from(text.as_str())
    }
}
impl From<String> for PhraseText {
    fn from(text: String) -> Self {
        Self::from(CompactString::from(text))
    }
}
impl From<CompactString> for PhraseText {
    fn from(text: CompactString) -> Self {
        if text.as_bytes().contains(&0) {
            let words: Vec<u32> = text.chars().map(u32::from).collect();
            let length = words.len();
            Self::stored(words, length)
        } else {
            Self(TextStorage::Scalar(text))
        }
    }
}
impl core::fmt::Display for PhraseText {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str().unwrap_or("<stored phrase text>"))
    }
}
impl PartialEq for PhraseText {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        if let (Some(left), Some(right)) = (self.as_str(), other.as_str()) {
            return left == right;
        }
        self.eq_stored(other)
    }
}
impl PhraseText {
    // Keep the uncommon decoding path out of the ordinary byte comparison.
    #[cold]
    fn eq_stored(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (TextStorage::Stored(left), TextStorage::Stored(right)) => {
                stored_outward(left) == stored_outward(right)
            }
            (TextStorage::Scalar(_), TextStorage::Scalar(_) | TextStorage::Stored(_))
            | (TextStorage::Stored(_), TextStorage::Scalar(_)) => {
                self.outward_words().eq(other.outward_words())
            }
        }
    }
}
impl Eq for PhraseText {}
impl PartialEq<str> for PhraseText {
    fn eq(&self, other: &str) -> bool {
        let other = scalar_outward(other);
        match self.outward_bytes() {
            Some(bytes) => bytes == other.as_bytes(),
            None => self.outward_words().eq(other.chars().map(u32::from)),
        }
    }
}
impl PartialEq<&str> for PhraseText {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}
impl PartialOrd for PhraseText {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for PhraseText {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        if let (Some(left), Some(right)) = (self.as_str(), other.as_str()) {
            return left.cmp(right);
        }
        match (&self.0, &other.0) {
            (TextStorage::Stored(left), TextStorage::Stored(right)) => {
                stored_outward(left).cmp(stored_outward(right))
            }
            (TextStorage::Scalar(_), TextStorage::Scalar(_) | TextStorage::Stored(_))
            | (TextStorage::Stored(_), TextStorage::Scalar(_)) => {
                self.outward_words().cmp(other.outward_words())
            }
        }
    }
}

impl Hash for PhraseText {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match &self.0 {
            TextStorage::Scalar(text) => text.as_str().hash(state),
            TextStorage::Stored(buffer) => {
                let words = stored_outward(buffer);
                match words
                    .iter()
                    .copied()
                    .map(char::from_u32)
                    .collect::<Option<CompactString>>()
                {
                    Some(text) => text.as_str().hash(state),
                    None => {
                        1_u8.hash(state);
                        words.hash(state);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
