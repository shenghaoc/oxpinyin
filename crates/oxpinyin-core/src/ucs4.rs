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
    /// Reads stored words, preserving non-scalars and zero suffixes.
    #[must_use]
    pub fn from_words(words: impl IntoIterator<Item = u32>) -> Option<Self> {
        Self::from_stored_words(words.into_iter().map(Some))
    }

    /// Reads every declared word, rejecting unreadable words and values at
    /// or above 0x80000000 before the first zero. This preserves the pin's
    /// per-item cut (`src/lookup/lookup.cpp:39-68` at 074a2219).
    #[must_use]
    pub fn from_stored_words(words: impl IntoIterator<Item = Option<u32>>) -> Option<Self> {
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

    /// Builds outward stored words with the declared count used for ranking.
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
        match &self.0 {
            TextStorage::Stored(buffer) => Some(buffer.get(1..).unwrap_or_default()),
            _ => None,
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

    /// The stored declared count, or the ordinary text's character count.
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
        match self.0 {
            TextStorage::Stored(_) => None,
            _ => Some(self.phrase_length()),
        }
    }

    /// Whether the outward text is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.outward_bytes()
            .map_or_else(|| self.outward_words().next().is_none(), <[u8]>::is_empty)
    }

    /// Whether this text occupies only its inline buffer.
    #[must_use]
    pub fn is_inline(&self) -> bool {
        matches!(&self.0, TextStorage::Scalar(text) if !text.is_heap_allocated())
    }
}

/// Concatenates ordinary texts in a CompactString, switching to stored words
/// only when a component contains a zero or non-scalar word.
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
            _ => {
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

    /// Finishes with ordinary text or stored words and their declared count.
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
            _ => self.outward_words().eq(other.outward_words()),
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
            _ => self.outward_words().cmp(other.outward_words()),
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
mod phrase_text_tests {
    use super::{PhraseText, PhraseTextConcat};

    #[test]
    fn constructors_keep_full_lengths_and_hash_the_outward_text() {
        use core::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        for source in ["", "abc", "甲乙", "a\0甲", "甲乙丙丁戊己庚辛壬癸"] {
            let text = PhraseText::from(source);
            assert_eq!(text.phrase_length(), source.chars().count());
            let scalar = !source.contains('\0');
            assert_eq!(text.char_count(), scalar.then(|| source.chars().count()));
            assert_eq!(text.as_str(), scalar.then_some(source));
            assert_eq!(
                matches!(text.0, super::TextStorage::Stored(_)),
                source.contains('\0')
            );
            let prefix = source.split('\0').next().unwrap();
            assert_eq!(text.outward_bytes(), scalar.then_some(prefix.as_bytes()));
            let mut actual = DefaultHasher::new();
            let mut expected = DefaultHasher::new();
            text.hash(&mut actual);
            prefix.hash(&mut expected);
            assert_eq!(actual.finish(), expected.finish());
            for skip in 0..=source.chars().count() + 1 {
                let tail = text.skip_chars(skip);
                assert_eq!(
                    tail.phrase_length(),
                    source.chars().count().saturating_sub(skip)
                );
            }
        }
        let mut sentence = PhraseTextConcat::new();
        sentence.push(&PhraseText::from("甲乙"));
        sentence.push(&PhraseText::from("abc"));
        let text = sentence.finish();
        assert_eq!(text.phrase_length(), 5);
        assert_eq!(text.outward_bytes(), Some("甲乙abc".as_bytes()));
    }

    #[test]
    fn inline_and_heap_boundaries_preserve_scalar_and_stored_views() {
        for unit in ["a", "é", "甲", "🦀"] {
            for count in [
                0, 1, 7, 8, 20, 21, 22, 23, 24, 25, 63, 64, 65, 127, 128, 129,
            ] {
                let source = unit.repeat(count);
                let text = PhraseText::from(source.as_str());
                let decoded = PhraseText::from_words(source.chars().map(u32::from)).unwrap();
                assert_eq!(text.as_str(), Some(source.as_str()));
                assert_eq!(decoded.as_str(), Some(source.as_str()));
                assert_eq!(text.phrase_length(), count);
                assert_eq!(decoded.phrase_length(), count);
                assert_eq!(
                    text.is_inline(),
                    source.len() <= core::mem::size_of::<compact_str::CompactString>()
                );
                assert_eq!(text, decoded);
                assert_eq!(key(&text), key(&decoded));
                let words = source.chars().map(u32::from).chain([0, 0xd800]);
                let stored = PhraseText::from_words(words).unwrap();
                assert_eq!(stored, text);
                assert_eq!(stored.phrase_length(), count + 2);
                assert_eq!(key(&stored), key(&text));
            }
        }
    }
    use core::cmp::Ordering;
    use core::hash::BuildHasher;
    use std::collections::HashSet;

    fn key(text: &PhraseText) -> u64 {
        std::hash::BuildHasherDefault::<std::hash::DefaultHasher>::default().hash_one(text)
    }

    fn stored(words: &[u32]) -> PhraseText {
        PhraseText::from_words(words.iter().copied()).expect("stored words")
    }

    /// Both forms of the same outward texts (甲 U+7532, 乙 U+4E59, 丙
    /// U+4E19), with differing headers and suffixes, beside non-scalar and
    /// empty ones.
    fn mixed_texts() -> Vec<PhraseText> {
        vec![
            PhraseText::from(""),
            PhraseText::from("\0甲"),
            stored(&[0, 0x7532]),
            PhraseText::from("甲"),
            PhraseText::from("甲\0乙"),
            stored(&[0x7532, 0, 0x4E19]),
            PhraseText::from_stored_parts(vec![0x7532], 9),
            PhraseText::from("甲乙"),
            PhraseText::from_stored_parts(vec![0x7532, 0x4E59], 2),
            stored(&[0x7532, 0xD800]),
            stored(&[0x7532, 0xD800, 0, 0x4E59]),
            stored(&[0xD800]),
            PhraseText::from("乙"),
            PhraseText::from("\u{ffff}"),
            PhraseText::from("\u{10000}"),
            stored(&[0x11_0000]),
        ]
    }

    #[test]
    fn a_zero_word_ends_the_outward_text() {
        let text = PhraseText::from_words([0x7532, 0, 0x4E59]).expect("stored words");
        assert_eq!(text.outward_words().collect::<Vec<_>>(), [0x7532]);
        assert_eq!(text.stored_words(), Some([0x7532, 0, 0x4E59].as_slice()));
        assert_eq!(text.phrase_length(), 3);
        assert!(text.as_str().is_none());
    }

    #[test]
    fn non_scalars_stay_as_stored_words() {
        let surrogate = PhraseText::from_words([0x7532, 0xD800, 0x4E59]).expect("stored words");
        assert_eq!(
            surrogate.stored_words(),
            Some([0x7532, 0xD800, 0x4E59].as_slice())
        );
        assert!(surrogate.as_str().is_none());

        let above = PhraseText::from_words([0x11_0000]).expect("stored words");
        assert_eq!(above.stored_words(), Some([0x11_0000].as_slice()));
    }

    #[test]
    fn general_conversions_preserve_nul_text_as_stored_words() {
        for text in [
            PhraseText::from("甲\0乙"),
            PhraseText::from(String::from("甲\0乙")),
            PhraseText::from(compact_str::CompactString::from("甲\0乙")),
        ] {
            assert_eq!(text.as_str(), None);
            assert_eq!(text.stored_words(), Some([0x7532, 0, 0x4e59].as_slice()));
            assert_eq!(text.phrase_length(), 3);
            assert_eq!(text.outward_words().collect::<Vec<_>>(), [0x7532]);
        }
    }

    #[test]
    fn ordinary_stored_text_stays_inline() {
        let text = PhraseText::from_stored_words([Some(0x7532), Some(0x4E59)])
            .expect("ordinary stored words");
        assert!(text.is_inline());
        assert_eq!(text.as_str(), Some("甲乙"));
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn the_text_is_one_candidate_row_field() {
        assert_eq!(core::mem::size_of::<PhraseText>(), 24);
    }

    #[test]
    fn a_zero_keeps_the_stored_length() {
        let text = PhraseText::from_words([0x7532, 0, 0x4E59]).expect("stored words");
        assert_eq!(text.outward_words().collect::<Vec<_>>(), [0x7532]);
        assert_eq!(text.phrase_length(), 3);
    }

    #[test]
    fn out_of_range_words_answer_none_before_the_zero() {
        assert!(PhraseText::from_words([0x8000_0000]).is_none());
        let after = PhraseText::from_words([0x7532, 0, 0x8000_0000]).expect("zero ends the read");
        assert_eq!(
            after.stored_words(),
            Some([0x7532, 0, 0x8000_0000].as_slice())
        );
        assert_eq!(after.phrase_length(), 3);
    }

    #[test]
    fn a_non_scalar_word_does_not_end_the_range_check() {
        assert!(PhraseText::from_words([0xD800, 0x8000_0000]).is_none());

        let kept = PhraseText::from_words([0xD800, 0, 0x8000_0000]).expect("zero ends the read");
        assert_eq!(
            kept.stored_words(),
            Some([0xD800, 0, 0x8000_0000].as_slice())
        );
        assert_eq!(kept.phrase_length(), 3);
        assert_eq!(kept.outward_words().collect::<Vec<_>>(), [0xD800]);
    }

    #[test]
    fn skip_chars_slices_both_forms() {
        let scalar = PhraseText::from("甲乙");
        assert_eq!(scalar.skip_chars(1).as_str(), Some("乙"));

        let stored = PhraseText::from_words([0x7532, 0xD800, 0x4E59]).expect("stored words");
        let suffix = stored.skip_chars(1);
        assert_eq!(suffix.stored_words(), Some([0xD800, 0x4E59].as_slice()));
        assert_eq!(suffix.phrase_length(), 2);
    }

    #[test]
    fn ordinary_text_compares_before_its_first_nul() {
        let text = PhraseText::from("甲\0乙");
        assert_eq!(text.phrase_length(), 3);
        assert_eq!(text, PhraseText::from("甲"));
        assert_eq!(text, PhraseText::from("甲\0丙"));
        assert_ne!(text, PhraseText::from("甲乙"));
        assert_eq!(text.cmp(&PhraseText::from("甲乙")), Ordering::Less);
        assert_eq!(key(&text), key(&PhraseText::from("甲")));
        assert!(PhraseText::from("\0甲").is_empty());
    }

    #[test]
    fn equality_is_transitive_across_forms() {
        let ordinary_with_nul = PhraseText::from("甲\0乙");
        let ordinary = PhraseText::from("甲");
        let with_zero = stored(&[0x7532, 0, 0x4E19]);
        assert!(matches!(with_zero.0, super::TextStorage::Stored(_)));
        let texts = [&ordinary_with_nul, &ordinary, &with_zero];
        for left in texts {
            for right in texts {
                assert_eq!(left, right);
                assert_eq!(left.cmp(right), Ordering::Equal);
                assert_eq!(key(left), key(right));
            }
        }
        assert_eq!(texts.into_iter().collect::<HashSet<_>>().len(), 1);
    }

    #[test]
    fn mixed_forms_compare_and_hash_by_outward_words() {
        let texts = mixed_texts();
        for left in &texts {
            for right in &texts {
                let words = left.outward_words().cmp(right.outward_words());
                assert_eq!(left.cmp(right), words, "{left:?} against {right:?}");
                assert_eq!(left.partial_cmp(right), Some(words));
                assert_eq!(left == right, words.is_eq(), "{left:?} against {right:?}");
                if left == right {
                    assert_eq!(key(left), key(right), "{left:?} and {right:?} hash apart");
                }
            }
        }
    }

    #[test]
    fn mixed_form_order_is_transitive() {
        let texts = mixed_texts();
        for first in &texts {
            for second in &texts {
                for third in &texts {
                    if first <= second && second <= third {
                        assert!(first <= third, "{first:?} <= {second:?} <= {third:?}");
                    }
                    if first == second && second == third {
                        assert_eq!(first, third);
                    }
                }
            }
        }
    }

    #[test]
    fn the_length_header_and_the_suffix_are_not_compared() {
        let pairs = [
            (
                PhraseText::from_stored_parts(vec![0x7532], 1),
                PhraseText::from_stored_parts(vec![0x7532], 5),
            ),
            (
                stored(&[0x7532, 0, 0x4E59]),
                stored(&[0x7532, 0, 0x4E19, 0x4E59]),
            ),
            (
                PhraseText::from_stored_parts(vec![0xD800], 1),
                PhraseText::from_stored_parts(vec![0xD800], 7),
            ),
            (stored(&[0xD800, 0, 0x4E59]), stored(&[0xD800, 0])),
        ];
        for (left, right) in &pairs {
            assert!(
                left.phrase_length() != right.phrase_length()
                    || left.stored_words() != right.stored_words(),
                "{left:?} and {right:?} store the same words"
            );
            assert_eq!(left, right);
            assert_eq!(left.cmp(right), Ordering::Equal);
            assert_eq!(key(left), key(right));
        }
    }

    #[test]
    fn str_comparison_matches_the_converted_text() {
        let strs = ["", "甲", "甲\0丙", "甲乙", "乙", "\0"];
        for text in mixed_texts() {
            for other in strs {
                let converted = PhraseText::from(other);
                assert_eq!(
                    text == *other,
                    text == converted,
                    "{text:?} against {other:?}"
                );
                assert_eq!(
                    text == other,
                    text == converted,
                    "{text:?} against {other:?}"
                );
            }
        }
        assert_eq!(stored(&[0x7532, 0, 0x4E59]), "甲");
        assert_ne!(stored(&[0x7532, 0xD800]), "甲");
    }

    #[test]
    fn outward_words_stay_exhausted_after_the_zero() {
        for text in [stored(&[0x7532, 0, 0x4E59]), PhraseText::from("甲\0乙")] {
            let mut words = text.outward_words();
            assert_eq!(words.next(), Some(0x7532));
            assert_eq!(words.next(), None);
            assert_eq!(words.next(), None, "{text:?} resumed after its zero");
        }
    }

    #[test]
    fn a_sentence_cuts_each_ordinary_part_at_its_own_nul() {
        let mut sentence = PhraseTextConcat::new();
        sentence.push(&PhraseText::from("甲\0乙"));
        sentence.push(&PhraseText::from("丙"));
        let sentence = sentence.finish();
        assert_eq!(
            sentence.outward_words().collect::<Vec<_>>(),
            [0x7532, 0x4E19]
        );
        assert_eq!(sentence.phrase_length(), 4);
        assert_eq!(sentence, "甲丙");
    }
}
