//! The C-facing encoding of a phrase text.
//!
//! The pin stores phrase text as UCS-4 words and hands C the bytes of
//! GLib's `g_ucs4_to_utf8`: the first zero word ends the string, and a
//! non-scalar word takes the historical three- to six-byte form. This
//! module is the single place that rule leaves the Rust text model; core
//! and engine keep the raw outward words.

use std::borrow::Cow;

use crate::vocab::PhraseText;

/// Borrows the NUL-free bytes of scalar text. For stored text, encodes each
/// word before the first zero with `g_unichar_to_utf8`'s historical widths,
/// independently of the declared count.
#[must_use]
#[inline]
pub fn phrase_text_bytes(text: &PhraseText) -> Cow<'_, [u8]> {
    match text.outward_bytes() {
        Some(bytes) => Cow::Borrowed(bytes),
        None => Cow::Owned(encode_words(text.stored_words().unwrap_or(&[]))),
    }
}

/// The length of [`phrase_text_bytes`]`(text)`, without encoding it.
#[must_use]
#[inline]
pub fn phrase_text_len(text: &PhraseText) -> usize {
    match text.outward_bytes() {
        Some(bytes) => bytes.len(),
        None => text
            .stored_words()
            .unwrap_or(&[])
            .iter()
            .take_while(|&&word| word != 0)
            .map(|&word| encoded_word_len(word))
            .sum(),
    }
}

/// The length of `g_ucs4_to_utf8` over a phrase item's declared words
/// (074a2219 `pinyin.cpp:715-716`): the bytes [`for_each_stored_word_utf8`]
/// hands out, which are [`phrase_text_bytes`] of the text that
/// `PhraseText::from_stored_words` reads. `None` where that text is
/// refused and the pin's conversion answers NULL: a word the item's bytes
/// do not hold, or a word at or above `0x8000_0000` before the first zero.
#[must_use]
pub fn stored_words_utf8_len(words: impl IntoIterator<Item = Option<u32>>) -> Option<usize> {
    let mut len = 0_usize;
    let mut past_zero = false;
    for word in words {
        let word = word?;
        if past_zero {
            continue;
        }
        if word == 0 {
            past_zero = true;
        } else if word >= 0x8000_0000 {
            return None;
        } else {
            len = len.saturating_add(encoded_word_len(word));
        }
    }
    Some(len)
}

/// Hands the bytes [`stored_words_utf8_len`] counts to `emit`, one word at
/// a time and with no intermediate text: each word before the first zero
/// in `g_unichar_to_utf8`'s form. For words that length refuses, the bytes
/// up to the refused word. None of the bytes is NUL.
pub fn for_each_stored_word_utf8(
    words: impl IntoIterator<Item = Option<u32>>,
    mut emit: impl FnMut(&[u8]),
) {
    for word in words {
        let Some(word) = word.filter(|&word| word != 0 && word < 0x8000_0000) else {
            break;
        };
        let (bytes, len) = word_bytes(word);
        emit(&bytes[..len]);
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

/// `g_unichar_to_utf8`'s lead-byte marker and byte count for one word,
/// with the historical forms for values above Unicode.
const fn word_form(word: u32) -> (u8, usize) {
    if word < 0x80 {
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
    }
}

const fn encoded_word_len(word: u32) -> usize {
    word_form(word).1
}

/// `g_unichar_to_utf8`'s encoder for one word, which takes the historical
/// forms for values above Unicode.
fn encode_word(word: u32, out: &mut Vec<u8>) {
    let (bytes, len) = word_bytes(word);
    out.extend_from_slice(&bytes[..len]);
}

/// One word's `g_unichar_to_utf8` bytes and their count.
fn word_bytes(word: u32) -> ([u8; 6], usize) {
    let (first, len) = word_form(word);
    let mut bytes = [0_u8; 6];
    let mut rest = word;
    for slot in bytes.iter_mut().take(len).skip(1).rev() {
        *slot = (rest & 0x3f) as u8 | 0x80;
        rest >>= 6;
    }
    bytes[0] = (rest as u8) | first;
    (bytes, len)
}

#[cfg(test)]
mod tests;
