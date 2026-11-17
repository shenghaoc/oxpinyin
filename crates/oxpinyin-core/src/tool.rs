//! Parser outputs used by the pin's nonshipping `test_parser2` program.

use core::ops::Range;

use crate::{ChewingKey, FullPinyinParser, SyllableKey};

/// A key and its `ChewingKeyRest`, retained together so their counts agree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Key {
    /// Parsed key, including tone.
    pub key: ChewingKey,
    /// Byte offsets before the pin's `guint16` outward conversion.
    pub span: Range<usize>,
}

impl Key {
    /// The existing parsers' canonical key and tone, with their raw span.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "every_key_decodes_from_its_chewing_fields verifies the shared inventory"
    )]
    pub fn from_syllable(key: SyllableKey, tone: u8, span: Range<usize>) -> Self {
        Self {
            // Every SyllableKey names a row in the same chewing inventory.
            key: ChewingKey::from_pinyin(key.text())
                .expect("SyllableKey names a chewing content row")
                .with_tone(tone),
            span,
        }
    }
}

/// The pin's returned length and parallel key/key-rest arrays.
#[derive(Default, Debug, Eq, PartialEq)]
pub struct Parse {
    /// `parse`'s consumed byte count, including skipped separators.
    pub consumed: usize,
    /// Keys and raw spans, in parser order.
    pub keys: Vec<Key>,
}

/// Direct-parser outputs from the existing single-key parsers and shared
/// segment walk (`pinyin_parser2.cpp:623-715`, `zhuyin_parser2.cpp:858-960`).
#[must_use]
pub fn parse_direct(input: &[u8], options: crate::OptionBits, zhuyin: bool) -> Parse {
    let mut keys = Vec::new();
    let consumed = crate::direct::parse(
        input,
        |span| {
            let segment = &input[span];
            if zhuyin {
                let (key, tone) = crate::scheme::parse_zhuyin_direct_key(
                    core::str::from_utf8(segment).ok()?,
                    options,
                )?;
                ChewingKey::from_pinyin(key.text()).map(|key| key.with_tone(tone))
            } else {
                FullPinyinParser.parse_one_key(options, segment)
            }
        },
        |key, span| keys.push(Key { key, span }),
    );
    Parse { consumed, keys }
}

#[cfg(test)]
mod tests;
