//! Full-pinyin single-key parsing.

use crate::{CHEWING_ZERO_TONE, ChewingKey, OptionBits, SyllableKey};

/// Whether a key is a complete syllable or an incomplete initial.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Completeness {
    /// A member of the frozen complete-syllable table.
    Complete,
    /// An initial-only key admitted under `PINYIN_INCOMPLETE`.
    Partial,
}

/// Stateless parser for one full-pinyin key.
#[derive(Clone, Copy, Debug, Default)]
pub struct FullPinyinParser;

impl FullPinyinParser {
    /// Parses one full-pinyin key from the whole of `input`, mirroring
    /// `FullPinyinParser2::parse_one_key`
    /// (`src/storage/pinyin_parser2.cpp:164-214`).
    ///
    /// `options` is the caller's option word. Apostrophes are refused
    /// (upstream asserts them away); under `USE_TONE` a trailing
    /// `1..=5` is the tone — stripped before the spelling probe and
    /// written onto the key on a match — and `FORCE_TONE`, nested
    /// inside `USE_TONE` exactly like the pin, rejects a toneless
    /// input. The spelling probe covers the whole stripped input, so a
    /// multi-syllable string such as `nihao` is refused, not matched at
    /// its first syllable; alias resolution through
    /// [`SyllableKey::from_option_text`] is the option gate the batch
    /// path's edges use.
    ///
    /// [`None`] is the upstream `false`, and also the apostrophe input, whose
    /// assert the C adapters turn into an abort. Empty input under
    /// `USE_TONE` is not an assert: the pin reads one byte before the
    /// buffer (class (b)) and answers `false`, as this does, and the C
    /// adapters return `false` for it too.
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
        if syllable.completeness() == crate::Completeness::Partial
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
