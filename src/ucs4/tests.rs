use core::cmp::Ordering;
use core::hash::BuildHasher;

use super::{PhraseText, PhraseTextConcat};

fn key(text: &PhraseText) -> u64 {
    std::hash::BuildHasherDefault::<std::hash::DefaultHasher>::default().hash_one(text)
}

fn stored(words: &[u32]) -> PhraseText {
    PhraseText::from_words(words.iter().copied()).expect("stored words")
}

// Rust PhraseText contract preserves full lengths and hashes the outward prefix; lookup.cpp:39–68 at 074a2219.
#[test]
fn constructors_keep_full_lengths_and_hash_the_outward_text() {
    use core::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    for (source, length, scalar, prefix) in [
        ("", 0, true, ""),
        ("甲乙", 2, true, "甲乙"),
        ("a\0甲", 3, false, "a"),
    ] {
        let text = PhraseText::from(source);
        assert_eq!(text.phrase_length(), length);
        assert_eq!(text.char_count(), scalar.then_some(length));
        assert_eq!(text.as_str(), scalar.then_some(source));
        assert_eq!(text.outward_bytes(), scalar.then_some(prefix.as_bytes()));
        if !scalar {
            assert_eq!(text.stored_words(), Some([0x61, 0, 0x7532].as_slice()));
        }
        let mut actual = DefaultHasher::new();
        let mut expected = DefaultHasher::new();
        text.hash(&mut actual);
        prefix.hash(&mut expected);
        assert_eq!(actual.finish(), expected.finish());
    }
    assert_eq!(
        PhraseText::from_words([0x7532]).unwrap().as_str(),
        Some("甲")
    );
    let mut sentence = PhraseTextConcat::new();
    sentence.push(&PhraseText::from("甲乙"));
    sentence.push(&PhraseText::from("abc"));
    let text = sentence.finish();
    assert_eq!(text.phrase_length(), 5);
    assert_eq!(text.outward_bytes(), Some("甲乙abc".as_bytes()));
}

// Rust from_words contract retains accepted nonscalar words instead of constructing Unicode scalars.
#[test]
fn non_scalars_stay_as_stored_words() {
    let words = [0x7532, 0xD800, 0x4E59];
    let text = PhraseText::from_words(words).expect("stored words");
    assert_eq!(text.stored_words(), Some(words.as_slice()));
    assert!(text.as_str().is_none());
}

// 074a2219 lookup/lookup.cpp:39–68 converts each phrase through its first zero; later words remain stored.
#[test]
fn a_non_scalar_word_does_not_end_the_range_check() {
    for (words, accepted) in [
        (&[0xD800, 0x8000_0000][..], false),
        (&[0xD800, 0, 0x8000_0000][..], true),
    ] {
        let text = PhraseText::from_words(words.iter().copied());
        assert_eq!(text.is_some(), accepted);
        if let Some(text) = text {
            assert_eq!(text.stored_words(), Some(words));
            assert_eq!(text.phrase_length(), 3);
            assert_eq!(text.outward_words().collect::<Vec<_>>(), [0xD800]);
        }
    }
}

// Rust skip_chars contract slices either representation and saturates the declared remaining length.
#[test]
fn skip_chars_slices_both_forms() {
    for source in ["甲乙", "a\0甲"] {
        let text = PhraseText::from(source);
        let length = source.chars().count();
        for skip in [length - 1, length, length + 1] {
            assert_eq!(
                text.skip_chars(skip).phrase_length(),
                length.saturating_sub(skip)
            );
        }
    }
    assert_eq!(PhraseText::from("甲乙").skip_chars(1).as_str(), Some("乙"));
    let suffix = stored(&[0x7532, 0xD800, 0x4E59]).skip_chars(1);
    assert_eq!(suffix.stored_words(), Some([0xD800, 0x4E59].as_slice()));
    assert_eq!(suffix.phrase_length(), 2);
}

// Rust equality, ordering and hashing use outward words across storage forms, ignoring headers and suffixes.
#[test]
fn mixed_forms_compare_and_hash_by_outward_words() {
    for (left, right, expected) in [
        (
            PhraseText::from("甲"),
            PhraseText::from("甲"),
            Ordering::Equal,
        ),
        (
            PhraseText::from("甲"),
            PhraseText::from("乙"),
            Ordering::Greater,
        ),
        (
            PhraseText::from("甲"),
            stored(&[0x7532, 0, 0x4E19]),
            Ordering::Equal,
        ),
        (
            stored(&[0x7532, 0, 0x4E19]),
            PhraseText::from("甲乙"),
            Ordering::Less,
        ),
        (
            stored(&[0x7532, 0xD800]),
            PhraseText::from("甲"),
            Ordering::Greater,
        ),
        (
            PhraseText::from("甲乙"),
            stored(&[0x7532, 0xD800]),
            Ordering::Less,
        ),
        (
            stored(&[0x7532, 0, 0x4E59]),
            stored(&[0x7532, 0, 0x4E19, 0x4E59]),
            Ordering::Equal,
        ),
        (
            stored(&[0x7532, 0xD800]),
            stored(&[0x11_0000]),
            Ordering::Less,
        ),
        (
            stored(&[0xD800, 0, 0x4E59]),
            stored(&[0xD800, 0]),
            Ordering::Equal,
        ),
        (
            PhraseText::from_stored_parts(vec![0x7532], 1),
            PhraseText::from_stored_parts(vec![0x7532], 5),
            Ordering::Equal,
        ),
    ] {
        assert_eq!(left.cmp(&right), expected, "{left:?}, {right:?}");
        assert_eq!(left.partial_cmp(&right), Some(expected));
        assert_eq!(left == right, expected.is_eq());
        if expected.is_eq() {
            assert_eq!(key(&left), key(&right));
        }
    }
}

// Rust PartialEq<str> compares the external string's prefix before its first NUL.
#[test]
fn str_comparison_matches_the_converted_text() {
    for (text, other, expected) in [
        (PhraseText::from("甲"), "甲\0丙", true),
        (PhraseText::from("甲"), "乙", false),
        (stored(&[0x7532, 0, 0x4E59]), "甲", true),
        (stored(&[0x7532, 0xD800]), "甲", false),
    ] {
        assert_eq!(text == *other, expected);
        assert_eq!(text == other, expected);
    }
}

// Rust OutwardWords is fused: neither the first zero nor later words can resume iteration.
#[test]
fn outward_words_stay_exhausted_after_the_zero() {
    let text = stored(&[0x7532, 0, 0x4E59]);
    let mut words = text.outward_words();
    assert_eq!(words.next(), Some(0x7532));
    assert_eq!(words.next(), None);
    assert_eq!(words.next(), None);
}

// 074a2219 lookup/lookup.cpp:39–68 converts each component before concatenating it.
#[test]
fn a_sentence_cuts_each_ordinary_part_at_its_own_nul() {
    let mut sentence = PhraseTextConcat::new();
    sentence.push(&PhraseText::from("甲\0乙"));
    sentence.push(&PhraseText::from("丙"));
    let sentence = sentence.finish();
    assert_eq!(
        sentence.outward_words().collect::<Vec<_>>(),
        [0x7532, 0x4E19]
    );
    assert_eq!(sentence.phrase_length(), 4);
    assert_eq!(sentence, "甲丙");
}
