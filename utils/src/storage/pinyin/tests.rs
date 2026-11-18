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
