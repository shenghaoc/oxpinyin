//! The direct parsers `ChewingLargeTable2::load_text` and
//! `FacadePhraseIndex::load_text` use for a table's spellings
//! (`src/storage/chewing_large_table2.cpp:129-141`,
//! `src/storage/phrase_index.cpp:561-575` at libpinyin 074a2219), and the
//! packed form of their keys.

use std::ffi::CStr;

use oxpinyin_core::ChewingKey;

/// `PinyinDirectParser2::parse` with `USE_TONE`
/// (`src/storage/pinyin_parser2.cpp:623-715`): the keys it appended before
/// the first failing syllable. Each syllable resolves through
/// `pinyin_index` (`:618-621,655`), not the wider `content_table`.
#[must_use]
pub(crate) fn parse(pinyin: &CStr) -> Vec<ChewingKey> {
    oxpinyin_core::tool::parse_direct(
        pinyin.to_bytes(),
        oxpinyin_core::OptionBits::USE_TONE,
        false,
    )
    .keys
    .into_iter()
    .map(|key| key.key)
    .collect()
}

/// `ZhuyinDirectParser2::parse` with `USE_TONE | FORCE_TONE`
/// (`utils/storage/gen_binary_files.cpp:136-141`): the same segment walk,
/// the tone defaulting to 1.
#[must_use]
pub(crate) fn parse_zhuyin(pinyin: &CStr) -> Vec<ChewingKey> {
    let Ok(text) = std::str::from_utf8(pinyin.to_bytes()) else {
        return Vec::new();
    };
    oxpinyin_core::parse_zhuyin_direct(
        text,
        oxpinyin_core::OptionBits::USE_TONE | oxpinyin_core::OptionBits::FORCE_TONE,
    )
    .into_iter()
    .filter_map(|(key, tone)| ChewingKey::from_pinyin(key.text()).map(|key| key.with_tone(tone)))
    .collect()
}

/// The packed `ChewingKey` bytes of one pronunciation, as a phrase item
/// stores them (`sizeof(ChewingKey) == 2`).
#[must_use]
pub(crate) fn packed(keys: &[ChewingKey]) -> Vec<u16> {
    keys.iter().map(|key| key.to_packed()).collect()
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn spellings(text: &str) -> Vec<String> {
        parse(&std::ffi::CString::new(text).unwrap())
            .iter()
            .map(|key| key.pinyin_string())
            .collect()
    }

    #[test]
    fn apostrophes_and_spaces_both_separate_syllables() {
        assert_eq!(spellings("ni'hao"), ["ni", "hao"]);
        assert_eq!(spellings("ni hao"), ["ni", "hao"]);
        assert_eq!(spellings("ni''hao"), ["ni", "hao"]);
        assert_eq!(spellings("ni'"), ["ni"]);
    }

    #[test]
    fn tone_digits_and_refusals() {
        assert_eq!(spellings("ni3'hao3"), ["ni3", "hao3"]);
        // No separator: one syllable, whose text keeps the first tone digit
        // and does not resolve.
        assert_eq!(spellings("ni3hao3"), Vec::<String>::new());
        // A trailing separator is skipped; an empty leading syllable stops
        // the parse.
        assert_eq!(spellings("'ni"), Vec::<String>::new());
        // An incomplete initial is refused without PINYIN_INCOMPLETE.
        assert_eq!(spellings("n"), Vec::<String>::new());
        // A correction spelling needs its option bit.
        assert_eq!(spellings("bagn"), Vec::<String>::new());
        // A content-table spelling with no pinyin-index row is refused.
        assert_eq!(spellings("ni'den4"), ["ni"]);
    }
}
