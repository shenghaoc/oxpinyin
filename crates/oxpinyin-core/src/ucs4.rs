//! Decoding C input bytes to the pin's raw UCS-4 words.
//!
//! The phrase-segment boundary accepts the historical UTF-8 encodings
//! GLib's converter accepts: canonical 1–6 byte sequences, including
//! surrogate code points, values above U+10FFFF, and the five- and
//! six-byte forms up to `0x7fff_ffff`. It rejects overlong, truncated and
//! otherwise malformed sequences. The result is raw `u32` words; this
//! module deliberately does not build `char` or `str` values, so a
//! non-scalar code point survives as the value the phrase table stores.

/// A byte sequence outside the converter's accepted set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ucs4Error;

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
pub fn decode(bytes: &[u8]) -> Result<Vec<u32>, Ucs4Error> {
    let mut words = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while let Some(&first) = bytes.get(position) {
        if first < 0x80 {
            words.push(u32::from(first));
            position += 1;
            continue;
        }
        let (width, minimum) = match first {
            0xc2..=0xdf => (2, 0x80),
            0xe0..=0xef => (3, 0x800),
            0xf0..=0xf7 => (4, 0x1_0000),
            0xf8..=0xfb => (5, 0x20_0000),
            0xfc..=0xfd => (6, 0x400_0000),
            _ => return Err(Ucs4Error),
        };
        let tail = bytes.get(position + 1..position + width).ok_or(Ucs4Error)?;
        let mut word = u32::from(match width {
            2 => first & 0x1f,
            3 => first & 0x0f,
            4 => first & 0x07,
            5 => first & 0x03,
            _ => first & 0x01,
        });
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
