//! Phonetic keys and parsers for full pinyin, double pinyin, and Zhuyin.
//!
//! [`ChewingKey`] holds the shared packed key fields and display spellings.
//! [`FullPinyinParser`] parses one full-pinyin key. The scheme parsers return
//! keys with byte spans and the consumed input prefix.
//! [`FullPinyinScheme`] selects the spelling index; [`FullPinyinIndexParse`]
//! describes the alternate-index parse and its selection rules.

pub use crate::chewing_key::{ChewingKey, KeyText};
pub(crate) use crate::full_pinyin_index::FullPinyinIndex;
pub use crate::full_pinyin_index::FullPinyinIndexParse;
pub(crate) use crate::scheme::{DoublePinyinKey, DoublePinyinParse, ZhuyinKey};
pub use crate::scheme::{
    DoublePinyinParser, DoublePinyinScheme, FullPinyinScheme, ZhuyinParse, ZhuyinParser,
    ZhuyinScheme, parse_zhuyin_direct,
};

use crate::OptionBits;
use crate::chewing_key::CHEWING_ZERO_TONE;
use crate::vocab::SyllableKey;

/// Whether a key is a complete syllable or an incomplete initial.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Completeness {
    /// A complete syllable or a complete matrix alternative.
    Complete,
    /// An initial without a final. Parsers can exclude it from a result.
    Partial,
}

/// Stateless parser for one full-pinyin key.
#[derive(Clone, Copy, Debug, Default)]
pub struct FullPinyinParser;

impl FullPinyinParser {
    // FullPinyinParser2::parse_one_key, pinyin_parser2.cpp:164–214. The pin asserts
    // an apostrophe-free input at line 170 and reads before the buffer for empty
    // input under USE_TONE at line 178 (class (b)). The Rust parser guards both.
    /// Parses one whole full-pinyin spelling into a [`ChewingKey`].
    ///
    /// Pass a nonempty spelling without apostrophes. Under [`OptionBits::USE_TONE`],
    /// a final ASCII digit from 1 to 5 supplies the tone. With that option set,
    /// [`OptionBits::FORCE_TONE`] requires the digit. Otherwise a spelling
    /// without a tone produces a key with tone zero.
    ///
    /// The spelling must match in full, or be a correction or ambiguity alias whose
    /// option bit is set. Initials alone require [`OptionBits::PINYIN_INCOMPLETE`].
    /// An unmatched spelling returns [`None`].
    #[must_use]
    pub fn parse_one_key(options: OptionBits, input: &str) -> Option<ChewingKey> {
        if input.contains('\'') {
            return None;
        }
        let (text, tone) = split_tone(options, input)?;
        let syllable = SyllableKey::from_option_text(text, options)?;
        // `check_pinyin_options` (`pinyin_parser2.cpp:38-58` at 074a2219):
        // an index row flagged `PINYIN_INCOMPLETE` — an initial-only
        // spelling — answers `false` unless the option word carries the
        // bit (issue #547: `b`, `zh`, `m` under 0x0 or 0x20).
        if syllable.completeness() == crate::parser::Completeness::Partial
            && !options.contains(OptionBits::PINYIN_INCOMPLETE)
        {
            return None;
        }
        let mut key = ChewingKey::from_pinyin(syllable.text())?;
        key.tone = tone;
        Some(key)
    }
}

// FullPinyinParser2::parse_one_key, pinyin_parser2.cpp:176–190 at 074a2219.
/// The tone half of a one-key parse: under [`OptionBits::USE_TONE`] a final
/// ASCII digit from 1 to 5 is the tone, and [`OptionBits::FORCE_TONE`] then
/// requires it. Returns the spelling without the digit, or [`None`] when a
/// required tone is absent.
pub(crate) fn split_tone(options: OptionBits, input: &str) -> Option<(&str, u8)> {
    if !options.contains(OptionBits::USE_TONE) {
        return Some((input, CHEWING_ZERO_TONE));
    }
    match input.bytes().last() {
        // An ASCII byte ends a character, so the slice stays on a boundary.
        Some(digit @ b'1'..=b'5') => Some((&input[..input.len() - 1], digit - b'0')),
        _ if options.contains(OptionBits::FORCE_TONE) => None,
        _ => Some((input, CHEWING_ZERO_TONE)),
    }
}

#[cfg(test)]
mod tests;
