//! The C-facing encoding of a phrase text.
//!
//! The pin stores phrase text as UCS-4 words and hands C the bytes of
//! GLib's `g_ucs4_to_utf8`: the first zero word ends the string, and a
//! non-scalar word takes the historical three- to six-byte form. This
//! module is the single place that rule leaves the Rust text model; core
//! and engine keep the raw outward words.

use std::borrow::Cow;

use oxpinyin_core::PhraseText;

/// The bytes the pin hands to C for `text`.
///
/// A scalar text borrows its NUL-free bytes; a stored-word
/// text encodes its outward words with `g_unichar_to_utf8`'s historical
/// widths. The result has no interior zero.
#[must_use]
#[inline]
pub fn phrase_text_bytes(text: &PhraseText) -> Cow<'_, [u8]> {
    match text.outward_bytes() {
        Some(bytes) => Cow::Borrowed(bytes),
        None => Cow::Owned(encode_words(text.stored_words().unwrap_or(&[]))),
    }
}

/// Whether `text` stores exactly `phrase`'s code points: the pin's check of
/// an existing item against an imported phrase, its stored length then a
/// `memcmp` over every stored word, a zero and what follows it included
/// (`pinyin.cpp:568-571`, `zhuyin.cpp:454-457` at 074a2219).
#[must_use]
pub fn stored_words_match(text: &PhraseText, phrase: &str) -> bool {
    match text.stored_words() {
        Some(words) => words.iter().copied().eq(phrase.chars().map(u32::from)),
        None => text.as_str() == Some(phrase),
    }
}

#[cold]
fn encode_words(words: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(words.len().saturating_mul(3));
    for &word in words {
        if word == 0 {
            break;
        }
        encode_word(word, &mut out);
    }
    out
}

/// `g_unichar_to_utf8`'s encoder for one word, which takes the historical
/// forms for values above Unicode.
fn encode_word(word: u32, out: &mut Vec<u8>) {
    let (first, len) = if word < 0x80 {
        (0x00, 1)
    } else if word < 0x800 {
        (0xc0, 2)
    } else if word < 0x1_0000 {
        (0xe0, 3)
    } else if word < 0x20_0000 {
        (0xf0, 4)
    } else if word < 0x400_0000 {
        (0xf8, 5)
    } else {
        (0xfc, 6)
    };
    let mut bytes = [0_u8; 6];
    let mut rest = word;
    for slot in bytes.iter_mut().take(len).skip(1).rev() {
        *slot = (rest & 0x3f) as u8 | 0x80;
        rest >>= 6;
    }
    bytes[0] = (rest as u8) | first;
    out.extend_from_slice(&bytes[..len]);
}

#[cfg(test)]
mod tests {
    use super::phrase_text_bytes;
    use oxpinyin_core::PhraseText;

    #[test]
    fn scalar_text_is_borrowed_and_nul_text_is_encoded() {
        let ordinary = PhraseText::from("甲乙");
        assert!(matches!(
            phrase_text_bytes(&ordinary),
            std::borrow::Cow::Borrowed(_)
        ));
        let text = PhraseText::from("甲\0乙");
        let bytes = phrase_text_bytes(&text);
        assert!(matches!(bytes, std::borrow::Cow::Owned(_)));
        assert_eq!(bytes.as_ref(), "甲".as_bytes());
    }

    #[test]
    fn stored_non_scalars_take_glibs_historical_encoding() {
        let surrogate = PhraseText::from_words([0x7532, 0xD800, 0x4E59]).expect("stored");
        assert_eq!(
            phrase_text_bytes(&surrogate).as_ref(),
            [0xE7, 0x94, 0xB2, 0xED, 0xA0, 0x80, 0xE4, 0xB9, 0x99]
        );

        let above = PhraseText::from_words([0x11_0000]).expect("stored");
        assert_eq!(phrase_text_bytes(&above).as_ref(), [0xF4, 0x90, 0x80, 0x80]);
    }

    #[test]
    fn stored_words_match_reads_every_stored_word() {
        use super::stored_words_match;
        assert!(stored_words_match(&PhraseText::from("甲乙"), "甲乙"));
        assert!(!stored_words_match(&PhraseText::from("甲\0乙"), "甲"));
        let with_zero = PhraseText::from_words([0x7532, 0, 0x4E59]).expect("stored");
        assert!(!stored_words_match(&with_zero, "甲"));
        let surrogate = PhraseText::from_words([0x7532, 0xD800]).expect("stored");
        assert!(!stored_words_match(&surrogate, "甲"));
        let ordinary = PhraseText::from_stored_parts(vec![0x7532, 0x4E59], 2);
        assert!(stored_words_match(&ordinary, "甲乙"));
    }

    #[test]
    fn stored_zero_words_are_cut() {
        let text = PhraseText::from_words([0x7532, 0, 0x4E59]).expect("stored");
        assert_eq!(phrase_text_bytes(&text).as_ref(), "甲".as_bytes());
    }
}
