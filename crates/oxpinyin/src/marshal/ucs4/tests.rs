use super::{decode, prefix_text};

#[test]
fn accepts_scalars_and_surrogates() {
    assert_eq!(decode(b"nihao").unwrap(), [0x6e, 0x69, 0x68, 0x61, 0x6f]);
    assert_eq!(decode("你".as_bytes()).unwrap(), [0x4f60]);
    assert_eq!(decode(&[0xed, 0xa0, 0x80]).unwrap(), [0xd800]);
    assert_eq!(decode(&[0xed, 0xbf, 0xbf]).unwrap(), [0xdfff]);
}

#[test]
fn accepts_above_unicode_and_historical_widths() {
    assert_eq!(decode(&[0xf4, 0x90, 0x80, 0x80]).unwrap(), [0x11_0000]);
    assert_eq!(decode(&[0xf5, 0x80, 0x80, 0x80]).unwrap(), [0x14_0000]);
    assert_eq!(
        decode(&[0xf8, 0x88, 0x80, 0x80, 0x80]).unwrap(),
        [0x20_0000]
    );
    assert_eq!(
        decode(&[0xfc, 0x84, 0x80, 0x80, 0x80, 0x80]).unwrap(),
        [0x400_0000]
    );
    assert_eq!(
        decode(&[0xfd, 0xbf, 0xbf, 0xbf, 0xbf, 0xbf]).unwrap(),
        [0x7fff_ffff]
    );
}

#[test]
fn rejects_malformed_and_overlong_forms() {
    for bytes in [
        &[0x80][..],
        &[0xc0, 0x80],
        &[0xc1, 0xbf],
        &[0xe0, 0x80, 0x80],
        &[0xf0, 0x80, 0x80, 0x80],
        &[0xf8, 0x80, 0x80, 0x80, 0x80],
        &[0xfc, 0x80, 0x80, 0x80, 0x80, 0x80],
        &[0xfe],
        &[0xff],
        &[0xed, 0xa0],
        &[0xe4, 0xbd],
    ] {
        assert!(decode(bytes).is_err(), "accepted {bytes:02x?}");
    }
}

#[test]
fn prefix_text_is_none_where_no_prefix_is_added() {
    assert!(prefix_text(None).is_none());
    assert!(prefix_text(Some(c"")).is_none());
    assert!(prefix_text(Some(c"\xe7\x94\xb2\xff")).is_none());
    assert_eq!(prefix_text(Some(c"甲乙")).unwrap(), "甲乙");
    let stored = prefix_text(Some(c"\xe7\x94\xb2\xed\xa0\x80")).unwrap();
    assert_eq!(stored.stored_words(), Some(&[0x7532, 0xd800][..]));
}
