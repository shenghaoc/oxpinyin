use super::{phrase_text_bytes, phrase_text_len};
use crate::vocab::PhraseText;

// Rust encoding borrows scalar bytes and owns stored encoding, cutting at the first zero as lookup.cpp:39–68 does.
#[test]
fn scalar_text_is_borrowed_and_nul_text_is_encoded() {
    for (text, borrowed, expected) in [
        (PhraseText::from("甲乙"), true, "甲乙"),
        (PhraseText::from("甲\0乙"), false, "甲"),
    ] {
        let bytes = phrase_text_bytes(&text);
        assert_eq!(matches!(bytes, std::borrow::Cow::Borrowed(_)), borrowed);
        assert_eq!(bytes.as_ref(), expected.as_bytes());
    }
}

// Rust phrase_text_bytes preserves the historical UCS-4 encoding widths used at the pin's GLib boundary.
#[test]
fn stored_non_scalars_take_glibs_historical_encoding() {
    for (text, expected) in [
        (
            PhraseText::from_words([0x7532, 0xD800, 0x4E59]).unwrap(),
            &[0xE7, 0x94, 0xB2, 0xED, 0xA0, 0x80, 0xE4, 0xB9, 0x99][..],
        ),
        (
            PhraseText::from_words([0x11_0000]).unwrap(),
            &[0xF4, 0x90, 0x80, 0x80][..],
        ),
        (
            PhraseText::from_stored_parts(vec![0x20_0000], 1),
            &[0xF8, 0x88, 0x80, 0x80, 0x80][..],
        ),
        (
            PhraseText::from_stored_parts(vec![0x400_0000], 1),
            &[0xFC, 0x84, 0x80, 0x80, 0x80, 0x80][..],
        ),
        (
            PhraseText::from_stored_parts(vec![0xFFFF_FFFF], 1),
            &[0xFF, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF][..],
        ),
    ] {
        assert_eq!(phrase_text_bytes(&text).as_ref(), expected);
    }
}

// Rust phrase_text_len promises the byte length of phrase_text_bytes without performing the encoding.
#[test]
fn length_matches_the_encoding_without_encoding() {
    for text in [
        PhraseText::from("甲乙"),
        PhraseText::from("甲\0乙"),
        PhraseText::from(""),
        PhraseText::from_words([0x41, 0x7FF, 0x7532, 0xD800, 0x1_F600]).unwrap(),
        PhraseText::from_words([0x20_0000, 0x400_0000, 0x7FFF_FFFF]).unwrap(),
    ] {
        assert_eq!(
            phrase_text_len(&text),
            phrase_text_bytes(&text).len(),
            "{text:?}"
        );
    }
}
