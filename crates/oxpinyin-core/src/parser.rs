//! Full-pinyin single-key parsing.

use crate::options::{FORCE_TONE, PINYIN_INCOMPLETE, USE_TONE};
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
    pub fn parse_one_key(&self, options: u32, input: &[u8]) -> Option<ChewingKey> {
        if input.contains(&b'\'') {
            return None;
        }
        let bits = OptionBits::from_bits(options);
        let mut tone = CHEWING_ZERO_TONE;
        let mut core = input;
        if bits.contains(USE_TONE) {
            if let Some((&last, rest)) = input.split_last()
                && (b'1'..=b'5').contains(&last)
            {
                tone = last - b'0';
                core = rest;
            }
            if bits.contains(FORCE_TONE) && tone == CHEWING_ZERO_TONE {
                return None;
            }
        }
        let text = core::str::from_utf8(core).ok()?;
        let syllable = SyllableKey::from_option_text(text, bits)?;
        // `check_pinyin_options` (`pinyin_parser2.cpp:38-58` at 074a2219):
        // an index row flagged `PINYIN_INCOMPLETE` — an initial-only
        // spelling — answers `false` unless the option word carries the
        // bit (issue #547: `b`, `zh`, `m` under 0x0 or 0x20).
        if syllable.completeness() == crate::Completeness::Partial
            && !bits.contains(PINYIN_INCOMPLETE)
        {
            return None;
        }
        let mut key = ChewingKey::from_pinyin(syllable.text())?;
        key.tone = tone;
        Some(key)
    }
}

#[cfg(test)]
mod tests {
    use super::FullPinyinParser;
    use crate::options::{FORCE_TONE, PINYIN_CORRECT_GN_NG, PINYIN_INCOMPLETE, USE_TONE};

    /// `FullPinyinParser2::parse_one_key`: the whole stripped input is
    /// one spelling probe — multi-syllable and apostrophe inputs are
    /// refused, a trailing `1..=5` is the tone under `USE_TONE`, and
    /// `FORCE_TONE` (nested inside `USE_TONE`) rejects toneless input.
    #[test]
    fn full_pinyin_one_key_law() {
        let parser = FullPinyinParser;

        let key = parser.parse_one_key(0, b"ni").expect("ni parses");
        assert_eq!(key.pinyin_string(), "ni");

        // The probe covers the whole input: no forward maximum match.
        assert_eq!(parser.parse_one_key(0, b"nihao"), None);
        // Apostrophes are refused (the pin asserts on them).
        assert_eq!(parser.parse_one_key(0, b"ni'hao"), None);
        assert_eq!(parser.parse_one_key(0, b""), None);

        // Toneless without USE_TONE; tone carried with it.
        let toned = parser.parse_one_key(USE_TONE, b"zai4").expect("zai4");
        assert_eq!(toned.pinyin_string(), "zai4");
        assert_eq!(parser.parse_one_key(0, b"zai4"), None);

        // FORCE_TONE is dead without USE_TONE, rejects the toneless
        // `zai` with it, and accepts the toned form.
        assert!(parser.parse_one_key(FORCE_TONE, b"zai").is_some());
        assert_eq!(parser.parse_one_key(USE_TONE | FORCE_TONE, b"zai"), None);
        assert!(
            parser
                .parse_one_key(USE_TONE | FORCE_TONE, b"zai4")
                .is_some()
        );
        // `6` is not a tone: the digit stays in the core and fails it.
        assert_eq!(parser.parse_one_key(USE_TONE, b"zai6"), None);

        // Initial-only keys are index rows flagged PINYIN_INCOMPLETE:
        // `check_pinyin_options` refuses them unless the option word
        // carries the bit (`pinyin_parser2.cpp:42-46`), so `b` parses
        // under PINYIN_INCOMPLETE and under nothing else (#547).
        assert_eq!(parser.parse_one_key(0, b"b"), None);
        assert_eq!(parser.parse_one_key(USE_TONE, b"b1"), None);
        let b = parser
            .parse_one_key(PINYIN_INCOMPLETE, b"b")
            .expect("initial-only under PINYIN_INCOMPLETE");
        assert_eq!(b.pinyin_string(), "b");
        assert_eq!(b.yunmu_string(), "");
        assert_eq!(
            parser
                .parse_one_key(PINYIN_INCOMPLETE, b"zh")
                .map(|k| k.to_packed()),
            Some(0x0017)
        );

        // Correction aliases resolve through the option gate.
        let ang = parser
            .parse_one_key(PINYIN_CORRECT_GN_NG, b"agn")
            .expect("agn corrects");
        assert_eq!(ang.pinyin_string(), "ang");
        assert_eq!(parser.parse_one_key(0, b"agn"), None);
    }
}
