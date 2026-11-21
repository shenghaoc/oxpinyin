use super::phrase_text_bytes;
use crate::vocab::PhraseText;

/// Whether `text` stores exactly `phrase`'s code points: the pin's check of
/// an existing item against an imported phrase, its stored length then a
/// `memcmp` over every stored word, a zero and what follows it included
/// (`pinyin.cpp:568-571`, `zhuyin.cpp:454-457` at 074a2219).
#[must_use]
fn stored_words_match(text: &PhraseText, phrase: &str) -> bool {
    match text.stored_words() {
        Some(words) => words.iter().copied().eq(phrase.chars().map(u32::from)),
        None => text.as_str() == Some(phrase),
    }
}

#[test]
fn scalar_text_is_borrowed_and_nul_text_is_encoded() {
    let ordinary = PhraseText::from("甲乙");
    assert!(matches!(
        phrase_text_bytes(&ordinary),
        std::borrow::Cow::Borrowed(_)
    ));
    let text = PhraseText::from("甲\0乙");
    let bytes = phrase_text_bytes(&text);
    assert!(matches!(bytes, std::borrow::Cow::Owned(_)));
    assert_eq!(bytes.as_ref(), "甲".as_bytes());
}

#[test]
fn stored_non_scalars_take_glibs_historical_encoding() {
    let surrogate = PhraseText::from_words([0x7532, 0xD800, 0x4E59]).expect("stored");
    assert_eq!(
        phrase_text_bytes(&surrogate).as_ref(),
        [0xE7, 0x94, 0xB2, 0xED, 0xA0, 0x80, 0xE4, 0xB9, 0x99]
    );

    let above = PhraseText::from_words([0x11_0000]).expect("stored");
    assert_eq!(phrase_text_bytes(&above).as_ref(), [0xF4, 0x90, 0x80, 0x80]);
}

/// Words past the four-byte range take GLib's five- and six-byte forms.
/// `from_words` refuses a word at or above 0x80000000, so the words are
/// stored the way the punctuation table stores them.
#[test]
fn words_above_four_bytes_take_glibs_five_and_six_byte_forms() {
    let cases: [(u32, &[u8]); 3] = [
        (0x20_0000, &[0xF8, 0x88, 0x80, 0x80, 0x80]),
        (0x400_0000, &[0xFC, 0x84, 0x80, 0x80, 0x80, 0x80]),
        (0xFFFF_FFFF, &[0xFF, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF]),
    ];
    for (word, bytes) in cases {
        let text = PhraseText::from_stored_parts(vec![word], 1);
        assert_eq!(phrase_text_bytes(&text).as_ref(), bytes, "{word:#x}");
    }
}

#[test]
fn length_matches_the_encoding_without_encoding() {
    use super::phrase_text_len;
    let texts = [
        PhraseText::from("甲乙"),
        PhraseText::from("甲\0乙"),
        PhraseText::from(""),
        PhraseText::from_words([0x41, 0x7FF, 0x7532, 0xD800, 0x1_F600]).expect("stored"),
        PhraseText::from_words([0x20_0000, 0x400_0000, 0x7FFF_FFFF]).expect("stored"),
        PhraseText::from_words([0x7532, 0, 0x4E59]).expect("stored"),
    ];
    for text in &texts {
        assert_eq!(
            phrase_text_len(text),
            phrase_text_bytes(text).len(),
            "{text:?}"
        );
    }
}

#[test]
fn stored_words_match_reads_every_stored_word() {
    assert!(stored_words_match(&PhraseText::from("甲乙"), "甲乙"));
    assert!(!stored_words_match(&PhraseText::from("甲\0乙"), "甲"));
    let with_zero = PhraseText::from_words([0x7532, 0, 0x4E59]).expect("stored");
    assert!(!stored_words_match(&with_zero, "甲"));
    let surrogate = PhraseText::from_words([0x7532, 0xD800]).expect("stored");
    assert!(!stored_words_match(&surrogate, "甲"));
    let ordinary = PhraseText::from_stored_parts(vec![0x7532, 0x4E59], 2);
    assert!(stored_words_match(&ordinary, "甲乙"));
}
