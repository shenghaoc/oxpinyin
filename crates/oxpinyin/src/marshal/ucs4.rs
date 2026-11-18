//! Decoding C input bytes to the pin's raw UCS-4 words.
//!
//! The C entries that take text convert it with GLib's `g_utf8_to_ucs4`,
//! which accepts the historical UTF-8 encodings: canonical 1–6 byte
//! sequences, including surrogate code points, values above U+10FFFF, and
//! the five- and six-byte forms up to `0x7fff_ffff`. It rejects overlong,
//! truncated and otherwise malformed sequences (GLib 2.84.4
//! `gutf8.c:661-743`, `:938-993`). The result is raw `u32` words; this
//! module deliberately does not build `char` or `str` values, so a
//! non-scalar code point survives as the value the phrase table stores.

use std::ffi::CStr;

use crate::PhraseText;

/// A byte sequence outside the converter's accepted set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Ucs4Error;

impl core::fmt::Display for Ucs4Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("invalid historical UTF-8 sequence")
    }
}

impl std::error::Error for Ucs4Error {}

/// Decodes `bytes` to raw UCS-4 words.
///
/// # Errors
///
/// Returns [`Ucs4Error`] for a continuation byte in lead position, a
/// truncated sequence, an overlong form, or a `0xfe`/`0xff` byte.
pub(crate) fn decode(bytes: &[u8]) -> Result<Vec<u32>, Ucs4Error> {
    let mut words = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while let Some(&first) = bytes.get(position) {
        if first < 0x80 {
            words.push(u32::from(first));
            position += 1;
            continue;
        }
        // The sequence width, its least value and the lead byte's payload bits.
        let (width, minimum, mask) = match first {
            0xc2..=0xdf => (2, 0x80, 0x1f),
            0xe0..=0xef => (3, 0x800, 0x0f),
            0xf0..=0xf7 => (4, 0x1_0000, 0x07),
            0xf8..=0xfb => (5, 0x20_0000, 0x03),
            0xfc..=0xfd => (6, 0x400_0000, 0x01),
            _ => return Err(Ucs4Error),
        };
        let tail = bytes.get(position + 1..position + width).ok_or(Ucs4Error)?;
        let mut word = u32::from(first & mask);
        for &byte in tail {
            if byte & 0xc0 != 0x80 {
                return Err(Ucs4Error);
            }
            word = (word << 6) | u32::from(byte & 0x3f);
        }
        if word < minimum {
            return Err(Ucs4Error);
        }
        words.push(word);
        position += width;
    }
    Ok(words)
}

/// A caller's prefix as the pin's `_compute_prefixes` converts it
/// (`g_utf8_to_ucs4(prefix, -1, NULL, &len, NULL)`, `pinyin.cpp:1397-1398`,
/// `zhuyin.cpp:918` at 074a2219), or `None` when that adds no prefix: a
/// rejected sequence, an empty string, or NULL. The pin dereferences a
/// NULL `prefix` (`gutf8.c:950`); by ruling that is the same no-prefix
/// outcome (upstream divergences, "NULL prediction prefix"). Accepted
/// non-scalar words keep the stored form.
#[must_use]
pub fn prefix_text(prefix: Option<&CStr>) -> Option<PhraseText> {
    crate::marshal::input::utf8_to_ucs4(prefix).filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests;
