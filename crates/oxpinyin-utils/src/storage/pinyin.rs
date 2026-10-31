//! `PinyinDirectParser2::parse` with `USE_TONE` (`src/storage/pinyin_parser2.cpp:623-715`
//! at libpinyin 074a2219), the parser `ChewingLargeTable2::load_text` and
//! `FacadePhraseIndex::load_text` use for a `pinyin` table.
//!
//! A spelling is split at `'` and space; consecutive separators are
//! skipped. For one syllable, a trailing `1`..`5` is the tone digit and the
//! rest resolves through the pinyin index with `USE_TONE` alone — an
//! incomplete spelling (`PINYIN_INCOMPLETE`) and a correction spelling
//! (`PINYIN_CORRECT_*`, `PINYIN_AMB_*`) need option bits the utilities do
//! not pass, so those are refused. A syllable that does not resolve stops
//! the parse; the keys parsed so far stand, exactly as the pin's
//! `parsed_len` return leaves them.

use std::ffi::CStr;

use oxpinyin_core::ChewingKey;

/// One syllable of the direct parser.
fn parse_one_key(syllable: &[u8]) -> Option<ChewingKey> {
    // An empty syllable is the pin's `input[parsed_len - 1]` read before
    // the allocated string (class (b)); oxpinyin refuses it, which leaves
    // the key count short exactly as the failing parse does.
    let mut parsed_len = syllable.len();
    if parsed_len == 0 {
        return None;
    }
    let mut tone = 0_u8;
    let last = syllable[parsed_len - 1];
    if last > b'0' && last <= b'5' {
        tone = last - b'0';
        parsed_len -= 1;
    }
    let text = std::str::from_utf8(&syllable[..parsed_len]).ok()?;
    let key = ChewingKey::from_pinyin(text)?;
    // `check_pinyin_options`: an initial-only row needs
    // `PINYIN_INCOMPLETE`, which the storage utilities never pass. The
    // content table stores those rows as initial + zero middle/final.
    if key.middle == 0 && key.final_ == 0 {
        return None;
    }
    Some(key.with_tone(tone))
}

/// `PinyinDirectParser2::parse`: the keys it appended before the first
/// failing syllable.
#[must_use]
pub(crate) fn parse(pinyin: &CStr) -> Vec<ChewingKey> {
    let bytes = pinyin.to_bytes();
    let mut keys = Vec::new();
    let mut cur = 0_usize;
    while cur < bytes.len() {
        let mut next = cur;
        while next < bytes.len() && bytes[next] != b' ' && bytes[next] != b'\'' {
            next += 1;
        }
        let Some(key) = parse_one_key(&bytes[cur..next]) else {
            return keys;
        };
        keys.push(key);
        let mut after = next;
        while after < bytes.len() && (bytes[after] == b' ' || bytes[after] == b'\'') {
            after += 1;
        }
        cur = after;
    }
    keys
}

/// `ZhuyinDirectParser2::parse` with `USE_TONE | FORCE_TONE`
/// (`utils/storage/gen_binary_files.cpp:136-141`): the same segment walk,
/// the tone defaulting to 1.
#[must_use]
pub(crate) fn parse_zhuyin(pinyin: &CStr) -> Vec<ChewingKey> {
    let Ok(text) = std::str::from_utf8(pinyin.to_bytes()) else {
        return Vec::new();
    };
    oxpinyin_core::parse_zhuyin_direct(text, oxpinyin_core::USE_TONE | oxpinyin_core::FORCE_TONE)
        .into_iter()
        .filter_map(|(key, tone)| {
            ChewingKey::from_pinyin(key.text()).map(|key| key.with_tone(tone))
        })
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
    }
}
