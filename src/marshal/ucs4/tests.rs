use super::{decode, prefix_text};

// Rust utf8_to_ucs4 accepts canonical historical UTF-8, including nonscalars and five-/six-byte forms.
#[test]
fn accepts_scalars_and_surrogates() {
    for (bytes, expected) in [
        (&b"nihao"[..], &[0x6e, 0x69, 0x68, 0x61, 0x6f][..]),
        ("你".as_bytes(), &[0x4f60][..]),
        (&[0xed, 0xa0, 0x80][..], &[0xd800][..]),
        (&[0xf4, 0x90, 0x80, 0x80][..], &[0x11_0000][..]),
        (&[0xf8, 0x88, 0x80, 0x80, 0x80][..], &[0x20_0000][..]),
        (&[0xfc, 0x84, 0x80, 0x80, 0x80, 0x80][..], &[0x400_0000][..]),
        (
            &[0xfd, 0xbf, 0xbf, 0xbf, 0xbf, 0xbf][..],
            &[0x7fff_ffff][..],
        ),
    ] {
        let input = std::ffi::CString::new(bytes).unwrap();
        let text = crate::marshal::utf8_to_ucs4(Some(&input)).unwrap();
        assert_eq!(
            text.outward_words().collect::<Vec<_>>(),
            expected,
            "{bytes:02x?}"
        );
    }
}

// Rust decode rejects invalid leads, truncation and overlong forms at each historical sequence width.
#[test]
fn rejects_malformed_and_overlong_forms() {
    for bytes in [
        &[0x80][..],
        &[0xc0, 0x80],
        &[0xe0, 0x80, 0x80],
        &[0xf0, 0x80, 0x80, 0x80],
        &[0xf8, 0x80, 0x80, 0x80, 0x80],
        &[0xfc, 0x80, 0x80, 0x80, 0x80, 0x80],
        &[0xfe],
        &[0xed, 0xa0],
    ] {
        assert!(decode(bytes).is_err(), "accepted {bytes:02x?}");
    }
}

// The NULL-prefix ruling and 074a2219 pinyin.cpp:1397–1398 admit only nonempty decoded prefixes.
#[test]
fn prefix_text_is_none_where_no_prefix_is_added() {
    for (input, expected) in [
        (None, None),
        (Some(c""), None),
        (Some(c"\xe7\x94\xb2\xff"), None),
        (Some(c"甲乙"), Some(&[0x7532, 0x4e59][..])),
        (
            Some(c"\xe7\x94\xb2\xed\xa0\x80"),
            Some(&[0x7532, 0xd800][..]),
        ),
    ] {
        let text = prefix_text(input);
        assert_eq!(
            text.map(|text| text.outward_words().collect::<Vec<_>>())
                .as_deref(),
            expected
        );
    }
}
