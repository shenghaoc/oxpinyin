//! Phonetic keys and parsers for full pinyin, double pinyin, and Zhuyin.
//!
//! [`ChewingKey`] holds the shared packed key fields and display spellings.
//! [`FullPinyinParser`] parses one full-pinyin key. The scheme parsers return
//! keys with byte spans and the consumed input prefix.
//! [`FullPinyinScheme`] selects the spelling index; [`FullPinyinIndexParse`]
//! describes the alternate-index parse and its selection rules.

pub use crate::chewing_key::{ChewingKey, KeyText};
pub use crate::full_pinyin_index::{FullPinyinIndex, FullPinyinIndexKey, FullPinyinIndexParse};
pub use crate::scheme::{
    DoublePinyinKey, DoublePinyinParse, DoublePinyinParser, DoublePinyinScheme, FullPinyinScheme,
    ZhuyinKey, ZhuyinParse, ZhuyinParser, ZhuyinScheme, parse_zhuyin_direct,
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
    /// The spelling must match in full. [`SyllableKey::from_option_text`] admits
    /// option-enabled aliases. Initials alone require [`OptionBits::PINYIN_INCOMPLETE`].
    /// An unmatched spelling returns [`None`].
    #[must_use]
    pub fn parse_one_key(&self, options: OptionBits, input: &[u8]) -> Option<ChewingKey> {
        if input.contains(&b'\'') {
            return None;
        }
        let mut tone = CHEWING_ZERO_TONE;
        let mut core = input;
        if options.contains(OptionBits::USE_TONE) {
            if let Some((&last, rest)) = input.split_last()
                && (b'1'..=b'5').contains(&last)
            {
                tone = last - b'0';
                core = rest;
            }
            if options.contains(OptionBits::FORCE_TONE) && tone == CHEWING_ZERO_TONE {
                return None;
            }
        }
        let text = core::str::from_utf8(core).ok()?;
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

#[cfg(test)]
mod tests;
