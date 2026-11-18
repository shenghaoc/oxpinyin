use super::{PhraseText, PhraseTextConcat};

#[test]
fn constructors_keep_full_lengths_and_hash_the_outward_text() {
    use core::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    for source in ["", "abc", "甲乙", "a\0甲", "甲乙丙丁戊己庚辛壬癸"] {
        let text = PhraseText::from(source);
        assert_eq!(text.phrase_length(), source.chars().count());
        let scalar = !source.contains('\0');
        assert_eq!(text.char_count(), scalar.then(|| source.chars().count()));
        assert_eq!(text.as_str(), scalar.then_some(source));
        assert_eq!(
            matches!(text.0, super::TextStorage::Stored(_)),
            source.contains('\0')
        );
        let prefix = source.split('\0').next().unwrap();
        assert_eq!(text.outward_bytes(), scalar.then_some(prefix.as_bytes()));
        let mut actual = DefaultHasher::new();
        let mut expected = DefaultHasher::new();
        text.hash(&mut actual);
        prefix.hash(&mut expected);
        assert_eq!(actual.finish(), expected.finish());
        for skip in 0..=source.chars().count() + 1 {
            let tail = text.skip_chars(skip);
            assert_eq!(
                tail.phrase_length(),
                source.chars().count().saturating_sub(skip)
            );
        }
    }
    let mut sentence = PhraseTextConcat::new();
    sentence.push(&PhraseText::from("甲乙"));
    sentence.push(&PhraseText::from("abc"));
    let text = sentence.finish();
    assert_eq!(text.phrase_length(), 5);
    assert_eq!(text.outward_bytes(), Some("甲乙abc".as_bytes()));
}

#[test]
fn inline_and_heap_boundaries_preserve_scalar_and_stored_views() {
    for unit in ["a", "é", "甲", "🦀"] {
        for count in [
            0, 1, 7, 8, 20, 21, 22, 23, 24, 25, 63, 64, 65, 127, 128, 129,
        ] {
            let source = unit.repeat(count);
            let text = PhraseText::from(source.as_str());
            let decoded = PhraseText::from_words(source.chars().map(u32::from)).unwrap();
            assert_eq!(text.as_str(), Some(source.as_str()));
            assert_eq!(decoded.as_str(), Some(source.as_str()));
            assert_eq!(text.phrase_length(), count);
            assert_eq!(decoded.phrase_length(), count);
            assert_eq!(text, decoded);
            assert_eq!(key(&text), key(&decoded));
            let words = source.chars().map(u32::from).chain([0, 0xd800]);
            let stored = PhraseText::from_words(words).unwrap();
            assert_eq!(stored, text);
            assert_eq!(stored.phrase_length(), count + 2);
            assert_eq!(key(&stored), key(&text));
        }
    }
}
use core::cmp::Ordering;
use core::hash::BuildHasher;

fn key(text: &PhraseText) -> u64 {
    std::hash::BuildHasherDefault::<std::hash::DefaultHasher>::default().hash_one(text)
}

fn stored(words: &[u32]) -> PhraseText {
    PhraseText::from_words(words.iter().copied()).expect("stored words")
}

/// Both forms of the same outward texts (甲 U+7532, 乙 U+4E59, 丙
/// U+4E19), with differing headers and suffixes, beside non-scalar and
/// empty ones.
fn mixed_texts() -> Vec<PhraseText> {
    vec![
        PhraseText::from(""),
        PhraseText::from("\0甲"),
        stored(&[0, 0x7532]),
        PhraseText::from("甲"),
        PhraseText::from("甲\0乙"),
        stored(&[0x7532, 0, 0x4E19]),
        PhraseText::from_stored_parts(vec![0x7532], 9),
        PhraseText::from("甲乙"),
        PhraseText::from_stored_parts(vec![0x7532, 0x4E59], 2),
        stored(&[0x7532, 0xD800]),
        stored(&[0x7532, 0xD800, 0, 0x4E59]),
        stored(&[0xD800]),
        PhraseText::from("乙"),
        PhraseText::from("\u{ffff}"),
        PhraseText::from("\u{10000}"),
        stored(&[0x11_0000]),
    ]
}

#[test]
fn non_scalars_stay_as_stored_words() {
    let surrogate = PhraseText::from_words([0x7532, 0xD800, 0x4E59]).expect("stored words");
    assert_eq!(
        surrogate.stored_words(),
        Some([0x7532, 0xD800, 0x4E59].as_slice())
    );
    assert!(surrogate.as_str().is_none());

    let above = PhraseText::from_words([0x11_0000]).expect("stored words");
    assert_eq!(above.stored_words(), Some([0x11_0000].as_slice()));
}

#[test]
fn general_conversions_preserve_nul_text_as_stored_words() {
    for text in [
        PhraseText::from("甲\0乙"),
        PhraseText::from(String::from("甲\0乙")),
        PhraseText::from(compact_str::CompactString::from("甲\0乙")),
    ] {
        assert_eq!(text.as_str(), None);
        assert_eq!(text.stored_words(), Some([0x7532, 0, 0x4e59].as_slice()));
        assert_eq!(text.phrase_length(), 3);
        assert_eq!(text.outward_words().collect::<Vec<_>>(), [0x7532]);
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_text_is_one_candidate_row_field() {
    assert_eq!(core::mem::size_of::<PhraseText>(), 24);
}

#[test]
fn out_of_range_words_answer_none_before_the_zero() {
    assert!(PhraseText::from_words([0x8000_0000]).is_none());
    let after = PhraseText::from_words([0x7532, 0, 0x8000_0000]).expect("zero ends the read");
    assert_eq!(
        after.stored_words(),
        Some([0x7532, 0, 0x8000_0000].as_slice())
    );
    assert_eq!(after.phrase_length(), 3);
}

#[test]
fn a_non_scalar_word_does_not_end_the_range_check() {
    assert!(PhraseText::from_words([0xD800, 0x8000_0000]).is_none());

    let kept = PhraseText::from_words([0xD800, 0, 0x8000_0000]).expect("zero ends the read");
    assert_eq!(
        kept.stored_words(),
        Some([0xD800, 0, 0x8000_0000].as_slice())
    );
    assert_eq!(kept.phrase_length(), 3);
    assert_eq!(kept.outward_words().collect::<Vec<_>>(), [0xD800]);
}

#[test]
fn skip_chars_slices_both_forms() {
    let scalar = PhraseText::from("甲乙");
    assert_eq!(scalar.skip_chars(1).as_str(), Some("乙"));

    let stored = PhraseText::from_words([0x7532, 0xD800, 0x4E59]).expect("stored words");
    let suffix = stored.skip_chars(1);
    assert_eq!(suffix.stored_words(), Some([0xD800, 0x4E59].as_slice()));
    assert_eq!(suffix.phrase_length(), 2);
}

#[test]
fn ordinary_text_compares_before_its_first_nul() {
    let text = PhraseText::from("甲\0乙");
    assert_eq!(text.phrase_length(), 3);
    assert_eq!(text, PhraseText::from("甲"));
    assert_eq!(text, PhraseText::from("甲\0丙"));
    assert_ne!(text, PhraseText::from("甲乙"));
    assert_eq!(text.cmp(&PhraseText::from("甲乙")), Ordering::Less);
    assert_eq!(key(&text), key(&PhraseText::from("甲")));
    assert!(PhraseText::from("\0甲").is_empty());
}

#[test]
fn mixed_forms_compare_and_hash_by_outward_words() {
    let texts = mixed_texts();
    for left in &texts {
        for right in &texts {
            let words = left.outward_words().cmp(right.outward_words());
            assert_eq!(left.cmp(right), words, "{left:?} against {right:?}");
            assert_eq!(left.partial_cmp(right), Some(words));
            assert_eq!(left == right, words.is_eq(), "{left:?} against {right:?}");
            if left == right {
                assert_eq!(key(left), key(right), "{left:?} and {right:?} hash apart");
            }
        }
    }
}

#[test]
fn the_length_header_and_the_suffix_are_not_compared() {
    let pairs = [
        (
            PhraseText::from_stored_parts(vec![0x7532], 1),
            PhraseText::from_stored_parts(vec![0x7532], 5),
        ),
        (
            stored(&[0x7532, 0, 0x4E59]),
            stored(&[0x7532, 0, 0x4E19, 0x4E59]),
        ),
        (
            PhraseText::from_stored_parts(vec![0xD800], 1),
            PhraseText::from_stored_parts(vec![0xD800], 7),
        ),
        (stored(&[0xD800, 0, 0x4E59]), stored(&[0xD800, 0])),
    ];
    for (left, right) in &pairs {
        assert!(
            left.phrase_length() != right.phrase_length()
                || left.stored_words() != right.stored_words(),
            "{left:?} and {right:?} store the same words"
        );
        assert_eq!(left, right);
        assert_eq!(left.cmp(right), Ordering::Equal);
        assert_eq!(key(left), key(right));
    }
}

#[test]
fn str_comparison_matches_the_converted_text() {
    let strs = ["", "甲", "甲\0丙", "甲乙", "乙", "\0"];
    for text in mixed_texts() {
        for other in strs {
            let converted = PhraseText::from(other);
            assert_eq!(
                text == *other,
                text == converted,
                "{text:?} against {other:?}"
            );
            assert_eq!(
                text == other,
                text == converted,
                "{text:?} against {other:?}"
            );
        }
    }
    assert_eq!(stored(&[0x7532, 0, 0x4E59]), "甲");
    assert_ne!(stored(&[0x7532, 0xD800]), "甲");
}

#[test]
fn outward_words_stay_exhausted_after_the_zero() {
    for text in [stored(&[0x7532, 0, 0x4E59]), PhraseText::from("甲\0乙")] {
        let mut words = text.outward_words();
        assert_eq!(words.next(), Some(0x7532));
        assert_eq!(words.next(), None);
        assert_eq!(words.next(), None, "{text:?} resumed after its zero");
    }
}

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
